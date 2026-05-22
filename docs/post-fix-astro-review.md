# Post-fix astro review (PRs #73-99)

Reviewer: senior astrophysicist + planetary-science lens, re-reviewing
after the 26-PR fix cycle described in `docs/post-implementation-fixes.md`.
Previous verdict (`docs/post-implementation-astro-review.md`):
**APPROVED_WITH_CONDITIONS** with six explicit gating conditions and
~40 secondary items.

## Overall assessment

The fix cycle has materially closed the gap between v2's "derived
physics" claim and the implementation. Five of the six gating
conditions are now genuinely satisfied (not just unit-tested in
isolation but wired into the production tick), and most of the
coupling / decorative-feature concerns have been retired by the P1
PRs. The remaining gaps are now honest: Europa's tidal budget is
flagged in-source as a structural calibration shortfall pending a
sub-day macro-step, and the magic-constant documentation pass
(Condition 5) is the only condition with zero progress. The
worrying class of failure — "feature exists but production never
calls it" — that drove the original review is essentially gone:
`hadley`, `subsurface_temperature`, `cosmic_ray_ground_flux`,
`hz_inner_edge_au`, `sub_stellar_point`, and `crustal_remanence`
are all on the live path. Verdict: **APPROVED_WITH_CONDITIONS**,
with the remaining conditions narrowed to (a) the magic-constant
ladder doc and (b) a follow-up cycle on the Europa structural
calibration gap.

## Conditions 1-6 status

| # | Condition | Status | Evidence |
|---|---|---|---|
| 1 | Wire `HadleyCirculation` + jet-velocity test | **Satisfied** | `sim/core/src/phases.rs:94-98` passes `Some(&laws.hadley)`; `sim/physics/src/orchestration.rs:461,690` accepts and runs it. Test `earth_like_steady_state_jet_velocity_in_subtropical_band` at `sim/physics/src/hadley.rs:954-1016` integrates 1000 ticks and pins `|v_q| ∈ [10, 60] m/s` at 45°N, with documented derivation that the algebraic steady-state is 30 m/s. |
| 2 | Europa + Enceladus tidal-heating tests | **Partially satisfied** | Both tests exist (`europa_like_configuration_global_heat_in_5_to_20_tw_range` at `tidal_heating.rs:1143-1165`; `enceladus_like_configuration_global_heat_in_5_to_50_gw_range` at `tidal_heating.rs:1188-1209`). Enceladus matches literature (~10.7 GW vs ~16 GW observed). Europa pins to a `[0.1, 5] TW` window with a `FIXME: calibration` — the produced ~0.42 TW is ~25× below the ~10 TW literature value. Structural cause documented at `tidal_heating.rs:74-104, 274-281` and `post-implementation-fixes.md:356-366`. |
| 3 | Explicit molecular mass + H/He fractionation test | **Satisfied** | `molecular_mass_amu` at `sim/physics/src/atmospheric_escape.rs:285+`; `JEANS_SCALE` is fully eliminated (no remaining hits in the tree). Test `h_vs_he_fractionation_ratio_above_thousand` at `atmospheric_escape.rs:940-970` asserts H/He Jeans factor ratio > 1000×, and `mars_co2_retention_higher_than_h2o` at `:982+` asserts >100× CO2/H2O margin on Mars conditions. |
| 4 | Wire `Star::hz_inner_edge_au` into habitability | **Satisfied** | `sim/world/src/habitability.rs:160-161` reads both inner+outer edges; `cell_habitability` produces a continuous degradation function (`distance / hz_inner` inside, `hz_outer / distance` outside). Acceptance tests `planet_outside_hz_inner_edge_habitability_degrades` (tests.rs:1023) and `planet_outside_hz_outer_edge_habitability_degrades` (:1069) confirm biome class drifts with stellar age. |
| 5 | Document the magic-constant ladder | **Not satisfied** | No `docs/magic-constants.md` or equivalent exists. Individual constants are well-commented in-source (`tidal_dimensional_calibration` at `tidal_heating.rs:50-104`; `EUV_DECAY_GYR` at `star.rs:457-470`; `WEATHERING_BASE` at `weathering.rs:67-68`; `kick_fraction` at `hadley.rs:603`), but the consolidated "fitted to Earth-analog; cross-planet not validated" ladder the condition asked for was never written. This is the cleanest remaining gap. |
| 6 | Link `H ∝ e²` to `de²/dt` for orbital-energy conservation | **Satisfied** | `heating_coefficient_per_e_squared` (`tidal_heating.rs:340-364`) and `synchronous_eccentricity_damping_rate` (`:384-395`) factor through the same `tidal_dimensional_calibration`, so `H = -dE_orbit/dt` is algebraic. Test `tidal_heat_matches_orbital_energy_loss_for_circular_decay` at `:1245` integrates the damping window and pins relative drift to 1%. Free-rotator path (`:409-415`) is the same coefficient scaled by 1/10 so the invariant survives the locking-state ladder. |

## Other previously-flagged items addressed

**Coupling gaps (7 from original review):**

1. Tidal heating → subsurface ocean: **resolved**. `PhysicsState::subsurface_temperature` (`state.rs:158, 634-668`) plus `subsurface_heat_fraction` per-substrate routing (`tidal_heating.rs:159-178`) plus conduction kernel (`:651-701`) plus init (`init_subsurface_temperature` at `:659-668`). Substrate fractions: Aqueous 90%, Hydrocarbon 90%, Ammoniacal 60%, Silicate 30%.
2. Mass-radius → wind/tide constants: **partial**. `Wind::earth_like()` is still the build entry-point for tests, but `wind.rs:186-189` notes `build_laws` overrides `scale_height_m` per-planet from `Atmosphere::scale_height_m`. `tide_k` itself (`tides.rs:102, 116, 144`) remains a hard scalar — not threaded through `Planet::gravity()`. Backlog candidate.
3. Tectonics → albedo: **resolved** by P3.7. `base_albedo_for(water_depth, biofuel, crust)` at `albedo.rs:217` with `Crust::{Basaltic, Granitic, Sedimentary, Hydrocarbon, Icy, Default}` (`:138-178`). Acceptance test `basaltic_crust_has_lower_base_albedo_than_granitic` at `:479-491`.
4. Magnetic reversal → mutation rate: **resolved** by P1.2. `sim/core/src/lib.rs:677-685` threads `state.cosmic_ray_ground_flux()` into `step_speciation`, and `:699` into `step_hgt`. Multiplier clamped `[1, 10]` at `speciation.rs:107-114`.
5. HZ migration → habitability transition: **resolved** (Condition 4).
6. Sub-stellar point → radiation: **resolved** by P1.5. `Radiation` stores `substellar_lat_turns / substellar_lon_turns` (`radiation.rs:272-275`) and the per-cell loop computes great-circle distance for `LockingMode::Synchronous` (`:570-612`) with day/night `T_eq` absorption fall-off via fourth-root Stefan-Boltzmann.
7. Cloud-top T → cirrus greenhouse: **not addressed**. `cirrus_greenhouse_k()` (`clouds.rs:121`) still returns a constant ~15 K per unit cloud fraction. Lapse coupling deferred.

**12 new backlog items (from original §"New items"):**

| # | Item | Status |
|---|---|---|
| 1 | Hadley + jet-velocity test | Done (Cond. 1) |
| 2 | Subsurface heat reservoir | Done (P1.1) |
| 3 | Derive `cal_factor` + Europa/Enceladus tests | Partial (Cond. 2) |
| 4 | Explicit mass Jeans + H/He test | Done (Cond. 3) |
| 5 | Arrhenius weathering | Done (P2.3) — `weathering.rs:22-94, 167-181` true Arrhenius `exp(Ea/R × (1/T_ref - 1/T))` |
| 6 | Couple `omega.0` to `axial_tilt_deg` | Done (P1.6) — `coriolis.rs:151-171` |
| 7 | Synchronous sub-stellar → radiation | Done (P1.5) |
| 8 | HZ-migration → habitability | Done (Cond. 4) |
| 9 | Crust-type base albedo | Done (P3.7) |
| 10 | Venus/Earth-CO2/Mars-MAVEN calibration tests | **Not done** — 16 anchors remain deferred |
| 11 | Derive Hadley band edges from angular-momentum closure | Done (P3.6) — `held_hou_hadley_edge` at `hadley.rs:247-281`; Newton-iterated `arcsin_unit` at `:295` |
| 12 | Link `cal_factor` ↔ `synchronous_damping_per_dt` | Done (Cond. 6) |

**Shortcut concerns (6 from original):**

- `cal_factor=1.75e8` fitted to Io → renamed `tidal_dimensional_calibration`, dimensional derivation documented; Io still anchors the multiplier (Europa gap unresolved).
- Hard-coded `[1.0, 2.3, 4.0, 6.0]` thresholds → still present at `hadley.rs:465-466` as cell-count branches; the underlying Hadley edge is now Held-Hou-derived, but the multi-cell jump points are still empirical.
- 30°/60°/90° band edges → resolved by P3.6 (Held-Hou closure feeds `compute_hadley_layout`).
- Piecewise-linear weathering → resolved by P2.3.
- `JEANS_SCALE=5` heuristic → resolved by P2.2.
- 5× weight range 16-44 amu → resolved by P2.2.

## New gaps surfaced after the fixes landed

1. **Europa structural calibration gap (~25×)**. Now visible because Enceladus matches and Europa doesn't — confirming the diagnosis at `tidal_heating.rs:78-104`: 1-macro = 1-day cadence rounds Europa's 3.55-day period to 4, losing `(4/3.55)⁵ ≈ 1.65×`; the rest is the Io multiplier absorbing melt-enhanced `k₂/Q` not present in icy moons. Fix path identified (sub-day macro or per-moon scaling). This is a *visible* gap rather than a hidden one — net win.
2. **Test pinning shifted from seed 42 to seed 1024**. Per `post-implementation-fixes.md:205-217`, the canary tests were re-pinned because the post-Items-12-24 RNG shift broke seeds 42 / 100 / 7 / 11 / 4096 in the "no civs at 16k ticks" bucket. Three `#[ignore]`'d slow tests on seed 42 (`knowledge_transmits...`, `successor_civs...`, `collapse_fires...`) are not yet re-pinned. **Implies**: the civ-formation pipeline's seed sensitivity widened — credibility of "Earth-analog runs in the median seed bucket" is now load-bearing on the seed-1024 carve-out.
3. **`step_hgt` reads only an Earth-surface `LocalConditions`** (`sim/core/src/lib.rs:698-699`). P3.3 plasmid sweep is wired but the per-cell biota layer the comment acknowledges as "a P0.1 follow-up" is still missing — HGT does not localise.
4. **Cell-count threshold ladder unmoored from the new Held-Hou closure**. P3.6 fixes the Hadley *edge* but the count branches (`hadley.rs:465-466`) still use the original `[1.0, 2.3, 4.0]` Rossby-ratio bands; these aren't derived from baroclinic instability either. The condition was technically met (band edges) but the cell-count special-casing remains.

## Remaining shortcut concerns

| File:line | Shortcut | Risk |
|---|---|---|
| `tidal_heating.rs:284` | `tidal_dimensional_calibration` empirical 5.4× multiplier on top of the dimensional ~3.27e7 base | Still Io-anchored; Europa is a known 25× miss |
| `tidal_heating.rs:322-324` | `orbital_energy_scale_per_e_squared = 15_700` is a magic constant | Energy-conservation tautology holds, but the absolute scale is fitted; not derived from `(GMm/2a)` first principles |
| `hadley.rs:603` | `kick_fraction = 1%` per tick | Test target of ~30 m/s steady state is hit, but the constant is unit-system-absorbing |
| `hadley.rs:465-466` | `[1.0, 2.3, 4.0, 6.0]` cell-count thresholds | See "new gaps" #4 above |
| `magnetism.rs:147` | `REVERSAL_TRIAL_DEN = 250_000` (1 tick = 1 year cadence) | Time-scale inconsistency with other laws (per-month) flagged in original review — **not resolved** |
| `atmospheric_escape.rs:149-165` | Jeans still uses *surface* T, not exobase T (~1000 K Earth) | Acknowledged in source comments at `:163-169`; H/He ordering correct, absolute timescales still per-tick-cadence-inflated |
| `radiation.rs:90-92, 141-143` | Hard 250 K greenhouse cap | Venus runaway plateau test still missing |
| `clouds.rs:121` | `cirrus_greenhouse_k = 15 K` constant | Coupling gap #7 still open |
| `wind.rs:170-191` | `Wind::earth_like()` is the default; mass-radius decoupled from `tide_k` and `wind_k` | Per-planet override of `scale_height_m` only |

## Calibration anchors satisfied vs missing

**Satisfied:**
- Io tidal heat ∈ [50, 200] TW (`tidal_heating.rs:784`)
- Enceladus tidal heat ∈ [1, 100] GW, lands at ~10.7 GW (`tidal_heating.rs:1188`)
- H/He Jeans ratio > 1000× at Earth conditions (`atmospheric_escape.rs:940`)
- Mars CO2/H2O Jeans ratio > 100× (`atmospheric_escape.rs:982`)
- Earth jet velocity ∈ [10, 60] m/s after 1000-tick steady state (`hadley.rs:954`)
- HZ-edge degradation continuity (`world/src/tests.rs:1023, 1069`)
- Tidal H = -dE/dt to 1% drift (`tidal_heating.rs:1245`)
- Faint-young-Sun ZAMS at 0.70 (`star.rs:413-423`)
- Crust-type albedo ordering (basalt < granite) (`albedo.rs:479`)
- Subsurface heat dominance for Aqueous/Hydrocarbon at 90% (`tidal_heating.rs:971, 1097`)
- Earth-analog escape < 5% over 100 ticks (`atmospheric_escape.rs:912`)

**Still missing (deferred):**
- Europa ~10 TW (in `[5, 20]` spec window) — currently pinned to `[0.1, 5]` with `FIXME`
- Carbonate-silicate steady-state CO2 ~280 ppm
- Mars-MAVEN absolute escape ~2-3 kg/s per channel
- Walker-Hays-Kasting snowball recovery timescale
- Venus runaway plateau T ∈ [700, 770] K
- Hadley jet velocity is anchored loosely (factor 2 slack) rather than 30 m/s ±20%
- 16-anchor backlog from xeno side (apex-predator cascade, mass-extinction recovery, etc.)

## Time-scale separation status

| Concern | Status |
|---|---|
| Tidal `H ∝ e²` vs `de²/dt` energy bookkeeping | **Resolved** (Cond. 6). Both rates trace through `tidal_dimensional_calibration`; `tidal_heat_matches_orbital_energy_loss_for_circular_decay` pins 1% drift over the damping window. |
| Atmospheric escape per-tick cadence (~10⁴× inflation) | **Not resolved**. Source comments at `atmospheric_escape.rs:326+` acknowledge "real Jeans escape uses exobase T ≈ 1000 K on Earth". Earth-vs-Mars *ratio* still right, absolute timescales still wrong. |
| Weathering vs volcanism balance at Earth-like CO2 | **Not resolved**. Arrhenius is in (P2.3) but the CO2 steady-state test isn't. Independent constants still unconstrained. |
| Magnetic-reversal cadence (`REVERSAL_TRIAL_DEN = 250_000`, 1 tick = 1 year) vs per-month elsewhere | **Not resolved**. Same constant at `magnetism.rs:147`. Cross-law cadence is still inconsistent. |

## Deferred items (4 from original)

- Save/load with cumulative-drift accumulator — no change.
- Spectral-line opacity / band saturation — no change; `greenhouse_cap_k = 250 K` still stands in for band-by-band optical depth.
- Per-cell exobase T — no change; surface T still used.
- Tilt-driven seasonal Hadley shift — `omega.0 = |Ω|·sin(tilt)` is now wired (P1.6 at `coriolis.rs:151-171`), so the necessary kinematic ingredient is present, but the seasonal Hadley-edge oscillation that the original review wanted is not yet a test or a documented behaviour.

## Approval status

**APPROVED_WITH_CONDITIONS** — narrowed.

The fix cycle has been substantive and largely on-target. Of the
six gating conditions, five are satisfied at the production-path
level (not merely unit-tested in isolation), and the sixth
(magic-constant ladder doc) is documentation-only. The two
material remaining astrophysics gaps are:

1. **Europa tidal-heating calibration shortfall (~25×)**. Honest,
   well-diagnosed, and in-source flagged. Acceptable as a known
   limitation if the doc layer makes "icy-moon tidal heating
   under-predicts by ~25× until sub-day macro cadence lands" a
   first-class caveat in the world-spec layer; not acceptable as
   an undocumented surprise.

2. **Magic-constant ladder doc (Cond. 5)**. The work to write
   `docs/magic-constants.md` is mechanical given how well-commented
   the individual constants now are (`tidal_dimensional_calibration`,
   `EUV_DECAY_GYR`, `WEATHERING_BASE`, `kick_fraction`,
   `orbital_energy_scale_per_e_squared`, `REVERSAL_TRIAL_DEN`,
   `JEANS_SCALE` removed, `cirrus_greenhouse_k`). A 1-2 page table
   summarising each constant's value, derivation status (derived /
   dimensional / fitted), the anchor it was tuned against, and the
   regimes where it's known to break.

**Conditions for full APPROVED**:

1. Write the magic-constant ladder doc (the explicit Condition 5).
2. Either fix Europa's tidal budget (sub-day macro or per-substrate
   scaling) or document the gap in `docs/physics.md` as a
   first-class limitation.
3. Address the cell-count threshold ladder at `hadley.rs:465-466`
   so the "derived from angular-momentum closure" claim covers
   both edge *and* count.
4. Re-pin the three `#[ignore]`'d slow canary tests on seed 1024
   (or whichever seed survives the new RNG shift) so the
   regression bench isn't load-bearing on a single seed.

The remaining time-scale and deferred items (exobase T, save/load,
spectral bands, magnetic-reversal cadence) are honest open problems
that don't block the "credible physics" claim at the Earth-analog
regime the sim is anchored to, but should be on the longer-term
backlog. With items 1-2 above closed, this would be a clean full
**APPROVED**.

---

### Reference file:line index (new evidence)

- Hadley wiring: `sim/core/src/phases.rs:94-98`, `sim/physics/src/orchestration.rs:461,690`, jet test `sim/physics/src/hadley.rs:954-1016`
- Held-Hou edge: `sim/physics/src/hadley.rs:247-281` with `arcsin_unit` Newton at `:295`
- Subsurface reservoir: `sim/physics/src/state.rs:158, 634-668`; routing `tidal_heating.rs:159-178, 651-701`
- Mass-explicit Jeans: `sim/physics/src/atmospheric_escape.rs:285+, 478-481`; H/He test `:940-970`
- HZ wiring: `sim/world/src/habitability.rs:160-161`; tests `world/src/tests.rs:1023, 1069`
- Sub-stellar radiation: `sim/physics/src/radiation.rs:272-275, 570-612`
- Coriolis tilt: `sim/physics/src/coriolis.rs:151-171`
- Arrhenius weathering: `sim/physics/src/weathering.rs:22-94, 167-181`
- Crust albedo: `sim/physics/src/albedo.rs:138-178, 217-291`; test `:479-491`
- Energy-conservation tidal pair: `sim/physics/src/tidal_heating.rs:322-435`; test `:1245`
- Per-cell crustal remanence: `sim/physics/src/magnetism.rs:461-563`
- Cosmic-ray → speciation/HGT: `sim/core/src/lib.rs:677-699`; multiplier clamp `sim/ecosystem/src/speciation.rs:107-114`
- DormantPool catastrophe seeding: `sim/civ/src/catastrophe/mod.rs:205-239`
- Civ capacity reads producer biomass: `sim/civ/src/capacity.rs:62-291`
- Ecosystem in core dep graph: `sim/core/Cargo.toml:16`
- Lifecycle dispatch: `sim/civ/src/capacity.rs:184-359`
- Faint-young-Sun: `sim/world/src/star.rs:391-455`
- Europa/Enceladus tidal calibration: `sim/physics/src/tidal_heating.rs:1143-1209`; module note `:74-104, 274-281`
- Differentiated kinds: `sim/ecosystem/src/lib.rs:54-201, 791-1035`
