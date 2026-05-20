//! Seasonal climate helpers. Pure functions of
//! `(tick, cell, planet, grid)` (or `(temperature_k, planet)` for
//! the capacity factor) — no physics-state mutation. Pulled out
//! of `lib.rs` so the seasonal-band rationale sits next to its
//! formula instead of getting buried mid-file.
//!
//! ## Hemisphere convention
//!
//! This file uses the **legacy** climate hemisphere mapping
//! (`row > mid` → northern, hemisphere sign `+1`), which is the
//! **opposite** of the canonical physics convention used by
//! `magnetism.rs`, `coriolis.rs`, `radiation.rs`, and
//! `sim/recognition/src/lib.rs::Signature::Hemisphere` (where
//! row 0 = north pole).
//!
//! The disagreement is documented and tested in
//! `sim/world/src/hemisphere.rs` (see
//! `legacy_climate_helper_returns_opposite_sign`). Flipping this
//! file to match the canonical convention would change which cells
//! get which seasonal phase, which transitively shifts the worldgen
//! RNG sampling (species selection, biome rolls, …) and breaks
//! determinism without a coordinated seed rebaseline. The audit-
//! only refactor keeps behaviour bit-identical by routing this
//! file through `hemisphere_for_row_climate_legacy`, so the
//! convention is named in one place rather than implied by an
//! inline `row_signed.signum()` call.

use crate::hemisphere::{hemisphere_for_row_climate_legacy, Hemisphere};
use crate::Planet;
use sim_arith::Real;

/// Seasonal temperature offset for a single cell at a single
/// tick. Pure function of `(tick, cell, planet, grid)` — no
/// physics-state mutation. Every input is seed-derived (planet
/// fields are sampled at run start; grid dimensions are part of
/// the run config). Driven by:
///
/// - **Planet `axial_tilt_deg`** scales the seasonal amplitude
///   (`tilt_factor = axial_tilt_deg / 45 ∈ [0, 1]`). A
///   tidally-locked planet (tilt = 0) gets zero offset; a
///   Uranus-like 45° tilt planet gets the full amplitude.
/// - **Planet `temperature_gradient`** sets the polar-amplitude
///   ceiling: at the pole on a max-tilt planet, the seasonal
///   swing equals one full equator-to-pole gradient, in K.
/// - **Latitude** scales linearly toward the pole. Equator stays
///   flat; pole swings wide.
/// - **Hemisphere** flips phase so northern winter / southern
///   summer co-occur at the same tick.
/// - **Month-in-year** drives the phase via a triangular wave
///   peaking at month 6 (mid-year). Triangular rather than
///   sinusoidal so it stays in deterministic Q32.32 arithmetic
///   without needing `sin` in `sim_arith`.
///
/// All amplitude scaling factors are derived from the planet
/// sample — there are no Earth-absolute K constants.
///
/// Returns the offset in K to add to `state.temperature()[cell]`
/// for time-varying capacity / recognition / etc.
#[must_use]
pub fn seasonal_temperature_offset(
    tick: u64,
    cell: u32,
    planet: &Planet,
    grid: &sim_physics::HexGrid,
) -> Real {
    let height = grid.height();
    if height == 0 {
        return Real::ZERO;
    }
    let width = grid.width();
    if width == 0 {
        return Real::ZERO;
    }
    let row = cell / width;
    let half_height = (i64::from(height) + 1) / 2; // ceil(height/2), ≥ 1
    let mid = i64::from(height) / 2;
    let row_signed = i64::from(row) - mid;
    let pole_dist_abs = i64::try_from(row_signed.unsigned_abs()).unwrap_or(i64::MAX);
    // Hemisphere sign: in the legacy climate mapping, +1 = northern
    // (`row > mid`), -1 = southern (`row < mid`), 0 = equator.
    // Northern + summer = positive offset. Routed through
    // `hemisphere_for_row_climate_legacy` so the (intentional, until
    // a coordinated seed rebaseline) disagreement with the canonical
    // physics convention is named in one place. See
    // `sim/world/src/hemisphere.rs` for the audit + tests.
    let hemi = hemisphere_for_row_climate_legacy(row, height);
    if matches!(hemi, Hemisphere::Equator) {
        return Real::ZERO;
    }
    // `physics_sign` on the legacy mapping returns +1 for the
    // legacy-North half (which is `row > mid`). The behaviour
    // is bit-identical to the previous `row_signed.signum()` branch.
    let hemisphere: i64 = hemi.physics_sign();
    // Latitude band ∈ [0, 1]: 0 at the equator, 1 at the pole.
    let latitude = Real::from_int(pole_dist_abs) / Real::from_int(half_height);
    // Tilt scaling ∈ [0, 1] — derived from planet sample.
    let tilt = (planet.axial_tilt_deg / Real::from_int(45)).clamp01();
    // Triangular phase ∈ [-1, +1]: month 0 → -1, mid-year →
    // +1. Period is the planet's orbital period in months
    // (sampled per planet, range 8..=16) — seasons align with the
    // planet's actual year, not a hardcoded 12-month Earth one.
    let period = u64::from(planet.orbital_period_months.max(1));
    let month = i64::try_from(tick % period).unwrap_or(0);
    let half = i64::try_from(period.max(2) / 2).unwrap_or(1); // ≥ 1
    let dist_from_peak = (month - half).abs();
    // phase = 1 - 2 * dist / half = 1 at dist=0, -1 at dist=half.
    let phase = Real::ONE - (Real::from_int(2 * dist_from_peak) / Real::from_int(half));
    // Amplitude derives from the planet's equator-to-pole gradient.
    // Polar cells on a max-tilt planet swing the full gradient
    // worth of K across the year.
    let amplitude = planet.temperature_gradient * tilt * latitude;
    let signed_phase = if hemisphere > 0 { phase } else { -phase };
    amplitude * signed_phase
}

/// Seasonal carrying-capacity factor. Bands are defined
/// **relative to the planet's `mean_temperature`** (seed-derived)
/// rather than Earth-absolute, so a sub-surface-ocean species
/// adapted to 270 K reads its own balmy zone as productive — not
/// "freezing" by Earth standards.
///
/// The current sharpened multipliers replaced earlier gentle
/// bands (1.0 / 0.92 / 0.80) that avoided crushing the *aggregate*
/// civ population — every cell scaled together, so a civ on a
/// high-tilt planet would shrink across the board each winter.
/// Per-cell dynamics isolate the bite: only the affected
/// cells (high-latitude, deep winter) feel the pressure, while
/// equatorial cells stay productive year-round. With that
/// isolation, the multipliers can sharpen to 1.0 / 0.85 / 0.65
/// without endangering civ-level survival.
///
/// Bands:
/// - **Productive** (`factor = 1.0`): cell temperature within
///   `0.5 × temperature_gradient` of the planet's mean.
/// - **Stressed** (`factor = 0.85`): within one full
///   `temperature_gradient` of the mean.
/// - **Extreme** (`factor = 0.65`): beyond. The cell isn't
///   zeroed; even a frozen tundra has some carrying capacity
///   (stored grain, fishing, hunting), but a 35% drop materially
///   thins that cell's cohort and surfaces in the viewport.
///
/// `temperature_gradient` is the planet's equator-to-pole spread
/// (sampled at planet creation), so a planet with a small spread
/// gets *narrow* productive zones — that planet's species is
/// more thermally fragile, just as on Earth tropical species die
/// faster outside their narrow comfort range than alpine species.
#[must_use]
pub fn seasonal_capacity_factor(temperature_k: Real, planet: &Planet) -> Real {
    // Comfort band: the substrate's biochemistry-supporting
    // temperature range. Cells inside this range run at full
    // capacity; cells outside decay smoothly toward a 0.30 floor.
    // The substrate's temperature range varies enormously across
    // chemistries — Aqueous 250-400 K, Ammoniacal 195-240 K,
    // Hydrocarbon 90-180 K, Silicate 800-1500 K — so a species's
    // "extreme cold" is its biochemistry's hot end on another
    // world. Replaces the prior 3-bucket (1.00/0.92/0.80) cliff,
    // which floored at 0.80 and so never visibly differentiated
    // equatorial vs polar cells on planets with narrow gradients.
    let (low_k_i, high_k_i) = planet.metabolic_substrate.temperature_range();
    let low_k = Real::from_int(low_k_i);
    let high_k = Real::from_int(high_k_i);
    if temperature_k >= low_k && temperature_k <= high_k {
        return Real::ONE;
    }
    // Outside the substrate band: decay linearly over the same
    // band-width again before hitting the floor. So a substrate
    // with a 150 K band tolerates ~150 K of overshoot before
    // capacity drops to the 0.30 floor.
    let band = (high_k - low_k).max(Real::from_int(10));
    let overshoot = if temperature_k < low_k {
        low_k - temperature_k
    } else {
        temperature_k - high_k
    };
    let floor = Real::from_ratio(3, 10);
    let span = Real::ONE - floor;
    let normalised = (overshoot / band).clamp01();
    Real::ONE - span * normalised
}
