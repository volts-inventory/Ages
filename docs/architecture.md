# Architecture (cross-crate)

How the simulation is built at the seams between crates. Per-crate
mechanics live in each `sim/<crate>/README.md`; per-feature behavior
lives in the topic docs alongside this one
([world.md](world.md), [physics.md](physics.md),
[recognition.md](recognition.md), [discovery.md](discovery.md),
[species.md](species.md), [civ.md](civ.md),
[culture.md](culture.md), [tech.md](tech.md),
[population.md](population.md), [catastrophes.md](catastrophes.md),
[viewport.md](viewport.md), [report.md](report.md)).

For project-wide vision and current state see
[PROJECT.md](PROJECT.md). For doc routing see
[MANIFEST.md](MANIFEST.md). For day-to-day status see
[../PLANNING.md](../PLANNING.md).

## Process layout

```
Rust sim (headless)
   │
   ├── live CLI event stream  ──>  stdout (during the run)
   ├── live narration stream  ──>  stdout (during the run,
   │                                NarratingEmitter, optional)
   └── ndjson event log       ──>  events.ndjson (during the run,
                                    includes periodic Snapshot
                                    digest events)
                               │
                               └── post-run report generator
                                   (walks event log, produces
                                    markdown report after run)
                               │
                               └── offline narration replay
                                   (replay_narration reads
                                    events.ndjson, prints sentences)
```

There is **no live UI** and **no LLM** anywhere in this project. The
sim runs headless and emits structured output. User experience is
the live CLI stream during the run, plus the optional
single-sentence narration, plus the markdown report after.

- **Sim** — Rust workspace. Pure compute. No rendering, audio, UI.
- **Protocol** — JSON schemas in `protocol/`. Single source of truth
  for the sim↔consumer contract. Rust types via `serde`. See
  `protocol/README.md`.

## Workspace crate map

The workspace has 13 members (see top-level `Cargo.toml`):

| Crate | Responsibility |
|-------|----------------|
| `sim/arith` | Q32.32 `Real` + Q64.32 `Pop` fixed-point types and transcendentals. The only path for real arithmetic in the project. `Real::percent(n)` is the canonical percentage shorthand. |
| `sim/world` | Planet sampling (substrate-first), terrain init, climate, habitability table, hemisphere split, tidal locking. |
| `sim/physics` | Heat / fluid / hydrology / magnetism / Lorentz / Coriolis / tides / radiation / vertical convection / chemistry / tectonics / tidal heating / atmospheric escape on a hex grid. Operator-splitting orchestrator. |
| `sim/recognition` | Phenomenon templates and signature matching against physics state. |
| `sim/species` | Species derivation from planet + recognition library; sensorium gating; habitat-glyph mapping. |
| `sim/ecosystem` | Functional-group ecosystem (planet/step + biogeochem + extinction + centrality + catastrophe), HGT, speciation. |
| `sim/population` | Cohort dynamics, substrate-derived demographics, lifecycle, migration. |
| `sim/civ` | Civ lifecycle: founding, collapse, succession, breakaway, tech, conflict, hypothesizer, apparatus, religion, cosmology, catastrophes. |
| `sim/events` | `Emitter` trait + NDJSON / tee / filter / throttle adapters. |
| `sim/report` | Post-run markdown report; live ASCII viewport; narration (streaming + replay); label tables; digest. |
| `sim/core` | Tick loop, phase walking, run orchestration, `build_laws`, nomads, `tick_steps/` per-phase helpers. |
| `protocol` | Wire schema (header / world / civ / discovery / snapshot). |
| `ages` | Run binary + `ages-report` binary. |

## Dependency hierarchy

Edges point from consumer to producer. There are **no cycles**.

```
ages
  └── sim_core ── sim_report ── sim_events ── protocol ── sim_arith
                     │   │   │
                     │   │   └── sim_civ ─┬── sim_population ─┐
                     │   │                ├── sim_species ────┤
                     │   │                ├── sim_recognition ┤
                     │   │                └── sim_ecosystem ──┤
                     │   └── sim_physics ────────────────────┤
                     └── sim_world ──────────────────────────┘
                                                              │
                                            sim_arith ◄───────┘
```

Reading rules:

- `sim_arith` is leaf — it depends on nothing else in the
  workspace.
- `protocol` depends only on `sim_arith` (for the few schema
  fields carrying fixed-point types).
- `sim_events` depends on `protocol` only — the emitter contract
  doesn't reach into any of the domain crates.
- Domain crates (`sim_world`, `sim_physics`, `sim_recognition`,
  `sim_species`, `sim_ecosystem`, `sim_population`, `sim_civ`)
  depend on `sim_arith` + `protocol` + each other in the
  layering above; they emit events through `sim_events`.
- `sim_core` is the orchestrator: it depends on every domain
  crate plus `sim_events` and walks them through the tick loop.
- `sim_report` is a pure consumer of the NDJSON the rest of the
  workspace produces; it depends on `sim_events` for the
  `Emitter` trait (`NarratingEmitter`'s inner-emitter contract)
  and on `protocol` + `sim_arith` for typed parsing.
- `ages` is the thin CLI glue around `sim_core` + `sim_report`.

The acyclic structure is enforced by Cargo at build time; any
attempt to introduce a back-edge fails compilation.

## Post-cleanup module structure

A round of organisational cleanup (Sprint 5 closeout) folded most
single-file 1k+-line modules into folders of mutually-cohesive
siblings. Public APIs were preserved by re-exporting from `mod.rs`,
so consumer crates didn't change. The folder shapes worth knowing:

### `sim/physics/src/`

- `tectonics/` — plates, slab pull, subduction, erosion,
  orchestrator, state, tests. Split from a single `tectonics.rs`.
- `radiation/` — equilibrium, greenhouse, locking, tests. Split
  from `radiation.rs`.
- `atmospheric_escape/` — Jeans, hydrodynamic, ion, photochemical,
  params. Split from `atmospheric_escape.rs`; each escape channel
  lives in its own sibling.
- `tidal_heating/` — formula, distribution, conduction, damping.
  Split from `tidal_heating.rs`.
- `chemistry/` — substance/substrate/reactions/redox/constants.

### `sim/civ/src/`

- `conflict/` — alliance, assessment, grudge, war. Split from a
  single `conflict.rs`.
- `catastrophe/` — apply (per-kind subfolder), cells, damage,
  factors, kind, record, triggers. Split from `catastrophe.rs`
  with a further split of `apply.rs` into per-kind siblings.
- `discovery/` — channels, emergence, events, hypothesizer,
  tool_emergence, types.
- `tech/` — consumption, effects, gating, identity, specs/.

### `sim/core/src/`

- `tick_steps/` — `breakaway_step`, `catastrophe_step`,
  `civ_lifecycle_step`, `contact_and_trade_step`,
  `emergent_founding_step`, `knowledge_diffusion_step`,
  `stateless_refound_step`, `war_and_alliance_step`. Each
  per-phase helper extracted out of `run_tick.rs`; the top-level
  `run_tick()` orchestrates them. Mechanical split, no
  behavioural change.
- `nomads/` — absorption, emergence, growth, init, observation,
  tests. Split from `nomads.rs` by phase.
- `lib.rs` itself split into `run_tick.rs` + `setup.rs` +
  `constants.rs`.

### `sim/world/src/`

- `planet.rs` carries `Planet` together with sibling files for
  star, climate, composition, sampling, init, habitability,
  hemisphere, tidal_locking. (The previous monolithic
  `lib.rs` was split out.)

### `sim/report/src/`

- `viewport/` — ansi, cards, config, emitter, layout, log,
  sidebar, state, tests. Split from `viewport/emitter.rs`; the
  per-civ sidebar state folded ten parallel `BTreeMap` fields
  into one `CivState` struct.
- `render/` — civ, figures, planet, timelines.
- `digest/` — build, types, tests.
- `narration.rs` — `NarratingEmitter` + `replay_narration` (both
  paths share the per-event `narrate_event` core).

### `sim/ecosystem/src/`

- `planet/` — step, biogeochem, extinction, catastrophe,
  centrality. Split from `planet.rs`.
- `tests/` — per-concern test modules. Split from `tests.rs`.

This is the post-cleanup layout. When adding new code, follow the
folder grain: a new tectonic-state field goes in
`physics/tectonics/state.rs`, a new escape channel goes in a new
`physics/atmospheric_escape/<name>.rs`, a new per-phase helper
goes in `core/tick_steps/<name>.rs`.

## Operator-splitting orchestration

Physics integration inside one civ-sim tick uses **Lie operator
splitting**, sequential and deterministic. Each civ-sim tick is one
sim-month; the orchestrator walks `macro_steps_per_step`
macro-steps per tick (one per sim-day by default, ~30 per tick).
Within each macro-step every law family integrates at its own
sub-step ratio:

- **Fluid** — finest sub-step (CFL-bound).
- **Heat** — coarser sub-step; reads post-fluid velocity for
  advection.
- **Chemistry / EM** — coarsest sub-step; chemistry sees post-heat
  state, EM sees post-fluid charge.

Coupling order is fixed (fluid → heat → chemistry → EM); no
state-dependent branching. Tectonics, tidal heating, radiation,
and atmospheric escape are coarse-grained law families called by
the orchestrator at their own intervals.

The orchestrator lives in `sim/physics/src/orchestration.rs`. In
debug builds it wraps each kernel in conservation asserts:

- **Per-kernel `debug_assert!`** — snapshot the relevant
  conservation total (substance mass, water depth + vapour, etc.)
  before each call; assert `< 1e-6` drift after. Catches a
  regression on the first tick.
- **Cumulative-drift accumulator** — sums each tick's signed
  delta into a running total across the run; asserts the
  absolute total stays under `1 / 1_000_000`. Catches slow leaks
  a few LSBs per tick that wouldn't trip the per-kernel guard.

Both are zero-cost in release builds.

## Q32.32 determinism

The determinism contract:

- Single seeded `ChaCha20Rng` threaded through the sim. No
  `thread_rng()`.
- Seed printed in the run header and stored in snapshots.
- No `HashMap` iteration in decision paths — `BTreeMap` or sorted
  iteration.
- All physics + fitting arithmetic via `sim/arith` (Q32.32
  fixed-point default). **No direct `f64` outside that crate** —
  enforced by lint / test discipline.
- Single-threaded physics integration unless we have a strategy
  for parallel-deterministic reduction.

The arithmetic types live in `sim_arith`:

- **`Real`** — Q32.32 fixed-point newtype wrapping `I32F32`.
  ±~2.1e9 range, ~2.3e-10 LSB precision. The default scalar.
- **`Pop`** — Q64.32 fixed-point newtype wrapping `I96F32`.
  ±~4e28 range, same fractional precision. Used where a count
  needs more headroom than `Real` (modern-era population in
  particular). Mixed arithmetic `Pop * Real → Pop`,
  `Pop / Pop → Real`.
- **`Real::percent(n)`** — canonical shorthand for
  `Real::from_ratio(n, 100)`; introduced during the audit pass
  and adopted across 549 call sites.

Validation:

- **Determinism test**: same seed run twice, full event log
  compared byte-for-byte. Snapshots compared at matching ticks.
- **Performance test**: criterion benchmark of physics-tick
  throughput and civ-tick throughput on a reference run; CI
  enforces a regression budget.
- **Divergence test**: 10 runs with planets sampled from distinct
  seeds — assert each produces a distinct recognized-phenomenon
  set and a distinct civ discovery profile.

## Event-emitter model

The sim writes events through the `Emitter` trait
(`sim/events/src/lib.rs`):

```rust
pub trait Emitter {
    type Error;
    fn emit(&mut self, event: &Event) -> Result<(), Self::Error>;
}
```

Implementations compose:

| Emitter | Role |
|---------|------|
| `JsonLinesEmitter<W>` | Writes one event per line as NDJSON to any `Write`. The canonical event log uses this against a `File`; tests use it against `Vec<u8>`. |
| `TeeEmitter<A, B>` | Splits emission to two emitters. Used to tee the NDJSON file to stdout for the live CLI stream. |
| `FilterEmitter<E, F>` | Forwards events matching a predicate; drops the rest. Wires `--cli=highlights` so the stdout stream carries only structural pins while the NDJSON file still gets everything. |
| `ThrottledEmitter<E>` | Rate-limits high-frequency events to avoid drowning the terminal. |
| `NarratingEmitter<E, W>` (in `sim_report`) | Wraps an inner `Emitter`; for every event, forwards to the inner emitter **and** writes a single human-readable sentence to a sink. Used by `ages --narration`. |

Run-end: the binary picks the combination based on flags. A
typical `ages --cli=highlights --narration` run uses
`NarratingEmitter::new(FilterEmitter::new(TeeEmitter::new(JsonLinesEmitter, StdoutEmitter)), stdout)`.

### Streaming narration vs replay

`sim_report` exposes two paths over the same `narrate_event` core:

1. **Streaming** — `NarratingEmitter::new(inner, sink)` wraps the
   inner emitter. As the sim runs, every event becomes one
   single-sentence narration line to the sink and the canonical
   event passes through to the inner emitter unmodified. Sink is
   typically stdout via `ages --narration`.
2. **Replay** — `replay_narration(path, sink)` reads a previously
   recorded NDJSON event log and emits the narration retroactively
   to the sink. Useful for post-hoc storytelling without re-running
   the sim. `ages --replay-narration <file>` wires this up.

Both share `NarratorState`, which tracks names seen for civs /
species / figures so later lines can render names instead of bare
ids. Year accounting reads `orbital_period_months` off the
`Planet` event so tick → year display matches the rest of the
report.

The Python `narrate.py` exists as an alternate consumer of the
same NDJSON contract; it predates the in-binary narrator and
remains for users who want richer prose templating.

## Live CLI event stream

Every event written to NDJSON can also be tee'd to stdout as it
happens. Verbosity levels:

- `--cli=quiet` — nothing to stdout (only NDJSON to file).
- `--cli=highlights` — only structural pins (run-start, planet,
  species, civ founding/collapse, catastrophe, tech unlocks, civ
  contact, knowledge transmission, conflict, run-end). Long runs
  become tail-able without flooding the terminal.
- `--cli=all` — every event tee'd to stdout. The default. Useful
  for short runs and for piping into ad-hoc consumer scripts.
- `--cli=viewport` — alternate-screen ASCII viewport, sharing the
  post-run report's frame renderer. See [viewport.md](viewport.md).

Streaming is deterministic: same seed, same stream. Stdout output
is downstream of the same iteration order that drives the NDJSON,
so byte-for-byte equality across runs holds.

## Event vs snapshot model

- **Events** are the canonical history. Append-only NDJSON. Every
  meaningful state change emits an event — physics-tier ticks emit
  aggregated events (not per-cell), civ-tier emits per-discovery /
  per-promotion / per-population-shift / etc. The post-run report
  consumes the event log directly; no other input is needed.
- **`Snapshot` events** are state-digest checkpoints emitted into
  the same NDJSON stream every `sim_core::SNAPSHOT_INTERVAL_TICKS`
  (currently 500 baseline-years × 12 = 6 000 ticks). Each carries
  the tick, active and collapsed civ ids, total species population,
  and running totals (confirmed relations, refinements,
  catastrophes, tech unlocks, knowledge transmissions, knowledge
  diffusions). Cheap and useful as cross-check anchors for offline
  tools; not a full-state checkpoint.
- A future replay-snapshot payload (per-cell physics state, full
  per-figure hypothesizers) would warrant a separate event variant
  and a clear consumer; deferred until that consumer materialises.

## Per-tick phase order

Events emitted within a civ-sim tick are ordered by phase, then by
sorted entity IDs within the phase. The sim walks phases in this
fixed order:

1. `TickStart` — epoch marker.
2. `PhysicsIntegration` — physics engine takes its operator-split
   sub-steps; aggregated physics events emitted. Apparatus cells
   write their clamp values into the physics state at the start
   of this phase, before the prev-state snapshot — so subsequent
   `TemporalDelta` measurements read the relaxation response.
3. `PatternRecognition` — recognized-phenomenon updates from
   physics state changes.
4. `CohortObservations` — discovery phase A.
5. `FigureObservations` — discovery phase B. At the end of this
   phase, each civ's `apparatus_cells` are sampled post-physics
   and the `(clamp, response)` pair is fed into the first active
   figure's hypothesizer measurement track via
   `Hypothesizer::record_experimental_measurement`.
6. `HypothesisTesting` — discovery phase D.
7. `Discovery` — discovery phase E.
8. `CapabilityEvaluation` — tech-ladder advance, latent →
   perceivable.
9. `PopulationDynamics` — births, deaths, migrations across the
   species.
10. `CivLifecycle` — civ founding, collapse, succession; artifact
    persistence; inter-civ knowledge transmission.

Within each phase, iterate active civs by sorted `civ_id`, then
regions by sorted `region_id`, then figures by sorted `figure_id`,
then recognized-phenomenon ids by sorted id. Historical (collapsed)
civs do not iterate in active-civ phases; their state is preserved
read-only for the post-run report. That iteration order *is* the
event-emission order — no write-time sort is needed.

The phase enum lives in `protocol/`. Sub-phase ordinals can be
added later without renumbering top-level phases.

Per-phase orchestration helpers live in `sim/core/src/tick_steps/`
— one file per cohesive step (`civ_lifecycle_step`,
`catastrophe_step`, `breakaway_step`, `contact_and_trade_step`,
`emergent_founding_step`, `knowledge_diffusion_step`,
`stateless_refound_step`, `war_and_alliance_step`). The top-level
`run_tick()` orchestrates them in the order above.

## Tick semantics

- **One civ-sim tick = one month.** Year length is per-planet
  (`orbital_period_months` sampled in `[8, 16]`) and drives the
  seasonal-template modulo and the Y/M display in every output.
  `--years` is a CLI convenience that multiplies by the planet's
  `orbital_period_months`, so a 16-month world runs 16 ticks per
  `--years 1`. `--ticks` is still accepted for low-level callers.
- Underneath, the physics engine takes many smaller dt sub-steps
  per civ-sim tick (operator-split across law families, ~30
  macro-steps per tick on a 30-day month).
- Per civ-sim tick: physics integrates forward; pattern recognition
  updates emergent-phenomenon state; cohort observations sample;
  named-figure observations sample; discovery pipeline runs;
  knowledge propagates; culture drifts; population dynamics resolve.
- Wall-clock rate: dominated by physics in early ticks, then
  dominated by civ-layer state growth as runs accumulate knowledge
  graphs.

## Run-end taxonomy

The sim ends with one of these reasons (`early_run_end_reason`
enum in `sim_core`):

- `species_extinction` — total cohort population fell below the
  founding floor with no active civ.
- `stagnation` — extended dark age (no active civ for
  `STAGNATION_THRESHOLD_TICKS` = 1000 baseline-years × 12 = 12 000
  ticks).
- `transcendence` — the species has sustained at-least-one-civ-
  with-all-three-tier-5-tools for `TRANSCENDENCE_SUSTAINED_TICKS`
  = 2000 baseline-years × 12 = 24 000 ticks. Gated by tier-5
  species-cumulative maturity (3000 confirmed relations) so it
  naturally lands on long substrate-aligned seeds. Tier-5 tools
  (`BioelectricResonator`, `FieldPropulsionEngine`,
  `MetamaterialLattice`) are narrative milestones rather than
  simulated capabilities — the project's vision boundary holds
  (no consciousness physics, no FTL).
- `fixed_horizon` — `cfg.max_ticks` reached without any of the
  above firing.

Per-civ collapse reasons are a separate enum and don't end the run
on their own — see [civ.md](civ.md#collapse).

## Post-run report

`sim/report` is a pure consumer of the NDJSON event log: it reads
the log produced by `ages` and emits a structured markdown report.
The `ages-report` binary wraps it for CLI use
(`ages-report --in events.ndjson --out report.md`). No snapshot
files are needed — every section is derived from the event stream.
Pipeline detail and section-by-section content live in
[report.md](report.md).

The render pipeline is split across `sim/report/src/render/`
(planet, civ, figures, timelines), with the digest summarised in
`digest/` and the live ASCII viewport in `viewport/`. All three
share the same frame renderer; the viewport simply paints the
same frame the post-run report's spatial-keyframe section uses.

## Build-time decisions

Implementation choices the build will encounter and resolve in
passing. None rise to design-question level; if one ends up being
more than trivial, capture it in the relevant per-feature doc.

- Event-log file layout: one big NDJSON per run vs rotated chunks.
- Tick boundary semantics: how the per-tick phase order is enforced
  and exposed to event consumers.
- Atomic snapshot writes: write to temp + fsync + rename.
- CLI stream formatting: per-event line format, colour conventions,
  width handling.
- Event-log compression and archival for very long runs.
- Schema versioning for events / snapshots.
- Physics aggregation policy: how per-cell physics events are
  summarised before emission so the event log doesn't drown in
  micro-updates.
