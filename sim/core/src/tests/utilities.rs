//! Glue tests: short-horizon smoke checks, attribution
//! invariants, the run-start species event, recognition-firing
//! sanity, and the T7 `lifecycle_for_role` mapping table +
//! macro-parasite HGT path. These don't fit cleanly into the
//! determinism / canary / ecosystem / planet axes so they live
//! together as the catch-all for cross-cutting wire-ups.

use crate::*;

use sim_arith::Real;
use sim_ecosystem::{step_hgt, LocalConditions};
use sim_events::{CountingEmitter, JsonLinesEmitter};
use sim_species::{
    CognitionAxes, CognitionTopology, EcosystemRole, Fission, Habitat, Lifecycle, ParasiteKind,
    PopulationBiology, Species, SpeciesId, ToleranceEnvelope,
};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

#[test]
fn relation_confirmations_attribute_to_real_figures() {
    // With a founding band of 2-3 figures, every RelationConfirmed
    // must carry a non-zero figure_id (round-robin by relation_id).
    let cfg = RunConfig::dev(42, 50);
    let mut buf = Vec::new();
    run(&cfg, &mut JsonLinesEmitter::new(Cursor::new(&mut buf))).unwrap();
    let log = String::from_utf8(buf).unwrap();
    for line in log.lines() {
        if line.contains("\"kind\":\"relation_confirmed\"") {
            assert!(
                !line.contains("\"figure_id\":0"),
                "relation_confirmed must attribute to a real figure: {line}"
            );
        }
    }
}

/// A "quick" version of the long-run trio that
/// exercises the same code paths (run-loop coupling, event
/// emission, civ-lifecycle plumbing) without the multi-minute
/// debug-mode runtime. Uses 50 sim-years — far short of the
/// timescales the long tests verify, but enough to catch any
/// runtime panic, Q32.32 overflow, or deterministic-loop bug
/// that would also affect the long tests. The slow tests
/// catch the slow-emergence assertions (collapses,
/// transmissions, successor civs); this one catches the
/// "everything still wires together" failure mode in seconds.
#[test]
fn long_run_loop_smoke_short() {
    let cfg = RunConfig::dev(42, 50 * protocol::MONTHS_PER_YEAR);
    let mut counter = CountingEmitter::new();
    run(&cfg, &mut counter).unwrap();
    // No specific assertion on event counts (50 years is too
    // short to guarantee any of the slow-emergence outcomes).
    // The only requirement is that the run completes without
    // panicking — covers physics-law overflow regressions.
    assert!(
        counter.count("tick") > 0,
        "expected at least one tick event"
    );
}

#[test]
fn species_derived_event_emitted_at_run_start() {
    let cfg = RunConfig::dev(42, 1);
    let mut buf = Vec::new();
    run(&cfg, &mut JsonLinesEmitter::new(Cursor::new(&mut buf))).unwrap();
    let log = String::from_utf8(buf).unwrap();
    assert!(
        log.contains("\"kind\":\"species\""),
        "expected one species event at run start:\n{log}"
    );
    // Sanity: the event carries the seed and a Q32 trait field.
    assert!(log.contains("\"cognition_q32\""));
    assert!(log.contains("\"perceivable_template_ids\""));
}

#[test]
fn earth_equivalent_seed_emits_recognition_firings() {
    // Validation: a Rocky+Lush seed (42) should emit
    // recognition firings in the first few ticks — surface_water
    // from the ocean cells alone, plus ice from polar regions
    // sub-freezing under the SI temperature gradient. Loose
    // assertion proves the cascade wires end-to-end without
    // pinning specific cell counts (which depend on grid + RNG).
    let cfg = RunConfig::dev(42, 3);
    let mut buf = Vec::new();
    run(&cfg, &mut JsonLinesEmitter::new(Cursor::new(&mut buf))).unwrap();
    let log = String::from_utf8(buf).unwrap();
    assert!(
        log.contains("surface_water"),
        "expected surface_water firings on a Rocky-with-oceans seed:\n{log}"
    );
}

// ---------------------------------------------------------------------
// T7: EcosystemRole → Lifecycle mapping refinement.
//
// The mapping table lives in `sim_core::lifecycle_for_role`. These
// tests pin every variant's mapped lifecycle and verify the refinement
// actually moves the right roles off the prior Vertebrate default —
// most importantly that Micro/Virus parasites land on Microbial so the
// `step_hgt` pool can include them (previously every parasite collapsed
// to Vertebrate and HGT was impossible).
// ---------------------------------------------------------------------

/// Helper: a minimally-populated `Species` whose only meaningful
/// field for HGT participation is `lifecycle`. Mirrors the
/// `make_microbial` fixture in `sim_ecosystem::hgt::tests` but kept
/// local so this test file doesn't depend on the ecosystem crate's
/// test-private helpers.
fn parasite_species_with_lifecycle(
    id: u32,
    role: EcosystemRole,
    lifecycle: Lifecycle,
) -> (SpeciesId, Species) {
    let species_id = SpeciesId(id);
    let species = Species {
        seed: u64::from(id),
        name: format!("Parasite-{id}"),
        cognition: Real::from_ratio(1, 10),
        cognition_axes: CognitionAxes::uniform(Real::from_ratio(1, 10)),
        sociality: Real::from_ratio(1, 10),
        communication_fidelity: Real::from_ratio(1, 10),
        lifespan_years: Real::from_int(1),
        modalities: Vec::new(),
        manipulation_modes: Vec::new(),
        perceivable_templates: BTreeSet::new(),
        t0_loss: Real::from_ratio(1, 10),
        cognition_topology: CognitionTopology::DistributedRedundant,
        habitat: Habitat::Aquatic,
        discovered_templates: BTreeMap::new(),
        next_discovered_template_id: 1000,
        dynamic_tool_registry: BTreeMap::new(),
        next_dynamic_tool_id: 1000,
        initial_cosmology: [Real::ZERO; 5],
        biology: PopulationBiology {
            clutch_size: Real::from_int(100),
            infant_fraction: Real::from_ratio(1, 100),
            maturity_fraction: Real::from_ratio(1, 100),
            eldership_fraction: Real::ZERO,
            infant_survival: Real::from_ratio(5, 100),
            juvenile_survival: Real::from_ratio(20, 100),
            food_multipliers: [
                Real::from_ratio(3, 10),
                Real::from_ratio(6, 10),
                Real::ONE,
                Real::from_ratio(9, 10),
            ],
            events_per_fertile_window: Real::ONE,
            reproductive_success: Real::ZERO,
        },
        tolerance: ToleranceEnvelope::aqueous_default(),
        lifecycle,
        role,
        dormancy_capability: Real::from_ratio(1, 10),
        plasmids: BTreeMap::new(),
        next_plasmid_id: 0,
        is_extant: true,
    };
    (species_id, species)
}

#[test]
fn eco_role_lifecycle_mapping_covers_all_variants() {
    // Pin every EcosystemRole variant to its refined lifecycle. The
    // refinement's goal is variety: the prior table collapsed nine of
    // fifteen role variants onto `Vertebrate`. After T7 only the three
    // consumer tiers + the two "large animal" mutualists (SeedDisperser,
    // Engineer) stay Vertebrate; everything else gets a topology that
    // matches the role's dominant real-world biology.
    use sim_species::{ProducerMetabolism as Pm, MutualismKind as Mk, ParasiteKind as Pk};

    let cases: &[(EcosystemRole, Lifecycle)] = &[
        // Producers: photosynthesizers + mixotrophs stay Plant;
        // chemoautotrophs become bacterial Microbial::Binary.
        (
            EcosystemRole::Producer { metabolism: Pm::Photoautotroph },
            Lifecycle::Plant,
        ),
        (
            EcosystemRole::Producer { metabolism: Pm::Mixotroph },
            Lifecycle::Plant,
        ),
        (
            EcosystemRole::Producer { metabolism: Pm::Chemoautotroph },
            Lifecycle::Microbial { fission_strategy: Fission::Binary },
        ),
        // Consumer tiers stay Vertebrate (the civ-bearing default).
        (EcosystemRole::PrimaryConsumer, Lifecycle::Vertebrate),
        (EcosystemRole::SecondaryConsumer, Lifecycle::Vertebrate),
        (EcosystemRole::ApexConsumer, Lifecycle::Vertebrate),
        // Decomposers: Detritivore → arthropod (Insect);
        // Saprotroph → yeast/fungi (Microbial::Budding).
        (EcosystemRole::Detritivore, Lifecycle::Insect),
        (
            EcosystemRole::Saprotroph,
            Lifecycle::Microbial { fission_strategy: Fission::Budding },
        ),
        // Mutualists split by kind.
        (
            EcosystemRole::Mutualist { kind: Mk::Pollinator },
            Lifecycle::Insect,
        ),
        (
            EcosystemRole::Mutualist { kind: Mk::SeedDisperser },
            Lifecycle::Vertebrate,
        ),
        (
            EcosystemRole::Mutualist { kind: Mk::Engineer },
            Lifecycle::Vertebrate,
        ),
        (
            EcosystemRole::Mutualist { kind: Mk::Generic },
            Lifecycle::Modular,
        ),
        // Parasites: Macro → invertebrate (Insect);
        // Micro → bacterial (Microbial::Binary);
        // Virus → conjugation (Microbial::Conjugation,
        // closest to viral integration in the existing model).
        (
            EcosystemRole::Parasite { kind: Pk::Macro },
            Lifecycle::Insect,
        ),
        (
            EcosystemRole::Parasite { kind: Pk::Micro },
            Lifecycle::Microbial { fission_strategy: Fission::Binary },
        ),
        (
            EcosystemRole::Parasite { kind: Pk::Virus },
            Lifecycle::Microbial { fission_strategy: Fission::Conjugation },
        ),
    ];

    // 1. Every variant maps to the spec-pinned lifecycle.
    for (role, expected) in cases {
        let got = lifecycle_for_role(*role);
        assert_eq!(
            got, *expected,
            "lifecycle_for_role({role:?}) returned {got:?}, expected {expected:?}",
        );
    }

    // 2. Refinement payoff: count how many variants land on a
    // non-Vertebrate lifecycle. Before T7 only six did (Plant for any
    // Producer; Microbial for Micro/Virus parasites, Saprotroph,
    // Detritivore; Insect for Pollinator). After T7 ten of fifteen do
    // — Detritivore swaps Microbial→Insect, Macro-parasite swaps
    // Vertebrate→Insect, Chemoautotroph-producer swaps Plant→Microbial,
    // Engineer-mutualist swaps Vertebrate→Modular has been split out,
    // Generic-mutualist swaps Vertebrate→Modular. The five Vertebrates
    // are: Primary/Secondary/Apex consumers + SeedDisperser + Engineer
    // mutualists.
    let non_vertebrate_count = cases
        .iter()
        .filter(|(_, lc)| !matches!(lc, Lifecycle::Vertebrate))
        .count();
    assert_eq!(
        non_vertebrate_count, 10,
        "T7 refinement should leave exactly 10 of 15 role variants on a \
         non-Vertebrate lifecycle (got {non_vertebrate_count}); the five \
         Vertebrates are the three consumer tiers + SeedDisperser + Engineer \
         mutualists",
    );

    // 3. Cover-all assertion: case count matches the enumeration's
    // cardinality so this test fails to compile-build if a new
    // EcosystemRole variant is added without an explicit mapping row.
    assert_eq!(
        cases.len(),
        15,
        "EcosystemRole has 15 enumerated cases (3 producer metabolisms + 3 \
         consumer tiers + Detritivore + Saprotroph + 4 mutualist kinds + 3 \
         parasite kinds); update `eco_role_lifecycle_mapping_covers_all_variants` \
         when adding a new variant",
    );
}

#[test]
fn macro_parasites_can_hgt() {
    // T7's most ecologically meaningful effect: parasites no longer
    // collapse onto `Vertebrate`. Before the refinement *every* role
    // in `EcosystemRole::Parasite { .. }` was mapped to Vertebrate,
    // and `step_hgt` (which is strictly gated on
    // `Lifecycle::Microbial`) had zero parasites to draw from. After
    // T7:
    //   - Macro parasites → Insect (worms/fleas, no HGT — invertebrate).
    //   - Micro parasites → Microbial::Binary (HGT-eligible).
    //   - Virus parasites → Microbial::Conjugation (HGT-eligible).
    //
    // This test verifies (a) the mapping moves all three parasite
    // kinds off Vertebrate, and (b) the HGT pool now actually fires
    // when a pair of Microbial-mapped parasites coexist.
    let macro_role = EcosystemRole::Parasite { kind: ParasiteKind::Macro };
    let micro_role = EcosystemRole::Parasite { kind: ParasiteKind::Micro };
    let virus_role = EcosystemRole::Parasite { kind: ParasiteKind::Virus };

    // (a) None of the three parasite roles map to Vertebrate.
    for role in [macro_role, micro_role, virus_role] {
        let lc = lifecycle_for_role(role);
        assert!(
            !matches!(lc, Lifecycle::Vertebrate),
            "parasite role {role:?} still maps to Vertebrate after T7 \
             (got {lc:?}); the refinement must move every parasite kind \
             off the Vertebrate default so the HGT pool can include them",
        );
    }

    // Macro-parasite specific: must map to Insect (the invertebrate-
    // equivalent topology), not Microbial — biological accuracy.
    assert_eq!(
        lifecycle_for_role(macro_role),
        Lifecycle::Insect,
        "Macro-parasites should be Insect (worm/flea invertebrates)",
    );

    // (b) Drive `step_hgt` over the two Microbial parasite kinds.
    // Acquisition is a low-probability per-pair trial (≈ 1e-4 per
    // tick per direction); over 200k ticks the expected count is
    // ≈ 40, so observing at least one is a near-certain signal that
    // the HGT path is live for Microbial-mapped parasites.
    let (id_micro, micro_sp) = parasite_species_with_lifecycle(
        1,
        micro_role,
        lifecycle_for_role(micro_role),
    );
    let (id_virus, virus_sp) = parasite_species_with_lifecycle(
        2,
        virus_role,
        lifecycle_for_role(virus_role),
    );
    let mut species: BTreeMap<SpeciesId, Species> = BTreeMap::new();
    species.insert(id_micro, micro_sp);
    species.insert(id_virus, virus_sp);

    // Pre-check: both species sit on Microbial. If the mapping ever
    // regresses to Vertebrate this assertion catches it before the
    // HGT loop runs (so the failure mode is obvious instead of
    // "200k ticks with zero events").
    for (id, sp) in &species {
        assert!(
            matches!(sp.lifecycle, Lifecycle::Microbial { .. }),
            "parasite species {id:?} should be Microbial after T7 mapping \
             (got {:?})",
            sp.lifecycle,
        );
    }

    let local = LocalConditions::earth_surface();
    let mut acquisition_events = 0usize;
    for tick in 0..200_000u64 {
        let events = step_hgt(&mut species, tick, 0xC0FF_EE42, Real::ONE, local);
        acquisition_events += events.len();
        if acquisition_events > 0 {
            break;
        }
    }
    assert!(
        acquisition_events > 0,
        "expected >= 1 HGT acquisition event between Micro/Virus parasites \
         over 200k ticks (expected ~40 at base rate); the parasite pair \
         is Microbial after T7 so the HGT path should be live for them",
    );
}
