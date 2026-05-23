//! P3.1 — Differentiated MutualismKind / ParasiteKind step.
//!
//! Five required test cases:
//!   12. pollinator_boosts_co_located_plant_clutch_size
//!   13. seed_disperser_extends_producer_range
//!   14. engineer_boosts_cohabitor_match_score
//!   15. virus_parasite_fires_episodically_not_every_tick
//!   16. macro_parasite_reduces_host_fertility_multiplier

use crate::*;
use sim_arith::Real;
use sim_species::{
    EcosystemRole, FunctionalResponse, Habitat, Interaction, InteractionKind, InteractionMatrix,
    MutualismKind, ParasiteKind, ProducerMetabolism, SpeciesId, ToleranceEnvelope,
};

/// Build a minimal (producer, mutualist) pair with a given mutualism
/// kind. Returns `(eco, producer_id, mutualist_id)`. Mutualism wired
/// symmetrically (a→b and b→a) per the matrix convention.
fn build_pair_for_mutualism(
    kind: MutualismKind,
    producer_biomass: Real,
    mutualist_biomass: Real,
    capacity: Real,
) -> (PlanetEcosystem, SpeciesId, SpeciesId) {
    let producer = SpeciesId(0);
    let mutualist = SpeciesId(1);
    let species = vec![
        EcoSpecies {
            species_id: producer,
            role: EcosystemRole::Producer {
                metabolism: ProducerMetabolism::Photoautotroph,
            },
            biomass: producer_biomass,
            is_extant: true,
            low_biomass_streak: 0,
            habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
            tolerance: ToleranceEnvelope::aqueous_default(),
        },
        EcoSpecies {
            species_id: mutualist,
            role: EcosystemRole::Mutualist { kind },
            biomass: mutualist_biomass,
            is_extant: true,
            low_biomass_streak: 0,
            habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
            tolerance: ToleranceEnvelope::aqueous_default(),
        },
    ];
    let mut matrix = InteractionMatrix::new();
    let mutualism_interaction = Interaction {
        kind: InteractionKind::Mutualism,
        strength: Real::from((10, 100)),
        functional_response: FunctionalResponse::Saturating,
        half_saturation: Real::from(HALF_SAT_MUTUALISM),
    };
    matrix.insert(mutualist, producer, mutualism_interaction);
    matrix.insert(producer, mutualist, mutualism_interaction);
    let eco = PlanetEcosystem::new(species, matrix, capacity);
    (eco, producer, mutualist)
}

#[test]
fn pollinator_boosts_co_located_plant_clutch_size() {
    // P3.1: a Pollinator-tagged mutualist drives the producer to a
    // strictly higher post-tick biomass than a Generic mutualist of
    // the same biomass and the same matrix wiring. "Plant clutch
    // size" at this layer = the producer's biomass growth from the
    // mutualism flux (the ecosystem step is the layer where the
    // coupling lands; clutch size on the species side is downstream
    // of the biomass it sustains).
    let cap = Real::from_int(1000);
    let producer_biomass = Real::from_int(100);
    let pollinator_biomass = Real::from_int(50); // 5% of capacity → +1.5× coupling.

    let (mut eco_pollinator, prod_p, _m_p) = build_pair_for_mutualism(
        MutualismKind::Pollinator,
        producer_biomass,
        pollinator_biomass,
        cap,
    );
    let (mut eco_generic, prod_g, _m_g) = build_pair_for_mutualism(
        MutualismKind::Generic,
        producer_biomass,
        pollinator_biomass,
        cap,
    );

    eco_pollinator.step();
    eco_generic.step();

    let pol_after = eco_pollinator.species.get(&prod_p).unwrap().biomass;
    let gen_after = eco_generic.species.get(&prod_g).unwrap().biomass;
    assert!(
        pol_after > gen_after,
        "Pollinator did not boost producer biomass over Generic: \
         pollinator={pol_after:?}, generic={gen_after:?}"
    );
    // And both should be above the starting biomass (the producer
    // grew this tick — no collapse).
    assert!(
        pol_after > producer_biomass,
        "Pollinator-paired producer did not grow above starting \
         biomass (start={producer_biomass:?}, after={pol_after:?})"
    );
}

#[test]
fn seed_disperser_extends_producer_range() {
    // P3.1: a SeedDisperser whose biomass clears the activation
    // threshold (0.5% of capacity) boosts the producer's mutualism
    // flux by 1.20×. A SeedDisperser whose biomass sits *below* the
    // threshold falls through to the generic mutualism flux and
    // therefore lifts the producer by strictly less.
    let cap = Real::from_int(1000);
    let producer_biomass = Real::from_int(100);
    // Above threshold: 6 units > 0.5% × 1000 = 5.
    let active_disperser = Real::from_int(6);
    // Below threshold: 4 units < 5.
    let inactive_disperser = Real::from_int(4);

    let (mut eco_active, prod_a, _m_a) = build_pair_for_mutualism(
        MutualismKind::SeedDisperser,
        producer_biomass,
        active_disperser,
        cap,
    );
    let (mut eco_inactive, prod_i, _m_i) = build_pair_for_mutualism(
        MutualismKind::SeedDisperser,
        producer_biomass,
        inactive_disperser,
        cap,
    );

    eco_active.step();
    eco_inactive.step();

    let active_after = eco_active.species.get(&prod_a).unwrap().biomass;
    let inactive_after = eco_inactive.species.get(&prod_i).unwrap().biomass;
    assert!(
        active_after > inactive_after,
        "Above-threshold SeedDisperser did not extend producer range: \
         active={active_after:?}, inactive={inactive_after:?}"
    );
}

#[test]
fn engineer_boosts_cohabitor_match_score() {
    // P3.1: an Engineer mutualist paired with a Producer applies a
    // +10% match-score boost to *cohabiting* species (same Habitat
    // tag). A consumer species sharing the engineer's host's habitat
    // should end up with strictly higher biomass than the same
    // consumer in an ecosystem where the engineer is a Generic
    // mutualist (no cohabitor boost).
    let cap = Real::from_int(1000);
    let producer_id = SpeciesId(0);
    let engineer_id = SpeciesId(1);
    let cohabitor_id = SpeciesId(2);

    let build = |kind: MutualismKind| -> PlanetEcosystem {
        let species = vec![
            EcoSpecies {
                species_id: producer_id,
                role: EcosystemRole::Producer {
                    metabolism: ProducerMetabolism::Photoautotroph,
                },
                biomass: Real::from_int(500),
                is_extant: true,
                low_biomass_streak: 0,
                habitat: Habitat::Terrestrial,
                cell_biomass: Vec::new(),
                tolerance: ToleranceEnvelope::aqueous_default(),
            },
            EcoSpecies {
                species_id: engineer_id,
                role: EcosystemRole::Mutualist { kind },
                biomass: Real::from_int(50),
                is_extant: true,
                low_biomass_streak: 0,
                habitat: Habitat::Terrestrial,
                cell_biomass: Vec::new(),
                tolerance: ToleranceEnvelope::aqueous_default(),
            },
            EcoSpecies {
                species_id: cohabitor_id,
                role: EcosystemRole::PrimaryConsumer,
                biomass: Real::from_int(30),
                is_extant: true,
                low_biomass_streak: 0,
                habitat: Habitat::Terrestrial,
                cell_biomass: Vec::new(),
                tolerance: ToleranceEnvelope::aqueous_default(),
            },
        ];
        let mut matrix = InteractionMatrix::new();
        // Engineer ↔ Producer mutualism.
        let mutualism_interaction = Interaction {
            kind: InteractionKind::Mutualism,
            strength: Real::from((10, 100)),
            functional_response: FunctionalResponse::Saturating,
            half_saturation: Real::from(HALF_SAT_MUTUALISM),
        };
        matrix.insert(engineer_id, producer_id, mutualism_interaction);
        matrix.insert(producer_id, engineer_id, mutualism_interaction);
        PlanetEcosystem::new(species, matrix, cap)
    };

    let mut eco_engineer = build(MutualismKind::Engineer);
    let mut eco_generic = build(MutualismKind::Generic);

    // Single step — the engineer cohabitor pass adds a biomass
    // delta on top of the consumer's normal passive decay.
    eco_engineer.step();
    eco_generic.step();

    let eng_cohabitor = eco_engineer.species.get(&cohabitor_id).unwrap().biomass;
    let gen_cohabitor = eco_generic.species.get(&cohabitor_id).unwrap().biomass;
    assert!(
        eng_cohabitor > gen_cohabitor,
        "Engineer did not boost cohabitor biomass over Generic: \
         engineer={eng_cohabitor:?}, generic={gen_cohabitor:?}"
    );
}

#[test]
fn virus_parasite_fires_episodically_not_every_tick() {
    // P3.1: a Virus parasite is inert between outbreaks (its host's
    // biomass should not decrease from the parasitism interaction
    // alone — the only decay path is the per-tick consumer-decay
    // pass, which applies regardless of the parasite). On an
    // outbreak tick (`tick % VIRUS_OUTBREAK_PERIOD == 0`, `tick >
    // 0`), the host loses a sizeable fraction of its biomass to the
    // outbreak. This test verifies *both* sides: no per-tick
    // biomass-cliff between outbreaks, AND a sharp drop on the
    // outbreak tick.
    let cap = Real::from_int(1000);
    let host = SpeciesId(0);
    let virus = SpeciesId(1);
    let species = vec![
        EcoSpecies {
            species_id: host,
            role: EcosystemRole::PrimaryConsumer,
            biomass: Real::from_int(500),
            is_extant: true,
            low_biomass_streak: 0,
            habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
            tolerance: ToleranceEnvelope::aqueous_default(),
        },
        EcoSpecies {
            species_id: virus,
            role: EcosystemRole::Parasite {
                kind: ParasiteKind::Virus,
            },
            biomass: Real::from_int(10),
            is_extant: true,
            low_biomass_streak: 0,
            habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
            tolerance: ToleranceEnvelope::aqueous_default(),
        },
    ];
    let mut matrix = InteractionMatrix::new();
    matrix.insert(
        virus,
        host,
        Interaction {
            kind: InteractionKind::Parasitism,
            strength: Real::from((10, 100)),
            functional_response: FunctionalResponse::Saturating,
            half_saturation: Real::from(HALF_SAT_SPECIALIST_PREDATOR),
        },
    );
    let mut eco = PlanetEcosystem::new(species, matrix, cap);

    // Drive the system tick-by-tick, recording per-tick deltas.
    let mut host_history: Vec<Real> = Vec::new();
    host_history.push(eco.species.get(&host).unwrap().biomass);
    for tick in 1..=200u64 {
        let _ = eco.step_at_tick(tick);
        host_history.push(eco.species.get(&host).unwrap().biomass);
    }

    // Between outbreaks (e.g. ticks 1..99) the per-tick drop comes
    // *only* from the global consumer-decay pass (1%/tick) — no
    // parasitism contribution. So the relative loss from tick i to
    // i+1 in non-outbreak ticks should be ≤ a small bound (say, 5%).
    // On the outbreak tick (tick 100) the loss should be much larger
    // — at least 25% (the 30% outbreak hit + the 1% decay).
    let outbreak_pre = host_history[99]; // biomass at tick 99.
    let outbreak_post = host_history[100]; // biomass at tick 100 — after outbreak.
    let outbreak_loss_frac = (outbreak_pre - outbreak_post) / outbreak_pre;
    assert!(
        outbreak_loss_frac >= Real::from((20, 100)),
        "Virus outbreak tick did not fire a large host-biomass loss: \
         pre={outbreak_pre:?}, post={outbreak_post:?}, frac={outbreak_loss_frac:?}"
    );

    // Non-outbreak ticks (sample tick 49 → 50) should lose only the
    // small consumer-decay fraction.
    let between_pre = host_history[49];
    let between_post = host_history[50];
    let between_loss_frac = (between_pre - between_post) / between_pre;
    assert!(
        between_loss_frac < Real::from((5, 100)),
        "Virus parasite leaked biomass on a non-outbreak tick: \
         pre={between_pre:?}, post={between_post:?}, frac={between_loss_frac:?}"
    );
    // And the outbreak loss should be much larger than the per-tick
    // background decay — order of magnitude check.
    assert!(
        outbreak_loss_frac > between_loss_frac * Real::from_int(4),
        "Virus outbreak loss not significantly larger than background: \
         outbreak={outbreak_loss_frac:?}, between={between_loss_frac:?}"
    );
}

#[test]
fn macro_parasite_reduces_host_fertility_multiplier() {
    // P3.1: a Macro parasite imposes an extra -10% fertility hit on
    // top of the generic parasitism flux. Compare against a Micro
    // parasite paired with a *sparse* host (below the crowding
    // threshold so Micro's extra penalty is zero) under identical
    // matrix wiring — the Macro host should bleed strictly more
    // biomass per tick than the Micro host.
    let cap = Real::from_int(1000);
    let host_id = SpeciesId(0);
    let parasite_id = SpeciesId(1);
    // Host biomass below the 5% crowding threshold so the Micro
    // branch's extra penalty is exactly zero — only the base flux
    // applies on the Micro side, and the Macro side adds the +10%
    // fertility penalty on top.
    let host_biomass = Real::from_int(20); // 2% of capacity, below 5%.
    let parasite_biomass = Real::from_int(5);

    let build = |kind: ParasiteKind| -> PlanetEcosystem {
        let species = vec![
            EcoSpecies {
                species_id: host_id,
                role: EcosystemRole::PrimaryConsumer,
                biomass: host_biomass,
                is_extant: true,
                low_biomass_streak: 0,
                habitat: Habitat::Terrestrial,
                cell_biomass: Vec::new(),
                tolerance: ToleranceEnvelope::aqueous_default(),
            },
            EcoSpecies {
                species_id: parasite_id,
                role: EcosystemRole::Parasite { kind },
                biomass: parasite_biomass,
                is_extant: true,
                low_biomass_streak: 0,
                habitat: Habitat::Terrestrial,
                cell_biomass: Vec::new(),
                tolerance: ToleranceEnvelope::aqueous_default(),
            },
        ];
        let mut matrix = InteractionMatrix::new();
        matrix.insert(
            parasite_id,
            host_id,
            Interaction {
                kind: InteractionKind::Parasitism,
                strength: Real::from((10, 100)),
                functional_response: FunctionalResponse::Saturating,
                half_saturation: Real::from(HALF_SAT_SPECIALIST_PREDATOR),
            },
        );
        PlanetEcosystem::new(species, matrix, cap)
    };

    let mut eco_macro = build(ParasiteKind::Macro);
    let mut eco_micro = build(ParasiteKind::Micro);

    eco_macro.step();
    eco_micro.step();

    let macro_host_after = eco_macro.species.get(&host_id).unwrap().biomass;
    let micro_host_after = eco_micro.species.get(&host_id).unwrap().biomass;

    assert!(
        macro_host_after < micro_host_after,
        "Macro parasite did not impose extra fertility hit: \
         macro_host={macro_host_after:?}, micro_host={micro_host_after:?}"
    );
}
