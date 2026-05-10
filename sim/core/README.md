# sim/core

Top-level run orchestration: seeded RNG, tick loop, phase walking,
event emission. The whole sim threads a single `Rng` through every
decision path.

## Status

- **M0–M2 shipped**: deterministic ChaCha20 RNG, tick loop walks the
  canonical phase order, NDJSON event emission via `sim/events`,
  byte-for-byte determinism verified in tests.
- **M3 pending**: hooking up named figures, discovery pipeline, civ
  founding triggers.

## Run config

`RunConfig`: seed, max_ticks, grid dimensions, `OrchestrationConfig`
(physics sub-step ratios + dt per family). `RunConfig::dev(seed,
max_ticks)` gives sensible small-scale defaults.

## Tick loop

Each tick walks `PHASE_ORDER` (the canonical 14-phase sequence).
For each phase:

1. Emit a `Tick { tick, phase }` event marking the phase boundary.
2. Run that phase's work (physics integrate, recognition scan, civ
   observe, etc.).

Within each phase, iterate active civs by sorted `civ_id`, then
regions by sorted `region_id`, then figures by sorted `figure_id`,
then recognized-phenomenon ids by sorted id. Combined with the
no-`HashMap`-in-decision-paths rule, byte-for-byte determinism falls
out for free.

## `build_laws(&Planet)`

Constructs all four physics laws (mechanics+gravity, fluid, heat,
EM, chemistry) with coefficients derived from the sampled planet.
Different seeds → different planets → different coefficients →
different physics outcomes. Structural shapes fixed, parametric
values vary.

## Determinism contract

- Single seeded `ChaCha20Rng` threaded through the sim. **No
  `thread_rng()`.**
- Seed printed in run header and stored in snapshots.
- No `HashMap` iteration in decision paths — `BTreeMap` or sorted
  iteration.
- All physics arithmetic via `sim/arith`. No direct `f64`
  outside that crate.
- Same-seed → byte-for-byte identical event log. Verified in
  `deterministic_event_log_for_same_seed` test.

## Cited by

[docs/architecture.md](../../docs/architecture.md),
[docs/physics.md](../../docs/physics.md) (operator splitting,
per-planet law coefficients).
