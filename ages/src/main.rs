//! `ages` — the headless run binary.
//!
//! Parses CLI args, opens the NDJSON event log, optionally tees to
//! stdout for the live CLI stream, runs the sim. The planet is
//! sampled from `--seed` at run start by sim-world; same seed
//! reproduces the same run bit-for-bit.

use anyhow::{Context, Result};
use sim_core::{run, run_interruptible, RunConfig};
use sim_events::{
    is_highlight_event, ChannelEmitter, FilterEmitter, JsonLinesEmitter, PaceControl, TeeEmitter,
    ThrottledEmitter,
};
use sim_report::{
    replay_narration, run_interactive_tui, NarratingEmitter, TempUnit, TuiOptions, ViewportConfig,
    ViewportEmitter,
};
use std::fs::File;
use std::io::{BufRead, BufWriter, IsTerminal};
use std::path::PathBuf;
use std::sync::mpsc::sync_channel;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

mod config_prompt;

fn main() -> Result<()> {
    let args = parse_args()?;

    // `--replay-narration` short-circuits the whole sim path: we
    // don't open a fresh event log, we don't spin up RunConfig, we
    // just read the supplied NDJSON log and narrate it line-by-line
    // to stdout. The post-run report continues to live behind
    // `ages-report`; this binary only owns the runtime narration
    // surface.
    if let Some(path) = args.replay_narration.as_deref() {
        let stdout = std::io::stdout();
        let written = replay_narration(path, stdout.lock())
            .with_context(|| format!("replay narration failed for {}", path.display()))?;
        eprintln!("ages: narrated {written} events from {}", path.display());
        return Ok(());
    }

    let file = File::create(&args.out)
        .with_context(|| format!("could not create event log at {}", args.out))?;
    let file_emitter = JsonLinesEmitter::new(BufWriter::new(file));

    // Optional grid override. Smaller grids run faster; larger
    // grids resolve climate bands and civ territories more finely.
    let mut cfg = RunConfig::dev(args.seed, args.ticks);
    if let Some(w) = args.grid_width {
        cfg.grid_width = w;
    }
    if let Some(h) = args.grid_height {
        cfg.grid_height = h;
    }
    // `--config` runs the interactive planet-builder before the sim.
    // Map geography stays seed-driven; planet-level scalars
    // (substrate, atmosphere, temperature, gravity, …) are
    // overridden from the user's picks via `PlanetOverrides`.
    if args.config {
        let stdin = std::io::stdin();
        let mut handle = stdin.lock();
        let mut buf = std::io::BufReader::new(&mut handle as &mut dyn BufRead);
        cfg.planet_overrides =
            config_prompt::run_interactive(&mut buf, std::io::stdout().lock())?;
    }
    let throttle = Duration::from_millis(args.tick_rate_ms);

    // `--narration` short-circuits the verbosity matrix: it owns
    // stdout for prose lines and cohabiting with `--cli=all` /
    // highlights / viewport would interleave NDJSON or ANSI frames
    // mid-sentence. The file emitter still receives the full event
    // stream. Throttling the narration sink would slow the inner
    // file write too (the inner emitter sits *inside* the narrator),
    // so the sink is unthrottled here — narration paces itself
    // naturally on long runs because most events fire once per
    // structural transition rather than per tick.
    if args.narration {
        let stdout = std::io::stdout();
        let mut emitter = NarratingEmitter::new(file_emitter, stdout.lock());
        run(&cfg, &mut emitter)?;
        return Ok(());
    }

    match args.cli {
        CliVerbosity::Quiet => {
            // No streaming, no point throttling — file always
            // writes at full speed for batch consumers.
            let mut emitter = file_emitter;
            run(&cfg, &mut emitter)?;
        }
        CliVerbosity::All => {
            // Stdout pipeline: throttle → write. Throttling outside
            // the writer means the sleep fires on the canonical
            // TickEnd event regardless of how many other events
            // fanned out this tick.
            let stdout_pipeline = ThrottledEmitter {
                inner: JsonLinesEmitter::new(std::io::stdout().lock()),
                tick_rate: throttle,
            };
            let mut emitter = TeeEmitter {
                a: file_emitter,
                b: stdout_pipeline,
            };
            run(&cfg, &mut emitter)?;
        }
        CliVerbosity::Highlights => {
            // File emitter sees the full event log; stdout
            // sees only the structural-pin subset. Long runs become
            // tail-able without flooding the terminal.
            //
            // Layering: throttle wraps filter. The throttle must
            // see ALL events (including Tick events) so its
            // TickEnd-trigger fires once per tick; the filter
            // inside it then drops everything except the highlight pin
            // set before passing to the stdout writer.
            let stdout_pipeline = ThrottledEmitter {
                inner: FilterEmitter {
                    inner: JsonLinesEmitter::new(std::io::stdout().lock()),
                    predicate: is_highlight_event,
                },
                tick_rate: throttle,
            };
            let mut emitter = TeeEmitter {
                a: file_emitter,
                b: stdout_pipeline,
            };
            run(&cfg, &mut emitter)?;
        }
        CliVerbosity::Viewport | CliVerbosity::ViewportDensity => {
            // Live viewport. On a real terminal this is the
            // interactive ratatui dashboard (`tui`): the sim runs on a
            // background thread feeding events over a channel while the
            // UI thread renders + handles keyboard controls. The
            // NDJSON file emitter stays in the loop so batch consumers
            // and the post-run report still see the full event log.
            //
            // When stdout is *not* a terminal (piped to a file / CI),
            // raw mode + the alternate screen would corrupt the sink,
            // so fall back to a plain, no-colour frame dump driven by
            // the legacy renderer with alt-screen disabled.
            let density_mode = matches!(args.cli, CliVerbosity::ViewportDensity);
            if std::io::stdout().is_terminal() {
                run_tui(cfg, file_emitter, &args, density_mode, throttle)?;
            } else {
                let viewport = ViewportEmitter::new(
                    std::io::stdout().lock(),
                    ViewportConfig {
                        frame_every: args.frame_every_ticks,
                        use_alt_screen: false,
                        use_color: false,
                        show_planet_card: args.viewport_planet_card,
                        log_lines: args.viewport_log_lines,
                        compact: args.viewport_compact,
                        temperature_unit: args.temperature_unit,
                        density_mode,
                    },
                );
                let stdout_pipeline = ThrottledEmitter {
                    inner: viewport,
                    tick_rate: throttle,
                };
                let mut emitter = TeeEmitter {
                    a: file_emitter,
                    b: stdout_pipeline,
                };
                run(&cfg, &mut emitter)?;
            }
        }
    }

    Ok(())
}

/// Drive the interactive TUI. The sim runs on a background thread
/// (NDJSON file + channel tee); this thread owns the terminal via
/// `run_interactive_tui` and returns when the user quits or
/// acknowledges run completion. The shared `PaceControl` lets the UI
/// pause / step / change speed; on exit we signal it and close the
/// channel so the sim thread unwinds, then join it.
fn run_tui(
    cfg: RunConfig,
    file_emitter: JsonLinesEmitter<BufWriter<File>>,
    args: &Args,
    density_mode: bool,
    initial_delay: Duration,
) -> Result<()> {
    let opts = TuiOptions {
        use_color: !args.no_color,
        density_mode,
        temperature_unit: args.temperature_unit,
        initial_delay,
        start_paused: false,
    };
    let pace = Arc::new(PaceControl::new(opts.initial_delay, opts.start_paused));
    // Bounded channel: if the UI falls behind, the sim blocks on send
    // (backpressure) rather than buffering an unbounded backlog.
    let (tx, rx) = sync_channel::<protocol::Event>(8192);
    let sim_pace = Arc::clone(&pace);
    let stop_pace = Arc::clone(&pace);
    let sim = thread::spawn(move || {
        let mut emitter = TeeEmitter {
            a: file_emitter,
            b: ChannelEmitter::new(tx, sim_pace),
        };
        // Stop at the next tick boundary once the UI sets the quit
        // flag. `run_interruptible` still emits a final `RunEnd` and
        // the BufWriter flushes on drop, so an early quit leaves a
        // well-formed NDJSON log for the post-run report.
        let _ = run_interruptible(&cfg, &mut emitter, move || !stop_pace.is_quit());
    });

    let tui_res = run_interactive_tui(&rx, &pace, &opts);

    // Signal the stop, then keep draining so the sim's final tick +
    // `RunEnd` aren't blocked on a full channel; the loop ends when
    // the sim thread drops its sender. Then join.
    pace.quit();
    while rx.recv().is_ok() {}
    let _ = sim.join();
    tui_res.context("interactive TUI failed")
}

// Adding `narration` pushed `Args` past the pedantic 3-bool ceiling
// for option-bag structs. The bag is internal to `main.rs` so the
// clippy guidance ("consider a state machine / enum") doesn't apply
// — every flag is independently toggled by the user from the CLI.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug)]
struct Args {
    seed: u64,
    ticks: u64,
    out: String,
    cli: CliVerbosity,
    /// Per-tick throttle applied to stdout streaming. `0` =
    /// no throttling (the default; sim runs at full speed).
    /// Larger values pace the live stream so a human watching
    /// stdout (or a future UI consumer) can follow tick-by-tick.
    tick_rate_ms: u64,
    /// Re-render the live ASCII viewport every N ticks (years).
    /// Only meaningful with `--cli=viewport`. Default: 50.
    frame_every_ticks: u64,
    /// Disable per-civ ANSI 256-colour shading in the viewport.
    /// Useful when piping to a non-terminal sink that doesn't
    /// render escape codes (CI logs, plain files).
    no_color: bool,
    /// Show the 2-line compact planet card above the
    /// frame. Default `true`. Disable with `--no-planet-card`
    /// on cramped terminals.
    viewport_planet_card: bool,
    /// Number of rows reserved below the frame for the
    /// scrolling event log. Default `5`. Set to `0` to disable
    /// the log section.
    viewport_log_lines: usize,
    /// Viewport mode: render each cell as a single
    /// character instead of `symbol + space`. Halves the grid
    /// width on screen (default 12-cell grid: 28 cols → 16 cols).
    /// Drops the hex-row offset so the visual is a square grid;
    /// useful on narrow phone terminals.
    viewport_compact: bool,
    /// Optional grid-width override (sim-wide).
    /// Smaller grids cost less per tick but resolve climate bands
    /// / territories coarsely. Defaults to 36 (was previously 32,
    /// originally 12). `RunConfig::dev`'s built-in 12 is overridden in
    /// `main` whenever the CLI default fires.
    grid_width: Option<u32>,
    /// Optional grid-height override (sim-wide).
    /// Defaults to 30 (was previously 20).
    grid_height: Option<u32>,
    /// Viewport temperature unit. Default `Fahrenheit`.
    /// CLI flag `--temperature-unit f|c|k`.
    temperature_unit: TempUnit,
    /// `--narration`: stream human-readable narration to stdout
    /// as the sim runs. Wraps the file emitter so the NDJSON log
    /// still receives the canonical stream. Mutually exclusive with
    /// `--cli=all|highlights|viewport*` (we silently force them
    /// into a side-channel since narration owns stdout).
    narration: bool,
    /// `--replay-narration <path>`: skip the sim run entirely and
    /// narrate a previously-recorded NDJSON event log to stdout.
    /// Mutually exclusive with `--narration` (a freshly running sim
    /// has nothing to replay).
    replay_narration: Option<PathBuf>,
    /// `--config`: run the interactive planet-builder before the
    /// sim starts. The seed still controls map geography; every
    /// other planet-level scalar can be set by hand.
    config: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum CliVerbosity {
    /// Nothing to stdout.
    Quiet,
    /// All events tee'd to stdout. Floods the terminal on long
    /// runs; pair with `--cli=highlights` for tail-able output.
    All,
    /// Structural-pin subset only — civ founding/collapse,
    /// catastrophes, tech unlocks, contacts, transmissions,
    /// conflicts, run-end. The canonical NDJSON file still carries
    /// the full stream.
    Highlights,
    /// Interactive ratatui dashboard (the default live experience on
    /// a real terminal): tabbed World / Civilizations / Planet views
    /// with a live colour map, selectable per-civ panels, scrolling
    /// event log, and keyboard controls (pause / step / speed / civ
    /// select / view switch). The sim runs on a background thread; the
    /// NDJSON file still carries the full event log. When stdout is
    /// not a terminal (piped / CI), falls back to a plain frame dump.
    Viewport,
    /// Same as `Viewport` but renders cells as density block
    /// glyphs (` ░ ▒ ▓ █`) sized by pop fill-% instead of the
    /// digit-ladder. Useful for scanning where mass sits rather
    /// than exact fill-%.
    ViewportDensity,
}

#[allow(clippy::too_many_lines)]
fn parse_args() -> Result<Args> {
    let mut seed: Option<u64> = None;
    let mut ticks: Option<u64> = None;
    // --years is resolved post-parse against the seed-sampled
    // planet's orbital_period_months so a 16-month planet runs 16
    // ticks per --years unit (not the baseline 12).
    let mut years_arg: Option<u64> = None;
    let mut out: String = "events.ndjson".to_string();
    let mut cli = CliVerbosity::All;
    let mut tick_rate_ms: u64 = 0;
    let mut frame_every_ticks: u64 = 50;
    let mut no_color: bool = false;
    let mut viewport_planet_card: bool = true;
    let mut viewport_log_lines: usize = 5;
    // Compact viewport + 36×30 grid is the
    // default. The default started at 24×16 to fit phone widths;
    // bumped to 32×20 once the viewport became the visual
    // focus; bumped again to 36×30 (1080 cells, ~1.7× the
    // earlier cell budget) — phone terminals have plenty of vertical
    // headroom in portrait, and 4 extra cells of width keep the
    // total viewport at 74 cols (still fits Termius portrait).
    // Override with `--no-viewport-compact` and `--grid-width` /
    // `--grid-height`.
    let mut viewport_compact: bool = true;
    let mut grid_width: Option<u32> = Some(36);
    let mut grid_height: Option<u32> = Some(30);
    let mut temperature_unit: TempUnit = TempUnit::Fahrenheit;
    let mut narration: bool = false;
    let mut replay_narration_path: Option<PathBuf> = None;
    let mut config: bool = false;

    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--seed" => {
                let v = iter.next().context("--seed needs a value")?;
                seed = Some(v.parse().context("--seed must be an integer")?);
            }
            "--ticks" => {
                let v = iter.next().context("--ticks needs a value")?;
                ticks = Some(v.parse().context("--ticks must be an integer")?);
            }
            "--years" => {
                let v = iter.next().context("--years needs a value")?;
                let years: u64 = v.parse().context("--years must be a positive integer")?;
                years_arg = Some(years);
            }
            "--out" => {
                out = iter.next().context("--out needs a path")?;
            }
            "--cli" => {
                let v = iter.next().context("--cli needs a value")?;
                cli = match v.as_str() {
                    "quiet" => CliVerbosity::Quiet,
                    "all" => CliVerbosity::All,
                    "highlights" => CliVerbosity::Highlights,
                    "viewport" => CliVerbosity::Viewport,
                    "viewport-density" => CliVerbosity::ViewportDensity,
                    other => anyhow::bail!(
                        "--cli must be one of {{quiet, all, highlights, viewport, viewport-density}}; got {other:?}"
                    ),
                };
            }
            "--tick-rate-ms" => {
                let v = iter.next().context("--tick-rate-ms needs a value")?;
                tick_rate_ms = v
                    .parse()
                    .context("--tick-rate-ms must be a non-negative integer")?;
            }
            "--frame-every-ticks" => {
                let v = iter.next().context("--frame-every-ticks needs a value")?;
                frame_every_ticks = v
                    .parse()
                    .context("--frame-every-ticks must be a positive integer")?;
                if frame_every_ticks == 0 {
                    anyhow::bail!("--frame-every-ticks must be > 0");
                }
            }
            "--no-color" => {
                no_color = true;
            }
            "--no-planet-card" => {
                viewport_planet_card = false;
            }
            "--viewport-log-lines" => {
                let v = iter.next().context("--viewport-log-lines needs a value")?;
                viewport_log_lines = v
                    .parse()
                    .context("--viewport-log-lines must be a non-negative integer")?;
            }
            // `--viewport-compact` (older explicit-on)
            // removed — compact is the default and the
            // flag was a no-op. `--no-viewport-compact` remains as
            // the opt-out into the standard 2-char-per-cell layout.
            "--no-viewport-compact" => {
                viewport_compact = false;
            }
            "--grid-width" => {
                let v = iter.next().context("--grid-width needs a value")?;
                let n: u32 = v
                    .parse()
                    .context("--grid-width must be a positive integer")?;
                if n == 0 {
                    anyhow::bail!("--grid-width must be > 0");
                }
                grid_width = Some(n);
            }
            "--grid-height" => {
                let v = iter.next().context("--grid-height needs a value")?;
                let n: u32 = v
                    .parse()
                    .context("--grid-height must be a positive integer")?;
                if n == 0 {
                    anyhow::bail!("--grid-height must be > 0");
                }
                grid_height = Some(n);
            }
            "--temperature-unit" => {
                let v = iter.next().context("--temperature-unit needs a value")?;
                temperature_unit = match v.as_str() {
                    "f" | "F" | "fahrenheit" => TempUnit::Fahrenheit,
                    "c" | "C" | "celsius" => TempUnit::Celsius,
                    "k" | "K" | "kelvin" => TempUnit::Kelvin,
                    other => anyhow::bail!("--temperature-unit must be f|c|k, got {other:?}"),
                };
            }
            "--narration" => {
                narration = true;
            }
            "--replay-narration" => {
                let v = iter.next().context("--replay-narration needs a path")?;
                replay_narration_path = Some(PathBuf::from(v));
            }
            "--config" => {
                config = true;
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            other => anyhow::bail!("unknown argument: {other}"),
        }
    }

    if narration && replay_narration_path.is_some() {
        anyhow::bail!("--narration and --replay-narration are mutually exclusive");
    }

    // `--replay-narration` skips the sim entirely; seed / ticks
    // aren't required for that path. Return early with placeholder
    // values for the never-read seed/ticks slots so the rest of
    // `main` only branches on `replay_narration.is_some()`.
    if replay_narration_path.is_some() {
        return Ok(Args {
            seed: 0,
            ticks: 0,
            out,
            cli,
            tick_rate_ms,
            frame_every_ticks,
            no_color,
            viewport_planet_card,
            viewport_log_lines,
            viewport_compact,
            grid_width,
            grid_height,
            temperature_unit,
            narration,
            replay_narration: replay_narration_path,
            config,
        });
    }

    let resolved_seed = seed.context("--seed required")?;
    // Resolve --years against the seed-sampled planet's actual
    // year length (orbital_period_months). `--ticks` (raw) overrides
    // when both are present; --years is the human-friendly default.
    if let Some(years) = years_arg {
        let planet = sim_world::sample_planet(resolved_seed);
        let period = u64::from(planet.orbital_period_months.max(1));
        ticks = Some(years.saturating_mul(period));
    }
    Ok(Args {
        seed: resolved_seed,
        ticks: ticks.context("--ticks or --years required")?,
        out,
        cli,
        tick_rate_ms,
        frame_every_ticks,
        no_color,
        viewport_planet_card,
        viewport_log_lines,
        viewport_compact,
        grid_width,
        grid_height,
        temperature_unit,
        narration,
        replay_narration: replay_narration_path,
        config,
    })
}

fn print_help() {
    println!(
        "ages — civilization-history simulator\n\
         \n\
         USAGE:\n  \
             ages --seed <u64> --years <u64> [--out <path>] [--cli <mode>] [--tick-rate-ms <u64>] [--frame-every-ticks <u64>]\n\
         \n\
         OPTIONS:\n  \
             --seed <u64>           seed for the deterministic RNG; also samples\n  \
                                    the planet (gravity, composition, atmosphere,\n  \
                                    biosphere, terrain, climate)\n  \
             --years <u64>          number of planet-years to run (preferred). Each\n  \
                                    year contains <orbital_period_months> ticks — sampled\n  \
                                    per planet from [8, 16]. On a 16-month planet\n  \
                                    --years 100 = 1600 ticks; on a 9-month planet 900.\n  \
             --ticks <u64>          raw tick count (low-level alternative to --years).\n  \
                                    1 tick = 1 month\n  \
             --out <path>           NDJSON event-log path (default: events.ndjson)\n  \
             --cli <mode>           live CLI verbosity: quiet|all|highlights|viewport|viewport-density (default: all)\n  \
                                    highlights = structural-pin subset (founding, collapse,\n  \
                                    catastrophe, tech, contact, transmission, conflict, run-end)\n  \
                                    viewport   = interactive TUI dashboard (tabbed World /\n  \
                                    Civilizations / Planet, live colour map, civ panels,\n  \
                                    event log) with keyboard controls: q quit · space pause ·\n  \
                                    s step · ←/→ speed · ↑/↓ select civ · [ ] scroll civ list ·\n  \
                                    Tab switch view · d density · PgUp/PgDn scroll log (or the\n  \
                                    civ-detail pane on the Civilizations tab). Piped output\n  \
                                    falls back to a plain frame dump.\n  \
                                    viewport-density = same dashboard, map starts in density\n  \
                                    mode (block-shaded fill-% instead of pop-fill digits)\n  \
             --tick-rate-ms <u64>   wall-clock sleep per tick on stdout streaming\n  \
                                    (default: 0 = no throttling). Useful for human\n  \
                                    readability or pacing a UI consumer. NDJSON file\n  \
                                    always writes at full speed regardless.\n  \
             --frame-every-ticks <u64>  viewport mode: re-render every N ticks (years).\n  \
                                    Default 50. Smaller = smoother, larger = lower bandwidth.\n  \
             --no-color             viewport mode: disable per-civ ANSI 256-color\n  \
                                    shading. Useful for non-terminal sinks.\n  \
             --no-planet-card       viewport mode: hide the 2-line planet card above\n  \
                                    the frame. Defaults to shown.\n  \
             --viewport-log-lines <usize>  viewport mode: rows reserved below the frame\n  \
                                    for a scrolling significant-event log (founded,\n  \
                                    collapsed, catastrophe, transmissions, tech, conflict).\n  \
                                    Default 5. Set to 0 to hide the log.\n  \
             --no-viewport-compact  viewport mode: opt out of the default compact\n  \
                                    1-char-per-cell layout and back to the standard\n  \
                                    2-char-per-cell with hex-row offset. Compact\n  \
                                    mode (the default) fits the 36×30 grid in\n  \
                                    ~40 cols — readable on portrait phone\n  \
                                    terminals + Termius.\n  \
             --grid-width <u32>     override sim grid width (default 36). Smaller\n  \
                                    grids run faster; larger resolve climate bands\n  \
                                    and territories more finely.\n  \
             --grid-height <u32>    override sim grid height (default 30).\n  \
             --temperature-unit f|c|k  viewport mode: temperature unit shown in the\n  \
                                    planet card. Default `f` (Fahrenheit). Internal\n  \
                                    physics is always Kelvin regardless.\n  \
             --narration            stream human-readable narration lines to stdout\n  \
                                    as the sim runs (one sentence per event). The\n  \
                                    NDJSON --out file still receives the full event\n  \
                                    stream; this flag is purely additive on the\n  \
                                    terminal side. Overrides --cli (narration owns\n  \
                                    stdout for the duration of the run).\n  \
             --replay-narration <path>  skip the sim entirely; read a previously-\n  \
                                    recorded NDJSON event log at <path> and emit\n  \
                                    narration lines to stdout retroactively. Mutually\n  \
                                    exclusive with --narration.\n  \
             --config               interactive planet-builder before the sim\n  \
                                    starts. Asks one question at a time (substrate,\n  \
                                    atmosphere, temperature, gravity, star, tilt,\n  \
                                    day length, year length, moons, magnetosphere,\n  \
                                    crust, biosphere). Map geography stays seeded;\n  \
                                    every other scalar is yours. Press 0 / Enter to\n  \
                                    leave a value at the seed default.\n\
         \n\
         Each seed yields a different planet. Try a few seeds — most\n\
         produce coherent worlds, some are inhospitable. Recognition\n\
         events differ per planet based on what physics + chemistry\n\
         produce.\n"
    );
}
