# Ages — Round 3 Structural Re-Audit

Auditor: senior Rust architect, 2026-05-23. Scope: deltas vs `docs/code-reaudit.md` (CB1-CB7 closure).

## 1. Verdict

**RESIDUAL.** Every file in `sim/` is now under 2400 lines, but the >1500-line threshold the prior two audits chose as the structural cliff is **not** met: seven files still sit above it. CB1-CB7 clearly landed (the top file shrank from 2636 → 2322, the `lib.rs` / `tectonics.rs` / `catastrophe` / `conflict` god-modules are decomposed), but the next tier — `atmospheric_escape`, `radiation`, `viewport/emitter`, `tidal_heating`, `run_tick`, `nomads`, `population/lib` — is unchanged at the line-count level. None of these are ship-blocking; they are all single-concern files where length reflects domain density rather than tangled responsibilities. Treat this round as confirming the round-2 backlog is real and unworked, not as a regression.

## 2. Top-20 length table

| Rank | File | Lines |
|---:|---|---:|
| 1 | `sim/core/src/tests.rs` | 2322 |
| 2 | `sim/physics/src/atmospheric_escape.rs` | 1819 |
| 3 | `sim/physics/src/radiation.rs` | 1796 |
| 4 | `sim/report/src/viewport/emitter.rs` | 1723 |
| 5 | `sim/physics/src/tidal_heating.rs` | 1699 |
| 6 | `sim/core/src/run_tick.rs` | 1695 |
| 7 | `sim/core/src/nomads.rs` | 1667 |
| 8 | `sim/population/src/lib.rs` | 1517 |
| 9 | `sim/ecosystem/src/planet.rs` | 1417 |
| 10 | `sim/physics/src/tectonics/mod.rs` | 1347 |
| 11 | `sim/civ/src/discovery/hypothesizer.rs` | 1303 |
| 12 | `sim/core/src/phases.rs` | 1280 |
| 13 | `sim/civ/src/catastrophe/apply.rs` | 1263 |
| 14 | `sim/physics/src/hadley.rs` | 1234 |
| 15 | `sim/civ/src/tests.rs` | 1219 |
| 16 | `sim/species/src/sampling.rs` | 1210 |
| 17 | `sim/ecosystem/tests/speciation.rs` | 1200 |
| 18 | `sim/world/src/tests.rs` | 1189 |
| 19 | `sim/civ/src/tech/specs/manipulation.rs` | 1150 |
| 20 | (tie/below) | <1150 |

## 3. Genuinely-actionable items (>1500 lines)

| ID | File | Lines | Action |
|---|---|---:|---|
| CC1 | `sim/core/src/tests.rs` | 2322 | Mirror the production split — `run_tick_tests.rs`, `nomads_tests.rs`, `phases_tests.rs`. Round-1 audit flagged this exact file at 2318; net delta is +4 lines, so it has not been touched. |
| CC2 | `sim/physics/src/atmospheric_escape.rs` | 1819 | Split per loss channel: `jeans.rs`, `ion.rs`, `hydrodynamic.rs`, `params.rs`. |
| CC3 | `sim/physics/src/radiation.rs` | 1796 | Split by `LockingMode` arm + stellar-input helpers. |
| CC4 | `sim/report/src/viewport/emitter.rs` | 1723 | One struct, but glyph/colour/animation tables and the per-tile emit loop are independently legible — extract sibling modules. |
| CC5 | `sim/physics/src/tidal_heating.rs` | 1699 | Already re-exports six names; per-export sibling split is the obvious cut. |
| CC6 | `sim/core/src/run_tick.rs` | 1695 | New file from CB1; the per-tick body is still one long function. Recommendation from round 2 stands: extract phase-group helpers (`run_tick/{physics.rs, ecosystem.rs, civ.rs}`) and keep `run_tick.rs` as the orchestrator. |
| CC7 | `sim/core/src/nomads.rs` | 1667 | 8 `pub(crate) fn`s spanning init / growth / emergence / absorption / observation — split lifecycle stages into siblings. |
| CC8 | `sim/population/src/lib.rs` | 1517 | Sole file in the crate. Split tests + cohort step out; `lib.rs` should be the façade. |

No genuine structural breakage detected — no circular deps, no fresh god-modules, no visibility regressions, no `Cargo.toml` drift. The remaining offenders are tractable length-only items.

## 4. Deferred (known)

These remain explicitly out-of-scope per prior rounds; noted, not proposed for this round:

- **`sim-test-support` dev-dep crate** — fixture duplication across `*/src/tests.rs` persists; the largest test-LOC-reduction lever, still uncreated.
- **`sim/physics/src/` flat-namespace reorg** — 27 sibling files; proposed `atmosphere/ surface/ radiation/ field/ core/` grouping not yet attempted.
- **`.claude/worktrees/` pruning** — stale `agent-*` worktrees still on disk; `git worktree prune` + manual rm still owed.

Adjacent items not in the deferred list but worth flagging without action: `sim/ecosystem/tests/speciation.rs` (1200) vs `sim/ecosystem/src/tests/` overlap, and the `sim/civ/src/conflict/` `pub const` tuning surface (round 2 item, still `pub`-re-exported from the façade).
