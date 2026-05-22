# Round 3 astrophysics re-review (post-F-wave, PRs #101-#107)

Reviewer: senior astrophysicist + planetary-science lens, third pass
after the seven-PR F-wave follow-ups landed on top of the post-fix
state reviewed in `docs/post-fix-astro-review.md`.

## Overall assessment

**APPROVED.** The F-wave is a clean, on-target landing of all four
narrowed round-2 gates. Condition 5 (magic-constant ladder) is now a
495-line enumeration with `origin` and `cross-planet status` columns
covering every constant in `sim/{physics, world, ecosystem, civ}/`.
The Europa tidal shortfall — the only material physics gap surviving
round 2 — is closed by a per-substrate multiplier (Aqueous/Hydrocarbon
25×, Silicate/Ammoniacal 1×) keyed off `MetabolicSubstrate`; the test
now pins the literal `[5, 20] TW` literature window. The Hadley cell-
count ladder is now derived from a Rhines-length closure
`N = √2 / sin(lat_h)`, removing the last empirical Rossby-ratio
constant from the headline circulation law. The three `#[ignore]`'d
slow canaries are re-pinned to seed 1024. With these landed, the
derived-physics claim is end-to-end honest at the Earth-analog regime,
with remaining gaps explicitly catalogued as `unvalidated` rather than
hidden.

## Round-2 remaining conditions verification

| # | Round-2 condition | Status | Evidence |
|---|---|---|---|
| 1 | Write `docs/magic-constants.md` (Condition 5) | **Satisfied** | `docs/magic-constants.md` (495 lines, commit `6fe1c9d`). Every constant in `sim/{physics,world,ecosystem,civ}/` enumerated with file:line, value, origin tag (Earth-fitted / Solar-system-fitted / dimensional / literature / empirical-best-fit / per-substrate / arithmetic), and cross-planet status (validated / partial / known-bad / unvalidated). Summary at `:381-387`: ~25 validated, ~5 partial, 3 known-bad. Per-constant TODO at `:391-481`. |
| 2 | Fix Europa tidal budget or first-class limitation | **Satisfied** | `sim/physics/src/tidal_heating.rs:300-363` (commit `484beae`). `tidal_dimensional_substrate_multiplier(Option<MetabolicSubstrate>)` returns 25× for Aqueous/Hydrocarbon and 1× for Silicate/Ammoniacal/None. `MoonHeating::with_substrate` plumbs the hint. Test `europa_like_configuration_global_heat_in_5_to_20_tw_range` at `:1246-1268` now pins the literal `[5 TW, 20 TW]` literature window — the round-2 `FIXME: calibration` is gone. Io and Enceladus tests still in their respective windows. The Ammoniacal=1× choice is justified by 1.37-day → 1-macro rounding inflating `n⁵ ≈ 4.83×`. |
| 3 | Cell-count threshold ladder at `hadley.rs:465-466` derived from closure | **Satisfied** | `sim/physics/src/hadley.rs:509-543` (commit `4d98434`). `cell_count_from_hadley_edge(lat_h)` computes `N = √2 / sin(lat_h)` directly from the Held-Hou edge, then quantises with `n<2` → 1 and a skip-to-3 at `[2, 3)`. Derivation chain at `:36-72`: `L_rhines = π·sqrt(U/β)`, `U = ΩR·sin²(lat_h)`, `β = 2Ω/R`, with worked examples (slow-rotator → 1, Earth-like → 3 at `lat_h ≈ 25°`, 8-hour rapid → capped at MAX). The `[1.0, 2.3, 4.0, 6.0]` ladder is fully removed. |
| 4 | Re-pin the three `#[ignore]`'d slow canaries on seed 1024 | **Satisfied** | `sim/core/src/tests.rs:151, 176, 195` (commit `8035e95`). All three run `RunConfig::dev(1024, ...)` with inline F5 comments documenting the seed-42 → 1024 migration. Verified: all pass `cargo test --release -- --include-ignored` (91s / 101s / 234s). |

All four round-2 gates closed.

## Original 6 conditions confirmation (spot-check)

| # | Original condition | Still satisfied? |
|---|---|---|
| 1 | Wire `HadleyCirculation` + jet-velocity test | **Yes** — `sim/core/src/phases.rs:95-98` still passes `Some(&laws.hadley)`; F7 strengthened the claim by deriving cell count from the same closure as the edge. |
| 2 | Europa + Enceladus tidal-heating tests | **Yes, and now upgraded** — Europa test now pins to literature window directly, not the calibration-flagged proxy. |
| 3 | Explicit molecular mass + H/He fractionation test | **Yes** — `sim/physics/src/atmospheric_escape.rs:285-293` still carries `molecular_mass_amu` per substance; H/He and Mars CO2/H2O tests intact. |
| 4 | Wire `Star::hz_inner_edge_au` into habitability | **Yes** — `sim/world/src/habitability.rs:160-161` reads both inner/outer edges; habitability degradation tests intact. |
| 5 | Document the magic-constant ladder | **Now satisfied** — see round-2 condition 1 above. |
| 6 | Link `H ∝ e²` to `de²/dt` for orbital-energy conservation | **Yes** — `sim/physics/src/tidal_heating.rs:413-465` `heating_coefficient_per_e_squared` / `synchronous_eccentricity_damping_rate` still factor through the shared `tidal_dimensional_calibration`. F6 adds the per-substrate multiplier *outside* this shared coefficient, preserving the `H = -dE/dt` algebraic identity within each substrate class. |

No regressions in the round-2 baseline.

## Time-scale separation status

| Concern | Status |
|---|---|
| Tidal `H ∝ e²` vs `de²/dt` energy bookkeeping | **Resolved** (unchanged from round 2). F6's substrate multiplier preserves the shared-coefficient invariant. |
| Atmospheric escape per-tick cadence vs ~10⁴-year physical timescale | **Not addressed** — `sim/physics/src/atmospheric_escape.rs:163-169, 326+` still acknowledges surface-T-vs-exobase-T inflation in source comments; magic-constants doc flags as `unvalidated`. |
| Weathering vs volcanism balance at Earth-like CO2 | **Addressed** — `weathering_thermostat_holds_earth_like_at_300k_equilibrium` (`weathering.rs:409`) and `weathering_volcanism_balance_holds_earth_like_co2` (`volcanism.rs:207`) now exist and pass. This was an open round-2 item and quietly landed. |
| Magnetic-reversal cadence (`REVERSAL_TRIAL_DEN = 250_000` assumes 1 tick = 1 year, simulation runs per-month) | **Not addressed** — flagged in `magic-constants.md:135-136` as `known-bad` cadence; cross-law cadence inconsistency remains. |

The carbonate-silicate steady-state pinning at Earth-like 300 K is a
real upgrade over round 2 — it removes the last item from the
"time-scale" worry list that wasn't either fixed or explicitly
catalogued.

## New gaps surfaced

None of substance. Three minor observations:

1. **F6 keys off `MetabolicSubstrate` rather than a dedicated
   `MoonComposition` enum.** Defensible — `Aqueous` already encodes
   "water-ocean-bearing" elsewhere — but a future methane-ocean /
   silicate-mantle moon won't trigger the 25× boost. Internally
   consistent given the existing enum.
2. **F1's species_registry canary runs ~30 min (16k ticks).** Right
   test to have, but a shorter version that still trips
   speciation/HGT would be welcome on the CI fast lane.
3. **F6's `Ammoniacal=1×` rests on the 1.37-day → 1-macro rounding**
   (Enceladus inflates `n⁵` by 4.83× as a numerical accident).
   Sub-day macro would un-inflate this and drop Enceladus to ~2.2 GW
   (still inside `[1, 100]`, but shy of literature ~16 GW).
   Documented honestly at `tidal_heating.rs:1297-1302` and
   `magic-constants.md:402-409`.

## Remaining backlog (priority-ordered)

Drawn from `magic-constants.md:391-481` plus carryover. None block the
Earth-analog credibility claim.

**A — known-bad cadence:** re-base `REVERSAL_TRIAL_DEN` /
`REVERSAL_DURATION_TICKS` to per-month (currently 12× too slow because
constants assume per-year ticks; trivially mechanical).

**B — missing absolute anchors:** Venus runaway plateau test
(`T ∈ [700, 770] K`); Mars-MAVEN absolute escape ~2-3 kg/s per channel
(only ratio tests today); Walker-Hays-Kasting snowball recovery
timescale; Earth jet velocity tightening from `[10, 60]` to 30 m/s
±20% (F7's Rhines closure makes this finally feasible).

**C — coupling gaps:** `cirrus_greenhouse_k` from cloud-top T × lapse
rate (`clouds.rs:121`); `tide_k`/`wind_k` threaded through
`Planet::gravity()` (`tides.rs:102`, `wind.rs:170-191`); per-cell
exobase T in atmospheric escape.

**D — substrate coverage:** sub-day macro-step would un-inflate
Enceladus rounding and let the F6 boost drop back to bare integer-
period penalty; Ammoniacal subsurface heat fraction 0.60 needs an
Enceladus-anchored test.

**E — bookkeeping:** audit `unvalidated` constants and decide which
graduate to `validated` vs stay `empirical-best-fit`-acceptable; merge
the magic-constants TODO with the 16-anchor xeno backlog.

## Approval status

**APPROVED.** Full, unconditional.

The simulation's physics has cleared the bar the original review set
out as "credible representation in both lenses". Every round-2
narrowed condition is satisfied at the production-path level and
verified by a test. The derived-physics claim is honest: Hadley
emergence (edge + count) is parameter-free beyond
`(Ω, R, g, H, Δθ, T_eq)`; tidal calibration is per-substrate with all
three solar-system anchors (Io, Enceladus, Europa) inside literature
windows; mass-explicit Jeans escape passes H/He by >1000×; HZ
migration drives habitability class drift; tidal heat and orbital
damping share a coefficient so energy is conserved; carbonate-silicate
thermostat holds at 300 K.

The remaining backlog is honest fidelity work — Venus runaway,
Mars-MAVEN absolutes, magnetic-reversal cadence — explicitly
enumerated in `magic-constants.md` with priority and cross-planet
status. The class of failure that drove the original review ("law
exists, production never calls it") is fully gone; the round-2
follow-up class ("law calibrated to Earth, extrapolation status
invisible") is now catalogued. This is the cleanest physics layer
across the three rounds.

Recommend merging the F-wave and using the magic-constants backlog as
next-sprint input. No further astrophysics gates required.

---

### Reference file:line index (F-wave evidence)

- Magic-constant ladder: `/home/user/Ages/docs/magic-constants.md` (495 lines, commit `6fe1c9d`)
- Europa per-substrate fix: `/home/user/Ages/sim/physics/src/tidal_heating.rs:300-363, 1246-1268` (commit `484beae`)
- Hadley cell count from Rhines: `/home/user/Ages/sim/physics/src/hadley.rs:36-72, 447-461, 485-543` (commit `4d98434`)
- Slow canary re-pin: `/home/user/Ages/sim/core/src/tests.rs:151-213` (commit `8035e95`)
- Species registry population (F1): `/home/user/Ages/sim/core/src/lib.rs:164, 217-257, 722-758` (commit `36db614`)
- Carbonate-silicate balance tests (newly noted): `/home/user/Ages/sim/physics/src/weathering.rs:409`, `/home/user/Ages/sim/physics/src/volcanism.rs:207`
- Test totals: 214/214 sim-physics, 34/34 sim-world pass on `--release`.
