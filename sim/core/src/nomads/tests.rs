//! Cross-phase tests for the nomads module. Each test exercises one
//! of the per-phase files via the facade-level re-exports; helper
//! functions that are `pub(super)` inside a child module are reached
//! through their child path (e.g. `super::growth::cell_cap_bonus`).

use super::*;
use sim_arith::Real;
use sim_physics::HexGrid;
use sim_species::Habitat;
use sim_world::{
    Atmosphere, BiosphereClass, Composition, Crust, Magnetosphere, MetabolicSubstrate,
    SpectralType, Star,
};
use std::collections::{BTreeMap, BTreeSet};

fn ocean_planet(_width: u32, _height: u32) -> sim_world::Planet {
    sim_world::Planet {
        seed: 1,
        name: "TestOcean".to_string(),
        // Earth-like mass/radius → derived gravity ≈ 9.81 m/s²
        // (Sprint 5 Item 21).
        mass: Real::ONE,
        radius: Real::ONE,
        composition: Composition::OceanWorld,
        mean_temperature: Real::from_int(290),
        temperature_gradient: Real::from_int(15),
        terrain_peak: Real::from_int(2_000),
        sea_level: Real::from_int(1_500),
        atmosphere: Atmosphere::Oxidising,
        atmospheric_composition: sim_world::AtmosphericComposition::vacuum(),
        // 1 atm in Pascals (the unit `surface_pressure` is in — real
        // planets sample 60k-180k Pa). The earlier `101_325/1000` was
        // ~101 Pa (near vacuum); harmless until #175 made terrain
        // boil-aware, which then evaporated this "ocean" and left the
        // aquatic species with no habitat.
        surface_pressure: Real::from_int(101_325),
        biosphere: BiosphereClass::Lush,
        biosphere_density: Real::from_ratio(7, 10),
        crustal_composition: sim_world::CrustalComposition::empty(),
        magnetosphere: Magnetosphere::Strong,
        crust: Crust::Basaltic,
        stellar_luminosity: Real::ONE,
        orbital_distance_au: Real::ONE,
        moon_count: 1,
        moons: vec![],
        orbital_eccentricity_x100: 5,
        axial_tilt_deg: Real::from_int(23),
        day_length_hours: Real::from_int(24),
        orbital_period_months: 12,
        metabolic_substrate: MetabolicSubstrate::Aqueous,
        substrate_perturbation: Real::ZERO,
        terrain_centre_q: 0,
        terrain_centre_r: 0,
        locking_state: sim_world::LockingState::FreeRotator,
        // Modern-Sun analog: G dwarf at ~45% through its 10 Gyr
        // MS lifetime. After P2.4's faint-young-sun correction,
        // `Star::new` lands at the *faint* ZAMS (0.70× = 953
        // W/m²); use `with_age` to keep this fixture at the
        // present-day Sun-on-Earth ~1361 W/m².
        star: Star::with_age(
            SpectralType::G,
            Real::from_int(1_361),
            Real::from_ratio(45, 10),
            Real::from_int(10),
        ),
    }
}

fn populated_state(
    planet: &sim_world::Planet,
    width: u32,
    height: u32,
) -> sim_physics::PhysicsState {
    let mut state = sim_physics::PhysicsState::new(HexGrid::new(width, height));
    sim_world::init_planet(&mut state, planet);
    state
}

/// Founder-effect loss: `absorb_into_civ` with the 15% loss
/// fraction drops 15% across every bracket before depositing
/// into the civ. Zero-loss path preserves full pop (existing
/// civ gaining territory).
#[test]
fn absorb_into_civ_applies_founder_loss_only_at_founding() {
    // Minimal biology for the deposit_distributed call (only
    // bracket fractions matter for routing the deposit).
    let biology = sim_species::PopulationBiology {
        clutch_size: Real::from_int(2),
        infant_fraction: Real::percent(10),
        maturity_fraction: Real::percent(20),
        eldership_fraction: Real::percent(10),
        infant_survival: Real::percent(70),
        juvenile_survival: Real::percent(85),
        food_multipliers: [
            Real::from_ratio(3, 10),
            Real::from_ratio(6, 10),
            Real::ONE,
            Real::from_ratio(9, 10),
        ],
        events_per_fertile_window: Real::ZERO,
        reproductive_success: Real::ZERO,
    };
    let mut civ = sim_civ::Civ::new(1, 0, sim_arith::Pop::ZERO);
    // Seed an empty cohort for cell 0 so the deposit lands in
    // `region_cohorts` rather than the fallback insert branch.
    civ.region_cohorts
        .insert(0, sim_civ::Cohort::empty_with_civ(1));

    // Path 1: founder absorb (15% loss). 1000 in → 850 out.
    let mut pops: BTreeMap<u32, Real> = BTreeMap::new();
    pops.insert(0, Real::from_int(1000));
    let founder_loss = Real::from(FOUNDING_ABSORB_LOSS);
    let absorbed = absorb_into_civ(&mut pops, &mut civ, [0u32], &biology, founder_loss);
    let expected = Real::from_int(850);
    let drift = if absorbed > expected {
        absorbed - expected
    } else {
        expected - absorbed
    };
    assert!(
        drift < Real::from_int(1),
        "founder absorb should retain 85%; got {absorbed:?} vs {expected:?}"
    );

    // Path 2: territory-expansion absorb (zero loss). 1000 in → 1000 out.
    let mut civ2 = sim_civ::Civ::new(2, 0, sim_arith::Pop::ZERO);
    civ2.region_cohorts
        .insert(0, sim_civ::Cohort::empty_with_civ(2));
    let mut pops2: BTreeMap<u32, Real> = BTreeMap::new();
    pops2.insert(0, Real::from_int(1000));
    let absorbed2 = absorb_into_civ(&mut pops2, &mut civ2, [0u32], &biology, Real::ZERO);
    let expected2 = Real::from_int(1000);
    let drift2 = if absorbed2 > expected2 {
        absorbed2 - expected2
    } else {
        expected2 - absorbed2
    };
    assert!(
        drift2 < Real::from_int(1),
        "expansion absorb should keep full pop; got {absorbed2:?}"
    );
}

/// `init_pops` concentrates the starting population in
/// `NOMAD_ORIGIN_CELL_COUNT` cells, not spread across every
/// habitable cell.
#[test]
fn init_pops_seeds_only_origin_cells() {
    let planet = ocean_planet(8, 6);
    let state = populated_state(&planet, 8, 6);
    let claimed = BTreeSet::new();
    let pops = init_pops(&state, &planet, Habitat::Aquatic, &claimed);
    // Earlier: `pops.len()` would equal the number of
    // habitable cells (~tens). Now: at most
    // NOMAD_ORIGIN_CELL_COUNT.
    assert!(
        pops.len() <= NOMAD_ORIGIN_CELL_COUNT,
        "init_pops should seed at most {} origins; got {}",
        NOMAD_ORIGIN_CELL_COUNT,
        pops.len()
    );
    // Total population sums to (approximately) INITIAL_NOMAD_TOTAL.
    let total: Real = pops.values().copied().fold(Real::ZERO, |a, b| a + b);
    let expected = Real::from_int(INITIAL_NOMAD_TOTAL);
    let diff = if total > expected {
        total - expected
    } else {
        expected - total
    };
    assert!(
        diff < Real::from_int(1),
        "total pop {total:?} ≠ {expected:?}"
    );
}

/// Default cognition × sociality used by `step_growth` tests.
/// Keeps tests independent of species sampling — these are
/// nomad-mechanic tests, not species-derivation tests.
fn test_traits() -> (Real, Real) {
    (Real::percent(50), Real::percent(60))
}

/// Lifespan used by `step_growth` tests. Pinned at the
/// diffusion baseline so the rescale factor is exactly 1.0
/// — tests targeting other knobs stay independent of the
/// per-species diffusion rescale.
fn test_lifespan() -> Real {
    Real::from_int(NOMAD_DIFFUSION_BASELINE_LIFESPAN_YEARS)
}

/// `step_growth` diffuses population to neighbouring
/// cells. After one tick from a single-origin seed, neighbour
/// cells carry non-zero nomadic pop.
#[test]
fn step_growth_diffuses_to_neighbours() {
    let planet = ocean_planet(8, 6);
    let state = populated_state(&planet, 8, 6);
    let claimed = BTreeSet::new();
    let mut pops = init_pops(&state, &planet, Habitat::Aquatic, &claimed);
    let initial_count = pops.len();
    assert!(initial_count <= NOMAD_ORIGIN_CELL_COUNT);
    let observations = BTreeMap::new();
    let (cog, soc) = test_traits();
    step_growth(
        &mut pops,
        &state,
        &planet,
        Habitat::Aquatic,
        &observations,
        cog,
        soc,
        test_lifespan(),
        Real::ONE,
        0,
        &claimed,
    );
    assert!(
        pops.len() > initial_count,
        "step_growth should spread nomads to neighbours; \
         before: {initial_count}, after: {}",
        pops.len()
    );
}

/// Regression guard: empty cells DO NOT spontaneously
/// generate population. Earlier, `step_growth` ran on every
/// habitable cell and grew toward cap regardless of presence —
/// so a cell with zero pop became cap×growth in one tick.
#[test]
fn step_growth_does_not_spontaneously_populate_empty_cells() {
    let planet = ocean_planet(8, 6);
    let state = populated_state(&planet, 8, 6);
    let claimed = BTreeSet::new();
    let mut pops: BTreeMap<u32, Real> = BTreeMap::new();
    let observations = BTreeMap::new();
    let (cog, soc) = test_traits();
    step_growth(
        &mut pops,
        &state,
        &planet,
        Habitat::Aquatic,
        &observations,
        cog,
        soc,
        test_lifespan(),
        Real::ONE,
        0,
        &claimed,
    );
    assert!(
        pops.is_empty(),
        "empty pops should stay empty; got {} cells",
        pops.len()
    );
}

/// Tech-tier 0 species (no observations) cannot push pop into
/// wrong-biome cells even when isolated. This is the strict
/// habitat-confinement gate — pre-tech species stay on the
/// connected habitat component containing each origin cell.
#[test]
fn step_growth_strict_block_at_tier_zero() {
    // Aquatic species on an ocean world: water-only diffusion.
    // Verify that with zero observations, pop does NOT enter
    // any land cells.
    let planet = ocean_planet(8, 6);
    let state = populated_state(&planet, 8, 6);
    let claimed = BTreeSet::new();
    let mut pops = init_pops(&state, &planet, Habitat::Aquatic, &claimed);
    let observations = BTreeMap::new();
    let (cog, soc) = test_traits();
    // Run many ticks so any leak would accumulate.
    for _ in 0..50 {
        step_growth(
            &mut pops,
            &state,
            &planet,
            Habitat::Aquatic,
            &observations,
            cog,
            soc,
            test_lifespan(),
            Real::ONE,
            0,
            &claimed,
        );
    }
    for (cell, pop) in &pops {
        if !is_habitat_match(&state, &planet, *cell, Habitat::Aquatic)
            && *pop > Real::from_ratio(1, 10)
        {
            panic!("tier-0 species leaked pop {pop:?} into wrong-biome cell {cell}");
        }
    }
}

/// Airborne species have innate +1 base tier from flight, so
/// even at zero tech they can transit through wrong-biome
/// (water) cells. Verifies the flight bonus path.
#[test]
fn step_growth_airborne_crosses_water_at_zero_tech() {
    let planet = ocean_planet(8, 6);
    let state = populated_state(&planet, 8, 6);
    let claimed = BTreeSet::new();
    // Airborne lives on land, like terrestrial.
    let mut pops = init_pops(&state, &planet, Habitat::Airborne, &claimed);
    if pops.is_empty() {
        // Tiny ocean grid may have no land origin; skip.
        return;
    }
    let observations = BTreeMap::new();
    let (cog, soc) = test_traits();
    // Run ticks to let flight-transit fire.
    let mut saw_water_pop = false;
    for _ in 0..200 {
        step_growth(
            &mut pops,
            &state,
            &planet,
            Habitat::Airborne,
            &observations,
            cog,
            soc,
            test_lifespan(),
            Real::ONE,
            0,
            &claimed,
        );
        for (cell, pop) in &pops {
            if !is_habitat_match(&state, &planet, *cell, Habitat::Airborne)
                && *pop > Real::from_ratio(1, 10)
            {
                saw_water_pop = true;
                break;
            }
        }
        if saw_water_pop {
            break;
        }
    }
    assert!(
        saw_water_pop,
        "airborne species should transit into wrong-biome \
         cells via innate flight even at zero tech"
    );
}

/// Terrestrial species with tech-tier ≥ 1 (≥10 tech score)
/// unlocks wrong-biome transit. Mirror of the airborne test
/// but via learned tech rather than innate ability.
#[test]
fn step_growth_terrestrial_unlocks_transit_with_tech() {
    let planet = ocean_planet(8, 6);
    let state = populated_state(&planet, 8, 6);
    let claimed = BTreeSet::new();
    let mut pops = init_pops(&state, &planet, Habitat::Terrestrial, &claimed);
    if pops.is_empty() {
        return;
    }
    // Inject enough observations into one populated cell to
    // push tech_score above TRANSIT_TIER_1_TECH (= 10).
    // tech_score = cog × soc × Σ counts; (0.5)(0.6) = 0.30, so
    // need Σ counts ≥ 34 for score ≥ 10.
    let (cog, soc) = test_traits();
    let mut observations: BTreeMap<u32, BTreeMap<u32, u64>> = BTreeMap::new();
    let seed_cell = *pops.keys().next().unwrap();
    observations.entry(seed_cell).or_default().insert(0, 50);
    let mut saw_water_pop = false;
    for _ in 0..200 {
        step_growth(
            &mut pops,
            &state,
            &planet,
            Habitat::Terrestrial,
            &observations,
            cog,
            soc,
            test_lifespan(),
            Real::ONE,
            0,
            &claimed,
        );
        for (cell, pop) in &pops {
            if !is_habitat_match(&state, &planet, *cell, Habitat::Terrestrial)
                && *pop > Real::from_ratio(1, 10)
            {
                saw_water_pop = true;
                break;
            }
        }
        if saw_water_pop {
            break;
        }
    }
    assert!(
        saw_water_pop,
        "terrestrial with tech ≥ tier 1 should transit \
         through wrong-biome cells"
    );
}

/// `cell_cap_bonus` returns 0 for cells with no observations or
/// observations below all unlock thresholds — this is the
/// regression guard that ensures pre-tech cells use the
/// baseline cap unchanged.
#[test]
fn cell_cap_bonus_zero_below_thresholds() {
    assert_eq!(super::growth::cell_cap_bonus(None), Real::ZERO);
    let mut obs = BTreeMap::new();
    // All template counts well below their thresholds.
    obs.insert(GROWTH_FIRE_TEMPLATE_ID, GROWTH_FIRE_THRESHOLD - 1);
    obs.insert(GROWTH_FERTILE_TEMPLATE_ID, GROWTH_FERTILE_THRESHOLD - 1);
    obs.insert(GROWTH_SOLVENT_TEMPLATE_ID, GROWTH_SOLVENT_THRESHOLD - 1);
    assert_eq!(super::growth::cell_cap_bonus(Some(&obs)), Real::ZERO);
}

/// Cap bonuses stack additively across templates. Fire (10%)
/// plus `fertile_land` (25%) plus `solvent_humid_band` (10%)
/// rounds to ~45% (with small Q32.32 rounding error since
/// 0.1, 0.25, 0.1 aren't binary-exact fractions).
#[test]
fn cell_cap_bonus_stacks_across_templates() {
    let mut obs = BTreeMap::new();
    obs.insert(GROWTH_FIRE_TEMPLATE_ID, GROWTH_FIRE_THRESHOLD);
    obs.insert(GROWTH_FERTILE_TEMPLATE_ID, GROWTH_FERTILE_THRESHOLD);
    obs.insert(GROWTH_SOLVENT_TEMPLATE_ID, GROWTH_SOLVENT_THRESHOLD);
    let bonus = super::growth::cell_cap_bonus(Some(&obs));
    let expected = Real::percent(45);
    let diff = if bonus > expected {
        bonus - expected
    } else {
        expected - bonus
    };
    assert!(
        diff < Real::from_ratio(1, 1000),
        "expected ~0.45 cap bonus from fire+fertile+solvent, got {bonus:?}"
    );
}

/// Growth-rate bonuses stack: `thermal_gradient` (+10%) +
/// `seasonal_thaw` (+10%) ≈ +20%.
#[test]
fn cell_growth_bonus_stacks_across_templates() {
    let mut obs = BTreeMap::new();
    obs.insert(GROWTH_THERMAL_TEMPLATE_ID, GROWTH_THERMAL_THRESHOLD);
    obs.insert(GROWTH_SEASONAL_TEMPLATE_ID, GROWTH_SEASONAL_THRESHOLD);
    let bonus = super::growth::cell_growth_bonus(Some(&obs));
    let expected = Real::percent(20);
    let diff = if bonus > expected {
        bonus - expected
    } else {
        expected - bonus
    };
    assert!(
        diff < Real::from_ratio(1, 1000),
        "expected ~0.20 growth bonus from thermal+seasonal, got {bonus:?}"
    );
}

/// A cell with `thermal_gradient` + `seasonal_thaw` observations
/// fills toward cap faster than a baseline cell. Verifies the
/// growth bonus actually accelerates logistic fill in
/// `step_growth`, not just the helper function.
#[test]
fn step_growth_growth_bonus_accelerates_filling() {
    let planet = ocean_planet(8, 6);
    let state = populated_state(&planet, 8, 6);
    let claimed = BTreeSet::new();
    let (cog, soc) = test_traits();
    // Two parallel scenarios with the same starting population:
    // one with growth-bonus observations, one without.
    let mut pops_baseline = init_pops(&state, &planet, Habitat::Aquatic, &claimed);
    if pops_baseline.is_empty() {
        return;
    }
    let mut pops_boosted = pops_baseline.clone();
    let obs_baseline: BTreeMap<u32, BTreeMap<u32, u64>> = BTreeMap::new();
    let mut obs_boosted: BTreeMap<u32, BTreeMap<u32, u64>> = BTreeMap::new();
    for cell in pops_boosted.keys() {
        let mut cell_obs = BTreeMap::new();
        cell_obs.insert(GROWTH_THERMAL_TEMPLATE_ID, GROWTH_THERMAL_THRESHOLD);
        cell_obs.insert(GROWTH_SEASONAL_TEMPLATE_ID, GROWTH_SEASONAL_THRESHOLD);
        obs_boosted.insert(*cell, cell_obs);
    }
    // Run both scenarios for the same number of ticks. Both runs
    // grow their cells logistically toward the (biosphere-coupled)
    // forager cap; the growth-bonus run closes the gap faster, so it
    // accumulates more total population over the window.
    for _ in 0..50 {
        step_growth(
            &mut pops_baseline,
            &state,
            &planet,
            Habitat::Aquatic,
            &obs_baseline,
            cog,
            soc,
            test_lifespan(),
            Real::ONE,
            0,
            &claimed,
        );
        step_growth(
            &mut pops_boosted,
            &state,
            &planet,
            Habitat::Aquatic,
            &obs_boosted,
            cog,
            soc,
            test_lifespan(),
            Real::ONE,
            0,
            &claimed,
        );
    }
    // Compare a non-origin cell that only grew via diffusion +
    // logistic fill in both runs. Pick any cell present in both.
    let baseline_total: Real = pops_baseline
        .values()
        .copied()
        .fold(Real::ZERO, |a, b| a + b);
    let boosted_total: Real = pops_boosted
        .values()
        .copied()
        .fold(Real::ZERO, |a, b| a + b);
    assert!(
        boosted_total > baseline_total,
        "growth-bonus run should accumulate more total pop than \
         baseline; baseline={baseline_total:?} boosted={boosted_total:?}"
    );
}

/// Lifespan diffusion rescale: 1.0 at the baseline lifespan,
/// >1 for shorter-lived species (capped at 4×), <1 for longer-
/// lived (floored at 0.25×). Pinned so the seed-495 Ylithar
/// case (177-yr lifespan) lands clearly below 1× rather than
/// at the historic flat-rate behaviour.
#[test]
fn lifespan_diffusion_scale_brackets() {
    let baseline = super::growth::lifespan_diffusion_scale(Real::from_int(
        NOMAD_DIFFUSION_BASELINE_LIFESPAN_YEARS,
    ));
    assert_eq!(baseline, Real::ONE, "baseline lifespan should map to 1.0×");
    let r_strategist = super::growth::lifespan_diffusion_scale(Real::from_int(4));
    assert_eq!(
        r_strategist,
        Real::from_int(4),
        "4-yr species should hit the 4× upper cap"
    );
    let k_strategist = super::growth::lifespan_diffusion_scale(Real::from_int(400));
    assert_eq!(
        k_strategist,
        Real::percent(25),
        "400-yr species should hit the 0.25× lower cap"
    );
    let ylithar = super::growth::lifespan_diffusion_scale(Real::from_int(177));
    assert!(
        ylithar < Real::ONE && ylithar > Real::percent(25),
        "177-yr species should land between the lower cap and 1.0×; got {ylithar:?}"
    );
}

/// `step_growth` with a long-lived species spreads slower than
/// with a baseline-lifespan species under identical inputs.
/// Anchors the seed-495 fix: Ylithar-like species (177y) no
/// longer fill a continent in 55 years.
#[test]
fn step_growth_long_lived_species_diffuses_slower() {
    let planet = ocean_planet(8, 6);
    let state = populated_state(&planet, 8, 6);
    let claimed = BTreeSet::new();
    let (cog, soc) = test_traits();
    let observations = BTreeMap::new();
    let mut pops_baseline = init_pops(&state, &planet, Habitat::Aquatic, &claimed);
    if pops_baseline.is_empty() {
        return;
    }
    let mut pops_long = pops_baseline.clone();
    let baseline_lifespan = test_lifespan();
    let long_lifespan = Real::from_int(200);
    for _ in 0..30 {
        step_growth(
            &mut pops_baseline,
            &state,
            &planet,
            Habitat::Aquatic,
            &observations,
            cog,
            soc,
            baseline_lifespan,
            Real::ONE,
            0,
            &claimed,
        );
        step_growth(
            &mut pops_long,
            &state,
            &planet,
            Habitat::Aquatic,
            &observations,
            cog,
            soc,
            long_lifespan,
            Real::ONE,
            0,
            &claimed,
        );
    }
    assert!(
        pops_long.len() <= pops_baseline.len(),
        "long-lived species should occupy ≤ as many cells as baseline; \
         baseline={} long={}",
        pops_baseline.len(),
        pops_long.len()
    );
}
