# Ages — Code Organisation Audit

Auditor: senior Rust architect, 2026-05-22. Scope: `sim/`, `ages/`, `protocol/`. Worktrees / `target/` excluded.

## Critical issues (must fix before ship-ready)

- **`sim/core/src/lib.rs:74` — `pub fn run<E>()` spans ~2300 lines (74→~2440).** The whole tick loop, run-start wiring, and a ton of inline closures live in one function in a 2519-line file. Phases were already extracted to `phases.rs`, but the orchestrator never finished migrating. This is the single largest readability/maintainability liability in the codebase.
- **`sim/ecosystem/src/lib.rs` (2354 lines) is the crate's god-module.** Top-level: 30+ pub consts, `LindemanViolation`, `EcoSpecies`, `PlanetEcosystem`, `sample_ecosystem*` (3 variants), `functional_response`, `virus_outbreak_hash`, `habitat_for_substrate`. Step logic, sampling, invariants, and constants are inlined. Already has `hgt.rs` + `speciation.rs` siblings — the rest never got split out.
- **`sim/ecosystem/src/tests.rs` (2636 lines) is the longest file in the repo** and is a single flat `#[cfg(test)] mod tests` (37 `#[test]` fns) covering pyramid invariants, LV cycles, keystone, competition, functional response, virus outbreak, character displacement. Needs splitting along the same axes as the production code.
- **Worktree leak: `/home/user/Ages/.claude/worktrees/` contains 40+ `agent-*` worktrees** (~9 GB of dup'd src trees with their own `target/`). `.gitignore` correctly excludes them, but `find` scans constantly trip on them, agent tools recurse in, and they bloat backups. Garbage-collect aggressively (e.g. `git worktree prune` + manual rm of stale ones).

## High-priority cleanup (this sprint)

### Length offenders (>1500 lines)

| File | Lines | Note |
|---|---:|---|
| `sim/ecosystem/src/tests.rs` | 2636 | split per-concern (see Critical) |
| `sim/core/src/lib.rs` | 2519 | shrink `run()` (see Critical) |
| `sim/ecosystem/src/lib.rs` | 2354 | split (see Critical) |
| `sim/core/src/tests.rs` | 2318 | mirror the production split |
| `sim/physics/src/tectonics.rs` | 2097 | split: `plates`, `slab_pull`, `subduction`, `erosion` |
| `sim/physics/src/atmospheric_escape.rs` | 1819 | split: `jeans`, `ion`, `hydrodynamic`, `params` |
| `sim/physics/src/radiation.rs` | 1796 | split by `LockingMode` arm and stellar input |
| `sim/report/src/viewport/emitter.rs` | 1723 | one struct, but glyph/colour/animation logic could move out |
| `sim/physics/src/tidal_heating.rs` | 1699 | already re-exported with 6 names — split per-export |
| `sim/core/src/nomads.rs` | 1667 | 8 `pub(crate) fn`s spanning init / growth / emergence / absorption / observation; split |
| `sim/civ/src/conflict.rs` | 1563 | bundles war (`resolve`, `WarDecision`, `assess_pair`), alliances (`propose_alliance`), and grudges (`GRUDGE_*` constants, `bump_grudge`). Three distinct concerns. |
| `sim/population/src/lib.rs` | 1517 | only file in the crate; split out tests + cohort step |
| `sim/civ/src/catastrophe/mod.rs` | 1497 | already a folder — push `CatastropheRecord`, `CatastropheKind`, and `check_and_apply` into siblings (`record.rs`, `kind.rs`, `apply.rs`) |

### Grouped-concern files needing splits

- **`sim/civ/src/conflict.rs`** → `war.rs` (resolve, strength, ConflictOutcome), `alliance.rs` (propose/dissolve, asymmetry rules), `grudge.rs` (decay + bumps), `assessment.rs` (`PairAssessment`, `WarDecision`, drive weights).
- **`sim/civ/src/catastrophe/mod.rs:32` (`CatastropheKind`) + `:72` (`CatastropheRecord`) + `:289` (`check_and_apply`)** — pull each into its own file; keep `mod.rs` as a thin facade.
- **`sim/ecosystem/src/lib.rs`** → `species.rs` (`EcoSpecies`), `planet.rs` (`PlanetEcosystem` + step), `sampling.rs` (3 `sample_ecosystem*` fns), `constants.rs` (the wall of 30+ tuned ratios), `invariants.rs` (`LindemanViolation`, `check_lindeman_invariant`).
- **`sim/core/src/lib.rs`** — extract per-tick body chunks from `run()` into `phases.rs` (already exists, partially populated). Move the 4 `pub const` and `lifecycle_for_role` into `config.rs` or a new `constants.rs`. Goal: `lib.rs` < 400 lines.

### Naming inconsistencies

- **Inline `mod tests` vs sibling `tests.rs` vs `tests/` dir:** all three styles are used. Sibling `tests.rs` (declared via `#[cfg(test)] mod tests;`) is dominant (`world`, `species`, `ecosystem`, `core`, `civ`, `recognition`, plus nested `viewport/tests.rs`, `digest/tests.rs`, `tech/tests.rs`, `discovery/tests.rs`). Integration tests in `sim/core/tests/find_seed.rs` and `sim/ecosystem/tests/speciation.rs` exist in `tests/` dirs. **Recommendation:** standardise — pick sibling `tests.rs` for unit, `tests/` for integration. `sim/ecosystem/tests/speciation.rs` (1200 lines) duplicates concerns covered in `src/tests.rs`; reconcile.
- **`sim/civ/src/`: flat `pub mod` for some, `mod` (private) for others, no obvious rule.** Counted 13 `pub mod` and 10 private. `apparatus`, `culture_hooks`, `economy`, `environmental_drift`, `religion`, `figures`, `cosmology` are all `pub` but only `apparatus`, `cosmology`, `figures`, `transmission`, `tech`, `catastrophe`, `conflict`, `discovery` have external consumers. See **Pub visibility hygiene** below.

### Cross-crate dependencies

- **`sim-core` depends on `sim-civ`**, which depends on `sim-ecosystem` / `sim-physics` / `sim-species` / `sim-world` / `sim-recognition` / `sim-population` / `protocol`. That hierarchy is consistent. No circular deps detected.
- **`sim/report/Cargo.toml:6` uses `sim-events = { path = "../events" }`** while every other crate uses `.workspace = true`. Inconsistent — switch to workspace form.
- **`sim-report` is depended on by `sim-core`** (line 9 of `sim/core/Cargo.toml`) and by `ages`. Reporting is downstream of the simulator in spirit but `sim-core` pulls it in — verify this is intentional (probably for the digest type), otherwise invert.

### Pub visibility hygiene

- **`sim/civ/src/lib.rs:27 pub mod culture_hooks`** — zero external consumers (`grep` returns nothing outside `sim/civ/`). Make `pub(crate)`.
- **`sim/civ/src/lib.rs:34 pub mod religion`** — only used inside `sim/civ/`. Same.
- **`sim/civ/src/lib.rs:23 pub mod apparatus`** — only used by `sim/core/src/phases.rs` (2 calls) and `tests.rs`. Either keep narrow `pub use` of the 2 needed fns or leave but document.
- **`sim/civ/src/lib.rs:42 pub mod environmental_drift`** — no external use. Make `pub(crate)`.
- **`sim/civ/src/lib.rs:41 pub mod economy`** — no external use. Make `pub(crate)`.
- **`sim/civ/src/conflict.rs` exposes 17 pub items, 30+ pub consts.** Many (e.g. `GRUDGE_BUMP_WINNER`, `HIER_W_*`, `DRIVE_W_*`) are tuning constants only the module itself uses. Promote to `pub(crate)` or move into a `pub mod tuning` opt-in surface.
- General: 740 `pub fn` vs 99 `pub(crate)` across `sim/`. Ratio is heavily skewed toward over-exposure; a clippy pass with `pub_use_of_private_extern_crate` + manual narrowing recommended.

### Test fixture duplication

- **3 sites build `Planet { ... }` literals, 11 sites build `PlanetEcosystem { ... }` literals, 2 sites build `Species { ... }`, and dozens build `PhysicsState`.** No shared `test_fixtures` crate or `#[cfg(test)] mod fixtures` module exists. The only consolidated helper is `ocean_planet()` at `sim/core/src/nomads.rs:1087`.
- Each crate's `tests.rs` reinvents `empty_state()` / `well_fed_state()` / `ocean_planet()` / similar. Examples: `sim/civ/src/tests.rs:6` (`empty_state`), `:10` (`well_fed_state`), `sim/ecosystem/src/tests.rs:32` (`capacity`). **Recommend a `sim-test-support` dev-dep crate** with `planet::*`, `species::*`, `ecosystem::*`, `physics::*` builders. Sprint-sized: this is the single biggest test-code-reduction lever.

## Polish (defer)

- **`sim/report/src/q32.rs`, `sim/report/src/parse.rs`, `sim/report/src/labels.rs`, `sim/report/src/html.rs`, `sim/report/src/ages.rs`** — flat sibling files in a crate that already has `digest/`, `render/`, `viewport/` sub-modules. Consider a `format/` (q32, parse) and `output/` (html, labels, ages) sub-grouping.
- **`sim/physics/src/` is the most extreme flat namespace: 27 sibling `.rs` files** all re-exported from `lib.rs:13-39`. Suggested sub-modules:
  - `atmosphere/` — `clouds`, `wind`, `coriolis`, `hadley`, `vertical`, `atmospheric_escape`
  - `surface/` — `tectonics`, `volcanism`, `weathering`, `hydrology`, `isostasy`
  - `radiation/` — `radiation`, `albedo`, `tidal_heating`, `tides`
  - `field/` — `magnetism`, `lorentz`, `em`
  - `core/` — `grid`, `state`, `laws`, `orchestration`, `mechanics`, `fluid`, `heat`, `hemisphere`
  Keeps `chemistry/` as-is.
- **`sim/world/src/`** also flat (12 files): `climate`, `composition`, `habitability`, `hemisphere`, `init`, `planet`, `sampling`, `star`, `tidal_locking`, `types`. Consider `body/` (planet, star, composition) vs `dynamics/` (climate, habitability, tidal_locking).
- **`sim/core/src/phases.rs:11`** — the comment block apologises for an incomplete `compute_territory` import migration. Finish the cleanup.
- **`sim/civ/src/conflict.rs:182 fn resolve`** — 150-line function with inline subtractions, war casualty calc, grudge bumps. Extract a `WarOutcome` builder.
- **`sim/physics/src/atmospheric_escape.rs:115` (`earth_like`) / `:131` (`mars_like`)** — preset constructors. Consider a `presets.rs` once more bodies (Venus, Titan) join.
- **`sim/arith/src/lib.rs` (952 lines)** — single file for the fixed-point `Real` type and ops. OK if it's all one type, but worth a tests-vs-impl split.

### Dead code candidates (needs deeper grep to confirm)

- `sim/civ/src/lib.rs:529-645` — wall of 40+ `pub const`s with long names; some may have no consumers after the M4/M9 refactors. Strip via `cargo +nightly udeps` + targeted grep.
- `sim/civ/src/lib.rs:58 pub use drift::COLLECTIVE_QUORUM_POP` — single-use re-exports inflate the public surface; collapse if no external consumer.
- `sim/civ/src/culture_hooks.rs`, `religion.rs`, `economy.rs`, `environmental_drift.rs` — verify these aren't ghost modules from an abandoned design direction (see pub-visibility section above).
- Clippy run came back clean (no `unused`/`dead_code` warnings), so true dead code is bounded — focus is on under-used `pub`.

## Suggested refactors (concrete, sized)

1. **Split `sim/core/src/lib.rs` (~2519 lines).** Extract `run()`'s tick-body into `run_tick.rs`; move setup wiring into `setup.rs` (already exists, expand). Targets: `lib.rs` <400, `run_tick.rs` ~600, `setup.rs` ~400.
2. **Split `sim/ecosystem/src/lib.rs` (2354 lines)** into `species.rs` (EcoSpecies), `planet.rs` (PlanetEcosystem + step), `sampling.rs`, `constants.rs`, `invariants.rs`. Keep `hgt.rs` + `speciation.rs`.
3. **Split `sim/civ/src/catastrophe/mod.rs` (1497 lines)** → already a directory; add `kind.rs`, `record.rs`, `apply.rs`. `mod.rs` becomes a 40-line facade.
4. **Split `sim/civ/src/conflict.rs` (1563 lines)** → folder `conflict/` with `war.rs`, `alliance.rs`, `grudge.rs`, `assessment.rs`, `mod.rs`.
5. **Split `sim/physics/src/tectonics.rs` (2097 lines)** → folder `tectonics/` with `plates.rs`, `slab_pull.rs`, `subduction.rs`, `erosion.rs`.
6. **Group `sim/physics/src/*.rs` (27 flat files)** into 5 sub-modules (`atmosphere/`, `surface/`, `radiation/`, `field/`, `core/`). Re-exports from `lib.rs` stay flat to preserve the API.
7. **Create `sim/test-support/` dev-dep crate** with `planet_fixture`, `species_fixture`, `physics_fixture`, `ecosystem_fixture` builders. De-duplicate ~50 inline literals across `tests.rs` files.
8. **Audit and downgrade `pub mod` → `pub(crate) mod`** in `sim/civ/src/lib.rs` for `culture_hooks`, `religion`, `economy`, `environmental_drift`.
9. **`sim/report/Cargo.toml:6`** — change `sim-events = { path = "../events" }` to `sim-events.workspace = true` for consistency.
10. **Prune `.claude/worktrees/`** — 40+ stale agent worktrees that hurt search/index performance.
