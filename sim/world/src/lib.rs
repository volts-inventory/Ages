//! `sim-world` — planet sampling and initial conditions for physics
//! state.
//!
//! Per `docs/world.md`, **the seed determines a planet**. The `Planet`
//! struct holds the bulk planet properties sampled from the run's
//! seed at run start; `init_planet(state, planet)` derives per-cell
//! physics state from those sampled properties.
//!
//! M2-era foundation samples ~10 key properties spanning bulk
//! geology, climate, atmosphere, and biosphere — enough to make
//! every seed produce a recognisably different world. The full
//! ~50-property planet seed lands as recognition templates and
//! species derivation demand each property in M3+.
//!
//! The `Planet` is fixed for the run once sampled. Run determinism
//! holds: same seed → same planet → same physics → same events.
//!
//! ## Habitability bias
//!
//! Composition / atmosphere / biosphere distributions are biased so
//! the typical seed lands a habitable world. A 30-seed sweep with
//! the prior uniform distributions produced 90% `species_extinction`
//! at tick ~99 — the project's "civilizations rise and fall over
//! thousands of years" vision was only delivered for ~1 in 10
//! seeds. The current bias targets ~70-75% multi-civ runs while
//! keeping sub-surface oceans and gaseous shells in the rotation
//! as genuinely-different (typically hostile) worlds.
//!
//! `terrain_peak` is also constrained to land above `sea_level`
//! (rocky: peak ≥ sea + 1500m; ocean world: peak ≥ sea + 500m).
//! Without that constraint, a fraction of otherwise-habitable
//! seeds drew a peak below the waterline → no land cells → no
//! biosphere fuel → `carrying_capacity` = 0 → instant collapse.

#![allow(clippy::module_name_repetitions)]

mod climate;
mod composition;
mod habitability;
mod hemisphere;
mod init;
mod planet;
mod sampling;
mod star;
mod tidal_locking;
mod types;

pub use climate::{
    atmosphere_albedo_x100, atmosphere_greenhouse_k, seasonal_capacity_factor,
    seasonal_temperature_offset,
};
pub use hemisphere::{
    hemisphere_for_row, hemisphere_for_row_climate_legacy, Hemisphere,
};
pub use composition::{AtmosphericComposition, CrustalComposition, Moon};
pub use habitability::{
    cell_habitability, effective_boil_k, habitability_multiplier, hz_factor,
    is_claimable_multiplier, is_land_glyph, is_water_glyph, surface_solvent_boiled,
    terrain_glyph_at, CLAIM_HABITABILITY_THRESHOLD_DEN, CLAIM_HABITABILITY_THRESHOLD_NUM,
};
pub use init::{discharge_threshold_for, init_planet};
pub use planet::{ContinentSeed, Planet};
pub use sampling::{
    planet_name_from_seed, sample_planet, sample_planet_with_overrides, PlanetOverrides,
};
pub use star::{bolometric_scale_at_age, SedFractions, SpectralType, Star};
pub use tidal_locking::{step_eccentricity_damping, sub_stellar_point};
pub use types::{
    Atmosphere, BiosphereClass, Composition, Crust, LockingState, Magnetosphere,
    MetabolicSubstrate,
};

#[cfg(test)]
mod tests;
