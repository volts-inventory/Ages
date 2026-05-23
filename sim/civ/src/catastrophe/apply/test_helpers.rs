//! Shared test fixtures for the per-catastrophe handler test
//! modules. Pre-CB2 these all lived alongside the dispatcher in
//! `apply.rs`; CB2 lifts them into a shared `#[cfg(test)]` module
//! so each handler's tests (`volcanic`, `disease`, `asteroid`,
//! `solar_flare`, `ice_age`) can `use super::super::test_helpers::*`
//! without duplicating the fixture wiring.

#![cfg(test)]

use sim_arith::Real;
use sim_physics::{HexGrid, PhysicsState, Substance};
use sim_recognition::RecognitionLibrary;
use sim_species::Species;
use sim_world::{sample_planet, Magnetosphere, Planet};

/// Default test species — `dormancy_capability = 0` so all
/// existing pre-Sprint-2-Item-7b catastrophe assertions still
/// hold (no damage-reduction multiplier). Dormancy-specific
/// tests below construct their own species with explicit
/// `dormancy_capability`.
pub(crate) fn test_species() -> Species {
    let planet = sample_planet(1);
    let lib = RecognitionLibrary::earth_like_default();
    let mut s = sim_species::derive(&planet, &lib);
    s.dormancy_capability = Real::ZERO;
    s
}

pub(crate) fn species_with_dormancy(dormancy: Real) -> Species {
    let mut s = test_species();
    s.dormancy_capability = dormancy;
    s
}

pub(crate) fn empty_state() -> PhysicsState {
    PhysicsState::new(HexGrid::new(4, 4))
}

pub(crate) fn well_fed_state() -> PhysicsState {
    let mut s = PhysicsState::new(HexGrid::new(4, 4));
    for v in s.substance_mut(Substance::Fuel.idx()) {
        *v = Real::from_int(10);
    }
    s
}

/// Test fixture: a benign Earth-like planet that doesn't
/// trigger any of the new gated catastrophes. Lets the
/// existing volcanic/disease tests run unaffected.
pub(crate) fn earth_like_planet() -> Planet {
    Planet {
        seed: 0,
        name: "TestPlanet".to_string(),
        // Earth-like mass/radius → derived gravity ≈ 9.81 m/s²
        // (Sprint 5 Item 21).
        mass: Real::ONE,
        radius: Real::ONE,
        composition: sim_world::Composition::Rocky,
        mean_temperature: Real::from_int(288),
        temperature_gradient: Real::from_int(20),
        terrain_peak: Real::from_int(8000),
        terrain_centre_q: 0,
        terrain_centre_r: 0,
        sea_level: Real::from_int(2000),
        atmosphere: sim_world::Atmosphere::Oxidising,
        atmospheric_composition: sim_world::AtmosphericComposition::vacuum(),
        biosphere_density: Real::from_ratio(3, 10),
        crustal_composition: sim_world::CrustalComposition::empty(),
        surface_pressure: Real::from_int(101_325),
        biosphere: sim_world::BiosphereClass::Lush,
        magnetosphere: Magnetosphere::Strong,
        crust: sim_world::Crust::Basaltic,
        stellar_luminosity: Real::from_int(1361),
        orbital_distance_au: Real::ONE,
        moon_count: 1,
        moons: vec![sim_world::Moon {
            mass_relative_x100: 100,
            orbital_period_macros: 28,
            inclination_deg_x10: 51,
            eccentricity: Real::ZERO,
        }],
        orbital_eccentricity_x100: 2,
        axial_tilt_deg: Real::from_int(23),
        day_length_hours: Real::from_int(24),
        orbital_period_months: 12,
        metabolic_substrate: sim_world::MetabolicSubstrate::Aqueous,
        substrate_perturbation: Real::ZERO,
        locking_state: sim_world::LockingState::FreeRotator,
        // Modern-Sun analog: G dwarf at ~45% through its 10 Gyr
        // MS lifetime. After P2.4's faint-young-sun correction,
        // `Star::new` lands at the *faint* ZAMS (0.70× = 953
        // W/m²); construct via `with_age` to keep this fixture
        // at the present-day Sun-on-Earth ~1361 W/m².
        star: sim_world::Star::with_age(
            sim_world::SpectralType::G,
            Real::from_int(1_361),
            Real::from_ratio(45, 10),
            Real::from_int(10),
        ),
    }
}

/// Flare-firing planet: weak magnetosphere + above-Earth
/// luminosity satisfy `solar_flare_fires`. Tick used by the
/// flare tests below: `1567 * MONTHS_PER_YEAR = 18804`.
pub(crate) fn flare_planet() -> Planet {
    let mut p = earth_like_planet();
    p.magnetosphere = Magnetosphere::Weak;
    p.stellar_luminosity = Real::from_int(1_500);
    p
}

/// Extremophile tolerance: radiation-tolerant envelope.
/// `radiation_max = 20` so the post-flare radiation flux (≈ 1.1)
/// still scores well inside the envelope. Other axes centred on
/// the test cell's conditions (T=300 K, pH=7, salinity=20 g/L,
/// p=1 atm) with margins that score above the radiation axis's
/// fit so the radiation gate (not an incidental other axis) is
/// the binding constraint on `match_score`.
pub(crate) fn extremophile_tolerance() -> sim_species::ToleranceEnvelope {
    sim_species::ToleranceEnvelope {
        temp_range: (Real::from_int(200), Real::from_int(400)),
        ph_range: (Real::from_int(5), Real::from_int(9)),
        salinity_range: (Real::from_int(10), Real::from_int(30)),
        radiation_max: Real::from_int(20),
        pressure_range: (Real::from_ratio(5, 10), Real::from_ratio(15, 10)),
    }
}

