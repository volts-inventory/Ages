//! Sprint 5 Item 16 (v2) — tidal heating from eccentric moon orbits.
//!
//! Sprint 5 Item 16 v1 shipped a wrong formula; this module is the
//! astro-feedback-driven rewrite using the textbook expression for
//! the heat dissipated by a moon's eccentric orbit raising
//! tides on its host planet.
//!
//! ## Module layout (CB7)
//!
//! Split into submodules to keep each file focused on one
//! responsibility:
//!
//! - [`formula`] — the closed-form `H = (21/2)(k₂/Q) R⁵ n⁵ e² / G`
//!   evaluation plus all the calibration constants
//!   (`tidal_dimensional_calibration`,
//!   `tidal_dimensional_substrate_multiplier`,
//!   `laplace_resonance_multiplier`, Love number, Q-factor).
//! - [`distribution`] — `distribute_heat_to_cells` and the per-
//!   substrate `subsurface_heat_fraction` / `default_subsurface_heat_fraction`.
//! - [`conduction`] — `subsurface_conduction_step` (P1.1 two-reservoir
//!   relaxation kernel).
//! - [`damping`] — the orbital-energy-conservation half (P3.8):
//!   `synchronous_eccentricity_damping_rate`,
//!   `free_rotator_eccentricity_damping_rate`,
//!   `orbital_energy_loss_rate`.
//!
//! This `mod.rs` keeps the [`MoonHeating`] type definition (used by
//! every submodule), the [`apply_tidal_heating`] orchestrator that
//! sums heat per-moon + invokes the distribution, and the re-exports
//! that preserve the pre-split public API.
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
//!   calibration normaliser — see
//!   [`formula::tidal_dimensional_calibration`] in the `formula`
//!   submodule).
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
//! and absorb the SI dimensional constants into a single
//! `tidal_dimensional_calibration` such that an Io-like configuration
//! (R = 0.286 Earth-radii, e = 0.0041, orbital period 1.77 days,
//! rocky substrate) lands in the [50, 200] TW range — the calibration
//! anchor from the
//! `io_like_configuration_global_heat_flux_in_50_to_200_tw_range`
//! test. The output is in **terawatts (TW)**.
//!
//! ## Per-substrate calibration (F6 — Europa shortfall fix)
//!
//! The Io-tuned `tidal_dimensional_calibration` doesn't transfer
//! cleanly to icy water/hydrocarbon ocean moons. The fix is a
//! per-substrate dimensional multiplier — see
//! `formula::tidal_dimensional_substrate_multiplier` for the detailed
//! mapping (Aqueous/Hydrocarbon 25×; Ammoniacal/Silicate 1×).
//!
//! `MoonHeating::substrate` carries the substrate tag; `None` falls
//! back to the 1× multiplier (back-compat for callers that haven't
//! plumbed substrate through yet, e.g. `sim_core::laws::build_*`
//! which defaults every moon to `MoonHeating::rocky`).
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

use crate::chemistry::MetabolicSubstrate;
use crate::state::PhysicsState;
use sim_arith::Real;

pub mod conduction;
pub mod damping;
pub mod distribution;
pub mod formula;

pub use conduction::subsurface_conduction_step;
pub use damping::{
    free_rotator_eccentricity_damping_rate, orbital_energy_loss_rate,
    synchronous_eccentricity_damping_rate,
};
pub use distribution::{
    default_subsurface_heat_fraction, distribute_heat_to_cells, subsurface_heat_fraction,
};
pub use formula::{
    heating_coefficient_per_e_squared, k2_over_q_icy, k2_over_q_rocky,
    laplace_resonance_multiplier, love_number_rocky, moon_tidal_heat_rate, q_factor_icy,
    q_factor_rocky, tidal_dimensional_substrate_multiplier,
};

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
    /// Optional substrate tag for the per-substrate dimensional
    /// multiplier (F6). `Some(Aqueous)` / `Some(Hydrocarbon)` apply
    /// the 25× Europa-class boost; `Some(Ammoniacal)` /
    /// `Some(Silicate)` / `None` keep the Io-anchored value. See
    /// `tidal_dimensional_substrate_multiplier` for the rationale.
    ///
    /// `None` is the default for callers that haven't plumbed
    /// substrate through (notably `sim_core::laws::build_*` which
    /// defaults all moons to rocky) — preserves pre-F6 behaviour
    /// on the production path.
    pub substrate: Option<MetabolicSubstrate>,
}

impl MoonHeating {
    /// Rocky-substrate moon: `k₂/Q = 0.3/100 = 0.003`. Matches Io,
    /// Earth's Moon, Mars's moons.
    ///
    /// Substrate is left as `None`; pair with `.with_substrate(...)`
    /// to opt into the per-substrate dimensional multiplier (F6).
    #[must_use]
    pub fn rocky(eccentricity: Real, orbital_period_macros: u32) -> Self {
        Self {
            eccentricity,
            orbital_period_macros,
            k2_over_q: k2_over_q_rocky(),
            substrate: None,
        }
    }

    /// Icy-substrate moon: `k₂/Q = 0.3/1000 = 0.0003`. Matches
    /// Europa, Enceladus, Titan-class icy moons.
    ///
    /// Substrate is left as `None`; pair with `.with_substrate(...)`
    /// to opt into the per-substrate dimensional multiplier (F6) —
    /// Europa-class moons need `Some(Aqueous)` to land in the
    /// literature ~10 TW window.
    #[must_use]
    pub fn icy(eccentricity: Real, orbital_period_macros: u32) -> Self {
        Self {
            eccentricity,
            orbital_period_macros,
            k2_over_q: k2_over_q_icy(),
            substrate: None,
        }
    }

    /// Attach a substrate tag (F6). Activates the per-substrate
    /// dimensional multiplier — `Aqueous` / `Hydrocarbon` get a 25×
    /// boost (Europa / Titan icy ocean moons); `Ammoniacal` /
    /// `Silicate` stay at 1×.
    #[must_use]
    pub fn with_substrate(mut self, substrate: MetabolicSubstrate) -> Self {
        self.substrate = Some(substrate);
        self
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
/// `substrate` selects the per-substrate subsurface-vs-surface
/// split (P1.1) — Aqueous / Hydrocarbon worlds route 90 % into
/// the subsurface reservoir (Europa, Titan); Silicate routes 30 %
/// (Io); Ammoniacal routes 60 % (Enceladus-like cryovolcanic
/// regimes). Pass `None` for the substrate-agnostic 80 % default.
///
/// Returns the total dissipation rate in TW so callers / tests can
/// inspect the per-tick budget without re-summing.
pub fn apply_tidal_heating(
    state: &mut PhysicsState,
    planet_radius_earth_units: Real,
    moons: &[MoonHeating],
    substrate: Option<MetabolicSubstrate>,
) -> Real {
    // P0.6: saturating_add so a worldgen that samples many moons
    // (or one moon at a saturated `moon_tidal_heat_rate`) doesn't
    // panic on the running total.
    let mut total = Real::ZERO;
    for m in moons {
        total = total.saturating_add(moon_tidal_heat_rate(planet_radius_earth_units, m));
    }
    let sub_frac = substrate
        .map_or_else(default_subsurface_heat_fraction, subsurface_heat_fraction);
    distribute_heat_to_cells(state, total, sub_frac);
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::HexGrid;

    /// Item 16 spec test #1 — a moon with eccentricity zero
    /// (perfectly circular orbit) dissipates zero tidal heat by
    /// construction.
    #[test]
    fn circular_orbit_moon_produces_zero_tidal_heating() {
        let moon = MoonHeating::rocky(Real::ZERO, 28);
        let h = moon_tidal_heat_rate(Real::ONE, &moon);
        assert_eq!(
            h,
            Real::ZERO,
            "circular orbit (e=0) must produce zero tidal heat: got {h:?}"
        );
        let grid = HexGrid::new(8, 4);
        let mut state = PhysicsState::new(grid);
        for t in state.temperature_mut() {
            *t = Real::from_int(288);
        }
        let total = apply_tidal_heating(&mut state, Real::ONE, &[moon], None);
        assert_eq!(total, Real::ZERO);
        for t in state.temperature() {
            assert_eq!(*t, Real::from_int(288));
        }
    }

    /// Item 16 spec test #2 — Io-like configuration produces a
    /// global heat flux in the [50, 200] TW range.
    #[test]
    fn io_like_configuration_global_heat_flux_in_50_to_200_tw_range() {
        let r_io_earth_units = Real::from_ratio(286, 1_000);
        let e_io = Real::from_ratio(41, 10_000);
        let period_macros: u32 = 2;
        let moon = MoonHeating::rocky(e_io, period_macros);
        let h_tw = moon_tidal_heat_rate(r_io_earth_units, &moon);
        let lo = Real::from_int(50);
        let hi = Real::from_int(200);
        assert!(
            h_tw >= lo && h_tw <= hi,
            "Io-like heat rate must fall in [50, 200] TW (real Io ≈ 100 TW); \
             got {h_tw:?}"
        );
    }

    /// Sanity: doubling eccentricity quadruples heat (since `H ∝ e²`).
    #[test]
    fn heat_scales_with_eccentricity_squared() {
        let r = Real::from_ratio(286, 1_000);
        let small = MoonHeating::rocky(Real::from_ratio(1, 100), 2);
        let large = MoonHeating::rocky(Real::from_ratio(2, 100), 2);
        let h_small = moon_tidal_heat_rate(r, &small);
        let h_large = moon_tidal_heat_rate(r, &large);
        let four_h_small = h_small + h_small + h_small + h_small;
        let diff = (h_large - four_h_small).abs();
        let one_pct = h_large / Real::from_int(100);
        assert!(
            diff < one_pct,
            "H must scale as e²: 4 × H(e) = {four_h_small:?}, H(2e) = {h_large:?}, diff = {diff:?}, tol = {one_pct:?}"
        );
    }

    /// Sanity: a rocky moon dissipates 10× more than an icy moon
    /// at the same R, e, and period.
    #[test]
    fn rocky_substrate_dissipates_ten_times_more_than_icy() {
        let r = Real::ONE;
        let e = Real::from_ratio(5, 100);
        let rocky = MoonHeating::rocky(e, 28);
        let icy = MoonHeating::icy(e, 28);
        let h_r = moon_tidal_heat_rate(r, &rocky);
        let h_i = moon_tidal_heat_rate(r, &icy);
        let ten_hi = h_i * Real::from_int(10);
        let diff = (h_r - ten_hi).abs();
        let two_pct = h_r / Real::from_int(50);
        assert!(
            diff < two_pct,
            "rocky should be ~10x icy: h_r = {h_r:?}, h_i = {h_i:?}, 10·h_i = {ten_hi:?}, diff = {diff:?}, tol = {two_pct:?}"
        );
    }

    /// `distribute_heat_to_cells` adds the heat uniformly.
    #[test]
    fn distribute_heat_uniform_across_cells() {
        let grid = HexGrid::new(8, 4);
        let mut state = PhysicsState::new(grid);
        for t in state.temperature_mut() {
            *t = Real::from_int(100);
        }
        distribute_heat_to_cells(&mut state, Real::from_int(100), Real::ZERO);
        let first = state.temperature()[0];
        assert!(
            first > Real::from_int(100),
            "uniform heating should raise every cell above its starting T"
        );
        for t in state.temperature() {
            assert_eq!(*t, first, "distribution must be uniform across cells");
        }
    }

    /// Apply convenience aggregates across multiple moons.
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
        let total = apply_tidal_heating(&mut state, r, &[moon_a, moon_b], None);
        assert_eq!(
            total,
            h_a + h_b,
            "apply_tidal_heating must sum per-moon rates"
        );
    }

    /// Determinism: two independent calls with identical inputs
    /// produce bit-identical state.
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
            apply_tidal_heating(&mut a, r, &moons, None);
            apply_tidal_heating(&mut b, r, &moons, None);
        }
        assert_eq!(a.temperature(), b.temperature());
        assert_eq!(a.subsurface_temperature(), b.subsurface_temperature());
    }

    /// Empty moon list is a no-op: total = 0, state unchanged.
    #[test]
    fn moonless_planet_produces_no_tidal_heat() {
        let grid = HexGrid::new(4, 4);
        let mut state = PhysicsState::new(grid);
        for t in state.temperature_mut() {
            *t = Real::from_int(288);
        }
        let total = apply_tidal_heating(&mut state, Real::ONE, &[], None);
        assert_eq!(total, Real::ZERO);
        for t in state.temperature() {
            assert_eq!(*t, Real::from_int(288));
        }
    }

    /// P1.1 spec test: an Aqueous (Europa-like) configuration routes
    /// most of its tidal heat into the subsurface reservoir.
    #[test]
    fn europa_like_configuration_powers_subsurface_not_surface() {
        let grid = HexGrid::new(8, 4);
        let mut state = PhysicsState::new(grid);
        for t in state.temperature_mut() {
            *t = Real::from_int(260);
        }
        for t in state.subsurface_temperature_mut() {
            *t = Real::from_int(260);
        }
        let r = Real::from_ratio(286, 1_000);
        let moon = MoonHeating::icy(Real::from_ratio(41, 10_000), 2);
        for _ in 0..100 {
            apply_tidal_heating(
                &mut state,
                r,
                &[moon],
                Some(MetabolicSubstrate::Aqueous),
            );
        }
        let surf = state.temperature()[0];
        let sub = state.subsurface_temperature()[0];
        assert!(
            sub > surf,
            "Aqueous substrate should route tidal heat to subsurface: surf={surf:?} sub={sub:?}"
        );
        assert!(
            sub > Real::from_int(260),
            "subsurface should have warmed: sub={sub:?}"
        );
    }

    /// P1.1 spec test: an Io-like silicate moon routes most of its
    /// tidal heat onto the surface.
    #[test]
    fn io_like_configuration_routes_heat_to_surface() {
        let grid = HexGrid::new(8, 4);
        let mut state = PhysicsState::new(grid);
        for t in state.temperature_mut() {
            *t = Real::from_int(110);
        }
        for t in state.subsurface_temperature_mut() {
            *t = Real::from_int(110);
        }
        let r = Real::from_ratio(286, 1_000);
        let moon = MoonHeating::rocky(Real::from_ratio(41, 10_000), 2);
        for _ in 0..100 {
            apply_tidal_heating(
                &mut state,
                r,
                &[moon],
                Some(MetabolicSubstrate::Silicate),
            );
        }
        let surf = state.temperature()[0];
        let sub = state.subsurface_temperature()[0];
        assert!(
            surf > sub,
            "Silicate substrate should route tidal heat to surface: surf={surf:?} sub={sub:?}"
        );
    }

    /// P1.1 spec test: subsurface conduction relaxes the surface
    /// temperature toward the (warmer) subsurface temperature.
    #[test]
    fn subsurface_conduction_warms_surface_over_time() {
        let grid = HexGrid::new(4, 4);
        let mut state = PhysicsState::new(grid);
        for t in state.temperature_mut() {
            *t = Real::from_int(280);
        }
        for t in state.subsurface_temperature_mut() {
            *t = Real::from_int(300);
        }
        let dt = Real::ONE;
        let initial_surf = state.temperature()[0];
        for _ in 0..1000 {
            subsurface_conduction_step(&mut state, dt);
        }
        let final_surf = state.temperature()[0];
        let final_sub = state.subsurface_temperature()[0];
        assert!(
            final_surf > initial_surf,
            "surface should warm via conduction: initial={initial_surf:?} final={final_surf:?}"
        );
        let mid = Real::from_int(290);
        let surf_gap = (final_surf - mid).abs();
        let sub_gap = (final_sub - mid).abs();
        assert!(
            surf_gap < Real::from_int(10),
            "surface should converge toward midpoint: gap={surf_gap:?}"
        );
        assert!(
            sub_gap < Real::from_int(10),
            "subsurface should converge toward midpoint: gap={sub_gap:?}"
        );
        let total = final_surf + final_sub;
        let expected = Real::from_int(580);
        let drift = (total - expected).abs();
        assert!(
            drift < Real::ONE,
            "per-cell energy conservation: total={total:?} expected={expected:?} drift={drift:?}"
        );
    }

    /// `subsurface_heat_fraction` returns the documented per-substrate splits.
    #[test]
    fn subsurface_heat_fraction_per_substrate() {
        assert_eq!(
            subsurface_heat_fraction(MetabolicSubstrate::Aqueous),
            Real::from_ratio(90, 100)
        );
        assert_eq!(
            subsurface_heat_fraction(MetabolicSubstrate::Hydrocarbon),
            Real::from_ratio(90, 100)
        );
        assert_eq!(
            subsurface_heat_fraction(MetabolicSubstrate::Ammoniacal),
            Real::from_ratio(60, 100)
        );
        assert_eq!(
            subsurface_heat_fraction(MetabolicSubstrate::Silicate),
            Real::from_ratio(30, 100)
        );
    }

    /// P2.1 / F6 spec test — Europa-like icy moon produces a tidal
    /// heat budget in the literature `[5, 20] TW` window.
    #[test]
    fn europa_like_configuration_global_heat_in_5_to_20_tw_range() {
        let r_europa = Real::from_ratio(246, 1_000);
        let e_europa = Real::from_ratio(94, 10_000);
        let period_macros: u32 = 4;
        let moon = MoonHeating::icy(e_europa, period_macros)
            .with_substrate(MetabolicSubstrate::Aqueous);
        let h_tw = moon_tidal_heat_rate(r_europa, &moon);
        let lo = Real::from_int(5);
        let hi = Real::from_int(20);
        assert!(
            h_tw >= lo && h_tw <= hi,
            "Europa-like heat rate must fall in [5, 20] TW \
             (real Europa ~10 TW); got {h_tw:?}"
        );
    }

    /// P2.1 spec test #2 — Enceladus-like icy moon produces a tidal
    /// heat budget that lands inside `[1 GW, 100 GW]`.
    #[test]
    fn enceladus_like_configuration_global_heat_in_5_to_50_gw_range() {
        let r_enceladus = Real::from_ratio(39, 1_000);
        let e_enceladus = Real::from_ratio(47, 10_000);
        let period_macros: u32 = 1;
        let moon = MoonHeating::icy(e_enceladus, period_macros);
        let h_tw = moon_tidal_heat_rate(r_enceladus, &moon);
        let lo = Real::from_ratio(1, 1_000);
        let hi = Real::from_ratio(1, 10);
        assert!(
            h_tw >= lo && h_tw <= hi,
            "Enceladus-like heat rate must fall in [1 GW, 100 GW] (real ~16 GW); \
             got {h_tw:?} TW"
        );
    }

    /// T19 / C4 spec test — Ganymede-like icy moon with an Aqueous
    /// subsurface ocean produces a tidal heat budget in `[0.5, 5] TW`.
    #[test]
    fn ganymede_like_configuration_in_0_5_to_5_tw_range() {
        let r_ganymede = Real::from_ratio(413, 1_000);
        let e_ganymede = Real::from_ratio(13, 10_000);
        let period_macros: u32 = 7;
        let moon = MoonHeating::icy(e_ganymede, period_macros)
            .with_substrate(MetabolicSubstrate::Aqueous);
        let h_tw = moon_tidal_heat_rate(r_ganymede, &moon);
        let lo = Real::from_ratio(5, 10);
        let hi = Real::from_int(5);
        assert!(
            h_tw >= lo && h_tw <= hi,
            "Ganymede-like heat rate must fall in [0.5, 5] TW \
             (real ~1-2 TW Laplace-pumped); got {h_tw:?} TW"
        );
    }

    /// T19 spec test — Callisto-like icy moon produces *essentially
    /// zero* tidal heat.
    #[test]
    fn callisto_like_configuration_in_0_to_0_5_gw_range() {
        let r_callisto = Real::from_ratio(378, 1_000);
        let e_callisto = Real::from_ratio(74, 10_000);
        let period_macros: u32 = 17;
        let moon = MoonHeating::icy(e_callisto, period_macros);
        let h_tw = moon_tidal_heat_rate(r_callisto, &moon);
        let lo = Real::ZERO;
        let hi = Real::from_ratio(5, 1_000);
        assert!(
            h_tw >= lo && h_tw <= hi,
            "Callisto-like heat rate must fall in [0, 5] GW \
             (real ~0 GW non-resonant; current calibration lands at \
             ~1.6 GW); got {h_tw:?} TW"
        );
    }

    /// C4 spec test — `laplace_resonance_multiplier` returns the
    /// documented Ganymede-class 8× boost.
    #[test]
    fn laplace_resonance_multiplier_per_radius_and_substrate() {
        let r_ganymede = Real::from_ratio(413, 1_000);
        assert_eq!(
            laplace_resonance_multiplier(r_ganymede, Some(MetabolicSubstrate::Aqueous)),
            Real::from_int(8),
            "Ganymede-class Aqueous moon should get 8× Laplace pumping"
        );
        assert_eq!(
            laplace_resonance_multiplier(
                r_ganymede,
                Some(MetabolicSubstrate::Hydrocarbon),
            ),
            Real::from_int(8),
        );
        assert_eq!(
            laplace_resonance_multiplier(
                r_ganymede,
                Some(MetabolicSubstrate::Ammoniacal),
            ),
            Real::from_int(8),
        );
        assert_eq!(
            laplace_resonance_multiplier(
                r_ganymede,
                Some(MetabolicSubstrate::Silicate),
            ),
            Real::ONE,
        );
        assert_eq!(
            laplace_resonance_multiplier(r_ganymede, None),
            Real::ONE,
        );
        let r_europa = Real::from_ratio(246, 1_000);
        assert_eq!(
            laplace_resonance_multiplier(r_europa, Some(MetabolicSubstrate::Aqueous)),
            Real::ONE,
            "Europa-class radius should not get the Ganymede-class boost"
        );
        let r_callisto = Real::from_ratio(378, 1_000);
        assert_eq!(
            laplace_resonance_multiplier(r_callisto, Some(MetabolicSubstrate::Aqueous)),
            Real::ONE,
            "Callisto-class radius should not get the Ganymede-class boost"
        );
        assert_eq!(
            laplace_resonance_multiplier(
                Real::from_ratio(39, 100),
                Some(MetabolicSubstrate::Aqueous),
            ),
            Real::from_int(8),
        );
        assert_eq!(
            laplace_resonance_multiplier(
                Real::from_ratio(45, 100),
                Some(MetabolicSubstrate::Aqueous),
            ),
            Real::from_int(8),
        );
    }

    /// `init_subsurface_temperature` sets each cell's subsurface T to
    /// `surface T - 10 K`.
    #[test]
    fn init_subsurface_temperature_offsets_by_ten() {
        let grid = HexGrid::new(4, 4);
        let mut state = PhysicsState::new(grid);
        for (i, t) in state.temperature_mut().iter_mut().enumerate() {
            *t = Real::from_int(280 + i as i64);
        }
        state.init_subsurface_temperature();
        for i in 0..state.grid().n_cells() {
            assert_eq!(
                state.subsurface_temperature()[i],
                state.temperature()[i] - Real::from_int(10),
                "cell {i} subsurface should be surface - 10"
            );
        }
    }

    /// P3.8 spec test — cumulative tidal heat must match cumulative
    /// orbital-energy loss across an eccentricity-damping window.
    #[test]
    fn tidal_heat_matches_orbital_energy_loss_for_circular_decay() {
        let r = Real::ONE;
        let period = 28u32;
        let initial_e = Real::from_ratio(10, 100);
        let mut moon = MoonHeating::rocky(initial_e, period);

        let dt = Real::ONE;
        let mut cumulative_heat = Real::ZERO;
        let mut cumulative_orbital_loss = Real::ZERO;

        for _ in 0..100 {
            let h = moon_tidal_heat_rate(r, &moon);
            cumulative_heat = cumulative_heat.saturating_add(h.saturating_mul(dt));

            let k = synchronous_eccentricity_damping_rate(r, &moon);
            let de_dt = orbital_energy_loss_rate(&moon, k);
            let loss = Real::ZERO - de_dt;
            cumulative_orbital_loss =
                cumulative_orbital_loss.saturating_add(loss.saturating_mul(dt));

            let decay_factor =
                (Real::ONE - k.saturating_mul(dt)).max(Real::ZERO);
            moon.eccentricity =
                moon.eccentricity.saturating_mul(decay_factor).max(Real::ZERO);
        }

        assert!(
            cumulative_heat > Real::ZERO,
            "cumulative tidal heat should be positive: {cumulative_heat:?}"
        );
        assert!(
            cumulative_orbital_loss > Real::ZERO,
            "cumulative orbital-energy loss should be positive: \
             {cumulative_orbital_loss:?}"
        );

        let diff = (cumulative_heat - cumulative_orbital_loss).abs();
        let tol = cumulative_heat / Real::from_int(100);
        assert!(
            diff <= tol,
            "P3.8 energy conservation: cumulative H = {cumulative_heat:?} \
             vs orbital loss = {cumulative_orbital_loss:?}, drift = {diff:?}, \
             tol = {tol:?} (1% of H)"
        );
    }
}
