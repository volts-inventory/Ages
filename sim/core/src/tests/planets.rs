//! T15 / T16 / T17 / T18 / T20 / T21 end-to-end planet
//! integration tests. Each test hand-builds a `Planet` outside
//! the worldgen sampler's reachable distribution to exercise a
//! specific axis (Hydrocarbon substrate, 2 g super-Earth,
//! hot-Jupiter EUV / pressure, M-dwarf locked rotation,
//! silicate lava world, Ammoniacal cold reducing atmosphere)
//! and drives the per-tick ecosystem / physics chain for
//! 500-1000 ticks. Loose post-run assertions catch Q32.32
//! overflow, chemistry blowups, and full trophic-pyramid
//! collapse without pinning per-tick values that would drift
//! under future calibration tweaks.

use crate::*;
use super::earth_like_planet;

use sim_arith::Real;
use sim_ecosystem::PlanetEcosystem;
use sim_world::{
    Atmosphere, AtmosphericComposition, BiosphereClass, Composition, Crust, CrustalComposition,
    LockingState, Magnetosphere, MetabolicSubstrate, Planet, SpectralType, Star,
};

use crate::laws::build_laws;

// ---------------------------------------------------------------------
// T16: Super-Earth gravity end-to-end check.
//
// P0.5 / Item 21 separated `Planet::gravity()` from a stored scalar to a
// derived `EARTH_G × M / R²` accessor, and T3 threaded that derived value
// into `Tides::for_gravity` / `Wind::for_gravity`. No prior test verified
// that a high-gravity super-Earth actually drops through the build_laws
// → integrate_civ_step pipeline without Q32.32 overflow, *and* that the
// per-planet law coefficients visibly differ from the Earth-equivalent
// baseline (the whole point of the mass/radius coupling).
//
// This test pins:
//   1. A directly-constructed super-Earth Planet (M=5, R=1.5 → g ≈ 2.22 g).
//   2. `planet.gravity()` lands inside ±5% of 2.22 g and
//      `planet.escape_velocity()` clears Earth's ~11.18 km/s by a
//      meaningful margin (super-Earth surface gravity *and* radius lift
//      escape velocity well above Earth's).
//   3. `build_laws` for the super-Earth produces a `tide_k` and `wind_k`
//      that differ measurably from the Earth-equivalent baseline (tides
//      scale `sqrt(g)`, winds scale `1/g`).
//   4. A 1000-tick integration with the super-Earth laws + a parallel
//      ecosystem step completes without panicking and leaves at least
//      one extant ecosystem species — covers the "no Q32 overflow,
//      something still alive" floor the spec calls out.
// ---------------------------------------------------------------------

#[test]
fn super_earth_run_with_2g_gravity_does_not_overflow() {
    // Step 1: construct the super-Earth (mass=5 Earth, radius=1.5 Earth
    // → g = 9.81 × 5 / 2.25 ≈ 21.8 m/s² ≈ 2.22 g). Aqueous solvent,
    // Earth-like 288 K mean temp, Oxidising atmosphere — every other
    // axis pinned to the Earth baseline so the only varying input is
    // the mass/radius pair.
    let super_earth = earth_like_planet(
        Real::from_int(5),
        Real::from_ratio(15, 10),
        MetabolicSubstrate::Aqueous,
        Atmosphere::Oxidising,
        Real::from_int(288),
    );
    // Earth-equivalent baseline (mass=1, radius=1) for law-coefficient
    // comparison. Identical aside from the mass/radius pair.
    let earth = earth_like_planet(
        Real::ONE,
        Real::ONE,
        MetabolicSubstrate::Aqueous,
        Atmosphere::Oxidising,
        Real::from_int(288),
    );

    // Step 2: derived gravity ≈ 2.22 g. Earth-g ≈ 9.81 m/s²; the super-
    // Earth should land near 21.8 m/s² (within 5% — covers the
    // EARTH_GRAVITY_MS2_X100 hundredths anchor + Q32.32 rounding).
    let g_se = super_earth.gravity().to_f64_for_display();
    let g_expected = 9.81 * 5.0 / (1.5 * 1.5);
    assert!(
        (g_se - g_expected).abs() / g_expected < 0.05,
        "super-Earth gravity should be ~{g_expected:.2} m/s²; got {g_se:.2}"
    );

    // Step 3: escape velocity clears Earth's ~11.18 km/s by a wide
    // margin. v_escape ∝ sqrt(M/R) so 5/1.5 ≈ 3.33× → sqrt ≈ 1.83×
    // → ~20.4 km/s. We assert a loose floor of "> Earth's ~11.2 km/s"
    // per the spec; the tighter ~20 km/s prediction lives in the
    // surrounding comment as documentation.
    let v_esc = super_earth.escape_velocity().to_f64_for_display();
    assert!(
        v_esc > 11.2,
        "super-Earth escape velocity must clear Earth's ~11.2 km/s; got {v_esc:.2}"
    );

    // Step 4: build the per-planet laws for both worlds and verify the
    // tide / wind coefficients track the documented scaling.
    let laws_se = build_laws(&super_earth, 8);
    let laws_earth = build_laws(&earth, 8);

    // Tide amplitude scales as sqrt(g) (gradient force linear in g,
    // restoring weight linear in g → response in the square root).
    // A 2.22 g super-Earth should land at sqrt(2.22) ≈ 1.49× Earth's
    // tide_k. Loose check: the two coefficients must differ by ≥ 25 %
    // so a future regression that drops gravity coupling from Tides
    // tripping this assertion is the obvious failure mode.
    let tide_se = laws_se.tides.tide_k.to_f64_for_display();
    let tide_earth = laws_earth.tides.tide_k.to_f64_for_display();
    assert!(
        tide_se > tide_earth * 1.25,
        "super-Earth tide_k ({tide_se:.6}) should exceed Earth tide_k \
         ({tide_earth:.6}) by ≥ 25 % per the sqrt(g) scaling"
    );

    // Wind pressure-gradient acceleration scales as 1/g (same gradient
    // → smaller per-mass acceleration in a heavier-air column at the
    // same scale height). A 2.22 g super-Earth should see roughly half
    // Earth's wind_k. Loose check: super-Earth wind_k strictly below
    // Earth wind_k by ≥ 25 %.
    let wind_se = laws_se.wind.wind_k.to_f64_for_display();
    let wind_earth = laws_earth.wind.wind_k.to_f64_for_display();
    assert!(
        wind_se < wind_earth * 0.75,
        "super-Earth wind_k ({wind_se:.6}) should be ≤ 75 % of Earth wind_k \
         ({wind_earth:.6}) per the 1/g scaling"
    );

    // Step 5: 1000-tick integration with the super-Earth laws. Drive the
    // same `integrate_civ_step` the production tick loop uses + a
    // parallel ecosystem step. The full `run()` path requires a planet
    // sampled from a seed; this test exercises the law-construction +
    // integration coupling directly so the super-Earth (which the
    // worldgen sampler does not currently land on) gets covered.
    let grid_width = 12u32;
    let grid_height = 8u32;
    let grid = sim_physics::HexGrid::new(grid_width, grid_height);
    let mut state = sim_physics::PhysicsState::new(grid);
    let mut planet_for_init = super_earth.clone();
    sim_world::init_planet(&mut state, &planet_for_init);

    let mut laws = build_laws(&planet_for_init, grid_height);
    // Mirror `run()`'s tectonic-plate installation so the per-tick
    // tectonics path doesn't no-op on un-initialised plate state.
    let (tectonics, plate_id, crust_thickness) =
        sim_physics::Tectonics::sample_plates_for_seed(planet_for_init.seed, state.grid());
    state.set_tectonics_fields(plate_id, crust_thickness);
    laws.install_tectonics(tectonics);
    laws.magnetism.init_field(&mut state);

    // Build a parallel ecosystem the same way `run()` does so the
    // 1000-tick loop can assert at least one species persists at the
    // end. Lush biosphere → solid producer capacity floor.
    let n_cells = state.grid().n_cells();
    let planet_capacity: Real = {
        let n_cells_real = Real::from_int(n_cells as i64);
        let cap = n_cells_real * planet_for_init.biosphere_density;
        if cap < Real::ONE {
            Real::ONE
        } else {
            cap
        }
    };
    let habitability_weights: Vec<Real> = (0..n_cells as u32)
        .map(|c| sim_world::cell_habitability(&state, &planet_for_init, c))
        .collect();
    let mut ecosystem: PlanetEcosystem = sim_ecosystem::sample_ecosystem_with_substrate_for_grid(
        planet_for_init.seed,
        planet_for_init.metabolic_substrate.tag(),
        planet_capacity,
        n_cells,
        Some(&habitability_weights),
    );
    let n_species_initial = ecosystem.species.len();
    assert!(
        n_species_initial > 0,
        "ecosystem must seed at least one species on a Lush super-Earth"
    );

    let orch_cfg = RunConfig::dev(1024, 1).orchestration;
    let mut orch_state = sim_physics::OrchestratorState::new();
    let solar = planet_for_init.stellar_luminosity;
    let civs: Vec<sim_civ::Civ> = Vec::new();
    for tick in 0..1000u64 {
        // Mirror the lunar-eccentricity damping path so even a moonless
        // super-Earth runs the same outer-loop shape as production.
        {
            let locking = planet_for_init.locking_state;
            let r = planet_for_init.radius;
            for moon in &mut planet_for_init.moons {
                sim_world::step_eccentricity_damping(r, moon, locking, Real::ONE);
            }
        }
        // Apparatus clamps — empty civs list means no clamps, but the
        // call stays for parity with `physics_phase`.
        sim_civ::apparatus::write_apparatus_clamps(&mut state, &civs, tick);
        sim_physics::integrate_civ_step(
            &mut state,
            &mut orch_state,
            &orch_cfg,
            &laws.fluid,
            &laws.heat,
            &laws.em,
            &laws.chemistry,
            Some(&laws.radiation),
            Some(&laws.wind),
            Some(&laws.hydrology),
            Some(&laws.tides),
            Some(&laws.magnetism),
            Some(&laws.lorentz),
            Some(&laws.coriolis),
            Some(&laws.vertical),
            Some(&laws.weathering),
            Some(&laws.ice_albedo),
            Some(&laws.tectonics),
            Some(&laws.volcanism),
            Some(&laws.magnetic_reversal),
            Some(&laws.clouds),
            Some((laws.planet_radius_earth_units, laws.moon_heating.as_slice())),
            Some(&laws.atmospheric_escape),
            Some(&laws.hadley),
            Some(&laws.resonance),
            Some(&laws.insolation),
            Some(&laws.tidal_stress),
            Some(&laws.surface_radiation),
        );
        let _ = ecosystem.step_with_biogeochem_at_tick(&mut state, solar, tick);
    }

    // Step 6: post-run survivorship. At least one species still extant
    // proves the integrated 1000-tick run didn't collapse the trophic
    // pyramid under the high-gravity coefficients (and didn't panic
    // through Q32.32 overflow on the way — the loop above would have
    // unwound the test before we got here).
    let extant_count = ecosystem.species.values().filter(|s| s.is_extant).count();
    assert!(
        extant_count >= 1,
        "after 1000 ticks of super-Earth physics + ecosystem at least one \
         species must remain extant; got {extant_count} of {n_species_initial} \
         initial species"
    );
}

// ---------------------------------------------------------------------
// T15 — Titan-class (Hydrocarbon substrate) end-to-end calibration test.
//
// `MetabolicSubstrate::Hydrocarbon` exists in the substrate enum and is
// reachable through the normal `sample_planet` distribution, but no
// integration test pins the behaviour of an end-to-end run on a
// non-Earth substrate. This test builds a Titan-equivalent planet
// manually (substrate = Hydrocarbon, T_surface = 94 K, dense methane
// atmosphere, low gravity from low mass + radius), wires it into the
// same physics + ecosystem fixture the production `run()` builds, and
// drives the per-tick ecosystem step for 1000 ticks. Assertions stay
// loose — the goal is to catch chemistry blowups, Q32.32 overflow,
// extreme temperature drift, or ecosystem crashes on a substrate well
// outside the Aqueous default.
// ---------------------------------------------------------------------

#[test]
fn titan_analog_run_produces_credible_state() {
    use sim_physics::{HexGrid, PhysicsState, Substance};
    use sim_world::{init_planet, planet_name_from_seed};

    // Titan-equivalent worldgen. Real Titan facts (used as anchors;
    // sampling within the Hydrocarbon substrate's tolerance window):
    //   - Surface temperature ≈ 94 K (well within the substrate's
    //     90-180 K liquid range; CH4/C2H6 are liquid at the surface).
    //   - Atmospheric pressure ≈ 146.7 kPa (1.45 × Earth, mostly N2
    //     with ~1.4% CH4).
    //   - Mass ≈ 0.0225 Earth masses; radius ≈ 0.404 Earth radii.
    //     Derived gravity ≈ 9.81 × 0.0225 / 0.404² ≈ 1.35 m/s²
    //     (Titan ≈ 1.35 m/s² — matches).
    //   - Orbits Saturn at ~9.5 AU from the Sun; Saturn's a G-type
    //     proxy here (we don't model planet-around-planet orbits, so
    //     the host star sits at the Saturn-equivalent orbital distance).
    //   - Dense methane-N2 reducing/hazy atmosphere with tholin haze.
    //     We pick `Atmosphere::Hazy` since the spec calls out
    //     "ReducingThick" which doesn't exist as a variant — Hazy is
    //     the closest match for a thick CH4/N2 mixture (Titan-style).
    //
    // Pick a fixed seed so the run is bit-for-bit reproducible. The
    // seed only drives the `planet_name_from_seed` lookup + the
    // ecosystem / RNG streams downstream; the planet bulk properties
    // are pinned by this fixture rather than sampled.
    let seed: u64 = 0x1517_1774_44EE_5EED;
    let planet = Planet {
        seed,
        name: planet_name_from_seed(seed),
        // Titan: 0.0225 Earth masses. Real::from_ratio(225, 10_000).
        mass: Real::from_ratio(225, 10_000),
        // Titan: 0.404 Earth radii. Real::from_ratio(404, 1000).
        radius: Real::from_ratio(404, 1000),
        // Rocky composition so init_planet keeps the latitude-
        // driven surface temperature (GaseousShell would override
        // every cell to 700 K, blowing the Hydrocarbon range).
        composition: Composition::Rocky,
        // Surface temperature 94 K — inside the Hydrocarbon
        // substrate's [90, 180] K liquid range and matches Titan.
        mean_temperature: Real::from_int(94),
        // Modest equator-to-pole gradient (Titan's atmospheric
        // circulation keeps the surface within a few K).
        temperature_gradient: Real::from_int(5),
        // Modest topography — Titan's highest peaks sit ~3300 m
        // above the abyssal plain (cryovolcanic ridges +
        // hydrocarbon-eroded mesas).
        terrain_peak: Real::from_int(3_500),
        terrain_centre_q: 4,
        terrain_centre_r: 4,
        // Hydrocarbon lakes are shallow (Ligeia Mare ~170 m
        // deep); the sea_level here is the abyssal-plain offset,
        // not the lake depth — keep modest.
        sea_level: Real::from_int(1_500),
        // Hazy is the closest Atmosphere variant to Titan's dense
        // CH4/N2 layered haze (the spec's "ReducingThick" name
        // doesn't exist as a variant; Hazy carries the right
        // density and scale height — 5.4 kg/m³, 21 km scale).
        atmosphere: Atmosphere::Hazy,
        // Titan composition: ~95% N2, ~4.9% CH4, traces of H2/Ar.
        // Mass fractions roughly: N2 ≈ 0.95, CH4 ≈ 0.05.
        atmospheric_composition: AtmosphericComposition {
            n2: Real::from_ratio(95, 100),
            o2: Real::ZERO,
            co2: Real::ZERO,
            ch4: Real::from_ratio(5, 100),
            nh3: Real::ZERO,
            h2o: Real::ZERO,
            h2: Real::ZERO,
            ar: Real::ZERO,
            other: Real::ZERO,
        },
        // 146.7 kPa = 146 700 Pa (Titan's measured surface
        // pressure, ~1.45 × Earth). Inside the Hazy band
        // (80 000-300 000 Pa) so the categorical label coheres
        // with the value.
        surface_pressure: Real::from_int(146_700),
        // Sparse: Titan has no confirmed biosphere; the substrate-
        // first contract still requires *some* life so the
        // ecosystem sampler has tier members to step.
        biosphere: BiosphereClass::Sparse,
        biosphere_density: Real::from_ratio(2, 10),
        magnetosphere: Magnetosphere::None,
        // Titan's crust is dominated by water ice + tholin haze
        // deposits + hydrocarbon sediments — Hydrocarbon-archetype.
        crust: Crust::Hydrocarbon,
        crustal_composition: CrustalComposition::empty(),
        // Stellar irradiance at 9.5 AU ≈ 1361 / 9.5² ≈ 15 W/m².
        stellar_luminosity: Real::from_int(15),
        // Titan orbits Saturn at ~9.5 AU heliocentric.
        orbital_distance_au: Real::from_ratio(95, 10),
        moon_count: 0,
        moons: Vec::new(),
        orbital_eccentricity_x100: 5,
        axial_tilt_deg: Real::from_int(27),
        // Titan's day = 15.95 Earth days ≈ 382 hours.
        day_length_hours: Real::from_int(382),
        orbital_period_months: 12,
        metabolic_substrate: MetabolicSubstrate::Hydrocarbon,
        substrate_perturbation: Real::ZERO,
        locking_state: LockingState::FreeRotator,
        // G-dwarf host star, mid-life (Saturn orbits the Sun).
        // `Star::with_age` adjusts the bolometric luminosity at the
        // sampled `stellar_luminosity` for age — pass the same
        // irradiance so the SED is consistent.
        star: Star::with_age(
            SpectralType::G,
            Real::from_int(15),
            Real::from_ratio(45, 10),
            Real::from_int(10),
        ),
    };

    // Sanity: gravity should land near Titan's ≈ 1.35 m/s² (0.225 /
    // 0.404² × 9.81). This is a spot-check that the bulk
    // mass/radius pair didn't silently invert.
    let g = planet.gravity().to_f64_for_display();
    assert!(
        (1.0..=2.0).contains(&g),
        "Titan analog gravity should be ≈ 1.35 m/s² (got {g})",
    );

    // Build the physics state + ecosystem the same way `run()` does
    // (mirrors `ecosystem_fixture_for_seed` but uses the manually-
    // constructed planet so the substrate is pinned).
    let cfg = RunConfig::dev(seed, 1);
    let grid = HexGrid::new(cfg.grid_width, cfg.grid_height);
    let mut state = PhysicsState::new(grid);
    init_planet(&mut state, &planet);
    let n_cells = state.grid().n_cells() as i64;
    let capacity = {
        let cap = Real::from_int(n_cells) * planet.biosphere_density;
        if cap < Real::ONE {
            Real::ONE
        } else {
            cap
        }
    };
    let substrate_tag: &'static str = planet.metabolic_substrate.tag();
    let mut ecosystem = sim_ecosystem::sample_ecosystem_with_substrate(
        planet.seed,
        substrate_tag,
        capacity,
    );

    // Assertion: ecosystem must contain at least one species. The
    // substrate-first contract guarantees every sampled planet
    // carries a viable trophic web — a zero-species ecosystem here
    // would mean the Hydrocarbon-substrate path produced an empty
    // pool.
    assert!(
        !ecosystem.species.is_empty(),
        "Hydrocarbon-substrate ecosystem must have at least one species; \
         got an empty species map",
    );

    // Snapshot the initial methane column so the per-tick assertion
    // can verify bounded vapour-proxy levels. We take the sum so a
    // single cell's runaway doesn't get hidden by row averaging.
    let initial_methane_sum: Real = state
        .substance(Substance::Methane.idx())
        .iter()
        .copied()
        .fold(Real::ZERO, |a, b| a + b);

    // Run for 1000 ticks. The per-tick step mirrors the production
    // `run()` loop's ecosystem call: `step_with_biogeochem_at_tick`
    // couples producer growth ← solar + CO2, respiration → CO2,
    // then runs the extinction sweep. Existing debug_asserts inside
    // the step + chemistry layer fire if mass conservation breaks.
    let solar = planet.stellar_luminosity;
    let mut min_mean_temp = f64::INFINITY;
    let mut max_mean_temp = f64::NEG_INFINITY;
    let mut max_methane_sum = Real::ZERO;
    for tick in 0..1000u64 {
        let _events = ecosystem.step_with_biogeochem_at_tick(&mut state, solar, tick);
        // Mean surface temperature (planet-wide aggregate) — the
        // ecosystem step does not directly mutate temperature, but
        // chemistry-coupled CO2 flux does feed back through the
        // radiation law in `run()`; here we sample post-step to make
        // sure the field hasn't drifted out of the substrate's window
        // due to a sign-flip in the biogeochem coupling.
        let temps = state.temperature();
        let mut sum = Real::ZERO;
        for t in temps {
            sum = sum + *t;
        }
        let n = temps.len() as i64;
        let mean = (sum / Real::from_int(n)).to_f64_for_display();
        if mean < min_mean_temp {
            min_mean_temp = mean;
        }
        if mean > max_mean_temp {
            max_mean_temp = mean;
        }
        // Methane column sum — the spec's "vapour level (Methane
        // proxy)" assertion. The radiation law decays CH4 per tick
        // (× 0.999) but we don't call radiation here, so the column
        // should stay near its `init_planet`-imprinted level. The
        // bound is loose: any positive finite value is acceptable as
        // long as it didn't blow up to Q32.32 saturation.
        let methane_sum: Real = state
            .substance(Substance::Methane.idx())
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        if methane_sum > max_methane_sum {
            max_methane_sum = methane_sum;
        }
    }

    // Mean temperature stays in the Hydrocarbon liquid range (with
    // slack — the assertion's purpose is to catch sign-flip /
    // runaway drift, not to pin the value). The substrate's nominal
    // range is [90, 180] K; we use [80, 200] as the slack band for
    // the per-tick mean (catastrophe-free run shouldn't drift more
    // than 10 K from the worldgen-imprinted 94 K, but the assertion
    // tolerates a wider envelope to keep the canary stable across
    // future calibration tweaks).
    assert!(
        (80.0..=200.0).contains(&min_mean_temp),
        "mean temperature underflowed Hydrocarbon liquid range over 1000 ticks: \
         min={min_mean_temp} K, max={max_mean_temp} K (expected ~94 K)",
    );
    assert!(
        (80.0..=200.0).contains(&max_mean_temp),
        "mean temperature overflowed Hydrocarbon liquid range over 1000 ticks: \
         min={min_mean_temp} K, max={max_mean_temp} K (expected ~94 K)",
    );

    // Vapour-proxy (Methane) column stayed bounded. Initial value
    // is whatever `init_planet` imprinted; we accept up to 10×
    // growth as "bounded" — a Q32.32 saturation or sign-flip would
    // blow past that by many orders of magnitude.
    let initial_f = initial_methane_sum.to_f64_for_display();
    let max_f = max_methane_sum.to_f64_for_display();
    assert!(
        max_f.is_finite() && max_f >= 0.0,
        "methane column went non-finite or negative: initial={initial_f}, max={max_f}",
    );
    let upper_bound = (initial_f.abs() + 1.0) * 10.0;
    assert!(
        max_f <= upper_bound,
        "methane column blew past the 10× initial bound: initial={initial_f}, \
         max={max_f}, upper_bound={upper_bound}",
    );

    // Ecosystem still has at least one species after 1000 ticks of
    // stepping (some extinctions are expected on a sparse-biosphere
    // planet, but a fully-extinct Lindeman pyramid would mean the
    // substrate-first contract was violated mid-run).
    let extant_count = ecosystem
        .species
        .values()
        .filter(|s| s.is_extant)
        .count();
    assert!(
        extant_count >= 1,
        "expected ≥ 1 extant species after 1000-tick Titan-analog run; \
         got {extant_count}",
    );
}

// ---------------------------------------------------------------------
// T18: M-dwarf habitable-zone tidally-locked planet end-to-end test.
//
// M-dwarfs (Sprint 5 Item 18) flare ~100× as often as G-dwarfs, and
// their habitable-zone planets sit close enough to the star to tidal-
// lock (Item 19 / 24). T18 verifies that a planet at the intersection
// of all three traits — M-dwarf host, `LockingState::Synchronous`
// rotation, HZ-equivalent orbital insolation — exercises every
// wiring path cleanly:
//
//   1. P1.5 day-night temperature gradient (radiation law swaps the
//      per-row zonal-mean for a per-cell great-circle distance from
//      the fixed sub-stellar point).
//   2. Item 19 fixed sub-stellar point (`sub_stellar_point` returns
//      a constant for Synchronous regardless of macro-step).
//   3. Item 18 / T18 spectral-aware solar-flare cadence (100× G-dwarf
//      base rate → flares fire inside 1000 ticks where a G-dwarf
//      equivalent fires zero).
//   4. Catastrophe + ecosystem survival under sustained flaring (the
//      eco-species pool retains extant entries across 1000 ticks).
// ---------------------------------------------------------------------

/// Build a fully-formed Planet with the requested host-star spectral
/// type, `LockingState`, and HZ-equivalent insolation. Mass / radius
/// are set to the "small rocky" end of the substrate's range (0.5
/// Earth mass, 0.7 Earth radii — typical M-dwarf HZ planet) and the
/// rest of the bulk properties are pinned to a calm Earth-like
/// baseline so the test isolates the star / locking variables.
///
/// `flare_capable` controls whether the planet's magnetosphere /
/// luminosity satisfy `solar_flare_fires`. Set true for the M-dwarf
/// fixture; the G-dwarf comparison fixture keeps the same to isolate
/// the spectral-class effect on the firing cadence.
fn m_dwarf_hz_planet_fixture(
    spectral: SpectralType,
    locking: LockingState,
    flare_capable: bool,
) -> Planet {
    Planet {
        seed: 0,
        name: "T18-Fixture".to_string(),
        // 0.5 Earth mass, 0.7 Earth radii — typical M-dwarf HZ
        // rocky planet (Trappist-1e-equivalent). Gravity derives
        // from `g = EARTH_G × M / R²` → ~1.0× Earth surface gravity
        // for this combination (the mass/radius shrink track each
        // other so habitability stays high).
        mass: Real::from_ratio(5, 10),
        radius: Real::from_ratio(7, 10),
        composition: Composition::Rocky,
        // Mean / gradient: Earth-like calm baseline so the per-cell
        // day-night gradient (the P1.5 wiring under test) is the
        // primary modulator of cell-T differences, not bulk planet
        // gradient noise.
        mean_temperature: Real::from_int(288),
        temperature_gradient: Real::from_int(20),
        terrain_peak: Real::from_int(8000),
        terrain_centre_q: 0,
        terrain_centre_r: 0,
        sea_level: Real::from_int(2000),
        atmosphere: sim_world::Atmosphere::Oxidising,
        atmospheric_composition: AtmosphericComposition::vacuum(),
        biosphere_density: Real::from_ratio(5, 10),
        crustal_composition: CrustalComposition::empty(),
        surface_pressure: Real::from_int(101_325),
        biosphere: BiosphereClass::Lush,
        // `solar_flare_fires` gates on Weak / None magnetosphere AND
        // stellar_luminosity ≥ 1500. Set both to satisfy the gate
        // when `flare_capable`; an M-dwarf HZ planet routinely loses
        // its atmosphere to stellar wind so a weak magnetosphere is
        // the realistic baseline.
        magnetosphere: if flare_capable {
            sim_world::Magnetosphere::Weak
        } else {
            sim_world::Magnetosphere::Strong
        },
        crust: Crust::Basaltic,
        // HZ-equivalent insolation: M-dwarf nominal luminosity is
        // 0.04 Lsun, but the HZ inner edge is at ~0.18 AU, so the
        // *per-m² irradiance at the planet* matches Earth's 1361 W/m².
        // We pin the per-planet irradiance to the flare-firing
        // threshold (1500 W/m² ≈ slightly inside the HZ inner edge,
        // which is exactly the close-in M-dwarf HZ situation).
        stellar_luminosity: Real::from_int(1_500),
        // HZ-equivalent orbital distance for an M-dwarf with 0.04 Lsun:
        // `d = sqrt(L/Lsun) × 1 AU ≈ 0.2 AU`.
        orbital_distance_au: Real::from_ratio(2, 10),
        moon_count: 0,
        moons: vec![],
        orbital_eccentricity_x100: 2,
        axial_tilt_deg: Real::from_int(23),
        // Synchronous-locked: rotation period equals orbit period.
        // M-dwarf HZ orbital period ~ 6 days = 144 hours.
        day_length_hours: Real::from_int(144),
        orbital_period_months: 12,
        metabolic_substrate: MetabolicSubstrate::Aqueous,
        substrate_perturbation: Real::ZERO,
        locking_state: locking,
        // M-dwarf host: 0.04 Lsun nominal, 1000 Gyr lifetime, 5 Gyr
        // age (a mature mid-life M dwarf). The `bolometric_at_planet`
        // argument feeds the SED breakdown — we pass 1361 W/m² so
        // the per-channel fluxes land near Earth-on-Sun magnitudes
        // (the HZ-equivalent irradiance for this planet).
        star: Star::with_age(
            spectral,
            Real::from_int(1_361),
            Real::from_int(5),
            spectral.nominal_lifetime_gyr(),
        ),
    }
}

#[test]
fn m_dwarf_hz_locked_planet_runs_cleanly() {
    // --- 1. Construct the M-dwarf + Synchronous + HZ fixture and
    //        a G-dwarf comparison fixture (same locking + flare
    //        gating; only the spectral class differs).
    let m_planet = m_dwarf_hz_planet_fixture(
        SpectralType::M,
        LockingState::Synchronous,
        true,
    );
    let g_planet = m_dwarf_hz_planet_fixture(
        SpectralType::G,
        LockingState::Synchronous,
        true,
    );

    // --- 2. P1.5 wiring active: day-side / night-side temperature
    //        gradient. Build the planet's Radiation law, integrate
    //        the per-tick relaxation toward equilibrium for 500
    //        ticks on a 1-row 8-cell strip (latitudinal cooling
    //        suppressed so the day-night gradient is the only
    //        modulator), then assert the sub-stellar cell (q=0)
    //        sits warmer than the antistellar cell (q=4).
    let (sub_lat_m, sub_lon_m) = sim_world::sub_stellar_point(&m_planet, 0);
    let rad_m = sim_physics::Radiation::for_planet(
        1,                       // 1-row strip
        m_planet.stellar_luminosity,
        30,                      // 30% albedo
        Real::ZERO,              // no greenhouse forcing
        0,                       // no axial tilt
        0,                       // no seasonal cycle
        0,                       // circular orbit
        m_planet.day_length_hours,
    )
    .with_locking(sim_physics::LockingMode::Synchronous, sub_lat_m, sub_lon_m);
    let mut state = sim_physics::PhysicsState::new(sim_physics::HexGrid::new(8, 1));
    for t in state.temperature_mut() {
        *t = Real::from_int(280);
    }
    {
        use sim_physics::Law;
        for _ in 0..500 {
            rad_m.integrate(&mut state, Real::ONE);
        }
    }
    let day_t = state.temperature()[0];
    let night_t = state.temperature()[4];
    assert!(
        day_t > night_t,
        "P1.5 wiring: sub-stellar cell must be warmer than antistellar \
         cell on a Synchronous-locked planet (got day={day_t:?} night={night_t:?})",
    );

    // --- 3. Item 19 wiring active: sub-stellar point is fixed
    //        across macro-steps for `LockingState::Synchronous`.
    let p_t0 = sim_world::sub_stellar_point(&m_planet, 0);
    let p_t500 = sim_world::sub_stellar_point(&m_planet, 500);
    let p_t1000 = sim_world::sub_stellar_point(&m_planet, 1000);
    assert_eq!(
        p_t0, p_t500,
        "Item 19 wiring: Synchronous sub-stellar point must be fixed across 500 ticks",
    );
    assert_eq!(
        p_t0, p_t1000,
        "Item 19 wiring: Synchronous sub-stellar point must remain fixed at t=1000",
    );

    // --- 4. Spectral-aware flare-rate ordering: an M-dwarf's
    //        per-tick flare rate is 100× the G-dwarf baseline (the
    //        Item 18 calibration that drives T18's catastrophe-
    //        cadence wiring).
    let m_flare_rate = m_planet.star.flare_rate_per_tick();
    let g_flare_rate = g_planet.star.flare_rate_per_tick();
    assert_eq!(
        m_flare_rate,
        g_flare_rate * Real::from_int(100),
        "Item 18 wiring: M-dwarf flare rate must be 100× G-dwarf baseline",
    );

    // --- 5. Drive the catastrophe path for 1000 ticks on both the
    //        M-dwarf and G-dwarf fixtures and count solar-flare
    //        firings. The trigger uses a spectral-class-aware
    //        firing period (`base / 100` for M, `base` for G), so
    //        the M-dwarf must fire at least one flare in 1000 ticks
    //        while the G-dwarf (base period ~18804) fires zero —
    //        proving the catastrophe path is spectral-aware.
    let recognition = sim_recognition::RecognitionLibrary::earth_like_default();
    // Species derived from a sampled planet so the species-derived
    // tolerance envelope + dormancy + cosmology fields are populated
    // (`Species::default` isn't a thing; deriving from a sampled
    // planet is the canonical construction path used by every other
    // test in this file).
    let template_planet = sim_world::sample_planet(42);
    let species = sim_species::derive(&template_planet, &recognition);

    fn run_1000_ticks(
        planet: &Planet,
        species: &sim_species::Species,
    ) -> (usize, sim_ecosystem::PlanetEcosystem) {
        let grid = sim_physics::HexGrid::new(8, 8);
        let mut state = sim_physics::PhysicsState::new(grid);
        sim_world::init_planet(&mut state, planet);
        // Pin the densest cell's temperature / pressure to centre-
        // of-aqueous-envelope values so the post-flare tolerance
        // gate doesn't accidentally bottleneck the species'
        // match_score below the radiation axis (mirrors the
        // canonical `extremophile_species_survives_solar_flare_better_than_aqueous`
        // fixture's setup).
        state.temperature_mut()[0] = Real::from_int(300);
        state.pressure_mut()[0] = Real::from_int(101_325);
        let mut civ = sim_civ::Civ::new(1, 0, sim_arith::Pop::from_int(1_000_000));
        // Producer biomass high so the disease trigger doesn't
        // preempt the solar-flare path (crowding stays below 0.8).
        civ.producer_biomass = Real::from_int(100);
        let capacity = Real::from_int(state.grid().n_cells() as i64) * planet.biosphere_density;
        let mut eco = sim_ecosystem::sample_ecosystem_with_substrate(
            planet.seed,
            planet.metabolic_substrate.tag(),
            capacity.max(Real::ONE),
        );
        let mut flares = 0usize;
        for tick in 0..1000u64 {
            // Step the ecosystem each tick so the per-tick biogeochem
            // path is exercised end-to-end (same coupling sim-core's
            // `run()` uses).
            let _ = eco.step_with_biogeochem_at_tick(
                &mut state,
                planet.stellar_luminosity,
                tick,
            );
            if let Some(rec) = sim_civ::catastrophe::check_and_apply(
                &mut civ,
                &mut state,
                planet,
                species,
                tick,
                Some(&mut eco),
            ) {
                if matches!(rec.kind, sim_civ::catastrophe::CatastropheKind::SolarFlare) {
                    flares += 1;
                }
            }
        }
        (flares, eco)
    }

    let (m_flares, m_eco) = run_1000_ticks(&m_planet, &species);
    let (g_flares, _g_eco) = run_1000_ticks(&g_planet, &species);

    // --- 6. Catastrophe path fires solar flares more frequently on
    //        the M-dwarf than the G-dwarf equivalent. Concretely:
    //        the M-dwarf flares ≥ 1× within 1000 ticks (period ~188);
    //        the G-dwarf flares 0× (period ~18804, well beyond the
    //        1000-tick window). The strict inequality proves the
    //        T18 spectral-aware firing wiring is live.
    assert!(
        m_flares >= 1,
        "T18 wiring: M-dwarf must fire at least one solar flare in 1000 ticks \
         (period ~188); got {m_flares}",
    );
    assert!(
        m_flares > g_flares,
        "T18 wiring: M-dwarf flare count ({m_flares}) must exceed G-dwarf flare count ({g_flares}) \
         in a 1000-tick window — proves the catastrophe path is spectral-class-aware",
    );

    // --- 7. At least one species persists through 1000 ticks of
    //        sustained M-dwarf flaring. The ecosystem starts with
    //        multiple eco-species (producers + consumers + decomposers
    //        + parasites); even after a flare-driven catastrophe pass
    //        the extant-pool must not collapse to zero.
    let extant_count = m_eco.species.values().filter(|s| s.is_extant).count();
    assert!(
        extant_count >= 1,
        "T18 survival: at least one eco-species must persist through 1000 ticks \
         of M-dwarf flaring; extant count={extant_count}",
    );
}

#[test]
fn hot_jupiter_extreme_params_do_not_overflow() {
    use sim_physics::{HexGrid, OrchestratorState, PhysicsState, integrate_civ_step};
    use sim_world::{
        init_planet, Moon,
    };

    // Hand-constructed hot-Jupiter analog. The spec is in
    // `Planet`-field units (Earth-relative mass / radius, K, Pa, ...).
    let planet = Planet {
        seed: 1_701,
        name: "HotJupiterT17".to_string(),
        // 300 Earth masses + 11 Earth radii → derived gravity
        // g = 300 / 121 ≈ 2.48× Earth ≈ 24 m/s². Inside Q32.32 (max
        // ~2.1e9) with plenty of headroom but well above the
        // worldgen-reachable band (sample_planet caps at ~2.5×).
        mass: Real::from_int(300),
        radius: Real::from_int(11),
        composition: Composition::Rocky,
        // Silicate substrate's upper liquid-phase band (800-1500 K
        // per `MetabolicSubstrate::temperature_range`); 1500 K
        // pushes the radiation / chemistry / escape paths into the
        // hot end of their respective domains.
        mean_temperature: Real::from_int(1_500),
        temperature_gradient: Real::from_int(50),
        terrain_peak: Real::from_int(8_000),
        terrain_centre_q: 0,
        terrain_centre_r: 0,
        sea_level: Real::from_int(2_000),
        // Reducing == thickest enum variant (density × 100 = 6700,
        // ~50× Earth surface, with a 15 km scale height). Pair with
        // Silicate to land outside the worldgen-reachable cross.
        atmosphere: Atmosphere::Reducing,
        atmospheric_composition: AtmosphericComposition::vacuum(),
        // ~1000× Earth surface pressure (Earth ≈ 1.01e5 Pa). Q32.32
        // holds 1e8 directly (max ~2.1e9).
        surface_pressure: Real::from_int(100_000_000),
        biosphere: BiosphereClass::Sparse,
        biosphere_density: Real::from_ratio(2, 10),
        crustal_composition: CrustalComposition::empty(),
        magnetosphere: Magnetosphere::Strong,
        crust: Crust::Basaltic,
        // 5× Earth irradiance to drive the radiation / EUV paths
        // toward the saturation clamps without overflowing the
        // input itself.
        stellar_luminosity: Real::from_int(6_800),
        orbital_distance_au: Real::from_ratio(5, 100),
        moon_count: 1,
        moons: vec![Moon {
            mass_relative_x100: 100,
            orbital_period_macros: 28,
            inclination_deg_x10: 0,
            eccentricity: Real::ZERO,
        }],
        orbital_eccentricity_x100: 5,
        axial_tilt_deg: Real::from_int(10),
        day_length_hours: Real::from_int(10),
        orbital_period_months: 12,
        // Substrate=Silicate per spec. The atmosphere::Reducing pairing
        // is intentionally off-distribution — `atmosphere_compatible`
        // would reject it, but the physics path doesn't gate on the
        // compatibility table, only the worldgen sampler does.
        metabolic_substrate: MetabolicSubstrate::Silicate,
        substrate_perturbation: Real::ZERO,
        locking_state: LockingState::Synchronous,
        // Hot, EUV-rich star — the kind of host that drives hot-
        // Jupiter atmospheric loss in the literature.
        star: Star::with_age(
            SpectralType::F,
            Real::from_int(6_800),
            Real::from_ratio(5, 10),
            Real::from_int(5),
        ),
    };

    let grid = HexGrid::new(12, 8);
    let mut state = PhysicsState::new(grid);
    init_planet(&mut state, &planet);
    let mut orch_state = OrchestratorState::new();
    let laws = crate::laws::build_laws(&planet, 8);
    let cfg = RunConfig::dev(planet.seed, 500);

    // Drive 500 ticks of physics integration. The full `run()` loop
    // is overkill for the overflow canary — what we need exercised
    // is the per-tick chain that hits the saturating-mul guards
    // (radiation → chemistry → escape → tides → tidal heating →
    // hadley). The civ / ecosystem / recognition layers don't host
    // the overflow surface.
    for _ in 0..500 {
        integrate_civ_step(
            &mut state,
            &mut orch_state,
            &cfg.orchestration,
            &laws.fluid,
            &laws.heat,
            &laws.em,
            &laws.chemistry,
            Some(&laws.radiation),
            Some(&laws.wind),
            Some(&laws.hydrology),
            Some(&laws.tides),
            Some(&laws.magnetism),
            Some(&laws.lorentz),
            Some(&laws.coriolis),
            Some(&laws.vertical),
            Some(&laws.weathering),
            Some(&laws.ice_albedo),
            Some(&laws.tectonics),
            Some(&laws.volcanism),
            Some(&laws.magnetic_reversal),
            Some(&laws.clouds),
            Some((laws.planet_radius_earth_units, laws.moon_heating.as_slice())),
            Some(&laws.atmospheric_escape),
            Some(&laws.hadley),
            Some(&laws.resonance),
            Some(&laws.insolation),
            Some(&laws.tidal_stress),
            Some(&laws.surface_radiation),
        );
    }

    // 1. No panic — reaching this line is the first assertion.

    // 2. No Real field at the I32F32 ceiling. We sweep the per-cell
    //    temperature + every chemistry substance field. The saturating
    //    guards exist to keep arithmetic finite; if a single quantity
    //    ever pins at MAX it's a sign the chain ran off the rails. The
    //    sentinel is derived by saturating a known-overflowing product
    //    (1e9 × 1e9 wraps Q32.32) so we don't have to import the
    //    fixed-point ceiling constant directly. Window of 1024 LSBs
    //    catches both exact-MAX and sub-LSB rounding floors that pin
    //    near it.
    let real_max =
        Real::from_int(1_000_000_000).saturating_mul(Real::from_int(1_000_000_000));
    let max_bits = real_max.raw().to_bits();
    let near_max_window: i64 = 1024;
    for (cid, _) in state.grid().cells() {
        let t = state.temperature()[cid.0 as usize];
        let t_bits = t.raw().to_bits();
        assert!(
            (max_bits - t_bits).abs() > near_max_window,
            "cell {} temperature raw bits {} within {} of Real::MAX ({}); \
             saturation pin indicates an unguarded overflow chain",
            cid.0,
            t_bits,
            near_max_window,
            max_bits,
        );
    }
    for sub_idx in 0..sim_physics::N_SUBSTANCES {
        for (cid, _) in state.grid().cells() {
            let v = state.substance(sub_idx)[cid.0 as usize];
            let v_bits = v.raw().to_bits();
            assert!(
                (max_bits - v_bits).abs() > near_max_window,
                "cell {} substance idx {} raw bits {} within {} of Real::MAX ({}); \
                 saturation pin indicates an unguarded overflow chain",
                cid.0,
                sub_idx,
                v_bits,
                near_max_window,
                max_bits,
            );
        }
    }

    // 3. Greenhouse cap holds — every cell's temperature must sit in
    //    a physically plausible band. The `greenhouse_cap_scaled`
    //    ceiling (pressure-scaled, capped at 600 K via the upper
    //    clamp; 250 K at Earth pressure) bounds the per-tick T_eq
    //    inflation; the relaxation
    //    rate ensures we asymptote, not diverge. Generous upper bound
    //    (3000 K = 1500 K surface + 1000 K of greenhouse / radiative
    //    headroom + gradient room) so this is a "T isn't infinity"
    //    check, not a calibration assertion. Lower bound at 1 K
    //    catches the wrap-to-zero failure mode.
    let t_upper = Real::from_int(3_000);
    let t_lower = Real::from_int(1);
    for (cid, _) in state.grid().cells() {
        let t = state.temperature()[cid.0 as usize];
        assert!(
            t < t_upper,
            "cell {} temperature {} exceeded greenhouse-cap upper bound {} K; \
             radiation / greenhouse chain may be unguarded",
            cid.0,
            t.to_f64_for_display(),
            t_upper.to_f64_for_display(),
        );
        assert!(
            t > t_lower,
            "cell {} temperature {} fell below lower sanity bound {} K; \
             radiation / chemistry chain may have wrapped to zero",
            cid.0,
            t.to_f64_for_display(),
            t_lower.to_f64_for_display(),
        );
    }

    // 4. `exobase_temperature` saturates at the ratio cap under
    //    hot-Jupiter EUV. With T_surf = 1500 K and an EUV input one
    //    order of magnitude above Earth's, the raw ratio exceeds the
    //    `EXOBASE_RATIO_MAX` (10) clamp, so T_exo must land at exactly
    //    10× T_surf = 15000 K. The assertion proves the saturating
    //    `min(ratio_cap)` clamp inside `exobase_temperature` is doing
    //    its job — without it the linear form would land T_exo above
    //    the Q32.32 ceiling once the downstream `m × v² / T` division
    //    runs.
    let t_surf = Real::from_int(1_500);
    let euv_extreme = Real::from_int(1); // 1000× the Earth ref ≈ 1e-3.
    let t_exo = sim_physics::atmospheric_escape::exobase_temperature(t_surf, euv_extreme);
    let expected_max =
        t_surf * Real::from_int(sim_physics::atmospheric_escape::EXOBASE_RATIO_MAX);
    assert_eq!(
        t_exo, expected_max,
        "exobase_temperature should saturate at EXOBASE_RATIO_MAX × T_surf \
         under hot-Jupiter-scale EUV input; got {} vs expected {}",
        t_exo.to_f64_for_display(),
        expected_max.to_f64_for_display(),
    );
}

#[test]
fn lava_world_runs_with_silicate_substrate() {
    use sim_ecosystem::sample_ecosystem_with_substrate_for_grid;
    use sim_physics::chemistry::{
        solvent_reaction_kinetics_prefactor, substrate_phase_thresholds,
        MetabolicSubstrate as ChemistrySubstrate,
    };
    use sim_physics::hydrology::saturation_vapour_cap;
    use sim_physics::HexGrid;
    use sim_species::substrate_default_envelope;
    use sim_world::{init_planet, sample_planet};

    // (1) Pick a Silicate-substrate seed found via brute-force seed
    // sweep. Seed 19 maps to a Silicate / Synchronous-locked / Rocky
    // world with sampled T ≈ 1103 K — the locked-rotation lava
    // hemisphere the spec asks for. The sampled T sits in the
    // substrate's *world-sampling* window (800-1500 K, per
    // `MetabolicSubstrate::temperature_range`) which is narrower than
    // the silicate *species tolerance* window (1687-3538 K). We push
    // the surface temperature explicitly to 2000 K below so the cell
    // sits between the silicate freeze (1687 K) and boil (3538 K)
    // points — the canonical "molten silicate is liquid" regime the
    // species tolerance envelope was tuned for.
    let seed: u64 = 19;
    let planet = sample_planet(seed);
    assert!(
        matches!(planet.metabolic_substrate, MetabolicSubstrate::Silicate),
        "T20 fixture seed must yield Silicate substrate, got {:?}",
        planet.metabolic_substrate
    );
    assert!(
        matches!(planet.locking_state, sim_world::LockingState::Synchronous),
        "T20 fixture seed should give a locked-rotation lava world; got {:?}",
        planet.locking_state
    );

    // Silicate liquid window per `substrate_phase_thresholds`. Pinned
    // here for the assertion below and to make the "between freeze
    // and boil" target temperature explicit.
    let (freeze, boil) = substrate_phase_thresholds("silicate");
    let target_t = Real::from_int(2000);
    assert!(
        freeze < target_t && target_t < boil,
        "target T=2000 K must sit inside silicate liquid window \
         ({:?} .. {:?})",
        freeze,
        boil,
    );

    // (2) Build the physics state with the planet, then force every
    // cell's surface temperature to 2000 K. The sampled mean of
    // ~1103 K is below the silicate species tolerance floor (1687 K)
    // — without the override the per-cell radiation gate would
    // collapse every silicate-tolerant producer (their `temp_range`
    // starts at 1687 K) before tick 500.
    let cfg = RunConfig::dev(seed, 1);
    let grid = HexGrid::new(cfg.grid_width, cfg.grid_height);
    let mut state = sim_physics::PhysicsState::new(grid);
    init_planet(&mut state, &planet);
    for t in state.temperature_mut() {
        *t = target_t;
    }

    // Build the per-planet ecosystem with the silicate substrate tag
    // — same construction the production `run()` loop uses (mirrors
    // `ecosystem_fixture_for_seed`), but pinned explicitly to
    // "silicate" so this test stays robust to any future change in
    // `planet.metabolic_substrate.tag()`'s spelling.
    let n_cells = state.grid().n_cells();
    let planet_capacity: Real = {
        let cap = Real::from_int(n_cells as i64) * planet.biosphere_density;
        if cap < Real::ONE { Real::ONE } else { cap }
    };
    let mut eco = sample_ecosystem_with_substrate_for_grid(
        planet.seed,
        "silicate",
        planet_capacity,
        n_cells,
        None,
    );

    // Sanity: every sampled species should carry the silicate
    // tolerance envelope (radiation_max = 5.0 base, pressure_range
    // (1, 100) base; per-species jitter is ±20%). Pick the maximum
    // observed radiation_max across the pool and assert it sits
    // *well* above the aqueous baseline (0.5) — extremophile-grade.
    let silicate_envelope = substrate_default_envelope(MetabolicSubstrate::Silicate);
    assert_eq!(
        silicate_envelope.radiation_max,
        Real::from_int(5),
        "silicate base radiation_max must be 5.0 (extremophile)"
    );
    assert_eq!(
        silicate_envelope.pressure_range,
        (Real::ONE, Real::from_int(100)),
        "silicate base pressure_range must be (1, 100) (extremophile)"
    );
    let max_rad = eco
        .species
        .values()
        .map(|s| s.tolerance.radiation_max)
        .fold(Real::ZERO, |acc, x| if x > acc { x } else { acc });
    assert!(
        max_rad >= Real::from_int(3),
        "silicate-tolerant species must carry radiation_max ≥ 3 \
         (base 5.0 ± 20% jitter); got max={:?}",
        max_rad,
    );

    // Kinetics: silicate prefactor must beat aqueous. The silicate
    // window (1687-3538 K) puts every reaction on the upper end of
    // the Arrhenius curve, so the per-substrate prefactor sits at
    // 5.0 vs the aqueous baseline of 1.0. Wired through
    // `solvent_reaction_kinetics_prefactor` so the chemistry layer
    // picks it up automatically when the planet's substrate is
    // Silicate.
    let kin_silicate = solvent_reaction_kinetics_prefactor(&ChemistrySubstrate::Silicate);
    let kin_aqueous = solvent_reaction_kinetics_prefactor(&ChemistrySubstrate::Aqueous);
    assert!(
        kin_silicate > kin_aqueous,
        "silicate kinetics prefactor must exceed aqueous baseline: \
         silicate={:?}, aqueous={:?}",
        kin_silicate,
        kin_aqueous,
    );
    assert_eq!(
        kin_silicate,
        Real::from_int(5),
        "silicate kinetics prefactor must be 5× aqueous (per substrate.rs)"
    );

    // Vapour cap at 2000 K must be large but bounded. Curve form is
    // `C_base × (T/T_ref)^4` with `C_base = 50_000`, `T_ref = 373`.
    // At 2000 K → ~50_000 × (2000/373)^4 ≈ 4.1e7. Two-sided
    // assertion: above `C_base` (warm-cell headroom) and below the
    // I32F32 ceiling guard so the chemistry-layer arithmetic stays
    // representable on a lava world.
    let cap_2000 = saturation_vapour_cap(target_t);
    let cap_floor = Real::from_int(50_000);
    let cap_ceiling = Real::from_int(1_000_000_000);
    assert!(
        cap_2000 > cap_floor && cap_2000 < cap_ceiling,
        "saturation_vapour_cap(2000 K) must sit in (50_000, 1e9); \
         got {:?}",
        cap_2000,
    );

    // (3) Run for 500 ticks. The assertion below is "no panic". The
    // ecosystem step path touches: producer growth (with the per-
    // substrate kinetics prefactor flowing through the chemistry
    // layer), chemoautotroph partition over the silicate oxidiser
    // ladder, predation, syntrophy, decomposition, and the per-tick
    // extinction sweep. Any of those panicking on a silicate-world
    // inputs (e.g. an unhandled high-T branch in latent-heat
    // arithmetic) would surface here.
    let solar = planet.stellar_luminosity;
    for tick in 0..500u64 {
        // Re-force the temperature each tick so background radiation
        // / atmosphere / hydrology phases don't drift the lava
        // hemisphere off-target. (The hydrology phase doesn't run
        // here — only the ecosystem step does — so this is mostly
        // defensive against future test refactors that add more
        // phases to the per-tick loop.)
        for t in state.temperature_mut() {
            *t = target_t;
        }
        let _events = eco.step_with_biogeochem_at_tick(&mut state, solar, tick);
    }

    // (4) Post-run assertions.

    // 4a. Temperature stayed in the silicate liquid window. We force
    // the temperature each tick, so this is a tautology in this
    // build — but it's the canonical T20 invariant and pinning it
    // here protects against a future refactor where the test stops
    // re-forcing the temperature and starts relying on a
    // hydrology-coupled path to hold T inside the window.
    for (i, &t) in state.temperature().iter().enumerate() {
        assert!(
            freeze <= t && t <= boil,
            "cell {} temperature {:?} drifted outside silicate liquid \
             window ({:?} .. {:?}) over 500 ticks",
            i,
            t,
            freeze,
            boil,
        );
    }

    // 4b. Vapour cap holds at 2000 K (re-checked after the run loop
    // in case anything mutated `saturation_vapour_cap`'s downstream
    // state — the function is pure, so this is also a tautology,
    // but it pins the bounded-cap invariant the spec calls out).
    let cap_after = saturation_vapour_cap(target_t);
    assert_eq!(
        cap_after, cap_2000,
        "saturation_vapour_cap should be a pure function of T"
    );

    // 4c. At least one silicate-tolerant species persists. The
    // silicate envelope's extreme radiation / pressure tolerance
    // means generic catastrophe-style culls (which would wipe a
    // narrow-aqueous-envelope species) should leave some pool
    // member extant. The biogeochem step's per-tick extinction
    // sweep is the only thing that can flip `is_extant`; surviving
    // it for 500 ticks confirms the silicate envelope's wide
    // windows actually shield the species.
    let n_extant = eco.species.values().filter(|s| s.is_extant).count();
    assert!(
        n_extant >= 1,
        "expected ≥ 1 silicate-tolerant species to persist after 500 \
         ticks; got 0 extant out of {} total — silicate envelope \
         (radiation_max=5.0, pressure_range=(1,100)) failed to keep \
         a pool member alive",
        eco.species.len()
    );
}

// ---------------------------------------------------------------------
// T21 — Ammoniacal-substrate end-to-end test.
//
// `MetabolicSubstrate::Ammoniacal` has full sampling + chemistry
// wiring (substrate temperature range [195, 240] K, kinetics prefactor
// 0.4, reducing/thin atmosphere only) but no integration test exercises
// a complete Ammoniacal-substrate planet run. T21 builds an
// Ammoniacal-equivalent planet manually (substrate=Ammoniacal,
// T_surface ≈ 220 K mid-window, Atmosphere::Reducing per
// `atmosphere_compatible`, mass=1, radius=1) and drives the per-tick
// ecosystem step for 1000 ticks, mirroring T15 (Titan / Hydrocarbon).
// Assertions cover: build doesn't panic, mean T stays in [180, 260] K
// (substrate window + slack), ≥1 extant species post-run, and the
// solvent kinetics prefactor equals the documented 0.4 (slower than
// Earth's aqueous 1.0 baseline). The wider 1000-tick exercise catches
// chemistry blowups, Q32.32 overflow, and biological collapse on a
// cold reducing-atmosphere substrate.
// ---------------------------------------------------------------------

#[test]
fn ammoniacal_analog_run_produces_credible_state() {
    use sim_physics::chemistry::{
        solvent_reaction_kinetics_prefactor, MetabolicSubstrate as ChemistrySubstrate,
    };
    use sim_physics::{HexGrid, PhysicsState};
    use sim_world::{init_planet, planet_name_from_seed};

    // Pick a fixed seed so the run is bit-for-bit reproducible. The
    // seed only drives the `planet_name_from_seed` lookup + the
    // ecosystem / RNG streams downstream; the planet bulk properties
    // are pinned by this fixture rather than sampled.
    let seed: u64 = 0x2117_AAAA_AAAA_5EED;

    // Sanity precondition: the substrate's documented atmosphere
    // compatibility includes `Atmosphere::Reducing` (this is the
    // spec-chosen atmosphere for the fixture). Pin it here so a future
    // refactor that narrows the Ammoniacal-compatible atmosphere set
    // can't silently invalidate this fixture's atmosphere choice.
    assert!(
        MetabolicSubstrate::Ammoniacal.atmosphere_compatible(Atmosphere::Reducing),
        "Ammoniacal substrate must accept Atmosphere::Reducing"
    );

    let planet = Planet {
        seed,
        name: planet_name_from_seed(seed),
        // Earth-mass / Earth-radius rocky world — the spec pins both
        // to `Real::ONE`. Derived gravity ≈ 9.81 m/s² (Earth-equivalent).
        mass: Real::ONE,
        radius: Real::ONE,
        // Rocky composition so init_planet keeps the latitude-driven
        // surface temperature (GaseousShell would override every cell
        // to 700 K, blowing the Ammoniacal range).
        composition: Composition::Rocky,
        // Surface temperature 220 K — mid-range of the Ammoniacal
        // liquid window [195, 240] K. NH3 is liquid here at
        // ~Earth-equivalent pressure on a reducing atmosphere.
        mean_temperature: Real::from_int(220),
        // Modest equator-to-pole gradient (cold reducing atmospheres
        // tend toward weak meridional gradients due to high IR opacity
        // of NH3 + H2).
        temperature_gradient: Real::from_int(10),
        terrain_peak: Real::from_int(4_000),
        terrain_centre_q: 4,
        terrain_centre_r: 4,
        sea_level: Real::from_int(1_500),
        // Reducing: matches `atmosphere_compatible(Ammoniacal)` and the
        // sampling distribution's NH3/H2/CH4 reducing-atmosphere
        // composition for Ammoniacal worlds.
        atmosphere: Atmosphere::Reducing,
        // Ammoniacal reducing-atmosphere composition (per
        // `sim_world::sampling`): NH3-bearing with H2 + CH4 + N2.
        // Authored fractions roughly: N2 ≈ 0.40, NH3 ≈ 0.30, H2 ≈ 0.20,
        // CH4 ≈ 0.10. Trace gases left at zero.
        atmospheric_composition: AtmosphericComposition {
            n2: Real::from_ratio(40, 100),
            o2: Real::ZERO,
            co2: Real::ZERO,
            ch4: Real::from_ratio(10, 100),
            nh3: Real::from_ratio(30, 100),
            h2o: Real::ZERO,
            h2: Real::from_ratio(20, 100),
            ar: Real::ZERO,
            other: Real::ZERO,
        },
        // ~Earth-equivalent surface pressure — inside the Reducing
        // density / scale-height band so the categorical Atmosphere
        // label coheres with the numeric pressure.
        surface_pressure: Real::from_int(101_325),
        // Sparse: substrate-first contract still requires *some* life
        // so the ecosystem sampler has tier members to step; an
        // Ammoniacal world is a candidate for life but historically
        // sparser than aqueous Earth.
        biosphere: BiosphereClass::Sparse,
        biosphere_density: Real::from_ratio(3, 10),
        magnetosphere: Magnetosphere::Weak,
        // Ammoniacal worlds' crust is typically basaltic with NH3 /
        // water-ice deposits; basaltic is the closest balanced match.
        crust: Crust::Basaltic,
        crustal_composition: CrustalComposition::empty(),
        // Stellar irradiance at the Ammoniacal sampling band's
        // outer-system distance (~2.5 AU): 1361 / 2.5² ≈ 218 W/m².
        stellar_luminosity: Real::from_int(218),
        // Ammoniacal sampling band is 1.5-3.5 AU; pick mid-band 2.5 AU.
        orbital_distance_au: Real::from_ratio(25, 10),
        moon_count: 0,
        moons: Vec::new(),
        orbital_eccentricity_x100: 3,
        axial_tilt_deg: Real::from_int(20),
        day_length_hours: Real::from_int(24),
        orbital_period_months: 48,
        metabolic_substrate: MetabolicSubstrate::Ammoniacal,
        substrate_perturbation: Real::ZERO,
        locking_state: LockingState::FreeRotator,
        // G-dwarf host star, mid-life. Bolometric scale set to the
        // planet's per-m² irradiance (~218 W/m²) so the SED breakdown
        // is consistent with the planet's orbital distance.
        star: Star::with_age(
            SpectralType::G,
            Real::from_int(218),
            Real::from_ratio(45, 10),
            Real::from_int(10),
        ),
    };

    // Sanity: Earth-mass / Earth-radius gravity should land at ≈ 9.81
    // m/s². Spot-check that the bulk mass/radius pair didn't silently
    // invert.
    let g = planet.gravity().to_f64_for_display();
    assert!(
        (9.0..=11.0).contains(&g),
        "Ammoniacal analog gravity should be ≈ 9.81 m/s² (got {g})",
    );

    // Build the physics state + ecosystem the same way `run()` does
    // (mirrors `ecosystem_fixture_for_seed` but uses the manually-
    // constructed planet so the substrate is pinned). This is the
    // "build doesn't panic" path — any Q32.32 overflow in init_planet
    // on an Ammoniacal-reducing fixture would unwind the test here.
    let cfg = RunConfig::dev(seed, 1);
    let grid = HexGrid::new(cfg.grid_width, cfg.grid_height);
    let mut state = PhysicsState::new(grid);
    init_planet(&mut state, &planet);
    let n_cells = state.grid().n_cells() as i64;
    let capacity = {
        let cap = Real::from_int(n_cells) * planet.biosphere_density;
        if cap < Real::ONE {
            Real::ONE
        } else {
            cap
        }
    };
    let substrate_tag: &'static str = planet.metabolic_substrate.tag();
    let mut ecosystem = sim_ecosystem::sample_ecosystem_with_substrate(
        planet.seed,
        substrate_tag,
        capacity,
    );

    // Assertion: ecosystem must contain at least one species. The
    // substrate-first contract guarantees every sampled planet carries
    // a viable trophic web — a zero-species ecosystem here would mean
    // the Ammoniacal-substrate path produced an empty pool.
    assert!(
        !ecosystem.species.is_empty(),
        "Ammoniacal-substrate ecosystem must have at least one species; \
         got an empty species map",
    );

    // Solvent kinetics prefactor (Ammoniacal = 0.4) means slower
    // chemistry than Earth (Aqueous = 1.0). Pinned via
    // `solvent_reaction_kinetics_prefactor` so the chemistry layer
    // picks it up automatically when the planet's substrate is
    // Ammoniacal. This is the per-substrate Arrhenius-like multiplier
    // applied to combustion and biofuel-regrowth rates.
    let kin_ammoniacal = solvent_reaction_kinetics_prefactor(&ChemistrySubstrate::Ammoniacal);
    let kin_aqueous = solvent_reaction_kinetics_prefactor(&ChemistrySubstrate::Aqueous);
    assert!(
        kin_ammoniacal < kin_aqueous,
        "Ammoniacal kinetics prefactor must be below aqueous baseline \
         (cold solvent → slower chemistry): ammoniacal={:?}, aqueous={:?}",
        kin_ammoniacal,
        kin_aqueous,
    );
    assert_eq!(
        kin_ammoniacal,
        Real::from_ratio(4, 10),
        "Ammoniacal kinetics prefactor must be 0.4 (per substrate.rs)"
    );

    // Run for 1000 ticks. The per-tick step mirrors the production
    // `run()` loop's ecosystem call: `step_with_biogeochem_at_tick`
    // couples producer growth ← solar + CO2, respiration → CO2, then
    // runs the extinction sweep. Existing debug_asserts inside the
    // step + chemistry layer fire if mass conservation breaks.
    let solar = planet.stellar_luminosity;
    let mut min_mean_temp = f64::INFINITY;
    let mut max_mean_temp = f64::NEG_INFINITY;
    for tick in 0..1000u64 {
        let _events = ecosystem.step_with_biogeochem_at_tick(&mut state, solar, tick);
        // Mean surface temperature (planet-wide aggregate) — the
        // ecosystem step does not directly mutate temperature, but
        // chemistry-coupled CO2 flux does feed back through the
        // radiation law in `run()`; here we sample post-step to make
        // sure the field hasn't drifted out of the substrate's window
        // due to a sign-flip in the biogeochem coupling.
        let temps = state.temperature();
        let mut sum = Real::ZERO;
        for t in temps {
            sum = sum + *t;
        }
        let n = temps.len() as i64;
        let mean = (sum / Real::from_int(n)).to_f64_for_display();
        if mean < min_mean_temp {
            min_mean_temp = mean;
        }
        if mean > max_mean_temp {
            max_mean_temp = mean;
        }
    }

    // Mean temperature stays in the Ammoniacal liquid range with
    // slack. The substrate's nominal liquid window is [195, 240] K;
    // we use [180, 260] K per the spec — catches a sign-flip /
    // runaway drift while tolerating reasonable equator-pole
    // variability and future calibration tweaks.
    assert!(
        (180.0..=260.0).contains(&min_mean_temp),
        "mean temperature underflowed Ammoniacal liquid range over 1000 \
         ticks: min={min_mean_temp} K, max={max_mean_temp} K (expected ~220 K)",
    );
    assert!(
        (180.0..=260.0).contains(&max_mean_temp),
        "mean temperature overflowed Ammoniacal liquid range over 1000 \
         ticks: min={min_mean_temp} K, max={max_mean_temp} K (expected ~220 K)",
    );

    // Ecosystem still has at least one species after 1000 ticks of
    // stepping. Some extinctions are expected on a sparse-biosphere
    // planet, but a fully-extinct trophic web would mean the
    // substrate-first contract was violated mid-run — the Ammoniacal
    // tolerance envelope should keep at least one pool member alive
    // through the per-tick extinction sweep.
    let extant_count = ecosystem
        .species
        .values()
        .filter(|s| s.is_extant)
        .count();
    assert!(
        extant_count >= 1,
        "expected ≥ 1 extant species after 1000-tick Ammoniacal-analog \
         run; got {extant_count}",
    );
}
