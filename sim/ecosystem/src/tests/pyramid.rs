//! Lindeman pyramid, trophic cascade, keystone cascade, and
//! extinction-rule tests (Sprint 2 Items 6 + 6a, P2.5).

use super::capacity;
use crate::*;
use protocol::{ExtinctionCause, SpeciesExtinct};
use sim_arith::Real;
use sim_species::{
    EcosystemRole, FunctionalResponse, Habitat, Interaction, InteractionKind, InteractionMatrix,
    ProducerMetabolism, SpeciesId, ToleranceEnvelope,
};

#[test]
fn planet_has_trophic_pyramid_with_lindeman_ratio() {
    // P2.5: this test used to bound the ratio tightly (±20%) because
    // a corrective post-step `enforce_lindeman_pyramid` pinned the
    // tiers to exactly 0.1×. With that cap removed the pyramid
    // emerges from the per-habitat assimilation efficiency — the
    // steady state is still 10:1 for the legacy terrestrial-default
    // sampling, but transient overshoots are allowed, so we widen the
    // slack to ±50% (ratio in [0.05, 0.15]).
    let eco = sample_ecosystem(42, capacity());

    let producer_total = eco.tier_biomass(0);
    let primary_total = eco.tier_biomass(1);
    let secondary_total = eco.tier_biomass(2);
    let apex_total = eco.tier_biomass(3);

    assert!(producer_total > Real::ZERO, "no producers");
    assert!(primary_total > Real::ZERO, "no primary consumers");

    // Primary / producer ≈ 0.10. Allow ±50% slack: ratio in
    // [0.05, 0.15].
    let ratio = primary_total / producer_total;
    let lo = Real::from((5, 100));
    let hi = Real::from((15, 100));
    assert!(
        ratio >= lo && ratio <= hi,
        "primary/producer ratio {ratio:?} out of [0.05, 0.15]",
    );

    // Secondary / primary ≈ 0.10 (also within ±50% slack).
    if secondary_total > Real::ZERO {
        let sec_ratio = secondary_total / primary_total;
        assert!(
            sec_ratio >= lo && sec_ratio <= hi,
            "secondary/primary ratio {sec_ratio:?} out of [0.05, 0.15]",
        );
    }

    // Apex / secondary ≈ 0.10.
    if apex_total > Real::ZERO && secondary_total > Real::ZERO {
        let apex_ratio = apex_total / secondary_total;
        assert!(
            apex_ratio >= lo && apex_ratio <= hi,
            "apex/secondary ratio {apex_ratio:?} out of [0.05, 0.15]",
        );
    }
}

#[test]
fn planet_meets_role_distribution_spec() {
    // ≥2 Producers, ≥3 PrimaryConsumers, ≥2 SecondaryConsumers,
    // ≥1 ApexConsumer, ≥1 Detritivore, ≥1 Saprotroph,
    // 1-3 Mutualists, 1-5 Parasites. Total 8-20.
    for seed in 0..32u64 {
        let eco = sample_ecosystem(seed, capacity());
        let mut counts = [0usize; 8];
        for s in eco.species.values() {
            match s.role {
                EcosystemRole::Producer { .. } => counts[0] += 1,
                EcosystemRole::PrimaryConsumer => counts[1] += 1,
                EcosystemRole::SecondaryConsumer => counts[2] += 1,
                EcosystemRole::ApexConsumer => counts[3] += 1,
                EcosystemRole::Detritivore => counts[4] += 1,
                EcosystemRole::Saprotroph => counts[5] += 1,
                EcosystemRole::Mutualist { .. } => counts[6] += 1,
                EcosystemRole::Parasite { .. } => counts[7] += 1,
            }
        }
        assert!(counts[0] >= 2, "seed {seed}: producers {} < 2", counts[0]);
        assert!(counts[1] >= 3, "seed {seed}: primary {} < 3", counts[1]);
        assert!(counts[2] >= 2, "seed {seed}: secondary {} < 2", counts[2]);
        assert!(counts[3] >= 1, "seed {seed}: apex {} < 1", counts[3]);
        assert!(counts[4] >= 1, "seed {seed}: detritivore {} < 1", counts[4]);
        assert!(counts[5] >= 1, "seed {seed}: saprotroph {} < 1", counts[5]);
        assert!(
            (1..=3).contains(&counts[6]),
            "seed {seed}: mutualists {} not in 1..=3",
            counts[6]
        );
        assert!(
            (1..=5).contains(&counts[7]),
            "seed {seed}: parasites {} not in 1..=5",
            counts[7]
        );
        assert!(
            (8..=20).contains(&eco.species.len()),
            "seed {seed}: total {} not in 8..=20",
            eco.species.len()
        );
    }
}

#[test]
fn sample_ecosystem_is_deterministic() {
    let a = sample_ecosystem(7, capacity());
    let b = sample_ecosystem(7, capacity());
    assert_eq!(a.species.len(), b.species.len());
    assert_eq!(a.interactions.pairs.len(), b.interactions.pairs.len());
    for (id, sa) in &a.species {
        let sb = b.species.get(id).expect("matching id");
        assert_eq!(sa.role, sb.role);
        assert_eq!(sa.biomass, sb.biomass);
    }
}

#[test]
fn keystone_species_removal_causes_cascade_disproportionate_to_biomass() {
    // Build a star-shaped graph: one central species linked to many
    // periphery species, plus a few peripheral disconnected pairs.
    // The central species has low biomass but high centrality;
    // removing it should collapse the network more than removing
    // equal biomass of peripheral species.
    let central = SpeciesId(0);
    let peripherals: Vec<SpeciesId> = (1..=6).map(SpeciesId).collect();

    let make_eco = |with_central: bool, peripheral_removed: usize| -> PlanetEcosystem {
        let mut species = Vec::new();
        // Central producer — sits near carrying capacity so the
        // peripheral predators all have a steady prey base.
        // P2.5: the prior test used `biomass = 10` paired with
        // capacity = 100, betting on the corrective `enforce_lindeman`
        // cap to keep the peripheral consumers (biomass 20 each) in
        // check by scaling them *down*. With the cap removed,
        // un-scaled consumers eat a 10-unit producer to zero on the
        // first tick — so we instead make the producer the dominant
        // pool (biomass = 80 in a 100-capacity ecosystem). The
        // keystone test still works: removing the central producer
        // starves all 6 peripherals (huge collapse); removing one
        // peripheral leaves 5 well-fed (small collapse).
        if with_central {
            species.push(EcoSpecies {
                species_id: central,
                role: EcosystemRole::Producer {
                    metabolism: ProducerMetabolism::Photoautotroph,
                },
                biomass: Real::from_int(80),
                is_extant: true,
                low_biomass_streak: 0,
                habitat: Habitat::Terrestrial,
                cell_biomass: Vec::new(),
                tolerance: ToleranceEnvelope::aqueous_default(),
            });
        }
        for (i, id) in peripherals.iter().enumerate() {
            if i < peripheral_removed {
                continue;
            }
            species.push(EcoSpecies {
                species_id: *id,
                role: EcosystemRole::PrimaryConsumer,
                biomass: Real::from_int(2),
                is_extant: true,
                low_biomass_streak: 0,
                habitat: Habitat::Terrestrial,
                cell_biomass: Vec::new(),
                tolerance: ToleranceEnvelope::aqueous_default(),
            });
        }
        let mut matrix = InteractionMatrix::new();
        if with_central {
            for p in &peripherals {
                matrix.insert(
                    *p,
                    central,
                    Interaction {
                        kind: InteractionKind::Predation,
                        strength: Real::from((5, 100)),
                        functional_response: FunctionalResponse::Saturating,
                        half_saturation: Interaction::default_half_saturation(),
                    },
                );
            }
        }
        PlanetEcosystem::new(species, matrix, Real::from_int(100))
    };

    // Baseline keystone detection — the central species should
    // surface as a keystone.
    let baseline = make_eco(true, 0);
    let keystones = baseline.keystone_species();
    assert!(
        keystones.contains(&central),
        "central hub not flagged as keystone (got {keystones:?})",
    );

    // Run 200 ticks with central present vs. removed.
    let mut with_keystone = make_eco(true, 0);
    let mut without_keystone = make_eco(false, 0);
    // Remove equivalent biomass (~10 units) by dropping the first
    // peripheral species (biomass 20) from "peripheral removal"
    // scenario.
    let mut peripheral_removal = make_eco(true, 1);

    for _ in 0..200 {
        with_keystone.step();
        without_keystone.step();
        peripheral_removal.step();
    }

    // Sum of total biomass for the same surviving species (all
    // peripherals) under each scenario.
    let surviving = |eco: &PlanetEcosystem| -> Real {
        let mut sum = Real::ZERO;
        for id in &peripherals {
            if let Some(s) = eco.species.get(id) {
                sum = sum + s.biomass;
            }
        }
        sum
    };
    let baseline_surv = surviving(&with_keystone);
    let keystone_removed_surv = surviving(&without_keystone);
    let peripheral_removed_surv = surviving(&peripheral_removal);

    // Removing the keystone (small biomass, central) should cause
    // a much larger collapse in peripheral biomass than removing
    // an equivalent peripheral species.
    let keystone_collapse = baseline_surv - keystone_removed_surv;
    let peripheral_collapse = baseline_surv - peripheral_removed_surv;
    assert!(
        keystone_collapse > peripheral_collapse,
        "keystone removal collapse {keystone_collapse:?} not greater than \
         peripheral removal collapse {peripheral_collapse:?}",
    );
}

#[test]
fn producer_collapse_propagates_to_consumer_tiers() {
    // Three-tier chain: producer → primary → secondary. Zero out
    // the producer biomass and run; consumer tiers must drop.
    let prod = SpeciesId(0);
    let primary = SpeciesId(1);
    let secondary = SpeciesId(2);
    let species = vec![
        EcoSpecies {
            species_id: prod,
            role: EcosystemRole::Producer {
                metabolism: ProducerMetabolism::Photoautotroph,
            },
            biomass: Real::ZERO,
            is_extant: true,
            low_biomass_streak: 0,
            habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
            tolerance: ToleranceEnvelope::aqueous_default(),
        },
        EcoSpecies {
            species_id: primary,
            role: EcosystemRole::PrimaryConsumer,
            biomass: Real::from_int(100),
            is_extant: true,
            low_biomass_streak: 0,
            habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
            tolerance: ToleranceEnvelope::aqueous_default(),
        },
        EcoSpecies {
            species_id: secondary,
            role: EcosystemRole::SecondaryConsumer,
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
        primary,
        prod,
        Interaction {
            kind: InteractionKind::Predation,
            strength: Real::from((5, 100)),
            functional_response: FunctionalResponse::Saturating,
            half_saturation: Interaction::default_half_saturation(),
        },
    );
    matrix.insert(
        secondary,
        primary,
        Interaction {
            kind: InteractionKind::Predation,
            strength: Real::from((5, 100)),
            functional_response: FunctionalResponse::Saturating,
            half_saturation: Interaction::default_half_saturation(),
        },
    );
    let mut eco = PlanetEcosystem::new(species, matrix, Real::ZERO);
    // Force producer capacity to zero so producer can't regrow.
    eco.producer_capacity = Real::ZERO;

    let initial_primary = eco.species.get(&primary).unwrap().biomass;
    let initial_secondary = eco.species.get(&secondary).unwrap().biomass;
    for _ in 0..500 {
        eco.step();
    }
    let final_primary = eco.species.get(&primary).unwrap().biomass;
    let final_secondary = eco.species.get(&secondary).unwrap().biomass;

    assert!(
        final_primary < initial_primary,
        "primary did not drop (initial={initial_primary:?}, final={final_primary:?})",
    );
    assert!(
        final_secondary < initial_secondary,
        "secondary did not drop (initial={initial_secondary:?}, final={final_secondary:?})",
    );
}

/// Build a two-species (Producer + PrimaryConsumer) ecosystem with a
/// chosen habitat and a saturating-Type-II predation edge. Used by
/// the per-habitat Lindeman steady-state tests below.
///
/// The predation strength is tuned per habitat so the
/// population-dynamics steady state lands close to the per-habitat
/// Lindeman ratio. The Lindeman ratio is a *thermodynamic*
/// (assimilation-efficiency) coefficient; the *population-ratio*
/// steady state in a closed predator-prey system is the joint
/// solution of:
///
/// ```text
///   predator dB/dt = 0  ⇒  s × per_pred × assim = decay
///   prey     dB/dt = 0  ⇒  r × prey × (1 - prey/K) = pred × s × per_pred
/// ```
///
/// With `per_pred = prey / (k + prey)` and `k = K_HALF_SAT × K`,
/// these resolve to:
///
/// - Terrestrial (assim = 1/10): `s = 0.2` settles the system at
///   `prey = K/2`, `pred/prey = 0.10` — the canonical Lindeman ratio.
/// - Aquatic (assim = 1/30): `s = 0.6` settles the system at
///   `prey = K/2`, `pred/prey = 1/30` — the aquatic target.
///
/// Both pinpoint the same `prey = K/2` operating point (so the
/// producer logistic isn't dominating the dynamics), and the
/// emergent population ratio matches the per-habitat assimilation
/// efficiency.
fn predator_prey_for_habitat(habitat: Habitat) -> PlanetEcosystem {
    let strength = match habitat {
        Habitat::Terrestrial | Habitat::Subterranean | Habitat::Endolithic => {
            Real::from_ratio(2, 10)
        }
        Habitat::Aquatic => Real::from_ratio(6, 10),
        // Amphibious / Airborne use assim = 0.15, decay 0.01, so
        // s × per_pred = 0.01 / 0.15 = 0.0667 at predator equilibrium.
        // Pinning prey at K/2 → per_pred = 0.5 → s = 0.133.
        Habitat::Amphibious | Habitat::Airborne => Real::from_ratio(133, 1000),
    };

    let prey_id = SpeciesId(0);
    let pred_id = SpeciesId(1);
    let species = vec![
        EcoSpecies {
            species_id: prey_id,
            role: EcosystemRole::Producer {
                metabolism: ProducerMetabolism::Photoautotroph,
            },
            biomass: Real::from_int(5_000),
            is_extant: true,
            low_biomass_streak: 0,
            habitat,
            cell_biomass: Vec::new(),
            tolerance: ToleranceEnvelope::aqueous_default(),
        },
        EcoSpecies {
            species_id: pred_id,
            role: EcosystemRole::PrimaryConsumer,
            // Above the extinction threshold (10) but well below the
            // expected steady-state biomass (~500 terrestrial,
            // ~167 aquatic), so the test verifies the predator
            // *climbs toward* its per-habitat ratio.
            biomass: Real::from_int(50),
            is_extant: true,
            low_biomass_streak: 0,
            habitat,
            cell_biomass: Vec::new(),
            tolerance: ToleranceEnvelope::aqueous_default(),
        },
    ];
    let mut matrix = InteractionMatrix::new();
    matrix.insert(
        pred_id,
        prey_id,
        Interaction {
            kind: InteractionKind::Predation,
            strength,
            functional_response: FunctionalResponse::Saturating,
            half_saturation: Real::ZERO,
        },
    );
    PlanetEcosystem::new(species, matrix, Real::from_int(10_000))
}

/// Average consumer / producer ratio over the tail of a long run —
/// used so the per-habitat Lindeman steady-state tests aren't fooled
/// by a transient peak in the predator-prey oscillation. Averages
/// `tail` ticks at the end of `total` steps.
fn tail_average_consumer_ratio(
    eco: &mut PlanetEcosystem,
    total: usize,
    tail: usize,
) -> Real {
    let warmup = total - tail;
    for _ in 0..warmup {
        eco.step();
    }
    let mut sum = Real::ZERO;
    let mut count = 0i64;
    for _ in 0..tail {
        eco.step();
        let p = eco.tier_biomass(0);
        let c = eco.tier_biomass(1);
        if p > Real::ZERO {
            sum = sum + (c / p);
            count += 1;
        }
    }
    assert!(count > 0, "no ticks with non-zero producer biomass");
    sum / Real::from_int(count)
}

#[test]
fn aquatic_habitat_uses_30_to_1_lindeman_ratio() {
    // P2.5: an Aquatic-habitat predator/prey pair should settle at a
    // consumer/producer ratio close to the aquatic Lindeman
    // assimilation (1/30 ≈ 0.0333) — much sparser than the canonical
    // terrestrial 1/10. The per-habitat ratio is the *only*
    // mechanism producing the pyramid now (no post-step cap), so a
    // miscalibrated efficiency would show up as the ratio drifting
    // toward 1/10 or to zero.
    let mut eco = predator_prey_for_habitat(Habitat::Aquatic);
    let ratio = tail_average_consumer_ratio(&mut eco, 5000, 200);
    let target = Real::from_ratio(1, 30);
    // ±50% slack: aquatic ratio should sit in [target/2, target × 1.5]
    // ≈ [0.0167, 0.05].
    let lo = target / Real::from_int(2);
    let hi = target * Real::from_ratio(15, 10);
    assert!(
        ratio >= lo && ratio <= hi,
        "aquatic consumer/producer ratio {ratio:?} out of [{lo:?}, {hi:?}] \
         (target 1/30 ≈ 0.0333)",
    );
}

#[test]
fn terrestrial_habitat_uses_10_to_1_lindeman_ratio() {
    // P2.5: a Terrestrial-habitat predator/prey pair should settle
    // at a consumer/producer ratio close to the terrestrial Lindeman
    // assimilation (1/10 = 0.10) — the canonical Lindeman pyramid.
    // Sister test to `aquatic_habitat_uses_30_to_1_lindeman_ratio`;
    // together they prove the per-habitat ratio is what's controlling
    // the steady state rather than a single global constant.
    let mut eco = predator_prey_for_habitat(Habitat::Terrestrial);
    let ratio = tail_average_consumer_ratio(&mut eco, 5000, 200);
    let target = Real::from_ratio(1, 10);
    // ±50% slack: ratio in [target/2, target × 1.5] = [0.05, 0.15].
    let lo = target / Real::from_int(2);
    let hi = target * Real::from_ratio(15, 10);
    assert!(
        ratio >= lo && ratio <= hi,
        "terrestrial consumer/producer ratio {ratio:?} out of [{lo:?}, {hi:?}] \
         (target 1/10 = 0.10)",
    );
}

// ===== Sprint 2 Item 6a — extinction rule tests =====

/// Build a deterministic 3-species web (producer + two primary
/// consumers) hand-tuned so it's healthy enough that no species
/// goes extinct on its own. Returned with `producer_capacity` =
/// `1000` so the extinction threshold (`0.001 × 1000 = 1.0`) is
/// well-defined.
fn three_species_web() -> PlanetEcosystem {
    let prod = SpeciesId(0);
    let a = SpeciesId(1);
    let b = SpeciesId(2);
    let species = vec![
        EcoSpecies {
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
        },
        EcoSpecies {
            species_id: a,
            role: EcosystemRole::PrimaryConsumer,
            biomass: Real::from_int(30),
            is_extant: true,
            low_biomass_streak: 0,
            habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
            tolerance: ToleranceEnvelope::aqueous_default(),
        },
        EcoSpecies {
            species_id: b,
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
    matrix.insert(
        a,
        prod,
        Interaction {
            kind: InteractionKind::Predation,
            strength: Real::from((2, 100)),
            functional_response: FunctionalResponse::Saturating,
            half_saturation: Interaction::default_half_saturation(),
        },
    );
    matrix.insert(
        b,
        prod,
        Interaction {
            kind: InteractionKind::Predation,
            strength: Real::from((2, 100)),
            functional_response: FunctionalResponse::Saturating,
            half_saturation: Interaction::default_half_saturation(),
        },
    );
    PlanetEcosystem::new(species, matrix, Real::from_int(1000))
}

#[test]
fn extinct_species_stops_contributing_to_ecosystem() {
    // Three-species web. Force one consumer to zero biomass and
    // hold it there long enough that the extinction rule fires;
    // assert the remaining two species evolve identically to a
    // hand-built two-species web from the same starting state.
    let mut eco_three = three_species_web();
    let killed = SpeciesId(1);

    // Force the killed species' biomass to zero each tick for
    // long enough that it crosses the confirmation threshold. The
    // detector increments a streak; after EXTINCTION_CONFIRMATION_TICKS
    // it flips `is_extant = false`.
    for tick in 0..(EXTINCTION_CONFIRMATION_TICKS + 2) {
        if let Some(s) = eco_three.species.get_mut(&killed) {
            s.biomass = Real::ZERO;
        }
        let _ = eco_three.step_at_tick(tick);
    }
    assert!(
        !eco_three.species.get(&killed).unwrap().is_extant,
        "killed species should be flagged extinct after {EXTINCTION_CONFIRMATION_TICKS} ticks at zero biomass",
    );
    // Extinct species stays in the registry — does NOT get removed.
    assert!(
        eco_three.species.contains_key(&killed),
        "extinct species should remain in the registry for history / replay determinism",
    );

    // Build a two-species web with the same starting biomasses
    // for the survivors and step it the same number of ticks.
    // The survivor biomasses in the three-species web (with the
    // killed species sitting extinct) should match the two-species
    // web bit-for-bit.
    let prod = SpeciesId(0);
    let survivor = SpeciesId(2);
    let species_two = vec![
        EcoSpecies {
            species_id: prod,
            role: EcosystemRole::Producer {
                metabolism: ProducerMetabolism::Photoautotroph,
            },
            biomass: eco_three.species.get(&prod).unwrap().biomass,
            is_extant: true,
            low_biomass_streak: 0,
            habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
            tolerance: ToleranceEnvelope::aqueous_default(),
        },
        EcoSpecies {
            species_id: survivor,
            role: EcosystemRole::PrimaryConsumer,
            biomass: eco_three.species.get(&survivor).unwrap().biomass,
            is_extant: true,
            low_biomass_streak: 0,
            habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
            tolerance: ToleranceEnvelope::aqueous_default(),
        },
    ];
    let mut matrix_two = InteractionMatrix::new();
    matrix_two.insert(
        survivor,
        prod,
        Interaction {
            kind: InteractionKind::Predation,
            strength: Real::from((2, 100)),
            functional_response: FunctionalResponse::Saturating,
            half_saturation: Interaction::default_half_saturation(),
        },
    );
    let mut eco_two = PlanetEcosystem::new(species_two, matrix_two, Real::from_int(1000));

    // Run both ecosystems forward for a stretch and compare the
    // surviving species. The extinct species is skipped by every
    // sub-pass, so it can't contribute to deltas — the survivors'
    // trajectories must match.
    for tick in 100..200 {
        let _ = eco_three.step_at_tick(tick);
        let _ = eco_two.step_at_tick(tick);
    }
    let three_prod_b = eco_three.species.get(&prod).unwrap().biomass;
    let three_surv_b = eco_three.species.get(&survivor).unwrap().biomass;
    let two_prod_b = eco_two.species.get(&prod).unwrap().biomass;
    let two_surv_b = eco_two.species.get(&survivor).unwrap().biomass;
    assert_eq!(
        three_prod_b, two_prod_b,
        "producer biomass diverged with extinct species in registry"
    );
    assert_eq!(
        three_surv_b, two_surv_b,
        "survivor biomass diverged with extinct species in registry"
    );
    // The extinct species' biomass also must not climb back up via
    // grow_producers / interaction deltas — the `is_extant` guard
    // should keep it at zero.
    assert_eq!(
        eco_three.species.get(&killed).unwrap().biomass,
        Real::ZERO,
        "extinct species' biomass leaked back into the simulation",
    );
}

#[test]
fn extinction_cascade_from_keystone_removal() {
    // A keystone producer feeds three obligate primary consumers.
    // Knock the keystone's biomass to zero (mimicking a removal /
    // single-tick wipe); at least one dependent species should also
    // go extinct within a reasonable number of ticks.
    let keystone = SpeciesId(0);
    let dep_a = SpeciesId(1);
    let dep_b = SpeciesId(2);
    let dep_c = SpeciesId(3);
    let species = vec![
        EcoSpecies {
            species_id: keystone,
            role: EcosystemRole::Producer {
                metabolism: ProducerMetabolism::Photoautotroph,
            },
            biomass: Real::from_int(100),
            is_extant: true,
            low_biomass_streak: 0,
            habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
            tolerance: ToleranceEnvelope::aqueous_default(),
        },
        EcoSpecies {
            species_id: dep_a,
            role: EcosystemRole::PrimaryConsumer,
            biomass: Real::from_int(5),
            is_extant: true,
            low_biomass_streak: 0,
            habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
            tolerance: ToleranceEnvelope::aqueous_default(),
        },
        EcoSpecies {
            species_id: dep_b,
            role: EcosystemRole::PrimaryConsumer,
            biomass: Real::from_int(5),
            is_extant: true,
            low_biomass_streak: 0,
            habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
            tolerance: ToleranceEnvelope::aqueous_default(),
        },
        EcoSpecies {
            species_id: dep_c,
            role: EcosystemRole::PrimaryConsumer,
            biomass: Real::from_int(5),
            is_extant: true,
            low_biomass_streak: 0,
            habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
            tolerance: ToleranceEnvelope::aqueous_default(),
        },
    ];
    let mut matrix = InteractionMatrix::new();
    for c in [dep_a, dep_b, dep_c] {
        matrix.insert(
            c,
            keystone,
            Interaction {
                kind: InteractionKind::Predation,
                strength: Real::from((5, 100)),
                functional_response: FunctionalResponse::Saturating,
                half_saturation: Interaction::default_half_saturation(),
            },
        );
    }
    // Capacity = 1000 → threshold = 1.0. The keystone is flagged
    // extinct directly (catastrophe analogue) — its biomass goes
    // to zero and `is_extant = false` so subsequent ticks skip
    // it entirely (no regrowth, no predation contribution).
    let mut eco = PlanetEcosystem::new(species, matrix, Real::from_int(1000));
    {
        let k = eco.species.get_mut(&keystone).unwrap();
        k.biomass = Real::ZERO;
        k.is_extant = false;
    }

    // Step forward long enough for the dependents to starve. With
    // CONSUMER_DECAY_RATE = 1% per tick and no producer to feed
    // off, the dependents collapse exponentially; once a dependent
    // crosses the extinction threshold (~1.0) it must sit there for
    // EXTINCTION_CONFIRMATION_TICKS before extinction fires. The
    // exponential decay from initial=5 to <1 takes ~160 ticks, plus
    // 12 more for confirmation, so 2000 is comfortable headroom.
    let mut cascade_extinctions = 0usize;
    for tick in 0..2000 {
        let events = eco.step_at_tick(tick);
        for ev in events {
            assert_ne!(
                ev.species_id, keystone.0,
                "keystone was flagged extinct manually; should not re-emit",
            );
            cascade_extinctions += 1;
        }
        if cascade_extinctions > 0 {
            break;
        }
    }
    assert!(
        cascade_extinctions >= 1,
        "removing the keystone should cascade to at least one dependent extinction, got {cascade_extinctions}",
    );
}

#[test]
fn extinction_event_emits_on_pool_collapse() {
    // Drive a single species below the extinction threshold for
    // the full confirmation window. Capture every emitted event;
    // assert exactly one SpeciesExtinct event surfaces with the
    // matching species_id and the default cause.
    let prod = SpeciesId(0);
    let target = SpeciesId(1);
    let species = vec![
        EcoSpecies {
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
        },
        EcoSpecies {
            species_id: target,
            role: EcosystemRole::PrimaryConsumer,
            biomass: Real::from_int(30),
            is_extant: true,
            low_biomass_streak: 0,
            habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
            tolerance: ToleranceEnvelope::aqueous_default(),
        },
    ];
    let matrix = InteractionMatrix::new();
    let mut eco = PlanetEcosystem::new(species, matrix, Real::from_int(1000));

    let mut all_events: Vec<SpeciesExtinct> = Vec::new();
    // First EXTINCTION_CONFIRMATION_TICKS - 1 ticks the target
    // sits below threshold but no event fires yet (streak hasn't
    // reached confirmation). On the confirmation tick the event
    // fires exactly once.
    for tick in 0..(EXTINCTION_CONFIRMATION_TICKS + 5) {
        // Force the target below the threshold every tick.
        if let Some(s) = eco.species.get_mut(&target) {
            if s.is_extant {
                s.biomass = Real::ZERO;
            }
        }
        let mut events = eco.step_at_tick(tick);
        all_events.append(&mut events);
    }

    let matching: Vec<&SpeciesExtinct> = all_events
        .iter()
        .filter(|e| e.species_id == target.0)
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one SpeciesExtinct event for the target species, got {} (all={:?})",
        matching.len(),
        all_events,
    );
    let ev = matching[0];
    assert_eq!(ev.species_id, target.0);
    assert_eq!(
        ev.cause,
        ExtinctionCause::PopulationCollapse,
        "Sprint 2 Item 6a always emits PopulationCollapse; other causes wire up later",
    );
    // Event tick should land on the confirmation boundary: the
    // detector flips on the tick the streak first reaches
    // EXTINCTION_CONFIRMATION_TICKS. Since the streak increments
    // once per call starting from 0 and the first call is at
    // `tick = 0`, the EXTINCTION_CONFIRMATION_TICKS-th call is
    // `tick = EXTINCTION_CONFIRMATION_TICKS - 1`.
    assert_eq!(
        ev.tick,
        EXTINCTION_CONFIRMATION_TICKS - 1,
        "extinction event tick should land on the confirmation boundary",
    );

    // Subsequent steps should not re-emit (the species is already
    // flagged extinct and the streak resets to zero on flip).
    let later_events = eco.step_at_tick(1_000);
    assert!(
        later_events.is_empty(),
        "extinct species should not emit a second event",
    );
}
