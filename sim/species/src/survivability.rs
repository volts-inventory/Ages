//! Species↔planet survivability: can this biochemistry actually live
//! under the world's climate, pressure, and atmosphere?
//!
//! The per-species [`ToleranceEnvelope`](crate::ToleranceEnvelope) is
//! sampled from the planet's metabolic substrate with per-seed jitter,
//! so two species on the same world differ in how much thermal /
//! pressure stress they shrug off. This module turns that envelope —
//! plus the substrate's atmosphere chemistry — into a single `[floor,
//! 1]` survivability scalar that scales **reproduction** (clutch) and
//! **carrying capacity** (population ceiling): a species well inside
//! its limits thrives; one clinging to the edge of its tolerance, or
//! breathing an atmosphere its metabolism can't use, reproduces less
//! and supports fewer individuals.

use crate::ToleranceEnvelope;
use sim_arith::Real;
use sim_world::Planet;

/// Floor on the survivability score. A species that emerged on this
/// world *did* survive genesis, so even a poor climate / atmosphere
/// match keeps a refugial fraction rather than zeroing reproduction +
/// capacity outright (which would extinguish the species before any
/// civ could form). 1/5 = 0.20.
pub const SURVIVABILITY_FLOOR: (i64, i64) = (1, 5);

/// Multiplicative penalty when the planet's atmosphere is chemically
/// incompatible with the species' metabolic substrate (e.g. an
/// ammoniacal reducer under an oxidising sky). Applied on top of the
/// climate / pressure fit, then floored. 2/5 = 0.40.
pub const ATMOSPHERE_INCOMPAT_PENALTY: (i64, i64) = (2, 5);

/// One standard atmosphere in Pascals — converts the planet's
/// `surface_pressure` (Pa) into the atm units the tolerance pressure
/// range is expressed in.
const PASCALS_PER_ATM: i64 = 101_325;

/// Width (K) over which the climate fit decays to zero once the
/// planet's mean temperature leaves the tolerance band. ~40 K of
/// overshoot — a couple of thermal biome-belts — drives the fit to the
/// floor.
const TEMP_DECAY_BAND_K: i64 = 40;

/// Width (atm) over which the pressure fit decays outside the
/// tolerance band.
const PRESSURE_DECAY_BAND_ATM: i64 = 3;

/// `1.0` anywhere inside `[lo, hi]`; linear decay to `0.0` across
/// `band` outside either edge. Soft *containment*, not the
/// centre-peaked [`ToleranceEnvelope::match_score`] — a species is
/// equally at home anywhere within its limits and only stressed once
/// it crosses them, so an Earth-like 290 K world scores a full 1.0
/// against water's 273–373 K range rather than being penalised for
/// sitting off the range midpoint.
fn soft_contain(v: Real, (lo, hi): (Real, Real), band: Real) -> Real {
    if v >= lo && v <= hi {
        return Real::ONE;
    }
    if band <= Real::ZERO {
        return Real::ZERO;
    }
    let over = if v < lo { lo - v } else { v - hi };
    (Real::ONE - over / band).clamp01()
}

/// Species↔planet survivability in `[SURVIVABILITY_FLOOR, 1]`.
///
/// - **Climate (temperature):** soft containment of the planet's mean
///   temperature in the species' tolerance `temp_range`.
/// - **Pressure:** soft containment of the surface pressure (converted
///   to atm) in `pressure_range`.
/// - **Atmosphere:** a multiplicative [`ATMOSPHERE_INCOMPAT_PENALTY`]
///   when the substrate can't metabolise under the planet's
///   atmosphere ([`sim_world::MetabolicSubstrate::atmosphere_compatible`]).
///
/// Climate and pressure combine as the weakest link (`min`) — biology
/// is gated by whichever axis is most marginal — then the atmosphere
/// penalty multiplies and the floor applies.
#[must_use]
pub fn planet_survivability(tolerance: &ToleranceEnvelope, planet: &Planet) -> Real {
    let temp_fit = soft_contain(
        planet.mean_temperature,
        tolerance.temp_range,
        Real::from_int(TEMP_DECAY_BAND_K),
    );
    let pressure_atm = planet.surface_pressure / Real::from_int(PASCALS_PER_ATM);
    let pressure_fit = soft_contain(
        pressure_atm,
        tolerance.pressure_range,
        Real::from_int(PRESSURE_DECAY_BAND_ATM),
    );
    let mut s = temp_fit.min(pressure_fit);
    if !planet
        .metabolic_substrate
        .atmosphere_compatible(planet.atmosphere)
    {
        s = s * Real::from_ratio(ATMOSPHERE_INCOMPAT_PENALTY.0, ATMOSPHERE_INCOMPAT_PENALTY.1);
    }
    s.max(Real::from_ratio(SURVIVABILITY_FLOOR.0, SURVIVABILITY_FLOOR.1))
        .min(Real::ONE)
}
