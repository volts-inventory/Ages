//! Disease-catastrophe handler: cell-targeted plague that starts
//! at the densest claimed cell and spreads to adjacent claimed
//! cells. Severity scales with biosphere richness; cooldown
//! stretches with metabolic substrate (silicate plagues run on a
//! different absolute cadence than aqueous ones). Cosmology
//! pivots toward communitarian + mystical (plague-cosmology
//! pattern). Disease stays biology-internal — no
//! `apply_catastrophe_at_cell` call on the ecosystem (per spec).

use crate::cosmology::Cosmology;
use crate::Civ;
use sim_arith::{Pop, Real};
use sim_ecosystem::PlanetEcosystem;
use sim_physics::PhysicsState;
use sim_species::Species;
use sim_world::Planet;

use super::super::cells::{apply_to_cell_and_neighbors, densest_claimed_cell};
use super::super::damage::{apply_resistance_and_dormancy, catastrophe_cell_conditions};
use super::super::factors::disease_severity_factor;
use super::super::kind::CatastropheKind;
use super::super::record::CatastropheRecord;
use super::super::triggers::disease_fires;
use super::super::{DISEASE_COOLDOWN_TICKS, DISEASE_POP_LOSS};

/// Try to fire the disease catastrophe this tick. Returns
/// `Some(record)` on fire; `None` otherwise.
pub(super) fn try_apply(
    civ: &mut Civ,
    state: &mut PhysicsState,
    planet: &Planet,
    species: &Species,
    tick: u64,
    _ecosystem: &mut Option<&mut PlanetEcosystem>,
) -> Option<CatastropheRecord> {
    // Disease — cell-targeted: starts at the densest cell
    // and spreads to adjacent claimed cells. Pre- it was a
    // uniform civ-wide pop drop, which read as artificial: a
    // plague hits cities first, not equally everywhere.
    //
    // Disease is biology-driven (crowding-disease dynamics tied to
    // generational time), so its cooldown stretches with substrate
    // metabolism — a silicate civ doesn't experience the same plague
    // cadence as an aqueous one in absolute ticks. The physics-
    // driven kinds (volcanic / asteroid / solar / ice age) keep raw
    // cooldowns: those are external to biology.
    let metabolism = planet.metabolic_substrate.metabolism();
    let disease_cooldown =
        crate::demographics::streak_ticks_for_metabolism(DISEASE_COOLDOWN_TICKS, metabolism);
    let disease_ready = civ
        .last_disease_tick
        .is_none_or(|t| tick.saturating_sub(t) >= disease_cooldown);
    if !(disease_ready && disease_fires(civ, state, planet, tick)) {
        return None;
    }
    // severity scales with biosphere richness. :
    // BasicHealing / MedicalIntervention / AdvancedMedicine /
    // GeneticManipulation reduce the realised loss via
    // apply_catastrophe_resistance — the headline catastrophe-
    // resistance effect for healthcare-bearing civs.
    let base_frac = Real::from(DISEASE_POP_LOSS);
    let severity_frac = base_frac * disease_severity_factor(planet.biosphere);
    // Tolerance: disease originates at the densest claimed cell;
    // fall back to cell 0 if the civ has no per-cell cohorts so
    // the tolerance gate still reads from real per-cell state.
    let disease_cell = densest_claimed_cell(civ).map_or(0, |c| c as usize);
    let cell_conds =
        catastrophe_cell_conditions(state, planet, disease_cell, Real::ZERO, Real::ZERO);
    let frac = apply_resistance_and_dormancy(civ, species, severity_frac, cell_conds, tick);
    let center_frac = frac * Real::from_int(2);
    let neighbor_frac = frac;
    let grid_w = state.grid().width();
    let grid_h = state.grid().height();
    let lost = if let Some(origin) = densest_claimed_cell(civ) {
        apply_to_cell_and_neighbors(
            civ,
            grid_w,
            grid_h,
            origin,
            center_frac,
            neighbor_frac,
            true,
        )
    } else {
        // Fallback for civs without per-cell cohorts (legacy /
        // tests): apply uniform fraction to aggregate.
        let before = civ.cohort.total();
        let target = (before * (Real::ONE - frac)).max(Pop::from_int(10));
        civ.cohort.shrink_to(target)
    };
    let _ = lost;
    civ.last_disease_tick = Some(tick);
    civ.last_catastrophe_tick = Some(tick);
    // pivot toward communitarian + mystical (plague-cosmology pattern).
    let push = Cosmology {
        empirical: Real::ZERO,
        communitarian: Real::percent(15),
        reformist: -Real::percent(5),
        mystical: Real::percent(15),
        hierarchical: Real::percent(5),
    };
    civ.apply_cosmology_push(&push, Real::ONE);
    Some(CatastropheRecord {
        kind: CatastropheKind::Disease,
        fraction_lost: frac,
    })
}

#[cfg(test)]
mod tests {
    use super::super::check_and_apply;
    use super::super::test_helpers::*;
    use super::super::super::kind::CatastropheKind;
    use super::super::super::DISEASE_AGE_FLOOR_TICKS;
    use crate::Civ;
    use sim_arith::{Pop, Real};
    use sim_physics::Substance;

    #[test]
    fn disease_fires_under_crowding_after_age_floor() {
        let mut civ = Civ::new(1, 0, Pop::from_int(50));
        let mut state = empty_state();
        // P0.5 — capacity now reads `civ.producer_biomass` rather
        // than `Substance::Fuel`. Calibration mirrors the legacy
        // fuel-tuned setup: producer_biomass = 0.001 × claimed_frac
        // (1.0 for empty claim) × per_unit (50_000) = 50, matching
        // civ pop so crowding = 1.0.
        state.substance_mut(Substance::Fuel.idx())[0] = Real::from_ratio(1, 1000);
        civ.producer_biomass = Real::from_ratio(1, 1000);
        let r = check_and_apply(
            &mut civ,
            &mut state,
            &earth_like_planet(),
            &test_species(),
            DISEASE_AGE_FLOOR_TICKS,
            None,
        );
        let rec = r.expect("disease should fire");
        assert_eq!(rec.kind, CatastropheKind::Disease);
        assert!(civ.cohort.total() < Pop::from_int(50));
        // Cosmology pivoted.
        assert!(civ.cosmology.mystical > Real::ZERO);
    }

    #[test]
    fn disease_blocked_before_age_floor() {
        let mut civ = Civ::new(1, 0, Pop::from_int(50));
        let mut state = empty_state();
        state.substance_mut(Substance::Fuel.idx())[0] = Real::from_ratio(1, 1000);
        civ.producer_biomass = Real::from_ratio(1, 1000);
        let r = check_and_apply(
            &mut civ,
            &mut state,
            &earth_like_planet(),
            &test_species(),
            DISEASE_AGE_FLOOR_TICKS - 1,
            None,
        );
        assert!(r.is_none());
    }

    /// Sprint 2 Item 7b spec test #1.
    ///
    /// Species with `dormancy = 0.9` takes ~10× less damage than
    /// `dormancy = 0` from the same catastrophe. We exercise the
    /// disease pathway because its fixture is the most stable:
    /// known-firing trigger, no per-civ-shelter to confound the
    /// loss math. The two civs are identical apart from the
    /// species' dormancy trait.
    #[test]
    fn dormant_species_survives_catastrophe_at_reduced_rate() {
        let baseline_pop = Pop::from_int(50);
        let dormancy_high = Real::percent(90);

        // Baseline run — dormancy = 0.
        let mut civ_low = Civ::new(1, 0, baseline_pop);
        let mut state_low = empty_state();
        state_low.substance_mut(Substance::Fuel.idx())[0] = Real::from_ratio(1, 1000);
        // P0.5 — match the disease trigger's `civ.producer_biomass`
        // crowding calibration so the test still drives crowding to 1.0.
        civ_low.producer_biomass = Real::from_ratio(1, 1000);
        let rec_low = check_and_apply(
            &mut civ_low,
            &mut state_low,
            &earth_like_planet(),
            &species_with_dormancy(Real::ZERO),
            DISEASE_AGE_FLOOR_TICKS,
            None,
        )
        .expect("baseline disease should fire");

        // Dormant run — dormancy = 0.9, otherwise identical.
        let mut civ_high = Civ::new(1, 0, baseline_pop);
        let mut state_high = empty_state();
        state_high.substance_mut(Substance::Fuel.idx())[0] = Real::from_ratio(1, 1000);
        civ_high.producer_biomass = Real::from_ratio(1, 1000);
        let rec_high = check_and_apply(
            &mut civ_high,
            &mut state_high,
            &earth_like_planet(),
            &species_with_dormancy(dormancy_high),
            DISEASE_AGE_FLOOR_TICKS,
            None,
        )
        .expect("dormant disease should fire");

        // Both should be the same `kind` (disease) — the dormancy
        // multiplier only shrinks fraction_lost, not the trigger.
        assert_eq!(rec_low.kind, CatastropheKind::Disease);
        assert_eq!(rec_high.kind, CatastropheKind::Disease);

        // Effective fraction should be ~10× smaller. Allow a small
        // tolerance because both also pass through
        // `apply_catastrophe_resistance` (which is 1.0 at zero
        // tools) — the ratio is exactly
        // `(1 - 0.9 × 1.0) / (1 - 0 × 1.0) = 0.10`.
        let ratio = rec_high.fraction_lost / rec_low.fraction_lost;
        // ~0.10 ± 1% — Q32.32 is exact for these magnitudes; the
        // tolerance only protects against incidental future
        // resistance bumps that future code paths might apply
        // uniformly to both.
        assert!(
            ratio <= Real::percent(11) && ratio >= Real::percent(9),
            "expected ~0.10× damage with dormancy=0.9, got ratio={ratio:?}",
        );
    }

    /// P1.3 acceptance test #1: a high-dormancy species' dormant
    /// pool gets seeded with the would-be casualties from a
    /// catastrophe. `dormancy = 0.9`, disease pathway, match_score
    /// = 0 (envelope nowhere near the cell — full base damage) →
    /// dormant pool must be populated with the surviving-but-
    /// dormant fraction.
    #[test]
    fn catastrophe_seeds_dormant_pool_for_tardigrade_species() {
        let baseline_pop = Pop::from_int(1_000_000);
        let mut civ = Civ::new(1, 0, baseline_pop);
        // P0.5 — set producer biomass so the disease trigger
        // doesn't preempt the path (crowding ≥ 0.8 of capacity).
        civ.producer_biomass = Real::from_int(100);
        let mut state = well_fed_state();
        // Pin cell 0 at centre-of-aqueous-envelope T/p so the
        // tolerance-gate fall-through doesn't accidentally zero
        // out the dormant seeding via match_score = 1.
        state.temperature_mut()[0] = Real::from_int(300);
        state.pressure_mut()[0] = Real::from_int(101_325);
        // Force-fire disease at the age floor.
        state.substance_mut(Substance::Fuel.idx())[0] = Real::from_ratio(1, 1000);
        civ.producer_biomass = Real::from_ratio(1, 1000);
        let species = species_with_dormancy(Real::percent(90));
        let rec = check_and_apply(
            &mut civ,
            &mut state,
            &earth_like_planet(),
            &species,
            DISEASE_AGE_FLOOR_TICKS,
            None,
        )
        .expect("disease must fire for tardigrade species");
        assert_eq!(rec.kind, CatastropheKind::Disease);
        // Headline P1.3 assertion: the dormant pool now holds
        // a non-zero surviving-but-dormant reservoir.
        assert!(
            civ.dormant_pool.population > Real::ZERO,
            "dormant pool must be seeded for dormancy=0.9 species, got {:?}",
            civ.dormant_pool.population,
        );
        // `entered_tick` should match the catastrophe tick so the
        // post-run telemetry can locate the cryptobiosis event.
        assert_eq!(civ.dormant_pool.entered_tick, DISEASE_AGE_FLOOR_TICKS);
    }
}
