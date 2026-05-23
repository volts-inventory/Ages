//! Per-concern test modules for `sim_core`. Split out of the
//! 2.3k-line `tests.rs` (CC1) to mirror the production-side
//! split (`run_tick.rs`, `tick_steps/`, `setup.rs` landed in
//! CA1 + CB3). Each submodule owns one mutually-cohesive slice
//! of the suite; shared fixtures (planet builders, helpers
//! used across multiple files) live here in `mod.rs` so the
//! submodules don't double-define them.

use sim_arith::Real;
use sim_world::{
    Atmosphere, AtmosphericComposition, BiosphereClass, Composition, Crust, CrustalComposition,
    LockingState, Magnetosphere, MetabolicSubstrate, Planet, SpectralType, Star,
};

mod canaries;
mod deterministic;
mod ecosystem;
mod planets;
mod utilities;

// ---------------------------------------------------------------------
// Shared fixtures used across multiple test submodules.
// ---------------------------------------------------------------------

/// Build a Planet with explicit mass/radius and otherwise Earth-like
/// fields. The substrate, atmosphere, and mean temperature come from
/// the caller so a single helper covers both the super-Earth case and
/// the Earth-equivalent baseline used for the law-coefficient diff.
///
/// Used by `planets::super_earth_run_with_2g_gravity_does_not_overflow`.
pub(super) fn earth_like_planet(
    mass: Real,
    radius: Real,
    substrate: MetabolicSubstrate,
    atmosphere: Atmosphere,
    mean_temperature: Real,
) -> Planet {
    Planet {
        seed: 1024,
        name: "T16-SuperEarth".to_string(),
        mass,
        radius,
        composition: Composition::Rocky,
        mean_temperature,
        temperature_gradient: Real::from_int(20),
        terrain_peak: Real::from_int(5_000),
        terrain_centre_q: 0,
        terrain_centre_r: 0,
        sea_level: Real::from_int(1_000),
        atmosphere,
        atmospheric_composition: AtmosphericComposition::vacuum(),
        surface_pressure: Real::from_int(101_325),
        biosphere: BiosphereClass::Lush,
        biosphere_density: Real::from_ratio(7, 10),
        magnetosphere: Magnetosphere::Strong,
        crust: Crust::Basaltic,
        crustal_composition: CrustalComposition::empty(),
        stellar_luminosity: Real::from_int(1_361),
        orbital_distance_au: Real::ONE,
        moon_count: 0,
        moons: Vec::new(),
        orbital_eccentricity_x100: 2,
        axial_tilt_deg: Real::from_int(23),
        day_length_hours: Real::from_int(24),
        orbital_period_months: 12,
        metabolic_substrate: substrate,
        substrate_perturbation: Real::ZERO,
        locking_state: LockingState::FreeRotator,
        // Modern-Sun analog: G-dwarf 45% through its 10 Gyr lifetime,
        // bolometric scale ~1.0 so the planet sees Earth-like irradiance.
        star: Star::with_age(
            SpectralType::G,
            Real::from_int(1_361),
            Real::from_ratio(45, 10),
            Real::from_int(10),
        ),
    }
}
