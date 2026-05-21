# Post-implementation astro review (Sprints 1-5, Items 1-24)

Reviewer: senior astrophysicist + planetary-science lens. Scope:
physics credibility of the 35-PR implementation against
`docs/implementation-plan.md` v2.

## Overall assessment

The sim has moved from "weather on a frozen planet" to "credible
Earth-analog dynamics with sketched extensibility." Every Sprint
1-5 item is present, the conservation invariants hold, and the
Earth/Io calibration tests pass. But the v2 plan's signature
claim — that headline laws *derive* their behaviour from physical
formulas — is only partially true: a parade of fitted multipliers
(`cal_factor`, `JEANS_SCALE`, `kick_fraction`, `WEATHERING_BASE`)
hides unit-system risk behind every non-Earth-analog test, and at
least one headline law (`HadleyCirculation`) is fully implemented
but **never instantiated by the orchestrator**. I would approve
with conditions: wire the loose laws, replace heuristic constants
with mass-dependent formulas where the science demands it, and
add cross-planet calibration anchors beyond Earth and Io.

## Critical gaps (S/M/L/XL)

### XL — `HadleyCirculation` is never integrated

`sim/physics/src/hadley.rs:529-536` implements `Law::integrate`,
but `sim/physics/src/orchestration.rs:393-432` doesn't accept it
and `sim/core/src/phases.rs:65-94` never builds one. Zonal jets
do not form at runtime. The three Sprint-5 acceptance tests
validate only the kinematic **layout function**. The signature
emergence story of v2 — "number of cells emerges from rotation ×
radius" — is dark code.

### XL — Tidal heating is decoupled from interior / hydrology

`tidal_heating::distribute_heat_to_cells`
(`sim/physics/src/tidal_heating.rs:297-318`) deposits energy
**uniformly** into surface T. Real Io concentrates heat at
mid-latitude shear zones; Europa / Enceladus power *subsurface*
oceans. There is no subsurface-heat reservoir (`grep subsurface |
geothermal` in `sim/physics/` returns nothing). The Io calibration
gates only the scalar total. This forecloses the most interesting
xenobiological consequence — subsurface habitats on tidally
heated moons.

### L — `cal_factor = 1.75e8` is fitted to Io alone

`sim/physics/src/tidal_heating.rs:152-154` absorbs `1/G` + radius-
unit + period-unit conversions into a single constant tuned so Io
lands in [50, 200] TW. No test that Europa (~10 TW) or Enceladus
(~16 GW) match published budgets. The `e²` scaling, the rocky/icy
`Q` ratio, and the Io anchor all share this factor — a units bug
would pass all three tests while producing nonsense on a 5-Earth-
radius super-Jupiter moon.

### L — Atmospheric escape replaces real Jeans with a heuristic

`sim/physics/src/atmospheric_escape.rs:158-159` introduces
`JEANS_SCALE = 5` to "lift our simplified `v_esc/sqrt(T)` ratio
into a Jeans-like discrimination range." Real Jeans escape
depends on `λ = m·v_esc²/(2kT)` with **explicit molecular mass m**.
Mass is squashed into per-substance weights (`substance_weight()`,
lines 221-237) that span only 0.2–1.0 across 16-44 amu — a 5×
discrimination where the real exponential gives ~10⁴× for H vs He
at Earth conditions. Tests check ordering only, not magnitude.

### M — Weathering is piecewise-linear, not Arrhenius

`sim/physics/src/weathering.rs:162-166` uses
`linear = 1 + (T-290)/50` clamped to `[0.1, 10]`. Constant
sensitivity across the habitable range, where Arrhenius gives
~10×/Gyr per ~10 K. Snowball recovery is therefore slower than
geochemistry predicts; no test pins recovery timescale.

### M — Magnetic shielding is a single planet-wide scalar

`PlanetEscapeParams::magnetic_strength` collapses to one number.
Partial magnetospheres — the regime of greatest scientific
interest (Mars's southern-highland crustal remanence umbrellas) —
cannot be represented.

### M — Coriolis `omega.0` never reads `Planet::axial_tilt_deg`

`sim/physics/src/coriolis.rs:124-139` hard-codes `omega.0 = 0`.
Tilted-axis worlds (every non-zero-tilt planet) have no zonal
rotation-axis component. Combined with the Hadley unwiring,
rotation-and-tilt dependence of climate is cosmetic.

## Shortcut concerns

| File:line | Shortcut | Risk |
|---|---|---|
| `tidal_heating.rs:152` | `cal_factor = 1.75e8` fitted from Io | breaks outside rocky Earth-radii moons |
| `hadley.rs:314-320` | Hard-coded `[1.0, 2.3, 4.0, 6.0]` cell-count thresholds | tuned to Earth + slow + 8-hour rotators |
| `hadley.rs:352-372` | 30°/60°/90° band edges special-cased | doesn't emerge from baroclinic instability |
| `weathering.rs:162-166` | Piecewise-linear, not Arrhenius | wrong snowball-recovery slope |
| `atmospheric_escape.rs:159` | `JEANS_SCALE=5` heuristic | replaces `m·v²/(2kT)` exponent |
| `atmospheric_escape.rs:221-237` | 5× weight range over 16-44 amu | exponential mass-dependence missing |
| `atmospheric_escape.rs:362` | `1/(1+B)` reused as ozone shield | no actual ozone chemistry |
| `radiation.rs:141-143` | Hard 250 K greenhouse cap | Venus runaway flatlines |
| `world/src/star.rs:382-407` | Linear MS drift 1.0→1.4× | ZAMS already at modern Sun; faint-young-Sun missing |
| `world/src/star.rs:391-402` | Linear ramp 1.4→1000× over final 5% | not Hertzsprung-gap / AGB shape |
| `world/src/star.rs:148-187` | Hard-coded SED fractions per class | no Planck function |
| `world/src/planet.rs:249-264` | Density per substrate only | mass × radius constraint ignored |

## Missing tests

- **H-vs-He fractionation** — gates real mass-dependent Jeans.
- **Faint-young-Sun** — ZAMS Sun should be 70%, not 100%, of modern.
  Current implementation makes this impossible.
- **Europa (~10 TW) + Enceladus (~16 GW) tidal heat** — one Io
  anchor is not enough to gate `cal_factor`.
- **Carbon-silicate steady-state CO2** — Earth lands at ~280 ppm
  in equilibrium; current weathering test pins drift but not value.
- **Mars-MAVEN absolute escape** — ~2-3 kg/s per channel rather
  than ">1% over 500 ticks."
- **Hadley jet velocity at Earth-like** — apply step's
  `kick_fraction = 1%` is unanchored against real ~30 m/s
  subtropical jet.
- **Walker-Hays-Kasting snowball recovery** — CO2 builds under
  ice cover until weathering kicks back in.
- **Venus runaway plateau** — Earth + 90 bar CO2 → T ∈ [700, 770] K.

## Coupling gaps

1. **Tidal heating → subsurface ocean / hydrology**. No subsurface
   reservoir; surface T is the only heat sink. Europa-class
   habitats cannot exist.
2. **Mass-radius (Item 21) → wind / tide constants**.
   `Planet::gravity()` is derived, but `Wind::earth_like()`
   (`wind.rs:170-191`) and `tide_k` are hard scalars. A 2-Earth-mass
   planet runs Earth's wind dynamics.
3. **Tectonics → albedo**. Mountain uplift + basalt-vs-granite
   contrast don't reach `base_albedo_for` — reads only
   water_depth + biofuel_ceiling (`albedo.rs:151-159`).
4. **Magnetic reversal → mutation rate**. Plan claims reversal →
   cosmic-ray flux → species drift. `magnetism::DipoleState` and
   `cosmic_ray_ground_flux()` exist; no read into `sim/species/`.
5. **Stellar HZ migration → habitability transition**.
   `Star::hz_inner_edge_au` / `hz_outer_edge_au` are accessors
   nobody reads. Stellar evolution doesn't downgrade
   `BiosphereClass`.
6. **Tidal-locking sub-stellar point → radiation**.
   `tidal_locking::sub_stellar_point` returns `(0,0)` for
   `Synchronous` (`world/src/tidal_locking.rs:115`), but
   `Radiation::integrate`'s per-row table never reads it. Locked
   worlds are climatically indistinguishable from spinning ones.
7. **Cloud-top T → cirrus greenhouse strength**.
   `cirrus_greenhouse_k = 15 K` is constant; real cirrus forcing
   scales with `T_surface − T_cloud_top`. No lapse coupling.

## Time-scale separation concerns

- **Tidal heating energy bookkeeping**. `H ∝ e²` is the
  instantaneous dissipation; `de²/dt ∝ -k·e²` is orbit-energy
  loss. Physically `H ≡ -E_orbit_dot`, so `cal_factor` in
  `moon_tidal_heat_rate` and `k` in `synchronous_damping_per_dt`
  should be **linked**. They're not. A `Synchronous` planet's
  accumulated tidal heat across the damping window can violate
  energy conservation silently.
- **Atmospheric escape per-tick cadence**. Mars lost atmosphere
  over ~3.5 Gyr; the implementation runs escape per sim-day and
  expects >1% loss in 500 ticks. Per-tick rate exaggerated by
  ~10⁴×; Earth-vs-Mars *ratio* lands right but absolute timescales
  do not.
- **Weathering vs volcanism balance**. Independent constants with
  no constraint they balance at Earth-like CO2.
- **Magnetic-reversal cadence vs other laws**. `REVERSAL_TRIAL_DEN
  = 250_000` treats 1 tick as 1 year; most other laws treat
  1 tick as 1 month. Cross-law calibration is inconsistent.

## Deferred items I'd want reconsidered

- **Save/load (R7)**. The cumulative-drift accumulator requires
  in-memory continuity; save/load that drops it cannot catch
  slow leaks post-restore.
- **Spectral-line opacity / band saturation**. `greenhouse_cap_k`
  stands in for what should be band-by-band optical depth.
  Venus CO2 bands saturate at very different T's than H2O bands.
- **Per-cell exobase T**. Atmospheric escape uses surface T;
  Earth's exobase is ~1000 K vs. surface 288 K. The Jeans factor
  is exponentially wrong by construction.

## New items (backlog additions)

1. **Wire `HadleyCirculation` + jet-velocity test** (target
   ~30 m/s steady state). *Trivial wiring, high payoff.* (S, 4 h)
2. **Subsurface heat reservoir** + conduction up to surface.
   Enables Europa-like habitats. (M, 16 h)
3. **Derive `cal_factor` dimensionally** + Europa/Enceladus tests. (M, 12 h)
4. **Explicit molecular mass + real Jeans exponent** + H/He
   fractionation test. (M, 14 h)
5. **Arrhenius weathering** (`exp(-Ea/RT)` is already in
   `sim_arith::transcendental`). (S, 4 h)
6. **Couple `omega.0` to `axial_tilt_deg`** for tilted worlds +
   seasonal Hadley shift. (S, 4 h)
7. **Synchronous-locking sub-stellar point → radiation**
   per-cell `T_eq`. (M, 10 h)
8. **HZ-migration → habitability downgrade** in
   `world/src/habitability.rs`. (M, 12 h)
9. **Crust-type base albedo** (basalt darker than granite). (S, 4 h)
10. **Calibration tests for Venus plateau, Earth-CO2 steady state,
    Mars-MAVEN escape**. (M, 14 h)
11. **Derive Hadley band edges from angular-momentum closure** so
    the 3-cell case isn't special-cased. (M, 14 h)
12. **Link `cal_factor` to `synchronous_damping_per_dt`** so
    tidal heat + eccentricity damping conserves orbital energy. (M, 12 h)

## Approval status

**APPROVED_WITH_CONDITIONS**.

The skeleton is complete, conservation invariants are respected,
and Earth-analog calibrations pass. But v2's claim of derived
physics is only partly true: Hadley emergence is computed but not
wired; tidal heating's formula is correct but the units-absorbing
calibration multiplier hides risk; Jeans escape replaces a mass-
dependent exponent with a substance-weight heuristic; weathering
replaces Arrhenius with a linear ramp.

**Conditions for full APPROVED**:

1. Wire `HadleyCirculation` into `phases.rs` + jet-velocity test.
2. Add Europa + Enceladus tidal-heating tests.
3. Replace `substance_weight` with explicit molecular masses;
   add H/He fractionation test.
4. Wire `Star::hz_inner_edge_au` / `hz_outer_edge_au` into
   habitability.
5. Document each magic constant (`cal_factor`, `JEANS_SCALE`,
   `kick_fraction`, `WEATHERING_BASE`, `EUV_DECAY_GYR`) with
   "fitted to Earth-analog; cross-planet not validated."
6. Link `H ∝ e²` to `de²/dt` so the tidal pair conserves orbital
   energy.

Without these, the sim is "Earth-analog-tuned with extensible
hooks" — credible inside Earth's parameter neighbourhood, untested
outside it. With them, it would be the "credible representation
in both lenses" the implementation plan promised.

---

### Reference file:line index

- Tidal heating formula + cal_factor + distribution:
  `sim/physics/src/tidal_heating.rs:152, 234-278, 297-318`
- Hadley layout + apply + missing wiring:
  `sim/physics/src/hadley.rs:206-290, 412-489`;
  `sim/physics/src/orchestration.rs:393-432`;
  `sim/core/src/phases.rs:65-94`
- Atmospheric escape Jeans + weights + orchestration:
  `sim/physics/src/atmospheric_escape.rs:158, 221-237, 259-269`;
  `sim/physics/src/orchestration.rs:714-728`
- Weathering T_factor: `sim/physics/src/weathering.rs:162-166`
- Coriolis 3D omega: `sim/physics/src/coriolis.rs:84-139`
- Greenhouse cap + H2O coefficient: `sim/physics/src/radiation.rs:90-92, 141-143`
- Star bolometric drift + EUV decay: `sim/world/src/star.rs:377-456`
- Planet gravity / escape velocity: `sim/world/src/planet.rs:188-237`
- Tidal-locking damping + sub-stellar point: `sim/world/src/tidal_locking.rs:72-147`
- Magnetic reversal Markov: `sim/physics/src/magnetism.rs:97-117`
- Cloud microphysics: `sim/physics/src/clouds.rs:121-188`
- Wind CFL sub-stepping: `sim/physics/src/wind.rs:236-329`
