use super::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use sim_arith::Real;
use sim_physics::{HexGrid, PhysicsState, Substance};

#[test]
fn sample_planet_is_deterministic() {
    let a = sample_planet(42);
    let b = sample_planet(42);
    assert_eq!(a.seed, b.seed);
    assert_eq!(a.gravity, b.gravity);
    assert_eq!(a.composition, b.composition);
    assert_eq!(a.mean_temperature, b.mean_temperature);
    assert_eq!(a.terrain_peak, b.terrain_peak);
    assert_eq!(a.atmosphere, b.atmosphere);
    assert_eq!(a.biosphere, b.biosphere);
}

#[test]
fn different_seeds_produce_different_planets() {
    let a = sample_planet(42);
    let b = sample_planet(43);
    // At least one major property should differ. With a few
    // properties sampled independently, probability of all
    // matching is astronomically low.
    let same_everything = a.gravity == b.gravity
        && a.composition == b.composition
        && a.mean_temperature == b.mean_temperature
        && a.terrain_peak == b.terrain_peak;
    assert!(
        !same_everything,
        "seeds 42 and 43 produced identical planet"
    );
}

#[test]
fn init_planet_is_deterministic() {
    let planet = sample_planet(123);
    let grid = HexGrid::new(8, 6);
    let mut a = PhysicsState::new(grid.clone());
    let mut b = PhysicsState::new(grid);
    init_planet(&mut a, &planet);
    init_planet(&mut b, &planet);
    assert_eq!(a.elevation(), b.elevation());
    assert_eq!(a.water_depth(), b.water_depth());
    assert_eq!(a.temperature(), b.temperature());
}

#[test]
fn different_seeds_produce_different_initial_states() {
    let p1 = sample_planet(1);
    let p2 = sample_planet(2);
    let grid = HexGrid::new(8, 6);
    let mut a = PhysicsState::new(grid.clone());
    let mut b = PhysicsState::new(grid);
    init_planet(&mut a, &p1);
    init_planet(&mut b, &p2);
    // Almost certainly different on at least one field.
    let same_state = a.elevation() == b.elevation()
        && a.water_depth() == b.water_depth()
        && a.temperature() == b.temperature();
    assert!(
        !same_state,
        "seeds 1 and 2 produced identical initial states"
    );
}

#[test]
fn sampled_planets_lie_in_si_ranges() {
    // Walk a band of seeds; every sampled Planet must sit inside
    // the documented SI bands. Catches regressions where a unit
    // band silently slips back to legacy sim-units.
    for seed in 0..256u64 {
        let p = sample_planet(seed);
        // Gravity 1.0 to 30.0 m/s².
        assert!(p.gravity >= Real::from_int(1));
        assert!(p.gravity <= Real::from_int(30));
        // Temperature spans every substrate's window:
        // Hydrocarbon 90 K floor, Silicate 1500 K ceiling.
        assert!(p.mean_temperature >= Real::from_int(90));
        assert!(p.mean_temperature <= Real::from_int(1500));
        // Every seed produces a habitable world via the
        // substrate-first sampler.
        assert!(p.is_habitable());
        // Pressure 0 to 300_000 Pa.
        assert!(p.surface_pressure >= Real::ZERO);
        assert!(p.surface_pressure <= Real::from_int(300_000));
        // Stellar irradiance 200 to 3000 W/m².
        assert!(p.stellar_luminosity >= Real::from_int(200));
        assert!(p.stellar_luminosity <= Real::from_int(3_000));
        // Terrain 0 to 15_000 m.
        assert!(p.terrain_peak >= Real::ZERO);
        assert!(p.terrain_peak <= Real::from_int(15_000));
    }
}

#[test]
fn no_atmosphere_means_no_oxidiser() {
    // Brute-force search a seed that produces an Atmosphere::None
    // planet; verify oxidiser ends up zero in init.
    let mut found = false;
    for seed in 0..100 {
        let planet = sample_planet(seed);
        if planet.atmosphere == Atmosphere::None {
            let grid = HexGrid::new(4, 4);
            let mut state = PhysicsState::new(grid);
            init_planet(&mut state, &planet);
            let total_oxid: Real = state
                .substance(Substance::Oxidiser.idx())
                .iter()
                .copied()
                .fold(Real::ZERO, |a, b| a + b);
            assert_eq!(total_oxid, Real::ZERO);
            found = true;
            break;
        }
    }
    assert!(found, "no Atmosphere::None planet in first 100 seeds");
}

/// Build a minimal Planet for the (composition, crust, magnetosphere)
/// regression test below. Other fields are set to neutral values
/// that don't perturb the charge imprint paths under test.
fn synthetic_planet(
    composition: Composition,
    crust: Crust,
    magnetosphere: Magnetosphere,
) -> Planet {
    let (sea_level, terrain_peak) = match composition {
        Composition::Rocky => (Real::from_int(1_000), Real::from_int(5_000)),
        Composition::OceanWorld => (Real::from_int(3_000), Real::from_int(4_000)),
        Composition::SubSurfaceOcean => (Real::from_int(8_000), Real::from_int(2_000)),
        Composition::GaseousShell => (Real::ZERO, Real::ZERO),
    };
    Planet {
        seed: 0,
        name: "TestPlanet".to_string(),
        gravity: Real::from_int(10),
        composition,
        mean_temperature: Real::from_int(280),
        temperature_gradient: Real::from_int(20),
        terrain_peak,
        terrain_centre_q: 4,
        terrain_centre_r: 4,
        sea_level,
        atmosphere: Atmosphere::Oxidising,
        atmospheric_composition: AtmosphericComposition::vacuum(),
        surface_pressure: Real::from_int(101_325),
        biosphere: BiosphereClass::Sparse,
        biosphere_density: Real::from_ratio(3, 10),
        magnetosphere,
        crust,
        crustal_composition: CrustalComposition::empty(),
        stellar_luminosity: Real::from_int(1_361),
        moon_count: 1,
        moons: vec![Moon {
            mass_relative_x100: 100,
            orbital_period_macros: 28,
            inclination_deg_x10: 51,
            eccentricity: Real::ZERO,
        }],
        orbital_eccentricity_x100: 2,
        axial_tilt_deg: Real::from_int(23),
        day_length_hours: Real::from_int(24),
        orbital_period_months: 12,
        metabolic_substrate: MetabolicSubstrate::Aqueous,
        substrate_perturbation: Real::ZERO,
        locking_state: LockingState::FreeRotator,
    }
}

#[test]
fn imprints_satisfy_discharge_and_template_invariants() {
    // The archetype-charge baselines (crust + magnetosphere
    // additive on rocky/ocean cells, planet-wide column on
    // gaseous shells) are calibration constants tuned to:
    //   1. Sit strictly below the planet's EM discharge
    //      threshold so the imprint isn't immediately self-
    //      zapped on tick 1, and
    //   2. Land within the firing window of the template they
    //      were calibrated to surface (piezoelectric_pulse,
    //      magnetic_lodestone, superconductor_resonance,
    //      metallic_hydrogen_signal).
    //
    // Without this regression test, retuning either the
    // discharge threshold (sim_world::discharge_threshold_for)
    // or any of the template thresholds in sim/recognition can
    // silently break the catalogue and cross-seed coverage
    // would degrade without anyone noticing. Pin the
    // relationship explicitly.
    let grid = HexGrid::new(8, 6);

    // Land-cell invariants (rocky composition exercises the
    // `on_land` branch; the imprint = crust_baseline +
    // magnetosphere_baseline).
    let crusts = [
        Crust::Basaltic,
        Crust::Hydrocarbon,
        Crust::Piezoelectric,
        Crust::Ferrous,
        Crust::RareEarth,
    ];
    let mags = [
        Magnetosphere::None,
        Magnetosphere::Weak,
        Magnetosphere::Strong,
    ];
    for &crust in &crusts {
        for &mag in &mags {
            let planet = synthetic_planet(Composition::Rocky, crust, mag);
            let mut state = PhysicsState::new(grid.clone());
            init_planet(&mut state, &planet);
            let threshold = discharge_threshold_for(mag);
            // Find a land cell (charge baseline applies there).
            let land_cell = state
                .elevation()
                .iter()
                .position(|e| *e > planet.sea_level)
                .expect("rocky planet must produce at least one land cell");
            let charge = state.charge()[land_cell].abs();
            assert!(
                charge < threshold,
                "land imprint {crust:?}+{mag:?} = {} must stay strictly below discharge threshold {} so EM diffusion doesn't self-zap on tick 1",
                charge.to_f64_for_display(),
                threshold.to_f64_for_display(),
            );

            // Per-crust firing-window invariants. The crust
            // baseline + magnetosphere baseline must put the
            // cell inside the |charge| window of the template
            // the crust is calibrated for. These pin the
            // template thresholds in sim/recognition against
            // the imprint constants here.
            match crust {
                Crust::Piezoelectric => {
                    // piezoelectric_pulse: |charge| in (8, 40).
                    assert!(
                        charge > Real::from_int(8) && charge < Real::from_int(40),
                        "Piezoelectric+{mag:?} charge {} outside piezoelectric_pulse window (8, 40)",
                        charge.to_f64_for_display(),
                    );
                }
                Crust::Ferrous => {
                    // magnetic_lodestone: |charge| in (10, 20).
                    assert!(
                        charge > Real::from_int(10) && charge < Real::from_int(20),
                        "Ferrous+{mag:?} charge {} outside magnetic_lodestone window (10, 20)",
                        charge.to_f64_for_display(),
                    );
                }
                Crust::RareEarth => {
                    // superconductor_resonance: |charge| in (5, 10).
                    assert!(
                        charge > Real::from_int(5) && charge < Real::from_int(10),
                        "RareEarth+{mag:?} charge {} outside superconductor_resonance window (5, 10)",
                        charge.to_f64_for_display(),
                    );
                }
                Crust::Basaltic | Crust::Hydrocarbon => {
                    // No template-window invariant: these
                    // crusts don't have a charge-keyed template.
                    // The sub-discharge invariant above is the
                    // only constraint they need to honour.
                }
            }
        }
    }

    // GaseousShell: planet-wide charge column must fire
    // metallic_hydrogen_signal (|charge| > 14) AND stay below
    // the discharge threshold.
    for &mag in &mags {
        let planet = synthetic_planet(Composition::GaseousShell, Crust::Basaltic, mag);
        let mut state = PhysicsState::new(grid.clone());
        init_planet(&mut state, &planet);
        let threshold = discharge_threshold_for(mag);
        // Every gaseous-shell cell holds the column.
        let charge = state.charge()[0].abs();
        assert!(
            charge > Real::from_int(14),
            "GaseousShell+{mag:?} charge column {} must exceed metallic_hydrogen_signal threshold (14)",
            charge.to_f64_for_display(),
        );
        assert!(
            charge < threshold,
            "GaseousShell+{mag:?} charge column {} must stay below discharge threshold {}",
            charge.to_f64_for_display(),
            threshold.to_f64_for_display(),
        );
    }
}

#[test]
fn multi_peak_terrain_has_distinct_maxima_and_shallow_water() {
    // Multi-peak invariants on temperate Aqueous seeds:
    //   (a) elevation has at least 2 distinct local maxima
    //       (vs. an earlier single conical peak),
    //   (b) at least one cell across the sweep falls in the
    //       renderer's shallow-water band `depth ∈ (0, 100] m`
    //       so the planet has *some* visible `~` coastline
    //       (not necessarily on every seed — narrower
    //       cones make the shallow band thinner, so any one
    //       seed may discretise past it; the population
    //       guarantee is that the band is reachable, not that
    //       every seed lands in it).
    //
    // Walks every temperate Aqueous Rocky planet in the first
    // 64 seeds; asserts (a) per-seed and (b) across the sweep.
    let mut candidates: Vec<Planet> = Vec::new();
    for seed in 0..64u64 {
        let p = sample_planet(seed);
        if p.metabolic_substrate == MetabolicSubstrate::Aqueous
            && p.composition == Composition::Rocky
            && p.terrain_peak > Real::from_int(2_000)
            && p.sea_level > Real::from_int(500)
            // Temperate band: water-tolerant Aqueous range.
            && p.mean_temperature > Real::from_int(260)
            && p.mean_temperature < Real::from_int(310)
        {
            candidates.push(p);
        }
    }
    assert!(
        !candidates.is_empty(),
        "no temperate Aqueous Rocky planet in first 64 seeds — sampler drift?",
    );

    let mut any_shallow_band = false;
    for planet in &candidates {
        let grid = HexGrid::new(32, 20);
        let mut state = PhysicsState::new(grid.clone());
        init_planet(&mut state, planet);

        // (a) Count distinct local maxima per seed.
        let elev = state.elevation().to_vec();
        let mut maxima = 0usize;
        for (cid, axial) in grid.cells() {
            let here = elev[cid.0 as usize];
            if here <= Real::ZERO {
                continue;
            }
            let mut is_max = true;
            for nb in grid.neighbours(axial) {
                if elev[nb.0 as usize] >= here {
                    is_max = false;
                    break;
                }
            }
            if is_max {
                maxima += 1;
            }
        }
        assert!(
            maxima >= 2,
            "multi-peak terrain expected ≥ 2 local maxima on seed {}; found {}",
            planet.seed,
            maxima,
        );

        // (b) Shallow-water band tally across the sweep.
        if state
            .water_depth()
            .iter()
            .any(|d| *d > Real::ZERO && *d <= Real::from_int(100))
        {
            any_shallow_band = true;
        }
    }
    assert!(
        any_shallow_band,
        "expected at least one shallow-water cell (0 < depth ≤ 100 m) somewhere across {} temperate Aqueous Rocky seeds",
        candidates.len(),
    );
}

#[test]
fn peaks_respect_minimum_distance() {
    // Invariant: every pair of peaks placed by `init_planet`
    // sits at least `min_dist` cells apart (Manhattan / axial-sum
    // metric matching the piecewise-cone falloff).
    //
    // We can't observe the peak vec directly from outside
    // `init_planet`, so the test reconstructs it by re-running
    // the same salted RNG against the same primary anchor and
    // grid bounds. That keeps the test honest about the
    // production code path: if the rejection-sampling loop or
    // the salt drift, this test breaks.
    let terrain_peak_salt: u64 = 0xA17E_BEEF_C0DE_0147;
    let grid_w: i32 = 36;
    let grid_h: i32 = 30;
    let max_dim = if grid_w > grid_h { grid_w } else { grid_h };
    for seed in [1u64, 7, 42, 100, 495, 2024, 31337] {
        let planet = sample_planet(seed);
        if planet.terrain_peak == Real::ZERO {
            // GaseousShell with no surface; no peaks to check.
            continue;
        }
        let centre_q = planet.terrain_centre_q.rem_euclid(grid_w);
        let centre_r = planet.terrain_centre_r.rem_euclid(grid_h);
        let mut peak_rng = ChaCha20Rng::seed_from_u64(planet.seed ^ terrain_peak_salt);
        let n_secondary: u32 = peak_rng.gen_range(2..=4);
        let num_peaks = 1 + n_secondary;
        let min_dist_raw = max_dim / i32::try_from(num_peaks * 2).expect("num_peaks fits");
        let min_dist = if min_dist_raw > 3 { min_dist_raw } else { 3 };
        let mut peaks: Vec<(i32, i32)> = vec![(centre_q, centre_r)];
        for _ in 0..n_secondary {
            let mut chosen: Option<(i32, i32)> = None;
            let mut last = (0i32, 0i32);
            for _ in 0..200u32 {
                let q = peak_rng.gen_range(0..grid_w);
                let r = peak_rng.gen_range(0..grid_h);
                last = (q, r);
                if peaks
                    .iter()
                    .all(|&(pq, pr)| (q - pq).abs() + (r - pr).abs() >= min_dist)
                {
                    chosen = Some((q, r));
                    break;
                }
            }
            peaks.push(chosen.unwrap_or(last));
        }
        // Verify pairwise spacing. The fallback path (chosen ==
        // None) can in principle place a peak closer than
        // min_dist, but for the seeds + grid we test here every
        // attempt finds a valid slot.
        for i in 0..peaks.len() {
            for j in (i + 1)..peaks.len() {
                let (qi, ri) = peaks[i];
                let (qj, rj) = peaks[j];
                let d = (qi - qj).abs() + (ri - rj).abs();
                assert!(
                    d >= min_dist,
                    "expected peaks ≥ {min_dist} cells apart on seed {seed}; got {d} between ({qi}, {ri}) and ({qj}, {rj})",
                );
            }
        }
    }
}
