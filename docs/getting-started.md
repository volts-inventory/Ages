# Getting started

A walkthrough for first-time users. Goal: from a fresh clone to a
finished run with a written history in about ten minutes. After
this you should know which doc to read next for whatever drew you
in — physics, civs, recognition, narration, or worldgen.

## Install

You need:

- **Rust 1.94.0** — the toolchain is pinned in
  `rust-toolchain.toml`. The first `cargo` command after clone
  will trigger `rustup` to install it automatically. If you
  don't have `rustup`, install it from <https://rustup.rs>.
- **Python 3** — only if you want the standalone prose narrator
  (`narrate.py`). The in-binary `--narration` flag has no Python
  dependency.
- **Bash** — only if you want the one-shot launcher (`run.sh`).

Clone and build in release mode (debug is several × slower; the
sim is fixed-point-heavy):

```sh
git clone <repo-url> ages
cd ages
cargo build --release
```

Two binaries land in `target/release/`:

- `ages` — the simulation itself.
- `ages-report` — turns an NDJSON event log into a markdown report.

## First run

The canonical hello-world:

```sh
cargo run --release -- --seed 42 --years 100
```

Defaults: writes the NDJSON event log to `runs/run.ndjson` and
streams every event to stdout (`--cli all`). For a 100-year run
that's a lot of stdout — pipe to `less` or pick a quieter mode
(see below).

To watch the planet evolve interactively instead:

```sh
cargo run --release -- --seed 42 --years 100 --cli viewport
```

This opens an alternate-screen ASCII viewport showing the planet,
the species, scrolling events, and per-civ panels. Hit `q` to
quit early; the NDJSON file is still complete on exit.

For a fresh-seed launch with all the defaults wired up:

```sh
./run.sh
```

`run.sh` generates a random seed, builds the release binary, opens
the viewport for a 5000-year run, and archives the NDJSON to
`runs/{date}-{seed}.ndjson`. Re-run a specific world with
`./run.sh <seed>`.

## What you'll see in the viewport

Themed ASCII boxes laid out around an ASCII map of the planet. The
panels:

- **Planet card.** Substrate (water / ammonia / hydrocarbon /
  silicate), surface temperature, atmospheric pressure, magnetic
  field, age. Updates rarely — these are slowly-varying state.
- **Species card.** Body plan, sensorium, lifecycle, cognition
  topology, tolerance envelope. Updates when speciation /
  extinction events fire.
- **Per-civ panels.** Population, cohesion, religion axis,
  latest tool unlock, war / peace status, pop-trend arrow.
  One panel per active civ.
- **Event log.** Scrolling list of events as they fire. Major
  events (founding, collapse, catastrophe, tech unlock, contact,
  war) are highlighted.
- **The map.** Hex grid rendered as ASCII. Each cell shows
  either a terrain glyph (uninhabited) or a per-civ digit /
  density block (claimed).

Full reference: `docs/viewport.md`.

## Real-time narration

Instead of streaming raw events, you can have the sim narrate
itself as prose as it runs:

```sh
cargo run --release -- --seed 42 --years 100 --narration
```

`--narration` owns stdout — it's mutually exclusive with the
`--cli` verbosity matrix because interleaving prose with NDJSON or
viewport ANSI frames mid-sentence would be unreadable. The NDJSON
file emitter still receives the full event stream, so you don't
lose anything.

Narration paces itself naturally because most narrated events fire
once per structural transition (a founding, a collapse, a war
declaration) rather than per tick.

## Replay narration

If you have an archived NDJSON log from a previous run and want to
read the prose without re-running the sim:

```sh
cargo run --release -- --replay-narration runs/2026-05-23-12345.ndjson
```

This skips the sim entirely — no fresh event log is written, no
`RunConfig` is constructed — and just reads the supplied NDJSON
line by line, emitting narration to stdout.

There's also a standalone Python narrator with a slightly
different style:

```sh
./narrate.py runs/2026-05-23-12345.ndjson
```

Pure stdlib Python 3; no dependencies. Useful if you want to pipe
the prose somewhere the Rust binary can't go.

## Build your own planet (`--config`)

By default the seed picks every planet attribute. If you want to
hand-build a world — silicate biology on a lava sea, ammonia
seas under a K-dwarf, Earth-mass at Titan temperature — pass
`--config` and answer the 12 prompts:

```sh
cargo run --release -- --seed 42 --years 100 --config --cli viewport
```

The cosmic-loom GM walks through:

1. Substrate (aqueous / ammoniacal / hydrocarbon / silicate)
2. Atmosphere (none / thin / oxidising / reducing / hazy)
3. Mean surface temperature (Pluto … Mars … Earth … Venus … molten)
4. Surface gravity (Moon … Mars … Earth … super-Earth … crushing)
5. Stellar host (M / K / G / F / A)
6. Axial tilt (0° … Earth 23° … Uranus 90°)
7. Day length (breakneck 6 h … Earth 24 h … Venus 2800 h)
8. Year length (8–16 months)
9. Moon count (0–4)
10. Magnetosphere (none / weak / strong)
11. Crust mineral (basaltic / hydrocarbon / piezoelectric / ferrous / rare-earth)
12. Biosphere richness (sparse / lush / hyperbiodiverse)

On every prompt, option `0` (or empty Enter) keeps the seed
default. **Map geography always comes from `--seed`** — the
prompt only overrides planet-level scalars, so the terrain
layout stays varied across `--config` runs on the same seed.

Substrate/atmosphere overrides automatically re-sample the
atmospheric and crustal compositions, so the resulting planet
stays internally consistent (a hydrocarbon biosphere will have
methane-rich air even if the user only picked the substrate).

After-pick warnings surface when choices fight: picking
aqueous + 40 K logs `⚠ outside the Aqueous liquid window` but
still proceeds.

## Reading the post-run report

Every run produces an NDJSON event log. To render that into a
human-readable markdown report:

```sh
cargo run --release --bin ages-report -- \
  --in runs/2026-05-23-12345.ndjson \
  --out report.md
```

The report covers:

- Planet card and species card.
- Per-civ chapters: founding, growth, conflicts, collapse,
  successor relationships.
- Per-civ economy and life-expectancy arcs.
- Spatial keyframes (who lived where, how densely).
- War campaigns and trade routes.
- Discoveries: hypotheses each civ entertained, which they
  refined, which they falsified.
- A highlight reel of the most narratively load-bearing events.

A representative seed-42 / 5000-year run produces about 14 civs
and around 1000 lines of report. Full reference: `docs/report.md`.

## CLI verbosity modes

`--cli` selects what goes to stdout. The NDJSON file always
receives the full event stream regardless of mode.

- `quiet` — file only, nothing on stdout. Best for batch.
- `all` — tee every event to stdout (default).
- `highlights` — only major events. Tail-able on long runs
  without flooding the terminal.
- `viewport` — live ASCII map in alternate-screen mode.
- `viewport-density` — same map, but claimed cells render as
  density blocks (` ░ ▒ ▓ █`) sized by pop fill-percentage
  instead of per-civ digits.

Pacing flags:

- `--tick-rate-ms <ms>` throttles the streaming sink so a long
  run is readable in real time. Defaults are mode-appropriate;
  the file write is never throttled.
- `--frame-every-ticks <n>` (viewport modes) renders one frame
  per *n* ticks instead of every tick. Useful for very long
  runs where per-tick refresh is too fast to follow.

Grid size:

- `--grid-width <w>` and `--grid-height <h>` override the
  default grid. Smaller grids run faster; larger grids resolve
  climate bands and civ territories more finely. Changing the
  grid changes the seed-to-world mapping — `(seed, w, h)` is
  the determinism key, not just `seed`.

## What just happened

You ran a deterministic physics simulation that:

1. Sampled a planet from your seed (worldgen: substrate,
   atmosphere, terrain, magnetic field, star, orbit).
2. Stepped physics tick-by-tick (heat, fluids, hydrology,
   tides, tectonics, atmospheric escape).
3. Evolved a species fitted to that planet's substrate and
   habitability.
4. Let civilizations form, expand, contact each other, fight,
   discover physics about their world, collapse, and seed
   successors.
5. Emitted every structural transition as an NDJSON event.

Same seed always reproduces the same run, byte-for-byte —
arithmetic is fixed-point (Q32.32), so there's no floating-point
non-determinism, and every randomness consumer threads a seeded
`ChaCha20Rng`.

## Recommended next reads

Pick by interest:

- **How the sim is shaped.** `docs/architecture.md` — process
  layout, tick loop, event model. Read this if you're going to
  touch any non-trivial code.
- **Physics.** `docs/physics.md` — laws, time-stepping,
  conservation invariants. Read alongside `sim/physics/README.md`
  for implementation notes.
- **Worldgen.** `docs/world.md` — planet sampling, atmosphere,
  terrain, hydrology.
- **Civilizations.** `docs/civ.md` — civ lifecycle, founding,
  collapse, succession.
- **Discovery and science.** `docs/discovery.md` — how civs
  fit functional forms against the physics they can perceive.
- **Recognition templates.** `docs/recognition.md` — what
  phenomena are visible to civs, gated by their sensorium.
- **Culture, religion, war.** `docs/culture.md`.
- **Population and migration.** `docs/population.md`.
- **Catastrophes.** `docs/catastrophes.md`.
- **Viewport.** `docs/viewport.md` — what every glyph and panel
  means.
- **Post-run report and narrator.** `docs/report.md`.
- **Full doc index.** `docs/MANIFEST.md`.
- **Contributing.** `CONTRIBUTING.md` at the repo root.
- **Operational rules for agents.** `AGENTS.md` at the repo
  root — also useful for human contributors who want the
  unvarnished version of the determinism and schema rules.

The vision document (`README.md` and `docs/PROJECT.md`) covers
the six guiding principles if you want the *why* before the
*how*.
