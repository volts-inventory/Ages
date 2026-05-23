//! Spatial distribution of tidal heat: per-substrate
//! surface/subsurface split + uniform per-cell deposition (P1.1).
//!
//! `distribute_heat_to_cells` is the orchestrator's hook for routing
//! the per-moon `moon_tidal_heat_rate` (in TW) into the planet's
//! surface-temperature and subsurface-temperature fields. The
//! substrate-keyed `subsurface_heat_fraction` picks the
//! Europa/Io/Enceladus split; `default_subsurface_heat_fraction` is
//! the substrate-agnostic 80 % fallback.

use crate::chemistry::MetabolicSubstrate;
use crate::state::PhysicsState;
use sim_arith::Real;

/// Fraction of tidal heat routed into the *subsurface* reservoir
/// for a given metabolic substrate (P1.1). The remainder lands on
/// the surface `temperature` field. Real ratios vary enormously:
///
/// - Io (silicate / low-Q rocky volcanism) is ~95 % surface because
///   tidal stress shatters the rocky crust into mid-latitude shear
///   zones that vent magma directly. → 30 % subsurface.
/// - Europa (Aqueous-on-icy-shell) is ~95 % subsurface because the
///   ice shell insulates surface from the warm interior ocean. →
///   90 % subsurface.
/// - Titan (Hydrocarbon) is similar to Europa — cryogenic surface,
///   warm subsurface ocean (likely H2O + ammonia) under the icy
///   shell. → 90 % subsurface.
/// - Enceladus / Ganymede analogues sit between; for v1 we map all
///   ammoniacal regimes to a 60 % subsurface split (the eutectic
///   point of NH3-H2O lets some heat vent to surface via cryovolcanism).
///
/// Returns a `Real` in `[0, 1]` interpretable as the subsurface
/// fraction; `Real::ONE - subsurface_fraction` is the surface
/// fraction.
#[must_use]
pub fn subsurface_heat_fraction(substrate: MetabolicSubstrate) -> Real {
    match substrate {
        MetabolicSubstrate::Aqueous | MetabolicSubstrate::Hydrocarbon => {
            Real::from_ratio(90, 100)
        }
        MetabolicSubstrate::Ammoniacal => Real::from_ratio(60, 100),
        MetabolicSubstrate::Silicate => Real::from_ratio(30, 100),
    }
}

/// Default subsurface-heat fraction for callers that don't know the
/// per-planet substrate (P1.1). 80 % subsurface matches the
/// astrophysical default the post-implementation review identified:
/// "Direct 80% of the tidal heat into subsurface, 20% into surface."
/// Production paths thread the planet's actual `MetabolicSubstrate`
/// via `subsurface_heat_fraction`; this default is the
/// substrate-agnostic fallback.
#[inline]
#[must_use]
pub fn default_subsurface_heat_fraction() -> Real {
    Real::from_ratio(80, 100)
}

/// Distribute a total heat dissipation rate (in TW) uniformly across
/// every cell, splitting between the subsurface reservoir and the
/// surface temperature field per the `subsurface_fraction` argument.
///
/// `total_heat_tw` is the sum of `moon_tidal_heat_rate` over every
/// moon orbiting the planet (in TW). `subsurface_fraction` ∈ `[0, 1]`
/// specifies what proportion of the heat goes into
/// `state.subsurface_temperature` (the rest lands on the surface
/// `temperature` field). Use `subsurface_heat_fraction(substrate)`
/// to pick the per-substrate ratio, or
/// `default_subsurface_heat_fraction()` for the substrate-agnostic
/// 80 % default.
///
/// P1.1 rationale: real tidal heating on Europa / Enceladus powers
/// subsurface oceans, not surface T; on Io it concentrates at
/// mid-latitude shear zones where the bulge tears the crust. The
/// previous "100 % uniform onto surface" distribution foreclosed
/// subsurface-ocean habitats on tidally heated moons. This split
/// is the minimum-viable correction; a future pass can replace the
/// uniform distribution with a latitude / longitude profile (the
/// TODO ladder calls out "concentrate heat at tidal-stress hot spots").
///
/// The `heat_to_kelvin` conversion factor (`1e-6`) is unchanged from
/// the original implementation: 100 TW of Io-scale heating distributed
/// across a 1000-cell grid produces a ~1e-7 K per-cell per-call delta,
/// comparable to radiation's per-step nudges.
pub fn distribute_heat_to_cells(
    state: &mut PhysicsState,
    total_heat_tw: Real,
    subsurface_fraction: Real,
) {
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
    // an unphysical thermal blowout. With tidal_dimensional_calibration land at
    // ~100 (TW), n_cells ~ 100-1000, and heat_to_kelvin = 1e-6,
    // the per-cell delta is ~1e-7 K per macro-step — same order as
    // radiation's per-step nudges.
    let heat_to_kelvin = Real::from_ratio(1, 1_000_000);
    let per_cell_total = total_heat_tw.saturating_mul(heat_to_kelvin)
        / Real::from_int(i64::try_from(n_cells).unwrap_or(1).max(1));
    // Clamp the fraction to `[0, 1]` defensively so a caller passing
    // an out-of-range value can't bias the totals out of conservation.
    let sub_frac = subsurface_fraction.clamp(Real::ZERO, Real::ONE);
    let surf_frac = Real::ONE - sub_frac;
    let per_cell_sub = per_cell_total.saturating_mul(sub_frac);
    let per_cell_surf = per_cell_total.saturating_mul(surf_frac);
    // Update surface first (mutable borrow #1), then subsurface
    // (mutable borrow #2). Q32.32 is bit-exact under saturating add
    // so the order doesn't affect determinism, but we keep it
    // surface-then-subsurface to mirror the reading order in
    // `subsurface_conduction_step`.
    for t in state.temperature_mut() {
        *t = t.saturating_add(per_cell_surf);
    }
    for t in state.subsurface_temperature_mut() {
        *t = t.saturating_add(per_cell_sub);
    }
}
