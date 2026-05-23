//! Solar-flare handler: gated on planet's stellar luminosity +
//! magnetosphere weakness. Hits modestly; pushes empirical +
//! reformist (the species observes the flare directly — drives
//! observational science). Flare radiation boost is modulated by
//! the magnetic-reversal cosmic-ray ground-flux multiplier (T8 —
//! strong dipole attenuates, weak/reversing field amplifies).

use crate::cosmology::Cosmology;
use crate::Civ;
use sim_arith::{Pop, Real};
use sim_ecosystem::PlanetEcosystem;
use sim_physics::PhysicsState;
use sim_species::Species;
use sim_world::Planet;

use super::super::cells::densest_claimed_cell;
use super::super::damage::{
    apply_resistance_and_dormancy, catastrophe_cell_conditions, solar_flare_radiation_boost,
};
use super::super::kind::CatastropheKind;
use super::super::record::CatastropheRecord;
use super::super::triggers::solar_flare_fires;
use super::super::{SOLAR_FLARE_COOLDOWN_TICKS, SOLAR_FLARE_POP_LOSS};

/// Try to fire the solar-flare catastrophe this tick.
pub(super) fn try_apply(
    civ: &mut Civ,
    state: &mut PhysicsState,
    planet: &Planet,
    species: &Species,
    tick: u64,
    ecosystem: &mut Option<&mut PlanetEcosystem>,
) -> Option<CatastropheRecord> {
    // Solar flare — gated on planet's stellar luminosity +
    // magnetosphere weakness. Hits modestly; pushes empirical
    // (the species observes the flare directly).
    let flare_ready = civ
        .last_solar_flare_tick
        .is_none_or(|t| tick.saturating_sub(t) >= SOLAR_FLARE_COOLDOWN_TICKS);
    if !(flare_ready && solar_flare_fires(planet, tick)) {
        return None;
    }
    // catastrophe resistance softens the flare's hit
    // (advanced shielding / underground habitats / radiation
    // medicine).
    // Tolerance: solar flare boosts the cell's radiation flux by
    // the flare magnitude, modulated by the magnetic-reversal
    // cosmic-ray ground-flux multiplier (Item 20) — a flare
    // hitting during a reversal window pushes radiation-sensitive
    // species well past their `radiation_max`, while a flare
    // hitting under a strong stable dipole gets *attenuated* (the
    // shielded surface sees less of the radiation pulse). T8: the
    // raw flux is `1 / (dipole_strength + 0.1)`, so it spans
    // ~[0.0, 10.0] across the dipole envelope. Clamp to
    // `[0.2, 5.0]` — strong magnetospheres dampen by up to 5×
    // (down to 0.2 of nominal), weak/reversing fields amplify by
    // up to 5×. The 0.2 floor preserves a minimum flare effect
    // even on heavily shielded worlds (the flare's particle
    // spectrum isn't entirely magnetic-deflectable).
    let raw_frac = Real::from(SOLAR_FLARE_POP_LOSS);
    let cosmic_amp = state
        .cosmic_ray_ground_flux()
        .max(Real::from_ratio(2, 10))
        .min(Real::from_int(5));
    let rad_boost = solar_flare_radiation_boost() * cosmic_amp;
    let flare_cell = densest_claimed_cell(civ).map_or(0, |c| c as usize);
    let cell_conds = catastrophe_cell_conditions(state, planet, flare_cell, Real::ZERO, rad_boost);
    let frac = apply_resistance_and_dormancy(civ, species, raw_frac, cell_conds, tick);
    let before = civ.cohort.total();
    let target = (before * (Real::ONE - frac)).max(Pop::from_int(10));
    let _lost = civ.cohort.shrink_to(target);
    // T2 — drain ecosystem biomass for every extant species,
    // tolerance-gated by the cell's post-flare radiation flux
    // (already cosmic-ray-amplified above). Calibrated to the
    // raw flare loss fraction so the eco signature matches the
    // headline catastrophe severity; each eco species' own
    // tolerance envelope gates the realised loss (extremophiles
    // with `radiation_max ≥ 5` shrug it off, narrow-envelope
    // species take the full hit).
    if let Some(eco) = ecosystem.as_deref_mut() {
        let (t, ph, sal, rad, p) = cell_conds;
        eco.apply_catastrophe_at_cell(raw_frac, t, ph, sal, rad, p);
    }
    civ.last_solar_flare_tick = Some(tick);
    civ.last_catastrophe_tick = Some(tick);
    // Empirical + reformist (the species sees the sky's
    // role in their fate — drives observational science).
    let push = Cosmology {
        empirical: Real::percent(15),
        communitarian: Real::ZERO,
        reformist: Real::percent(10),
        mystical: Real::percent(5),
        hierarchical: Real::ZERO,
    };
    civ.apply_cosmology_push(&push, Real::ONE);
    Some(CatastropheRecord {
        kind: CatastropheKind::SolarFlare,
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

    /// P0.4 acceptance test: same solar flare, two species
    /// differing only in their `ToleranceEnvelope`. Extremophile
    /// (`radiation_max = 20`) survives at >> 3× the rate of an
    /// aqueous-default (`radiation_max = 0.5`) species — measured
    /// as the death-rate ratio (the only metric capable of
    /// resolving the spec target from a 10% flat base loss).
    #[test]
    fn extremophile_species_survives_solar_flare_better_than_aqueous() {
        // Big civ so the 10-pop floor doesn't dominate.
        let initial_pop = Pop::from_int(1_000_000);
        let planet = flare_planet();
        let flare_tick = 1567 * protocol::MONTHS_PER_YEAR;

        // Aqueous species — default narrow envelope, radiation_max
        // = 0.5. Flare rad = 0.1 + 1.0 = 1.1 ⇒ rad_score = 0 ⇒
        // match_score = 0 ⇒ full 10% loss.
        let mut aqueous = test_species();
        aqueous.tolerance = sim_species::ToleranceEnvelope::aqueous_default();
        let mut civ_aq = Civ::new(1, 0, initial_pop);
        // P0.5 — set producer biomass high enough that the disease
        // trigger (crowding ≥ 0.8 of capacity) doesn't preempt the
        // solar-flare path. Cap = producer_biomass × claimed_frac
        // (1.0 for empty claim) × per_unit (50_000) so
        // `producer_biomass = 100` yields cap = 5M, well above the
        // 1M civ pop ⇒ crowding 0.2 ⇒ no disease.
        civ_aq.producer_biomass = Real::from_int(100);
        let mut state_aq = well_fed_state();
        // Pin cell 0 to centre-of-aqueous-envelope T/p so the
        // non-radiation axes don't accidentally bottleneck the
        // aqueous species's match_score below the radiation gate.
        state_aq.temperature_mut()[0] = Real::from_int(300);
        state_aq.pressure_mut()[0] = Real::from_int(101_325);
        let rec_aq =
            check_and_apply(&mut civ_aq, &mut state_aq, &planet, &aqueous, flare_tick, None)
                .expect("flare must fire on weak-magnetosphere planet at tick=18804");
        assert_eq!(rec_aq.kind, CatastropheKind::SolarFlare);

        // Extremophile — wide envelope, radiation_max = 20. Flare
        // rad = 1.1 ⇒ rad_score = 1 - 1.1/20 ≈ 0.945 ⇒ match_score
        // ≈ 0.945 ⇒ loss = 0.10 × 0.055 ≈ 0.0055.
        let mut extremophile = test_species();
        extremophile.tolerance = extremophile_tolerance();
        let mut civ_ex = Civ::new(2, 0, initial_pop);
        // P0.5 — same producer-biomass override as the aqueous civ
        // so the disease trigger doesn't preempt the flare path.
        civ_ex.producer_biomass = Real::from_int(100);
        let mut state_ex = well_fed_state();
        // Same cell-conditions setup as the aqueous run — only the
        // species' tolerance envelope differs between the two civs.
        state_ex.temperature_mut()[0] = Real::from_int(300);
        state_ex.pressure_mut()[0] = Real::from_int(101_325);
        let rec_ex = check_and_apply(
            &mut civ_ex,
            &mut state_ex,
            &planet,
            &extremophile,
            flare_tick,
            None,
        )
        .expect("flare must fire for extremophile under same conditions");
        assert_eq!(rec_ex.kind, CatastropheKind::SolarFlare);

        // The extremophile's loss fraction must be at least 3× smaller
        // than the aqueous species' — measured as the loss ratio, the
        // only metric that resolves the spec target from a 10% flat
        // base loss. In practice it's ~18× smaller (0.10 vs ~0.0055).
        assert!(
            rec_aq.fraction_lost >= rec_ex.fraction_lost * Real::from_int(3),
            "expected aqueous loss >= 3× extremophile loss; aqueous={:?}, extremophile={:?}",
            rec_aq.fraction_lost,
            rec_ex.fraction_lost,
        );
        // And the extremophile's surviving population is strictly
        // larger than the aqueous one — the headline observable.
        assert!(
            civ_ex.cohort.total() > civ_aq.cohort.total(),
            "extremophile survivors must exceed aqueous survivors; ex={:?}, aq={:?}",
            civ_ex.cohort.total(),
            civ_aq.cohort.total(),
        );
    }

    /// T2 acceptance test #2: ecosystem analog of the civ-side P0.4
    /// flare test. Two eco species, identical except for their
    /// `ToleranceEnvelope`. The extremophile (wide radiation
    /// envelope) survives the same flare with strictly more
    /// biomass than the aqueous-default narrow-envelope species.
    /// Mirrors `extremophile_species_survives_solar_flare_better_than_aqueous`
    /// on the eco side now that solar flare is wired into
    /// `apply_catastrophe_at_cell`.
    #[test]
    fn extremophile_eco_species_survives_solar_flare_better() {
        use sim_ecosystem::EcoSpecies;
        use sim_species::{
            EcosystemRole, Habitat, InteractionMatrix, ProducerMetabolism, SpeciesId,
            ToleranceEnvelope,
        };

        let planet = flare_planet();
        let flare_tick = 1567 * protocol::MONTHS_PER_YEAR;

        let initial_pop = Pop::from_int(1_000_000);
        let mut civ = Civ::new(1, 0, initial_pop);
        // P0.5 — bump producer biomass so the disease trigger
        // doesn't preempt the flare path.
        civ.producer_biomass = Real::from_int(100);
        let mut state = well_fed_state();
        // Pin cell 0 at centre-of-aqueous-envelope T/p so non-rad
        // axes are non-binding (only the radiation axis differs
        // between the two eco species).
        state.temperature_mut()[0] = Real::from_int(300);
        state.pressure_mut()[0] = Real::from_int(101_325);

        let starting_biomass = Real::from_int(1_000);
        let aqueous_id = SpeciesId(101);
        let extremo_id = SpeciesId(102);
        let aqueous = EcoSpecies {
            species_id: aqueous_id,
            role: EcosystemRole::Producer {
                metabolism: ProducerMetabolism::Photoautotroph,
            },
            biomass: starting_biomass,
            is_extant: true,
            low_biomass_streak: 0,
            habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
            tolerance: ToleranceEnvelope::aqueous_default(),
        };
        let extremophile_tol = ToleranceEnvelope {
            temp_range: (Real::from_int(200), Real::from_int(400)),
            ph_range: (Real::from_int(5), Real::from_int(9)),
            salinity_range: (Real::from_int(10), Real::from_int(30)),
            radiation_max: Real::from_int(20),
            pressure_range: (Real::from_ratio(5, 10), Real::from_ratio(15, 10)),
        };
        let extremophile = EcoSpecies {
            species_id: extremo_id,
            role: EcosystemRole::Producer {
                metabolism: ProducerMetabolism::Photoautotroph,
            },
            biomass: starting_biomass,
            is_extant: true,
            low_biomass_streak: 0,
            habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
            tolerance: extremophile_tol,
        };
        let mut eco = sim_ecosystem::PlanetEcosystem::new(
            vec![aqueous, extremophile],
            InteractionMatrix::new(),
            Real::from_int(10_000),
        );

        let rec = check_and_apply(
            &mut civ,
            &mut state,
            &planet,
            &test_species(),
            flare_tick,
            Some(&mut eco),
        )
        .expect("flare must fire on weak-magnetosphere planet at tick = 1567 * MONTHS_PER_YEAR");
        assert_eq!(rec.kind, CatastropheKind::SolarFlare);

        let aq_after = eco.species.get(&aqueous_id).unwrap().biomass;
        let ex_after = eco.species.get(&extremo_id).unwrap().biomass;
        let aq_loss = starting_biomass - aq_after;
        let ex_loss = starting_biomass - ex_after;
        // Aqueous species must take real damage (rad = 0.1 + 1.0
        // = 1.1 > radiation_max = 0.5 ⇒ match_score = 0).
        assert!(
            aq_loss > Real::ZERO,
            "aqueous eco species took no damage: after={aq_after:?}",
        );
        // The extremophile's biomass loss must be at least 3×
        // smaller than the aqueous species' — mirrors the civ-side
        // P0.4 acceptance bound. In practice it's ~18× smaller.
        assert!(
            aq_loss >= ex_loss * Real::from_int(3),
            "expected aqueous loss >= 3× extremophile loss; \
             aqueous_loss={aq_loss:?}, extremo_loss={ex_loss:?}",
        );
        // Headline observable: more extremophile biomass survives.
        assert!(
            ex_after > aq_after,
            "extremophile survivors must exceed aqueous survivors; \
             ex_after={ex_after:?}, aq_after={aq_after:?}",
        );
    }

    /// T8 — strong magnetosphere dampens solar-flare damage. Two runs
    /// of the same flare-firing planet (Weak magnetosphere class +
    /// above-Earth luminosity, so `solar_flare_fires` triggers) at
    /// the same tick, differing only in the physics-state
    /// `dipole_strength`:
    ///
    /// * `dipole_strength = 10.0` → `cosmic_ray_ground_flux ≈ 0.099`
    ///   → clamps to `0.2` → flare radiation is *attenuated*.
    /// * `dipole_strength = 0.1` → `cosmic_ray_ground_flux ≈ 5.0`
    ///   → clamps to `5.0` → flare radiation is *amplified* 5×.
    ///
    /// Same aqueous species (narrow radiation envelope) so the
    /// tolerance gate registers the radiation differential as a
    /// `fraction_lost` differential. Strong-dipole loss must be
    /// strictly less than weak/reversing-dipole loss.
    #[test]
    fn strong_magnetosphere_suppresses_flare_damage() {
        // Big civ so the 10-pop floor on `target` doesn't dominate.
        let initial_pop = Pop::from_int(1_000_000);
        let planet = flare_planet();
        let flare_tick = 1567 * protocol::MONTHS_PER_YEAR;

        // Aqueous species — narrow radiation envelope so the cosmic-
        // amp differential bites on `match_score`.
        let mut species = test_species();
        species.tolerance = sim_species::ToleranceEnvelope::aqueous_default();

        // ------------- Strong-dipole run (T8 suppression) -------------
        let mut civ_strong = Civ::new(1, 0, initial_pop);
        // Producer biomass override so the disease trigger doesn't
        // preempt the flare path (matches the extremophile test).
        civ_strong.producer_biomass = Real::from_int(100);
        let mut state_strong = well_fed_state();
        // Centre cell 0 on aqueous-envelope T/p so non-radiation
        // axes don't accidentally bottleneck match_score.
        state_strong.temperature_mut()[0] = Real::from_int(300);
        state_strong.pressure_mut()[0] = Real::from_int(101_325);
        // Strong dipole → flux ≈ 0.099 → clamps to 0.2 (5× suppression).
        *state_strong.dipole_strength_mut() = Real::from_int(10);
        let rec_strong = check_and_apply(
            &mut civ_strong,
            &mut state_strong,
            &planet,
            &species,
            flare_tick,
            None,
        )
        .expect("flare must fire on weak-magnetosphere-class planet at tick=18804");
        assert_eq!(rec_strong.kind, CatastropheKind::SolarFlare);

        // ------------- Reversal-window run (max amplification) --------
        let mut civ_reversal = Civ::new(2, 0, initial_pop);
        civ_reversal.producer_biomass = Real::from_int(100);
        let mut state_reversal = well_fed_state();
        state_reversal.temperature_mut()[0] = Real::from_int(300);
        state_reversal.pressure_mut()[0] = Real::from_int(101_325);
        // Deep-reversal dipole → flux ≈ 5.0 → clamps to 5.0 (5× amp).
        *state_reversal.dipole_strength_mut() = Real::from_ratio(1, 10);
        let rec_reversal = check_and_apply(
            &mut civ_reversal,
            &mut state_reversal,
            &planet,
            &species,
            flare_tick,
            None,
        )
        .expect("flare must fire under reversal-window dipole too");
        assert_eq!(rec_reversal.kind, CatastropheKind::SolarFlare);

        // The strong-magnetosphere run must take strictly less damage
        // than the reversal-window run — the headline T8 observable.
        assert!(
            rec_strong.fraction_lost < rec_reversal.fraction_lost,
            "T8: strong-dipole flare damage ({:?}) must be < reversal-window damage ({:?})",
            rec_strong.fraction_lost,
            rec_reversal.fraction_lost,
        );
        // And the surviving population must be strictly larger under
        // the strong dipole.
        assert!(
            civ_strong.cohort.total() > civ_reversal.cohort.total(),
            "T8: strong-dipole survivors must exceed reversal survivors; \
             strong={:?}, reversal={:?}",
            civ_strong.cohort.total(),
            civ_reversal.cohort.total(),
        );
    }
}
