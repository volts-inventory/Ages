# Ages

**Ages simulates the rise and fall of civilizations on a procedurally
generated planet, and writes the story of what they discovered.**

You give it a seed. It samples a world (atmosphere, oceans, magnetism,
chemistry), evolves a species to fit that world, and lets civilizations
emerge, learn physics from what they can perceive, transmit knowledge
across collapses, and eventually end. When the run finishes, you get a
markdown biography of the species across its full history.

It is a headless, deterministic Rust simulation. No LLM, no API keys, no
external services — the planet's physics is the source of truth, and the
same `(seed, grid)` pair always produces byte-for-byte identical output.

Built for people who like procedural worlds, alternate-history
thought experiments, deterministic simulations, or just watching a
strange planet's science evolve in ASCII.

## Install

Requirements:

- Rust toolchain (stable; the build uses `cargo build --release`)
- Python 3 (only needed for the post-run prose narrator)
- Bash (only needed for the `run.sh` launcher)

Clone and build:

```sh
git clone <this-repo> ages
cd ages
cargo build --release
```

That produces two binaries in `target/release/`:

- `ages` — the simulation itself
- `ages-report` — turns an NDJSON event log into a markdown report

## Usage

The fastest way in:

```sh
./run.sh
```

`run.sh` picks a random seed, builds the binary, and opens a live ASCII
viewport for a 5000-year run paced at roughly half a sim-year per frame.
The seed prints to stdout before launch, and the run's full event log is
archived to `runs/{date}-{seed}.ndjson`.

Replay a memorable world:

```sh
./run.sh 12345        # rerun seed 12345 (same geometry → identical run)
```

Generate a prose story from any archived run:

```sh
./narrate.py runs/2026-05-10-1900-12345.ndjson
```

Generate the full markdown report:

```sh
cargo run --release --bin ages-report -- \
  --in runs/2026-05-10-1900-12345.ndjson \
  --out report.md
```

### Custom runs

For non-default knobs, invoke the binary directly:

```sh
# Custom seed + years with live viewport:
cargo run --release --bin ages -- --seed 42 --years 1000 \
  --cli viewport --tick-rate-ms 50 --frame-every-ticks 6 \
  --out runs/manual.ndjson

# Quiet batch run (NDJSON only, no terminal output):
cargo run --release --bin ages -- --seed 42 --years 5000 \
  --cli quiet --out events.ndjson
```

CLI verbosity modes (`--cli`):

- `quiet` — only the NDJSON file is written
- `all` — every event tee'd to stdout (default)
- `highlights` — only structural pins (founding, collapse, catastrophe,
  tech unlocks, contact, transmission, conflict, template/tool
  discovery, run-end)
- `viewport` — live ASCII planet + territory map, refreshed every
  `--frame-every-ticks` ticks (36×30 grid by default)

The NDJSON file always writes at full speed regardless of verbosity.
Determinism is a contract: identical `(seed, grid)` produces identical
NDJSON, no matter the `--cli` mode or tick rate.

### What you'll see

The live viewport sections itself into themed boxes — planet identity,
the map, a glyph legend, the species card, scrolling event log, and
per-civ sidebars:

```
---------------- Lyra-a ----------------
        ocean world · scorching
    methane-rich · 241F · strong mag
      129h · 11mo · 17° · 2 moons
         Y0 M0 · 1 civ · 1F/0C · 1834p

----------------- map ------------------
     01234567890123456789012345678901
    +--------------------------------+
   0|A11≈≈≈≈≈≈░▒▒▒▒▒△△△△△△△△△△▲▲▲△△△1|
   1|11≈≈≈≈≈≈░▒▒▒▒▒△△△△△△△△△△▲▲▲▲▲△△1|
   …
  19|111≈≈≈≈≈≈≈≈≈░▒▒▒▒▒△△△△△△000△△△△△|
    +--------------------------------+

----------------- key ------------------
  A=cap · 1=civ · #=war · 0=nomad · ~sea · ≈deep
     ▲peak · △hill · ▒land · ░coast
```

Terrain glyphs colour-code in capable terminals: blue water, brown
mountains, green land, white peaks. `0` glyphs mark **nomadic**
(unclaimed) population concentrations.

After the run, `narrate.py` reads the same NDJSON and prints a templated
prose story — title, world description, inhabitants, major arcs grouped
by year, ending stats. Pure stdlib Python 3.

## Features

**Substrate-first planet sampling.** Every seed produces life of some
chemistry — Aqueous, Ammoniacal, Hydrocarbon, or Silicate — and a
biosphere derived from it. Atmospheres, oceans, terrain, magnetism, and
moons are all sampled before the species fits the world.

**Deterministic physics on a hex grid.** Heat, fluids, hydrology,
magnetism, Lorentz force, Coriolis, tides, radiation, and vertical
convection, all run through fixed-point (Q32.32) arithmetic so the same
seed always reproduces exactly.

**Recognition templates.** 39 authored templates turn raw physics state
into named phenomena (lightning, frost, tides, auroras…). Civs discover
new templates at runtime from confirmed thresholds; the species canon
adopts them across civ generations.

**Per-civ science.** Each civ runs its own hypothesizer over what its
species can perceive, fitting twelve functional forms (Constant, Linear,
PeriodicSine, InverseSquare, ExpDecay, ExpGrowth, Logistic, Polynomial2,
Polynomial3, ThresholdStep, PowerLaw, Logarithmic) to two tracks:

- **Firing relations** — does the template fire as a function of a
  channel reading? Recovers thresholds.
- **Measurement relations** — both axes continuous, including spatial
  Laplacians and temporal deltas. Recovers SI coefficients (e.g. heat
  diffusion `α`).

The form vocabulary is gated by which structural tags the civ's
perceivable templates carry — civs that never observe a `Periodic`
template literally cannot fit sinusoidal forms. **Different worlds,
different sciences.**

**Predict, falsify, refine.** Confirmed relations track a falsification
streak; sustained mispredictions force an Occam-adjusted refinement on
probation. A rivals pool holds alternative forms for the same
(template, channel) — when a rival overtakes the incumbent, it
displaces it (phlogiston-vs-oxygen, geocentric-vs-heliocentric).
Confirmed measurements auto-generate residual children up to three
levels deep.

**Controlled experiments.** Civs that unlock the experiment-apparatus
tool deterministically clamp a physics channel through a four-value
ladder and sample the response. Apparatus samples weigh double and mark
confirmed relations as experimental.

**Knowledge fidelity continuum.** Inter-civ transmission isn't binary.
High comprehension transfers the relation as confirmed knowledge;
mid-comprehension lands in a **mythologization band** that nudges the
receiving civ's cosmology along an aligned axis without transferring
the content. A society that lost the original physics retains the taboo
or sacred reverence around it.

**Inheritance with revalidation.** Successor civs re-fit transmitted
relations after a 50-tick window; failures emit `RelationLapsed` and
drop.

**Tech tree.** 58 tools across 5 tiers with strict prereqs plus
serendipitous unlocks. Emergent tools propose from confirmed-relation
clusters; all fold through the same effect aggregator across 8 effect
categories.

**Population, migration, conflict.** Per-cell heterogeneous populations
with substrate-derived demographics, gradient-driven migration, a
nomadic species pool, habitat-priority diffusion, and per-terrain
habitability multipliers. Cohesion-driven civil war and breakaway
paths. Multi-tick wars with marching fronts. Belligerence-driven war
declaration gated on contact and kinship-dampened drive scores.

**Two-layer culture.** Slow-drift species-anchored cosmology (5 axes)
plus fast-divergent civ-keyed religion (3 axes). Kinship weighted
dominantly by religion.

**Outputs.** A representative 5000-year run produces ~14 civs, 7000+
confirmed relations, 1800+ knowledge transmissions across collapse
boundaries, 4000+ conflict skirmishes grouped into war campaigns, and
~1000 lines of report. Five output channels:

1. **NDJSON event log** — append-only, one event per line, the
   canonical history of the run. Schema in `protocol/`, split by domain
   (`header`, `world_events`, `civ_events`, `discovery_events`,
   `snapshot`).
2. **Periodic `Snapshot` events** — state-digest checkpoints every 500
   ticks (totals, active and collapsed civs).
3. **Live CLI stream** — events tee'd to stdout (verbosity
   configurable).
4. **Live ASCII viewport** — the planet rendered into your terminal as
   it evolves, using the same renderer the post-run keyframes use.
5. **Markdown post-run report** (`ages-report`) — planet card, species
   card, Ages timeline, run summary, memorable figures, species canon,
   per-civ chapters (with discovered templates and invented tools),
   transmission/diffusion/conflict tables, spatial-timeline keyframes
   (paired ownership + density maps), population timeline, war
   campaigns, and the highlight reel.

### Spatial timeline (paired keyframes)

Every few keyframes across the run, the report renders both an
ownership map and a density map of the same moment:

- **Ownership** — `A`/`B`/`C` capital letters mark each civ's centroid;
  cells claimed by exactly one civ render as that civ's digit (`1`–`9`,
  `*` for civs ≥10); disputed cells render as `#`; nomadic-only cells
  render as `0`; unclaimed cells show terrain.
- **Density** — same capital letters; claimed cells render as Unicode
  block-shading (` ░ ▒ ▓ █`) keyed to per-cell population relative to
  the densest cell in the frame. Capitals pop as `█`; frontiers fade to
  `░`.

Together: who owns what, and where the people actually live.

## Project structure

```
run.sh                    one-shot fresh-world launcher
narrate.py                post-run prose narrator
ages/                     headless run binary + ages-report binary
sim/
├── arith/                deterministic real arithmetic (Q32.32 fixed point)
├── physics/              gravity, fluids, heat, EM, climate /
│                         atmosphere / tides / Coriolis / Lorentz /
│                         vertical convection
│   └── chemistry/        constants, substances, reactions,
│                         substrate phase thresholds
├── world/                planet sampling (substrate-first)
├── recognition/          phenomenon templates (39 authored;
│                         emergent templates extend at runtime)
├── species/              species derivation; sensorium gating
├── civ/                  civ lifecycle, hypothesizer, tech, culture
├── core/                 tick loop, phase ordering, run orchestration
├── events/               emitter + filter + counting + viewport tee
├── report/               post-run markdown report; live ASCII viewport
└── population/           cohorts; substrate-derived demographics
protocol/                 wire schema, split by event domain
docs/                     cross-crate + per-feature docs
runs/                     archived NDJSON event logs (gitignored)
```

For mechanics depth on a single topic, jump straight to the per-feature
docs in [`docs/`](docs/). A condensed map of vision-vs-shipped lives in
[`docs/PROJECT.md`](docs/PROJECT.md).

Highlights:

- [`docs/architecture.md`](docs/architecture.md) — process layout,
  event/snapshot model, phase order
- [`docs/world.md`](docs/world.md) — planet model: substrate,
  atmosphere, terrain, hydrology, magnetism
- [`docs/physics.md`](docs/physics.md) — laws, grid, time-stepping
- [`docs/recognition.md`](docs/recognition.md) — templates, signatures,
  fields, perceivable vs latent gating
- [`docs/discovery.md`](docs/discovery.md) — form vocabulary, fits,
  theory hierarchy, rivals, mythologization, experiments
- [`docs/species.md`](docs/species.md) — derivation, sensorium, per-civ
  drift
- [`docs/civ.md`](docs/civ.md) — civ lifecycle, founding, succession,
  cohesion, breakaway
- [`docs/culture.md`](docs/culture.md) — cosmology, religion, kinship,
  conflict, war
- [`docs/tech.md`](docs/tech.md) — 58-tool tree, prereqs, effects,
  serendipity, emergent + apparatus
- [`docs/population.md`](docs/population.md) — cohorts, demographics,
  migration, nomads
- [`docs/catastrophes.md`](docs/catastrophes.md) — five kinds, severity,
  cells
- [`docs/viewport.md`](docs/viewport.md) — live CLI viewport layout
- [`docs/report.md`](docs/report.md) — post-run markdown report +
  `narrate.py`
- [`docs/MANIFEST.md`](docs/MANIFEST.md) — full doc index with line
  counts

## Contributing

Contributions are welcome. Before opening a PR:

1. **Read [`AGENTS.md`](AGENTS.md)** for the project's operational
   rules and routing conventions. It applies to human and AI
   contributors alike.
2. **Check [`PLANNING.md`](PLANNING.md)** for current state and
   in-flight work, so you don't duplicate or step on something already
   under way.
3. **Skim the relevant per-feature doc** in [`docs/`](docs/) for the
   subsystem you're touching.
4. **Run the tests** — `cargo test --workspace` — and a representative
   live run (`./run.sh 42`) before submitting.
5. **Preserve determinism.** Any change that touches the simulation
   path must keep the `(seed, grid) → NDJSON` contract intact. The
   viewport, throttle, and live CLI all sit outside the compute path
   for a reason.

Open an issue first for anything substantial; small fixes can go
straight to a PR.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <http://opensource.org/licenses/MIT>)

at your option. See the workspace `Cargo.toml` for the canonical
declaration.
