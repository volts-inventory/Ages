//! Interactive terminal UI for live runs. Replaces the legacy
//! hand-rolled ANSI viewport with a [`ratatui`] dashboard driven on
//! its own thread: the sim runs in the background feeding events over
//! a channel (see [`sim_events::ChannelEmitter`]), this loop drains
//! them into a [`ViewportEmitter`] state-mirror at ~30 fps, polls the
//! keyboard for controls, and re-renders. Pause / step / speed are
//! mediated by a shared [`PaceControl`] the sim thread consults each
//! tick.
//!
//! The data model is the existing [`ViewportEmitter`] (used here as a
//! pure state accumulator via [`ViewportEmitter::apply`]); all the
//! event-classification, log, and per-civ snapshot logic is reused
//! through its public accessors so the TUI never re-implements it.

mod view;

use crate::viewport::{TempUnit, ViewportConfig, ViewportEmitter};
use protocol::Event;
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::crossterm::{
    cursor::{Hide, Show},
    event::{self, Event as CEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::Terminal;
use sim_events::PaceControl;
use std::io::{self, Sink};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

/// Knobs the binary hands the TUI when it spins up.
pub struct TuiOptions {
    /// Per-civ ANSI palette colours on the map + swatches. Disable
    /// with `--no-color`; the map then falls back to civ-id digits.
    pub use_color: bool,
    /// Start the map in density mode (terrain glyph, brightness =
    /// fill %) rather than the pop-digit ladder. Toggle live with `d`.
    pub density_mode: bool,
    /// Temperature unit for the planet card.
    pub temperature_unit: TempUnit,
    /// Initial per-tick delay (maps to a starting speed-ladder rung).
    pub initial_delay: Duration,
    /// Begin paused (the user presses space / step to advance).
    pub start_paused: bool,
}

/// Running-speed ladder: per-tick delays from slowest to fastest.
const SPEED_DELAYS_MS: [u64; 8] = [500, 250, 120, 60, 30, 12, 4, 0];
/// Human labels paired with `SPEED_DELAYS_MS`.
const SPEED_LABELS: [&str; 8] = ["0.1x", "0.25x", "0.5x", "1x", "2x", "5x", "15x", "max"];

fn nearest_speed_idx(delay: Duration) -> usize {
    let ms = u64::try_from(delay.as_millis()).unwrap_or(u64::MAX);
    SPEED_DELAYS_MS
        .iter()
        .enumerate()
        .min_by_key(|(_, d)| d.abs_diff(ms))
        .map_or(3, |(i, _)| i)
}

/// Which dashboard view is active.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    World,
    Civilizations,
    Planet,
}

impl Tab {
    const ALL: [Tab; 3] = [Tab::World, Tab::Civilizations, Tab::Planet];

    fn title(self) -> &'static str {
        match self {
            Tab::World => "World",
            Tab::Civilizations => "Civilizations",
            Tab::Planet => "Planet",
        }
    }

    fn index(self) -> usize {
        match self {
            Tab::World => 0,
            Tab::Civilizations => 1,
            Tab::Planet => 2,
        }
    }

    fn next(self) -> Tab {
        Tab::ALL[(self.index() + 1) % Tab::ALL.len()]
    }

    fn prev(self) -> Tab {
        Tab::ALL[(self.index() + Tab::ALL.len() - 1) % Tab::ALL.len()]
    }
}

/// UI-local state, distinct from the sim-mirrored `ViewportEmitter`:
/// which tab/civ is selected, scroll offsets, the speed rung, and the
/// quit / run-complete latches. The handful of independent bool flags
/// are each toggled by a distinct key, so a state enum wouldn't fit.
#[allow(clippy::struct_excessive_bools)]
struct UiState {
    tab: Tab,
    selected_civ: usize,
    /// Index of the first civ shown in the list pane. Moved by `[`/`]`
    /// independently of the selection, and nudged by selection moves
    /// so the cursor stays in view. Clamped against the pane height at
    /// render time.
    civ_scroll: usize,
    /// Visible row count of the civ-list pane, cached each frame so
    /// keyboard scrolling can page by a screenful.
    civ_rows: usize,
    /// Lines scrolled down from the top of the civ-detail pane.
    detail_scroll: usize,
    /// Lines scrolled up from the bottom of the event log. `0` =
    /// following the newest line.
    log_scroll: usize,
    density: bool,
    use_color: bool,
    speed_idx: usize,
    should_quit: bool,
    run_complete: bool,
}

impl UiState {
    fn new(opts: &TuiOptions) -> Self {
        Self {
            tab: Tab::World,
            selected_civ: 0,
            civ_scroll: 0,
            civ_rows: 0,
            detail_scroll: 0,
            log_scroll: 0,
            density: opts.density_mode,
            use_color: opts.use_color,
            speed_idx: nearest_speed_idx(opts.initial_delay),
            should_quit: false,
            run_complete: false,
        }
    }

    fn faster(&mut self, pace: &PaceControl) {
        if self.speed_idx + 1 < SPEED_DELAYS_MS.len() {
            self.speed_idx += 1;
            pace.set_delay(Duration::from_millis(SPEED_DELAYS_MS[self.speed_idx]));
        }
    }

    fn slower(&mut self, pace: &PaceControl) {
        if self.speed_idx > 0 {
            self.speed_idx -= 1;
            pace.set_delay(Duration::from_millis(SPEED_DELAYS_MS[self.speed_idx]));
        }
    }

    fn select_next(&mut self, civ_count: usize) {
        if civ_count > 0 {
            self.selected_civ = (self.selected_civ + 1) % civ_count;
            self.detail_scroll = 0;
            self.follow_selection();
        }
    }

    fn select_prev(&mut self, civ_count: usize) {
        if civ_count > 0 {
            self.selected_civ = (self.selected_civ + civ_count - 1) % civ_count;
            self.detail_scroll = 0;
            self.follow_selection();
        }
    }

    /// Nudge the civ-list scroll offset so the current selection stays
    /// within the visible window (using the last-rendered row count).
    fn follow_selection(&mut self) {
        let rows = self.civ_rows;
        if rows == 0 {
            return;
        }
        if self.selected_civ < self.civ_scroll {
            self.civ_scroll = self.selected_civ;
        } else if self.selected_civ >= self.civ_scroll + rows {
            self.civ_scroll = self.selected_civ + 1 - rows;
        }
    }

    /// Page the civ list up/down independently of the selection.
    /// Down is clamped at render time against the civ count.
    fn civ_page(&mut self, down: bool) {
        let page = self.civ_rows.max(1);
        self.civ_scroll = if down {
            self.civ_scroll.saturating_add(page)
        } else {
            self.civ_scroll.saturating_sub(page)
        };
    }

    fn handle_key(&mut self, key: KeyEvent, civ_count: usize, pace: &PaceControl) {
        // On the Civilizations tab, PgUp/PgDn/Home/End drive the
        // detail pane (there's no log there); elsewhere they drive the
        // event log. This keeps a single, intuitive key set without a
        // separate pane-focus mode.
        let detail_focus = self.tab == Tab::Civilizations;
        match (key.code, key.modifiers) {
            (KeyCode::Char('q') | KeyCode::Esc, _)
            | (KeyCode::Char('c' | 'd'), KeyModifiers::CONTROL) => self.should_quit = true,
            (KeyCode::Char(' '), _) => pace.toggle_pause(),
            (KeyCode::Char('s' | '.'), _) => pace.step(1),
            (KeyCode::Right | KeyCode::Char('+' | '='), _) => self.faster(pace),
            (KeyCode::Left | KeyCode::Char('-' | '_'), _) => self.slower(pace),
            (KeyCode::Down | KeyCode::Char('j'), _) => self.select_next(civ_count),
            (KeyCode::Up | KeyCode::Char('k'), _) => self.select_prev(civ_count),
            (KeyCode::Char('['), _) => self.civ_page(false),
            (KeyCode::Char(']'), _) => self.civ_page(true),
            (KeyCode::Tab, _) => self.tab = self.tab.next(),
            (KeyCode::BackTab, _) => self.tab = self.tab.prev(),
            (KeyCode::Char('1'), _) => self.tab = Tab::World,
            (KeyCode::Char('2'), _) => self.tab = Tab::Civilizations,
            (KeyCode::Char('3'), _) => self.tab = Tab::Planet,
            (KeyCode::Char('d'), _) => self.density = !self.density,
            (KeyCode::PageUp, _) => {
                if detail_focus {
                    self.detail_scroll = self.detail_scroll.saturating_sub(5);
                } else {
                    self.log_scroll = self.log_scroll.saturating_add(5);
                }
            }
            (KeyCode::PageDown, _) => {
                if detail_focus {
                    self.detail_scroll = self.detail_scroll.saturating_add(5);
                } else {
                    self.log_scroll = self.log_scroll.saturating_sub(5);
                }
            }
            (KeyCode::Home, _) => {
                if detail_focus {
                    self.detail_scroll = 0;
                } else {
                    self.log_scroll = usize::MAX;
                }
            }
            (KeyCode::End, _) => {
                if detail_focus {
                    self.detail_scroll = usize::MAX;
                } else {
                    self.log_scroll = 0;
                }
            }
            _ => {}
        }
    }

    fn speed_label(&self) -> &'static str {
        SPEED_LABELS[self.speed_idx]
    }
}

/// Run the interactive dashboard until the user quits (or the sim
/// finishes and the user acknowledges). Owns the terminal for its
/// lifetime: enables raw mode + the alternate screen on entry and
/// always restores them on exit, even on error. Signals
/// [`PaceControl::quit`] on the way out so the background sim thread
/// unwinds.
pub fn run_interactive_tui(
    rx: &Receiver<Event>,
    pace: &Arc<PaceControl>,
    opts: &TuiOptions,
) -> io::Result<()> {
    let cfg = ViewportConfig {
        frame_every: 0,
        use_alt_screen: false,
        use_color: opts.use_color,
        show_planet_card: true,
        // Generous scrollback for the event-log pane.
        log_lines: 500,
        compact: true,
        temperature_unit: opts.temperature_unit,
        density_mode: opts.density_mode,
    };
    let mut model: ViewportEmitter<Sink> = ViewportEmitter::new(io::sink(), cfg);
    let mut ui = UiState::new(opts);

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, Hide)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    // Force a full clear before the first frame. ratatui's diff
    // renderer otherwise assumes a blank screen and skips writing
    // cells it considers unchanged (spaces), which lets whatever was
    // already on the terminal — the `cargo build` output that run.sh
    // prints just before launching — show through the gaps in the
    // layout. Clearing gives a known-blank canvas to diff against.
    terminal.clear()?;

    let res = run_loop(&mut terminal, &mut model, &mut ui, rx, pace);

    // Always restore the terminal, even if the loop errored.
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen, Show);
    let _ = terminal.show_cursor();
    // Tell the sim thread to stop if it's still running.
    pace.quit();
    res
}

fn run_loop<B: Backend>(
    terminal: &mut Terminal<B>,
    model: &mut ViewportEmitter<Sink>,
    ui: &mut UiState,
    rx: &Receiver<Event>,
    pace: &PaceControl,
) -> io::Result<()> {
    loop {
        // Drain everything the sim has produced since the last frame.
        loop {
            match rx.try_recv() {
                Ok(ev) => {
                    if matches!(ev, Event::RunEnd { .. }) {
                        ui.run_complete = true;
                    }
                    model.apply(&ev);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    ui.run_complete = true;
                    break;
                }
            }
        }

        let civ_count = model.active_civ_count();
        if ui.selected_civ >= civ_count {
            ui.selected_civ = civ_count.saturating_sub(1);
        }

        terminal.draw(|f| view::draw(f, model, ui, pace))?;

        if event::poll(Duration::from_millis(33))? {
            if let CEvent::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    ui.handle_key(key, civ_count, pace);
                }
            }
        }
        if ui.should_quit {
            return Ok(());
        }
    }
}
