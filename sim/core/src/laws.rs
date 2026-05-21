//! Physics-law builders. Different seeds → different
//! planets → different law coefficients (structural
//! shapes fixed, parametric values vary). Pulled out of `lib.rs` so
//! the per-substrate / per-atmosphere coefficient tables sit next to
//! their rationale.

use sim_arith::Real;
use sim_physics::{
    chemistry::Chemistry, em::Electromagnetism, fluid::GravityFlow, heat::HeatConduction, Mechanics,
};
use sim_world::{Atmosphere, Magnetosphere};

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
    pub weathering: sim_physics::Weathering,
    pub ice_albedo: sim_physics::IceAlbedo,
    /// Sprint 4 Item 12: tectonics + fluvial erosion. Plate roster
    /// is populated by `sim_world::init_planet` after the grid is
    /// known (the sampler needs `HexGrid` dimensions). Default-
    /// constructed via `earth_like()` here — the integrator no-ops
    /// until the plate roster is installed.
    pub tectonics: sim_physics::Tectonics,
    /// Sprint 4 Item 12d: volcanic CO2 + H2O outgassing.
    pub volcanism: sim_physics::Volcanism,
    /// Sprint 5 Item 20: geomagnetic-reversal Markov chain. Drives the
    /// per-planet `dipole_state` / `dipole_strength` envelope that
    /// feeds `state.cosmic_ray_ground_flux()`. Earth-like trial rate
    /// (~1/250 000 per tick) + 1000-tick reversal window; planets
    /// without a magnetosphere still get this law installed but the
    /// effect is invisible because the per-cell vector field is zero.
    pub magnetic_reversal: sim_physics::MagneticReversal,
    /// Sprint 5 Item 23: cloud microphysics. Per-cell cloud
    /// fraction derived from vapour saturation + vertical-motion
    /// proxies; classified as cirrus vs stratus by elevation and
    /// updraft strength. Couples to albedo and greenhouse via the
    /// per-cell `cloud_fraction` + `cloud_type` fields the law
    /// authors.
    pub clouds: sim_physics::Clouds,
    /// Sprint 5 Item 16 (v2): per-moon tidal-heating descriptors.
    /// Mirrors `tides`/`moon_tides`: each `Moon` in `planet.moons`
    /// gets projected onto a `MoonHeating` (rocky default; substrate
    /// classification is a future refinement once worldgen tags moons
    /// with their own composition). Empty for moonless planets — the
    /// orchestrator call becomes a no-op.
    pub moon_heating: Vec<sim_physics::MoonHeating>,
    /// Sprint 5 Item 16 (v2): the planet's radius in Earth-radii,
    /// passed alongside `moon_heating` so the orchestrator's tidal-
    /// heating hook reads a consistent R⁵ value. Stored here rather
    /// than fetched from `planet.radius` at call time so the
    /// orchestrator's surface stays planet-agnostic.
    pub planet_radius_earth_units: Real,
    /// Sprint 5 Item 17: multi-channel atmospheric escape
    /// parameters (Jeans, hydrodynamic, photochemical, ion).
    /// Derived from the sampled planet's escape velocity, the
    /// host star's EUV / UV SED channels, and the planet's
    /// magnetosphere class — bundled in a single struct so the
    /// orchestrator can apply all four escape channels per
    /// macro-step without re-deriving them.
    pub atmospheric_escape: sim_physics::PlanetEscapeParams,
    /// Sprint 5 Item 15 / P0.2: Hadley / Ferrel / polar circulation
    /// cells. Pre-computed layout (`compute_hadley_layout`) plus the
    /// `apply_hadley_circulation` step packaged as a `Law` so the
    /// orchestrator can drop it into the macro-step pipeline. The
    /// number of cells per hemisphere emerges from the planet's
    /// rotation rate × radius via the Rossby deformation radius —
    /// Earth-likes get three cells, slow rotators collapse to one,
    /// rapid rotators get four-or-more. Vacuum planets short-circuit
    /// via the law's `has_atmosphere` flag.
    pub hadley: sim_physics::HadleyCirculation,
}

/// Build all physics laws with coefficients derived from a sampled
/// Planet. Different seeds → different planets → different law
/// coefficients → different physics outcomes (structural
/// shapes fixed, parametric values vary).
pub(crate) fn build_laws(planet: &sim_world::Planet, grid_height: u32) -> Laws {
    let mechanics = Mechanics {
        gravity: planet.gravity(),
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
    let radiation_base = sim_physics::Radiation::for_planet(
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
    // P1.5: thread the tidal-locking sub-stellar point through.
    // For `LockingState::Synchronous` planets the sub-stellar
    // point is fixed at (0, 0); the radiation law swaps its per-
    // row zonal-mean equilibrium for a per-cell great-circle-
    // distance day-night gradient anchored on that point.
    // Locked worlds get a permanent terminator — the
    // climatically interesting feature that distinguishes a
    // tidally-locked exoplanet from a free-rotator one. For
    // `Resonance` / `FreeRotator` the sub-stellar longitude
    // rotates with `macro_step`, so on the macro-step timescale
    // the day-night gradient washes out and the per-row table is
    // the right physics; pass `LockingMode::Other` and let the
    // diurnal-amplitude path handle the slow-rotator residual.
    let locking_mode = match planet.locking_state {
        sim_world::LockingState::Synchronous => sim_physics::LockingMode::Synchronous,
        sim_world::LockingState::FreeRotator | sim_world::LockingState::Resonance { .. } => {
            sim_physics::LockingMode::Other
        }
    };
    // `sub_stellar_point(planet, 0)` returns (0, 0) for
    // `Synchronous` (locked face); for non-synchronous regimes
    // the law ignores the value (`LockingMode::Other`).
    let (sub_lat, sub_lon) = sim_world::sub_stellar_point(planet, 0);
    let radiation = radiation_base.with_locking(locking_mode, sub_lat, sub_lon);

    // Atmospheric wind. Per-planet tuning lives in
    // `wind_for_atmosphere`: density scaling on all three
    // coefficients (advect_k ∝ ρ, wind_k ∝ 1/ρ, friction ∝ ρ) and
    // the scale-height plumb-through for the energy-conserving
    // advection pass. Short-circuits on `Atmosphere::None` worlds
    // via the `has_atmosphere` flag.
    let wind = wind_for_atmosphere(planet.atmosphere);

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

    // Sprint 5 Item 16 (v2): per-moon tidal-heating descriptors.
    // The lunar-bulge `tides` law moves water around; tidal heating
    // is the *energy* side of the same coupling — eccentric orbits
    // dissipate friction heat into the moon (and by extension the
    // planet's temperature field). We default every moon to a rocky
    // substrate (`k₂/Q ≈ 0.003`) as a v1 anchor — most rocky-planet
    // moons in our reference set (Earth's Moon, Io, Mars's moons)
    // are rocky. A future pass would let worldgen sample each moon's
    // composition independently and choose `MoonHeating::icy` for
    // Europa-class icy moons.
    let moon_heating: Vec<sim_physics::MoonHeating> = planet
        .moons
        .iter()
        .map(|m| sim_physics::MoonHeating::rocky(m.eccentricity, m.orbital_period_macros))
        .collect();
    let planet_radius_earth_units = planet.radius;

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

    // Carbon-silicate weathering thermostat. CO2 consumption
    // accelerates with temperature + precipitation; balances
    // Sprint 4's volcanism CO2 source so atmospheric CO2 (and
    // through the greenhouse effect, surface T) holds at a
    // habitable equilibrium instead of drifting toward Venus.
    let weathering = sim_physics::Weathering::earth_like();

    // Ice-albedo feedback (Sprint 3 Item 13). Sigmoid +
    // bimodal-channel albedo lets the radiative-balance loop
    // fall into one of two basins (snowball or habitable)
    // instead of relaxing to a unique intermediate
    // equilibrium. The freeze point is derived from the
    // planet's substrate so methane / ammonia / silicate
    // worlds transition at their own substrate's phase
    // boundary rather than Earth-water's 273 K.
    let substrate_tag = planet.metabolic_substrate.tag();
    let (substrate_freeze_k, _) =
        sim_physics::chemistry::substrate_phase_thresholds(substrate_tag);
    let perturb = Real::ONE + planet.substrate_perturbation;
    let ice_albedo = sim_physics::IceAlbedo::for_freeze_point(substrate_freeze_k * perturb);

    // Tectonics + fluvial erosion (Sprint 4 Item 12). Default
    // coefficients here; the per-planet plate roster is sampled in
    // `sim_world::init_planet` and installed via
    // `state.set_tectonics_fields(...)` + `Laws::install_tectonics`.
    let tectonics = sim_physics::Tectonics::earth_like();

    // Volcanic CO2 + H2O outgassing (Sprint 4 Item 12d).
    let volcanism = sim_physics::Volcanism::earth_like();

    // Geomagnetic-reversal Markov chain (Sprint 5 Item 20). One
    // earth-like calibration covers every planet — trial rate +
    // reversal duration are not currently substrate-tuned. Planets
    // with `Magnetosphere::None` still install the law: the
    // `cosmic_ray_ground_flux()` accessor reads the per-planet
    // dipole-strength envelope unconditionally, so the law continues
    // to mutate state even on no-dipole worlds (downstream couplings
    // can decide whether to ignore the result).
    let magnetic_reversal = sim_physics::MagneticReversal::earth_like();

    // Cloud microphysics (Sprint 5 Item 23). Per-cell cloud
    // fraction + type driven by vapour saturation and the
    // vertical-motion proxy. Earth-like defaults; per-substrate
    // tuning can come later.
    let clouds = sim_physics::Clouds::earth_like();

    // Hadley / Ferrel / polar circulation cells (Sprint 5 Item 15
    // / P0.2). Layout emerges from rotation × radius via the Rossby
    // deformation radius; the `apply_hadley_circulation` step inside
    // `Law::integrate` reads the layout each macro-step and applies
    // the angular-momentum-implied jet kick. Vacuum planets get
    // `has_atmosphere = false` so the law no-ops on `Atmosphere::None`.
    let hadley = sim_physics::HadleyCirculation::for_planet(
        planet.day_length_hours,
        planet.radius,
        planet.gravity(),
        i64::from(planet.atmosphere.scale_height_m()),
        has_atmosphere,
    );

    // Multi-channel atmospheric escape (Sprint 5 Item 17). Builds
    // the per-planet escape parameters from the sampled planet
    // properties: escape velocity from mass/radius, EUV / UV from
    // the host star's SED, magnetic strength from the magnetosphere
    // class. The orchestrator applies the four channels (Jeans,
    // hydrodynamic, photochemical, ion) once per macro-step after
    // chemistry.
    let atmospheric_escape = sim_physics::PlanetEscapeParams {
        escape_velocity_km_s: planet.escape_velocity(),
        euv_flux_w_m2: planet.star.euv_flux,
        uv_flux_w_m2: planet.star.uv_flux,
        magnetic_strength: match planet.magnetosphere {
            Magnetosphere::None => Real::ZERO,
            Magnetosphere::Weak => Real::ONE,
            Magnetosphere::Strong => Real::from_int(3),
        },
    };

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
        weathering,
        ice_albedo,
        tectonics,
        volcanism,
        magnetic_reversal,
        clouds,
        moon_heating,
        planet_radius_earth_units,
        atmospheric_escape,
        hadley,
    }
}

impl Laws {
    /// Replace the default-constructed `tectonics` law with one that
    /// holds the planet-specific plate roster. The roster is sampled
    /// by `sim_world::init_planet` (which has access to the
    /// `HexGrid`); `build_laws` runs before that point in the
    /// pipeline, so this setter exists to back-fill the roster once
    /// init has run. Future Sprint 4 sub-items that mutate the
    /// roster (subduction consuming a plate, slab-pull editing
    /// velocities) will use the same setter.
    pub(crate) fn install_tectonics(&mut self, tectonics: sim_physics::Tectonics) {
        self.tectonics = tectonics;
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

/// Build the per-planet `Wind` law, applying complete atmospheric-
/// density coupling on all three of its coefficients.
///
/// Previously only `advect_k` scaled with density, which left a half-
/// coupled asymmetry: a Mars-thin atmosphere generated Earth-strength
/// winds that just happened to advect very little heat. Physically the
/// pressure-gradient force per unit mass goes as `∇P / ρ` and ideal-
/// gas `P = ρ R T`, so the *acceleration* per Kelvin of gradient
/// scales as `1/ρ`. Drag (surface friction) is proportional to mass
/// per unit volume so scales linearly with `ρ`.
///
/// Three coefficients get tuned together:
///   * `advect_k` ∝ ρ          — thinner air carries less heat per
///     unit velocity. Capped at 5× to keep the CFL bound on the
///     upwind transport for Venus-like thick atmospheres.
///   * `wind_k` ∝ 1/ρ          — same gradient → bigger acceleration
///     in a thinner atmosphere (per-unit-mass force).
///   * `friction_per_tick` ∝ ρ — thinner air → less drag → wind
///     sustains longer. Clamped at 10 % of Earth-baseline so a Mars-
///     thin world still loses momentum eventually (vacuum guarded
///     separately via `has_atmosphere`).
///
/// Vacuum planets (`Atmosphere::None`) get `has_atmosphere = false`
/// so the integrator short-circuits; no scaling is applied because
/// the coefficients are unused.
pub(crate) fn wind_for_atmosphere(atmosphere: Atmosphere) -> sim_physics::Wind {
    let mut wind = sim_physics::Wind::earth_like();
    wind.has_atmosphere = !matches!(atmosphere, Atmosphere::None);
    // Thread the per-atmosphere scale height through so the
    // energy-conserving advection pass uses the right column-mass
    // ratios for this planet's air (Earth-like 8.4 km, Mars 11 km,
    // Venus 15 km, Titan 21 km, vacuum 1 m sentinel).
    wind.scale_height_m = atmosphere.scale_height_m();
    let density_x100 = atmosphere.density_x100();
    if density_x100 > 0 {
        let raw_density_factor = Real::from_int(density_x100) / Real::from_int(122);
        let advect_factor = raw_density_factor.min(Real::from_int(5));
        wind.advect_k = wind.advect_k * advect_factor;
        // Inverse density for the pressure-gradient acceleration:
        // dividing the Earth-baseline coefficient by `ρ/ρ_earth` is
        // equivalent to multiplying by `ρ_earth/ρ`. Clamp at 100×
        // so a Mars-thin atmosphere gets appreciably stronger
        // pressure-gradient force per K without overflowing fixed-
        // point on extreme ratios.
        let inverse_density_factor =
            (Real::from_int(122) / Real::from_int(density_x100)).min(Real::from_int(100));
        wind.wind_k = wind.wind_k * inverse_density_factor;
        // Linear density scaling for friction, with a floor at 10 %
        // of Earth-baseline so the thinnest non-vacuum atmospheres
        // still damp eventually.
        let friction_factor = raw_density_factor.max(Real::from_ratio(1, 10));
        wind.friction_per_tick = wind.friction_per_tick * friction_factor;
    }
    wind
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wind_density_scales_all_three_coefficients() {
        // Earth-baseline reference. Atmosphere::Oxidising has
        // `density_x100 = 122`, so the raw density factor is exactly
        // 1.0 — every coefficient should equal its Earth-like
        // baseline.
        let baseline = sim_physics::Wind::earth_like();
        let earth = wind_for_atmosphere(Atmosphere::Oxidising);
        assert_eq!(earth.advect_k, baseline.advect_k);
        assert_eq!(earth.wind_k, baseline.wind_k);
        assert_eq!(earth.friction_per_tick, baseline.friction_per_tick);
        assert!(earth.has_atmosphere);

        // Mars-thin: density_x100 = 2 (about 1/60 Earth). All three
        // coefficients should shift the right direction:
        //   advect_k goes down (less heat per unit velocity),
        //   wind_k goes up (more accel per K),
        //   friction_per_tick goes down (less drag, longer-lived
        //   winds).
        let mars = wind_for_atmosphere(Atmosphere::Thin);
        assert!(
            mars.advect_k < baseline.advect_k,
            "thin atmosphere should advect less heat per unit velocity: \
             mars.advect_k={:?} baseline.advect_k={:?}",
            mars.advect_k,
            baseline.advect_k
        );
        assert!(
            mars.wind_k > baseline.wind_k,
            "thin atmosphere should accelerate more per K of gradient: \
             mars.wind_k={:?} baseline.wind_k={:?}",
            mars.wind_k,
            baseline.wind_k
        );
        assert!(
            mars.friction_per_tick < baseline.friction_per_tick,
            "thin atmosphere should sustain wind longer: \
             mars.friction={:?} baseline.friction={:?}",
            mars.friction_per_tick,
            baseline.friction_per_tick
        );
        // Friction floor: 10 % of baseline = 0.30 × 0.10 = 0.03.
        // Mars raw factor (2/122 ≈ 0.0164) hits the floor.
        let floor = baseline.friction_per_tick * Real::from_ratio(1, 10);
        assert_eq!(
            mars.friction_per_tick, floor,
            "Mars-thin friction should clamp to the 10 % floor"
        );

        // Venus-reducing: density_x100 = 6700 (~55× Earth). The
        // advect_k cap at 5× must kick in; wind_k should drop sharply
        // (1/55); friction climbs (~55×).
        let venus = wind_for_atmosphere(Atmosphere::Reducing);
        let advect_cap = baseline.advect_k * Real::from_int(5);
        assert_eq!(
            venus.advect_k, advect_cap,
            "Venus-reducing should hit the 5× advect_k CFL cap"
        );
        assert!(
            venus.wind_k < baseline.wind_k,
            "Venus-thick atmosphere should accelerate less per K: \
             venus.wind_k={:?} baseline.wind_k={:?}",
            venus.wind_k,
            baseline.wind_k
        );
        assert!(
            venus.friction_per_tick > baseline.friction_per_tick,
            "Venus-thick atmosphere should drag more per tick: \
             venus.friction={:?} baseline.friction={:?}",
            venus.friction_per_tick,
            baseline.friction_per_tick
        );

        // Vacuum: has_atmosphere flag flips off; baseline coefficients
        // are kept (they're unused but harmless).
        let vacuum = wind_for_atmosphere(Atmosphere::None);
        assert!(!vacuum.has_atmosphere);
    }
}
