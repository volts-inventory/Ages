//! Run-setup helpers used by `run()` once per run start (or at
//! per-tick boundaries that don't fit cleanly inside the main tick
//! loop): emitting the run-start `SpeciesNomadsChanged`, the
//! per-civ `SpeciesDrift` snapshot, and computing the per-run
//! `PlanetContext` that calibrates recognition relative-to-this-
//! planet.
//!
//! `setup_run` performs the full pre-tick-loop assembly:
//! samples the planet, initialises physics + ecosystem state,
//! builds laws/recognition/species, populates the species
//! registry, seeds the nomadic pools, and constructs a
//! `RunState` for the tick loop to advance.
//! `lifecycle_for_role` is the per-role → `Lifecycle` mapping
//! used when stamping `EcoSpecies` entries into the per-species
//! registry — kept alongside the run setup since both are part of
//! the species-registry initialisation.

use crate::events::{planet_to_event, species_to_event};
use crate::laws::{build_laws, ignition_threshold_for};
use crate::nomads;
use crate::run_tick::RunState;
use crate::RunConfig;
use protocol::{Event, RunHeader, SpeciesDrift, SpeciesNomadsChanged, SCHEMA_VERSION};
use sim_arith::Real;
use sim_civ::Civ;
use sim_ecosystem::{PlanetEcosystem, SpeciationTracker};
use sim_events::Emitter;
use sim_physics::{HexGrid, OrchestratorState, PhysicsState};
use sim_recognition::RecognitionLibrary;
use sim_species::{
    EcosystemRole, Fission, Lifecycle, ModalityKind, MutualismKind, ParasiteKind,
    ProducerMetabolism, SpeciesId,
};
use sim_world::{
    init_planet, sample_planet_with_overrides, Atmosphere, Composition, Magnetosphere,
};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn emit_nomads_changed<E: Emitter>(
    emitter: &mut E,
    tick: u64,
    pops: &BTreeMap<u32, sim_arith::Real>,
) -> Result<(), E::Error> {
    let mut cells: Vec<u32> = pops.keys().copied().collect();
    cells.sort_unstable();
    let pop_q32: Vec<i64> = cells
        .iter()
        .map(|c| {
            pops.get(c)
                .copied()
                .unwrap_or(sim_arith::Real::ZERO)
                .raw()
                .to_bits()
        })
        .collect();
    emitter.emit(&Event::SpeciesNomadsChanged(SpeciesNomadsChanged {
        tick,
        cells,
        population_q32: pop_q32,
    }))
}

/// Emit a `SpeciesDrift` event when a civ's inherited drift
/// crosses the half-step threshold on at least one channel. No-op
/// for inaugural civs (zero drift) and breakaway civs that happen
/// to roll a near-zero step. Called from the three founding sites
/// in sim/core right after the corresponding `CivFounded` emit so
/// the drift snapshot lands in tick order with its civ.
pub(crate) fn emit_species_drift_if_meaningful<E: Emitter>(
    emitter: &mut E,
    civ: &Civ,
) -> Result<(), E::Error> {
    if !civ.has_meaningful_drift() {
        return Ok(());
    }
    emitter.emit(&Event::SpeciesDrift(SpeciesDrift {
        tick: civ.founded_tick,
        civ_id: civ.id,
        parent_civ_id: civ.parent_civ_id,
        cognition_delta_q32: civ.cognition_delta.raw().to_bits(),
        sociality_delta_q32: civ.sociality_delta.raw().to_bits(),
        lifespan_delta_years_q32: civ.lifespan_delta_years.raw().to_bits(),
        communication_fidelity_delta_q32: civ.communication_fidelity_delta.raw().to_bits(),
    }))
}

/// Build the per-run `PlanetContext` consumed by recognition
/// scans. Carries the planet-derived calibration so climate-relative
/// signatures and `AboveIgnition` fire relative to *this planet*.
///
/// The *mean* and *gradient* come from the actual post-init cell-
/// temperature distribution rather than `planet.mean_temperature`
/// directly: per-cell imprinting (e.g. `GaseousShell` pinning
/// cells to a 700 K deep-atmosphere column independently of the
/// planet's stated 232 K cloud-top metadata) means the planet's
/// stated mean isn't always representative of the cells civs
/// actually observe. Reading the post-init state guarantees the
/// climate bands match what's on the grid.
pub(crate) fn build_planet_context(
    planet: &sim_world::Planet,
    state: &sim_physics::PhysicsState,
) -> sim_recognition::PlanetContext {
    let temps = state.temperature();
    let n = temps.len().max(1);
    let mut sum = sim_arith::Real::ZERO;
    let mut tmin = temps[0];
    let mut tmax = temps[0];
    for t in temps {
        sum = sum + *t;
        if *t < tmin {
            tmin = *t;
        }
        if *t > tmax {
            tmax = *t;
        }
    }
    let mean = sum / sim_arith::Real::from_int(i64::try_from(n).unwrap_or(1));
    // Gradient defined as max - min across the planet — the actual
    // span the cells span, regardless of how the planet metadata
    // labelled the equator-pole spread.
    let gradient = (tmax - tmin).max(sim_arith::Real::ZERO);
    sim_recognition::PlanetContext {
        mean_temperature: mean,
        temperature_gradient: gradient,
        ignition_threshold: ignition_threshold_for(planet.atmosphere),
        orbital_period_months: planet.orbital_period_months,
        is_tidally_locked: planet.is_tidally_locked(),
    }
}

/// Map an `EcosystemRole` to the `Lifecycle` topology used when
/// stamping the role's `EcoSpecies` into the per-species registry.
///
/// The mapping is the F1 refinement that replaces the original
/// coarse "everything that isn't Producer/Microbial → Vertebrate"
/// lookup. Each role is mapped to the topology that best matches
/// the dominant real-world biology for that ecological niche:
///
/// | Role                                     | Lifecycle                       | Rationale                          |
/// |------------------------------------------|---------------------------------|------------------------------------|
/// | `Producer { Photoautotroph }`            | `Plant`                         | Land/aquatic plants, algae.        |
/// | `Producer { Chemoautotroph }`            | `Microbial { Binary }`          | Bacterial chemolithotrophs.        |
/// | `Producer { Mixotroph }`                 | `Plant`                         | Plant-dominant trait set.          |
/// | `PrimaryConsumer`                        | `Vertebrate`                    | Herbivore tetrapods.               |
/// | `SecondaryConsumer`                      | `Vertebrate`                    | Mid-tier carnivores.               |
/// | `ApexConsumer`                           | `Vertebrate`                    | Top predators.                     |
/// | `Detritivore`                            | `Insect`                        | Decomposer arthropods.             |
/// | `Saprotroph`                             | `Microbial { Budding }`         | Yeast / fungal-like.               |
/// | `Mutualist { Pollinator }`               | `Insect`                        | Bees, butterflies.                 |
/// | `Mutualist { SeedDisperser }`            | `Vertebrate`                    | Birds, mammals.                    |
/// | `Mutualist { Engineer }`                 | `Vertebrate`                    | Beavers, corals.                   |
/// | `Mutualist { Generic }`                  | `Modular`                       | Colonial / coral-equivalent.       |
/// | `Parasite { Macro }`                     | `Insect`                        | Worms, fleas — invertebrate.       |
/// | `Parasite { Micro }`                     | `Microbial { Binary }`          | Protozoa, bacteria.                |
/// | `Parasite { Virus }`                     | `Microbial { Conjugation }`     | Closest to viral integration/HGT.  |
///
/// Micro/Virus parasites land on `Microbial`, so they participate
/// in the `step_hgt` pool (which is gated on `Lifecycle::Microbial`).
/// Macro parasites + Pollinator/Detritivore land on `Insect`, which
/// routes through the insect step function rather than the default
/// vertebrate cohort. The non-Vertebrate routing is the refinement's
/// main payoff: it gives ecosystem composition real lifecycle
/// variety instead of collapsing nine of fifteen role variants onto
/// the same `Vertebrate` default.
#[must_use]
pub fn lifecycle_for_role(role: EcosystemRole) -> Lifecycle {
    match role {
        EcosystemRole::Producer {
            metabolism: ProducerMetabolism::Photoautotroph | ProducerMetabolism::Mixotroph,
        } => Lifecycle::Plant,
        EcosystemRole::Producer {
            metabolism: ProducerMetabolism::Chemoautotroph,
        } => Lifecycle::Microbial {
            fission_strategy: Fission::Binary,
        },
        EcosystemRole::Parasite {
            kind: ParasiteKind::Macro,
        } => Lifecycle::Insect,
        EcosystemRole::Parasite {
            kind: ParasiteKind::Micro,
        } => Lifecycle::Microbial {
            fission_strategy: Fission::Binary,
        },
        EcosystemRole::Parasite {
            kind: ParasiteKind::Virus,
        } => Lifecycle::Microbial {
            fission_strategy: Fission::Conjugation,
        },
        EcosystemRole::Saprotroph => Lifecycle::Microbial {
            fission_strategy: Fission::Budding,
        },
        EcosystemRole::Detritivore => Lifecycle::Insect,
        EcosystemRole::Mutualist {
            kind: MutualismKind::Pollinator,
        } => Lifecycle::Insect,
        EcosystemRole::Mutualist {
            kind: MutualismKind::SeedDisperser | MutualismKind::Engineer,
        } => Lifecycle::Vertebrate,
        EcosystemRole::Mutualist {
            kind: MutualismKind::Generic,
        } => Lifecycle::Modular,
        EcosystemRole::PrimaryConsumer
        | EcosystemRole::SecondaryConsumer
        | EcosystemRole::ApexConsumer => Lifecycle::Vertebrate,
    }
}

/// Build the full pre-tick-loop run state. Emits all the
/// run-start events (`RunStart`, `RunMetadata`, `Planet`, `PlanetMap`,
/// `Species`, `SpeciesCosmologyBias`, the initial nomads-changed)
/// then returns the `RunState` for the tick loop to advance.
///
/// Behaviour is identical to the original inline setup section
/// of `run()`; this is purely a relocation of locals into a struct.
pub(crate) fn setup_run<E: Emitter>(
    cfg: &RunConfig,
    emitter: &mut E,
) -> Result<RunState, E::Error> {
    emitter.emit(&Event::RunStart(RunHeader {
        schema_version: SCHEMA_VERSION,
        seed: cfg.seed,
        ages_version: env!("CARGO_PKG_VERSION").to_string(),
    }))?;

    // Presentation metadata — substrate freeze/boil ranges
    // (sourced from `sim_physics::chemistry::substrate_phase_thresholds`)
    // and the label tables (sourced from `sim_report::labels::build_run_metadata`).
    let metadata = sim_report::labels::build_run_metadata(|substrate| {
        let (freeze, boil) = sim_physics::chemistry::substrate_phase_thresholds(substrate);
        (freeze.to_f64_for_display(), boil.to_f64_for_display())
    });
    emitter.emit(&Event::RunMetadata(metadata))?;

    let grid = HexGrid::new(cfg.grid_width, cfg.grid_height);
    let mut state = PhysicsState::new(grid);
    let orch_state = OrchestratorState::new();
    let mut planet = sample_planet_with_overrides(cfg.seed, &cfg.planet_overrides);
    emitter.emit(&Event::Planet(planet_to_event(&planet)))?;
    init_planet(&mut state, &planet);

    // P0.1: multi-species ecosystem.
    let substrate_tag: &'static str = planet.metabolic_substrate.tag();
    let planet_capacity: Real = {
        let n_cells_real = Real::from_int(state.grid().n_cells() as i64);
        let cap = n_cells_real * planet.biosphere_density;
        if cap < Real::ONE {
            Real::ONE
        } else {
            cap
        }
    };
    let n_cells = state.grid().n_cells();
    let habitability_weights: Vec<Real> = (0..n_cells as u32)
        .map(|c| sim_world::cell_habitability(&state, &planet, c))
        .collect();
    let ecosystem: PlanetEcosystem = sim_ecosystem::sample_ecosystem_with_substrate_for_grid(
        planet.seed,
        substrate_tag,
        planet_capacity,
        n_cells,
        Some(&habitability_weights),
    );
    let mut species_registry: BTreeMap<SpeciesId, sim_species::Species> = BTreeMap::new();
    let speciation_tracker = SpeciationTracker::new();

    // PlanetMap — per-cell elevation + water_depth in row-major order.
    let elevation_q32: Vec<i64> = state
        .elevation()
        .iter()
        .map(|r| r.raw().to_bits())
        .collect();
    let water_depth_q32: Vec<i64> = state
        .water_depth()
        .iter()
        .map(|r| r.raw().to_bits())
        .collect();
    emitter.emit(&Event::PlanetMap(protocol::PlanetMap {
        grid_width: cfg.grid_width,
        grid_height: cfg.grid_height,
        elevation_q32,
        water_depth_q32,
    }))?;
    // Run-start vegetation snapshot so the very first viewport frame
    // tints land by producer life rather than elevation. Subsequent
    // updates arrive on the yearly cadence from `run_tick`.
    emitter.emit(&Event::CellBiomass(protocol::CellBiomass {
        tick: 0,
        producer_index_q32: crate::run_tick::cell_producer_index_q32(&ecosystem),
    }))?;
    let mut laws = build_laws(&planet, cfg.grid_height);
    laws.magnetism.init_field(&mut state);
    // Plate roster sampled with the planet-scale area_factor so its
    // per-tick rate coefficients (convergence_rate / divergence_rate
    // / erosion_k) come back already lifted by `radius²`. Earth
    // (factor 1.0) reduces to the legacy single-arg path.
    let area_factor = planet.radius * planet.radius;
    let (tectonics, plate_id, crust_thickness) = sim_physics::Tectonics::sample_plates_for_planet(
        planet.seed,
        state.grid(),
        area_factor,
    );
    state.set_tectonics_fields(plate_id, crust_thickness);
    laws.install_tectonics(tectonics);
    let recognition = RecognitionLibrary::earth_like_default();
    let planet_ctx = build_planet_context(&planet, &state);

    // Touch `planet` once mutably so the per-tick tidal-locking
    // damping (which mutates `planet.moons[*].eccentricity`) keeps
    // a mut binding — kept for parity with the original `let mut
    // planet`. No-op semantically.
    let _ = &mut planet;

    let mut species = sim_species::derive(&planet, &recognition);
    emitter.emit(&Event::Species(species_to_event(&species)))?;

    for eco in ecosystem.species.values() {
        let mut s = species.clone();
        s.seed = planet.seed.wrapping_add(u64::from(eco.species_id.0));
        s.name = format!("{}-eco{}", species.name, eco.species_id.0);
        s.role = eco.role;
        s.lifecycle = lifecycle_for_role(eco.role);
        species_registry.insert(eco.species_id, s);
    }
    emitter.emit(&Event::SpeciesCosmologyBias(
        protocol::SpeciesCosmologyBias {
            tick: 0,
            empirical_q32: species.initial_cosmology[0].raw().to_bits(),
            communitarian_q32: species.initial_cosmology[1].raw().to_bits(),
            reformist_q32: species.initial_cosmology[2].raw().to_bits(),
            mystical_q32: species.initial_cosmology[3].raw().to_bits(),
            hierarchical_q32: species.initial_cosmology[4].raw().to_bits(),
        },
    ))?;

    // Archetype classification (open lever-signature). Emitted once at
    // run start from the world+species prior; downstream the realized
    // archetype refines as civs confirm relations and unlock tools.
    {
        use sim_civ::archetype::Lever;
        let profile = sim_civ::archetype::classify_world_species(&planet, &species);
        let ranked = profile.scores.ranked();
        let lever_names: Vec<String> = Lever::ALL.iter().map(|l| l.name().to_string()).collect();
        let lever_scores_q32: Vec<i64> = Lever::ALL
            .iter()
            .map(|l| profile.scores.get(*l).raw().to_bits())
            .collect();
        emitter.emit(&Event::ArchetypeDerived(protocol::ArchetypeDerived {
            tick: 0,
            label: profile.label.name(),
            dominant_lever: ranked[0].0.name().to_string(),
            secondary_lever: ranked[1].0.name().to_string(),
            cognition_mode: profile.cognition.name().to_string(),
            lever_names,
            lever_scores_q32,
        }))?;
    }

    let species_modality_kinds: Vec<ModalityKind> =
        species.modalities.iter().map(|m| m.kind).collect();
    let civs: Vec<Civ> = Vec::new();

    let nomad_pops = nomads::init_pops(
        &state,
        &planet,
        species.habitat,
        &std::collections::BTreeSet::new(),
    );
    emit_nomads_changed(emitter, 0, &nomad_pops)?;

    let nomad_pressure_streak: BTreeMap<u32, u64> = BTreeMap::new();
    let nomad_observations: BTreeMap<u32, BTreeMap<u32, u64>> = BTreeMap::new();

    let species_channels: BTreeSet<sim_recognition::ChannelKind> = species
        .modalities
        .iter()
        .map(|m| m.kind.to_channel())
        .collect();
    let species_manipulations: BTreeSet<sim_species::ManipulationKind> =
        species.manipulation_modes.iter().map(|m| m.kind).collect();
    let species_baseline = species.perceivable_templates.clone();
    // Touch `species` mutably as in the original setup loop (which
    // refreshed civs' available_forms over `&mut civs`). With an
    // empty civs vec at this point the loop is a no-op, but we keep
    // a placeholder so the mutability and order match the original.
    let _ = &mut species;

    let has_magnetosphere = planet.magnetosphere != Magnetosphere::None;
    let has_em_medium = planet.atmosphere != Atmosphere::None
        || matches!(
            planet.composition,
            Composition::OceanWorld | Composition::SubSurfaceOcean
        );

    Ok(RunState {
        planet,
        state,
        orch_state,
        laws,
        recognition,
        planet_ctx,
        species,
        ecosystem,
        species_registry,
        speciation_tracker,
        species_modality_kinds,
        civs,
        next_civ_id: 1,
        last_collapse_tick: None,
        last_breakaway_tick: None,
        emitted_contacts: BTreeSet::new(),
        war_state: BTreeMap::new(),
        trade_routes: BTreeMap::new(),
        ticks_without_active_civ: 0,
        early_run_end: None,
        total_confirmed_relations: 0,
        total_refinements: 0,
        total_catastrophes: 0,
        total_tech_unlocks: 0,
        total_knowledge_transmissions: 0,
        total_knowledge_diffusions: 0,
        first_tier5_complete_tick: None,
        nomad_pops,
        last_emergent_tick: 0,
        nomad_pressure_streak,
        nomad_observations,
        species_channels,
        species_manipulations,
        species_baseline,
        has_magnetosphere,
        has_em_medium,
    })
}
