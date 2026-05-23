//! Tidal-locking regime + synchronous day-night gradient. For a
//! `Synchronous` planet the sub-stellar point is fixed; per-cell
//! absorbed insolation falls off as the great-circle distance from
//! that point grows. For `Other` (free rotators / resonances) the
//! per-row zonal-mean equilibrium table in [`super::Radiation`]
//! carries the physics and this module is unused.

use sim_arith::transcendental::{cos, pow, sin};
use sim_arith::Real;

/// Tidal-locking regime as seen by the radiation law. Mirrors the
/// `LockingState` enum in `sim-world` but lives here so `sim-physics`
/// doesn't depend on `sim-world` (the dependency points the other
/// way). `Synchronous` flips on the per-cell day-night gradient
/// (substellar point fixed); `Other` falls back to the per-row /
/// diurnal-amplitude path used for free rotators and resonances.
///
/// See `Radiation::with_locking` for the wire-in surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockingMode {
    /// Synchronous (1:1) tidal lock. Sub-stellar point fixed at
    /// the supplied (lat, lon) in fractional turns; per-cell day-
    /// night gradient computed each tick from the great-circle
    /// angle to that point.
    Synchronous,
    /// Anything not 1:1-locked — free rotator or `p:q` resonance.
    /// The sub-stellar longitude rotates over time, so on a
    /// macro-step timescale the day-night gradient washes out and
    /// the existing per-row table (annual + zonal mean) is the
    /// right physics.
    Other,
}

/// Night-side residual absorption fraction for a synchronous
/// world. Stellar input drops to ~5 % of full at the antistellar
/// point — atmospheric IR redistribution + some scattered light
/// from the day-night limb keeps the dark side from going to a
/// true 0 K. Matches the value in `post-implementation-fixes.md`
/// P1.5 spec.
pub(super) fn night_factor() -> Real {
    Real::from_ratio(5, 100)
}

/// Pre-computed (sin, cos) of the fixed sub-stellar point, used by
/// the per-cell great-circle distance formula. Computed once per
/// `integrate` call so the hot loop only pays the `sin`/`cos` of
/// each cell's own lat/lon.
#[derive(Debug, Clone, Copy)]
pub(super) struct SubStellarTrig {
    pub sin_lat: Real,
    pub cos_lat: Real,
    pub lon_rad: Real,
}

impl SubStellarTrig {
    pub(super) fn new(substellar_lat_turns: Real, substellar_lon_turns: Real) -> Self {
        let two_pi_v = sim_arith::transcendental::two_pi();
        let sub_lat_rad = substellar_lat_turns * two_pi_v;
        let sub_lon_rad = substellar_lon_turns * two_pi_v;
        Self {
            sin_lat: sin(sub_lat_rad),
            cos_lat: cos(sub_lat_rad),
            lon_rad: sub_lon_rad,
        }
    }
}

/// Synchronous-lock per-cell day-night absorption factor as a
/// multiplier on the bare-row equilibrium `T_eq`.
///
/// Strategy:
/// 1. Convert the cell's `(row, col)` to (lat, lon) in radians.
/// 2. Great-circle angle to the sub-stellar point:
///    `cos(angle) = sin(lat₁)·sin(lat₂) + cos(lat₁)·cos(lat₂)·cos(Δlon)`.
/// 3. Map `cos(angle)` ∈ `[-1, +1]` to a smoothed day fraction
///    `(cos + 1) / 2` ∈ `[0, 1]`. The strict-Lambertian
///    `max(0, cos)` clamps both terminator and antistellar to 0
///    (same equilibrium T), which collapses the terminator-zone
///    tests; the smoothed shape mimics atmosphere-mediated heat
///    transport across the terminator.
/// 4. Absorption multiplier: `night + (1 - night) · day_fraction`.
/// 5. T_eq scales as the fourth root of absorption (Stefan-
///    Boltzmann), so return `absorption^(1/4)`.
///
/// `pi_v`, `two_pi_v`, and `sub_trig` are pre-computed by the
/// caller so the per-cell loop avoids redundant transcendentals.
pub(super) fn synchronous_day_factor(
    axial_q: i32,
    axial_r: i32,
    width_i: i32,
    height_i: i32,
    half_h: i32,
    pi_v: Real,
    two_pi_v: Real,
    sub_trig: SubStellarTrig,
) -> Real {
    // Cell latitude in radians. Convention: row 0 = north pole
    // (lat = +π/2), row `half_h` = equator (lat = 0), row
    // `height-1` = south pole (lat ≈ -π/2). Map row → lat
    // directly via `lat = π · (half_h - r) / height`, which gives
    // the half-turn range [-π/2, +π/2] without round-tripping
    // through fractional turns.
    let row_i = axial_r.rem_euclid(height_i);
    let lat_rad = pi_v.saturating_mul(Real::from_ratio(
        i64::from(half_h - row_i),
        i64::from(height_i.max(1)),
    ));
    // Longitude in radians, `axial.q ∈ [0, width)`.
    let lon_turns = Real::from_ratio(
        i64::from(axial_q.rem_euclid(width_i)),
        i64::from(width_i),
    );
    let lon_rad = lon_turns * two_pi_v;
    // Great-circle distance:
    //   cos(angle) = sin(lat₁)·sin(lat₂)
    //              + cos(lat₁)·cos(lat₂)·cos(Δlon).
    let sin_lat = sin(lat_rad);
    let cos_lat = cos(lat_rad);
    let d_lon = lon_rad - sub_trig.lon_rad;
    let cos_angle = sin_lat
        .saturating_mul(sub_trig.sin_lat)
        .saturating_add(cos_lat.saturating_mul(sub_trig.cos_lat).saturating_mul(cos(d_lon)));
    // Day side fraction. The strict-Lambertian
    // `max(0, cos_angle)` clamps both terminator and
    // antistellar to 0, leaving them at the same
    // equilibrium T — physically reasonable for a
    // radiation-only model (neither sees direct
    // insolation) but it collapses the terminator
    // zone tests. We instead use the smoothed half-
    // cosine `(cos_angle + 1) / 2`, which maps
    // [-1, +1] → [0, 1]: 1 at sub-stellar, 0.5 at
    // the terminator (90°), 0 at antistellar. This
    // is the same shape a real atmosphere produces
    // by carrying day-side heat across the
    // terminator via winds + conduction; our
    // radiation law approximates that redistribution
    // with the smoothed profile directly so the per-
    // cell equilibrium picks up the right gradient
    // even on a 1-row grid (where the diffusion
    // laws can't move heat between longitudes).
    let cell_day = (cos_angle + Real::ONE) * Real::from_ratio(1, 2);
    let cell_day = cell_day.clamp01();
    // Absorption multiplier: night + (1 - night) · cell_day.
    // night = 0.05 floor at the antistellar point,
    // 1.0 ceiling at the sub-stellar point, ~0.525
    // at the terminator. T_eq scales as the fourth
    // root (Stefan-Boltzmann).
    let night = night_factor();
    let one_minus_night = Real::ONE - night;
    let absorption_mult = night.saturating_add(one_minus_night.saturating_mul(cell_day));
    let quarter = Real::from_ratio(1, 4);
    pow(absorption_mult, quarter)
}
