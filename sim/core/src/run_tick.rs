//! Per-tick body extracted from `run()`. `RunState` carries the
//! mutable run state across ticks; `run_tick()` advances one tick.
//!
//! The split is mechanical: every `let mut` defined inside the
//! original `for tick in ...` loop stays inline here, and every
//! `let mut` defined *before* the loop in the old `run()` becomes
//! a `RunState` field. Public surface (`run()`, `RunConfig`) is
//! unchanged; this module is purely an internal restructuring.
//!
//! CB3 split: each phase helper now lives in its own submodule
//! under `tick_steps/`. The orchestrator below is unchanged; only
//! the helpers were relocated.

use crate::constants::{
    SNAPSHOT_INTERVAL_TICKS, STAGNATION_THRESHOLD_TICKS, TRANSCENDENCE_SUSTAINED_TICKS,
};
use crate::laws::Laws;
use crate::nomads;
use crate::phases;
use crate::setup::emit_nomads_changed;
use crate::tick_steps::{
    breakaway_step, catastrophe_step, civ_lifecycle_step, contact_and_trade_step,
    emergent_founding_step, knowledge_diffusion_step, stateless_refound_step,
    war_and_alliance_step,
};
use crate::RunConfig;
use protocol::{Event, Phase, TickEvent};
use sim_arith::Real;
use sim_civ::{conflict, tech, Civ};
use sim_ecosystem::{step_hgt, step_speciation, LocalConditions, PlanetEcosystem, SpeciationTracker};
use sim_events::Emitter;
use sim_physics::{OrchestratorState, PhysicsState};
use sim_recognition::RecognitionLibrary;
use sim_species::SpeciesId;
use std::collections::{BTreeMap, BTreeSet};

/// Per-run mutable state threaded across ticks. Built once by
/// `setup::setup_run`, mutated in place by `run_tick`, and finalised
/// by `run()` after the tick loop. Field naming and types match the
/// original locals in the monolithic `run()` 1:1 — no behavioural
/// changes, only relocation.
pub(crate) struct RunState {
    pub planet: sim_world::Planet,
    pub state: PhysicsState,
    pub orch_state: OrchestratorState,
    pub laws: Laws,
    pub recognition: RecognitionLibrary,
    pub planet_ctx: sim_recognition::PlanetContext,
    pub species: sim_species::Species,
    pub ecosystem: PlanetEcosystem,
    pub species_registry: BTreeMap<SpeciesId, sim_species::Species>,
    pub speciation_tracker: SpeciationTracker,
    pub species_modality_kinds: Vec<sim_species::ModalityKind>,
    pub civs: Vec<Civ>,
    pub next_civ_id: u32,
    pub last_collapse_tick: Option<u64>,
    pub last_breakaway_tick: Option<u64>,
    pub emitted_contacts: BTreeSet<(u32, u32)>,
    pub war_state: BTreeMap<(u32, u32), u64>,
    pub trade_routes: BTreeMap<(u32, u32), u64>,
    pub ticks_without_active_civ: u64,
    pub early_run_end: Option<(u64, &'static str)>,
    pub total_confirmed_relations: u32,
    pub total_refinements: u32,
    pub total_catastrophes: u32,
    pub total_tech_unlocks: u32,
    pub total_knowledge_transmissions: u32,
    pub total_knowledge_diffusions: u32,
    pub first_tier5_complete_tick: Option<u64>,
    pub nomad_pops: BTreeMap<u32, Real>,
    pub last_emergent_tick: u64,
    pub nomad_pressure_streak: BTreeMap<u32, u64>,
    pub nomad_observations: BTreeMap<u32, BTreeMap<u32, u64>>,
    pub species_channels: BTreeSet<sim_recognition::ChannelKind>,
    pub species_manipulations: BTreeSet<sim_species::ManipulationKind>,
    pub species_baseline: BTreeSet<u32>,
    pub has_magnetosphere: bool,
    pub has_em_medium: bool,
}

/// Advance one tick of the run. Returns `Ok(true)` to continue,
/// `Ok(false)` if `rs.early_run_end` has been set (caller should
/// break the loop), or propagates emitter errors.
#[allow(clippy::too_many_lines)]
pub(crate) fn run_tick<E: Emitter>(
    rs: &mut RunState,
    cfg: &RunConfig,
    emitter: &mut E,
    tick: u64,
) -> Result<bool, E::Error> {
    // Sprint 5 Item 19: per-tick tidal-locking dynamics. Damp
    // each moon's orbital eccentricity at a rate driven by the
    // planet's locking_state: Synchronous damps fast, FreeRotator
    // slowly, Resonance not at all (gravitational forcing from
    // other bodies sustains a steady-state e). One macro-step's
    // worth of damping per civ-tick.
    //
    // P3.8: pass `planet.radius` so the damping rate `k` is
    // derived from the same `tidal_dimensional_calibration` as
    // the heating rate H, locking in `H = -dE_orbit/dt` energy
    // conservation.
    {
        let dt = Real::ONE;
        let locking = rs.planet.locking_state;
        let r = rs.planet.radius;
        for moon in &mut rs.planet.moons {
            sim_world::step_eccentricity_damping(r, moon, locking, dt);
        }
    }
    let prev_state_for_measurements = phases::physics_phase(
        emitter,
        tick,
        &mut rs.state,
        &mut rs.orch_state,
        cfg,
        &rs.laws,
        &rs.civs,
    )?;
    let firings = phases::recognition_phase(
        emitter,
        tick,
        &rs.state,
        &rs.recognition,
        &rs.planet_ctx,
        &rs.species,
        &rs.civs,
        &rs.nomad_pops,
        &mut rs.nomad_observations,
    )?;
    let hypothesis_events = phases::cohort_and_figure_phase(
        emitter,
        tick,
        &rs.state,
        &prev_state_for_measurements,
        &mut rs.civs,
        &rs.species_baseline,
        &firings,
    )?;
    phases::discovery_emission_phase(
        emitter,
        tick,
        &rs.recognition,
        &mut rs.species,
        &mut rs.civs,
        &hypothesis_events,
        &mut rs.total_confirmed_relations,
        &mut rs.total_refinements,
        &rs.state,
        &rs.planet,
    )?;

    phases::tech_unlock_phase(
        emitter,
        tick,
        &rs.planet,
        &rs.recognition,
        &rs.species_channels,
        &rs.species_manipulations,
        &rs.species_baseline,
        &rs.species_modality_kinds,
        rs.has_magnetosphere,
        rs.has_em_medium,
        &mut rs.civs,
        rs.total_confirmed_relations,
        &mut rs.total_tech_unlocks,
        &rs.state,
    )?;

    // Per-tick resource consumption: unlocked tools with
    // burnable resource_prereqs draw down their substance
    // across territory. Mass-conservative against the
    // chemistry layer (mirrors combustion's
    // fuel + oxidiser → ash). Renewable (Fuel) recovers via
    // the regrowth reaction; Fossil monotonically depletes.
    phases::resource_consumption_phase(&mut rs.state, &rs.civs);

    // P0.5: read the live `PlanetEcosystem::tier_biomass(0)`
    // reading and thread it into the per-civ population step.
    // The ecosystem step itself runs later in the tick (after
    // catastrophes); the read here picks up the value left by
    // the *previous* tick's step. That's the right semantics —
    // population dynamics at tick N respond to the producer
    // pool the ecosystem produced through tick N-1, the same
    // way they respond to the *previous* tick's temperature
    // and substance fields. Tick 0 reads the freshly-sampled
    // ecosystem's starting biomass, before any step has run.
    let producer_biomass = rs.ecosystem.tier_biomass(0);
    phases::population_phase(
        emitter,
        tick,
        &rs.state,
        &rs.planet,
        &rs.species,
        &mut rs.civs,
        &mut rs.nomad_pops,
        producer_biomass,
    )?;
    // P0.5: emit per-civ ecological-resilience snapshots after
    // the population step has cached each civ's
    // `ecological_resilience` from the producer biomass
    // reading. Gate on the `RESILIENCE_EMIT_DELTA_FLOOR`
    // threshold (currently 0.05) so per-tick microdrift stays
    // out of the log.
    let resilience_threshold = Real::from_ratio(
        sim_civ::RESILIENCE_EMIT_DELTA_FLOOR.0,
        sim_civ::RESILIENCE_EMIT_DELTA_FLOOR.1,
    );
    let mut resilience_events: Vec<protocol::CivResilienceTick> = Vec::new();
    for civ in rs.civs.iter_mut().filter(|c| c.is_active()) {
        if (civ.ecological_resilience - civ.last_emitted_resilience).abs() >= resilience_threshold {
            resilience_events.push(protocol::CivResilienceTick {
                tick,
                civ_id: civ.id,
                resilience_q32: civ.ecological_resilience.raw().to_bits(),
                producer_biomass_q32: civ.producer_biomass.raw().to_bits(),
                previous_q32: civ.last_emitted_resilience.raw().to_bits(),
            });
            civ.last_emitted_resilience = civ.ecological_resilience;
        }
    }
    for ev in resilience_events {
        emitter.emit(&Event::CivResilienceTick(ev))?;
    }

    // Collapse + catastrophe per civ; founding
    // checks once after, with concurrent and sequential
    // triggers.
    emitter.emit(&Event::Tick(TickEvent {
        tick,
        phase: Phase::CivLifecycle,
    }))?;
    civ_lifecycle_step(rs, emitter, tick)?;

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
    let solar_irradiance = rs.planet.stellar_luminosity;
    let extinct_events =
        rs.ecosystem
            .step_with_biogeochem_at_tick(&mut rs.state, solar_irradiance, tick);
    for ev in extinct_events {
        // P0.1: open a post-extinction adaptive-radiation window
        // on the speciation tracker so the post-extinction
        // multiplier kicks in on subsequent ticks. The window
        // closes naturally after `POST_EXTINCTION_BOOST_TICKS`.
        rs.speciation_tracker.register_extinction_event(tick);
        emitter.emit(&Event::SpeciesExtinct(ev))?;
    }
    // Speciation + HGT. See `lib.rs` original for full rationale.
    let cosmic_mult = rs.state.cosmic_ray_ground_flux();
    let speciation_results = step_speciation(
        tick,
        &rs.ecosystem.species,
        &rs.species_registry,
        &mut rs.ecosystem.interactions,
        &mut rs.speciation_tracker,
        cosmic_mult,
    );
    for (daughter, event) in speciation_results {
        rs.species_registry
            .insert(SpeciesId(event.daughter_id), daughter);
        emitter.emit(&Event::SpeciationOccurred(event))?;
    }
    let hgt_local = LocalConditions::earth_surface();
    let hgt_events = step_hgt(
        &mut rs.species_registry,
        tick,
        cfg.seed,
        cosmic_mult,
        hgt_local,
    );
    for ev in hgt_events {
        emitter.emit(&Event::HorizontalGeneTransfer(ev))?;
    }

    // Catastrophe check on every active civ.
    catastrophe_step(rs, emitter, tick)?;

    phases::cultural_drift_phase(emitter, tick, &mut rs.civs)?;
    phases::culture_flip_phase(emitter, tick, &rs.state, &rs.planet, &mut rs.civs)?;

    // Founding check (v2 — two triggers OR-combined).
    if rs.civs.iter().all(|c| !c.is_active()) {
        stateless_refound_step(rs, cfg, emitter, tick)?;
    }

    // M5: concurrent breakaway.
    breakaway_step(rs, cfg, emitter, tick)?;

    // M5 + tech-gated contact: emit CivContact for any newly
    // co-existing pair that's also reachable.
    let active_ids: Vec<u32> = rs
        .civs
        .iter()
        .filter(|c| c.is_active())
        .map(|c| c.id)
        .collect();
    contact_and_trade_step(rs, emitter, tick, &active_ids)?;

    // Q-war conflict + cross-civ diffusion.
    if active_ids.len() >= 2 {
        if tick.is_multiple_of(conflict::CONFLICT_CHECK_TICKS) {
            war_and_alliance_step(rs, emitter, tick, &active_ids)?;
        }
        if tick.is_multiple_of(100) {
            knowledge_diffusion_step(rs, emitter, tick, &active_ids)?;
        }
    }

    // Nomadic species pool.
    let claim_union: BTreeSet<u32> = rs
        .civs
        .iter()
        .filter(|c| c.is_active())
        .flat_map(|c| c.claimed_cells.iter().copied())
        .collect();
    nomads::step_growth(
        &mut rs.nomad_pops,
        &rs.state,
        &rs.planet,
        rs.species.habitat,
        &rs.nomad_observations,
        rs.species.cognition,
        rs.species.sociality,
        rs.species.lifespan_years,
        &claim_union,
    );
    nomads::ambient_emergence(
        &mut rs.nomad_pops,
        &rs.state,
        &rs.planet,
        rs.species.habitat,
        &claim_union,
        tick,
    );

    // Emergent civ founding from saturated nomadic clusters.
    nomads::update_pressure_streak(
        &mut rs.nomad_pressure_streak,
        &rs.nomad_pops,
        &rs.state,
        &rs.planet,
        rs.species.habitat,
    );
    if tick >= rs.last_emergent_tick + nomads::EMERGENT_FOUNDING_COOLDOWN_TICKS {
        emergent_founding_step(rs, cfg, emitter, tick, &claim_union)?;
    }
    emit_nomads_changed(emitter, tick, &rs.nomad_pops)?;

    emitter.emit(&Event::Tick(TickEvent {
        tick,
        phase: Phase::TickEnd,
    }))?;

    // Species-level run-end checks.
    let any_active = rs.civs.iter().any(Civ::is_active);
    let civ_pop: sim_arith::Pop = rs
        .civs
        .iter()
        .map(|c| c.cohort.total())
        .fold(sim_arith::Pop::ZERO, |acc, x| acc + x);
    let nomad_pop: Real = rs
        .nomad_pops
        .values()
        .copied()
        .fold(Real::ZERO, |acc, x| acc + x);
    let total_pop: sim_arith::Pop = civ_pop + sim_arith::Pop::from_real(nomad_pop);

    if tick.is_multiple_of(SNAPSHOT_INTERVAL_TICKS) {
        let active_civ_ids: Vec<u32> = rs
            .civs
            .iter()
            .filter(|c| c.is_active())
            .map(|c| c.id)
            .collect();
        let collapsed_civ_ids: Vec<u32> = rs
            .civs
            .iter()
            .filter(|c| c.collapsed_tick.is_some())
            .map(|c| c.id)
            .collect();
        emitter.emit(&Event::Snapshot(protocol::Snapshot {
            tick,
            active_civ_ids,
            collapsed_civ_ids,
            total_population_q32: total_pop.raw().to_bits(),
            total_confirmed_relations: rs.total_confirmed_relations,
            total_refinements: rs.total_refinements,
            total_catastrophes: rs.total_catastrophes,
            total_tech_unlocks: rs.total_tech_unlocks,
            total_knowledge_transmissions: rs.total_knowledge_transmissions,
            total_knowledge_diffusions: rs.total_knowledge_diffusions,
        }))?;
    }
    let extinction_floor =
        sim_civ::founding_min_population(rs.planet.biosphere, rs.species.cognition)
            / Real::from_int(2);
    if !any_active && total_pop <= extinction_floor {
        rs.early_run_end = Some((tick, "species_extinction"));
        return Ok(false);
    }
    if any_active {
        rs.ticks_without_active_civ = 0;
    } else {
        rs.ticks_without_active_civ += 1;
        if rs.ticks_without_active_civ >= STAGNATION_THRESHOLD_TICKS {
            rs.early_run_end = Some((tick, "stagnation"));
            return Ok(false);
        }
    }

    // Transcendence run-end.
    let any_civ_at_tier5 = rs.civs.iter().any(|c| {
        tech::ToolKind::TIER_FIVE
            .iter()
            .all(|t| c.unlocked_tools.contains(t))
    });
    if any_civ_at_tier5 && rs.first_tier5_complete_tick.is_none() {
        rs.first_tier5_complete_tick = Some(tick);
    }
    if let Some(first) = rs.first_tier5_complete_tick {
        if tick.saturating_sub(first) >= TRANSCENDENCE_SUSTAINED_TICKS {
            // Resolve the divergent archetype endpoint for the civ that
            // crossed the threshold. The realized archetype is read from
            // its tool roster (a tier-5 roster is a strong lever signal);
            // different levers reach different fates.
            if let Some(civ) = rs.civs.iter().find(|c| {
                tech::ToolKind::TIER_FIVE
                    .iter()
                    .all(|t| c.unlocked_tools.contains(t))
            }) {
                let tools: Vec<tech::ToolKind> = civ.unlocked_tools.iter().copied().collect();
                let profile =
                    sim_civ::archetype::classify_realized(&rs.planet, &rs.species, &[], &tools);
                let endpoint = sim_civ::archetype::endpoint_for(&profile);
                emitter.emit(&Event::ArchetypeEndpoint(protocol::ArchetypeEndpoint {
                    tick,
                    civ_id: civ.id,
                    civ_name: civ.name.clone(),
                    label: profile.label.name(),
                    dominant_lever: profile.label.dominant_lever().name().to_string(),
                    cognition_mode: profile.cognition.name().to_string(),
                    endpoint_mode: endpoint.mode.to_string(),
                    description: endpoint.description,
                }))?;
            }
            rs.early_run_end = Some((tick, "transcendence"));
            return Ok(false);
        }
    }
    Ok(true)
}

