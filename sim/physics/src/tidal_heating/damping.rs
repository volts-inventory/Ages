//! Orbital-energy conservation: eccentricity damping rates and
//! `dE_orbit/dt` (P3.8).
//!
//! The energy-conservation contract is `H = -dE_orbit/dt`: the
//! instantaneous tidal heat dissipated by the moon (`H = C_H × e²`)
//! must equal the rate at which orbital energy is drained by
//! eccentricity damping (`-dE_orbit/dt = 2k × E_scale × e²`). Picking
//! `k = C_H / (2 × E_scale)` makes the two sides algebraically equal
//! by construction — see the `tidal_heat_matches_orbital_energy_loss_*`
//! test in `super::tests`.

use sim_arith::Real;

use super::formula::heating_coefficient_per_e_squared;
use super::MoonHeating;

/// Orbital-energy scale per unit `e²` for a synchronously-locked moon
/// (P3.8), in TW × macro-step units. This is the constant that links
/// the instantaneous tidal heat dissipation `H = C_H × e²` to the
/// orbital-energy decay rate `dE_orbit/dt = -2k × E_scale × e²` —
/// energy conservation requires `H = -dE_orbit/dt`, hence
/// `k = C_H / (2 × E_scale)`. The factor of 2 comes from
/// `d(e²)/dt = 2e × de/dt = -2k × e²` under linear damping
/// `de/dt = -k × e`.
///
/// ## Calibration
///
/// Picked so an Earth-Moon-like configuration (R = 1 Earth-radii,
/// orbital period = 28 macro-steps, rocky substrate) produces a
/// synchronous damping rate of `k ≈ 0.10` per macro-step — preserving
/// the pre-P3.8 fixed-coefficient behaviour of the canonical test
/// fixture in `sim_world::tidal_locking::tests`. Working through:
///
/// - R=1, period=28, k₂/Q=0.003:
///   - n = 2π/28 ≈ 0.2244 rad/macro → n⁵ ≈ 5.7e-4
///   - C_H = (21/2)(0.003)(1)(5.7e-4)(1.75e8) ≈ 3140 (TW per e²)
/// - Target k = 0.10/macro → E_scale = C_H / (2k) ≈ 15 700
///
/// Short-period moons (Io-class: period ≤ 2 macros) produce much
/// larger `C_H` (~3.2e6 for Io), yielding `k ≫ 1` per macro — damping
/// saturates to "circularise in one tick", which is physically right
/// (Io's circularisation timescale is short relative to a macro-step).
/// The `LockingState::Resonance` branch in `sim_world::tidal_locking`
/// then prevents that damping for moons in gravitationally-pumped
/// orbits, so the steady-state e is preserved.
#[inline]
fn orbital_energy_scale_per_e_squared() -> Real {
    Real::from_int(15_700)
}

/// Synchronously-locked eccentricity damping coefficient `k` derived
/// from the heating coefficient (P3.8). Returns a `Real` such that
/// `de/dt = -k × e` (linear damping) gives an orbital-energy decay
/// rate that exactly matches the instantaneous tidal heat `H`:
///
/// ```text
///   H = C_H × e²                      (heat dissipated, TW)
///   dE_orbit/dt = -2k × E_scale × e²  (orbital energy lost, TW)
///   H = -dE_orbit/dt   ⟹   k = C_H / (2 × E_scale)
/// ```
///
/// This is the *synchronous* rate — `sim_world::tidal_locking` scales
/// it down by ~10× for `FreeRotator` planets (slower
/// tidal-friction-only damping) and zeroes it out for `Resonance`
/// planets (gravitational pumping sustains e).
///
/// Returns `Real::ZERO` for degenerate moons (period = 0).
#[must_use]
pub fn synchronous_eccentricity_damping_rate(
    planet_radius_earth_units: Real,
    moon: &MoonHeating,
) -> Real {
    let c_h = heating_coefficient_per_e_squared(planet_radius_earth_units, moon);
    if c_h == Real::ZERO {
        return Real::ZERO;
    }
    let two_e_scale = orbital_energy_scale_per_e_squared()
        .saturating_mul(Real::from_int(2));
    c_h / two_e_scale
}

/// Free-rotator eccentricity damping coefficient `k` derived from the
/// synchronous rate (P3.8). Free-rotator planets damp ~10× slower than
/// synchronously-locked ones — ordinary tidal friction only, without
/// the spin-orbit-coupling boost the locked state gets from the bulge
/// dragging against the host's rotation.
///
/// Defined as `synchronous_eccentricity_damping_rate / 10` so both
/// rates trace back to the same `tidal_dimensional_calibration` and
/// the energy-conservation invariant scales consistently (free
/// rotators dump 1/10 of the heat per unit time at the same e, so the
/// orbital-energy loss is also 1/10 — matching `H ∝ k`).
#[must_use]
pub fn free_rotator_eccentricity_damping_rate(
    planet_radius_earth_units: Real,
    moon: &MoonHeating,
) -> Real {
    synchronous_eccentricity_damping_rate(planet_radius_earth_units, moon)
        / Real::from_int(10)
}

/// Orbital energy loss rate for one moon under linear eccentricity
/// damping `de/dt = -k × e` (P3.8). Returns
/// `dE_orbit/dt = -2 × k × E_scale × e²` in TW — the rate of orbital
/// energy decay per unit time.
///
/// By construction, when `k` is the synchronously-derived rate from
/// `synchronous_eccentricity_damping_rate`, this returns
/// `-moon_tidal_heat_rate(R, moon)` exactly — the energy-conservation
/// contract `H = -dE_orbit/dt` that the spec for P3.8 requires.
///
/// Result is *negative* (orbital energy decreases as e damps).
#[must_use]
pub fn orbital_energy_loss_rate(moon: &MoonHeating, damping_rate_k: Real) -> Real {
    let e2 = moon.eccentricity.saturating_mul(moon.eccentricity);
    let two_e_scale = orbital_energy_scale_per_e_squared()
        .saturating_mul(Real::from_int(2));
    // dE/dt = -2 × k × E_scale × e²
    Real::ZERO - damping_rate_k.saturating_mul(two_e_scale).saturating_mul(e2)
}
