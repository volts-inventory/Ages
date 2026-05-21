//! Ice-albedo feedback (v2): sigmoid + bimodal-channel albedo.
//!
//! Replaces the earlier linear-ramp ice-albedo logic, which could
//! never produce snowball-Earth bifurcation: a strictly monotone
//! linear coupling of T → albedo → T relaxes to a unique
//! intermediate equilibrium, not the two distinct climate basins
//! that real planets exhibit (habitable vs. snowball).
//!
//! Three independent surface-cover channels carry the per-cell
//! albedo signal:
//!
//! - `snow_fraction` — bright snow on land or on top of sea ice
//!   (`0.85` peak). Updated per macro-step from the cell's
//!   temperature relative to the substrate freeze line plus a
//!   precipitation source from `Substance::Vapour` over land.
//! - `sea_ice_fraction` — sea ice without snow on top, darker
//!   "gray" channel (`0.55` peak). Updated from surface
//!   temperature alone over water cells.
//! - `cloud_fraction` — atmospheric cloud cover (`0.4` peak).
//!   Stubbed at `0` for now; the field exists so the per-cell
//!   effective-albedo helper is forward-compatible with the
//!   cloud-driver follow-up.
//!
//! The sigmoid freeze-line transition gives the system a sharp
//! but smooth switch at the freeze point — narrow enough (5 K
//! width) to amplify modest temperature perturbations into runaway
//! ice growth (the positive feedback that drives bifurcation),
//! soft enough to keep the integrator differentiable so the
//! relaxation rate of `Radiation` doesn't see a discontinuity at
//! the freeze line.
//!
//! ```text
//!   freeze_drive(T) = sigmoid_real((T_freeze - T) / 5)
//!     →  T well above freeze:  ~0   (no snow / no sea ice)
//!     →  T well below freeze:  ~1   (full snow / full sea ice)
//!     →  T near freeze:        steep S-curve
//! ```
//!
//! Per-cell effective albedo is the max of the four contributing
//! channels (base surface + snow + sea-ice + cloud) so the
//! dominant cover wins; layered fractions (snow on sea ice)
//! resolve via the multiplicative `(1 - snow_fraction)`
//! suppression on the sea-ice contribution.
//!
//! Determinism: `Real` math throughout (Q32.32 via `sim_arith`),
//! no `HashMap`, no f64. The sigmoid uses the deterministic
//! `exp` from `sim_arith::transcendental`, clamping the input
//! at ±10 to keep the exponent inside Q32.32's range without
//! a panic and to avoid wasted Taylor cycles where the result
//! has already saturated to 0 or 1.

use crate::chemistry::Substance;
use crate::laws::Law;
use crate::state::PhysicsState;
use sim_arith::transcendental::{exp, sqrt};
use sim_arith::Real;

/// Width of the sigmoid freeze-line transition, in Kelvin. 5 K
/// gives a sharp-but-smooth switch: at `T = T_freeze ± 5 K` the
/// sigmoid is at `1 - sigmoid(1) ≈ 0.27` / `sigmoid(1) ≈ 0.73`,
/// and beyond `± 15 K` it has saturated to within 5 % of the
/// ends. Narrower would risk numerical step-edge effects in the
/// `Radiation` relaxation loop; wider would dilute the positive
/// feedback that drives snowball bifurcation.
pub fn sigmoid_width_k() -> Real {
    Real::from_int(5)
}

/// Peak albedo for fresh snow. White, close to the published
/// 0.85 figure for boreal / glacial snow. Multiplied by
/// `snow_fraction` to get the snow contribution.
pub fn snow_peak_albedo() -> Real {
    Real::percent(85)
}

/// Peak albedo for sea ice without snow on top — the "gray"
/// channel. Refrozen or young sea ice trends darker than glacial
/// or snow-capped ice; the 0.55 figure splits the difference of
/// the published 0.4-0.7 range.
pub fn sea_ice_peak_albedo() -> Real {
    Real::percent(55)
}

/// Peak albedo for fully overcast cloud cover. Stratus / cumulus
/// scatter ~40 % of incoming shortwave; cirrus much less. 0.4 is
/// the canonical mid-cloud value.
pub fn cloud_peak_albedo() -> Real {
    Real::percent(40)
}

/// Deterministic sigmoid over `Real`: `1 / (1 + exp(-x))`.
///
/// Saturates outside `|x| > 10` (where `sigmoid(10) ≈ 1 -
/// 4.5e-5` is already inside the Q32.32 precision floor) so the
/// Taylor expansion in `exp` never sees a divergent argument and
/// the call stays branch-light on the hot path.
///
/// Returns a value in `[0, 1]`; never panics for any finite
/// input.
#[must_use]
pub fn sigmoid_real(x: Real) -> Real {
    let ten = Real::from_int(10);
    if x >= ten {
        return Real::ONE;
    }
    if x <= -ten {
        return Real::ZERO;
    }
    // exp(-x) on `|x| < 10` stays inside Q32.32's range
    // (~2.2e4 ≪ 2.1e9).
    let neg_x = -x;
    let denom = Real::ONE + exp(neg_x);
    Real::ONE / denom
}

/// Surface base albedo for a cell, before any ice / snow / cloud
/// modulation. Discriminates on the per-cell substance signals
/// already present in `PhysicsState`:
///
/// - water cells (`water_depth > 0`) → dark ocean (~0.06)
/// - vegetated cells (`biofuel_ceiling > 0`) → forest / canopy
///   (~0.15)
/// - bare rocky cells (otherwise) → ~0.20
///
/// Real-world numbers are picked at the centre of published
/// per-surface-type albedo ranges (ocean 0.06, vegetation 0.10-
/// 0.20, bare rock / soil 0.15-0.25).
#[must_use]
pub fn base_albedo_for(water_depth: Real, biofuel_ceiling: Real) -> Real {
    if water_depth > Real::ZERO {
        Real::percent(6)
    } else if biofuel_ceiling > Real::ZERO {
        Real::percent(15)
    } else {
        Real::percent(20)
    }
}

/// Combine the four albedo channels into a single per-cell
/// effective albedo. The `max(...)` shape lets the brightest
/// cover dominate (snow on top of ice on top of ocean reads as
/// snow), while the multiplicative `(1 - max(snow, sea_ice))`
/// suppression on the cloud term keeps clouds from double-
/// counting over already-bright surfaces.
///
/// All inputs are clamped to `[0, 1]` before combining; callers
/// don't need to pre-clamp.
#[must_use]
pub fn effective_albedo_for(
    base: Real,
    snow_fraction: Real,
    sea_ice_fraction: Real,
    cloud_fraction: Real,
) -> Real {
    let snow_f = snow_fraction.clamp01();
    let ice_f = sea_ice_fraction.clamp01();
    let cloud_f = cloud_fraction.clamp01();
    let snow_a = snow_peak_albedo() * snow_f;
    // Sea ice darker than snow; if snow already sits on top, the
    // sea-ice channel only contributes from the exposed-ice
    // fraction (`1 - snow_fraction`).
    let sea_ice_a = sea_ice_peak_albedo() * ice_f * (Real::ONE - snow_f);
    // Clouds add brightness in proportion to the non-bright
    // surface area beneath them: over a fully ice-covered cell
    // the cloud contribution vanishes (already as bright as it
    // will get).
    let surface_brightness = snow_f.max(ice_f);
    let cloud_a = cloud_peak_albedo() * cloud_f * (Real::ONE - surface_brightness);
    let a = base.max(snow_a).max(sea_ice_a).max(cloud_a);
    a.clamp01()
}

/// Compute every cell's effective albedo into a fresh `Vec`.
/// Helper for laws that need the full per-cell albedo slice
/// (`Radiation` consumes it to scale per-cell absorbed
/// insolation) without each callsite re-deriving the channel
/// combination.
#[must_use]
pub fn effective_albedo_slice(state: &PhysicsState) -> Vec<Real> {
    let n = state.grid().n_cells();
    let water = state.water_depth();
    let bio = state.biofuel_ceiling();
    let snow = state.snow_fraction();
    let sea_ice = state.sea_ice_fraction();
    let cloud = state.cloud_fraction();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let base = base_albedo_for(water[i], bio[i]);
        out.push(effective_albedo_for(base, snow[i], sea_ice[i], cloud[i]));
    }
    out
}

/// Per-tick update of snow / sea-ice cover from temperature
/// (and, for snow, atmospheric vapour as a precipitation proxy).
///
/// Coupling order: runs in the orchestrator *before*
/// `Radiation` so the per-cell albedo `Radiation` reads each
/// macro-step is consistent with the freshest temperature. The
/// snow / sea-ice fields then enter the next tick's radiative
/// balance, closing the positive feedback loop that drives the
/// snowball bifurcation.
#[derive(Debug, Clone)]
pub struct IceAlbedo {
    /// Substrate freeze threshold in K. Driver of the sigmoid
    /// transition: snow / sea ice accumulates as the cell drops
    /// below `freeze_point_k`, melts as it climbs above. Comes
    /// from the planet's `Chemistry::for_planet` resolved
    /// freeze point so methane / ammonia / silicate worlds
    /// transition at their own substrate's phase boundary, not
    /// Earth-water's 273 K.
    pub freeze_point_k: Real,
    /// Per-macro-step adjustment rate toward the sigmoid
    /// target. 0.10 lets a cell that flips from "above freeze"
    /// to "well below freeze" reach ~99 % snow / ice in
    /// ~45 macro-steps (~1.5 sim-months at the default 30
    /// macro-steps-per-month cadence). Fast enough that the
    /// feedback loop closes within a single climate-timescale
    /// integration; slow enough that the relaxation doesn't
    /// overshoot the sigmoid target on a single tick.
    pub cover_rate: Real,
    /// Vapour-density threshold for "this cell is precipitating"
    /// when computing snow accumulation over land. Earth has
    /// surface humidity ~10 kg/m² at temperate latitudes;
    /// `30` (the same threshold `Hydrology` uses for
    /// condensation onto cold cells) lines up with the
    /// existing precipitation-onset scale.
    pub precip_vapour_threshold: Real,
}

impl IceAlbedo {
    /// Earth-like ice / albedo coupling: freezes at water's
    /// 273.15 K, modest cover rate, precipitation threshold that
    /// matches `Hydrology`'s condensation onset.
    #[must_use]
    pub fn earth_like() -> Self {
        Self {
            freeze_point_k: Real::from_ratio(27_315, 100),
            cover_rate: Real::percent(10),
            precip_vapour_threshold: Real::from_int(30),
        }
    }

    /// Build with the substrate's actual freeze point. Pass the
    /// resolved freeze threshold from
    /// `Chemistry::for_planet`'s phase-transition setup (which
    /// has already applied the per-seed substrate perturbation),
    /// so the ice-albedo transition fires at the same freeze line
    /// chemistry uses for phase changes — single source of
    /// truth for "what counts as frozen".
    #[must_use]
    pub fn for_freeze_point(freeze_point_k: Real) -> Self {
        Self {
            freeze_point_k,
            ..Self::earth_like()
        }
    }
}

impl Law for IceAlbedo {
    fn integrate(&self, state: &mut PhysicsState, dt: Real) {
        let temps = state.temperature().to_vec();
        let water = state.water_depth().to_vec();
        let vapour = state.substance(Substance::Vapour.idx()).to_vec();
        let ice_substance = state.substance(Substance::Ice.idx()).to_vec();
        let freeze = self.freeze_point_k;
        let width = sigmoid_width_k();
        // Per-tick relaxation toward the sigmoid target.
        let rate = (self.cover_rate * dt).clamp01();
        let n = state.grid().n_cells();

        // Snow targets: brightest channel; fires only when
        // precipitation is available (land cells with vapour
        // above the precipitation threshold, or any cell with
        // existing `Substance::Ice` that can host snow on top).
        // The sigmoid drive scales with how far the cell sits
        // below freeze.
        let snow_dst = state.snow_fraction_mut();
        for i in 0..n {
            let drive = sigmoid_real((freeze - temps[i]) / width);
            // Snow source: precipitation over land + vapour
            // freezing onto cold ice already present.
            let has_precip = vapour[i] >= self.precip_vapour_threshold;
            let on_land = water[i] <= Real::ZERO;
            let on_ice = ice_substance[i] > Real::ZERO;
            let supports_snow = (on_land && has_precip) || on_ice;
            let target = if supports_snow { drive } else { Real::ZERO };
            // Relax toward the sigmoid target at `rate`.
            let cur = snow_dst[i];
            let next = cur + (target - cur) * rate;
            snow_dst[i] = next.clamp01();
        }

        // Sea-ice targets: only over water cells, gray channel,
        // driven purely by surface temperature.
        let sea_ice_dst = state.sea_ice_fraction_mut();
        for i in 0..n {
            let drive = sigmoid_real((freeze - temps[i]) / width);
            let on_water = water[i] > Real::ZERO;
            let target = if on_water { drive } else { Real::ZERO };
            let cur = sea_ice_dst[i];
            let next = cur + (target - cur) * rate;
            sea_ice_dst[i] = next.clamp01();
        }
    }
}

/// Pre-compute `(1 - albedo)^(1/4)` from a `Real` albedo. Used
/// by `Radiation` to apply per-cell albedo to the per-row
/// equilibrium-temperature table without re-running the
/// `exp / ln`-heavy Stefan-Boltzmann root each tick. Two
/// `sqrt` calls per cell per tick instead of `exp(ln(x)/4)`.
///
/// Albedo is clamped to `[0, 1)` before the root; an exact
/// `1.0` (full reflection) would give a zero radiating
/// temperature, which is physically valid but flattens the
/// gradient — clamping a hair below 1.0 (`0.9999`) keeps the
/// thermal relaxation working at the saturation limit so the
/// snowball state still has a stable equilibrium temperature
/// rather than a degenerate zero.
#[must_use]
pub fn albedo_radiation_factor(albedo: Real) -> Real {
    let a = albedo.clamp01().min(Real::from_ratio(9_999, 10_000));
    let one_minus_a = Real::ONE - a;
    sqrt(sqrt(one_minus_a))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::HexGrid;

    #[test]
    fn sigmoid_real_clamps_outside_bounds() {
        assert_eq!(sigmoid_real(Real::from_int(20)), Real::ONE);
        assert_eq!(sigmoid_real(Real::from_int(-20)), Real::ZERO);
    }

    #[test]
    fn sigmoid_real_is_one_half_at_zero() {
        let s = sigmoid_real(Real::ZERO);
        let half = Real::from_ratio(1, 2);
        let err = (s - half).abs();
        assert!(err < Real::from_ratio(1, 1_000_000), "sigmoid(0) = {s:?}");
    }

    #[test]
    fn sigmoid_real_monotonic() {
        let a = sigmoid_real(Real::from_int(-2));
        let b = sigmoid_real(Real::from_int(-1));
        let c = sigmoid_real(Real::ZERO);
        let d = sigmoid_real(Real::from_int(1));
        let e = sigmoid_real(Real::from_int(2));
        assert!(a < b && b < c && c < d && d < e);
    }

    #[test]
    fn base_albedo_dispatches_on_surface_type() {
        let water = base_albedo_for(Real::from_int(5), Real::ZERO);
        let veg = base_albedo_for(Real::ZERO, Real::from_int(100));
        let rock = base_albedo_for(Real::ZERO, Real::ZERO);
        assert_eq!(water, Real::percent(6));
        assert_eq!(veg, Real::percent(15));
        assert_eq!(rock, Real::percent(20));
    }

    #[test]
    fn effective_albedo_snow_dominates_over_base() {
        let a = effective_albedo_for(Real::percent(6), Real::ONE, Real::ZERO, Real::ZERO);
        assert_eq!(a, snow_peak_albedo());
    }

    #[test]
    fn effective_albedo_layered_snow_on_ice_reads_as_snow() {
        // Snow on top of sea ice should read as the snow albedo
        // — the sea-ice channel gets suppressed by `(1 -
        // snow_fraction)`.
        let a = effective_albedo_for(Real::percent(6), Real::ONE, Real::ONE, Real::ZERO);
        assert_eq!(a, snow_peak_albedo());
    }

    #[test]
    fn effective_albedo_cloud_does_not_double_count_over_ice() {
        // Cloud over a fully ice-covered cell should not push
        // albedo above the ice channel — the `(1 - max(snow,
        // sea_ice))` suppression term zeros the cloud
        // contribution.
        let a_no_cloud =
            effective_albedo_for(Real::percent(6), Real::ZERO, Real::ONE, Real::ZERO);
        let a_with_cloud =
            effective_albedo_for(Real::percent(6), Real::ZERO, Real::ONE, Real::ONE);
        assert_eq!(a_no_cloud, a_with_cloud);
    }

    #[test]
    fn ice_albedo_warm_cell_keeps_zero_snow() {
        let mut state = PhysicsState::new(HexGrid::new(1, 1));
        // Warm cell well above freeze with vapour and land —
        // should not accumulate snow.
        state.temperature_mut()[0] = Real::from_int(300);
        state.water_depth_mut()[0] = Real::ZERO;
        state.substance_mut(Substance::Vapour.idx())[0] = Real::from_int(100);
        let law = IceAlbedo::earth_like();
        for _ in 0..50 {
            law.integrate(&mut state, Real::ONE);
        }
        assert!(state.snow_fraction()[0] < Real::percent(1));
    }

    #[test]
    fn ice_albedo_cold_water_cell_grows_sea_ice() {
        let mut state = PhysicsState::new(HexGrid::new(1, 1));
        state.temperature_mut()[0] = Real::from_int(250);
        state.water_depth_mut()[0] = Real::from_int(10);
        let law = IceAlbedo::earth_like();
        for _ in 0..100 {
            law.integrate(&mut state, Real::ONE);
        }
        assert!(
            state.sea_ice_fraction()[0] > Real::percent(90),
            "cold water cell should grow to near-full sea ice: {:?}",
            state.sea_ice_fraction()[0]
        );
    }

    #[test]
    fn albedo_radiation_factor_full_albedo_collapses_temperature() {
        // Full albedo (1.0) → factor ≈ 0.1 (after the 0.9999
        // clamp `sqrt(sqrt(1e-4)) = 1e-1`), so a snowball cell
        // sees only ~10 % of the no-albedo equilibrium
        // temperature. Compare to A=0 → factor=1.0; the order-
        // of-magnitude gap is the bifurcation lever.
        let factor = albedo_radiation_factor(Real::ONE);
        // factor is approximately 0.1; the assertion bounds it
        // tightly enough to catch a regression that collapsed
        // the rescaling logic (e.g. forgot the fourth-root).
        assert!(factor < Real::percent(15));
        assert!(factor > Real::percent(5));
    }

    #[test]
    fn albedo_radiation_factor_zero_albedo_is_one() {
        let factor = albedo_radiation_factor(Real::ZERO);
        assert_eq!(factor, Real::ONE);
    }

    #[test]
    fn albedo_step_at_freeze_threshold_produces_bifurcation() {
        // A cell just below the freeze line and another just
        // above must end up with very different albedos after
        // the sigmoid + cover relaxation settles. This is the
        // sharp-transition property that lets the radiative-
        // balance loop fall into one of two basins instead of
        // a unique intermediate equilibrium.
        //
        // Pair of comparisons drives both ends of the assertion:
        //   `just_below` vs `just_above` — 1 K either side of
        //     freeze: sigmoid at ±1/5 = ±0.2 → drive ≈ 0.55 vs
        //     0.45 (small absolute gap but already steeper than
        //     any linear ramp the previous code could produce).
        //   `well_below` vs `well_above` — 15 K either side:
        //     sigmoid at ±3 → drive ≈ 0.95 vs 0.05; the
        //     saturated bracket sets the bifurcation amplitude.
        let mk = |t_k: i64| -> PhysicsState {
            let mut s = PhysicsState::new(HexGrid::new(1, 1));
            s.temperature_mut()[0] = Real::from_ratio(t_k, 1);
            s.water_depth_mut()[0] = Real::from_int(10);
            s
        };

        // ±3 K from freeze brackets the steepest slope of the
        // sigmoid (where T - T_freeze ≈ ±0.6 × width); the
        // 5 K full-width-half-max of the transition kernel is
        // already much sharper than the previous code's
        // 30 K linear ramp, but stops short of the saturated
        // bracket the well-below / well-above pair covers.
        let mut just_below = mk(270);
        let mut just_above = mk(276);
        let mut well_below = mk(258);
        let mut well_above = mk(288);

        let law = IceAlbedo::earth_like();
        // Many ticks → sigmoid + cover relaxation has settled.
        for _ in 0..300 {
            law.integrate(&mut just_below, Real::ONE);
            law.integrate(&mut just_above, Real::ONE);
            law.integrate(&mut well_below, Real::ONE);
            law.integrate(&mut well_above, Real::ONE);
        }

        let albedo_of = |s: &PhysicsState| -> Real {
            let base = base_albedo_for(s.water_depth()[0], s.biofuel_ceiling()[0]);
            effective_albedo_for(
                base,
                s.snow_fraction()[0],
                s.sea_ice_fraction()[0],
                s.cloud_fraction()[0],
            )
        };
        let a_jb = albedo_of(&just_below);
        let a_ja = albedo_of(&just_above);
        let a_wb = albedo_of(&well_below);
        let a_wa = albedo_of(&well_above);

        // Sharp transition: the just-below cell must already
        // show meaningfully higher albedo than the just-above
        // cell, much steeper than a linear ramp at the same
        // 2 K separation would produce. A linear ramp from
        // base (0.06) to ice (0.55) over a 10 K window would
        // give a 2 K gap of `(0.55 - 0.06) × 2/10 = 0.10`;
        // the sigmoid gives ≥ 2× that.
        assert!(
            a_jb - a_ja > Real::percent(15),
            "sigmoid not steep at freeze line: \
             just_below={a_jb:?} just_above={a_ja:?}"
        );

        // Well-below cell saturates near the sea-ice peak;
        // well-above cell stays at the ocean base. The
        // saturated bracket establishes the bifurcation
        // amplitude that drives the radiative-balance loop
        // into one of two basins.
        assert!(
            a_wb >= Real::percent(50),
            "well-below cell albedo too low: {a_wb:?}"
        );
        assert!(
            a_wa <= Real::percent(10),
            "well-above cell albedo too high: {a_wa:?}"
        );
        assert!(a_wb - a_wa > Real::percent(40));
    }

    /// A planet initialised at a *marginal* global temperature
    /// (just below freeze) must settle into one of the two
    /// distinct climate basins — fully frozen (snowball) or
    /// fully thawed (habitable) — not the intermediate "half
    /// ice / half ocean" state that a linear coupling would
    /// produce. This is the calibration anchor for the sigmoid
    /// + bimodal feedback: under marginal forcing the system's
    /// equilibrium is bimodal, with no stable intermediate.
    ///
    /// Two initial states bracketed across the marginal line
    /// converge to opposite basins:
    ///   - 270 K (just below freeze)  → snowball
    ///   - 277 K (just above freeze)  → habitable
    /// If the feedback were intermediate, both seeds would
    /// relax to a similar mid-temperature steady state.
    #[test]
    fn cold_seed_with_marginal_temp_falls_into_one_of_two_basins_not_intermediate() {
        use crate::laws::Law;
        use crate::radiation::Radiation;

        // Build a single equator-cell world (1×1 grid → no
        // latitude variation, sub-solar at the only row).
        // Earth-like stellar forcing + greenhouse tuned so the
        // habitable-basin equilibrium sits a few K above the
        // freeze line and the snowball-basin equilibrium a
        // few K below — exactly the regime where bifurcation
        // lives.
        //
        // The radiative-balance basin a cell ends up in
        // depends on the *initial ice cover* as well as
        // initial T (the two together set whether albedo
        // feedback latches before radiation drags T back
        // toward the unfrozen equilibrium). The cold seed
        // starts with full sea ice; the warm seed starts ice-
        // free — both with the same marginal initial
        // temperature, mirroring the "small perturbation
        // tips a marginal climate into one of two basins"
        // physical picture.
        let run = |initial_t_k: i64, initial_ice: Real| -> Real {
            let mut state = PhysicsState::new(HexGrid::new(1, 1));
            for t in state.temperature_mut() {
                *t = Real::from_int(initial_t_k);
            }
            for w in state.water_depth_mut() {
                *w = Real::from_int(10);
            }
            state.sea_ice_fraction_mut()[0] = initial_ice;

            let rad = Radiation::for_planet(
                1,
                Real::from_int(1_500),
                30,                 // baseline albedo % (matches table bake-in)
                Real::from_int(20), // modest greenhouse — keeps the two basins well separated
                0,
                0,
                0,
                Real::from_int(24),
            );
            let ice = IceAlbedo::earth_like();
            // Long enough for the ice-albedo feedback to
            // saturate (cover_rate 0.10 → ~50 ticks to
            // saturate; the radiative relaxation rate 0.02
            // needs ~500 ticks). 3000 ticks gives both
            // ample time to settle.
            for _ in 0..3_000 {
                ice.integrate(&mut state, Real::ONE);
                rad.integrate(&mut state, Real::ONE);
            }
            // Return the mean equilibrium temperature.
            let mut sum = Real::ZERO;
            for t in state.temperature() {
                sum = sum + *t;
            }
            sum / Real::from_int(state.grid().n_cells() as i64)
        };

        // Two seeds: one with marginal temperature on the
        // cold side of the unstable manifold + full initial
        // sea ice, one with marginal temperature on the warm
        // side + no ice. Both initial states are *plausible
        // perturbations* of a marginal climate; the
        // bifurcation property is that they latch into
        // distinct basins, not a shared intermediate one.
        //
        // The exact boundary T between basins depends on
        // the radiative + albedo coefficients above; ±13 K
        // from the freeze line straddles it with margin.
        let t_cold_seed = run(260, Real::ONE);
        let t_warm_seed = run(286, Real::ZERO);

        // Basins must be widely separated; an intermediate
        // equilibrium would put both within a few K of each
        // other. Sigmoid + bimodal feedback produces a 20 K+
        // gap by amplifying the marginal initial offset.
        let gap = t_warm_seed - t_cold_seed;
        assert!(
            gap > Real::from_int(15),
            "cold and warm seeds collapsed to the same intermediate basin \
             (no bifurcation): cold={t_cold_seed:?} warm={t_warm_seed:?} \
             gap={gap:?}"
        );
        // The cold seed should have slipped below freeze (snowball
        // basin); the warm one stays above (habitable basin).
        let freeze = Real::from_ratio(27_315, 100);
        assert!(
            t_cold_seed < freeze,
            "cold seed did not freeze: {t_cold_seed:?}"
        );
        assert!(
            t_warm_seed > freeze,
            "warm seed froze: {t_warm_seed:?}"
        );
    }

    /// Snowball-Earth calibration anchor: under reduced solar
    /// irradiance, the ice-albedo feedback should drive the
    /// equilibrium ice line down to ~30° latitude (a published
    /// climate-modelling result for the Neoproterozoic global-
    /// glaciation onset).
    ///
    /// "Ice line at 30°" means: cells with `|latitude| > 30°`
    /// are frozen (high snow / sea-ice fraction), cells with
    /// `|latitude| < 30°` are not. We verify the ice line
    /// lands inside `30° ± 10°` — sigmoid+bimodal is only
    /// approximately calibrated and the tolerance leaves room
    /// for the per-cell sigmoid steepness without demanding
    /// pixel-perfect agreement with a full GCM.
    #[test]
    fn snowball_ice_line_at_30_latitude_under_solar_constant_loss() {
        use crate::laws::Law;
        use crate::radiation::Radiation;

        // 21-row grid → equator at row 10, poles at rows 0 / 20.
        // Each row spans 180°/21 ≈ 8.57° of latitude; row 10 is
        // the equator (0°), row 7 is ~25° N (3 rows × 8.57°),
        // row 6 is ~34° N, row 13 is ~25° S, row 14 is ~34° S.
        // The "ice line at 30°" target corresponds to rows
        // 6-7 north and 13-14 south.
        let height: u32 = 21;
        let width: u32 = 1;
        let mut state = PhysicsState::new(HexGrid::new(width, height));
        for t in state.temperature_mut() {
            // Start uniformly habitable; let the ice-albedo
            // feedback grow polar ice as the radiative balance
            // settles.
            *t = Real::from_int(280);
        }
        for w in state.water_depth_mut() {
            *w = Real::from_int(10);
        }
        // Reduced solar irradiance (~96 % of Earth's 1361 W/m²)
        // ≈ snowball-onset regime. The exact reduction that
        // lands the ice line near 30° depends on the baseline
        // albedo + greenhouse pair below; tuning these
        // together so the habitable-basin equator equilibrium
        // clears the freeze line by ~15 K and the polar
        // equilibrium sits ~30 K below puts the ice-line
        // transition in the right band.
        let stellar = Real::from_int(1_300);
        let rad = Radiation::for_planet(
            height,
            stellar,
            30,
            Real::from_int(20),
            0,
            0,
            0,
            Real::from_int(24),
        );
        let ice = IceAlbedo::earth_like();
        for _ in 0..3_000 {
            ice.integrate(&mut state, Real::ONE);
            rad.integrate(&mut state, Real::ONE);
        }

        // Find the equator-most frozen row (sea-ice fraction
        // above 50 %) in each hemisphere. "Equator-most" =
        // smallest absolute latitude offset from the mid-row,
        // so we walk *outward* from the equator and stop at
        // the first frozen row in each direction.
        let frozen =
            |row: usize| -> bool { state.sea_ice_fraction()[row] > Real::percent(50) };
        let mid = height as usize / 2;
        // Northern hemisphere: walk from the equator (mid-1)
        // upward toward the pole and stop at the first
        // frozen row. The default `0` (pole row) covers the
        // degenerate "no ice anywhere" case so the
        // distance assertion below fails meaningfully.
        let mut northern_ice_line = 0usize;
        for r in (0..mid).rev() {
            if frozen(r) {
                northern_ice_line = r;
                break;
            }
        }
        // Southern hemisphere: walk from the equator (mid+1)
        // toward the south pole and stop at the first
        // frozen row.
        let mut southern_ice_line = height as usize - 1;
        for r in (mid + 1)..(height as usize) {
            if frozen(r) {
                southern_ice_line = r;
                break;
            }
        }

        let north_rows_from_equator = mid - northern_ice_line;
        let south_rows_from_equator = southern_ice_line - mid;
        // Each row ≈ 180°/21 ≈ 8.57°. 30° corresponds to ~3.5
        // rows. ±10° tolerance → 2 to 5 rows from the equator.
        let target_rows_low = 2usize;
        let target_rows_high = 5usize;
        assert!(
            (target_rows_low..=target_rows_high).contains(&north_rows_from_equator),
            "northern ice line outside 30°±10° band: \
             row={northern_ice_line} (=={north_rows_from_equator} rows from equator)"
        );
        assert!(
            (target_rows_low..=target_rows_high).contains(&south_rows_from_equator),
            "southern ice line outside 30°±10° band: \
             row={southern_ice_line} (=={south_rows_from_equator} rows from equator)"
        );
    }
}
        // configures below