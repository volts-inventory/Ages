//! Sprint 5 Item 16 (v2) — tidal heating from eccentric moon orbits.
//!
//! Sprint 5 Item 16 v1 shipped a wrong formula; this module is the
//! astro-feedback-driven rewrite using the textbook expression for
//! the heat dissipated by a moon's eccentric orbit raising
//! tides on its host planet.
//!
//! ## Formula
//!
//! ```text
//! H = (21/2) × (k₂/Q) × R⁵ × n⁵ × e² / G
//! ```
//!
//! where
//! - `k₂` is the moon's tidal Love number (~0.3 for rocky bodies),
//! - `Q` is the tidal quality factor (~100 for rocky, ~1000 for icy),
//! - `R` is the body radius,
//! - `n = 2π / orbital_period` is the orbital mean motion,
//! - `e` is the orbital eccentricity, and
//! - `G` is the gravitational constant (here folded into a fixed
//!   calibration normaliser — see `cal_factor()` below).
//!
//! The derivation collapses the standard
//! `(21/2)(k₂/Q) × G × M_p² × R⁵ × n × e² / a⁶` form via Kepler's
//! third law (`n² = G M_p / a³`) so the dependence on the host mass
//! `M_p` and semi-major axis `a` is replaced by `n⁵ / G`. This is
//! the form referenced by the astro-feedback note that triggered the
//! rewrite.
//!
//! ## Calibration
//!
//! Fixed-point Q32.32 can't represent SI radii (~10⁶ m) and SI
//! angular velocities (~10⁻⁵ rad/s) directly — `R⁵` alone overflows
//! at SI units while `n⁵` underflows below the LSB. We therefore
//! compute the formula in `Real`-natural units:
//!
//! - `R` in Earth-radii (Earth = 1),
//! - `n` in radians per macro-step (1 macro-step ≈ 1 sim-day), and
//! - `e` dimensionless in `[0, 1)`.
//!
//! and absorb the SI dimensional constants into a single `cal_factor`
//! such that an Io-like configuration (R = 0.286 Earth-radii,
//! e = 0.0041, orbital period 1.77 days, rocky substrate) lands
//! in the [50, 200] TW range — the calibration anchor from the
//! `io_like_configuration_global_heat_flux_in_50_to_200_tw_range`
//! test. The output is in **terawatts (TW)**.
//!
//! ## Coupling
//!
//! Wired into `orchestration::integrate_civ_step` after the lunar-
//! tide bulge displacement (`Tides`) and before chemistry — the heat
//! source the eccentric orbit dumps into the moon is the same
//! tidal-friction term that drives the bulge; chemistry then runs
//! against the post-heating temperature state.
//!
//! Couples to `sim-world::tidal_locking` (Item 19): a
//! `LockingState::Synchronous` planet's `step_eccentricity_damping`
//! drains `Moon::eccentricity` toward zero, so locked moons in
//! circular orbits produce zero heat (`circular_orbit_moon_produces_zero_tidal_heating`).
//! A `Resonance { p, q }` planet's eccentricity is *not* damped,
//! sustaining steady-state heat (the Io-Europa-Ganymede mechanism).
//!
//! ## Distribution
//!
//! `distribute_heat_to_cells` spreads the per-moon heat rate uniformly
//! across the planet's grid. Real Io concentrates tidal stress at
//! mid-latitudes where the bulge shears against the rotating crust;
//! we leave the spatial profile uniform for now. The TODO ladder
//! lists "concentrate heat at tidal-stress hot spots" as a future
//! pass; the call signature is stable so that future refinement is
//! local to this module.

use crate::state::PhysicsState;
use sim_arith::transcendental::two_pi;
use sim_arith::Real;

/// Tidal Love number `k₂` for rocky bodies. Earth ≈ 0.299; Mercury ≈ 0.45.
/// Spec anchors at 0.3 for rocky substrates.
///
/// `k₂` quantifies how much the moon deforms in response to the host
/// planet's tidal stress: 0 = perfectly rigid (no deformation, no
/// heating), 3/2 = perfectly fluid (the upper bound).
#[inline]
pub fn love_number_rocky() -> Real {
    Real::from_ratio(3, 10)
}

/// Tidal quality factor `Q` for rocky bodies. Earth ≈ 12 (very
/// dissipative due to oceans), Moon ≈ 30, Mars ≈ 80; the canonical
/// "rocky" anchor for tidal-heating problems is Q ≈ 100.
///
/// High Q = low dissipation; the (k₂/Q) ratio is what enters the
/// formula. The factor of 100 corresponds to a rocky body that lags
/// a tidal bulge by a few degrees per orbit — Io's published
/// effective Q is ~100.
#[inline]
pub fn q_factor_rocky() -> Real {
    Real::from_int(100)
}

/// Tidal quality factor `Q` for icy bodies. Europa-class icy moons
/// dissipate an order of magnitude less than rocky bodies — water
/// ice flows enough to relax shear, but slowly. Spec anchors at
/// Q ≈ 1000.
#[inline]
pub fn q_factor_icy() -> Real {
    Real::from_int(1_000)
}

/// `k₂ / Q` for a rocky substrate. The dimensionless dissipation
/// coefficient that actually enters the formula. For Earth-ish
/// rocky moons: 0.3 / 100 = 0.003. Used by the default
/// `MoonHeating::rocky` constructor and by the
/// `io_like_configuration_*` calibration test.
#[inline]
#[must_use]
pub fn k2_over_q_rocky() -> Real {
    love_number_rocky() / q_factor_rocky()
}

/// `k₂ / Q` for an icy substrate. 0.3 / 1000 = 0.0003. Europa-class
/// moons dissipate an order of magnitude less heat per orbit than
/// rocky bodies of the same R, e, and n.
#[inline]
#[must_use]
pub fn k2_over_q_icy() -> Real {
    love_number_rocky() / q_factor_icy()
}

/// Calibration multiplier that absorbs the SI-dimensional constants
/// (`1/G` and the radius/period unit conversions) into a single Real.
///
/// Derivation: the dimensional formula in SI yields a heat rate
/// `H [W] = (21/2)(k₂/Q) × R⁵[m⁵] × n⁵[rad/s⁵] × e² / G[m³/(kg·s²)]`.
/// We input R in Earth-radii, n in rad-per-macro-step, and want
/// output in TW (= 10¹² W). Working through Io as the calibration
/// anchor:
///
/// - R = 0.286 Earth-radii → R⁵ ≈ 0.001914 (Earth-radii)⁵
/// - n = 2π / 1.77 ≈ 3.55 rad/macro-step → n⁵ ≈ 564.5
/// - e = 0.0041 → e² ≈ 1.681e-5
/// - k₂/Q = 0.003
///
/// Pre-calibration product: 10.5 × 0.003 × 0.001914 × 564.5 × 1.681e-5
/// ≈ 5.72e-7. Real Io heat ≈ 100 TW, so `cal_factor` ≈ 1.75e8 to
/// land the calibration test inside [50, 200] TW. We pick
/// `cal_factor = 1.75e8` exactly (= 175_000_000); a future re-anchor
/// against a different reference body (Enceladus, Europa) would tune
/// this constant — the test that pins it allows a 4× range so the
/// number is a working estimate, not a finely-tuned constant.
#[inline]
fn cal_factor() -> Real {
    Real::from_int(175_000_000)
}

/// Per-moon heating descriptor — the physics-layer projection of
/// `sim_world::composition::Moon` for the tidal-heating law.
///
/// `sim-physics` deliberately doesn't depend on `sim-world` (the
/// crate-dependency graph runs the other way), so the orchestrator
/// in `sim-core` adapts a `&Moon` into a `MoonHeating` the same way
/// it adapts each moon into a `MoonTide` for the lunar-bulge law.
/// See `sim_core::laws::build_*` for the worldgen-side adapter.
#[derive(Debug, Clone, Copy)]
pub struct MoonHeating {
    /// Orbital eccentricity `e` in `[0, 1)`. Drives the `e²` term;
    /// `e = 0` circular orbits produce zero heat by construction.
    pub eccentricity: Real,
    /// Orbital period in macro-steps. Earth's moon ≈ 28; Io ≈ 1.77.
    /// `n = 2π / period_macros` is the mean motion.
    pub orbital_period_macros: u32,
    /// Dissipation coefficient `k₂ / Q`. Rocky ≈ 0.003 (Earth,
    /// Io); icy ≈ 0.0003 (Europa-class). `MoonHeating::rocky` /
    /// `MoonHeating::icy` are the substrate-default constructors.
    pub k2_over_q: Real,
}

impl MoonHeating {
    /// Rocky-substrate moon: `k₂/Q = 0.3/100 = 0.003`. Matches Io,
    /// Earth's Moon, Mars's moons.
    #[must_use]
    pub fn rocky(eccentricity: Real, orbital_period_macros: u32) -> Self {
        Self {
            eccentricity,
            orbital_period_macros,
            k2_over_q: k2_over_q_rocky(),
        }
    }

    /// Icy-substrate moon: `k₂/Q = 0.3/1000 = 0.0003`. Matches
    /// Europa, Enceladus, Titan-class icy moons.
    #[must_use]
    pub fn icy(eccentricity: Real, orbital_period_macros: u32) -> Self {
        Self {
            eccentricity,
            orbital_period_macros,
            k2_over_q: k2_over_q_icy(),
        }
    }
}

/// Tidal heat dissipated by one eccentric moon orbit, in TW.
///
/// Implements the corrected Sprint 5 Item 16 formula
/// `H = (21/2) × (k₂/Q) × R⁵ × n⁵ × e² / G` (the v1 implementation's
/// formula was wrong; astro feedback identified the right
/// closed form and this is the rewrite).
///
/// `planet_radius_earth_units` is the moon's *body* radius in
/// Earth-radii — the deforming body in `R⁵` is the moon itself
/// (where the heat is dissipated), not the host planet. The
/// parameter name uses "planet_radius" only for consistency with
/// the spec wording in `docs/implementation-plan.md` Item 16; the
/// physical interpretation is "the radius of the body that's being
/// flexed and heated." For the Io-Jupiter case the heating body is
/// Io, whose R ≈ 0.286 Earth-radii.
///
/// Returns `Real::ZERO` immediately for circular orbits (`e = 0`)
/// and for degenerate input (`orbital_period_macros = 0`) — both
/// would otherwise multiply through to zero anyway, but the early
/// returns make the intent explicit and skip the
/// `n = 2π / period` divide-by-zero.
///
/// ## Numerical order
///
/// The multiplications are ordered to keep every intermediate
/// product inside Q32.32's representable range (`~2.3e-10` LSB,
/// `~2.1e9` ceiling). Specifically `n⁵ × R⁵ × e²` is computed
/// first — `n⁵` is large (~564 for Io) and `R⁵ × e²` is small
/// (~3e-8), but the *order* keeps the partial products bounded:
/// `n⁵ × R⁵` (~1.08), then `× e²` (~1.8e-5), then `× k₂/Q × (21/2)`
/// (~5.7e-7), then `× cal_factor` (~100 TW for Io).
#[must_use]
pub fn moon_tidal_heat_rate(planet_radius_earth_units: Real, moon: &MoonHeating) -> Real {
    if moon.eccentricity == Real::ZERO {
        // Circular orbit dissipates no tidal heat by construction.
        // `e² = 0` would carry through anyway; the early return
        // skips the trig + multiplies and makes the test
        // `circular_orbit_moon_produces_zero_tidal_heating` an
        // exact bit-zero comparison rather than a tolerance check.
        return Real::ZERO;
    }
    if moon.orbital_period_macros == 0 {
        // Degenerate input — would otherwise divide by zero when
        // computing `n = 2π / period`. Treated as no orbit, no heat.
        return Real::ZERO;
    }

    // n = 2π / period (radians per macro-step). Period clamped to
    // >= 1 above; the divide is safe.
    let period = Real::from_int(i64::from(moon.orbital_period_macros));
    let n = two_pi() / period;

    // R⁵ and n⁵ by repeated multiplication. `pow(R, 5)` would
    // route through ln/exp (~30 ULPs of round-off); the direct
    // chain keeps the precision tight and avoids the
    // ln-of-non-positive panic guard.
    let r = planet_radius_earth_units;
    let r2 = r * r;
    let r4 = r2 * r2;
    let r5 = r4 * r;

    let n2 = n * n;
    let n4 = n2 * n2;
    let n5 = n4 * n;

    let e2 = moon.eccentricity * moon.eccentricity;

    // Order: n5 × r5 first (~1.08 for Io — safely O(1)), then
    // × e² (~1.8e-5), then × k₂/Q × (21/2), finally × cal_factor
    // to land in TW. See module-level numerical-order note.
    let n5r5 = n5 * r5;
    let n5r5e2 = n5r5 * e2;
    let twenty_one_halves = Real::from_ratio(21, 2);
    let coeff = twenty_one_halves * moon.k2_over_q;
    let pre_cal = n5r5e2 * coeff;
    pre_cal * cal_factor()
}

/// Distribute a total heat dissipation rate (in TW) uniformly across
/// every cell's temperature field.
///
/// `total_heat_tw` is the sum of `moon_tidal_heat_rate` over every
/// moon orbiting the planet (in TW). The "distribution" here is
/// uniform: every cell gets `total_heat_tw × heat_to_kelvin / n_cells`
/// added to its temperature each call. The `heat_to_kelvin`
/// conversion factor (`1e-6`) is chosen so that 100 TW of Io-scale
/// heating raises a typical 1000-cell grid by ~1e-4 K per call —
/// a modest perturbation comparable to radiative-balance per-step
/// nudges, well below the inter-tick variability of the equilibrium
/// solver.
///
/// Future passes (the TODO ladder calls out "concentrate heat at
/// tidal-stress hot spots") would replace the uniform distribution
/// with a latitude- and longitude-weighted profile; this signature
/// is stable so that refinement is internal to the module.
pub fn distribute_heat_to_cells(state: &mut PhysicsState, total_heat_tw: Real) {
    if total_heat_tw == Real::ZERO {
        return;
    }
    let n_cells = state.grid().n_cells();
    if n_cells == 0 {
        return;
    }
    // 1 TW spread over the planet raises temperature by tiny
    // amounts per macro-step; the conversion factor is tuned so
    // Io-scale heating produces a modest perturbation rather than
    // an unphysical thermal blowout. With cal_factor land at
    // ~100 (TW), n_cells ~ 100-1000, and heat_to_kelvin = 1e-6,
    // the per-cell delta is ~1e-7 K per macro-step — same order as
    // radiation's per-step nudges.
    let heat_to_kelvin = Real::from_ratio(1, 1_000_000);
    let per_cell = total_heat_tw * heat_to_kelvin
        / Real::from_int(i64::try_from(n_cells).unwrap_or(1).max(1));
    for t in state.temperature_mut() {
        *t = *t + per_cell;
    }
}

/// Convenience: sum `moon_tidal_heat_rate` over every moon orbiting
/// the planet and apply `distribute_heat_to_cells` once. This is
/// the orchestration hook — `sim_core::laws::build_*` constructs a
/// `Vec<MoonHeating>` from `Planet::moons` (mirroring the
/// `MoonTide` adapter pattern), passes the planet's radius alongside,
/// and the orchestrator calls this once per macro-step between
/// `Tides` and `Chemistry`.
///
/// Returns the total dissipation rate in TW so callers / tests can
/// inspect the per-tick budget without re-summing.
pub fn apply_tidal_heating(
    state: &mut PhysicsState,
    planet_radius_earth_units: Real,
    moons: &[MoonHeating],
) -> Real {
    let mut total = Real::ZERO;
    for m in moons {
        total = total + moon_tidal_heat_rate(planet_radius_earth_units, m);
    }
    distribute_heat_to_cells(state, total);
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::HexGrid;

    /// Item 16 spec test #1 — a moon with eccentricity zero
    /// (perfectly circular orbit) dissipates zero tidal heat by
    /// construction. The formula's `e²` factor would land here at
    /// exactly zero anyway; the early return makes the comparison
    /// bit-exact.
    #[test]
    fn circular_orbit_moon_produces_zero_tidal_heating() {
        let moon = MoonHeating::rocky(Real::ZERO, 28);
        let h = moon_tidal_heat_rate(Real::ONE, &moon);
        assert_eq!(
            h,
            Real::ZERO,
            "circular orbit (e=0) must produce zero tidal heat: got {h:?}"
        );
        // And the convenience `apply_tidal_heating` agrees — total
        // returned is zero, and per-cell temperature stays unchanged.
        let grid = HexGrid::new(8, 4);
        let mut state = PhysicsState::new(grid);
        for t in state.temperature_mut() {
            *t = Real::from_int(288);
        }
        let total = apply_tidal_heating(&mut state, Real::ONE, &[moon]);
        assert_eq!(total, Real::ZERO);
        for t in state.temperature() {
            assert_eq!(*t, Real::from_int(288));
        }
    }

    /// Item 16 spec test #2 — Io-like configuration produces a
    /// global heat flux in the [50, 200] TW range. The calibration
    /// anchor: Io's measured tidal heat is ~100 TW; the spec
    /// tolerates a 4× window to leave room for the unit conversion
    /// factor `cal_factor` to be tuned against future reference
    /// bodies (Europa, Enceladus) without invalidating this test.
    ///
    /// Inputs match the spec's Io anchor:
    ///   R = 0.286 Earth-radii, e = 0.0041, period ≈ 1.77 days
    ///   (~2 macro-steps), rocky substrate (k₂/Q = 0.003).
    #[test]
    fn io_like_configuration_global_heat_flux_in_50_to_200_tw_range() {
        let r_io_earth_units = Real::from_ratio(286, 1_000); // 0.286
        // Io's eccentricity 0.0041 → ratio 41 / 10_000.
        let e_io = Real::from_ratio(41, 10_000);
        // Io's orbital period is 1.77 days; the macro-step cadence
        // is 1 macro-step ≈ 1 day, so period_macros = 1.77 — but
        // u32 can't carry the fractional part, so we approximate
        // via the integer floor (2) and absorb the 13% difference
        // into the [50, 200] TW window. A future macro-step
        // refinement that supports sub-day periods would pass a
        // `Real::from_ratio(177, 100)` directly to the rate fn.
        let period_macros: u32 = 2;
        let moon = MoonHeating::rocky(e_io, period_macros);
        let h_tw = moon_tidal_heat_rate(r_io_earth_units, &moon);
        // Real::from_int comparisons. 50 TW ≤ h ≤ 200 TW.
        let lo = Real::from_int(50);
        let hi = Real::from_int(200);
        assert!(
            h_tw >= lo && h_tw <= hi,
            "Io-like heat rate must fall in [50, 200] TW (real Io ≈ 100 TW); \
             got {h_tw:?} (cal_factor or unit conversion may need re-tuning)"
        );
    }

    /// Sanity: doubling eccentricity quadruples heat (since `H ∝ e²`).
    /// Confirms the `e²` term is wired the right way and isn't
    /// e.g. linear.
    #[test]
    fn heat_scales_with_eccentricity_squared() {
        let r = Real::from_ratio(286, 1_000);
        let small = MoonHeating::rocky(Real::from_ratio(1, 100), 2);
        let large = MoonHeating::rocky(Real::from_ratio(2, 100), 2);
        let h_small = moon_tidal_heat_rate(r, &small);
        let h_large = moon_tidal_heat_rate(r, &large);
        // h_large / h_small should equal 4 (within fixed-point
        // round-off). Use the cross-multiply form so we don't divide
        // by a small Real.
        let four_h_small = h_small + h_small + h_small + h_small;
        let diff = (h_large - four_h_small).abs();
        // Tolerance is a relative-error bound. The Q32.32 chain has
        // a ~1e-4 relative round-off accumulating across the e²,
        // n⁵, R⁵, and cal-factor multiplies (each multiply can lose
        // up to 1 LSB on its 32-bit fractional part, so a 5-step
        // chain on values of magnitude ~1e3 lands with ~1e-3 absolute
        // drift on the ~1e3 output). We assert the result is within
        // 1% of 4× to catch a sign-error or wrong-power regression
        // while tolerating the fixed-point drift.
        let one_pct = h_large / Real::from_int(100);
        assert!(
            diff < one_pct,
            "H must scale as e²: 4 × H(e) = {four_h_small:?}, H(2e) = {h_large:?}, diff = {diff:?}, tol = {one_pct:?}"
        );
    }

    /// Sanity: a rocky moon dissipates 10× more than an icy moon
    /// at the same R, e, and period. `k₂/Q` for rocky / icy is
    /// `0.003 / 0.0003 = 10`.
    #[test]
    fn rocky_substrate_dissipates_ten_times_more_than_icy() {
        let r = Real::ONE;
        let e = Real::from_ratio(5, 100);
        let rocky = MoonHeating::rocky(e, 28);
        let icy = MoonHeating::icy(e, 28);
        let h_r = moon_tidal_heat_rate(r, &rocky);
        let h_i = moon_tidal_heat_rate(r, &icy);
        // h_r / h_i ≈ 10 → h_r ≈ 10 × h_i.
        let ten_hi = h_i * Real::from_int(10);
        let diff = (h_r - ten_hi).abs();
        // 2% relative tolerance — Q32.32 round-off across the
        // five-step product chain accumulates a few LSBs of
        // absolute drift. The dominant error here is the `k₂/Q` ratio
        // itself: `0.3 / 1000` in fixed-point lands at ~1288490 ULPs,
        // not the mathematically-exact 0.0003 (which isn't
        // representable). That ~0.000_001 truncation propagates
        // into a ~1% drift on the final 10× ratio. A 2% bound is
        // loose enough to absorb that and tight enough to catch a
        // regression that swaps the substrate ratio entirely (the
        // wrong-direction mistake would put the ratio at 100×, far
        // outside this window).
        let two_pct = h_r / Real::from_int(50);
        assert!(
            diff < two_pct,
            "rocky should be ~10x icy: h_r = {h_r:?}, h_i = {h_i:?}, 10·h_i = {ten_hi:?}, diff = {diff:?}, tol = {two_pct:?}"
        );
    }

    /// `distribute_heat_to_cells` adds the heat uniformly. Confirms
    /// every cell receives the same temperature delta after the call,
    /// and the delta is non-zero for a non-zero total.
    #[test]
    fn distribute_heat_uniform_across_cells() {
        let grid = HexGrid::new(8, 4);
        let mut state = PhysicsState::new(grid);
        for t in state.temperature_mut() {
            *t = Real::from_int(100);
        }
        distribute_heat_to_cells(&mut state, Real::from_int(100));
        let first = state.temperature()[0];
        // Some delta from 100 K.
        assert!(
            first > Real::from_int(100),
            "uniform heating should raise every cell above its starting T"
        );
        // And every cell receives the same delta.
        for t in state.temperature() {
            assert_eq!(*t, first, "distribution must be uniform across cells");
        }
    }

    /// Apply convenience aggregates across multiple moons. A
    /// two-moon system should produce the sum of the per-moon
    /// dissipation rates.
    #[test]
    fn apply_tidal_heating_sums_over_moons() {
        let grid = HexGrid::new(8, 4);
        let mut state = PhysicsState::new(grid);
        for t in state.temperature_mut() {
            *t = Real::from_int(288);
        }
        let r = Real::from_ratio(286, 1_000);
        let moon_a = MoonHeating::rocky(Real::from_ratio(41, 10_000), 2);
        let moon_b = MoonHeating::icy(Real::from_ratio(50, 10_000), 4);
        let h_a = moon_tidal_heat_rate(r, &moon_a);
        let h_b = moon_tidal_heat_rate(r, &moon_b);
        let total = apply_tidal_heating(&mut state, r, &[moon_a, moon_b]);
        assert_eq!(
            total,
            h_a + h_b,
            "apply_tidal_heating must sum per-moon rates"
        );
    }

    /// Determinism: two independent calls with identical inputs
    /// produce bit-identical state. The whole module is `Real`
    /// arithmetic so this should hold trivially; the test pins it
    /// against a future regression that introduces a non-deterministic
    /// fold order.
    #[test]
    fn tidal_heating_is_deterministic() {
        let grid = HexGrid::new(6, 4);
        let mut a = PhysicsState::new(grid.clone());
        let mut b = PhysicsState::new(grid);
        for t in a.temperature_mut() {
            *t = Real::from_int(250);
        }
        for t in b.temperature_mut() {
            *t = Real::from_int(250);
        }
        let r = Real::from_ratio(286, 1_000);
        let moons = vec![
            MoonHeating::rocky(Real::from_ratio(41, 10_000), 2),
            MoonHeating::icy(Real::from_ratio(80, 10_000), 5),
        ];
        for _ in 0..10 {
            apply_tidal_heating(&mut a, r, &moons);
            apply_tidal_heating(&mut b, r, &moons);
        }
        assert_eq!(a.temperature(), b.temperature());
    }

    /// Empty moon list is a no-op: total = 0, state unchanged.
    #[test]
    fn moonless_planet_produces_no_tidal_heat() {
        let grid = HexGrid::new(4, 4);
        let mut state = PhysicsState::new(grid);
        for t in state.temperature_mut() {
            *t = Real::from_int(288);
        }
        let total = apply_tidal_heating(&mut state, Real::ONE, &[]);
        assert_eq!(total, Real::ZERO);
        for t in state.temperature() {
            assert_eq!(*t, Real::from_int(288));
        }
    }
}
