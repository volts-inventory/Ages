# Ages

A headless, deterministic Rust simulation that produces **the
biography of a species across its full history**. A planet is
sampled from a seed; a species evolves to fit it; nomadic
populations diffuse across habitable terrain; civilizations
condense out of dense regions, rise, fall, and seed successors
within the same species over thousands of years — each civ
deriving different physics from ours because their world is
different. Knowledge survives collapses through inherited
artifacts with comprehension-decayed transmission. The post-run
report covers the species' full multi-civ history, with a
spatial-timeline section that shows *where* civs grew and *how
densely* their populations packed at each historical snapshot.

A deterministic physics engine (gravity, fluids, heat, simple
chemistry, simplified EM on a hex grid) runs underneath.
Recognition templates turn emergent physics state into named
phenomena. Species sensoria gate what's perceivable. Named
figures fit functional forms to the data they observe — both
binary firing relations ("does lightning correlate with charge?")
and **continuous measurement relations** ("how does my temperature
relate to my neighbours' mean temperature?"), so civs recover
continuous law coefficients in real (SI) units, not just
thresholds. **No LLM in the loop, no API keys, no external
services.**

## How a civ learns from its world

```
physics state                   ──>  recognition         ──>  observation         ──>  hypothesis
(temperature, charge, water,         scan templates           per-civ sensorium        fit forms,
 substances per cell)                fire (template_id,       gate firings;            confirm at
                                     cell) events             collect samples         exp(-1)
```

Twelve fitted forms — `Constant`, `Linear`, `PeriodicSine`,
`InverseSquare`, `ExpDecay`, `ExpGrowth`, `Logistic`,
`Polynomial2`, `Polynomial3`, `ThresholdStep`, `PowerLaw`,
`Logarithmic`. Two parallel sample tracks run on the per-civ
hypothesizer:

- **Firing relations** — `(x = channel reading, y = did the
  template fire on this cell)`. Recovers thresholds. Example: a
  civ confirms `lightning_buildup ↔ charge_magnitude` as a
  `ThresholdStep` with cutpoint ≈ 40.
- **Measurement relations** — `(x = channel reading, y = channel
  reading)`. Both axes continuous, including spatial-Laplacian
  and temporal-delta channels. Recovers SI coefficients. Example:
  a civ fits `delta_temperature ↔ laplacian_temperature` linear
  and recovers the planet's heat-diffusion `α`.

A confirmed relation that drifts under new data triggers
refinement — the civ proposes an Occam-adjusted alternative,
runs it on probation, and either supersedes the old form or
reverts. Cosmology axes bias confirmation: dogmatic civs find
heretical forms harder to confirm; reformist civs confirm faster.

The form vocabulary itself is gated by which structural tags the
civ's perceivable templates carry, so civilisations that never
observe a `Periodic` template literally cannot fit sinusoidal
forms — different planets produce structurally different sciences.

**Competing hypotheses.** Beyond the single confirmed fit per
relation, the hypothesizer holds a **rivals pool** of alternative
forms for the same `(template, channel)`. When a rival's
confidence overtakes the primary's, it displaces the incumbent
(who returns to the rivals pool). Phlogiston-vs-oxygen,
geocentric-vs-heliocentric, miasma-vs-germ: multiple theories
coexist before one wins out.

**Theory hierarchy.** Confirmed measurements auto-generate
residual children that fit *what's left* against another channel,
up to three levels deep. Newton's gravity → Mercury's
perihelion → ... — civs derive layered explanations.

**Knowledge fidelity continuum.** Inter-civ transmission isn't
binary. High comprehension transfers the relation as confirmed
knowledge; mid-comprehension lands in a **mythologization band**
that doesn't transfer the content but nudges the receiving civ's
cosmology along an axis. A society that lost the original physics
of a phenomenon retains the taboo, ritual, or sacred reverence
around it.

**Controlled experiments.** Civs that unlock the experiment-
apparatus tool deterministically clamp a physics channel on one
of their cells through a four-value ladder and sample the
response. Apparatus measurements weigh double in the fit
accumulator and mark confirmed relations as experimental — a
tactile-only species with manipulation tools recovers heat
diffusion from a clean clamp-and-relax protocol; a no-tool
species stays observation-only.

## Quick start

**Requirements:** Rust toolchain (stable; `cargo build --release`),
Python 3 for the narrator, Bash for the launch script. No
external services.

```sh
./run.sh                                          # watch a fresh random world live
./narrate.py runs/2026-…-{seed}.ndjson            # tell the story afterward
```

`run.sh` generates a random seed, builds the binary, and
launches the live viewport for a 5000-year run paced at half a
sim-year per frame. Each run archives its NDJSON to
`runs/{date}-{seed}.ndjson` so previous worlds aren't
overwritten. The seed is echoed to stdout before launch so
memorable worlds can be replayed with `./run.sh <seed>` — the
canonical replay path, since determinism is per-`(seed, grid)`
pair and `run.sh` inherits the binary's default geometry along
with the seed.

`narrate.py` reads any of those NDJSON files and prints a
templated prose story — title, world description, inhabitants,
major arcs grouped by year (foundings / tech unlocks /
catastrophes / collapses), and ending stats. Pure stdlib Python
3, no LLM.

## What you'll see

The live viewport sections itself into themed boxes — planet
identity, the map, a glyph legend, the species card, scrolling
event log, and per-civ sidebar panels:

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
-------------- Cyranites ---------------
      centralized medium cognition
    sense: tactile · manip: tentacle
    44y · solitary · noisy · carbon
----------------- log ------------------
      y0 civ 1 founded (10 cells)
```

Terrain glyphs colour-code in capable terminals: blue water,
brown mountains, green land, white peaks. `0` glyphs mark
**nomadic** (unclaimed) population concentrations; civs absorb
nomads on BFS expansion and may emerge from high-density nomad
regions when prerequisites are met.

Post-run, `narrate.py` reads the same NDJSON and renders prose:

```
════════════════════════════════════════════════════════════
                    The Story of Mira-a
               seed 7  ·  99 simulated years
════════════════════════════════════════════════════════════

THE WORLD
─────────
Mira-a is an ammoniacal world. Atmospheric mix is methane-rich;
mean surface temperature -85°F. The planet has a strong
magnetosphere and 2 moons. Days last 46 hours; the year runs
8 months at an 8° axial tilt. The biosphere is lush.

THE INHABITANTS
───────────────
The Quilites are solitary carbon-based life — centralized
cognition at the low tier. They sense their world through
touch, and manipulate it with secreted chemicals. Lifespans
average 61 years; their communication is precise.

MAJOR ARCS
──────────
⌚ Year 0
   M 0 — Civilization 1 took root, founded by 2 figures
          across 10 cells.
   M11 — Civ 1 unlocked thermal sensor.

⌚ Year 99
   M11 — Civilization 1 collapsed — civil war.
```

Same vocabulary in both surfaces — substrate freeze/boil ranges
and label tables (`ocean world` / `scorching` / `solitary` /
`precise` / `carbon`) ride through the NDJSON via the
`RunMetadata` event so a label change in Rust propagates to the
narrator without code edits on the Python side.

## Manual invocations

For non-default knobs:

```sh
# Custom seed + years (the launch script hardcodes 5000):
cargo run --release --bin ages -- --seed 42 --years 1000 \
  --cli viewport --tick-rate-ms 50 --frame-every-ticks 6 \
  --out runs/manual.ndjson

# Quiet batch run (NDJSON only):
cargo run --release --bin ages -- --seed 42 --years 5000 \
  --cli quiet --out events.ndjson

# Markdown post-run report from the event log:
cargo run --release --bin ages-report -- --in events.ndjson --out report.md
```

CLI verbosity (`--cli`):

- `quiet` — only the NDJSON file
- `all` — every event tee'd to stdout (default)
- `highlights` — only structural pins (founding, collapse,
  catastrophe, tech unlocks, contact, transmission, conflict,
  template/tool discovery, run-end)
- `viewport` — live ASCII planet + territory map refreshed in
  alternate-screen mode every `--frame-every-ticks` ticks
  (36×30 grid by default).

The NDJSON file always writes at full speed regardless of
verbosity. Determinism is a contract: same `(seed, grid)` pair =
byte-for-byte identical NDJSON regardless of `--cli` mode or
`--tick-rate-ms`. `./run.sh <seed>` is the canonical replay
path; direct `cargo run --bin ages -- --seed N ...` must pass
the same `--grid-width` / `--grid-height` to reproduce a prior
run byte-for-byte.

## Outputs

Five output channels per the project vision:

1. **NDJSON event log** — append-only, one event per line, the
   canonical history of the run. Schema in `protocol/`, split
   by domain (`header`, `world_events`, `civ_events`,
   `discovery_events`, `snapshot`).
2. **Periodic `Snapshot` events** — state-digest checkpoints
   embedded in the NDJSON every 500 ticks (totals, active and
   collapsed civs).
3. **Live CLI stream** — events tee'd to stdout during the run
   (configurable verbosity).
4. **Live ASCII viewport** (`--cli=viewport`) — the planet
   rendered into your terminal as it evolves, with the same
   renderer the post-run keyframes use.
5. **Markdown post-run report** (via `ages-report`) — a
   structured biography with planet card (incl. ASCII map),
   species card, the species' Ages timeline, run summary,
   memorable figures, species canon, per-civ chapters
   (including discovered templates and invented tools),
   transmission/diffusion/conflict tables, the spatial-timeline
   keyframes (paired ownership + density maps), the population
   timeline, war campaigns, and the highlight reel of structural
   pins.

A representative seed-42 / 5000-year run produces ~14 civs,
7000+ confirmed relations, 1800+ knowledge transmissions across
collapse boundaries, 4000+ conflict skirmishes grouped into war
campaigns, ~1000 lines of report.

## Spatial timeline (paired keyframes)

Every ~6 keyframes across the run, the report renders **both**
an ownership map and a density map of the same moment:

- **Ownership** — `A`/`B`/`C` capital letters mark each civ's
  centroid; cells claimed by exactly one civ render as that
  civ's digit (`1`–`9`, `*` for civs ≥10); disputed cells render
  as `#`; nomadic-only cells render as `0`; unclaimed cells show
  terrain.
- **Density** — same capital letters; claimed cells render as
  Unicode block-shading (` ░ ▒ ▓ █`) keyed to per-cell
  population relative to the densest cell in the frame. Capitals
  pop as `█`; frontiers fade to `░`.

Together: who owns what, and where the people actually live.
The live viewport uses the ownership renderer; reports show
both.

## Key concepts

A condensed summary lives in
[`docs/PROJECT.md`](docs/PROJECT.md) — vision principles tabulated
against the shipped features that serve each. For mechanics
depth on a single topic, jump straight to the per-feature docs
(linked in [Where to read deeper](#where-to-read-deeper) below).

The headline ideas:

- **1 tick = 1 month**, year length per-planet (8–16 months
  sampled per seed). Substrate-first sampling produces life of
  some chemistry on every seed (Aqueous, Ammoniacal, Hydrocarbon,
  Silicate). The species is the run's persistent unit; civs are
  bounded episodes within it.
- Recognition templates turn emergent physics into named
  phenomena. Species sensoria gate which templates are
  perceivable. The form vocabulary a civ can fit follows the
  structural tags its perceivable templates carry — civs that
  never see a `Periodic` template literally cannot propose
  sinusoidal forms. **Different worlds, different sciences.**
- Cosmology (5 axes, slow-drift, species-anchored) and religion
  (3 axes, fast-divergent, civ-keyed) are separate layers.
  Religion-weighted kinship gates belligerence-driven war.
- 59 tools across 5 tiers, including the `ExperimentApparatus`
  tool that gates *controlled-conditions intervention* alongside
  passive observation — a tactile-only ToolExtension-bearing
  species can derive heat-diffusion `α` from a clean
  clamp-and-relax experiment; a no-tool species stays
  observation-only.
- Inheritance has three comprehension bands: full transfer,
  mythologization (the relation content drops but cosmology
  shifts along an aligned axis), and silence. A society that
  lost the original physics retains the cultural shadow.
- Run-end is species-level: `species_extinction`, `stagnation`,
  `transcendence`, `fixed_horizon`. Per-civ collapse (a separate
  enum) covers `food_crisis`, `civil_war`, `conflict`,
  `catastrophe`, `cultural_lock`, `territory_too_small`.
- **Determinism is a contract.** Same `(seed, grid)` pair =
  byte-for-byte identical NDJSON, regardless of `--cli` mode or
  `--tick-rate-ms`. Every numeric value flows through
  `sim_arith::Real` (Q32.32 fixed-point). The viewport,
  throttle, and live CLI all sit *outside* the simulation's
  compute path.

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
├── world/                planet sampling (substrate-first);
│   ├── types.rs          categorical enums (Composition,
│   │                     Atmosphere, BiosphereClass,
│   │                     Magnetosphere, Crust, MetabolicSubstrate)
│   ├── composition.rs    continuous AtmosphericComposition,
│   │                     CrustalComposition, Moon
│   ├── planet.rs         Planet record (incl. is_tidally_locked,
│   │                     biosphere_density, substrate_perturbation)
│   ├── climate.rs        seasonal_temperature_offset / capacity
│   ├── habitability.rs   per-glyph habitability multiplier table
│   ├── sampling.rs       sample_planet (substrate-first weighted)
│   └── init.rs           per-cell terrain + water + biosphere
│                         + atmosphere + magnetosphere painter
├── recognition/          phenomenon templates (39 authored;
│                         emergent templates extend at runtime)
├── species/              species derivation; sensorium gating
├── civ/                  civ lifecycle: founding, collapse,
│                         succession, tech tiers 1–5, transcendence,
│                         culture wiring, 5-kind catastrophes,
│                         firing-relation hypothesizer +
│                         continuous measurement track
│   ├── apparatus.rs      civ-built experiment apparatus
│   ├── catastrophe/      factors / cells / triggers / orchestrator
│   ├── discovery/        channels, types, events, hypothesizer
│   ├── tech/             effects, gating, identity (58 tools), specs
│   ├── cohesion + civil-war path
│   ├── breakaway path
│   ├── transmission + mythologization
│   └── rivals storage + displacement API
├── core/                 tick loop, phase ordering, run orchestration
├── events/               emitter + filter + counting + viewport
│                         tee + nomads tracker
├── report/               post-run markdown report (with keyframes);
│                         live ASCII viewport;
│                         labels module (single source of truth)
└── population/           cohorts; substrate-derived demographics
protocol/                 wire schema, split by event domain
docs/                     cross-crate + per-feature docs
runs/                     archived NDJSON event logs (gitignored)
```

## Where to read deeper

- [`AGENTS.md`](AGENTS.md) — operational rules + routing for
  AI contributors
- [`PLANNING.md`](PLANNING.md) — current state, last change,
  resumption anchor for mid-task work
- [`docs/PROJECT.md`](docs/PROJECT.md) — vision principles
  tabulated against shipped features; the "where does the
  project stand vs its ambition?" view
- [`docs/architecture.md`](docs/architecture.md) — process
  layout, event/snapshot model, phase order, post-run report
- [`docs/world.md`](docs/world.md) — planet model: substrate,
  atmosphere, terrain, hydrology, magnetism
- [`docs/physics.md`](docs/physics.md) — laws, grid, time-
  stepping, operator splitting
- [`docs/recognition.md`](docs/recognition.md) — templates,
  signatures, fields, perceivable vs latent gating
- [`docs/discovery.md`](docs/discovery.md) — form vocabulary,
  fits, theory hierarchy, rivals, mythologization, experiments
- [`docs/species.md`](docs/species.md) — derivation, sensorium,
  per-civ drift
- [`docs/civ.md`](docs/civ.md) — civ lifecycle, founding,
  succession, cohesion, breakaway
- [`docs/culture.md`](docs/culture.md) — cosmology, religion,
  kinship, conflict, war
- [`docs/tech.md`](docs/tech.md) — 59-tool tree, prereqs,
  effects, serendipity, emergent + apparatus
- [`docs/population.md`](docs/population.md) — cohorts,
  demographics, migration, nomads, habitat diffusion
- [`docs/catastrophes.md`](docs/catastrophes.md) — 5 kinds,
  severity, cells
- [`docs/viewport.md`](docs/viewport.md) — live CLI viewport
  layout and rendering
- [`docs/report.md`](docs/report.md) — post-run markdown report
  + `narrate.py`
- [`docs/MANIFEST.md`](docs/MANIFEST.md) — doc index with line
  counts
- [`docs/decisions/INDEX.md`](docs/decisions/INDEX.md) —
  decision-record archive (historical rationale; current
  behavior lives in the per-feature docs above)
- Per-crate `sim/<crate>/README.md` — implementation details

## License

MIT or Apache-2.0 (see workspace `Cargo.toml`).
