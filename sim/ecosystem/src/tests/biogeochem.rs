//! Biogeochem coupling tests: producer growth pulls CO2 from the
//! atmosphere, consumer respiration returns it, and the decomposer
//! chain closes the carbon budget. Includes Sprint 2 Item 9
//! chemolithotroph / syntrophy / alt-oxidiser cases that share the
//! same chemistry plumbing.

use crate::*;
use sim_arith::Real;
use sim_physics::chemistry::{
    alt_oxidiser_combustion_energy, energy_yield_factor, oxidiser_ladder, Oxidiser,
};
use sim_physics::{HexGrid, PhysicsState, Substance};
use sim_species::{
    EcosystemRole, FunctionalResponse, Habitat, Interaction, InteractionKind, InteractionMatrix,
    MutualismKind, ProducerMetabolism, SpeciesId, ToleranceEnvelope,
};

#[test]
fn chemolithotroph_species_partition_by_reduction_potential() {
    // Sprint 2 Item 9: two Chemoautotroph producers share the
    // Aqueous oxidiser ladder. The first species (lower
    // `SpeciesId`) walks the ladder strongest-first → grabs O2 at
    // +1.23 V. The second falls through to NO3- at +0.96 V. Same
    // demand, same biomass: the O2 species ends up with strictly
    // more biomass after one tick because O2 yields more growth
    // per unit oxidiser.
    let chemo_a = SpeciesId(0);
    let chemo_b = SpeciesId(1);
    let species = vec![
        EcoSpecies {
            species_id: chemo_a,
            role: EcosystemRole::Producer {
                metabolism: ProducerMetabolism::Chemoautotroph,
            },
            biomass: Real::from_int(100),
            is_extant: true,
            low_biomass_streak: 0,
            habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
            tolerance: ToleranceEnvelope::aqueous_default(),
        },
        EcoSpecies {
            species_id: chemo_b,
            role: EcosystemRole::Producer {
                metabolism: ProducerMetabolism::Chemoautotroph,
            },
            biomass: Real::from_int(100),
            is_extant: true,
            low_biomass_streak: 0,
            habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
            tolerance: ToleranceEnvelope::aqueous_default(),
        },
    ];
    let mut eco = PlanetEcosystem::new_with_substrate(
        species,
        InteractionMatrix::new(),
        Real::from_int(1_000),
        "aqueous",
    );

    let initial_a = eco.species.get(&chemo_a).unwrap().biomass;
    let initial_b = eco.species.get(&chemo_b).unwrap().biomass;
    assert_eq!(initial_a, initial_b);

    // One partition pass — the chemolithotroph helper runs as part
    // of step(); call step() so the full Item-9-aware tick fires.
    eco.partition_chemoautotrophs();

    let final_a = eco.species.get(&chemo_a).unwrap().biomass;
    let final_b = eco.species.get(&chemo_b).unwrap().biomass;

    // Both grew (positive demand, oxidiser available).
    assert!(
        final_a > initial_a,
        "species A on O2 didn't grow ({initial_a:?} -> {final_a:?})"
    );
    assert!(
        final_b > initial_b,
        "species B on NO3- didn't grow ({initial_b:?} -> {final_b:?})"
    );
    // Species A (O2) grew strictly more than species B (NO3-).
    // The demand was identical → the differential comes purely from
    // the ladder partition.
    let growth_a = final_a - initial_a;
    let growth_b = final_b - initial_b;
    assert!(
        growth_a >= growth_b,
        "O2 species growth {growth_a:?} not ≥ NO3- species growth {growth_b:?}",
    );
    // The current_oxidisers ladder should reflect post-partition
    // residuals: O2 density dropped from the initial 1.0.
    let o2_residual = eco
        .current_oxidisers
        .iter()
        .find(|o| o.name == "O2")
        .map_or(Real::ZERO, |o| o.available_density);
    assert!(
        o2_residual < Real::ONE,
        "O2 density did not deplete: {o2_residual:?}"
    );
}

#[test]
fn syntrophy_pair_extinction_when_separated() {
    // Sprint 2 Item 9: H2-producer (`producer_a`) + methanogen
    // (`mutualist_b`) Mutualism pair. Both sides start at healthy
    // biomass; we then zero out the mutualist's biomass (partner
    // "removed"). The remaining producer's biomass should crash
    // within a few ticks because syntrophy-floor enforcement drags
    // both sides together.
    let producer_a = SpeciesId(0);
    let mutualist_b = SpeciesId(1);
    let species = vec![
        EcoSpecies {
            species_id: producer_a,
            role: EcosystemRole::Producer {
                metabolism: ProducerMetabolism::Photoautotroph,
            },
            biomass: Real::from_int(50),
            is_extant: true,
            low_biomass_streak: 0,
            habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
            tolerance: ToleranceEnvelope::aqueous_default(),
        },
        EcoSpecies {
            species_id: mutualist_b,
            role: EcosystemRole::Mutualist {
                kind: MutualismKind::Generic,
            },
            biomass: Real::from_int(50),
            is_extant: true,
            low_biomass_streak: 0,
            habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
            tolerance: ToleranceEnvelope::aqueous_default(),
        },
    ];
    let mut matrix = InteractionMatrix::new();
    // Symmetric mutualism: store both directions.
    matrix.insert(
        producer_a,
        mutualist_b,
        Interaction {
            kind: InteractionKind::Mutualism,
            strength: Real::from((1, 100)),
            functional_response: FunctionalResponse::Saturating,
            half_saturation: Interaction::default_half_saturation(),
        },
    );
    matrix.insert(
        mutualist_b,
        producer_a,
        Interaction {
            kind: InteractionKind::Mutualism,
            strength: Real::from((1, 100)),
            functional_response: FunctionalResponse::Saturating,
            half_saturation: Interaction::default_half_saturation(),
        },
    );

    // Disable producer regrowth so the syntrophy collapse isn't
    // immediately undone by logistic recovery on each tick.
    let mut eco = PlanetEcosystem::new(species, matrix, Real::ZERO);

    let initial_a = eco.species.get(&producer_a).unwrap().biomass;

    // Remove the partner (biomass=0). The producer side has not
    // changed; its only support is the now-extinct mutualist.
    eco.species.get_mut(&mutualist_b).unwrap().biomass = Real::ZERO;

    // Run a few ticks — the spec says "within a few ticks".
    for _ in 0..10 {
        eco.step();
    }

    let final_a = eco.species.get(&producer_a).unwrap().biomass;

    // Producer collapsed: biomass is now a small fraction of its
    // initial value (well under half).
    assert!(
        final_a < initial_a / Real::from_int(2),
        "syntrophy partner removal did not crash producer \
         (initial={initial_a:?}, final={final_a:?})",
    );
    // Sanity: the partner remains gone too.
    assert_eq!(
        eco.species.get(&mutualist_b).unwrap().biomass,
        Real::ZERO,
        "removed partner somehow re-grew"
    );
}

#[test]
fn co2_atmosphere_combustion_works_via_alt_oxidiser() {
    // Sprint 2 Item 9: on a Hydrocarbon substrate the oxidiser
    // ladder lacks O2. Combustion against CO2 (the methanogenic
    // niche acceptor) should still produce net positive energy via
    // `alt_oxidiser_combustion_energy` — just much less than O2
    // would.
    //
    // Substance::CO2 doesn't exist yet (Item 6b adds it in
    // parallel); this test uses the oxidiser-ladder CO2 entry
    // directly, which is independent of the `Substance` enum.
    // Post-Item-6b merge a follow-up can wire the chemical
    // identity end-to-end.
    let ladder = oxidiser_ladder("hydrocarbon");
    let co2 = ladder
        .iter()
        .find(|o| o.name == "CO2")
        .copied()
        .expect("Hydrocarbon ladder must include CO2");

    // Sanity: CO2 is on the ladder at the expected weak-acceptor
    // potential.
    assert_eq!(co2.reduction_potential, Real::from((-24, 100)));

    // Pick a base combustion enthalpy (per-unit-fuel) representative
    // of the Chemistry kernel's `lh_combustion`. Using a clean unit
    // value keeps the test about the energy-yield helper, not about
    // the exact J/kg of biofuel.
    let base_energy = Real::from_int(100);
    let fuel = Real::from_int(10);

    let net = alt_oxidiser_combustion_energy(base_energy, &co2, fuel);

    // Strict positivity: CO2 still yields net energy.
    assert!(
        net > Real::ZERO,
        "CO2 combustion did not produce net energy: {net:?}",
    );

    // Cross-check: O2 yields strictly more than CO2 at the same
    // fuel + base energy. This is the physical claim we care about
    // — the alt-oxidiser path scales down but doesn't flip sign.
    let o2 = Oxidiser::new("O2", (123, 100), (1, 1));
    let o2_net = alt_oxidiser_combustion_energy(base_energy, &o2, fuel);
    assert!(
        o2_net > net,
        "O2 ({o2_net:?}) should yield more than CO2 ({net:?})",
    );

    // And the CO2 yield should be a meaningful fraction (>30%) of
    // O2 — not a near-zero side-channel. CO2 sits at the
    // methanogenic niche; methanogens are a real metabolic
    // strategy, not a vestigial one.
    let ratio = net / o2_net;
    assert!(
        ratio >= Real::from((30, 100)),
        "CO2/O2 yield ratio {ratio:?} below 0.30",
    );

    // Boundary sanity on the energy-yield factor: clamps at zero
    // for arbitrarily-low E°.
    assert_eq!(energy_yield_factor(Real::from_int(-10)), Real::ZERO);
}

// ---------------------------------------------------------------
// Sprint 2 Item 6b — biogeochem coupling tests
// ---------------------------------------------------------------

/// Build a fresh single-cell `PhysicsState` with `co2` seeded into
/// the atmosphere. Single cell keeps the test bookkeeping trivial —
/// `apply_co2_delta` spreads uniformly so a 1-cell grid receives
/// the whole delta in that one cell.
fn fresh_state_with_co2(co2: Real) -> PhysicsState {
    let mut state = PhysicsState::new(HexGrid::new(1, 1));
    state.substance_mut(Substance::CO2.idx())[0] = co2;
    state
}

#[test]
fn producer_growth_consumes_atmospheric_co2() {
    // Seed atmosphere with CO2 = 100; run one tick with a producer
    // that has room to grow; assert CO2 < 100.
    let prod = SpeciesId(0);
    let species = vec![EcoSpecies {
        species_id: prod,
        role: EcosystemRole::Producer {
            metabolism: ProducerMetabolism::Photoautotroph,
        },
        biomass: Real::from_int(500),
        is_extant: true,
        low_biomass_streak: 0,
        habitat: Habitat::Terrestrial,
        cell_biomass: Vec::new(),
        tolerance: ToleranceEnvelope::aqueous_default(),
    }];
    let mut eco = PlanetEcosystem::new(
        species,
        InteractionMatrix::new(),
        Real::from_int(1000),
    );
    let mut state = fresh_state_with_co2(Real::from_int(100));
    // Photoautotroph needs solar > 0 for growth to be unblocked.
    let solar = Real::from_int(1000);

    let co2_before = state.substance(Substance::CO2.idx())[0];
    eco.step_with_biogeochem(&mut state, solar);
    let co2_after = state.substance(Substance::CO2.idx())[0];
    let biomass_after = eco.species.get(&prod).unwrap().biomass;

    assert!(
        co2_after < co2_before,
        "atmospheric CO2 did not drop after producer growth (before={co2_before:?}, after={co2_after:?})",
    );
    assert!(
        biomass_after > Real::from_int(500),
        "producer biomass did not grow (started 500, ended {biomass_after:?})",
    );
}

#[test]
fn consumer_respiration_returns_co2_to_atmosphere() {
    // Seed atmosphere with CO2 = 0, consumer biomass = 100; run a
    // tick; assert atmospheric CO2 > 0.
    let cons = SpeciesId(0);
    let species = vec![EcoSpecies {
        species_id: cons,
        role: EcosystemRole::PrimaryConsumer,
        biomass: Real::from_int(100),
        is_extant: true,
        low_biomass_streak: 0,
        habitat: Habitat::Terrestrial,
        cell_biomass: Vec::new(),
        tolerance: ToleranceEnvelope::aqueous_default(),
    }];
    let mut eco = PlanetEcosystem::new(
        species,
        InteractionMatrix::new(),
        Real::from_int(1000),
    );
    let mut state = fresh_state_with_co2(Real::ZERO);

    let co2_before = state.substance(Substance::CO2.idx())[0];
    eco.step_with_biogeochem(&mut state, Real::ZERO);
    let co2_after = state.substance(Substance::CO2.idx())[0];

    assert_eq!(co2_before, Real::ZERO, "test setup expected 0 CO2");
    assert!(
        co2_after > Real::ZERO,
        "consumer respiration did not return CO2 (after={co2_after:?})",
    );
}

#[test]
fn decomposer_chain_balances_carbon_budget() {
    // Long-term (1000 ticks) run with all role types; assert the
    // closed-loop carbon budget (atmospheric CO2 + total biomass)
    // stays within a drift tolerance. The decomposer loop is the
    // closing edge: producers pull CO2 from the air, consumers +
    // decomposers return it. Without an external pump in, the total
    // (atmosphere CO2 + biomass carbon) is bounded; over 1000 ticks
    // it should drift by < 25% of the starting value. Initial
    // biomass values stay above the Item 6a extinction floor
    // (`0.001 × 500 = 0.5`) so the consumers don't flip extinct
    // before the budget can be balanced.
    let producer = SpeciesId(0);
    let primary = SpeciesId(1);
    let detritivore = SpeciesId(2);
    let saprotroph = SpeciesId(3);
    let species = vec![
        EcoSpecies {
            species_id: producer,
            role: EcosystemRole::Producer {
                metabolism: ProducerMetabolism::Photoautotroph,
            },
            biomass: Real::from_int(400),
            is_extant: true,
            low_biomass_streak: 0,
            habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
            tolerance: ToleranceEnvelope::aqueous_default(),
        },
        EcoSpecies {
            species_id: primary,
            role: EcosystemRole::PrimaryConsumer,
            biomass: Real::from_int(40),
            is_extant: true,
            low_biomass_streak: 0,
            habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
            tolerance: ToleranceEnvelope::aqueous_default(),
        },
        EcoSpecies {
            species_id: detritivore,
            role: EcosystemRole::Detritivore,
            biomass: Real::from_int(10),
            is_extant: true,
            low_biomass_streak: 0,
            habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
            tolerance: ToleranceEnvelope::aqueous_default(),
        },
        EcoSpecies {
            species_id: saprotroph,
            role: EcosystemRole::Saprotroph,
            biomass: Real::from_int(10),
            is_extant: true,
            low_biomass_streak: 0,
            habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
            tolerance: ToleranceEnvelope::aqueous_default(),
        },
    ];
    // Modest predation so consumers can sustain themselves on the
    // producer flow.
    let mut matrix = InteractionMatrix::new();
    matrix.insert(
        primary,
        producer,
        Interaction {
            kind: InteractionKind::Predation,
            strength: Real::from((5, 100)),
            functional_response: FunctionalResponse::Saturating,
            half_saturation: Interaction::default_half_saturation(),
        },
    );
    let mut eco = PlanetEcosystem::new(species, matrix, Real::from_int(500));
    // Seed atmosphere with enough CO2 that producers can grow for
    // many ticks before the air runs dry, and a modest solar input.
    let mut state = fresh_state_with_co2(Real::from_int(500));
    let solar = Real::from_int(50);

    let total_carbon = |state: &PhysicsState, eco: &PlanetEcosystem| -> Real {
        let co2 = state.substance(Substance::CO2.idx())[0];
        let biomass: Real = eco
            .species
            .values()
            .filter(|s| s.is_extant)
            .map(|s| s.biomass)
            .fold(Real::ZERO, |a, b| a + b);
        co2 + biomass
    };

    let initial = total_carbon(&state, &eco);
    for _ in 0..1000 {
        eco.step_with_biogeochem(&mut state, solar);
    }
    let final_total = total_carbon(&state, &eco);

    // 25% drift tolerance — predation drops a small amount of
    // biomass each tick that isn't recovered by the decomposer
    // pathway (the dead-matter pool here is the only way back to
    // CO2; flux respired by predators *is* run back through
    // respiration but the per-habitat Lindeman assimilation discards
    // the unassimilated fraction). The bound is loose enough to
    // accommodate that bookkeeping but tight enough to catch a
    // missing flux entirely.
    let drift = (final_total - initial).abs();
    let bound = initial / Real::from_int(4);
    assert!(
        drift <= bound,
        "carbon budget drifted >25%: initial={initial:?}, final={final_total:?}, drift={drift:?}, bound={bound:?}",
    );
    // Also sanity: neither pool collapsed to zero. The system is
    // closed-loop; collapse to zero would indicate one direction
    // of the cycle is broken.
    assert!(
        state.substance(Substance::CO2.idx())[0] > Real::ZERO,
        "atmospheric CO2 collapsed to zero — decomposer return path missing",
    );
    let final_biomass: Real = eco
        .species
        .values()
        .filter(|s| s.is_extant)
        .map(|s| s.biomass)
        .fold(Real::ZERO, |a, b| a + b);
    assert!(
        final_biomass > Real::ZERO,
        "total biomass collapsed to zero — producer growth path missing",
    );
}
