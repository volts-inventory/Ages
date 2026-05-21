//! Sprint 5 Item 19 — per-tick tidal-locking dynamics.
//!
//! Two pieces:
//!
//! - `step_eccentricity_damping(moon, locking_state, dt)` damps a
//!   moon's orbital eccentricity toward zero each tick. Rate depends
//!   on the host planet's tidal-locking regime: `Synchronous` damps
//!   fast (locked orbits become circular within tidal-friction
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
//! `e' = e × (1 - k × dt)` with `k` chosen per locking state; for
//! the integration step sizes the orchestrator uses (`heat_dt` ~ 1
//! macro-step), the per-tick decay factor stays well under 1 and
//! the Real range holds without saturation.

use crate::composition::Moon;
use crate::planet::Planet;
use crate::types::LockingState;
use sim_arith::Real;

/// Per-tick eccentricity damping coefficient `k` for a
/// `Synchronous` locked moon. Synchronous worlds circularise on
/// tidal-friction timescales an order of magnitude faster than free
/// rotators (the same friction that locked the rotation also drains
/// orbital eccentricity). `k = 0.10` per macro-step gives an
/// e-folding time of ~10 ticks — fast enough that a Synchronous
/// fixture run for a few dozen ticks lands at e ≈ 0, slow enough
/// that the per-tick decay factor `(1 - k × dt)` stays safely
/// positive for any sensible `dt`.
///
/// Wrapped in a function rather than a `const` because
/// `Real::from_ratio` is not `const fn` in the underlying
/// `fixed::I32F32` library.
#[inline]
fn synchronous_damping_per_dt() -> Real {
    Real::from_ratio(10, 100)
}

/// Per-tick eccentricity damping coefficient for `FreeRotator`
/// planets. Slow tidal-friction-only damping; about an order of
/// magnitude weaker than Synchronous. `k = 0.01` per macro-step.
#[inline]
fn free_rotator_damping_per_dt() -> Real {
    Real::from_ratio(1, 100)
}

/// Step one moon's eccentricity by `dt`. `locking_state` is the host
/// planet's regime — drives the per-tick damping coefficient.
/// `Synchronous` damps fast, `FreeRotator` slowly, `Resonance`
/// doesn't damp (gravitational forcing from other bodies sustains
/// the steady-state e; the forcing itself isn't modelled in this PR
/// — we just don't damp).
///
/// The damping is linear-decay: `e' = max(0, e - k × dt × e)` =
/// `e × max(0, 1 - k × dt)`. Eccentricity is clamped to `[0, 1)` —
/// it can never go negative, and damping can never push it past
/// zero into the unphysical regime.
pub fn step_eccentricity_damping(
    moon: &mut Moon,
    locking_state: LockingState,
    dt: Real,
) {
    let k = match locking_state {
        LockingState::Synchronous => synchronous_damping_per_dt(),
        LockingState::FreeRotator => free_rotator_damping_per_dt(),
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
            moon_count: 1,
            moons: vec![moon_with_e(Real::from_ratio(5, 100))],
            orbital_eccentricity_x100: 2,
            axial_tilt_deg: Real::from_int(23),
            day_length_hours: Real::from_int(24),
            orbital_period_months: 12,
            metabolic_substrate: MetabolicSubstrate::Aqueous,
            substrate_perturbation: Real::ZERO,
            locking_state,
            star: crate::Star::new(crate::SpectralType::G, Real::from_int(1_361)),
        }
    }

    /// Item 19 acceptance test #1 — a tidally-locked (Synchronous)
    /// moon's eccentricity damps to zero. Set up a moon with
    /// e = 0.10, run N ticks of damping, assert e → 0.
    #[test]
    fn tidally_locked_moon_eccentricity_damps_to_zero() {
        let mut moon = moon_with_e(Real::from_ratio(10, 100));
        let dt = Real::ONE;
        // 200 ticks is many e-folds at k = 0.10/dt: each tick
        // multiplies e by 0.90, so after 200 ticks e is below
        // 0.10 × 0.9^200 ≈ 7e-11 — comfortably under the Q32.32
        // LSB precision of ~2.3e-10.
        for _ in 0..200 {
            step_eccentricity_damping(&mut moon, LockingState::Synchronous, dt);
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
        for _ in 0..500 {
            step_eccentricity_damping(&mut moon, locking, dt);
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
        for _ in 0..50 {
            step_eccentricity_damping(&mut sync_moon, LockingState::Synchronous, dt);
            step_eccentricity_damping(&mut free_moon, LockingState::FreeRotator, dt);
        }
        assert!(
            free_moon.eccentricity > sync_moon.eccentricity,
            "FreeRotator should damp slower than Synchronous: free={:?} sync={:?}",
            free_moon.eccentricity,
            sync_moon.eccentricity
        );
    }
}
