//! Physics-law builders. Different seeds → different
//! planets → different law coefficients (structural
//! shapes fixed, parametric values vary). Pulled out of `lib.rs` so
//! the per-substrate / per-atmosphere coefficient tables sit next to
//! their rationale.

use sim_arith::Real;
use sim_physics::{
    chemistry::Chemistry, em::Electromagnetism, fluid::GravityFlow, heat::HeatConduction, Mechanics,
};
use sim_world::Atmosphere;

/// All physics laws built for the run, bundled into one struct so
/// the tick loop and per-phase helpers thread a single reference
/// instead of twelve parameters.
pub(crate) struct Laws {
    pub fluid: GravityFlow,
    pub heat: HeatConduction,
    pub em: Electromagnetism,
    pub chemistry: Chemistry,
    pub radiation: sim_physics::Radiation,
    pub wind: sim_physics::Wind,
    pub hydrology: sim_physics::Hydrology,
    pub tides: sim_physics::Tides,
    pub magnetism: sim_physics::Magnetism,
    pub lorentz: sim_physics::Lorentz,
    pub coriolis: sim_physics::Coriolis,
    pub vertical: sim_physics::VerticalConvection,
}

/// Build all physics laws with coefficients derived from a sampled
/// Planet. Different seeds → different planets → different law
/// coefficients → different physics outcomes (structural
/// shapes fixed, parametric values vary).
pub(crate) fn build_laws(planet: &sim_world::Planet, grid_height: u32) -> Laws {
    let mechanics = Mechanics {
        gravity: planet.gravity,
    };
    let fluid = GravityFlow::from_mechanics(mechanics);

    let alpha = match planet.atmosphere {
        Atmosphere::None => Real::percent(5),
        Atmosphere::Thin => Real::percent(8),
        Atmosphere::Oxidising | Atmosphere::Reducing => Real::percent(10),
        Atmosphere::Hazy => Real::percent(12),
    };
    let heat = HeatConduction { alpha };

    let conductivity = if planet.atmosphere == Atmosphere::None {
        Real::from_ratio(5, 1000)
    } else {
        Real::percent(1)
    };
    let discharge_threshold = sim_world::discharge_threshold_for(planet.magnetosphere);
    let em = Electromagnetism {
        conductivity,
        discharge_threshold,
        discharge_energy: Real::from_int(5),
        // Wind advects charge along velocity. Earth-like
        // default keeps wind-driven transport comparable to
        // molecular conductivity.
        charge_advect_k: Real::percent(1),
    };

    // Ignition threshold in K. Oxidising atmospheres ignite cooler;
    // reducing or thin ones need more heat. None disables combustion
    // by setting the threshold beyond any reachable temperature.
    let ignition_threshold = ignition_threshold_for(planet.atmosphere);
    // Substrate-relative chemistry. Solvent freeze/boil
    // thresholds dispatch on the planet's MetabolicSubstrate so a
    // methane world's solvent freezes at 91 K, a silicate world at
    // 1687 K, etc. The Substance::{Water, Ice, Vapour} enum carries
    // the canonical "solvent liquid/solid/gas" semantics for the run.
    // Thread the per-seed substrate perturbation so each seed's
    // chemistry phase points are slightly distinct within the
    // substrate's tolerance window.
    let chemistry = Chemistry::for_planet_with_perturbation(
        planet.surface_pressure,
        ignition_threshold,
        planet.metabolic_substrate.tag(),
        planet.substrate_perturbation,
    );

    // Radiative-balance law. Per-row equilibrium temperature
    // derived from stellar irradiance, atmosphere albedo, and
    // greenhouse offset. Each tick relaxes cell temps toward
    // their row's T_eq; runs alongside HeatConduction so radiation
    // sources the gradient and diffusion smooths it.
    let albedo_x100 = match planet.atmosphere {
        Atmosphere::None => 10,
        Atmosphere::Thin => 20,
        Atmosphere::Oxidising => 30,
        Atmosphere::Reducing => 35,
        Atmosphere::Hazy => 50,
    };
    let greenhouse_k = match planet.atmosphere {
        Atmosphere::None => Real::ZERO,
        Atmosphere::Thin => Real::from_int(10),
        Atmosphere::Oxidising | Atmosphere::Reducing => Real::from_int(35),
        Atmosphere::Hazy => Real::from_int(60),
    };
    // Thread axial tilt + orbital period into Radiation so
    // it can pre-compute a per-(row, season) table and the
    // seasonal swing emerges instead of being a constant
    // annual-mean gradient.
    let axial_tilt_deg = i64::from(planet.axial_tilt_deg.raw().to_num::<i32>().max(0));
    // Each macro-step ≈ one sim-day; `orbital_period_months`
    // months × 30 days = year-length in macro-steps.
    let year_macros = u64::from(planet.orbital_period_months).saturating_mul(30);
    let radiation = sim_physics::Radiation::for_planet(
        grid_height,
        planet.stellar_luminosity,
        albedo_x100,
        greenhouse_k,
        axial_tilt_deg,
        year_macros,
        // Orbital eccentricity drives perihelion / aphelion
        // insolation swing.
        planet.orbital_eccentricity_x100,
        // Day length drives diurnal cycling. Fast rotators
        // average out; slow / tidally-locked rotators see real
        // day/night asymmetry across macro-steps.
        planet.day_length_hours,
    );

    // Atmospheric wind. Earth-like defaults for now; per-planet
    // tuning (e.g. friction × atmospheric thickness, advect_k ×
    // atmospheric mass) is a follow-up once `Atmosphere` carries
    // numeric scale-height + density rather than just a tag.
    // Short-circuit on `Atmosphere::None` worlds — no
    // medium means no wind dynamics.
    let mut wind = sim_physics::Wind::earth_like();
    wind.has_atmosphere = !matches!(planet.atmosphere, Atmosphere::None);
    // Atmospheric density coupling. Wind & heat advection
    // transport mass; thinner atmospheres carry less heat per
    // unit velocity. Scale `advect_k` linearly with the planet's
    // density relative to Earth's (122 cg/m³ → factor 1.0). Clamp
    // at 5× to keep CFL safe under thick Venus-like atmospheres.
    // Without this every planet's wind transported heat at
    // Earth-like efficiency, masking the climatological difference
    // between Mars-Thin and Venus-Reducing entirely.
    let density_x100 = planet.atmosphere.density_x100();
    if density_x100 > 0 {
        let density_factor =
            (Real::from_int(density_x100) / Real::from_int(122)).min(Real::from_int(5));
        wind.advect_k = wind.advect_k * density_factor;
    }

    // Hydrologic cycle. Substrate-aware Clausius-Clapeyron
    // so a methane / ammonia / silicate world cycles its solvent
    // at the right phase boundary.
    // Now per-cell — Hydrology computes elevation-derived
    // pressure via the barometric formula (`P = P_0 · exp(-h/H)`)
    // and threads it through `substrate_boiling_point_k` so
    // mountain cells boil at lower temperatures than coastal
    // cells. Sub-sea-level basins get slightly *higher* boil
    // points.
    // Thread the atmosphere's per-class scale height into
    // Hydrology so altitude-pressure varies correctly with the
    // planet's atmospheric type. Earth/Oxidising = 8400 m,
    // Mars/Thin = 11000, Venus/Reducing = 15000, Titan/Hazy =
    // 21000, None = 1 (vacuum).
    // Pass vacuum guard so Hydrology short-circuits on
    // Atmosphere::None worlds.
    let has_atmosphere = !matches!(planet.atmosphere, Atmosphere::None);
    let hydrology = sim_physics::Hydrology::for_substrate(
        planet.metabolic_substrate.tag(),
        planet.surface_pressure,
        planet.atmosphere.scale_height_m(),
        has_atmosphere,
    );

    // Lunar gravitational tides. Each moon
    // contributes a cos(2θ) bulge at its own period; the
    // per-cell potential is the mass-weighted superposition.
    // Multi-moon planets get genuine spring/neap-style
    // interference. Moonless planets pass an empty list and
    // the law no-ops.
    let moon_tides: Vec<sim_physics::MoonTide> = planet
        .moons
        .iter()
        .map(|m| sim_physics::MoonTide {
            mass_relative: Real::from_ratio(m.mass_relative_x100, 100),
            period_macros: m.orbital_period_macros,
            declination_r: m.inclination_deg_x10 / 30,
        })
        .collect();
    let tides = sim_physics::Tides::for_planet(moon_tides);

    // Planetary magnetic vector field. Strength comes from
    // the planet's `Magnetosphere` class (None / Weak / Strong);
    // direction is the axis-aligned dipole pattern (cos-latitude
    // magnitude, pointing from south pole toward north).
    let dipole_strength = match planet.magnetosphere {
        sim_world::Magnetosphere::None => Real::ZERO,
        sim_world::Magnetosphere::Weak => Real::from_int(10),
        sim_world::Magnetosphere::Strong => Real::from_int(50),
    };
    let magnetism = sim_physics::Magnetism::for_strength(dipole_strength);

    // Lorentz coupling. Runs after Magnetism so it reads the
    // freshest magnetic field magnitudes; couples charge × wind ×
    // magnetic field into a single consistent dynamics. Earth-like
    // default `lorentz_k = 1e-5` keeps the per-tick velocity nudge
    // far below `Wind`'s pressure-gradient acceleration.
    let lorentz = sim_physics::Lorentz::earth_like();

    // Coriolis deflection from the planet's rotation rate.
    // Faster spinners get stronger Coriolis (proportional to
    // 1/day_length).
    let coriolis = sim_physics::Coriolis::for_planet(planet.day_length_hours, has_atmosphere);

    // Vertical convection between surface and upper
    // atmosphere. Maintains a real lapse rate per cell.
    let vertical = sim_physics::VerticalConvection::earth_like();

    Laws {
        fluid,
        heat,
        em,
        chemistry,
        radiation,
        wind,
        hydrology,
        tides,
        magnetism,
        lorentz,
        coriolis,
        vertical,
    }
}

/// Combustion auto-ignition temperature for a given atmosphere.
/// Shared between `build_laws` (chemistry combustion gate) and the
/// `PlanetContext` consumed by `RecognitionLibrary::scan` so the
/// `fire` recognition template fires on the *same* threshold the
/// underlying chemistry uses. Single source of truth.
pub(crate) fn ignition_threshold_for(atmosphere: Atmosphere) -> Real {
    match atmosphere {
        Atmosphere::Oxidising => Real::from_int(500),
        Atmosphere::Hazy => Real::from_int(700),
        Atmosphere::Reducing => Real::from_int(900),
        Atmosphere::Thin => Real::from_int(800),
        Atmosphere::None => Real::from_int(1_000_000),
    }
}
