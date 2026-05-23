//! Functional response evaluators (Holling Type I/II/III).
//!
//! Pulled out of `lib.rs` in CA2 so the per-tick
//! [`crate::planet::PlanetEcosystem::apply_interactions`] step has a
//! tight, well-documented dependency surface.

use sim_arith::Real;
use sim_species::FunctionalResponse;

/// Evaluate a functional response. `prey` is the affected species'
/// biomass; `k` is the half-saturation constant in the same units.
///
/// - `Linear` (Type I): `prey`.
/// - `Saturating` (Type II): `prey / (k + prey)`.
/// - `Sigmoidal` (Type III): `prey² / (k² + prey²)`.
///
/// The pair's `strength` and the predator biomass multiply this
/// number in the caller — keeping the function unit-free (per
/// per-predator per-strength unit) makes the per-pair branch in
/// `apply_interactions` readable.
#[must_use]
pub fn functional_response(response: FunctionalResponse, prey: Real, k: Real) -> Real {
    match response {
        FunctionalResponse::Linear => prey,
        FunctionalResponse::Saturating => {
            let denom = k + prey;
            if denom <= Real::ZERO {
                Real::ZERO
            } else {
                prey / denom
            }
        }
        FunctionalResponse::Sigmoidal => {
            let prey_sq = prey * prey;
            let k_sq = k * k;
            let denom = k_sq + prey_sq;
            if denom <= Real::ZERO {
                Real::ZERO
            } else {
                prey_sq / denom
            }
        }
    }
}
