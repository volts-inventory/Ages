//! Core run orchestration: seeded RNG, tick loop, phase walking.
//!
//! The whole sim threads a single `Rng` through every decision path.
//! The per-tick phase order is enforced here.
//!
//! `RunConfig` + `Rng` + `rng_from_seed` live in `mod config`.
//! The run-start setup helpers (`emit_nomads_changed`,
//! `emit_species_drift_if_meaningful`, `build_planet_context`) live
//! in `mod setup`. The big `run()` function and the per-run
//! constants stay here.

use protocol::{
    AllianceDissolveReason, AllianceDissolved, AllianceFormed, CatastropheFired, CivCollapsed,
    CivContact, CivFounded, CivTerritoryChanged, ConflictResolved, Event, KnowledgeDiffused,
    KnowledgeTransmitted, PeaceConcluded, PeaceReason, Phase, RunHeader, TickEvent, WarDeclared,
    SCHEMA_VERSION,
};
use sim_arith::Real;
use sim_civ::{catastrophe, conflict, cosmology, tech, transmission, Civ};
use sim_ecosystem::{
    sample_ecosystem_with_substrate, step_hgt, step_speciation, PlanetEcosystem,
    SpeciationTracker,
};
use sim_events::Emitter;
use sim_physics::{HexGrid, OrchestratorState, PhysicsState};
use sim_recognition::RecognitionLibrary;
use sim_species::SpeciesId;
use sim_world::{init_planet, sample_planet, Atmosphere, Composition, Magnetosphere};
use std::collections::{BTreeMap, BTreeSet};

mod config;
mod contact;
mod events;
mod laws;
mod nomads;
mod phases;
mod setup;
mod territory;

pub use config::{rng_from_seed, Rng, RunConfig};

use events::{claimed_cells_for_event, figure_born_event, planet_to_event, species_to_event};
use laws::build_laws;
use setup::{build_planet_context, emit_nomads_changed, emit_species_drift_if_meaningful};
use territory::{
    compute_territory, pick_distant_habitable_cell, pick_habitable_cell, target_cell_count,
};

/// Walk the per-tick phases in their fixed order. Returning a slice
/// keeps the order canonical and unchangeable from outside.
pub const PHASE_ORDER: &[Phase] = &[
    Phase::TickStart,
    Phase::PhysicsIntegration,
    Phase::PatternRecognition,
    Phase::CohortObservations,
    Phase::FigureObservations,
    Phase::PatternDetection,
    Phase::HypothesisTesting,
    Phase::Discovery,
    Phase::AdoptionAndDecay,
    Phase::CapabilityEvaluation,
    Phase::PopulationDynamics,
    Phase::CivLifecycle,
    Phase::CulturalDrift,
    Phase::TickEnd,
];

/// The civ-sim tick loop. Walks the per-tick phase order each
/// tick. Physics laws are built from the sampled Planet so each seed
/// produces a different science.
#[allow(clippy::too_many_lines)]
pub fn run<E: Emitter>(cfg: &RunConfig, emitter: &mut E) -> Result<(), E::Error> {
    let _rng = rng_from_seed(cfg.seed);

    emitter.emit(&Event::RunStart(RunHeader {
        schema_version: SCHEMA_VERSION,
        seed: cfg.seed,
        ages_version: env!("CARGO_PKG_VERSION").to_string(),
    }))?;

    // Presentation metadata — substrate freeze/boil ranges
    // (sourced from `sim_physics::chemistry::substrate_phase_thresholds`)
    // and the label tables (sourced from `sim_report::labels::build_run_metadata`).
    // Emitted right after `RunStart` so the live viewport, the
    // post-run report, and `narrate.py` all read the same wire-
    // format vocabulary instead of duplicating tables.
    let metadata = sim_report::labels::build_run_metadata(|substrate| {
        let (freeze, boil) = sim_physics::chemistry::substrate_phase_thresholds(substrate);
        (freeze.to_f64_for_display(), boil.to_f64_for_display())
    });
    emitter.emit(&Event::RunMetadata(metadata))?;

    let grid = HexGrid::new(cfg.grid_width, cfg.grid_height);
    let mut state = PhysicsState::new(grid);
    // Persistent orchestrator state — carries the cumulative
    // conservation-drift accumulators across every tick of the run
    // so the debug-mode slow-leak detector sees the full history.
    // Constructed once here; threaded into every `physics_phase`
    // call so the per-tick deltas accumulate into a single tally.
    let mut orch_state = OrchestratorState::new();
    // `planet` is mutable to let Sprint 5 Item 19's per-tick tidal-
    // locking dynamics damp each moon's eccentricity over time. The
    // bulk-property fields the rest of the run reads (gravity,
    // composition, atmosphere, ...) are still effectively immutable
    // — only `planet.moons[*].eccentricity` is touched per tick.
    let mut planet = sample_planet(cfg.seed);
    emitter.emit(&Event::Planet(planet_to_event(&planet)))?;
    init_planet(&mut state, &planet);

    // P0.1: multi-species ecosystem. Sampled once at worldgen-time
    // from `(planet.seed, planet.metabolic_substrate)` and stepped
    // every tick inside the main loop (between the chemistry and
    // catastrophe phases). Producer carrying capacity scales with the
    // planet's biosphere density × grid size so a lush planet starts
    // with a thicker biomass pool than a sparse one. The seed XOR
    // (`sample_ecosystem_with_substrate` applies `0xEC05_0001_5751_1F00`
    // internally) keeps the ecosystem draw in its own deterministic
    // namespace — collisions with the worldgen / tectonics / species
    // RNG streams are avoided by giving each stream its own discriminator.
    let substrate_tag: &'static str = planet.metabolic_substrate.tag();
    // Planet-level producer capacity: every cell carries a unit of
    // primary-production headroom, scaled by the continuous biosphere
    // density (0 = lifeless → 0 capacity; 1 = lush → 1 unit per cell).
    // Clamped to a floor of 1.0 so even a near-lifeless world has a
    // non-degenerate Lindeman pyramid for the per-tick step (the
    // extinction sweep then collapses unsupported tiers naturally).
    let planet_capacity: Real = {
        let n_cells_real = Real::from_int(state.grid().n_cells() as i64);
        let cap = n_cells_real * planet.biosphere_density;
        if cap < Real::ONE {
            Real::ONE
        } else {
            cap
        }
    };
    let mut ecosystem: PlanetEcosystem =
        sample_ecosystem_with_substrate(planet.seed, substrate_tag, planet_capacity);
    // Speciation + HGT trackers/registries live alongside the
    // ecosystem. The species registry feeds `step_speciation` (which
    // reads a parent `Species` from the registry to derive daughters)
    // and `step_hgt` (which iterates Microbial species to swap traits).
    // The civ-bearing `species` derived above is *not* an ecosystem
    // species record — those are the trophic-tier members sampled by
    // `sample_ecosystem_with_substrate`. Until the per-trait species
    // registry is wired in (P1+), the registry stays empty: the per-
    // tick step still runs (extinction sweep + biogeochem are gated
    // on the ecosystem's own species map, not the registry), but no
    // daughter species or HGT events will fire. The events still flow
    // through the emitter when they do — that's the wire-up this PR
    // is responsible for.
    let mut species_registry: BTreeMap<SpeciesId, sim_species::Species> = BTreeMap::new();
    let mut speciation_tracker = SpeciationTracker::new();

    // PlanetMap — per-cell elevation + water_depth in row-major
    // order (matches HexGrid::cells()). Emitted once after
    // init_planet so the post-run report can draw an ASCII map
    // of the world without re-sampling the planet.
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
    let mut laws = build_laws(&planet, cfg.grid_height);
    // Seed the per-cell magnetic vector field once at planet
    // init. The per-tick `integrate` then overwrites with the
    // diurnal-modulated value each macro-step.
    laws.magnetism.init_field(&mut state);
    // Sprint 4 Item 12: sample the tectonic plate roster + per-cell
    // plate-id + per-cell crust-thickness from the planet seed, and
    // install the plate roster into `laws.tectonics`. The state-side
    // fields are installed via `set_tectonics_fields`. Must come
    // after `init_planet` so the grid is sized and after `build_laws`
    // so the default-constructed Tectonics is in place to be
    // replaced.
    let (tectonics, plate_id, crust_thickness) =
        sim_physics::Tectonics::sample_plates_for_seed(planet.seed, state.grid());
    state.set_tectonics_fields(plate_id, crust_thickness);
    laws.install_tectonics(tectonics);
    let recognition = RecognitionLibrary::earth_like_default();
    // Per-run climate calibration for relative thresholds.
    // Built after init_planet so per-cell imprints (e.g.
    // GaseousShell's 700 K column) feed into the band calibration.
    let planet_ctx = build_planet_context(&planet, &state);

    // Derive the species after recognition library is built.
    // The species is the run's persistent unit; civs found within it.
    // Species is now mutable across the run — civs propose
    // emergent recognition templates that graduate into the
    // species canon as `species.discovered_templates`.
    let mut species = sim_species::derive(&planet, &recognition);
    emitter.emit(&Event::Species(species_to_event(&species)))?;
    // Emit the species' starting cosmology pole-position
    // (cosmology bias) as a one-shot record event. Lets the post-run
    // report's species card show "Mira's species starts at
    // +Communitarian +0.20" without re-deriving the bias formula
    // from species traits.
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

    // Thread the species' cognition trait into the tolerance and
    // minimum-sample formulas. Smarter species demand tighter fits
    // and need fewer points to attempt one. The species seed feeds
    // the per-civ phoneme grammar so figure names reproduce
    // deterministically per (species, civ_id).
    // Modalities flow into the civ's NameGrammar so figure names
    // reflect the species' communication channel (acoustic species
    // get syllabic names, visual species get brightness names, etc.).
    let species_modality_kinds: Vec<sim_species::ModalityKind> =
        species.modalities.iter().map(|m| m.kind).collect();
    // The species owns multiple civs across its history.
    // Inaugural civ + any successor founded after a collapse
    // share the same Vec; the active one (last in the vec with
    // `is_active`) drives the per-tick civ work.
    let mut civs: Vec<Civ> = Vec::new();
    let mut next_civ_id: u32 = 1;
    let mut last_collapse_tick: Option<u64> = None;
    let mut last_breakaway_tick: Option<u64> = None;
    let mut emitted_contacts: std::collections::BTreeSet<(u32, u32)> =
        std::collections::BTreeSet::new();
    // Q-war: per-pair war state. Key is normalised `(min, max)`
    // civ_id pair; value is the tick the matching `WarDeclared`
    // fired (used to compute `PeaceConcluded.duration_ticks`).
    // Presence in this map = "currently at war"; absence = "at
    // peace." Entries are inserted on `WarDecision::DeclareWar`
    // and removed on every peace path (belligerence drop, loser
    // defeat, overlap empty, civ collapse).
    let mut war_state: std::collections::BTreeMap<(u32, u32), u64> =
        std::collections::BTreeMap::new();
    // M8: active trade routes between peaceful contacted civs.
    // Keyed by normalised (low, high) pair just like `war_state`;
    // value is the tick the route opened. Per-tick surplus flow
    // runs over every entry. Routes open when `CivContact` fires
    // on a peaceful pair, close on `WarDeclared` / civ collapse /
    // hierarchy drift above the peaceful floor.
    let mut trade_routes: std::collections::BTreeMap<(u32, u32), u64> =
        std::collections::BTreeMap::new();
    // Stagnation tracking: ticks with no active civ. Reset on
    // every founding (active civ rejoins the species). When the
    // streak crosses STAGNATION_THRESHOLD_TICKS the run ends with
    // reason `stagnation`.
    let mut ticks_without_active_civ: u64 = 0;
    // Run-end reason determined by per-tick stagnation checks; falls
    // through to `fixed_horizon` when the loop reaches max_ticks
    // with the species still going.
    let mut early_run_end: Option<(u64, &'static str)> = None;

    // Running tallies for periodic Snapshot events (vision's fourth
    // output channel). Incremented as their respective events fire;
    // emitted as a digest every SNAPSHOT_INTERVAL_TICKS.
    let mut total_confirmed_relations: u32 = 0;
    let mut total_refinements: u32 = 0;
    let mut total_catastrophes: u32 = 0;
    let mut total_tech_unlocks: u32 = 0;
    let mut total_knowledge_transmissions: u32 = 0;
    let mut total_knowledge_diffusions: u32 = 0;
    // First tick at which the species has had at least one civ
    // holding all three tier-5 capabilities. Transcendence
    // fires once this has held for `TRANSCENDENCE_SUSTAINED_TICKS`
    // — the species' summit isn't reached the moment a tool
    // unlocks, only after sustained operation at that capability
    // level (typically several civs cycling through tier-5
    // unlocks across the maturity gate).
    let mut first_tier5_complete_tick: Option<u64> = None;

    // No inaugural civ. The species starts as nomads
    // distributed across the planet; civs emerge from those
    // nomads via the per-tick `nomads::scan_for_emergence` path
    // once a cell crosses both the population and tech
    // thresholds. The first emergent founding typically lands
    // a few hundred ticks in, after the species has had time
    // to learn its surroundings via the recognition pipeline.
    //
    // The earlier inaugural civ was a placeholder for "always
    // at least one civ at tick 0" — purely engineering
    // convenience, no narrative basis. Removing it lets the sim
    // be a true life+physics simulation: species is the
    // persistent unit, civs are emergent cultural artefacts.
    let n_cells = state.grid().n_cells();
    let _ = n_cells; // Kept for downstream `target_cell_count` calls.
    let mut nomad_pops = nomads::init_pops(
        &state,
        &planet,
        species.habitat,
        &std::collections::BTreeSet::new(),
    );
    emit_nomads_changed(emitter, 0, &nomad_pops)?;
    // Tick of last emergent founding. Cooldown prevents
    // every nomad cell crossing the threshold simultaneously
    // from spawning N civs on one tick.
    let mut last_emergent_tick: u64 = 0;
    // Per-cell sustained-density streak: how many consecutive
    // ticks each nomad cell has held above its relative
    // saturation threshold. Drives the founding gate's
    // sustained-density check (a transient demographic peak can't
    // spawn a civ; a cell that genuinely fills up and *stays*
    // full for ~5 sim-years can).
    let mut nomad_pressure_streak: std::collections::BTreeMap<u32, u64> =
        std::collections::BTreeMap::new();
    // Per-cell, per-template observation counts for
    // nomads. Replaces the earlier opaque scalar tech accumulator.
    // Each recognition firing at a nomadic cell increments
    // `[cell][template_id]`; on civ emergence the new civ
    // inherits the merged per-template counts from its claimed
    // cells, which then feed the tool-unlock pipeline.
    // A coast civ founds with `surface_water` + `flood_zone` +
    // `fertile_land` already observed; an inland-volcanic civ
    // founds with `fire` + `thermal_gradient` +
    // `magnetic_field_strong`. The founding region literally
    // shapes which technologies the civ can build first.
    let mut nomad_observations: std::collections::BTreeMap<
        u32,
        std::collections::BTreeMap<u32, u64>,
    > = std::collections::BTreeMap::new();

    // Sensorium-tech inputs derived from species + planet.
    // Cached for the run since neither species traits nor bulk
    // planet properties drift mid-run.
    let species_channels: BTreeSet<sim_recognition::ChannelKind> = species
        .modalities
        .iter()
        .map(|m| m.kind.to_channel())
        .collect();
    let species_manipulations: BTreeSet<sim_species::ManipulationKind> =
        species.manipulation_modes.iter().map(|m| m.kind).collect();
    let species_baseline = species.perceivable_templates.clone();
    // Seed every active civ's hypothesizer form vocab from
    // the species-baseline perceivable templates' structural tags.
    for c in civs.iter_mut().filter(|c| c.is_active()) {
        c.refresh_available_forms_with_modalities(
            &species_baseline,
            &recognition,
            &species_modality_kinds,
        );
    }
    let has_magnetosphere = planet.magnetosphere != Magnetosphere::None;
    let has_em_medium = planet.atmosphere != Atmosphere::None
        || matches!(
            planet.composition,
            Composition::OceanWorld | Composition::SubSurfaceOcean
        );

    for tick in 0..cfg.max_ticks {
        // Sprint 5 Item 19: per-tick tidal-locking dynamics. Damp
        // each moon's orbital eccentricity at a rate driven by the
        // planet's locking_state: Synchronous damps fast, FreeRotator
        // slowly, Resonance not at all (gravitational forcing from
        // other bodies sustains a steady-state e). One macro-step's
        // worth of damping per civ-tick.
        {
            let dt = Real::ONE;
            let locking = planet.locking_state;
            for moon in &mut planet.moons {
                sim_world::step_eccentricity_damping(moon, locking, dt);
            }
        }
        let prev_state_for_measurements = phases::physics_phase(
            emitter,
            tick,
            &mut state,
            &mut orch_state,
            cfg,
            &laws,
            &civs,
        )?;
        let firings = phases::recognition_phase(
            emitter,
            tick,
            &state,
            &recognition,
            &planet_ctx,
            &species,
            &civs,
            &nomad_pops,
            &mut nomad_observations,
        )?;
        let hypothesis_events = phases::cohort_and_figure_phase(
            emitter,
            tick,
            &state,
            &prev_state_for_measurements,
            &mut civs,
            &species_baseline,
            &firings,
        )?;
        phases::discovery_emission_phase(
            emitter,
            tick,
            &recognition,
            &mut species,
            &mut civs,
            &hypothesis_events,
            &mut total_confirmed_relations,
            &mut total_refinements,
            &state,
            &planet,
        )?;

        phases::tech_unlock_phase(
            emitter,
            tick,
            &planet,
            &recognition,
            &species_channels,
            &species_manipulations,
            &species_baseline,
            &species_modality_kinds,
            has_magnetosphere,
            has_em_medium,
            &mut civs,
            total_confirmed_relations,
            &mut total_tech_unlocks,
            &state,
        )?;

        // Per-tick resource consumption: unlocked tools with
        // burnable resource_prereqs draw down their substance
        // across territory. Mass-conservative against the
        // chemistry layer (mirrors combustion's
        // fuel + oxidiser → ash). Renewable (Fuel) recovers via
        // the regrowth reaction; Fossil monotonically depletes.
        phases::resource_consumption_phase(&mut state, &civs);

        phases::population_phase(
            emitter,
            tick,
            &state,
            &planet,
            &species,
            &mut civs,
            &mut nomad_pops,
        )?;

        // Collapse + catastrophe per civ; founding
        // checks once after, with concurrent and sequential
        // triggers.
        emitter.emit(&Event::Tick(TickEvent {
            tick,
            phase: Phase::CivLifecycle,
        }))?;
        let mut collapse_events: Vec<CivCollapsed> = Vec::new();
        let mut cohesion_events: Vec<protocol::CohesionShifted> = Vec::new();
        let mut life_expectancy_events: Vec<protocol::CivLifeExpectancyChanged> = Vec::new();
        let mut surplus_events: Vec<protocol::CivSurplusChanged> = Vec::new();
        // M8: per-civ at-war count for the surplus drain. Read
        // from `war_state` (keyed by normalised (low, high) pairs).
        // Pre-computed here so the civ-step loop stays mutable.
        let mut at_war_counts: std::collections::BTreeMap<u32, u64> =
            std::collections::BTreeMap::new();
        for (a, b) in war_state.keys() {
            *at_war_counts.entry(*a).or_insert(0) += 1;
            *at_war_counts.entry(*b).or_insert(0) += 1;
        }
        for civ in civs.iter_mut().filter(|c| c.is_active()) {
            // Terrain-aware food security so a civ stranded
            // on peaks / shallow sea starves on the schedule its
            // habitable claim deserves rather than the raw fuel
            // sum's optimistic story.
            if let Some(reason) = civ.check_collapse_with_terrain(tick, &state, &planet) {
                let final_pop_q32 = civ.cohort.total().raw().to_bits();
                let final_figs = u32::try_from(
                    civ.figures
                        .iter()
                        .filter(|f| f.retired_tick.is_none())
                        .count(),
                )
                .unwrap_or(u32::MAX);
                let civ_id = civ.id;
                civ.collapse(tick);
                civ.apply_cosmology_push(&cosmology::push_for_civ_collapsed(), Real::ONE);
                // Collapse pushes religion much harder than
                // cosmology — surviving founders attribute the
                // catastrophe to gods/punishment, ritual hardens,
                // sacred-time goes eschatological.
                civ.apply_religion_push(&sim_civ::religion::push_for_civ_collapsed(), Real::ONE);
                collapse_events.push(CivCollapsed {
                    tick,
                    civ_id,
                    reason: reason.tag().to_string(),
                    final_population_q32: final_pop_q32,
                    final_figure_count: final_figs,
                });
            }
            // Emit a CohesionShifted event when the civ's
            // cohesion has drifted ≥ 0.05 absolute since the last
            // emission. Gate keeps the per-tick noise out of the log.
            let threshold = sim_arith::Real::from_ratio(
                sim_civ::COHESION_EMIT_THRESHOLD.0,
                sim_civ::COHESION_EMIT_THRESHOLD.1,
            );
            if (civ.cohesion - civ.last_emitted_cohesion).abs() >= threshold {
                cohesion_events.push(protocol::CohesionShifted {
                    tick,
                    civ_id: civ.id,
                    cohesion_q32: civ.cohesion.raw().to_bits(),
                    previous_q32: civ.last_emitted_cohesion.raw().to_bits(),
                });
                civ.last_emitted_cohesion = civ.cohesion;
            }
            // Emit a CivLifeExpectancyChanged event when the civ's
            // life expectancy at birth has shifted by ≥ 24 months
            // (2 years at the BASELINE_MONTHS_PER_YEAR baseline)
            // since the last emission, OR if no emission has fired
            // yet (sentinel zero from founding).
            let life_threshold_months = sim_arith::Real::from_int(24);
            let current_life = civ.life_expectancy_months();
            let last_life = civ.last_emitted_life_expectancy_months;
            let never_emitted = last_life <= sim_arith::Real::ZERO;
            let drifted = (current_life - last_life).abs() >= life_threshold_months;
            if never_emitted || drifted {
                life_expectancy_events.push(protocol::CivLifeExpectancyChanged {
                    tick,
                    civ_id: civ.id,
                    life_expectancy_months_q32: current_life.raw().to_bits(),
                });
                civ.last_emitted_life_expectancy_months = current_life;
            }
            // M8: step the per-civ surplus accumulator. Utilisation
            // = aggregate_pop / aggregate_cap (terrain-aware). A civ
            // running at high fill accumulates surplus; a civ at war
            // drains it.
            let pop = civ.aggregate_population();
            let cap = civ.carrying_capacity_with_terrain(&state, &planet);
            let utilisation = if cap.raw().to_num::<i64>() > 0 {
                let pop_r = pop.to_real_nonneg();
                let cap_r = sim_arith::Real::from_int(cap.raw().to_num::<i64>().max(1));
                (pop_r / cap_r).clamp01()
            } else {
                sim_arith::Real::ZERO
            };
            let at_war_count = at_war_counts.get(&civ.id).copied().unwrap_or(0);
            sim_civ::economy::step_surplus(civ, utilisation, at_war_count);
            // Emit a CivSurplusChanged event when the absolute
            // delta crosses the emit floor, OR when this is the
            // civ's first non-zero emission so the founding state
            // surfaces in the log.
            let emit_floor = sim_arith::Real::from_int(sim_civ::economy::SURPLUS_EMIT_DELTA_FLOOR);
            let delta = civ.surplus - civ.last_emitted_surplus;
            if delta.abs() >= emit_floor {
                surplus_events.push(protocol::CivSurplusChanged {
                    tick,
                    civ_id: civ.id,
                    surplus_q32: civ.surplus.raw().to_bits(),
                    previous_q32: civ.last_emitted_surplus.raw().to_bits(),
                });
                civ.last_emitted_surplus = civ.surplus;
            }
        }
        if !collapse_events.is_empty() {
            last_collapse_tick = Some(tick);
            for ev in collapse_events {
                emitter.emit(&Event::CivCollapsed(ev))?;
            }
        }
        for ev in cohesion_events {
            emitter.emit(&Event::CohesionShifted(ev))?;
        }
        for ev in life_expectancy_events {
            emitter.emit(&Event::CivLifeExpectancyChanged(ev))?;
        }
        for ev in surplus_events {
            emitter.emit(&Event::CivSurplusChanged(ev))?;
        }

        // P0.1: per-tick ecosystem step. Runs AFTER the chemistry
        // phase (chemistry is integrated inside `physics_phase` above,
        // so atmospheric CO2 is current at this point) and BEFORE the
        // catastrophe phase (so the just-updated biomass + extinction
        // sweep is visible to catastrophe survival math through the
        // emitted `SpeciesExtinct` events). The canonical biogeochem
        // step couples producer growth ← atmospheric CO2 + solar /
        // oxidiser energy and respiration / decomposition → atmospheric
        // CO2, then runs the extinction sweep.
        //
        // `solar_irradiance` is the planet's nominal stellar
        // irradiance (Earth ≈ 1361 W/m²). The ecosystem uses it as
        // the energy budget for Photoautotrophs; Chemoautotrophs draw
        // off the planet-wide Oxidiser pool, Mixotrophs draw both.
        let solar_irradiance = planet.stellar_luminosity;
        let extinct_events =
            ecosystem.step_with_biogeochem_at_tick(&mut state, solar_irradiance, tick);
        for ev in extinct_events {
            // P0.1: open a post-extinction adaptive-radiation window
            // on the speciation tracker so the post-extinction
            // multiplier kicks in on subsequent ticks. The window
            // closes naturally after `POST_EXTINCTION_BOOST_TICKS`.
            speciation_tracker.register_extinction_event(tick);
            emitter.emit(&Event::SpeciesExtinct(ev))?;
        }
        // Speciation + HGT. Both run once per tick (the per-tick
        // probabilities for polyploidy / HGT are baked into the
        // step functions themselves; the per-tick allopatric and
        // sympatric streak counters live on the tracker). The
        // species registry is empty until P1 wires the trait
        // registry through, so these calls return empty event
        // vectors today — but the wire-up is in place so the
        // moment the registry is populated the events flow.
        let speciation_results =
            step_speciation(tick, &ecosystem.species, &species_registry, &mut speciation_tracker);
        for (daughter, event) in speciation_results {
            // Register the daughter in the registry so later
            // ticks can speciate off her too. The ecosystem-side
            // `EcoSpecies` record + role wiring is a P1
            // responsibility; the registry insert here only
            // affects future speciation passes.
            species_registry.insert(SpeciesId(event.daughter_id), daughter);
            emitter.emit(&Event::SpeciationOccurred(event))?;
        }
        let hgt_events = step_hgt(&mut species_registry, tick, cfg.seed);
        for ev in hgt_events {
            emitter.emit(&Event::HorizontalGeneTransfer(ev))?;
        }

        // Catastrophe check on every active civ.
        let mut cat_events: Vec<(u32, catastrophe::CatastropheRecord)> = Vec::new();
        for civ in civs.iter_mut().filter(|c| c.is_active()) {
            let civ_id = civ.id;
            if let Some(rec) = catastrophe::check_and_apply(civ, &mut state, &planet, &species, tick) {
                // M7: bump the civ's selection bias from the
                // catastrophe's per-kind weights so survivors'
                // trait distribution shifts toward the survival-
                // correlated traits — passed to successors via
                // `inherit_species_drift_with_environment`.
                civ.record_catastrophe_selection_bias(rec.kind, rec.fraction_lost);
                // M8: catastrophes consume stored surplus first —
                // grain reserves feed the displaced, the polity
                // diverts public works to rescue + repair.
                sim_civ::economy::drain_surplus_on_catastrophe(civ);
                cat_events.push((civ_id, rec));
            }
        }
        for (civ_id, rec) in cat_events {
            emitter.emit(&Event::CatastropheFired(CatastropheFired {
                tick,
                civ_id,
                catastrophe_kind: rec.kind.tag().to_string(),
                fraction_lost_q32: rec.fraction_lost.raw().to_bits(),
            }))?;
            total_catastrophes = total_catastrophes.saturating_add(1);
        }

        phases::cultural_drift_phase(emitter, tick, &mut civs)?;
        phases::culture_flip_phase(emitter, tick, &state, &planet, &mut civs)?;

        // Founding check (v2 — two triggers OR-combined).
        if civs.iter().all(|c| !c.is_active()) {
            // Trigger v1: stateless-pop + recent-remnant + min dark age.
            let parent_collapse = last_collapse_tick;
            let elapsed_collapse = parent_collapse.map(|t| tick.saturating_sub(t));
            let metabolism = planet.metabolic_substrate.metabolism();
            let dark_age_min = sim_civ::streak_ticks_for_metabolism(
                sim_civ::FOUNDING_MIN_DARK_AGE_TICKS,
                metabolism,
            );
            let remnant_window = sim_civ::streak_ticks_for_metabolism(
                sim_civ::RECENT_REMNANT_WINDOW_TICKS,
                metabolism,
            );
            let v1_eligible =
                elapsed_collapse.is_some_and(|e| (dark_age_min..=remnant_window).contains(&e));
            // Trigger v2: charismatic-founder + post-catastrophe.
            // Probe what the next civ's founding band would look
            // like to see if any figure clears charisma >= 0.8.
            let probe_civ = Civ::with_species(
                next_civ_id,
                tick,
                sim_arith::Pop::from_int(50),
                species.cognition,
                species.seed,
                &species_modality_kinds,
                species.initial_cosmology,
            );
            let charismatic_present = probe_civ
                .figures
                .iter()
                .any(|f| f.charisma >= Real::from_ratio(8, 10));
            let recent_catastrophe = civs
                .iter()
                .filter_map(|c| c.last_catastrophe_tick)
                .max()
                .is_some_and(|t| tick.saturating_sub(t) <= 100);
            let v2_eligible = recent_catastrophe && charismatic_present;
            if v1_eligible || v2_eligible {
                let stateless_total = civs
                    .iter()
                    .filter(|c| c.cohort.civ_membership.is_none())
                    .map(|c| c.cohort.total())
                    .fold(sim_arith::Pop::ZERO, |a, b| a + b);
                // Substrate-derived founding floor — sparse
                // worlds need more founders than lush ones.
                let founding_floor =
                    sim_civ::founding_min_population(planet.biosphere, species.cognition);
                if stateless_total >= founding_floor {
                    let parent_id = civs
                        .iter()
                        .rev()
                        .find(|c| c.collapsed_tick.is_some())
                        .map_or(0, |c| c.id);
                    // Parent's centroid (the predecessor's
                    // capital) so the successor can pick a distinct
                    // cell. Earlier the band-id-derived
                    // figures[0].cell_assignment routinely landed
                    // both civs on cell 0; an earlier fix corrected the display
                    // tie but the underlying centroid still collided.
                    let parent_centroid = civs
                        .iter()
                        .find(|c| c.id == parent_id)
                        .map(|c| c.territory_centroid);
                    for c in &mut civs {
                        if c.cohort.civ_membership.is_none() {
                            // Stateless cohort gets absorbed into the
                            // new civ; zero every bracket on the
                            // predecessor so we don't double-count.
                            c.cohort.scale_in_place(Real::ZERO);
                        }
                    }
                    let stateless_cohort = sim_civ::Cohort::with_civ(stateless_total, next_civ_id);
                    let mut new_civ = Civ::refound_from_stateless(
                        next_civ_id,
                        tick,
                        stateless_cohort,
                        species.cognition,
                        species.seed,
                        &species_modality_kinds,
                        parent_id,
                    );
                    // Deterministic kingdom-style civ name.
                    new_civ.name = sim_civ::civ_name_from_seed(cfg.seed, next_civ_id);
                    // Inherit parent's species drift so this
                    // successor's effective traits are species + drift.
                    // Done before `dynamics_for_civ` so the per-tick
                    // birth/death rates reflect the inherited drift.
                    if let Some(parent_civ) = civs.iter().find(|c| c.id == parent_id) {
                        new_civ.inherit_species_drift_with_environment(
                            parent_civ,
                            planet.seed,
                            planet.metabolic_substrate.metabolism(),
                            planet.biosphere,
                        );
                        new_civ.inherit_lineage_from(parent_civ);
                    }
                    new_civ.dynamics = sim_civ::dynamics_for_civ(&new_civ, &species, &planet);
                    new_civ.configure_substrate_with_topology(
                        species.habitat,
                        new_civ.effective_cognition(&species),
                        new_civ.effective_sociality(&species),
                        planet.metabolic_substrate.metabolism(),
                        state.grid().width().saturating_mul(state.grid().height()),
                        species.cognition_topology,
                    );
                    // Successor's territory sized to its own
                    // founding population, centred on its first
                    // figure's attention focus. Decoupled from
                    // parent's final claim so a small successor
                    // doesn't inherit a sprawling dead empire.
                    //
                    // Pick a centroid as far as possible from
                    // every currently-claimed cell across all active
                    // civs. The Sumatra-vs-China model — a successor
                    // arising in a remote region rather than next door
                    // to the parent (the adjacent-shift fallback). Falls
                    // back to `pick_habitable_cell` if no occupied
                    // cells exist (degenerate, since the parent that
                    // just collapsed should still have its claim).
                    let occupied: BTreeSet<u32> = civs
                        .iter()
                        .flat_map(|c| c.claimed_cells.iter().copied())
                        .collect();
                    new_civ.territory_centroid = if occupied.is_empty() {
                        pick_habitable_cell(
                            new_civ.territory_centroid,
                            state.grid(),
                            &state,
                            &planet,
                            &new_civ,
                            species.habitat,
                        )
                    } else {
                        pick_distant_habitable_cell(
                            new_civ.territory_centroid,
                            state.grid(),
                            &state,
                            &planet,
                            &new_civ,
                            species.habitat,
                            &occupied,
                        )
                    };
                    let target = target_cell_count(&new_civ, state.grid().n_cells());
                    // Stateless refound: every civ is collapsed,
                    // so their `claimed_cells` (still populated on
                    // the dead civ structs) represent abandoned
                    // land the successor inherits. Pass an empty
                    // `forbidden` so the BFS claims those husks.
                    let cells = compute_territory(
                        new_civ.territory_centroid,
                        target,
                        state.grid(),
                        &state,
                        &planet,
                        &new_civ,
                        species.habitat,
                        &BTreeSet::new(),
                    );
                    let _ = parent_centroid; // distant placement supersedes adjacency
                    new_civ.claim_cells(&cells);
                    new_civ.refresh_available_forms_with_modalities(
            &species_baseline,
            &recognition,
            &species_modality_kinds,
        );
                    let initial_pop_q32 = new_civ.cohort.total().raw().to_bits();
                    let band = u32::try_from(new_civ.figures.len()).unwrap_or(0);
                    for f in &new_civ.figures {
                        emitter.emit(&Event::FigureBorn(figure_born_event(new_civ.id, f)))?;
                    }
                    let new_cells = claimed_cells_for_event(&new_civ);
                    let new_caps: Vec<i128> = new_cells
                        .iter()
                        .map(|&c| {
                            new_civ
                                .cell_capacity(&state, c, tick, &planet)
                                .raw()
                                .to_bits()
                        })
                        .collect();
                    emitter.emit(&Event::CivFounded(CivFounded {
                        tick,
                        civ_id: new_civ.id,
                        parent_civ_id: Some(parent_id),
                        name: new_civ.name.clone(),
                        initial_population_q32: initial_pop_q32,
                        founding_figure_count: band,
                        claimed_cells: new_cells,
                        cell_capacities_q32: new_caps,
                    }))?;
                    new_civ.last_territory_emit_tick = tick;
                    emit_species_drift_if_meaningful(emitter, &new_civ)?;

                    // Inter-civ transmission: comprehend
                    // the predecessor's confirmed relations into
                    // the successor's first figure, gated by
                    // linguistic distance + age decay + tier.
                    // Decay constant derives from species.
                    let decay_ticks = species.transmission_decay_ticks();
                    let comm_speed = species.communication_speed_multiplier();
                    let (transmissions, mythologizations) =
                        if let Some(parent_civ) = civs.iter().find(|c| c.id == parent_id) {
                            transmission::transmit_from_parent(
                                &mut new_civ,
                                parent_civ,
                                tick,
                                decay_ticks,
                                comm_speed,
                            )
                        } else {
                            (Vec::new(), Vec::new())
                        };
                    let dest_civ_id = new_civ.id;
                    let dest_figure_id = new_civ.figures.first().map_or(0, |f| f.id);
                    for record in &transmissions {
                        emitter.emit(&Event::KnowledgeTransmitted(KnowledgeTransmitted {
                            tick,
                            source_civ_id: parent_id,
                            dest_civ_id,
                            dest_figure_id,
                            relation_id: record.relation.relation_id,
                            source_form: record.relation.form.tag().to_string(),
                            comprehension_q32: record.comprehension.raw().to_bits(),
                        }))?;
                        total_knowledge_transmissions =
                            total_knowledge_transmissions.saturating_add(1);
                    }
                    // Emit mythologization events for the
                    // sub-threshold transmissions that residue into
                    // cosmology rather than transferring as
                    // confirmed knowledge.
                    for myth in &mythologizations {
                        emitter.emit(&Event::RelationMythologized(
                            protocol::RelationMythologized {
                                tick,
                                source_civ_id: parent_id,
                                dest_civ_id,
                                relation_id: myth.relation_id,
                                axis: myth.axis,
                                magnitude_q32: myth.magnitude.raw().to_bits(),
                                comprehension_q32: myth.comprehension.raw().to_bits(),
                            },
                        ))?;
                    }

                    civs.push(new_civ);
                    next_civ_id += 1;
                }
            }
        }

        // M5: concurrent breakaway. Fires alongside an
        // existing civ when the parent has dogmatism > 0.7 and a
        // would-be founding band has charisma >= 0.7. Splits the
        // parent's population in half (both civs claim the same
        // cells; conflict resolution handles overlap in a follow-up
        // PR). 500-tick global cooldown prevents thrash.
        let breakaway_cooldown_ok =
            last_breakaway_tick.is_none_or(|t| tick.saturating_sub(t) >= 500);
        // Cohesion-fragmentation breakaway. Picks a civ whose
        // cohesion has stayed in `[CIVIL_WAR_COHESION_FLOOR,
        // COHESION_BREAKAWAY_TRIGGER]` for `COHESION_BREAKAWAY_STREAK_TICKS`
        // — a regional faction that's been disgruntled long enough
        // to fork off. Takes precedence over the dogmatic path when
        // both candidates exist on the same tick (fragmentation is
        // the more urgent failure mode).
        let cohesion_breakaway_streak_metabolised = sim_civ::streak_ticks_for_metabolism(
            sim_civ::COHESION_BREAKAWAY_STREAK_TICKS,
            planet.metabolic_substrate.metabolism(),
        );
        let cohesion_parent_id: Option<u32> = if breakaway_cooldown_ok {
            civs.iter()
                .find(|c| {
                    c.is_active()
                        && c.cohesion_breakaway_streak >= cohesion_breakaway_streak_metabolised
                })
                .map(|c| c.id)
        } else {
            None
        };
        let dogmatic_parent_id: Option<u32> =
            if breakaway_cooldown_ok && cohesion_parent_id.is_none() {
                civs.iter()
                    .find(|c| c.is_active() && c.cosmology.dogmatism() > Real::from_ratio(7, 10))
                    .map(|c| c.id)
            } else {
                None
            };
        // Pair the picked parent id with a share fraction
        // (30% for cohesion path, 50% for dogmatic) and a tag so
        // post-fork tweaks know which path fired.
        let breakaway_pick: Option<(u32, (i64, i64), bool)> = cohesion_parent_id
            .map(|id| {
                (
                    id,
                    sim_civ::COHESION_BREAKAWAY_SHARE,
                    true, // is_cohesion
                )
            })
            .or_else(|| dogmatic_parent_id.map(|id| (id, (1, 2), false)));
        if let Some((parent_id, (share_num, share_den), is_cohesion_breakaway)) = breakaway_pick {
            let probe = Civ::with_species(
                next_civ_id,
                tick,
                sim_arith::Pop::from_int(50),
                species.cognition,
                species.seed,
                &species_modality_kinds,
                species.initial_cosmology,
            );
            let charismatic = probe
                .figures
                .iter()
                .any(|f| f.charisma >= Real::from_ratio(7, 10));
            if charismatic {
                let parent_idx = civs
                    .iter()
                    .position(|c| c.id == parent_id)
                    .expect("dogmatic parent in civs");
                let parent_aggregate = civs[parent_idx].cohort.total();
                // Share fraction is 30% for cohesion-driven
                // breakaway, 50% for dogmatic — a falling-apart civ
                // forks off a smaller faction than a charismatic
                // ideological split does.
                let share_ratio = Real::from_ratio(share_num, share_den);
                let breakaway_share = parent_aggregate * share_ratio;
                // Substrate-derived breakaway floor — half the
                // substrate's founding floor.
                let breakaway_floor =
                    sim_civ::founding_min_population(planet.biosphere, species.cognition)
                        / Real::from_int(2);
                if breakaway_share >= breakaway_floor {
                    'breakaway: {
                        // Parent's centroid for the same successor-
                        // centroid distinctness rule the stateless-refound
                        // path uses (above). Read before mutating
                        // region_cohorts so we capture the live capital.
                        let parent_centroid = civs[parent_idx].territory_centroid;
                        // Civil-war seizure: the breakaway's centroid IS
                        // a parent border cell — a cell the parent claims
                        // that has at least one non-parent neighbour
                        // (frontier with water, unclaimed land, or
                        // another civ). The faction seats itself in a
                        // captured village rather than settling new
                        // ground next door. If parent has no border cells
                        // (degenerate: parent has zero claims), skip the
                        // breakaway entirely.
                        //
                        // Selection: lowest cell id among border
                        // candidates that isn't already another live
                        // civ's capital. Deterministic, no extra RNG
                        // draw, bit-for-bit replayable. The
                        // anti-collision filter handles the two-
                        // breakaways-in-one-tick case (two siblings of
                        // the same parent, or a breakaway happening
                        // while a same-tick stateless refound seeded
                        // its centroid on what would otherwise be the
                        // lowest border cell) — without it the second
                        // capital lands on the first's cell and the
                        // frame renderer's older-civ-wins de-collide
                        // hides the new civ's letter on the map.
                        // Relax the filter if every border collides
                        // (degenerate): a doubled capital is better
                        // than skipping the breakaway entirely.
                        let mut border_candidates: Vec<u32> = Vec::new();
                        for &cell in &civs[parent_idx].claimed_cells {
                            let axial = state.grid().axial_of(sim_physics::CellId(cell));
                            let has_non_parent_nbr = state
                                .grid()
                                .neighbours(axial)
                                .iter()
                                .any(|nbr| !civs[parent_idx].claimed_cells.contains(&nbr.0));
                            if has_non_parent_nbr {
                                border_candidates.push(cell);
                            }
                        }
                        border_candidates.sort_unstable();
                        let live_centroids: BTreeSet<u32> = civs
                            .iter()
                            .filter(|c| c.collapsed_tick.is_none())
                            .map(|c| c.territory_centroid)
                            .collect();
                        let pick = border_candidates
                            .iter()
                            .find(|c| !live_centroids.contains(c))
                            .or_else(|| border_candidates.first())
                            .copied();
                        let Some(centroid) = pick else {
                            break 'breakaway;
                        };
                        // Transfer the seized cell from parent to (the
                        // eventual) breakaway: drop it from parent's
                        // claim + region_cohorts, then resync parent's
                        // aggregate so subsequent `scale_in_place(keep)`
                        // operates on the smaller post-seize total.
                        let mut seized_cohort = sim_civ::Cohort::empty_with_civ(next_civ_id);
                        civs[parent_idx].claimed_cells.remove(&centroid);
                        if let Some(c) = civs[parent_idx].region_cohorts.remove(&centroid) {
                            seized_cohort.merge_in(&c);
                        }
                        civs[parent_idx].resync_aggregate_from_regions();
                        let seized_cells: Vec<u32> = vec![centroid];
                        // Recompute breakaway_share against the
                        // post-seize parent so totals conserve:
                        //   parent_remaining = (orig - seized) × keep
                        //   breakaway_total  = (orig - seized) × share + seized
                        //                    = orig - parent_remaining ✓
                        let parent_aggregate_post = civs[parent_idx].cohort.total();
                        let breakaway_share = parent_aggregate_post * share_ratio;
                        let keep_share = Real::ONE - share_ratio;
                        civs[parent_idx].cohort.scale_in_place(keep_share);
                        for c in civs[parent_idx].region_cohorts.values_mut() {
                            c.scale_in_place(keep_share);
                        }
                        let _ = parent_centroid; // retained for symmetry with stateless-refound path
                                                 // Cohesion path — give the parent a small
                                                 // recovery boost (the disgruntled faction left)
                                                 // so it doesn't immediately re-trigger another
                                                 // breakaway streak. Capped at 1.0 by clamp.
                        if is_cohesion_breakaway {
                            let recovery = Real::from_ratio(
                                sim_civ::COHESION_PARENT_RECOVERY.0,
                                sim_civ::COHESION_PARENT_RECOVERY.1,
                            );
                            civs[parent_idx].cohesion =
                                (civs[parent_idx].cohesion + recovery).clamp01();
                            civs[parent_idx].cohesion_breakaway_streak = 0;
                        }
                        // Emit the parent's post-seizure territory so
                        // viewport / report consumers see the seized
                        // cell leave the parent's claim immediately —
                        // otherwise the next CivTerritoryChanged for
                        // the parent doesn't fire until its next
                        // expand_via_overflow tick, leaving a phantom
                        // multi-claim where both civs appear to own the
                        // seized cell. Only emit when seizure actually
                        // happened (no-op if seized_cells is empty).
                        if !seized_cells.is_empty() {
                            let parent_civ_ref = &civs[parent_idx];
                            let parent_claimed_sorted = claimed_cells_for_event(parent_civ_ref);
                            let parent_cell_pops: Vec<i128> = parent_claimed_sorted
                                .iter()
                                .map(|c| {
                                    parent_civ_ref
                                        .region_cohorts
                                        .get(c)
                                        .map_or(0i128, |cohort| cohort.total().raw().to_bits())
                                })
                                .collect();
                            let parent_cell_caps: Vec<i128> = parent_claimed_sorted
                                .iter()
                                .map(|&c| {
                                    parent_civ_ref
                                        .cell_capacity(&state, c, tick, &planet)
                                        .raw()
                                        .to_bits()
                                })
                                .collect();
                            emitter.emit(&Event::CivTerritoryChanged(CivTerritoryChanged {
                                tick,
                                civ_id: parent_id,
                                claimed_cells: parent_claimed_sorted,
                                population_q32: parent_civ_ref.cohort.total().raw().to_bits(),
                                cell_populations_q32: parent_cell_pops,
                                cell_capacities_q32: parent_cell_caps,
                            }))?;
                            civs[parent_idx].last_territory_emit_tick = tick;
                        }
                        // Breakaway founding cohort: migrating dissidents
                        // from the rest of parent territory + the full
                        // residents of the seized cell. Merging brackets
                        // (not just the scalar count) preserves the
                        // seized cell's age structure so the breakaway
                        // doesn't start with a synthetic all-fertile band.
                        let mut breakaway_cohort =
                            sim_civ::Cohort::with_civ(breakaway_share, next_civ_id);
                        breakaway_cohort.merge_in(&seized_cohort);
                        breakaway_cohort.civ_membership = Some(next_civ_id);
                        let mut new_civ = Civ::refound_from_stateless(
                            next_civ_id,
                            tick,
                            breakaway_cohort,
                            species.cognition,
                            species.seed,
                            &species_modality_kinds,
                            parent_id,
                        );
                        // Deterministic kingdom-style civ name.
                        new_civ.name = sim_civ::civ_name_from_seed(cfg.seed, next_civ_id);
                        // Breakaway descends from a still-living
                        // parent — inherit its drift verbatim and add a
                        // step. The parent and child diverge from this
                        // point on as separate civs.
                        {
                            let parent_civ = &civs[parent_idx];
                            new_civ.inherit_species_drift_with_environment(
                                parent_civ,
                                planet.seed,
                                planet.metabolic_substrate.metabolism(),
                                planet.biosphere,
                            );
                            new_civ.inherit_lineage_from(parent_civ);
                        }
                        // Cohesion-driven breakaway starts at a
                        // higher cohesion than its falling-apart parent
                        // — fresh authority, shared cause, smaller scale.
                        // Dogmatic-driven breakaway leaves the default
                        // cohesion (1.0) since it's a charismatic split
                        // rather than a fragmentation event.
                        if is_cohesion_breakaway {
                            new_civ.cohesion = Real::from_ratio(
                                sim_civ::COHESION_BREAKAWAY_INITIAL.0,
                                sim_civ::COHESION_BREAKAWAY_INITIAL.1,
                            );
                            new_civ.last_emitted_cohesion = new_civ.cohesion;
                        }
                        new_civ.dynamics = sim_civ::dynamics_for_civ(&new_civ, &species, &planet);
                        new_civ.configure_substrate_with_topology(
                            species.habitat,
                            new_civ.effective_cognition(&species),
                            new_civ.effective_sociality(&species),
                            planet.metabolic_substrate.metabolism(),
                            state.grid().width().saturating_mul(state.grid().height()),
                            species.cognition_topology,
                        );
                        // Breakaway sized to its half-share of the
                        // parent's population; centred on the seized
                        // parent border cell (the rebellion seats itself
                        // in a captured village). The civil-war /
                        // cultural-secession model: dissidents take over
                        // a chunk of parent territory rather than
                        // migrating to virgin frontier. Stateless
                        // refounds keep the distant-placement path
                        // (`pick_distant_habitable_cell` above) for the
                        // Sumatra-vs-China successor model — that's a
                        // "parent is dead, successor rises elsewhere"
                        // story; this is "parent still rules, faction
                        // splits off from a border city."
                        new_civ.territory_centroid = centroid;
                        let target = target_cell_count(&new_civ, state.grid().n_cells());
                        // Breakaway: parent still holds its (post-
                        // seizure) territory. Pass a fresh `forbidden`
                        // set computed from civs as they stand AFTER the
                        // seizure — the seized cell is no longer in any
                        // civ's claim, so the BFS will pick it up (it's
                        // the centroid, force-included) plus any
                        // unclaimed habitable neighbours. The breakaway
                        // can BFS *through* remaining parent territory
                        // (parent land is still traversable) but never
                        // double-claims it.
                        let occupied_post: BTreeSet<u32> = civs
                            .iter()
                            .flat_map(|c| c.claimed_cells.iter().copied())
                            .collect();
                        let cells = compute_territory(
                            new_civ.territory_centroid,
                            target,
                            state.grid(),
                            &state,
                            &planet,
                            &new_civ,
                            species.habitat,
                            &occupied_post,
                        );
                        let _ = parent_centroid; // distant placement supersedes adjacency
                        new_civ.claim_cells(&cells);
                        new_civ.refresh_available_forms_with_modalities(
            &species_baseline,
            &recognition,
            &species_modality_kinds,
        );
                        let initial_pop_q32 = new_civ.cohort.total().raw().to_bits();
                        let band = u32::try_from(new_civ.figures.len()).unwrap_or(0);
                        for f in &new_civ.figures {
                            emitter.emit(&Event::FigureBorn(figure_born_event(new_civ.id, f)))?;
                        }
                        let new_cells = claimed_cells_for_event(&new_civ);
                        let new_caps: Vec<i128> = new_cells
                            .iter()
                            .map(|&c| {
                                new_civ
                                    .cell_capacity(&state, c, tick, &planet)
                                    .raw()
                                    .to_bits()
                            })
                            .collect();
                        emitter.emit(&Event::CivFounded(CivFounded {
                            tick,
                            civ_id: new_civ.id,
                            parent_civ_id: Some(parent_id),
                            name: new_civ.name.clone(),
                            initial_population_q32: initial_pop_q32,
                            founding_figure_count: band,
                            claimed_cells: new_cells,
                            cell_capacities_q32: new_caps,
                        }))?;
                        new_civ.last_territory_emit_tick = tick;
                        emit_species_drift_if_meaningful(emitter, &new_civ)?;

                        // Inter-civ transmission for the breakaway
                        // path. The parent is still alive, so age decay
                        // is zero (the band leaves with current knowledge);
                        // linguistic distance is the only gate that
                        // matters here. Without this, breakaway successors
                        // would start tabula rasa on the same planet the
                        // parent already mapped — the opposite of the inaugural
                        // "knowledge survives" promise.
                        let (transmissions, mythologizations) = {
                            let parent_civ = &civs[parent_idx];
                            let decay_ticks = species.transmission_decay_ticks();
                            let comm_speed = species.communication_speed_multiplier();
                            transmission::transmit_from_parent(
                                &mut new_civ,
                                parent_civ,
                                tick,
                                decay_ticks,
                                comm_speed,
                            )
                        };
                        let dest_civ_id = new_civ.id;
                        let dest_figure_id = new_civ.figures.first().map_or(0, |f| f.id);
                        for record in &transmissions {
                            emitter.emit(&Event::KnowledgeTransmitted(KnowledgeTransmitted {
                                tick,
                                source_civ_id: parent_id,
                                dest_civ_id,
                                dest_figure_id,
                                relation_id: record.relation.relation_id,
                                source_form: record.relation.form.tag().to_string(),
                                comprehension_q32: record.comprehension.raw().to_bits(),
                            }))?;
                            total_knowledge_transmissions =
                                total_knowledge_transmissions.saturating_add(1);
                        }
                        // Mythologization events for the breakaway
                        // path's sub-threshold transmissions.
                        for myth in &mythologizations {
                            emitter.emit(&Event::RelationMythologized(
                                protocol::RelationMythologized {
                                    tick,
                                    source_civ_id: parent_id,
                                    dest_civ_id,
                                    relation_id: myth.relation_id,
                                    axis: myth.axis,
                                    magnitude_q32: myth.magnitude.raw().to_bits(),
                                    comprehension_q32: myth.comprehension.raw().to_bits(),
                                },
                            ))?;
                        }

                        let new_id = new_civ.id;
                        civs.push(new_civ);
                        next_civ_id += 1;
                        last_breakaway_tick = Some(tick);
                        // Emit CivContact for the (parent, breakaway) pair
                        // *only* when they're actually within reach. Distant placement
                        // places breakaways at distant habitable cells, so
                        // a child dropped on the far side of the world
                        // shouldn't fire the "they met" beat at the moment
                        // of splintering — the M5 cadence-gated pass below
                        // will pick them up later when/if either civ grows
                        // into range or unlocks navigation tech.
                        let pair = if parent_id < new_id {
                            (parent_id, new_id)
                        } else {
                            (new_id, parent_id)
                        };
                        if !emitted_contacts.contains(&pair) {
                            let parent_civ =
                                civs.iter().find(|c| c.id == parent_id).expect("parent civ");
                            let child_civ = civs
                                .iter()
                                .find(|c| c.id == new_id)
                                .expect("just-pushed breakaway civ");
                            if contact::civs_in_contact(
                                parent_civ,
                                child_civ,
                                species.habitat,
                                &state,
                            ) {
                                emitted_contacts.insert(pair);
                                // PR4: mirror the contact in each
                                // civ's `contact_history` set so
                                // the alliance-formation rule
                                // (`propose_alliance`) sees prior
                                // contact history when scanning
                                // pairs later.
                                if let Some(parent_mut) =
                                    civs.iter_mut().find(|c| c.id == parent_id)
                                {
                                    parent_mut.contact_history.insert(new_id);
                                }
                                if let Some(child_mut) = civs.iter_mut().find(|c| c.id == new_id) {
                                    child_mut.contact_history.insert(parent_id);
                                }
                                emitter.emit(&Event::CivContact(CivContact {
                                    tick,
                                    civ_a: pair.0,
                                    civ_b: pair.1,
                                }))?;
                            }
                        }
                    }
                } // close 'breakaway: { ... } and `if breakaway_share >= ...`
            }
        }

        // M5 + tech-gated contact: emit CivContact for any newly
        // co-existing pair that's also reachable given each civ's
        // contact range (foot → boats → navigation → radio) and
        // terrain (water blocks land-only civs unless they have
        // watercraft; radio-tier civs ignore terrain). Bounded BFS
        // per un-met pair runs on the `CONTACT_CHECK_TICKS` cadence
        // — yearly is plenty for a diplomatic-scale event and keeps
        // the per-tick BFS budget bounded.
        let active_ids: Vec<u32> = civs
            .iter()
            .filter(|c| c.is_active())
            .map(|c| c.id)
            .collect();
        if tick.is_multiple_of(contact::CONTACT_CHECK_TICKS) {
            for i in 0..active_ids.len() {
                for j in (i + 1)..active_ids.len() {
                    let pair = if active_ids[i] < active_ids[j] {
                        (active_ids[i], active_ids[j])
                    } else {
                        (active_ids[j], active_ids[i])
                    };
                    if emitted_contacts.contains(&pair) {
                        continue;
                    }
                    let civ_a = civs.iter().find(|c| c.id == pair.0).expect("active civ");
                    let civ_b = civs.iter().find(|c| c.id == pair.1).expect("active civ");
                    if !contact::civs_in_contact(civ_a, civ_b, species.habitat, &state) {
                        continue;
                    }
                    emitted_contacts.insert(pair);
                    // PR4: mirror the contact in each civ's
                    // `contact_history` so `propose_alliance` can
                    // gate on prior-contact later. Done before the
                    // event emission so contact-history bookkeeping
                    // is consistent even if the emit fails.
                    if let Some(a_mut) = civs.iter_mut().find(|c| c.id == pair.0) {
                        a_mut.contact_history.insert(pair.1);
                    }
                    if let Some(b_mut) = civs.iter_mut().find(|c| c.id == pair.1) {
                        b_mut.contact_history.insert(pair.0);
                    }
                    // Re-borrow immutably for the peaceful-pair
                    // check below (the prior `civ_a` / `civ_b` refs
                    // were invalidated by the `iter_mut` calls).
                    let civ_a = civs.iter().find(|c| c.id == pair.0).expect("active civ");
                    let civ_b = civs.iter().find(|c| c.id == pair.1).expect("active civ");
                    emitter.emit(&Event::CivContact(CivContact {
                        tick,
                        civ_a: pair.0,
                        civ_b: pair.1,
                    }))?;
                    // M8: open a trade route if both civs sit
                    // below the peaceful-hierarchy floor. Per-tick
                    // surplus flow then smooths the buffer across
                    // the pair until war / collapse / hierarchy
                    // drift closes it.
                    if conflict::is_peaceful_pair(civ_a, civ_b) {
                        if !trade_routes.contains_key(&pair) {
                            trade_routes.insert(pair, tick);
                            emitter.emit(&Event::TradeRouteEstablished(
                                protocol::TradeRouteEstablished {
                                    tick,
                                    civ_a: pair.0,
                                    civ_b: pair.1,
                                },
                            ))?;
                        }
                    }
                }
            }
        }

        // M8: per-tick trade flow over open routes. Each route
        // shifts a small fraction of the gap between the pair's
        // surpluses toward parity. Prune routes touching collapsed
        // civs first, then run the flow over the survivors.
        let active_set_now: std::collections::BTreeSet<u32> = civs
            .iter()
            .filter(|c| c.is_active())
            .map(|c| c.id)
            .collect();
        let stale_routes: Vec<(u32, u32)> = trade_routes
            .keys()
            .copied()
            .filter(|(a, b)| !active_set_now.contains(a) || !active_set_now.contains(b))
            .collect();
        for pair in stale_routes {
            trade_routes.remove(&pair);
            emitter.emit(&Event::TradeRouteClosed(protocol::TradeRouteClosed {
                tick,
                civ_a: pair.0,
                civ_b: pair.1,
                reason: "civ_collapsed".to_string(),
            }))?;
        }
        let route_pairs: Vec<(u32, u32)> = trade_routes.keys().copied().collect();
        for (a_id, b_id) in route_pairs {
            // Split-borrow trick: split_at_mut around the lower
            // index lets us mutably borrow both civs without
            // overlapping borrows.
            let pos_a = civs.iter().position(|c| c.id == a_id);
            let pos_b = civs.iter().position(|c| c.id == b_id);
            if let (Some(pa), Some(pb)) = (pos_a, pos_b) {
                let (lo, hi) = if pa < pb { (pa, pb) } else { (pb, pa) };
                let (left, right) = civs.split_at_mut(hi);
                let civ_lo = &mut left[lo];
                let civ_hi = &mut right[0];
                sim_civ::economy::trade_flow_between(civ_lo, civ_hi);
            }
        }

        // Q-war conflict + cross-civ diffusion. Periodic checks
        // over ordered pairs of active civs. War is now stateful:
        // a per-pair belligerence score (`assess_pair`) gates
        // whether `resolve()` runs each check, with hysteresis
        // between declare (0.55) and end (0.35) thresholds.
        if active_ids.len() >= 2 {
            if tick.is_multiple_of(conflict::CONFLICT_CHECK_TICKS) {
                // Prune war_state entries whose civs have gone
                // inactive (collapse). Emit PeaceConcluded so
                // downstream consumers see the war close.
                let mut peace_events: Vec<PeaceConcluded> = Vec::new();
                let active_set: std::collections::BTreeSet<u32> =
                    active_ids.iter().copied().collect();
                let stale_pairs: Vec<(u32, u32)> = war_state
                    .keys()
                    .copied()
                    .filter(|(a, b)| !active_set.contains(a) || !active_set.contains(b))
                    .collect();
                for pair in stale_pairs {
                    let started = war_state.remove(&pair).unwrap_or(tick);
                    peace_events.push(PeaceConcluded {
                        tick,
                        civ_a: pair.0,
                        civ_b: pair.1,
                        reason: PeaceReason::TerritoryResolved,
                        duration_ticks: tick.saturating_sub(started),
                    });
                }

                let mut conflict_events: Vec<ConflictResolved> = Vec::new();
                let mut war_events: Vec<WarDeclared> = Vec::new();
                // M8: trade routes closed by war declarations
                // during this check pass. Emitted alongside the
                // WarDeclared events so consumers see the route
                // close on the same tick as the war.
                let mut trade_close_events: Vec<protocol::TradeRouteClosed> = Vec::new();
                for i in 0..active_ids.len() {
                    for j in (i + 1)..active_ids.len() {
                        let civ_id_first = active_ids[i];
                        let civ_id_second = active_ids[j];
                        let Some(slot_first) = civs.iter().position(|c| c.id == civ_id_first)
                        else {
                            continue;
                        };
                        let Some(slot_second) = civs.iter().position(|c| c.id == civ_id_second)
                        else {
                            continue;
                        };
                        let idx_a = slot_first;
                        let idx_b = slot_second;
                        let (lo, hi) = if idx_a < idx_b {
                            (idx_a, idx_b)
                        } else {
                            (idx_b, idx_a)
                        };
                        let pair = if civ_id_first < civ_id_second {
                            (civ_id_first, civ_id_second)
                        } else {
                            (civ_id_second, civ_id_first)
                        };
                        let already_at_war = war_state.contains_key(&pair);

                        let (left, right) = civs.split_at_mut(hi);
                        let civ_lo = &mut left[lo];
                        let civ_hi = &mut right[0];

                        // Q-war: war requires prior contact. If
                        // the pair has never met (per `CivContact`
                        // bookkeeping) skip entirely — they
                        // can't be at war and can't go to war.
                        if !emitted_contacts.contains(&pair) {
                            continue;
                        }

                        // Empty-overlap path: if the pair was at
                        // war and overlap is now gone (e.g. via
                        // independent territory loss), close the
                        // war as `TerritoryResolved`.
                        let overlap_empty = conflict::overlap(civ_lo, civ_hi).is_empty();
                        if overlap_empty {
                            if already_at_war {
                                let started = war_state.remove(&pair).unwrap_or(tick);
                                peace_events.push(PeaceConcluded {
                                    tick,
                                    civ_a: pair.0,
                                    civ_b: pair.1,
                                    reason: PeaceReason::TerritoryResolved,
                                    duration_ticks: tick.saturating_sub(started),
                                });
                            }
                            continue;
                        }

                        let Some(assessment) =
                            conflict::assess_pair(civ_lo, civ_hi, &state, &planet, tick)
                        else {
                            continue;
                        };
                        let decision =
                            conflict::decide_war(already_at_war, assessment.belligerence);

                        match decision {
                            conflict::WarDecision::StayPeaceful => {
                                // Border friction with no losses.
                                continue;
                            }
                            conflict::WarDecision::ConcludePeace => {
                                let started = war_state.remove(&pair).unwrap_or(tick);
                                peace_events.push(PeaceConcluded {
                                    tick,
                                    civ_a: pair.0,
                                    civ_b: pair.1,
                                    reason: PeaceReason::BelligerenceDropped,
                                    duration_ticks: tick.saturating_sub(started),
                                });
                                continue;
                            }
                            conflict::WarDecision::DeclareWar => {
                                war_state.insert(pair, tick);
                                war_events.push(WarDeclared {
                                    tick,
                                    aggressor_civ_id: assessment.aggressor_id,
                                    defender_civ_id: assessment.defender_id,
                                    belligerence_q32: assessment.belligerence.raw().to_bits(),
                                    drive_q32: assessment.drive.raw().to_bits(),
                                    kinship_q32: assessment.kinship.raw().to_bits(),
                                });
                                // M8: war closes any open trade
                                // route between these civs. Emitted
                                // separately so consumers can show
                                // the trade-collapse cascade.
                                if trade_routes.remove(&pair).is_some() {
                                    trade_close_events.push(protocol::TradeRouteClosed {
                                        tick,
                                        civ_a: pair.0,
                                        civ_b: pair.1,
                                        reason: "war_declared".to_string(),
                                    });
                                }
                                // fall through to resolve()
                            }
                            conflict::WarDecision::ContinueWar => {
                                // already at war, fall through to resolve()
                            }
                        }

                        let outcome = if idx_a < idx_b {
                            conflict::resolve(civ_lo, civ_hi, tick)
                        } else {
                            conflict::resolve(civ_hi, civ_lo, tick)
                        };
                        if let Some(o) = outcome {
                            let defeated = o.loser_defeated;
                            conflict_events.push(ConflictResolved {
                                tick,
                                winner_civ_id: o.winner_id,
                                loser_civ_id: o.loser_id,
                                disputed_cell_count: u32::try_from(o.disputed_cells.len())
                                    .unwrap_or(u32::MAX),
                                loss_fraction_q32: o.loss_fraction.raw().to_bits(),
                                loser_defeated: defeated,
                            });
                            if defeated {
                                let started = war_state.remove(&pair).unwrap_or(tick);
                                peace_events.push(PeaceConcluded {
                                    tick,
                                    civ_a: pair.0,
                                    civ_b: pair.1,
                                    reason: PeaceReason::Defeated,
                                    duration_ticks: tick.saturating_sub(started),
                                });
                            }
                        }
                    }
                }
                for ev in war_events {
                    emitter.emit(&Event::WarDeclared(ev))?;
                }
                for ev in trade_close_events {
                    emitter.emit(&Event::TradeRouteClosed(ev))?;
                }
                for ev in conflict_events {
                    emitter.emit(&Event::ConflictResolved(ev))?;
                }
                for ev in peace_events {
                    emitter.emit(&Event::PeaceConcluded(ev))?;
                }

                // PR4: Alliance formation + dissolution pass. Runs
                // on the same cadence as the conflict check; uses
                // the just-updated `war_state` map so the
                // war-misalignment dissolution rule sees the
                // current tick's war declarations.
                //
                // Formation: for every in-contact, at-peace pair
                // whose cosmology + religion distances fall under
                // the formation gates and that has prior
                // contact history, insert symmetric
                // `allied_with` flags and seed trust.
                // Dissolution: three paths (drift, war
                // misalignment, trust eroded); whichever fires
                // first removes the symmetric flags and emits the
                // matching `AllianceDissolved` event.
                let mut alliance_formed_events: Vec<AllianceFormed> = Vec::new();
                let mut alliance_dissolved_events: Vec<AllianceDissolved> = Vec::new();
                // Snapshot the current at-war set (normalised
                // pairs) once so the war-misalignment check is
                // self-consistent across the whole pass.
                let at_war_pairs: std::collections::BTreeSet<(u32, u32)> =
                    war_state.keys().copied().collect();

                // Plan dissolutions first — read-only over civs;
                // record the (pair, reason) tuples, then mutate.
                let mut dissolutions: Vec<((u32, u32), AllianceDissolveReason)> = Vec::new();
                let mut formations: Vec<(u32, u32)> = Vec::new();
                let mut trust_updates: Vec<((u32, u32), Real)> = Vec::new();

                for i in 0..active_ids.len() {
                    for j in (i + 1)..active_ids.len() {
                        let pair = if active_ids[i] < active_ids[j] {
                            (active_ids[i], active_ids[j])
                        } else {
                            (active_ids[j], active_ids[i])
                        };
                        let Some(civ_a_idx) = civs.iter().position(|c| c.id == pair.0) else {
                            continue;
                        };
                        let Some(civ_b_idx) = civs.iter().position(|c| c.id == pair.1) else {
                            continue;
                        };
                        let civ_a = &civs[civ_a_idx];
                        let civ_b = &civs[civ_b_idx];
                        let mutually_allied = civ_a.allied_with.contains(&civ_b.id)
                            && civ_b.allied_with.contains(&civ_a.id);

                        if mutually_allied {
                            // 1. Drift dissolution.
                            if conflict::alliance_drifted_apart(civ_a, civ_b) {
                                dissolutions
                                    .push((pair, AllianceDissolveReason::CosmologyDrift));
                                continue;
                            }
                            // 2. War-misalignment: one ally at war
                            // with a third party the other is at
                            // peace with.
                            let a_wars: std::collections::BTreeSet<u32> = at_war_pairs
                                .iter()
                                .filter_map(|(lo, hi)| {
                                    if *lo == civ_a.id {
                                        Some(*hi)
                                    } else if *hi == civ_a.id {
                                        Some(*lo)
                                    } else {
                                        None
                                    }
                                })
                                .filter(|other| *other != civ_b.id)
                                .collect();
                            let b_wars: std::collections::BTreeSet<u32> = at_war_pairs
                                .iter()
                                .filter_map(|(lo, hi)| {
                                    if *lo == civ_b.id {
                                        Some(*hi)
                                    } else if *hi == civ_b.id {
                                        Some(*lo)
                                    } else {
                                        None
                                    }
                                })
                                .filter(|other| *other != civ_a.id)
                                .collect();
                            let misaligned = a_wars
                                .iter()
                                .any(|t| !b_wars.contains(t))
                                || b_wars.iter().any(|t| !a_wars.contains(t));
                            if misaligned {
                                dissolutions
                                    .push((pair, AllianceDissolveReason::WarMisalignment));
                                continue;
                            }
                            // 3. Trust decay (third dissolution
                            // path — confidence falloff).
                            let cosmo_gap = conflict::cosmology_distance(civ_a, civ_b);
                            let religion_gap = conflict::religion_distance(civ_a, civ_b);
                            // Use the lower (more pessimistic)
                            // side's trust as the pair's effective
                            // trust — alliances dissolve when
                            // *either* side loses faith.
                            let trust_a = civ_a
                                .alliance_trust
                                .get(&civ_b.id)
                                .copied()
                                .unwrap_or(Real::from(conflict::ALLIANCE_TRUST_INITIAL));
                            let trust_b = civ_b
                                .alliance_trust
                                .get(&civ_a.id)
                                .copied()
                                .unwrap_or(Real::from(conflict::ALLIANCE_TRUST_INITIAL));
                            let prior = trust_a.min(trust_b);
                            let post = conflict::step_alliance_trust(
                                prior,
                                cosmo_gap,
                                religion_gap,
                            );
                            trust_updates.push((pair, post));
                            if post < Real::from(conflict::ALLIANCE_TRUST_FLOOR) {
                                dissolutions
                                    .push((pair, AllianceDissolveReason::TrustEroded));
                            }
                        } else {
                            // Formation candidate: must have prior
                            // contact + low cosmology+religion gap
                            // + not at war.
                            let at_war_now = at_war_pairs.contains(&pair);
                            if conflict::propose_alliance(civ_a, civ_b, at_war_now, tick) {
                                formations.push(pair);
                            }
                        }
                    }
                }

                // Apply trust updates first (records the per-pair
                // post-decay value). Dissolutions after may clear
                // the entry; that's fine — `BTreeMap::remove` is a
                // no-op on missing keys.
                let trust_initial = Real::from(conflict::ALLIANCE_TRUST_INITIAL);
                for (pair, post) in trust_updates {
                    if let Some(civ) = civs.iter_mut().find(|c| c.id == pair.0) {
                        civ.alliance_trust.insert(pair.1, post);
                    }
                    if let Some(civ) = civs.iter_mut().find(|c| c.id == pair.1) {
                        civ.alliance_trust.insert(pair.0, post);
                    }
                }
                // Apply formations: symmetric `allied_with` insert
                // + initial trust seed + event.
                for pair in formations {
                    if let Some(civ) = civs.iter_mut().find(|c| c.id == pair.0) {
                        civ.allied_with.insert(pair.1);
                        civ.alliance_trust.insert(pair.1, trust_initial);
                    }
                    if let Some(civ) = civs.iter_mut().find(|c| c.id == pair.1) {
                        civ.allied_with.insert(pair.0);
                        civ.alliance_trust.insert(pair.0, trust_initial);
                    }
                    alliance_formed_events.push(AllianceFormed {
                        tick,
                        civ_a: pair.0,
                        civ_b: pair.1,
                    });
                }
                // Apply dissolutions: symmetric `allied_with`
                // remove + clear the trust scalar + event.
                for (pair, reason) in dissolutions {
                    // Stamp the dissolution tick into both sides'
                    // alliance_cooldown maps so `propose_alliance`
                    // can enforce the cooldown next time these two
                    // come into proximity. Without this the pair
                    // flaps form/dissolve at the 0.4/0.6 hysteresis
                    // edge.
                    if let Some(civ) = civs.iter_mut().find(|c| c.id == pair.0) {
                        civ.allied_with.remove(&pair.1);
                        civ.alliance_trust.remove(&pair.1);
                        civ.alliance_cooldown.insert(pair.1, tick);
                    }
                    if let Some(civ) = civs.iter_mut().find(|c| c.id == pair.1) {
                        civ.allied_with.remove(&pair.0);
                        civ.alliance_trust.remove(&pair.0);
                        civ.alliance_cooldown.insert(pair.0, tick);
                    }
                    alliance_dissolved_events.push(AllianceDissolved {
                        tick,
                        civ_a: pair.0,
                        civ_b: pair.1,
                        reason,
                    });
                }
                for ev in alliance_formed_events {
                    emitter.emit(&Event::AllianceFormed(ev))?;
                }
                for ev in alliance_dissolved_events {
                    emitter.emit(&Event::AllianceDissolved(ev))?;
                }
            }
            if tick.is_multiple_of(100) {
                let mut diffusions: Vec<KnowledgeDiffused> = Vec::new();
                for i in 0..active_ids.len() {
                    for j in 0..active_ids.len() {
                        if i == j {
                            continue;
                        }
                        let source_civ_id = active_ids[i];
                        let receiver_civ_id = active_ids[j];
                        let Some(source_slot) = civs.iter().position(|c| c.id == source_civ_id)
                        else {
                            continue;
                        };
                        let Some(receiver_slot) = civs.iter().position(|c| c.id == receiver_civ_id)
                        else {
                            continue;
                        };
                        let src_idx = source_slot;
                        let dst_idx = receiver_slot;
                        let peaceful = {
                            let s = &civs[src_idx];
                            let d = &civs[dst_idx];
                            conflict::is_peaceful_pair(s, d)
                        };
                        if !peaceful {
                            continue;
                        }
                        let (lo, hi) = if src_idx < dst_idx {
                            (src_idx, dst_idx)
                        } else {
                            (dst_idx, src_idx)
                        };
                        let (left, right) = civs.split_at_mut(hi);
                        let civ_lo = &mut left[lo];
                        let civ_hi = &mut right[0];
                        let (source, dest) = if src_idx < dst_idx {
                            (&*civ_lo, civ_hi)
                        } else {
                            (&*civ_hi, civ_lo)
                        };
                        let dest_id = dest.id;
                        let dest_fig = dest.figures.first().map_or(0, |f| f.id);
                        let records = transmission::diffuse_between(
                            source,
                            dest,
                            tick,
                            species.communication_speed_multiplier(),
                        );
                        for r in records {
                            diffusions.push(KnowledgeDiffused {
                                tick,
                                source_civ_id,
                                dest_civ_id: dest_id,
                                dest_figure_id: dest_fig,
                                relation_id: r.relation.relation_id,
                                source_form: r.relation.form.tag().to_string(),
                                comprehension_q32: r.comprehension.raw().to_bits(),
                            });
                        }
                    }
                }
                for ev in diffusions {
                    emitter.emit(&Event::KnowledgeDiffused(ev))?;
                    total_knowledge_diffusions = total_knowledge_diffusions.saturating_add(1);
                }
            }
        }

        // Nomadic species pool — slow logistic growth on
        // unclaimed habitable cells. Civs absorbed nomads from
        // any newly-gained cells during the territory-tracking
        // step above, so this only touches genuinely unclaimed
        // habitable cells. Emit a per-tick mirror so the viewport
        // can render the `0` glyph; the event also lets the
        // post-run report compute nomadic biomass over time.
        let claim_union: std::collections::BTreeSet<u32> = civs
            .iter()
            .filter(|c| c.is_active())
            .flat_map(|c| c.claimed_cells.iter().copied())
            .collect();
        nomads::step_growth(
            &mut nomad_pops,
            &state,
            &planet,
            species.habitat,
            &nomad_observations,
            species.cognition,
            species.sociality,
            species.lifespan_years,
            &claim_union,
        );
        // Ambient seeding: every AMBIENT_NOMAD_CHECK_TICKS, drop a
        // small founding nomad pop into one deterministic empty
        // habitable cell. Models off-grid migrant bands wandering
        // into vacated regions — without this, a continent that
        // lost its nomads to civ absorption stays empty forever
        // (no neighbours to diffuse from). The seed is small so
        // diffusion + logistic growth (the dominant per-tick
        // mechanisms) still drive most of the population dynamic;
        // ambient emergence just keeps the seed kernel alive.
        nomads::ambient_emergence(
            &mut nomad_pops,
            &state,
            &planet,
            species.habitat,
            &claim_union,
            tick,
        );

        // Emergent civ founding from saturated nomadic clusters.
        // After growth, update the per-cell sustained-density
        // streak, then scan for cells that have held the
        // saturation gate long enough (and have a dense neighbour
        // hinterland and enough accumulated tech) to spawn a civ.
        // Cooldown prevents every-cell-at-once spawning; distance
        // check keeps emergent civs far from existing ones.
        nomads::update_pressure_streak(
            &mut nomad_pressure_streak,
            &nomad_pops,
            &state,
            &planet,
            species.habitat,
        );
        if tick >= last_emergent_tick + nomads::EMERGENT_FOUNDING_COOLDOWN_TICKS {
            let centroids: Vec<u32> = civs
                .iter()
                .filter(|c| c.is_active())
                .map(|c| c.territory_centroid)
                .collect();
            if let Some(emerge_cell) = nomads::scan_for_emergence(
                &nomad_pops,
                &nomad_observations,
                &nomad_pressure_streak,
                species.cognition,
                species.sociality,
                &state,
                &planet,
                species.habitat,
                &centroids,
                &claim_union,
            ) {
                last_emergent_tick = tick;
                // Founding band seed: ~200 people. The new civ
                // immediately absorbs the local nomadic pool via
                // `absorb_into_civ`, so the real first-tick population
                // is this seed + the nomad pop at the founding cell.
                // Lifted from 20 alongside the carrying-capacity
                // baseline lift (2,500 → 50,000/fuel-unit) so a fresh
                // civ on Earth-equivalent terrain starts at ~0.4% of
                // cell cap rather than 0.04%, which shortens the
                // founding densification window and produces
                // recognisable city-scale civs within a few sim-
                // centuries.
                let mut new_civ = Civ::with_species(
                    next_civ_id,
                    tick,
                    sim_arith::Pop::from_int(200),
                    species.cognition,
                    species.seed,
                    &species_modality_kinds,
                    species.initial_cosmology,
                );
                new_civ.name = sim_civ::civ_name_from_seed(cfg.seed, next_civ_id);
                // Emergent civs after a collapse inherit drift
                // from the most-recently-collapsed civ (the species'
                // direct ancestral lineage); first-ever emergent civ
                // (no prior collapse) starts at zero drift.
                if let Some(parent_civ) = civs
                    .iter()
                    .filter(|c| c.collapsed_tick.is_some())
                    .max_by_key(|c| c.collapsed_tick.unwrap_or(0))
                {
                    new_civ.inherit_species_drift_with_environment(
                        parent_civ,
                        planet.seed,
                        planet.metabolic_substrate.metabolism(),
                        planet.biosphere,
                    );
                    new_civ.inherit_lineage_from(parent_civ);
                }
                new_civ.dynamics = sim_civ::dynamics_for_civ(&new_civ, &species, &planet);
                new_civ.configure_substrate_with_topology(
                    species.habitat,
                    new_civ.effective_cognition(&species),
                    new_civ.effective_sociality(&species),
                    planet.metabolic_substrate.metabolism(),
                    state.grid().width().saturating_mul(state.grid().height()),
                    species.cognition_topology,
                );
                new_civ.territory_centroid = emerge_cell;
                let target = target_cell_count(&new_civ, state.grid().n_cells());
                // Emergent civ: forbid every active civ's claims so
                // a high-pressure nomad cluster doesn't double-
                // claim a cell some other civ already owns. Distant
                // placement (via `nomads::scan_for_emergence`)
                // already biases the centroid away from existing
                // civs, but the BFS-fill could still reach back
                // into a neighbour without this guard.
                let cells = compute_territory(
                    new_civ.territory_centroid,
                    target,
                    state.grid(),
                    &state,
                    &planet,
                    &new_civ,
                    species.habitat,
                    &claim_union,
                );
                let gained: Vec<u32> = cells.iter().copied().collect();
                new_civ.claim_cells(&cells);
                // Fresh civ — apply the founder-effect bracket-uniform
                // loss. See `nomads::FOUNDING_ABSORB_LOSS` for the
                // band → polity reorg-tax rationale.
                let founder_loss = sim_arith::Real::from_ratio(
                    nomads::FOUNDING_ABSORB_LOSS.0,
                    nomads::FOUNDING_ABSORB_LOSS.1,
                );
                nomads::absorb_into_civ(
                    &mut nomad_pops,
                    &mut new_civ,
                    gained.clone(),
                    &species.biology,
                    founder_loss,
                );
                // Per-template *firing counts* from the nomadic
                // phase are NOT inherited. The previous design
                // dumped 100+ years of pre-emergence nomad
                // observation pressure onto a fresh civ, which —
                // combined with low per-civ observation thresholds
                // — let 12-year-old civs unlock tier-5 tools. Under
                // the experiment-driven gate, tech comes from the
                // civ's own confirmed relations and apparatus work,
                // not from inherited environmental pressure.
                //
                // The nomad-observations map is still drained (so
                // the cells stop being scored as densely-observed
                // for emergence purposes), but the counts are
                // discarded rather than folded into
                // `new_civ.observations`.
                let _drained =
                    nomads::drain_observations_for_cells(&mut nomad_observations, gained);
                new_civ.refresh_available_forms_with_modalities(
            &species_baseline,
            &recognition,
            &species_modality_kinds,
        );
                let initial_pop_q32 = new_civ.cohort.total().raw().to_bits();
                let band = u32::try_from(new_civ.figures.len()).unwrap_or(0);
                for f in &new_civ.figures {
                    emitter.emit(&Event::FigureBorn(figure_born_event(new_civ.id, f)))?;
                }
                let claimed = claimed_cells_for_event(&new_civ);
                let claimed_caps: Vec<i128> = claimed
                    .iter()
                    .map(|&c| {
                        new_civ
                            .cell_capacity(&state, c, tick, &planet)
                            .raw()
                            .to_bits()
                    })
                    .collect();
                emitter.emit(&Event::CivFounded(CivFounded {
                    tick,
                    civ_id: new_civ.id,
                    parent_civ_id: None, // emergent — no parent civ
                    name: new_civ.name.clone(),
                    initial_population_q32: initial_pop_q32,
                    founding_figure_count: band,
                    claimed_cells: claimed,
                    cell_capacities_q32: claimed_caps,
                }))?;
                new_civ.last_territory_emit_tick = tick;
                emit_species_drift_if_meaningful(emitter, &new_civ)?;
                civs.push(new_civ);
                next_civ_id += 1;
            }
        }
        emit_nomads_changed(emitter, tick, &nomad_pops)?;

        emitter.emit(&Event::Tick(TickEvent {
            tick,
            phase: Phase::TickEnd,
        }))?;

        // Species-level run-end checks. Civ-collapse is *not* a run-end —
        // the species persists across collapses. Run-end fires on
        // species-extinction (population truly gone, no recovery
        // possible) or stagnation (extended dark age that won't
        // refound). `fixed_horizon` is the loop's natural end.
        let any_active = civs.iter().any(Civ::is_active);
        let civ_pop: sim_arith::Pop = civs
            .iter()
            .map(|c| c.cohort.total())
            .fold(sim_arith::Pop::ZERO, |acc, x| acc + x);
        // Species-level total includes nomadic pop. Earlier
        // the species existed only through its civs, so an empty
        // civ list meant species-extinction. With nomadic pop
        // tracked separately, the species can persist (and birth
        // future civs) even with no civs currently active.
        let nomad_pop: Real = nomad_pops
            .values()
            .copied()
            .fold(Real::ZERO, |acc, x| acc + x);
        let total_pop: sim_arith::Pop = civ_pop + sim_arith::Pop::from_real(nomad_pop);

        // Periodic state-digest snapshot — vision's fourth output
        // channel. Emit every SNAPSHOT_INTERVAL_TICKS plus once at
        // tick 0 (anchor) so consumers always have a baseline. Skip
        // tick 0's snapshot only if the inaugural civ hasn't founded
        // yet — but in practice the founding happens during run
        // setup before the loop, so tick 0 is safe.
        if tick.is_multiple_of(SNAPSHOT_INTERVAL_TICKS) {
            let active_civ_ids: Vec<u32> = civs
                .iter()
                .filter(|c| c.is_active())
                .map(|c| c.id)
                .collect();
            let collapsed_civ_ids: Vec<u32> = civs
                .iter()
                .filter(|c| c.collapsed_tick.is_some())
                .map(|c| c.id)
                .collect();
            emitter.emit(&Event::Snapshot(protocol::Snapshot {
                tick,
                active_civ_ids,
                collapsed_civ_ids,
                total_population_q32: total_pop.raw().to_bits(),
                total_confirmed_relations,
                total_refinements,
                total_catastrophes,
                total_tech_unlocks,
                total_knowledge_transmissions,
                total_knowledge_diffusions,
            }))?;
        }
        // Floor: half the substrate's founding-min. Below this,
        // even a recovery would lack the seed pop for a refound.
        // Derives from biosphere + species.cognition rather
        // than the flat 100/2 placeholder.
        let extinction_floor =
            sim_civ::founding_min_population(planet.biosphere, species.cognition)
                / Real::from_int(2);
        if !any_active && total_pop <= extinction_floor {
            early_run_end = Some((tick, "species_extinction"));
            break;
        }
        if any_active {
            ticks_without_active_civ = 0;
        } else {
            ticks_without_active_civ += 1;
            if ticks_without_active_civ >= STAGNATION_THRESHOLD_TICKS {
                early_run_end = Some((tick, "stagnation"));
                break;
            }
        }

        // Transcendence run-end. Two-phase: first record the
        // first-ever tick at which any civ holds all three tier-5
        // capabilities; then fire transcendence once that state
        // has been sustained for `TRANSCENDENCE_SUSTAINED_TICKS`.
        // Per the project's vision boundary the sim doesn't
        // simulate the post-transition mode (no consciousness-as-
        // physics, no FTL); the run simply ends with `transcendence`
        // as the run-end reason and the report renders the
        // transition as the final age. The two-phase gate captures
        // the story's "species sustained mature tier-5 tradition
        // across multiple generations" arc rather than firing on a
        // single high-cognition civ's quick unlock.
        let any_civ_at_tier5 = civs.iter().any(|c| {
            tech::ToolKind::TIER_FIVE
                .iter()
                .all(|t| c.unlocked_tools.contains(t))
        });
        if any_civ_at_tier5 && first_tier5_complete_tick.is_none() {
            first_tier5_complete_tick = Some(tick);
        }
        if let Some(first) = first_tier5_complete_tick {
            if tick.saturating_sub(first) >= TRANSCENDENCE_SUSTAINED_TICKS {
                early_run_end = Some((tick, "transcendence"));
                break;
            }
        }
    }

    let (end_tick, end_reason) = early_run_end.unwrap_or((cfg.max_ticks, "fixed_horizon"));
    emitter.emit(&Event::RunEnd {
        tick: end_tick,
        reason: end_reason.to_string(),
    })?;

    Ok(())
}

/// Stagnation threshold: how many consecutive ticks without an
/// active civ qualifies as a species-level stagnation run-end.
/// 2× the breakaway-cooldown (500) to avoid ending mid-bounce
/// when a v2/v3 founding is imminent.
pub const STAGNATION_THRESHOLD_TICKS: u64 = 1000 * protocol::MONTHS_PER_YEAR;

/// Transcendence sustainment threshold: how many ticks the
/// species must hold at-least-one-civ-with-all-tier-5 status
/// before transcendence fires. The transcendence arc is the
/// species' tech-tree summit and should take thousands of years
/// to reach AND a sustained mature operation at that capability
/// level; the threshold encodes "this is a tradition, not a
/// one-time peak." Combined with `species_maturity_floor` (3000
/// confirmed relations needed before any tier-5 tool unlocks)
/// this typically pushes transcendence to year 10000+.
pub const TRANSCENDENCE_SUSTAINED_TICKS: u64 = 2000 * protocol::MONTHS_PER_YEAR;

/// Periodic Snapshot emission cadence (vision's fourth output
/// channel). Every 500 ticks the run digest emits as a `Snapshot`
/// event — active civ ids, total population, running totals for
/// confirmed relations / refinements / catastrophes / tech /
/// transmissions / diffusions. Cheap enough to keep without
/// bloating the event log (1 event per 500 ticks vs. the thousands
/// of recognition / tick / discovery events between).
pub const SNAPSHOT_INTERVAL_TICKS: u64 = 500 * protocol::MONTHS_PER_YEAR;

#[cfg(test)]
mod tests;
