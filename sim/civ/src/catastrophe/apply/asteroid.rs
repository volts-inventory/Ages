//! Asteroid-impact handler: rare, dramatic, hits hard.
//! Deterministic impact site per `(tick, civ_id)`; impact cell
//! takes 2× the global fraction, adjacent claimed cells 0.5×
//! (debris, fires). Asteroid radiation boost (prompt gamma +
//! activation products) blows narrow-envelope species past their
//! radiation_max on the ecosystem side. Cosmology pivots mystical
//! + reformist (rebuild pressure).

use crate::cosmology::Cosmology;
use crate::Civ;
use sim_arith::{Pop, Real};
use sim_ecosystem::PlanetEcosystem;
use sim_physics::PhysicsState;
use sim_species::Species;
use sim_world::Planet;

use super::super::cells::{apply_to_cell_and_neighbors, deterministic_cell_pick};
use super::super::damage::{
    apply_resistance_and_dormancy, asteroid_radiation_boost, catastrophe_cell_conditions,
};
use super::super::kind::CatastropheKind;
use super::super::record::CatastropheRecord;
use super::super::triggers::asteroid_fires;
use super::super::{ASTEROID_COOLDOWN_TICKS, ASTEROID_POP_LOSS};

/// Try to fire the asteroid catastrophe this tick.
pub(super) fn try_apply(
    civ: &mut Civ,
    state: &mut PhysicsState,
    planet: &Planet,
    species: &Species,
    tick: u64,
    ecosystem: &mut Option<&mut PlanetEcosystem>,
) -> Option<CatastropheRecord> {
    // Asteroid impact — rare, dramatic, hits hard. Gated only by
    // tick-based deterministic firing window + cooldown.
    let asteroid_ready = civ
        .last_asteroid_tick
        .is_none_or(|t| tick.saturating_sub(t) >= ASTEROID_COOLDOWN_TICKS);
    if !(asteroid_ready && asteroid_fires(tick)) {
        return None;
    }
    // cell-targeted: deterministic impact site per
    // (tick, civ_id). Impact cell takes 2× the global
    // fraction; adjacent claimed cells take 0.5× (debris,
    // fires). If the civ has no claim, fall back to uniform
    // pop drop so a brand-new civ still feels the global
    // aftermath. : catastrophe-resistance tools soften
    // the absolute loss (built shelter survives debris).
    let raw_frac = Real::from(ASTEROID_POP_LOSS);
    // Tolerance: read the deterministic impact cell's conditions
    // for the tolerance gate. No extra rad/temp delta — asteroid
    // damage is kinetic and dust-driven, not radiation-driven, and
    // the cell's pre-impact state is the right baseline for the
    // surviving sub-population.
    let asteroid_cell = deterministic_cell_pick(civ, tick).map_or(0, |c| c as usize);
    let cell_conds =
        catastrophe_cell_conditions(state, planet, asteroid_cell, Real::ZERO, Real::ZERO);
    let frac = apply_resistance_and_dormancy(civ, species, raw_frac, cell_conds, tick);
    let center_frac = frac * Real::from_int(2);
    let neighbor_frac = frac / Real::from_int(2);
    let grid_w = state.grid().width();
    let grid_h = state.grid().height();
    let lost = if let Some(impact) = deterministic_cell_pick(civ, tick) {
        apply_to_cell_and_neighbors(
            civ,
            grid_w,
            grid_h,
            impact,
            center_frac,
            neighbor_frac,
            true,
        )
    } else {
        let before = civ.cohort.total();
        let target = (before * (Real::ONE - frac)).max(Pop::from_int(10));
        civ.cohort.shrink_to(target)
    };
    let _ = lost;
    // T2 — drain ecosystem biomass for every extant species,
    // tolerance-gated by the impact cell's conditions plus the
    // asteroid-specific radiation boost (prompt gamma +
    // activation products). Calibrated to the raw asteroid
    // loss fraction so the eco signature matches the headline
    // catastrophe severity; each eco species' own tolerance
    // envelope gates the realised loss (extremophiles with
    // wide radiation envelopes shrug it off).
    if let Some(eco) = ecosystem.as_deref_mut() {
        let eco_cell_conds = catastrophe_cell_conditions(
            state,
            planet,
            asteroid_cell,
            Real::ZERO,
            asteroid_radiation_boost(),
        );
        let (t, ph, sal, rad, p) = eco_cell_conds;
        eco.apply_catastrophe_at_cell(raw_frac, t, ph, sal, rad, p);
    }
    civ.last_asteroid_tick = Some(tick);
    civ.last_catastrophe_tick = Some(tick);
    // Asteroid pushes mystical strongly + reformist (rebuild
    // pressure) — civilization-shaking event.
    let push = Cosmology {
        empirical: -Real::percent(5),
        communitarian: Real::percent(10),
        reformist: Real::percent(15),
        mystical: Real::percent(20),
        hierarchical: -Real::percent(5),
    };
    civ.apply_cosmology_push(&push, Real::ONE);
    Some(CatastropheRecord {
        kind: CatastropheKind::Asteroid,
        fraction_lost: frac,
    })
}

#[cfg(test)]
mod tests {
    use super::super::check_and_apply;
    use super::super::test_helpers::*;
    use super::super::super::kind::CatastropheKind;
    use crate::Civ;
    use sim_arith::{Pop, Real};

    /// T2 acceptance test #1: non-volcanic catastrophes now strip
    /// the ecosystem's trophic-web biomass too. Pre-T2, only
    /// volcanic touched the ecosystem; asteroid / solar flare /
    /// ice age / disease left the eco pool untouched. T2 wires
    /// asteroid / solar flare / ice age into
    /// `apply_catastrophe_at_cell` (disease stays biology-internal
    /// per spec). This test fires an asteroid and asserts every
    /// extant eco species' biomass drops.
    #[test]
    fn non_volcanic_catastrophes_now_affect_ecosystem() {
        // Asteroid tick must satisfy `tick.is_multiple_of(4733 *
        // MONTHS_PER_YEAR)` AND tick > 0.
        let asteroid_tick = 4733 * protocol::MONTHS_PER_YEAR;
        let initial_pop = Pop::from_int(1_000_000);
        let mut civ = Civ::new(1, 0, initial_pop);
        // P0.5 — keep crowding low enough that disease doesn't
        // preempt the asteroid path. With no claimed cells the
        // claimed_cell_fraction defaults to 1.0; producer_biomass
        // = 100 × per_unit (50_000) = 5M capacity ≫ 1M civ pop, so
        // crowding ≈ 0.2 and disease stays dormant. Asteroid's
        // deterministic_cell_pick returns None on an empty-claim
        // civ and falls back to cell 0 for the conditions read,
        // so the eco call still fires.
        civ.producer_biomass = Real::from_int(100);
        let mut state = well_fed_state();
        // Pin cell 0 at centre-of-aqueous-envelope conditions so
        // tolerance reads from a real cell state.
        state.temperature_mut()[0] = Real::from_int(300);
        state.pressure_mut()[0] = Real::from_int(101_325);

        // Build a small eco fixture with a single producer at
        // narrow aqueous tolerance — the asteroid's
        // `rad += asteroid_radiation_boost (= 5.0)` blows past the
        // aqueous `radiation_max = 0.5`, driving match_score to 0
        // and exposing the species to the full headline loss.
        let mut eco = sim_ecosystem::sample_ecosystem_with_substrate_for_grid(
            42,
            "aqueous",
            Real::from_int(10_000),
            state.grid().n_cells(),
            None,
        );
        // Baseline biomass for all extant species pre-catastrophe.
        let before: std::collections::BTreeMap<sim_species::SpeciesId, Real> = eco
            .species
            .iter()
            .filter_map(|(id, s)| if s.is_extant { Some((*id, s.biomass)) } else { None })
            .collect();
        assert!(
            !before.is_empty(),
            "eco fixture must seed at least one extant species",
        );

        let rec = check_and_apply(
            &mut civ,
            &mut state,
            &earth_like_planet(),
            &test_species(),
            asteroid_tick,
            Some(&mut eco),
        )
        .expect("asteroid must fire at tick = 4733 * MONTHS_PER_YEAR");
        assert_eq!(rec.kind, CatastropheKind::Asteroid);

        // At least one extant species must show a strict biomass
        // drop — the headline T2 observable. We expect *every*
        // species at narrow aqueous tolerance to lose biomass since
        // rad = baseline (0.1) + 5.0 = 5.1 >> radiation_max = 0.5.
        let mut any_dropped = false;
        for (id, b0) in &before {
            let b1 = eco.species.get(id).map(|s| s.biomass).unwrap_or(Real::ZERO);
            if b1 < *b0 {
                any_dropped = true;
            }
            // No species should *gain* biomass from the catastrophe.
            assert!(
                b1 <= *b0,
                "species {:?} biomass increased through asteroid: \
                 before={:?}, after={:?}",
                id,
                b0,
                b1,
            );
        }
        assert!(
            any_dropped,
            "no eco species lost biomass through asteroid — T2 wiring missing?",
        );
    }
}
