# Ages

A deterministic simulator that writes the biography of an alien
species across thousands of years on a procedurally generated
planet.

You give it a seed. It samples a world (ocean depths, atmosphere,
magnetism, terrain), evolves a species that fits the world, lets
civilizations rise and fall inside that species, and produces a
written history — a planet card, a species card, civ-by-civ
chapters, a population timeline, war campaigns, and the
discoveries each civilization made about the physics of its
world. No LLM. No API keys. No network. Same seed always
produces the same history.

It's for people who like emergent worlds, alternative-physics
toys, replayable seeds, or just reading the story a computer
made up on its own.

## Install

You need:

- Rust (stable) with `cargo`
- Python 3 (only if you want the prose narrator)
- Bash (only if you want the one-shot launcher)

Clone and build:

```sh
git clone <repo-url> ages
cd ages
cargo build --release
```

That produces two binaries in `target/release/`:

- `ages` — the simulation itself
- `ages-report` — turns an NDJSON event log into a markdown
  report

## Usage

### One-shot: watch a world live

```sh
./run.sh
```

Generates a random seed, builds the release binary, and opens a
live ASCII viewport for a 5000-year run. The seed prints to
stdout before launch; rerun the same world with `./run.sh
<seed>`. The full event log archives to `runs/{date}-{seed}.ndjson`.

### Tell the story afterward

```sh
./narrate.py runs/2026-05-11-1830-12345.ndjson
```

Prints the run as readable prose: title, world description,
inhabitants, major events grouped by year, ending stats. Pure
stdlib Python 3.

### Render the full markdown report

```sh
cargo run --release --bin ages-report -- \
  --in runs/2026-05-11-1830-12345.ndjson \
  --out report.md
```

### Manual invocations

```sh
# Custom seed and run length with live viewport
cargo run --release --bin ages -- \
  --seed 42 --years 1000 \
  --cli viewport --out runs/manual.ndjson

# Quiet batch run (NDJSON file only)
cargo run --release --bin ages -- \
  --seed 42 --years 5000 \
  --cli quiet --out events.ndjson
```

CLI modes (`--cli`):

- `quiet` — write the NDJSON file, nothing else
- `all` — tee every event to stdout (default)
- `highlights` — only major events (founding, collapse,
  catastrophe, tech unlocks, contact, war, run-end)
- `viewport` — live ASCII map refreshed in alternate-screen mode

The NDJSON file is written at full speed regardless of mode.
Same `(seed, grid)` pair produces byte-for-byte identical NDJSON
regardless of `--cli` mode or tick rate.

## Features

- **Procedural worlds, every seed habitable.** Substrate-first
  sampling (water, ammonia, hydrocarbon, or silicate chemistry)
  ensures every seed grows life of *some* kind.
- **Real physics on a hex grid.** Heat, fluids, hydrology,
  magnetism, Lorentz force, Coriolis, tides, radiation, and
  vertical convection — all in Q32.32 fixed-point arithmetic so
  runs are bit-reproducible.
- **Civilizations that do science.** Civs fit functional forms
  (linear, sine, inverse-square, logistic, power law, and
  others) against the physics they can actually perceive. They
  hold competing hypotheses, refine wrong ones, build
  experimental apparatus to clamp variables, and pass knowledge
  to successor civs with comprehension decay.
- **Different worlds, different sciences.** What a civ can fit
  is gated by what its sensorium can perceive and which
  recognition templates fire on its planet. A civ that never
  sees a periodic phenomenon literally cannot propose a sine
  curve.
- **Multi-civ histories.** Civs found, expand, collapse, and
  seed successors within the same species across thousands of
  years. Cohesion drives civil wars; belligerence drives
  inter-civ wars; religion-weighted kinship gates whether
  contact turns hostile.
- **Live viewport.** Themed ASCII boxes show the planet, the
  species, scrolling events, and per-civ panels as the
  simulation runs.
- **Post-run report.** A markdown biography covering the planet,
  the species, every civ, spatial keyframes (who lived where,
  how densely), war campaigns, discoveries, and a highlight
  reel. A representative seed-42 / 5000-year run produces ~14
  civs and ~1000 lines of report.
- **Prose narrator.** `narrate.py` reads the same NDJSON and
  prints the run as a readable story.

## Output channels

Every run produces some subset of the following, depending on
mode:

1. **NDJSON event log** — append-only, one event per line; the
   canonical record. Schema in `protocol/`.
2. **Snapshot events** — state-digest checkpoints embedded in
   the NDJSON every 500 ticks.
3. **Live stdout stream** — events tee'd in real time
   (configurable verbosity).
4. **Live ASCII viewport** — terminal-rendered planet that
   evolves as you watch.
5. **Markdown report** — produced by `ages-report` from any
   saved NDJSON.

## Documentation

In-repo docs, by topic:

- [`docs/PROJECT.md`](docs/PROJECT.md) — project vision and
  guiding principles, mapped to shipped features
- [`docs/architecture.md`](docs/architecture.md) — process
  layout, tick loop, event model
- [`docs/world.md`](docs/world.md) — planet sampling,
  atmosphere, terrain, hydrology
- [`docs/physics.md`](docs/physics.md) — laws and time-stepping
- [`docs/recognition.md`](docs/recognition.md) — phenomenon
  templates
- [`docs/species.md`](docs/species.md) — species derivation and
  drift
- [`docs/civ.md`](docs/civ.md) — civ lifecycle
- [`docs/discovery.md`](docs/discovery.md) — hypothesizer, fits,
  rivals, experiments
- [`docs/culture.md`](docs/culture.md) — cosmology, religion,
  kinship, war
- [`docs/tech.md`](docs/tech.md) — tools and tech tree
- [`docs/population.md`](docs/population.md) — cohorts,
  migration, nomads
- [`docs/catastrophes.md`](docs/catastrophes.md) — catastrophe
  kinds
- [`docs/viewport.md`](docs/viewport.md) — live CLI viewport
- [`docs/report.md`](docs/report.md) — post-run report and
  narrator
- [`docs/MANIFEST.md`](docs/MANIFEST.md) — full doc index
- [`AGENTS.md`](AGENTS.md) — operational rules for contributors
- [`PLANNING.md`](PLANNING.md) — current state and active work

Each crate under `sim/` has its own `README.md` with
implementation notes.

## Project layout

```
run.sh           one-shot fresh-world launcher
narrate.py       post-run prose narrator
ages/            run binary and ages-report binary
sim/             simulation crates (arith, physics, world,
                 recognition, species, civ, population,
                 events, report, core)
protocol/        wire schema for NDJSON events
docs/            cross-crate and per-feature documentation
runs/            archived NDJSON event logs (gitignored)
```

## Contributing

Contributions are welcome. Before opening a PR:

1. Read [`AGENTS.md`](AGENTS.md) for repo conventions
   (determinism rules, schema discipline, where things live).
2. Work on a feature branch, not `main`.
3. Run the local checks:

   ```sh
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```

4. If your change touches scope or design, update the relevant
   per-feature doc and `PLANNING.md` in the same commit.

For substantial design changes, open an issue first to discuss
direction.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)
  or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <http://opensource.org/licenses/MIT>)

at your option.
