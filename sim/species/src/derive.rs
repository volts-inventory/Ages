//! `derive` — top-level entry point: takes a sampled `Planet` plus
//! the recognition library and produces the run's `Species`.

use crate::sampling::{
    compute_t0_loss, derive_habitat, derive_initial_cosmology, derive_population_biology,
    derive_tolerance_envelope, sample_manipulation, sample_modalities, sample_unit,
    species_name_from_seed, template_channels,
};
use crate::species::Species;
use crate::types::{
    CognitionTopology, EcosystemRole, Lifecycle, ModalityKind, DYNAMIC_TOOL_ID_START,
};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use sim_arith::Real;
use sim_recognition::RecognitionLibrary;
use sim_world::Planet;
use std::collections::{BTreeMap, BTreeSet};

pub fn derive(planet: &Planet, recognition_lib: &RecognitionLibrary) -> Species {
    let name = species_name_from_seed(planet.seed);
    // Distinct RNG stream from the physics seed so species sampling
    // can't entangle with mid-run physics RNG draws.
    let mut rng = ChaCha20Rng::seed_from_u64(planet.seed ^ 0xCAFE_BABE_DEAD_BEEF);

    let cognition = sample_unit(&mut rng);
    let sociality = sample_unit(&mut rng);
    let communication_fidelity = sample_unit(&mut rng);

    // Lifespan placeholder: 5..=200 yr uniform. Refine when this
    // gets a Real-valued planet-temperature mapping.
    let lifespan_years = Real::from_int(rng.gen_range(5..=200));

    let modalities = sample_modalities(planet, &mut rng);
    let manipulation_modes = sample_manipulation(planet, &mut rng);

    let mod_set: BTreeSet<ModalityKind> = modalities.iter().map(|m| m.kind).collect();
    let perceivable_templates: BTreeSet<u32> = recognition_lib
        .templates
        .iter()
        .filter(|t| template_channels(t.id).iter().any(|c| mod_set.contains(c)))
        .map(|t| t.id)
        .collect();

    // Cognition topology — biased toward Centralized (the
    // vertebrate-equivalent default) so most seeds remain familiar;
    // the remaining three substrates surface the cephalopod-,
    // hive-, and slime-mold-archetypes per the project's
    // "different cognition substrate" direction. Distribution:
    // 70% Centralized, 15% DistributedRedundant, 10% Collective,
    // 5% Acentric — preserves Centralized's prior 70% share and
    // lets the rarer substrates surface meaningfully without
    // crowding the modal seed.
    let cognition_topology = {
        let roll = rng.gen_range(0..20);
        if roll < 14 {
            CognitionTopology::Centralized
        } else if roll < 17 {
            CognitionTopology::DistributedRedundant
        } else if roll < 19 {
            CognitionTopology::Collective
        } else {
            CognitionTopology::Acentric
        }
    };

    // DistributedRedundant-cognition behavioural fork. Their
    // distributed nervous systems give them a different
    // relationship to attention and parallel introspection —
    // captured here as a small (+10%) cognition bonus that
    // ripples through tolerance and minimum-sample formulas.
    // Ceiling at 1.0 so the trait stays in [0, 1].
    let cognition = if matches!(cognition_topology, CognitionTopology::DistributedRedundant) {
        let bumped = cognition * Real::percent(110);
        if bumped > Real::ONE {
            Real::ONE
        } else {
            bumped
        }
    } else {
        cognition
    };

    // t0_loss is computed against the original cognition value
    // above; recompute against the bumped cognition so its
    // memory-loss term tracks the actual species capability.
    let t0_loss = compute_t0_loss(cognition, sociality, lifespan_years, communication_fidelity);

    let habitat = derive_habitat(planet, &modalities, &manipulation_modes);
    let initial_cosmology = derive_initial_cosmology(
        cognition,
        sociality,
        communication_fidelity,
        habitat,
        planet,
        &modalities,
    );
    let biology = derive_population_biology(
        cognition,
        sociality,
        lifespan_years,
        habitat,
        cognition_topology,
        &manipulation_modes,
    );
    // Per-species environmental tolerance envelope. Derived from the
    // planet's metabolic substrate (each substrate carries a different
    // "baseline biology" window) with ±20% per-axis jitter from the
    // species seed so individual species end up as distinguishable
    // generalists / extremophiles within the substrate.
    let tolerance = derive_tolerance_envelope(planet.seed, planet.metabolic_substrate);

    // Dormancy capability sample (Sprint 2 Item 7b). Drawn from
    // the same per-seed stream as the other species traits so
    // replay stays bit-identical. The distribution is skewed
    // strongly toward 0 — most species cannot enter cryptobiosis;
    // tardigrade-grade dormancy is rare. We square the unit sample
    // so the median lands near 0.25 and 0.9+ values only appear
    // for the top decile of seeds.
    let raw_dormancy = sample_unit(&mut rng);
    let dormancy_capability = raw_dormancy * raw_dormancy;

    Species {
        seed: planet.seed,
        name,
        cognition,
        // Perturb each axis off the base scalar deterministically
        // from the species seed so downstream consumers that wire
        // to `cognition_axes.working_memory` (or `.abstraction`,
        // or `.social`) see genuinely different per-axis values
        // rather than the bit-identical alias `uniform` produced.
        // The seed is mixed with the planet seed plus a per-axis
        // domain tag inside `from_scalar_with_seed` — no new RNG
        // draw, fully deterministic on `planet.seed`.
        cognition_axes: crate::types::CognitionAxes::from_scalar_with_seed(
            cognition,
            planet.seed,
        ),
        sociality,
        communication_fidelity,
        lifespan_years,
        modalities,
        manipulation_modes,
        perceivable_templates,
        t0_loss,
        cognition_topology,
        habitat,
        // Empty at genesis; civs propose new templates as
        // they accumulate observation pressure across the run.
        discovered_templates: BTreeMap::new(),
        next_discovered_template_id: sim_recognition::DISCOVERED_TEMPLATE_ID_START,
        // Empty at genesis; civs propose dynamic tools when
        // their confirmed-relation clusters cross thresholds.
        dynamic_tool_registry: BTreeMap::new(),
        next_dynamic_tool_id: DYNAMIC_TOOL_ID_START,
        // Per-seed cosmology pole-position bias.
        initial_cosmology,
        biology,
        tolerance,
        lifecycle: Lifecycle::Vertebrate,
        role: EcosystemRole::PrimaryConsumer,
        dormancy_capability,
        // Empty plasmid registry at genesis; populated by HGT
        // trials each tick (P3.3 plasmid-sweep model).
        plasmids: BTreeMap::new(),
        next_plasmid_id: 0,
        // Newly-derived species are alive by default; the
        // extinction rule (Sprint 2 Item 6a) flips this off when
        // the per-species biomass collapses for the confirmation
        // window.
        is_extant: true,
    }
}
