//! `derive` — top-level entry point: takes a sampled `Planet` plus
//! the recognition library and produces the run's `Species`.

use crate::sampling::{
    compute_t0_loss, derive_habitat, derive_initial_cosmology, derive_population_biology,
    sample_manipulation, sample_modalities, sample_unit, species_name_from_seed, template_channels,
};
use crate::species::Species;
use crate::types::{CognitionTopology, ModalityKind, DYNAMIC_TOOL_ID_START};
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
    // the Distributed branch surfaces the cephalopod-archetype
    // species per the project's "different cognition substrate"
    // direction.
    let cognition_topology = if rng.gen_range(0..10) < 7 {
        CognitionTopology::Centralized
    } else {
        CognitionTopology::Distributed
    };

    // Distributed-cognition behavioural fork. Their distributed
    // nervous systems give them a different relationship to
    // attention and parallel introspection — captured here as a
    // small (+10%) cognition bonus that ripples through
    // tolerance and minimum-sample formulas. The sim's existing
    // cognition machinery makes this an emergent biological
    // advantage: Distributed species fit relations slightly faster
    // and tighter, which over thousands of years compounds into
    // earlier tier-5 unlocks and richer scientific traditions —
    // honoring the story's "their biology gives them an
    // introspective edge" without adding a separate behavioural
    // fork branch. Ceiling at 1.0 so the trait stays in [0, 1].
    let cognition = if matches!(cognition_topology, CognitionTopology::Distributed) {
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

    Species {
        seed: planet.seed,
        name,
        cognition,
        cognition_axes: crate::types::CognitionAxes::uniform(cognition),
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
    }
}
