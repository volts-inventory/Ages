//! Sprint 2 Item 6 tests.
//!
//! Five required cases (per plan v2):
//! 1. `planet_has_trophic_pyramid_with_lindeman_ratio`
//! 2. `predator_prey_pair_exhibits_lotka_volterra_cycles`
//! 3. `keystone_species_removal_causes_cascade_disproportionate_to_biomass`
//! 4. `producer_collapse_propagates_to_consumer_tiers`
//! 5. `competition_pair_excludes_at_equilibrium`

use super::*;
use sim_arith::Real;
use sim_species::{
    EcosystemRole, FunctionalResponse, Interaction, InteractionKind, InteractionMatrix,
    ProducerMetabolism, SpeciesId,
};

fn capacity() -> Real {
    Real::from_int(1000)
}

#[test]
fn planet_has_trophic_pyramid_with_lindeman_ratio() {
    // After sampling, primary tier biomass should sit at ~10% of
    // producer tier (within 20% slack per spec).
    let eco = sample_ecosystem(42, capacity());

    let producer_total = eco.tier_biomass(0);
    let primary_total = eco.tier_biomass(1);
    let secondary_total = eco.tier_biomass(2);
    let apex_total = eco.tier_biomass(3);

    assert!(producer_total > Real::ZERO, "no producers");
    assert!(primary_total > Real::ZERO, "no primary consumers");

    // Primary / producer ≈ 0.10. Allow ±20% slack: i.e. ratio in
    // [0.08, 0.12].
    let ratio = primary_total / producer_total;
    let lo = Real::from((8, 100));
    let hi = Real::from((12, 100));
    assert!(
        ratio >= lo && ratio <= hi,
        "primary/producer ratio {ratio:?} out of [0.08, 0.12]",
    );

    // Secondary / primary ≈ 0.10 (also within ±20% slack).
    if secondary_total > Real::ZERO {
        let sec_ratio = secondary_total / primary_total;
        assert!(
            sec_ratio >= lo && sec_ratio <= hi,
            "secondary/primary ratio {sec_ratio:?} out of [0.08, 0.12]",
        );
    }

    // Apex / secondary ≈ 0.10.
    if apex_total > Real::ZERO && secondary_total > Real::ZERO {
        let apex_ratio = apex_total / secondary_total;
        assert!(
            apex_ratio >= lo && apex_ratio <= hi,
            "apex/secondary ratio {apex_ratio:?} out of [0.08, 0.12]",
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
fn predator_prey_pair_exhibits_lotka_volterra_cycles() {
    // Two-species ecosystem: producer (prey) + primary consumer
    // (predator). Run 1000 ticks; count zero-crossings of the
    // predator's biomass derivative. Genuine LV oscillation gives
    // ≥2 crossings (one peak, one trough → 2 sign changes); a
    // monotonic decay or flat line gives 0-1.
    let prey_id = SpeciesId(0);
    let pred_id = SpeciesId(1);
    let species = vec![
        EcoSpecies {
            species_id: prey_id,
            role: EcosystemRole::Producer {
                metabolism: ProducerMetabolism::Photoautotroph,
            },
            biomass: Real::from_int(800),
            is_extant: true,
        },
        EcoSpecies {
            species_id: pred_id,
            role: EcosystemRole::PrimaryConsumer,
            biomass: Real::from_int(5),
            is_extant: true,
        },
    ];
    let mut matrix = InteractionMatrix::new();
    // Stronger-than-default predation so the cycles materialise in
    // 1000 ticks. The capacity (10_000) is set well above the
    // Lindeman cap so the predator's growth is rate-limited by the
    // functional response, not by the cap binding immediately —
    // otherwise both biomasses sit pinned at the pyramid ceiling
    // and never oscillate.
    matrix.insert(
        pred_id,
        prey_id,
        Interaction {
            kind: InteractionKind::Predation,
            strength: Real::from((50, 100)),
            functional_response: FunctionalResponse::Saturating,
        },
    );

    let mut eco = PlanetEcosystem::new(species, matrix, Real::from_int(10_000));

    let mut history: Vec<Real> = Vec::with_capacity(1000);
    for _ in 0..1000 {
        eco.step();
        history.push(eco.species.get(&pred_id).unwrap().biomass);
    }

    // Compute discrete derivative and count sign changes.
    let mut sign_changes = 0usize;
    let mut last_sign: i32 = 0;
    for w in history.windows(2) {
        let d = w[1] - w[0];
        let s: i32 = if d > Real::ZERO {
            1
        } else if d < Real::ZERO {
            -1
        } else {
            0
        };
        if s != 0 && last_sign != 0 && s != last_sign {
            sign_changes += 1;
        }
        if s != 0 {
            last_sign = s;
        }
    }
    assert!(
        sign_changes >= 2,
        "predator biomass did not oscillate (only {sign_changes} sign changes in derivative)",
    );
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
        // Central producer — low biomass.
        if with_central {
            species.push(EcoSpecies {
                species_id: central,
                role: EcosystemRole::Producer {
                    metabolism: ProducerMetabolism::Photoautotroph,
                },
                biomass: Real::from_int(10),
                is_extant: true,
            });
        }
        for (i, id) in peripherals.iter().enumerate() {
            if i < peripheral_removed {
                continue;
            }
            species.push(EcoSpecies {
                species_id: *id,
                role: EcosystemRole::PrimaryConsumer,
                biomass: Real::from_int(20),
                is_extant: true,
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
        },
        EcoSpecies {
            species_id: primary,
            role: EcosystemRole::PrimaryConsumer,
            biomass: Real::from_int(100),
            is_extant: true,
        },
        EcoSpecies {
            species_id: secondary,
            role: EcosystemRole::SecondaryConsumer,
            biomass: Real::from_int(10),
            is_extant: true,
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
        },
    );
    matrix.insert(
        secondary,
        primary,
        Interaction {
            kind: InteractionKind::Predation,
            strength: Real::from((5, 100)),
            functional_response: FunctionalResponse::Saturating,
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

#[test]
fn competition_pair_excludes_at_equilibrium() {
    // Two PrimaryConsumers competing for the same producer pool.
    // One starts at higher biomass (asymmetric initial condition);
    // the stronger competitor should drive the weaker toward
    // extinction. Distinct dynamic from predation — no oscillation,
    // monotonic collapse on the losing side.
    let prod = SpeciesId(0);
    let strong = SpeciesId(1);
    let weak = SpeciesId(2);

    let species = vec![
        EcoSpecies {
            species_id: prod,
            role: EcosystemRole::Producer {
                metabolism: ProducerMetabolism::Photoautotroph,
            },
            biomass: Real::from_int(500),
            is_extant: true,
        },
        EcoSpecies {
            species_id: strong,
            role: EcosystemRole::PrimaryConsumer,
            biomass: Real::from_int(40),
            is_extant: true,
        },
        EcoSpecies {
            species_id: weak,
            role: EcosystemRole::PrimaryConsumer,
            biomass: Real::from_int(5),
            is_extant: true,
        },
    ];
    let mut matrix = InteractionMatrix::new();
    // Both species predate the producer.
    matrix.insert(
        strong,
        prod,
        Interaction {
            kind: InteractionKind::Predation,
            strength: Real::from((3, 100)),
            functional_response: FunctionalResponse::Saturating,
        },
    );
    matrix.insert(
        weak,
        prod,
        Interaction {
            kind: InteractionKind::Predation,
            strength: Real::from((3, 100)),
            functional_response: FunctionalResponse::Saturating,
        },
    );
    // Strong out-competes weak (symmetric competition with the
    // strong side winning because of initial-biomass asymmetry +
    // higher inflicted competition strength).
    matrix.insert(
        strong,
        weak,
        Interaction {
            kind: InteractionKind::Competition,
            strength: Real::from((8, 100)),
            functional_response: FunctionalResponse::Linear,
        },
    );
    matrix.insert(
        weak,
        strong,
        Interaction {
            kind: InteractionKind::Competition,
            strength: Real::from((2, 100)),
            functional_response: FunctionalResponse::Linear,
        },
    );

    let mut eco = PlanetEcosystem::new(species, matrix, Real::from_int(500));

    let weak_initial = eco.species.get(&weak).unwrap().biomass;
    let strong_initial = eco.species.get(&strong).unwrap().biomass;
    for _ in 0..1000 {
        eco.step();
    }
    let weak_final = eco.species.get(&weak).unwrap().biomass;
    let strong_final = eco.species.get(&strong).unwrap().biomass;

    // The weak species should collapse to a small fraction of its
    // starting biomass. The strong one should persist (not have
    // collapsed too).
    assert!(
        weak_final < weak_initial / Real::from_int(2),
        "weak species did not collapse (initial {weak_initial:?} -> final {weak_final:?})",
    );
    assert!(
        strong_final > weak_final,
        "strong competitor did not outlast weak (strong {strong_final:?} vs weak {weak_final:?})",
    );
    let _ = strong_initial;
}

#[test]
fn functional_response_linear_is_identity_in_prey() {
    let k = Real::from_int(10);
    let prey = Real::from_int(7);
    assert_eq!(
        functional_response(FunctionalResponse::Linear, prey, k),
        prey
    );
}

#[test]
fn functional_response_saturating_caps_at_one() {
    let k = Real::from_int(1);
    // prey → ∞: prey/(k+prey) → 1.
    let huge = Real::from_int(1_000_000);
    let r = functional_response(FunctionalResponse::Saturating, huge, k);
    assert!(r > Real::from((99, 100)));
    assert!(r <= Real::ONE);
}

#[test]
fn functional_response_sigmoidal_caps_at_one() {
    // Q32.32 can hold (2^31 - 1), so prey² must stay below that.
    // Pick prey = 1000, k = 1 → 1_000_000 / (1 + 1_000_000) ≈ 0.999.
    let k = Real::from_int(1);
    let big = Real::from_int(1_000);
    let r = functional_response(FunctionalResponse::Sigmoidal, big, k);
    assert!(r > Real::from((99, 100)));
    assert!(r <= Real::ONE);
}

#[test]
fn functional_response_saturating_uses_holling_type_ii() {
    // At prey == k, response = 0.5.
    let k = Real::from_int(4);
    let prey = Real::from_int(4);
    let r = functional_response(FunctionalResponse::Saturating, prey, k);
    assert_eq!(r, Real::from((1, 2)));
}

#[test]
fn functional_response_sigmoidal_uses_holling_type_iii() {
    // At prey == k, response = 0.5.
    let k = Real::from_int(4);
    let prey = Real::from_int(4);
    let r = functional_response(FunctionalResponse::Sigmoidal, prey, k);
    assert_eq!(r, Real::from((1, 2)));
}

#[test]
fn lindeman_pyramid_enforcement_caps_overgrown_consumers() {
    // Hand-construct a state that violates the pyramid: producer
    // biomass 100, primary 50 (should be capped at 10), secondary
    // 30 (should be capped at 0.1× primary after primary is
    // capped).
    let prod = SpeciesId(0);
    let primary = SpeciesId(1);
    let secondary = SpeciesId(2);
    let species = vec![
        EcoSpecies {
            species_id: prod,
            role: EcosystemRole::Producer {
                metabolism: ProducerMetabolism::Photoautotroph,
            },
            biomass: Real::from_int(100),
            is_extant: true,
        },
        EcoSpecies {
            species_id: primary,
            role: EcosystemRole::PrimaryConsumer,
            biomass: Real::from_int(50),
            is_extant: true,
        },
        EcoSpecies {
            species_id: secondary,
            role: EcosystemRole::SecondaryConsumer,
            biomass: Real::from_int(30),
            is_extant: true,
        },
    ];
    let mut eco = PlanetEcosystem::new(
        species,
        InteractionMatrix::new(),
        Real::from_int(100),
    );
    eco.enforce_lindeman_pyramid();
    let p = eco.species.get(&primary).unwrap().biomass;
    let s = eco.species.get(&secondary).unwrap().biomass;
    // Primary now ≤ 10.
    assert!(p <= Real::from_int(10), "primary not capped: {p:?}");
    // Secondary now ≤ 0.1 × primary = ≤ 1.
    assert!(
        s <= Real::from_int(1),
        "secondary not capped: {s:?} (primary={p:?})"
    );
}
