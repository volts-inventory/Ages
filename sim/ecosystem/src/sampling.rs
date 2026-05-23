//! Per-planet ecosystem sampling.
//!
//! Carved out of `lib.rs` in CA2. Holds the seed-driven `sample_*`
//! family that produces a sized + Lindeman-respecting species set,
//! plus the canonical interaction wiring used by every fresh planet.

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use sim_arith::Real;
use sim_physics::chemistry::oxidiser_ladder;
use sim_species::{
    sample_tolerance_for_substrate, EcosystemRole, FunctionalResponse, Habitat, Interaction,
    InteractionKind, InteractionMatrix, MutualismKind, ParasiteKind, ProducerMetabolism,
    SpeciesId,
};

use crate::constants::{
    HALF_SAT_APEX_PREDATOR, HALF_SAT_HABITAT_MOD, HALF_SAT_MUTUALISM,
    HALF_SAT_SPECIALIST_PREDATOR, LINDEMAN_RATIO,
};
use crate::planet::PlanetEcosystem;
use crate::species::EcoSpecies;

/// Sample an 8-20 species ecosystem honoring the role-distribution
/// spec (≥2 Producers, ≥3 PrimaryConsumers, ≥2 SecondaryConsumers,
/// ≥1 ApexConsumer, ≥1 Detritivore, ≥1 Saprotroph, 1-3 Mutualists,
/// 1-5 Parasites). Biomasses are seeded at Lindeman-respecting
/// totals so the first step doesn't need to scrub a violation.
///
/// Determinism: derived solely from `(planet_seed, producer_capacity)`
/// via a dedicated `ChaCha20` stream keyed off the seed.
#[must_use]
pub fn sample_ecosystem(planet_seed: u64, producer_capacity: Real) -> PlanetEcosystem {
    let mut rng = ChaCha20Rng::seed_from_u64(planet_seed ^ 0xEC05_5751_1F00_0BA1);
    let mut species: Vec<EcoSpecies> = Vec::new();
    let mut next_id: u32 = 0;

    // Sample role counts within spec bounds. Producer biomass is
    // split evenly across `n_producers`, primary across `n_primary`,
    // etc., so each tier total is *exact* (producer = capacity,
    // primary = 0.1 × capacity, secondary = 0.01 × capacity, apex =
    // 0.001 × capacity) regardless of how the role count happened
    // to land. Off-pyramid roles (detritivore / saprotroph /
    // mutualist / parasite) get small fixed per-species biomasses.
    let n_producers = rng.gen_range(2..=4);
    let n_primary = rng.gen_range(3..=5);
    let n_secondary = rng.gen_range(2..=3);
    let n_apex = rng.gen_range(1..=2);
    let n_mutualists = rng.gen_range(1..=3);
    let n_parasites = rng.gen_range(1..=5);

    let producer_tier = producer_capacity;
    let primary_tier = producer_capacity * Real::from(LINDEMAN_RATIO);
    let secondary_tier =
        primary_tier * Real::from(LINDEMAN_RATIO);
    let apex_tier = secondary_tier * Real::from(LINDEMAN_RATIO);

    let producer_per = producer_tier / Real::from_int(n_producers);
    let primary_per = primary_tier / Real::from_int(n_primary);
    let secondary_per = secondary_tier / Real::from_int(n_secondary);
    let apex_per = apex_tier / Real::from_int(n_apex);

    // Off-pyramid biomasses (small constants). Detritivores +
    // saprotrophs work on dead matter (not capped by Lindeman);
    // mutualists + parasites depend on a single host species, so a
    // small biomass keeps the network coupled without warping the
    // pyramid.
    let detritivore_per = producer_capacity * Real::from((2, 100));
    let saprotroph_per = producer_capacity * Real::from((1, 100));
    let mutualist_per = producer_capacity * Real::from((1, 100));
    let parasite_per = producer_capacity * Real::from((5, 1000));

    let push = |species: &mut Vec<EcoSpecies>,
                    next_id: &mut u32,
                    role: EcosystemRole,
                    biomass: Real| {
        // F3: derive a per-species tolerance envelope from the planet
        // seed mixed with the dense species index. Legacy callsites
        // default to the Aqueous substrate (matching the legacy
        // habitat = Terrestrial / substrate_tag = "aqueous"
        // back-compat); `sample_ecosystem_with_substrate` rewrites
        // these per-species envelopes against the real substrate
        // after the species list is built. Per-species seed mixing
        // keeps extremophiles + generalists distinguishable within
        // a substrate (same ±20% jitter the civ-side draw uses).
        let species_seed = planet_seed
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(u64::from(*next_id));
        let tolerance = sample_tolerance_for_substrate(species_seed, "aqueous");
        species.push(EcoSpecies {
            species_id: SpeciesId(*next_id),
            role,
            biomass,
            is_extant: true,
            low_biomass_streak: 0,
            // Legacy sampling stream defaults to Terrestrial — the
            // canonical 10:1 Lindeman ratio — so existing fixtures
            // get bit-for-bit identical numerics. The
            // substrate-aware path
            // (`sample_ecosystem_with_substrate`) can override
            // habitat to match the planet's solvent chemistry.
            habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
            tolerance,
        });
        *next_id += 1;
    };

    // Producers — metabolism cycles through the three variants.
    for i in 0..n_producers {
        let metabolism = match i % 3 {
            0 => ProducerMetabolism::Photoautotroph,
            1 => ProducerMetabolism::Chemoautotroph,
            _ => ProducerMetabolism::Mixotroph,
        };
        push(
            &mut species,
            &mut next_id,
            EcosystemRole::Producer { metabolism },
            producer_per,
        );
    }
    for _ in 0..n_primary {
        push(
            &mut species,
            &mut next_id,
            EcosystemRole::PrimaryConsumer,
            primary_per,
        );
    }
    for _ in 0..n_secondary {
        push(
            &mut species,
            &mut next_id,
            EcosystemRole::SecondaryConsumer,
            secondary_per,
        );
    }
    for _ in 0..n_apex {
        push(
            &mut species,
            &mut next_id,
            EcosystemRole::ApexConsumer,
            apex_per,
        );
    }
    push(
        &mut species,
        &mut next_id,
        EcosystemRole::Detritivore,
        detritivore_per,
    );
    push(
        &mut species,
        &mut next_id,
        EcosystemRole::Saprotroph,
        saprotroph_per,
    );
    for i in 0..n_mutualists {
        let kind = match i % 4 {
            0 => MutualismKind::Pollinator,
            1 => MutualismKind::SeedDisperser,
            2 => MutualismKind::Engineer,
            _ => MutualismKind::Generic,
        };
        push(
            &mut species,
            &mut next_id,
            EcosystemRole::Mutualist { kind },
            mutualist_per,
        );
    }
    for i in 0..n_parasites {
        let kind = match i % 3 {
            0 => ParasiteKind::Macro,
            1 => ParasiteKind::Micro,
            _ => ParasiteKind::Virus,
        };
        push(
            &mut species,
            &mut next_id,
            EcosystemRole::Parasite { kind },
            parasite_per,
        );
    }

    // Cap at 20 species total. Max draw is
    // 4+5+3+2+1+1+3+5 = 24 — trim parasites from the tail.
    while species.len() > 20 {
        species.pop();
    }

    let interactions = build_interaction_matrix(&species);

    PlanetEcosystem::new(species, interactions, producer_capacity)
}

/// Same as [`sample_ecosystem`] but lets the caller pin the substrate
/// tag that drives the Chemoautotroph oxidiser-ladder partition. The
/// production callsite in `sim-core::run` derives the tag from
/// `planet.metabolic_substrate.tag()` so the per-planet ecosystem
/// matches the planet's solvent chemistry; tests + back-compat go
/// through the existing `sample_ecosystem` (which pins to `"aqueous"`).
///
/// The seed XOR uses a different discriminator
/// (`0xEC05_0001_5751_1F00`) than the legacy `sample_ecosystem`
/// (`0xEC05_5751_1F00_0BA1`) so the two namespaces don't alias each
/// other — sim-core's production stream gets its own deterministic
/// draw that won't collide with the legacy unit-test stream.
#[must_use]
pub fn sample_ecosystem_with_substrate(
    planet_seed: u64,
    substrate_tag: &'static str,
    producer_capacity: Real,
) -> PlanetEcosystem {
    let inner_seed = planet_seed ^ 0xEC05_0001_5751_1F00;
    let mut eco = sample_ecosystem(inner_seed, producer_capacity);
    eco.substrate_tag = substrate_tag;
    eco.current_oxidisers = oxidiser_ladder(substrate_tag);
    // P2.5: substrate-derived habitat. An aqueous (water-solvent)
    // world is implicitly aquatic — the per-habitat Lindeman
    // assimilation drops to ~3.3% (30:1) so producer-heavy pyramids
    // emerge. Non-aqueous substrates default to Terrestrial; a
    // hydrocarbon-lake species *could* be aquatic too but the
    // calibration data is thinner there, so the conservative default
    // is the 10:1 terrestrial value.
    let habitat = habitat_for_substrate(substrate_tag);
    for s in eco.species.values_mut() {
        s.habitat = habitat;
        // F3: re-derive per-species tolerance against the actual
        // substrate (the inner `sample_ecosystem` defaulted them to
        // Aqueous). Same per-species seed mixing — the `push`
        // closure inside `sample_ecosystem` used
        // `inner_seed.wrapping_mul(GOLDEN) + species_id`; reproduce
        // that here so the substrate-aware path produces the same
        // (seed, species_id) → envelope mapping the legacy path
        // would have produced if it had the substrate at push time.
        let species_seed = inner_seed
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(u64::from(s.species_id.0));
        s.tolerance = sample_tolerance_for_substrate(species_seed, substrate_tag);
    }
    eco
}

/// F2 — same as [`sample_ecosystem_with_substrate`] but pins the
/// per-cell biomass distribution against the planet's grid cell
/// count. Initialises every species' `cell_biomass` via
/// [`PlanetEcosystem::initialise_cell_biomass`] — uniform split
/// when `per_cell_weights` is `None`, biome-class-weighted (T9) when
/// it is `Some`. The aggregate stays identical to the non-grid path
/// — only the per-cell decomposition changes — so existing
/// aggregate-only tests stay bit-for-bit stable.
///
/// T9: callers that have a per-cell habitability vector (e.g.
/// `sim/core` derives one from `sim_world::cell_habitability`) can
/// pass it through `per_cell_weights` so high-habitability cells
/// (lush coast / inland) start with more biomass than peaks or
/// deserts. The weighted path preserves the `sum(cell_biomass) ==
/// aggregate` invariant.
#[must_use]
pub fn sample_ecosystem_with_substrate_for_grid(
    planet_seed: u64,
    substrate_tag: &'static str,
    producer_capacity: Real,
    n_cells: usize,
    per_cell_weights: Option<&[Real]>,
) -> PlanetEcosystem {
    let mut eco = sample_ecosystem_with_substrate(planet_seed, substrate_tag, producer_capacity);
    eco.initialise_cell_biomass(n_cells, per_cell_weights);
    eco
}

/// Derive the dominant per-species habitat from a planet's solvent
/// substrate tag (P2.5). Used by
/// [`sample_ecosystem_with_substrate`] to pin the per-habitat
/// Lindeman assimilation ratio without requiring callers to set it
/// per-species.
///
/// Mapping:
/// - `"aqueous"` → `Aquatic` (30:1 Lindeman ratio — water-solvent
///   life is overwhelmingly fish-equivalent for trophic
///   accounting).
/// - All other substrates (`"ammoniacal"`, `"hydrocarbon"`,
///   `"silicate"`, fallback) → `Terrestrial` (10:1). The
///   canonical default; off-Earth substrates that *might* warrant a
///   different ratio aren't calibrated well enough to pin
///   confidently.
#[must_use]
pub fn habitat_for_substrate(substrate_tag: &str) -> Habitat {
    match substrate_tag {
        "aqueous" => Habitat::Aquatic,
        _ => Habitat::Terrestrial,
    }
}

/// Build a canonical interaction matrix from a sampled species list.
///
/// Wiring rules:
/// - Every primary consumer preys on every producer (Saturating).
/// - Every secondary consumer preys on every primary consumer
///   (Saturating).
/// - Every apex consumer preys on every secondary consumer
///   (Saturating).
/// - Same-tier consumers compete (Competition, symmetric).
/// - Mutualists pair with the first Producer (Mutualism, symmetric).
/// - Parasites prey on the first PrimaryConsumer host (Parasitism).
/// - Detritivore + Saprotroph have HabitatModification edges to all
///   producers (they enable the recycling loop).
fn insert_competition(matrix: &mut InteractionMatrix, ids: &[SpeciesId], strength: Real) {
    // Competition uses a Linear functional response so the
    // half-saturation value never enters the per-tick math; carry the
    // neutral default (0.5) for forward compatibility.
    let half_saturation = Real::from(HALF_SAT_MUTUALISM);
    for (i, a) in ids.iter().enumerate() {
        for b in &ids[i + 1..] {
            matrix.insert(
                *a,
                *b,
                Interaction {
                    kind: InteractionKind::Competition,
                    strength,
                    functional_response: FunctionalResponse::Linear,
                    half_saturation,
                },
            );
            matrix.insert(
                *b,
                *a,
                Interaction {
                    kind: InteractionKind::Competition,
                    strength,
                    functional_response: FunctionalResponse::Linear,
                    half_saturation,
                },
            );
        }
    }
}

fn build_interaction_matrix(species: &[EcoSpecies]) -> InteractionMatrix {
    let mut matrix = InteractionMatrix::new();

    let producers: Vec<_> = species
        .iter()
        .filter(|s| matches!(s.role, EcosystemRole::Producer { .. }))
        .map(|s| s.species_id)
        .collect();
    let primary: Vec<_> = species
        .iter()
        .filter(|s| matches!(s.role, EcosystemRole::PrimaryConsumer))
        .map(|s| s.species_id)
        .collect();
    let secondary: Vec<_> = species
        .iter()
        .filter(|s| matches!(s.role, EcosystemRole::SecondaryConsumer))
        .map(|s| s.species_id)
        .collect();
    let apex: Vec<_> = species
        .iter()
        .filter(|s| matches!(s.role, EcosystemRole::ApexConsumer))
        .map(|s| s.species_id)
        .collect();
    let detritivores: Vec<_> = species
        .iter()
        .filter(|s| matches!(s.role, EcosystemRole::Detritivore))
        .map(|s| s.species_id)
        .collect();
    let saprotrophs: Vec<_> = species
        .iter()
        .filter(|s| matches!(s.role, EcosystemRole::Saprotroph))
        .map(|s| s.species_id)
        .collect();
    let mutualists: Vec<_> = species
        .iter()
        .filter(|s| matches!(s.role, EcosystemRole::Mutualist { .. }))
        .map(|s| s.species_id)
        .collect();
    let parasites: Vec<_> = species
        .iter()
        .filter(|s| matches!(s.role, EcosystemRole::Parasite { .. }))
        .map(|s| s.species_id)
        .collect();

    let predation_strength = Real::from((2, 100));
    let competition_strength = Real::from((1, 100));
    let mutualism_strength = Real::from((1, 100));
    let parasite_strength = Real::from((1, 100));
    let habmod_strength = Real::from((1, 100));

    // Per-pair half-saturation calibration (Sprint 2 Item P2.6).
    // Specialist predators (primary → producer, secondary → primary)
    // get the lynx-hare 0.30 — small predators saturate slowly. Apex
    // predators get the wolf-deer 0.10 — large apex predators
    // saturate fast on big prey items. Parasites inherit the
    // specialist baseline because micro-/macro-parasites depend on
    // host availability rather than apex-style satiation. Mutualism +
    // engineering effects get their own per-kind calibration.
    let half_sat_specialist = Real::from(HALF_SAT_SPECIALIST_PREDATOR);
    let half_sat_apex = Real::from(HALF_SAT_APEX_PREDATOR);
    let half_sat_mutualism = Real::from(HALF_SAT_MUTUALISM);
    let half_sat_habmod = Real::from(HALF_SAT_HABITAT_MOD);

    // Predation up the tier ladder.
    for c in &primary {
        for p in &producers {
            matrix.insert(
                *c,
                *p,
                Interaction {
                    kind: InteractionKind::Predation,
                    strength: predation_strength,
                    functional_response: FunctionalResponse::Saturating,
                    half_saturation: half_sat_specialist,
                },
            );
        }
    }
    for c in &secondary {
        for p in &primary {
            matrix.insert(
                *c,
                *p,
                Interaction {
                    kind: InteractionKind::Predation,
                    strength: predation_strength,
                    functional_response: FunctionalResponse::Saturating,
                    half_saturation: half_sat_specialist,
                },
            );
        }
    }
    for a in &apex {
        for s in &secondary {
            matrix.insert(
                *a,
                *s,
                Interaction {
                    kind: InteractionKind::Predation,
                    strength: predation_strength,
                    functional_response: FunctionalResponse::Saturating,
                    half_saturation: half_sat_apex,
                },
            );
        }
    }

    // Same-tier competition (symmetric: store both directions).
    insert_competition(&mut matrix, &primary, competition_strength);
    insert_competition(&mut matrix, &secondary, competition_strength);
    insert_competition(&mut matrix, &apex, competition_strength);

    // Mutualism (symmetric) with the first producer.
    if let Some(host) = producers.first() {
        for m in &mutualists {
            matrix.insert(
                *m,
                *host,
                Interaction {
                    kind: InteractionKind::Mutualism,
                    strength: mutualism_strength,
                    functional_response: FunctionalResponse::Saturating,
                    half_saturation: half_sat_mutualism,
                },
            );
            matrix.insert(
                *host,
                *m,
                Interaction {
                    kind: InteractionKind::Mutualism,
                    strength: mutualism_strength,
                    functional_response: FunctionalResponse::Saturating,
                    half_saturation: half_sat_mutualism,
                },
            );
        }
    }

    // Parasites on the first primary host.
    if let Some(host) = primary.first() {
        for p in &parasites {
            matrix.insert(
                *p,
                *host,
                Interaction {
                    kind: InteractionKind::Parasitism,
                    strength: parasite_strength,
                    functional_response: FunctionalResponse::Saturating,
                    half_saturation: half_sat_specialist,
                },
            );
        }
    }

    // Detritivore + Saprotroph engineering effects.
    for d in detritivores.iter().chain(saprotrophs.iter()) {
        for p in &producers {
            matrix.insert(
                *d,
                *p,
                Interaction {
                    kind: InteractionKind::HabitatModification,
                    strength: habmod_strength,
                    functional_response: FunctionalResponse::Linear,
                    half_saturation: half_sat_habmod,
                },
            );
        }
    }

    matrix
}
