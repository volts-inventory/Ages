# Ages — Post-CA1-CA8 Re-Audit

Auditor: senior Rust architect, 2026-05-23. Scope: deltas vs `docs/code-audit.md`.

## 1. Verdict

**RESIDUAL_ITEMS.** All eight CA1-CA8 items in the cleanup wave landed cleanly and meet (or beat) their stated targets. The four largest god-files were either decomposed into per-concern siblings or shrunk to thin façades, the test mega-module is now seven topical files, the Cargo inconsistency is fixed, and the four civ `pub mod` overshares are downgraded. The repository's *structural* hotspots are resolved. What remains is the deferred backlog the original audit explicitly flagged as "this sprint" / "polish" — physics sub-folder reorg, test-support crate, the still-1500+-line physics & population files, and the worktree leak — none of which were in CA1-CA8 scope.

## 2. CA1-CA8 closure table

| ID | Item | Fixed? | Evidence |
|---|---|---|---|
| CA1 | `sim/core/src/lib.rs` <400 lines, `run()` extracted | YES | `sim/core/src/lib.rs:1-71` (70 lines total). `run()` body at `:48-67` delegates to `setup::setup_run` + `run_tick::run_tick`. New siblings `run_tick.rs` (1695), `setup.rs` (392), `constants.rs`. |
| CA2 | `sim/ecosystem/src/lib.rs` <300 lines, god-module split | YES | `sim/ecosystem/src/lib.rs:1-104` (104 lines, pure re-export façade). Concerns split into `constants.rs`, `functional.rs`, `species.rs`, `planet.rs` (1417), `sampling.rs`, `invariants.rs`. |
| CA3 | `sim/physics/src/tectonics.rs` split | YES | Now a folder: `sim/physics/src/tectonics/{mod.rs, plates.rs, slab_pull.rs, subduction.rs, erosion.rs}`. `mod.rs:10-20` documents the layout. (mod.rs is 1347 — see §3.) |
| CA4 | `sim/civ/src/conflict.rs` split | YES | Now a folder: `sim/civ/src/conflict/{mod.rs (38), war.rs (580), alliance.rs, grudge.rs, assessment.rs (556)}`. `mod.rs:17-38` is a clean façade with the pre-split public surface preserved. |
| CA5 | `sim/civ/src/catastrophe/mod.rs` shrunk to ~50-line façade | YES | `sim/civ/src/catastrophe/mod.rs:1-56` (55 lines). Implementation distributed across `apply.rs` (1263), `cells.rs`, `damage.rs`, `factors.rs`, `kind.rs`, `record.rs`, `triggers.rs`. |
| CA6 | `sim/ecosystem/src/tests.rs` split | YES | Now `sim/ecosystem/src/tests/{mod.rs, pyramid.rs (844), dynamics.rs, biogeochem.rs, tolerance.rs, biomass.rs, parasitism.rs, integration.rs}`. `tests/mod.rs:20-46` documents the axis split. |
| CA7 | `sim/report/Cargo.toml` workspace-form `sim-events` | YES | `sim/report/Cargo.toml:17` reads `sim-events.workspace = true`. Path form is gone. |
| CA8 | `pub mod` → `pub(crate) mod` for civ internals | YES | `sim/civ/src/lib.rs:27` (`pub(crate) mod culture_hooks`), `:34` (`pub(crate) mod religion`), `:41` (`pub(crate) mod economy`), `:42` (`pub(crate) mod environmental_drift`). |

## 3. New structural issues surfaced

- **`sim/physics/src/tectonics/mod.rs:1` is still 1347 lines** after CA3. The split moved `plates`/`slab_pull`/`subduction`/`erosion` out, but the `Tectonics` struct + `Law::integrate` orchestrator + boundary-uplift math still bulk the façade. Worth a follow-up to extract the orchestrator into `integrate.rs`.
- **`sim/civ/src/catastrophe/apply.rs:1` is 1263 lines** — CA5 moved the bulk from `mod.rs` to `apply.rs`, but `check_and_apply` itself is now the new offender. Per-kind applicators (volcanic/disease/asteroid/solar-flare/ice-age) could move into `apply/` siblings.
- **`sim/core/src/run_tick.rs:1` is 1695 lines** — CA1 successfully shrank `lib.rs`, but the per-tick body is now a single long function in a new file. Consistent with the audit's "shrink to <600" recommendation being only partly met.
- **`sim/ecosystem/src/planet.rs:1` is 1417 lines** — CA2 isolated `PlanetEcosystem` cleanly, but `step` and its helpers are large enough to warrant a future `planet/{state.rs, step.rs}` split.

## 4. Remaining backlog (untouched by CA1-CA8)

Items the original audit recommended but that fell outside the CA1-CA8 scope:

1. **`sim-test-support` dev-dep crate** — not created (`sim/test-support/` absent). Fixture duplication across `tests.rs` files persists. Largest single test-code-reduction lever.
2. **`sim/physics/src/` flat-namespace regroup** — still 27 sibling `.rs` files; the proposed `atmosphere/ surface/ radiation/ field/ core/` sub-modules were not created.
3. **`sim/physics/src/atmospheric_escape.rs` (1819), `radiation.rs` (1796), `tidal_heating.rs` (1699)** — still untouched length offenders.
4. **`sim/report/src/viewport/emitter.rs` (1723)**, **`sim/core/src/nomads.rs` (1667)**, **`sim/population/src/lib.rs` (1517)**, **`sim/core/src/tests.rs` (2322)** — all still over the 1500-line threshold.
5. **`sim/civ/src/conflict/` pub-const tuning surface** — CA4 split the file but the 30+ `GRUDGE_*` / `HIER_W_*` / `DRIVE_W_*` consts remain `pub` (now re-exported from the façade). Audit recommended `pub(crate)` or a `tuning` sub-module.
6. **`.claude/worktrees/` leak** — 40+ stale `agent-*` worktrees still present (28× 2636-line `ecosystem/src/tests.rs` copies confirm pre-CA6 trees are still on disk). `git worktree prune` + manual rm still needed.
7. **`sim/ecosystem/tests/speciation.rs` (1200)** vs `src/tests/` reconciliation — integration vs unit overlap not addressed.
8. **`sim/civ/src/lib.rs:529-645` wall of `pub const`s** — dead-code candidate from the original audit; not pruned.
