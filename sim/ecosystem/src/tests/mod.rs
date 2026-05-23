//! Sprint 2 Item 6 + Item 6b + Item 9 tests, split per-concern (CA6).
//!
//! Item 6 (five required cases per plan v2):
//! 1. `planet_has_trophic_pyramid_with_lindeman_ratio`
//! 2. `predator_prey_pair_exhibits_lotka_volterra_cycles`
//! 3. `keystone_species_removal_causes_cascade_disproportionate_to_biomass`
//! 4. `producer_collapse_propagates_to_consumer_tiers`
//! 5. `competition_pair_excludes_at_equilibrium`
//!
//! Item 9 (three required cases per plan v2):
//! 6. `chemolithotroph_species_partition_by_reduction_potential`
//! 7. `syntrophy_pair_extinction_when_separated`
//! 8. `co2_atmosphere_combustion_works_via_alt_oxidiser`
//!
//! Item 6b — biogeochem coupling (three required cases):
//! 9. `producer_growth_consumes_atmospheric_co2`
//! 10. `consumer_respiration_returns_co2_to_atmosphere`
//! 11. `decomposer_chain_balances_carbon_budget`
//!
//! Split layout (CA6):
//! - `pyramid`     — Lindeman pyramid, trophic cascade, keystone cascade
//! - `dynamics`    — predator-prey LV cycles, functional response, competition
//! - `biogeochem`  — producer growth from CO2, consumer respiration, decomposer
//! - `tolerance`   — F3 tolerance envelope, extremophile survival
//! - `biomass`     — F2 per-cell biomass, T9 biome-weighted init
//! - `parasitism`  — P3.1 differentiated mutualism/parasite, virus outbreak
//! - `integration` — full-planet end-to-end tests

use sim_arith::Real;

/// Default per-planet producer carrying capacity used by every test
/// that calls `sample_ecosystem(..)` without a custom capacity. Pinned
/// to 1000 so the extinction floor (`0.001 × 1000 = 1.0`) sits well
/// below the smallest healthy biomass in the sampler outputs.
pub(crate) fn capacity() -> Real {
    Real::from_int(1000)
}

pub mod biogeochem;
pub mod biomass;
pub mod dynamics;
pub mod integration;
pub mod parasitism;
pub mod pyramid;
pub mod tolerance;
