//! Sprint 5 Item 19 — per-tick tidal-locking dynamics.
//!
//! Two pieces:
//!
//! - `step_eccentricity_damping(planet_radius, moon, locking_state, dt)`
//!   damps a moon's orbital eccentricity toward zero each tick. Rate
//!   depends on the host planet's tidal-locking regime: `Synchronous`
//!   damps fast (locked orbits become circular within tidal-friction
//!   timescales), `FreeRotator` damps slowly (ordinary tidal
//!   dissipation), `Resonance { .. }` does *not* damp — Laplace-type
//!   gravitational forcing from other bodies (Io-Europa-Ganymede)
//!   pumps eccentricity at the same rate friction removes it, so the
//!   steady-state e stays non-zero. The pumping itself isn't modelled
//!   in this PR; the spec says "just don't damp" for `Resonance`.
//!
//! - `sub_stellar_point(planet, macro_step)` returns the planet's
//!   lat/lon under-the-star coordinate. For `Synchronous` planets
//!   this is fixed at `(0, 0)`; one face is perpetually toward the
//!   star. For everything else it rotates with `macro_step` (the
//!   sim's day proxy) — sub-stellar longitude advances by
//!   `2π / day_length_macros` per macro-step.
//!
//! All arithmetic is `Real` (Q32.32) so the per-tick step is bit-
//! deterministic. The damping uses a simple exponential decay
//! `e' = e × (1 - k × dt)` with `k` derived from
//! `sim_physics::tidal_heating::synchronous_eccentricity_damping_rate`
//! (P3.8). That function shares the `tidal_dimensional_calibration`
//! constant with the heating rate `H`, so the orbital-energy loss
//! `dE_orbit/dt = -2k × E_scale × e²` exactly matches the tidal heat
//! `H = C_H × e²` (energy conservation `H = -dE_orbit/dt`). For
//! typical orchestrator step sizes (`heat_dt ~ 1` macro-step), the
//! per-tick decay factor stays well under 1 and the Real range holds
//! without saturation.

use crate::composition::Moon;
use crate::planet::Planet;
use crate::types::LockingState;
use sim_arith::Real;
use sim_physics::tidal_heating::{
    free_rotator_eccentricity_damping_rate, synchronous_eccentricity_damping_rate,
    MoonHeating,
};

/// Adapter: build the `sim_physics::tidal_heating::MoonHeating` view
/// of a `Moon` for the damping-rate helpers (P3.8). Same shape as the
/// adapters in `sim_core::laws::build_*` but local to the damping site
/// since it's the only consumer.
///
/// `k₂/Q` is left at the rocky default (`0.003`); we don't currently
/// carry the per-moon substrate label on `Moon`. A future refinement
/// can plumb the moon's composition through and pick rocky vs icy.
fn moon_heating_view(moon: &Moon) -> MoonHeating {
    MoonHeating::rocky(moon.eccentricity, moon.orbital_period_macros)
}

/// Step one moon's eccentricity by `dt`. `planet_radius_earth_units`
/// and `locking_state` together drive the per-tick damping coefficient
/// `k`:
///
/// - `Synchronous` damps fast: `k =
///   sim_physics::tidal_heating::synchronous_eccentricity_damping_rate(R, moon)`.
/// - `FreeRotator` damps slowly: `k =
///   sim_physics::tidal_heating::free_rotator_eccentricity_damping_rate(R, moon)`
///   (1/10 of the synchronous rate).
/// - `Resonance` does not damp — gravitational forcing from other
///   bodies (Io-Europa-Ganymede Laplace resonance) sustains e at its
///   steady-state value. The forcing itself is out of scope for this
///   PR; we model the maintenance as "don't damp".
///
/// The damping is linear-decay: `e' = max(0, e - k × dt × e)` =
/// `e × max(0, 1 - k × dt)`. Eccentricity is clamped to `[0, 1)` — it
/// can never go negative, and damping can never push it past zero
/// into the unphysical regime.
///
/// ## P3.8 energy conservation
///
/// The damping coefficient `k` is derived from the *same*
/// `tidal_dimensional_calibration` constant as the heating rate `H`
/// in `sim_physics::tidal_heating::moon_tidal_heat_rate`, so the
/// orbital energy loss `dE_orbit/dt = -2k × E_scale × e²` exactly
/// matches the tidal heat dissipation `H = C_H × e²` — see the
/// `tidal_heat_matches_orbital_energy_loss_for_circular_decay` test
/// in `sim_physics::tidal_heating`.
pub fn step_eccentricity_damping(
    planet_radius_earth_units: Real,
    moon: &mut Moon,
    locking_state: LockingState,
    dt: Real,
) {
    let view = moon_heating_view(moon);
    let k = match locking_state {
        LockingState::Synchronous => {
            synchronous_eccentricity_damping_rate(planet_radius_earth_units, &view)
        }
        LockingState::FreeRotator => {
            free_rotator_eccentricity_damping_rate(planet_radius_earth_units, &view)
        }
        // Resonance-pumped orbits don't damp — gravitational forcing
        // from other bodies (Io-Europa-Ganymede Laplace resonance)
        // sustains e at its steady-state value. The forcing itself
        // is out of scope for this PR; we model the maintenance as
        // "don't damp".
        LockingState::Resonance { .. } => return,
    };
    let decay_factor = (Real::ONE - k.saturating_mul(dt)).max(Real::ZERO);
    let new_e = moon.eccentricity.saturating_mul(decay_factor);
    moon.eccentricity = new_e.max(Real::ZERO);
}

/// Sub-stellar point — the (latitude, longitude) on the planet
/// where the star is directly overhead. For `Synchronous` planets
/// this is fixed at `(0, 0)`: the locked face perpetually shows
/// the same hemisphere to the star, so the sub-stellar point
/// doesn't move. For non-locked planets the longitude rotates
/// with `macro_step` at a rate of one full revolution per
/// `day_length_macros` macro-steps.
///
/// Coordinates are in fractional turns: latitude in `[-1/4, +1/4]`
/// (so `+1/4` = north pole, `-1/4` = south pole) and longitude in
/// `[0, 1)`. The fractional-turn convention sidesteps the need for
/// deterministic trig at this layer — downstream consumers
/// (radiation, climate) can convert via `sim_arith::transcendentals`
/// where needed.
///
/// `macro_step` is the planet-elapsed macro-step counter (one per
/// sim-day). Pass `state.macro_step()` at the call site for live
/// rotation tracking.
#[must_use]
pub fn sub_stellar_point(planet: &Planet, macro_step: u64) -> (Real, Real) {
    match planet.locking_state {
        // Locked planet: sub-stellar point is fixed at (0, 0) by
        // convention. The same hemisphere always faces the star.
        LockingState::Synchronous => (Real::ZERO, Real::ZERO),
        // Free rotator or resonance: longitude advances with
        // macro_step. We approximate `day_length_hours` as a count
        // of macro-steps (one macro-step ≈ one sim-day = 24 hours);
        // the actual conversion factor is fine to leave at unity
        // for the per-tick rotation tracker.
        LockingState::FreeRotator | LockingState::Resonance { .. } => {
            // Day length in macro-steps. `day_length_hours / 24`
            // approximates the day in sim-days (macro-steps).
            // Clamp at 1 to avoid div-by-zero on degenerate fixtures.
            let day_len_macros = (planet.day_length_hours
                / Real::from_int(24))
            .max(Real::ONE);
            // Longitude = fractional part of (macro_step / day_len).
            // Real division yields the fractional turn directly.
            // Cast via `i64::try_from` and saturate at i64::MAX so we
            // never silently wrap for u64 macro-step values that
            // exceed the i64 range (a 32-bit-Q32.32 conversion at
            // that scale would lose precision anyway; the tests live
            // well below that).
            let elapsed = Real::from_int(
                i64::try_from(macro_step).unwrap_or(i64::MAX),
            );
            let raw_turns = elapsed / day_len_macros;
            // Strip the integer-turn component so the result stays
            // in `[0, 1)`. `raw_turns - floor(raw_turns)` via
            // saturating sub of the truncated integer part.
            let whole = Real::from_int(raw_turns.to_int_truncated());
            let longitude = raw_turns - whole;
            (Real::ZERO, longitude)
        }
    }
}

/// Tiny helper: truncate a `Real` toward zero into an `i64`.
/// `sim-arith` doesn't expose this directly on `Real`; we do the
/// raw-bit shift here. Used only for the sub-stellar-point modulo-
/// turn reduction above.
trait RealTruncate {
    fn to_int_truncated(self) -> i64;
}

impl RealTruncate for Real {
    fn to_int_truncated(self) -> i64 {
        // Q32.32: integer part is the upper 32 bits of the i64 raw.
        // Right-shift by 32 truncates toward negative infinity, but
        // for non-negative inputs (the only ones we feed here) that
        // matches toward-zero truncation.
        self.raw().to_bits() >> 32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composition::{AtmosphericComposition, CrustalComposition, Moon};
    use crate::planet::Planet;
    use crate::types::{
        Atmosphere, BiosphereClass, Composition, Crust, LockingState, Magnetosphere,
        MetabolicSubstrate,
    };

    fn moon_with_e(e: Real) -> Moon {
        Moon {
            mass_relative_x100: 100,
            orbital_period_macros: 28,
            inclination_deg_x10: 51,
            eccentricity: e,
        }
    }

    fn planet_with(locking_state: LockingState) -> Planet {
        Planet {
            seed: 1,
            name: String::from("test"),
            mass: Real::ONE,
            radius: Real::ONE,
            composition: Composition::Rocky,
            mean_temperature: Real::from_int(288),
            temperature_gradient: Real::from_int(20),
            terrain_peak: Real::from_int(8000),
            terrain_centre_q: 0,
            terrain_centre_r: 0,
            continent_centres: Vec::new(),
            islands: Vec::new(),
            lakes: Vec::new(),
            sea_level: Real::from_int(2000),
            atmosphere: Atmosphere::Oxidising,
            atmospheric_composition: AtmosphericComposition::vacuum(),
            surface_pressure: Real::from_int(101_325),
            biosphere: BiosphereClass::Lush,
            biosphere_density: Real::from_ratio(5, 10),
            magnetosphere: Magnetosphere::Strong,
            crust: Crust::Basaltic,
            crustal_composition: CrustalComposition::empty(),
            stellar_luminosity: Real::from_int(1_361),
            orbital_distance_au: Real::ONE,
            moon_count: 1,
            moons: vec![moon_with_e(Real::from_ratio(5, 100))],
            orbital_eccentricity_x100: 2,
            axial_tilt_deg: Real::from_int(23),
            day_length_hours: Real::from_int(24),
            orbital_period_months: 12,
            metabolic_substrate: MetabolicSubstrate::Aqueous,
            substrate_perturbation: Real::ZERO,
            locking_state,
            // Modern-Sun analog: G dwarf at ~45% through its 10 Gyr
            // MS lifetime. After P2.4's faint-young-sun correction,
            // `Star::new` lands at the *faint* ZAMS (0.70× = 953
            // W/m²); construct via `with_age` so the fixture keeps
            // its present-day Sun-on-Earth ~1361 W/m² semantics.
            star: crate::Star::with_age(
                crate::SpectralType::G,
                Real::from_int(1_361),
                Real::from_ratio(45, 10),
                Real::from_int(10),
            ),
        }
    }

    /// Item 19 acceptance test #1 — a tidally-locked (Synchronous)
    /// moon's eccentricity damps to zero. Set up a moon with
    /// e = 0.10, run N ticks of damping, assert e → 0.
    #[test]
    fn tidally_locked_moon_eccentricity_damps_to_zero() {
        let mut moon = moon_with_e(Real::from_ratio(10, 100));
        let dt = Real::ONE;
        // P3.8: planet radius = 1 Earth-radii, moon period = 28 macros.
        // The `orbital_energy_scale_per_e_squared = 15_700` calibration
        // is picked so this configuration produces `k ≈ 0.10` per
        // macro, preserving the pre-P3.8 fixed-coefficient behaviour.
        // 200 ticks is many e-folds: e × 0.9^200 ≈ 7e-11, below the
        // Q32.32 LSB ~2.3e-10.
        let r = Real::ONE;
        for _ in 0..200 {
            step_eccentricity_damping(r, &mut moon, LockingState::Synchronous, dt);
        }
        // After 200 damped steps e should be at or below the
        // sub-LSB floor — i.e. effectively zero.
        assert!(
            moon.eccentricity <= Real::from_ratio(1, 1_000_000),
            "Synchronous moon eccentricity should damp to ~0, got {:?}",
            moon.eccentricity
        );
        // And monotonically — never went negative.
        assert!(
            moon.eccentricity >= Real::ZERO,
            "eccentricity went negative: {:?}",
            moon.eccentricity
        );
    }

    /// Item 19 acceptance test #2 — a Laplace-resonance-locked
    /// moon's eccentricity is *not* damped (the resonance
    /// pumping is modelled in this PR by "don't damp"). Run N
    /// ticks, assert e stays non-zero at its initial value.
    #[test]
    fn laplace_resonance_pumps_eccentricity_to_steady_state() {
        let initial_e = Real::from_ratio(5, 100);
        let mut moon = moon_with_e(initial_e);
        let dt = Real::ONE;
        // Io-Europa-Ganymede is a 4:2:1 Laplace resonance; we use
        // the 2:1 pair here as the canonical case.
        let locking = LockingState::Resonance { p: 2, q: 1 };
        let r = Real::ONE;
        for _ in 0..500 {
            step_eccentricity_damping(r, &mut moon, locking, dt);
        }
        // Steady-state: no damping, so e remains exactly the
        // initial value. The "pumping" half of the dynamics isn't
        // modelled in this PR — the spec calls for "just don't
        // damp" — so the test verifies the no-damp behaviour
        // explicitly.
        assert_eq!(
            moon.eccentricity, initial_e,
            "Resonance-locked moon eccentricity should not damp; got {:?}",
            moon.eccentricity
        );
    }

    /// Item 19 acceptance test #3 — a Synchronous planet's sub-
    /// stellar point is fixed across ticks. Call
    /// `sub_stellar_point` at two different macro-steps and assert
    /// the two return values match.
    #[test]
    fn tidally_locked_planet_has_fixed_sub_stellar_point() {
        let planet = planet_with(LockingState::Synchronous);
        let p_t0 = sub_stellar_point(&planet, 0);
        let p_t1 = sub_stellar_point(&planet, 100);
        let p_t2 = sub_stellar_point(&planet, 10_000);
        assert_eq!(
            p_t0, p_t1,
            "Synchronous planet sub-stellar point should be fixed across ticks"
        );
        assert_eq!(
            p_t0, p_t2,
            "Synchronous planet sub-stellar point should be fixed at large t"
        );
        // The convention is (0, 0).
        assert_eq!(p_t0, (Real::ZERO, Real::ZERO));
    }

    /// Sanity check: a `FreeRotator` planet's sub-stellar
    /// longitude actually moves with `macro_step`. Distinct from
    /// the locked-fixed case in the spec test above. The fixture's
    /// `day_length_hours = 24` makes one macro-step exactly one
    /// full revolution; we use a longer day here to get a
    /// non-aliased mid-rotation reading.
    #[test]
    fn free_rotator_sub_stellar_point_advances_with_macro_step() {
        let mut planet = planet_with(LockingState::FreeRotator);
        // 96-hour day = 4 macro-steps per full revolution. At
        // macro_step=1 the longitude should be 1/4 of a turn.
        planet.day_length_hours = Real::from_int(96);
        let p_t0 = sub_stellar_point(&planet, 0);
        let p_t1 = sub_stellar_point(&planet, 1);
        assert_ne!(
            p_t0.1, p_t1.1,
            "FreeRotator longitude should advance with macro_step"
        );
        assert_eq!(p_t1.1, Real::from_ratio(1, 4));
    }

    /// Sanity check: `FreeRotator` damping is slower than
    /// `Synchronous` damping. After the same number of ticks the
    /// `FreeRotator` moon should still have substantially more
    /// eccentricity than the `Synchronous` one.
    #[test]
    fn free_rotator_damps_slower_than_synchronous() {
        let initial_e = Real::from_ratio(10, 100);
        let mut sync_moon = moon_with_e(initial_e);
        let mut free_moon = moon_with_e(initial_e);
        let dt = Real::ONE;
        let r = Real::ONE;
        for _ in 0..50 {
            step_eccentricity_damping(
                r,
                &mut sync_moon,
                LockingState::Synchronous,
                dt,
            );
            step_eccentricity_damping(
                r,
                &mut free_moon,
                LockingState::FreeRotator,
                dt,
            );
        }
        assert!(
            free_moon.eccentricity > sync_moon.eccentricity,
            "FreeRotator should damp slower than Synchronous: free={:?} sync={:?}",
            free_moon.eccentricity,
            sync_moon.eccentricity
        );
    }
}
