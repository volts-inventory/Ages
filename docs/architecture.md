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
[../PLANNING.md](../PLANNING.md). For doc routing see
[MANIFEST.md](MANIFEST.md).

## Process layout

```
Rust sim (headless)
   │
   ├── live CLI event stream  ──>  stdout (during the run)
   └── ndjson event log       ──>  events.ndjson (during the run,
                                    includes periodic Snapshot
                                    digest events)
                               │
                               └── post-run report generator
                                   (walks event log, produces
                                    markdown report after run)
```

There is **no live UI** and **no LLM** anywhere in this project. The
sim runs headless and emits structured output. User experience is
the live CLI stream during the run plus the markdown report after.

- **Sim** — Rust workspace. Pure compute. No rendering, audio, UI.
- **Protocol** — JSON schemas in `protocol/`. Single source of truth
  for the sim↔consumer contract. Rust types via `serde`. See
  `protocol/README.md`.

## Crate map

| Crate | Responsibility |
|-------|----------------|
| `sim/arith` | Q32.32 fixed-point `Real` + transcendentals. The only path for real arithmetic in the project. |
| `sim/world` | Planet sampling (substrate-first), terrain init, climate, habitability table. |
| `sim/physics` | Heat / fluid / hydrology / magnetism / Lorentz / Coriolis / tides / radiation / vertical convection laws on a hex grid. |
| `sim/recognition` | Phenomenon templates and signature matching against physics state. |
| `sim/species` | Species derivation from planet + recognition library; sensorium gating. |
| `sim/population` | Cohort dynamics, substrate-derived demographics, migration. |
| `sim/civ` | Civ lifecycle: founding, collapse, succession, breakaway, tech, conflict, hypothesizer, apparatus, religion, cosmology, catastrophes. |
| `sim/events` | NDJSON emitter, filter, viewport tee, nomads tracker. |
| `sim/report` | Post-run markdown report; live ASCII viewport; label tables. |
| `sim/core` | Tick loop, phase walking, run orchestration, `build_laws`. |
| `protocol` | Wire schema (header / world / civ / discovery / snapshot). |
| `ages` | Run binary + `ages-report` binary. |

## Live CLI event stream

Every event written to NDJSON can also be tee'd to stdout as it
happens. Three verbosity levels are shipped:

- `--cli=quiet` — nothing to stdout (only NDJSON to file).
- `--cli=highlights` — only structural pins (run-start, planet,
  species, civ founding/collapse, catastrophe, tech unlocks, civ
  contact, knowledge transmission, conflict, run-end). Long runs
  become tail-able without flooding the terminal.
- `--cli=all` — every event tee'd to stdout. The default. Useful
  for short runs and for piping into ad-hoc consumer scripts.
- `--cli=viewport` — alternate-screen ASCII viewport, sharing the
  post-run report's frame renderer. See [viewport.md](viewport.md).

Streaming is deterministic: same seed, same stream. Stdout output is
downstream of the same iteration order that drives the NDJSON, so
byte-for-byte equality across runs holds.

## Event vs snapshot model

- **Events** are the canonical history. Append-only NDJSON. Every
  meaningful state change emits an event — physics-tier ticks emit
  aggregated events (not per-cell), civ-tier emits per-discovery /
  per-promotion / per-population-shift / etc. The post-run report
  consumes the event log directly; no other input is needed.
- **`Snapshot` events** are state-digest checkpoints emitted into
  the same NDJSON stream every `sim_core::SNAPSHOT_INTERVAL_TICKS`
  (currently 500 baseline-years × 12 = 6 000 ticks). Each carries
  the tick, active and collapsed
  civ ids, total species population, and running totals (confirmed
  relations, refinements, catastrophes, tech unlocks, knowledge
  transmissions, knowledge diffusions). Cheap and useful as
  cross-check anchors for offline tools; not a full-state checkpoint.
- A future replay-snapshot payload (per-cell physics state, full
  per-figure hypothesizers) would warrant a separate event variant
  and a clear consumer; deferred until that consumer materialises.

## Per-tick phase order

Events emitted within a civ-sim tick are ordered by phase, then by
sorted entity IDs within the phase. The sim walks phases in this
fixed order:

1. `TickStart` — epoch marker.
2. `PhysicsIntegration` — physics engine takes its sub-steps;
   aggregated physics events emitted. Apparatus cells write their
   clamp values into the physics state at the start of this phase,
   before the prev-state snapshot — so subsequent `TemporalDelta`
   measurements read the relaxation response.
3. `PatternRecognition` — recognized-phenomenon updates from physics
   state changes.
4. `CohortObservations` — discovery phase A.
5. `FigureObservations` — discovery phase B. At the end of this
   phase, each civ's `apparatus_cells` are sampled post-physics and
   the `(clamp, response)` pair is fed into the first active
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

The phase enum lives in `protocol/`. Sub-phase ordinals can
be added later without renumbering top-level phases.

## Determinism contract

- Single seeded `ChaCha20Rng` threaded through the sim. No
  `thread_rng()`.
- Seed printed in the run header and stored in snapshots.
- No `HashMap` iteration in decision paths — `BTreeMap` or sorted
  iteration.
- All physics + fitting arithmetic via `sim/arith` (Q32.32
  fixed-point default). **No direct `f64` outside that crate** —
  enforced by lint / test discipline.
- Single-threaded physics integration unless we have a strategy for
  parallel-deterministic reduction.

### Validation

- **Determinism test**: same seed run twice, full event log compared
  byte-for-byte. Snapshots also compared at matching ticks.
- **Performance test**: criterion benchmark of physics-tick
  throughput and civ-tick throughput on a reference run; CI enforces
  a regression budget.
- **Divergence test**: 10 runs with planets sampled from distinct
  seeds — assert each produces a distinct recognized-phenomenon set
  and a distinct civ discovery profile.

## Tick semantics

- **One civ-sim tick = one month.** Year length is per-planet
  (`orbital_period_months` sampled in `[8, 16]`) and drives the
  seasonal-template modulo and the Y/M display in every output.
  `--years` is a CLI convenience that multiplies by the planet's
  `orbital_period_months`, so a 16-month world runs 16 ticks per
  `--years 1`. `--ticks` is still accepted for low-level callers.
- Underneath, the physics engine takes many smaller dt sub-steps
  per civ-sim tick.
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
