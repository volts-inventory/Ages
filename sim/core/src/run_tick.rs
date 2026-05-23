//! Per-tick body extracted from `run()`. `RunState` carries the
//! mutable run state across ticks; `run_tick()` advances one tick.
//!
//! The split is mechanical: every `let mut` defined inside the
//! original `for tick in ...` loop stays inline here, and every
//! `let mut` defined *before* the loop in the old `run()` becomes
//! a `RunState` field. Public surface (`run()`, `RunConfig`) is
//! unchanged; this module is purely an internal restructuring.

use crate::constants::{
    SNAPSHOT_INTERVAL_TICKS, STAGNATION_THRESHOLD_TICKS, TRANSCENDENCE_SUSTAINED_TICKS,
};
use crate::contact;
use crate::events::{claimed_cells_for_event, figure_born_event};
use crate::laws::Laws;
use crate::nomads;
use crate::phases;
use crate::setup::{emit_nomads_changed, emit_species_drift_if_meaningful};
use crate::territory::{
    compute_territory, pick_distant_habitable_cell, pick_habitable_cell, target_cell_count,
};
use crate::RunConfig;
use protocol::{
    AllianceDissolveReason, AllianceDissolved, AllianceFormed, CatastropheFired, CivCollapsed,
    CivContact, CivFounded, CivTerritoryChanged, ConflictResolved, Event, KnowledgeDiffused,
    KnowledgeTransmitted, PeaceConcluded, PeaceReason, Phase, TickEvent, WarDeclared,
};
use sim_arith::Real;
use sim_civ::{catastrophe, conflict, cosmology, tech, transmission, Civ};
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
            rs.early_run_end = Some((tick, "transcendence"));
            return Ok(false);
        }
    }
    Ok(true)
}

/// Per-tick civ-lifecycle step: collapse checks, cohesion / life-
/// expectancy / surplus deltas, and the emitter calls for each.
fn civ_lifecycle_step<E: Emitter>(
    rs: &mut RunState,
    emitter: &mut E,
    tick: u64,
) -> Result<(), E::Error> {
    let mut collapse_events: Vec<CivCollapsed> = Vec::new();
    let mut cohesion_events: Vec<protocol::CohesionShifted> = Vec::new();
    let mut life_expectancy_events: Vec<protocol::CivLifeExpectancyChanged> = Vec::new();
    let mut surplus_events: Vec<protocol::CivSurplusChanged> = Vec::new();
    let mut at_war_counts: BTreeMap<u32, u64> = BTreeMap::new();
    for (a, b) in rs.war_state.keys() {
        *at_war_counts.entry(*a).or_insert(0) += 1;
        *at_war_counts.entry(*b).or_insert(0) += 1;
    }
    for civ in rs.civs.iter_mut().filter(|c| c.is_active()) {
        if let Some(reason) = civ.check_collapse_with_terrain(tick, &rs.state, &rs.planet) {
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
            civ.apply_religion_push(&sim_civ::push_for_civ_collapsed(), Real::ONE);
            collapse_events.push(CivCollapsed {
                tick,
                civ_id,
                reason: reason.tag().to_string(),
                final_population_q32: final_pop_q32,
                final_figure_count: final_figs,
            });
        }
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
        let pop = civ.aggregate_population();
        let cap = civ.carrying_capacity_with_terrain(&rs.state, &rs.planet);
        let utilisation = if cap.raw().to_num::<i64>() > 0 {
            let pop_r = pop.to_real_nonneg();
            let cap_r = cap.to_real_nonneg().max(Real::ONE);
            (pop_r / cap_r).clamp01()
        } else {
            sim_arith::Real::ZERO
        };
        let at_war_count = at_war_counts.get(&civ.id).copied().unwrap_or(0);
        sim_civ::step_surplus(civ, utilisation, at_war_count);
        let emit_floor = sim_arith::Real::from_int(sim_civ::SURPLUS_EMIT_DELTA_FLOOR);
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
        rs.last_collapse_tick = Some(tick);
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
    Ok(())
}

fn catastrophe_step<E: Emitter>(
    rs: &mut RunState,
    emitter: &mut E,
    tick: u64,
) -> Result<(), E::Error> {
    let mut cat_events: Vec<(u32, catastrophe::CatastropheRecord)> = Vec::new();
    for civ in rs.civs.iter_mut().filter(|c| c.is_active()) {
        let civ_id = civ.id;
        if let Some(rec) = catastrophe::check_and_apply(
            civ,
            &mut rs.state,
            &rs.planet,
            &rs.species,
            tick,
            Some(&mut rs.ecosystem),
        ) {
            civ.record_catastrophe_selection_bias(rec.kind, rec.fraction_lost);
            sim_civ::drain_surplus_on_catastrophe(civ);
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
        rs.total_catastrophes = rs.total_catastrophes.saturating_add(1);
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn stateless_refound_step<E: Emitter>(
    rs: &mut RunState,
    cfg: &RunConfig,
    emitter: &mut E,
    tick: u64,
) -> Result<(), E::Error> {
    let parent_collapse = rs.last_collapse_tick;
    let elapsed_collapse = parent_collapse.map(|t| tick.saturating_sub(t));
    let metabolism = rs.planet.metabolic_substrate.metabolism();
    let dark_age_min =
        sim_civ::streak_ticks_for_metabolism(sim_civ::FOUNDING_MIN_DARK_AGE_TICKS, metabolism);
    let remnant_window =
        sim_civ::streak_ticks_for_metabolism(sim_civ::RECENT_REMNANT_WINDOW_TICKS, metabolism);
    let v1_eligible =
        elapsed_collapse.is_some_and(|e| (dark_age_min..=remnant_window).contains(&e));
    let probe_civ = Civ::with_species(
        rs.next_civ_id,
        tick,
        sim_arith::Pop::from_int(50),
        rs.species.cognition,
        rs.species.seed,
        &rs.species_modality_kinds,
        rs.species.initial_cosmology,
    );
    let charismatic_present = probe_civ
        .figures
        .iter()
        .any(|f| f.charisma >= Real::from_ratio(8, 10));
    let recent_catastrophe = rs
        .civs
        .iter()
        .filter_map(|c| c.last_catastrophe_tick)
        .max()
        .is_some_and(|t| tick.saturating_sub(t) <= 100);
    let v2_eligible = recent_catastrophe && charismatic_present;
    if !(v1_eligible || v2_eligible) {
        return Ok(());
    }
    let stateless_total = rs
        .civs
        .iter()
        .filter(|c| c.cohort.civ_membership.is_none())
        .map(|c| c.cohort.total())
        .fold(sim_arith::Pop::ZERO, |a, b| a + b);
    let founding_floor =
        sim_civ::founding_min_population(rs.planet.biosphere, rs.species.cognition);
    if stateless_total < founding_floor {
        return Ok(());
    }
    let parent_id = rs
        .civs
        .iter()
        .rev()
        .find(|c| c.collapsed_tick.is_some())
        .map_or(0, |c| c.id);
    let parent_centroid = rs
        .civs
        .iter()
        .find(|c| c.id == parent_id)
        .map(|c| c.territory_centroid);
    for c in &mut rs.civs {
        if c.cohort.civ_membership.is_none() {
            c.cohort.scale_in_place(Real::ZERO);
        }
    }
    let stateless_cohort = sim_civ::Cohort::with_civ(stateless_total, rs.next_civ_id);
    let mut new_civ = Civ::refound_from_stateless(
        rs.next_civ_id,
        tick,
        stateless_cohort,
        rs.species.cognition,
        rs.species.seed,
        &rs.species_modality_kinds,
        parent_id,
    );
    new_civ.name = sim_civ::civ_name_from_seed(cfg.seed, rs.next_civ_id);
    if let Some(parent_civ) = rs.civs.iter().find(|c| c.id == parent_id) {
        new_civ.inherit_species_drift_with_environment(
            parent_civ,
            rs.planet.seed,
            rs.planet.metabolic_substrate.metabolism(),
            rs.planet.biosphere,
        );
        new_civ.inherit_lineage_from(parent_civ);
    }
    new_civ.dynamics = sim_civ::dynamics_for_civ(&new_civ, &rs.species, &rs.planet);
    new_civ.configure_substrate_with_topology(
        rs.species.habitat,
        new_civ.effective_cognition(&rs.species),
        new_civ.effective_sociality(&rs.species),
        rs.planet.metabolic_substrate.metabolism(),
        rs.state.grid().width().saturating_mul(rs.state.grid().height()),
        rs.species.cognition_topology,
    );
    new_civ.configure_lifecycle_state(&rs.species.lifecycle);
    let occupied: BTreeSet<u32> = rs
        .civs
        .iter()
        .flat_map(|c| c.claimed_cells.iter().copied())
        .collect();
    new_civ.territory_centroid = if occupied.is_empty() {
        pick_habitable_cell(
            new_civ.territory_centroid,
            rs.state.grid(),
            &rs.state,
            &rs.planet,
            &new_civ,
            rs.species.habitat,
        )
    } else {
        pick_distant_habitable_cell(
            new_civ.territory_centroid,
            rs.state.grid(),
            &rs.state,
            &rs.planet,
            &new_civ,
            rs.species.habitat,
            &occupied,
        )
    };
    let target = target_cell_count(&new_civ, rs.state.grid().n_cells());
    let cells = compute_territory(
        new_civ.territory_centroid,
        target,
        rs.state.grid(),
        &rs.state,
        &rs.planet,
        &new_civ,
        rs.species.habitat,
        &BTreeSet::new(),
    );
    let _ = parent_centroid;
    new_civ.claim_cells(&cells);
    new_civ.refresh_available_forms_with_modalities(
        &rs.species_baseline,
        &rs.recognition,
        &rs.species_modality_kinds,
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
                .cell_capacity(&rs.state, c, tick, &rs.planet)
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

    let decay_ticks = rs.species.transmission_decay_ticks();
    let comm_speed = rs.species.communication_speed_multiplier();
    let (transmissions, mythologizations) =
        if let Some(parent_civ) = rs.civs.iter().find(|c| c.id == parent_id) {
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
        rs.total_knowledge_transmissions = rs.total_knowledge_transmissions.saturating_add(1);
    }
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

    rs.civs.push(new_civ);
    rs.next_civ_id += 1;
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn breakaway_step<E: Emitter>(
    rs: &mut RunState,
    cfg: &RunConfig,
    emitter: &mut E,
    tick: u64,
) -> Result<(), E::Error> {
    let breakaway_cooldown_ok = rs
        .last_breakaway_tick
        .is_none_or(|t| tick.saturating_sub(t) >= 500);
    let cohesion_breakaway_streak_metabolised = sim_civ::streak_ticks_for_metabolism(
        sim_civ::COHESION_BREAKAWAY_STREAK_TICKS,
        rs.planet.metabolic_substrate.metabolism(),
    );
    let cohesion_parent_id: Option<u32> = if breakaway_cooldown_ok {
        rs.civs
            .iter()
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
            rs.civs
                .iter()
                .find(|c| c.is_active() && c.cosmology.dogmatism() > Real::from_ratio(7, 10))
                .map(|c| c.id)
        } else {
            None
        };
    let breakaway_pick: Option<(u32, (i64, i64), bool)> = cohesion_parent_id
        .map(|id| (id, sim_civ::COHESION_BREAKAWAY_SHARE, true))
        .or_else(|| dogmatic_parent_id.map(|id| (id, (1, 2), false)));
    let Some((parent_id, (share_num, share_den), is_cohesion_breakaway)) = breakaway_pick else {
        return Ok(());
    };
    let probe = Civ::with_species(
        rs.next_civ_id,
        tick,
        sim_arith::Pop::from_int(50),
        rs.species.cognition,
        rs.species.seed,
        &rs.species_modality_kinds,
        rs.species.initial_cosmology,
    );
    let charismatic = probe
        .figures
        .iter()
        .any(|f| f.charisma >= Real::from_ratio(7, 10));
    if !charismatic {
        return Ok(());
    }
    let parent_idx = rs
        .civs
        .iter()
        .position(|c| c.id == parent_id)
        .expect("dogmatic parent in civs");
    let parent_aggregate = rs.civs[parent_idx].cohort.total();
    let share_ratio = Real::from_ratio(share_num, share_den);
    let breakaway_share = parent_aggregate * share_ratio;
    let breakaway_floor =
        sim_civ::founding_min_population(rs.planet.biosphere, rs.species.cognition)
            / Real::from_int(2);
    if breakaway_share < breakaway_floor {
        return Ok(());
    }
    let parent_centroid = rs.civs[parent_idx].territory_centroid;
    let mut border_candidates: Vec<u32> = Vec::new();
    for &cell in &rs.civs[parent_idx].claimed_cells {
        let axial = rs.state.grid().axial_of(sim_physics::CellId(cell));
        let has_non_parent_nbr = rs
            .state
            .grid()
            .neighbours(axial)
            .iter()
            .any(|nbr| !rs.civs[parent_idx].claimed_cells.contains(&nbr.0));
        if has_non_parent_nbr {
            border_candidates.push(cell);
        }
    }
    border_candidates.sort_unstable();
    let live_centroids: BTreeSet<u32> = rs
        .civs
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
        return Ok(());
    };
    let mut seized_cohort = sim_civ::Cohort::empty_with_civ(rs.next_civ_id);
    rs.civs[parent_idx].claimed_cells.remove(&centroid);
    if let Some(c) = rs.civs[parent_idx].region_cohorts.remove(&centroid) {
        seized_cohort.merge_in(&c);
    }
    rs.civs[parent_idx].resync_aggregate_from_regions();
    let seized_cells: Vec<u32> = vec![centroid];
    let parent_aggregate_post = rs.civs[parent_idx].cohort.total();
    let breakaway_share = parent_aggregate_post * share_ratio;
    let keep_share = Real::ONE - share_ratio;
    rs.civs[parent_idx].cohort.scale_in_place(keep_share);
    for c in rs.civs[parent_idx].region_cohorts.values_mut() {
        c.scale_in_place(keep_share);
    }
    let _ = parent_centroid;
    if is_cohesion_breakaway {
        let recovery = Real::from_ratio(
            sim_civ::COHESION_PARENT_RECOVERY.0,
            sim_civ::COHESION_PARENT_RECOVERY.1,
        );
        rs.civs[parent_idx].cohesion = (rs.civs[parent_idx].cohesion + recovery).clamp01();
        rs.civs[parent_idx].cohesion_breakaway_streak = 0;
    }
    if !seized_cells.is_empty() {
        let parent_civ_ref = &rs.civs[parent_idx];
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
                    .cell_capacity(&rs.state, c, tick, &rs.planet)
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
        rs.civs[parent_idx].last_territory_emit_tick = tick;
    }
    let mut breakaway_cohort = sim_civ::Cohort::with_civ(breakaway_share, rs.next_civ_id);
    breakaway_cohort.merge_in(&seized_cohort);
    breakaway_cohort.civ_membership = Some(rs.next_civ_id);
    let mut new_civ = Civ::refound_from_stateless(
        rs.next_civ_id,
        tick,
        breakaway_cohort,
        rs.species.cognition,
        rs.species.seed,
        &rs.species_modality_kinds,
        parent_id,
    );
    new_civ.name = sim_civ::civ_name_from_seed(cfg.seed, rs.next_civ_id);
    {
        let parent_civ = &rs.civs[parent_idx];
        new_civ.inherit_species_drift_with_environment(
            parent_civ,
            rs.planet.seed,
            rs.planet.metabolic_substrate.metabolism(),
            rs.planet.biosphere,
        );
        new_civ.inherit_lineage_from(parent_civ);
    }
    if is_cohesion_breakaway {
        new_civ.cohesion = Real::from_ratio(
            sim_civ::COHESION_BREAKAWAY_INITIAL.0,
            sim_civ::COHESION_BREAKAWAY_INITIAL.1,
        );
        new_civ.last_emitted_cohesion = new_civ.cohesion;
    }
    new_civ.dynamics = sim_civ::dynamics_for_civ(&new_civ, &rs.species, &rs.planet);
    new_civ.configure_substrate_with_topology(
        rs.species.habitat,
        new_civ.effective_cognition(&rs.species),
        new_civ.effective_sociality(&rs.species),
        rs.planet.metabolic_substrate.metabolism(),
        rs.state.grid().width().saturating_mul(rs.state.grid().height()),
        rs.species.cognition_topology,
    );
    new_civ.configure_lifecycle_state(&rs.species.lifecycle);
    new_civ.territory_centroid = centroid;
    let target = target_cell_count(&new_civ, rs.state.grid().n_cells());
    let occupied_post: BTreeSet<u32> = rs
        .civs
        .iter()
        .flat_map(|c| c.claimed_cells.iter().copied())
        .collect();
    let cells = compute_territory(
        new_civ.territory_centroid,
        target,
        rs.state.grid(),
        &rs.state,
        &rs.planet,
        &new_civ,
        rs.species.habitat,
        &occupied_post,
    );
    let _ = parent_centroid;
    new_civ.claim_cells(&cells);
    new_civ.refresh_available_forms_with_modalities(
        &rs.species_baseline,
        &rs.recognition,
        &rs.species_modality_kinds,
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
                .cell_capacity(&rs.state, c, tick, &rs.planet)
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

    let (transmissions, mythologizations) = {
        let parent_civ = &rs.civs[parent_idx];
        let decay_ticks = rs.species.transmission_decay_ticks();
        let comm_speed = rs.species.communication_speed_multiplier();
        transmission::transmit_from_parent(&mut new_civ, parent_civ, tick, decay_ticks, comm_speed)
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
        rs.total_knowledge_transmissions = rs.total_knowledge_transmissions.saturating_add(1);
    }
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
    rs.civs.push(new_civ);
    rs.next_civ_id += 1;
    rs.last_breakaway_tick = Some(tick);
    let pair = if parent_id < new_id {
        (parent_id, new_id)
    } else {
        (new_id, parent_id)
    };
    if !rs.emitted_contacts.contains(&pair) {
        let parent_civ = rs
            .civs
            .iter()
            .find(|c| c.id == parent_id)
            .expect("parent civ");
        let child_civ = rs
            .civs
            .iter()
            .find(|c| c.id == new_id)
            .expect("just-pushed breakaway civ");
        if contact::civs_in_contact(parent_civ, child_civ, rs.species.habitat, &rs.state) {
            rs.emitted_contacts.insert(pair);
            if let Some(parent_mut) = rs.civs.iter_mut().find(|c| c.id == parent_id) {
                parent_mut.contact_history.insert(new_id);
            }
            if let Some(child_mut) = rs.civs.iter_mut().find(|c| c.id == new_id) {
                child_mut.contact_history.insert(parent_id);
            }
            emitter.emit(&Event::CivContact(CivContact {
                tick,
                civ_a: pair.0,
                civ_b: pair.1,
            }))?;
        }
    }
    Ok(())
}

fn contact_and_trade_step<E: Emitter>(
    rs: &mut RunState,
    emitter: &mut E,
    tick: u64,
    active_ids: &[u32],
) -> Result<(), E::Error> {
    if tick.is_multiple_of(contact::CONTACT_CHECK_TICKS) {
        for i in 0..active_ids.len() {
            for j in (i + 1)..active_ids.len() {
                let pair = if active_ids[i] < active_ids[j] {
                    (active_ids[i], active_ids[j])
                } else {
                    (active_ids[j], active_ids[i])
                };
                if rs.emitted_contacts.contains(&pair) {
                    continue;
                }
                let civ_a = rs.civs.iter().find(|c| c.id == pair.0).expect("active civ");
                let civ_b = rs.civs.iter().find(|c| c.id == pair.1).expect("active civ");
                if !contact::civs_in_contact(civ_a, civ_b, rs.species.habitat, &rs.state) {
                    continue;
                }
                rs.emitted_contacts.insert(pair);
                if let Some(a_mut) = rs.civs.iter_mut().find(|c| c.id == pair.0) {
                    a_mut.contact_history.insert(pair.1);
                }
                if let Some(b_mut) = rs.civs.iter_mut().find(|c| c.id == pair.1) {
                    b_mut.contact_history.insert(pair.0);
                }
                let civ_a = rs.civs.iter().find(|c| c.id == pair.0).expect("active civ");
                let civ_b = rs.civs.iter().find(|c| c.id == pair.1).expect("active civ");
                emitter.emit(&Event::CivContact(CivContact {
                    tick,
                    civ_a: pair.0,
                    civ_b: pair.1,
                }))?;
                if conflict::is_peaceful_pair(civ_a, civ_b)
                    && !rs.trade_routes.contains_key(&pair)
                {
                    rs.trade_routes.insert(pair, tick);
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

    // M8: per-tick trade flow over open routes.
    let active_set_now: BTreeSet<u32> = rs
        .civs
        .iter()
        .filter(|c| c.is_active())
        .map(|c| c.id)
        .collect();
    let stale_routes: Vec<(u32, u32)> = rs
        .trade_routes
        .keys()
        .copied()
        .filter(|(a, b)| !active_set_now.contains(a) || !active_set_now.contains(b))
        .collect();
    for pair in stale_routes {
        rs.trade_routes.remove(&pair);
        emitter.emit(&Event::TradeRouteClosed(protocol::TradeRouteClosed {
            tick,
            civ_a: pair.0,
            civ_b: pair.1,
            reason: "civ_collapsed".to_string(),
        }))?;
    }
    let route_pairs: Vec<(u32, u32)> = rs.trade_routes.keys().copied().collect();
    for (a_id, b_id) in route_pairs {
        let pos_a = rs.civs.iter().position(|c| c.id == a_id);
        let pos_b = rs.civs.iter().position(|c| c.id == b_id);
        if let (Some(pa), Some(pb)) = (pos_a, pos_b) {
            let (lo, hi) = if pa < pb { (pa, pb) } else { (pb, pa) };
            let (left, right) = rs.civs.split_at_mut(hi);
            let civ_lo = &mut left[lo];
            let civ_hi = &mut right[0];
            sim_civ::trade_flow_between(civ_lo, civ_hi);
        }
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn war_and_alliance_step<E: Emitter>(
    rs: &mut RunState,
    emitter: &mut E,
    tick: u64,
    active_ids: &[u32],
) -> Result<(), E::Error> {
    let mut peace_events: Vec<PeaceConcluded> = Vec::new();
    let active_set: BTreeSet<u32> = active_ids.iter().copied().collect();
    let stale_pairs: Vec<(u32, u32)> = rs
        .war_state
        .keys()
        .copied()
        .filter(|(a, b)| !active_set.contains(a) || !active_set.contains(b))
        .collect();
    for pair in stale_pairs {
        let started = rs.war_state.remove(&pair).unwrap_or(tick);
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
    let mut trade_close_events: Vec<protocol::TradeRouteClosed> = Vec::new();
    for i in 0..active_ids.len() {
        for j in (i + 1)..active_ids.len() {
            let civ_id_first = active_ids[i];
            let civ_id_second = active_ids[j];
            let Some(slot_first) = rs.civs.iter().position(|c| c.id == civ_id_first) else {
                continue;
            };
            let Some(slot_second) = rs.civs.iter().position(|c| c.id == civ_id_second) else {
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
            let already_at_war = rs.war_state.contains_key(&pair);

            let (left, right) = rs.civs.split_at_mut(hi);
            let civ_lo = &mut left[lo];
            let civ_hi = &mut right[0];

            if !rs.emitted_contacts.contains(&pair) {
                continue;
            }

            let overlap_empty = conflict::overlap(civ_lo, civ_hi).is_empty();
            if overlap_empty {
                if already_at_war {
                    let started = rs.war_state.remove(&pair).unwrap_or(tick);
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
                conflict::assess_pair(civ_lo, civ_hi, &rs.state, &rs.planet, tick)
            else {
                continue;
            };
            let decision = conflict::decide_war(already_at_war, assessment.belligerence);

            match decision {
                conflict::WarDecision::StayPeaceful => continue,
                conflict::WarDecision::ConcludePeace => {
                    let started = rs.war_state.remove(&pair).unwrap_or(tick);
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
                    rs.war_state.insert(pair, tick);
                    war_events.push(WarDeclared {
                        tick,
                        aggressor_civ_id: assessment.aggressor_id,
                        defender_civ_id: assessment.defender_id,
                        belligerence_q32: assessment.belligerence.raw().to_bits(),
                        drive_q32: assessment.drive.raw().to_bits(),
                        kinship_q32: assessment.kinship.raw().to_bits(),
                    });
                    if rs.trade_routes.remove(&pair).is_some() {
                        trade_close_events.push(protocol::TradeRouteClosed {
                            tick,
                            civ_a: pair.0,
                            civ_b: pair.1,
                            reason: "war_declared".to_string(),
                        });
                    }
                }
                conflict::WarDecision::ContinueWar => {}
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
                    let started = rs.war_state.remove(&pair).unwrap_or(tick);
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

    // PR4: Alliance formation + dissolution pass.
    let mut alliance_formed_events: Vec<AllianceFormed> = Vec::new();
    let mut alliance_dissolved_events: Vec<AllianceDissolved> = Vec::new();
    let at_war_pairs: BTreeSet<(u32, u32)> = rs.war_state.keys().copied().collect();

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
            let Some(civ_a_idx) = rs.civs.iter().position(|c| c.id == pair.0) else {
                continue;
            };
            let Some(civ_b_idx) = rs.civs.iter().position(|c| c.id == pair.1) else {
                continue;
            };
            let civ_a = &rs.civs[civ_a_idx];
            let civ_b = &rs.civs[civ_b_idx];
            let mutually_allied = civ_a.allied_with.contains(&civ_b.id)
                && civ_b.allied_with.contains(&civ_a.id);

            if mutually_allied {
                if conflict::alliance_drifted_apart(civ_a, civ_b) {
                    dissolutions.push((pair, AllianceDissolveReason::CosmologyDrift));
                    continue;
                }
                let a_wars: BTreeSet<u32> = at_war_pairs
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
                let b_wars: BTreeSet<u32> = at_war_pairs
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
                let misaligned = a_wars.iter().any(|t| !b_wars.contains(t))
                    || b_wars.iter().any(|t| !a_wars.contains(t));
                if misaligned {
                    dissolutions.push((pair, AllianceDissolveReason::WarMisalignment));
                    continue;
                }
                let cosmo_gap = conflict::cosmology_distance(civ_a, civ_b);
                let religion_gap = conflict::religion_distance(civ_a, civ_b);
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
                let post = conflict::step_alliance_trust(prior, cosmo_gap, religion_gap);
                trust_updates.push((pair, post));
                if post < Real::from(conflict::ALLIANCE_TRUST_FLOOR) {
                    dissolutions.push((pair, AllianceDissolveReason::TrustEroded));
                }
            } else {
                let at_war_now = at_war_pairs.contains(&pair);
                if conflict::propose_alliance(civ_a, civ_b, at_war_now, tick) {
                    formations.push(pair);
                }
            }
        }
    }

    let trust_initial = Real::from(conflict::ALLIANCE_TRUST_INITIAL);
    for (pair, post) in trust_updates {
        if let Some(civ) = rs.civs.iter_mut().find(|c| c.id == pair.0) {
            civ.alliance_trust.insert(pair.1, post);
        }
        if let Some(civ) = rs.civs.iter_mut().find(|c| c.id == pair.1) {
            civ.alliance_trust.insert(pair.0, post);
        }
    }
    for pair in formations {
        if let Some(civ) = rs.civs.iter_mut().find(|c| c.id == pair.0) {
            civ.allied_with.insert(pair.1);
            civ.alliance_trust.insert(pair.1, trust_initial);
        }
        if let Some(civ) = rs.civs.iter_mut().find(|c| c.id == pair.1) {
            civ.allied_with.insert(pair.0);
            civ.alliance_trust.insert(pair.0, trust_initial);
        }
        alliance_formed_events.push(AllianceFormed {
            tick,
            civ_a: pair.0,
            civ_b: pair.1,
        });
    }
    for (pair, reason) in dissolutions {
        if let Some(civ) = rs.civs.iter_mut().find(|c| c.id == pair.0) {
            civ.allied_with.remove(&pair.1);
            civ.alliance_trust.remove(&pair.1);
            civ.alliance_cooldown.insert(pair.1, tick);
        }
        if let Some(civ) = rs.civs.iter_mut().find(|c| c.id == pair.1) {
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
    Ok(())
}

fn knowledge_diffusion_step<E: Emitter>(
    rs: &mut RunState,
    emitter: &mut E,
    tick: u64,
    active_ids: &[u32],
) -> Result<(), E::Error> {
    let mut diffusions: Vec<KnowledgeDiffused> = Vec::new();
    for i in 0..active_ids.len() {
        for j in 0..active_ids.len() {
            if i == j {
                continue;
            }
            let source_civ_id = active_ids[i];
            let receiver_civ_id = active_ids[j];
            let Some(source_slot) = rs.civs.iter().position(|c| c.id == source_civ_id) else {
                continue;
            };
            let Some(receiver_slot) = rs.civs.iter().position(|c| c.id == receiver_civ_id) else {
                continue;
            };
            let src_idx = source_slot;
            let dst_idx = receiver_slot;
            let peaceful = {
                let s = &rs.civs[src_idx];
                let d = &rs.civs[dst_idx];
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
            let (left, right) = rs.civs.split_at_mut(hi);
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
                rs.species.communication_speed_multiplier(),
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
        rs.total_knowledge_diffusions = rs.total_knowledge_diffusions.saturating_add(1);
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn emergent_founding_step<E: Emitter>(
    rs: &mut RunState,
    cfg: &RunConfig,
    emitter: &mut E,
    tick: u64,
    claim_union: &BTreeSet<u32>,
) -> Result<(), E::Error> {
    let centroids: Vec<u32> = rs
        .civs
        .iter()
        .filter(|c| c.is_active())
        .map(|c| c.territory_centroid)
        .collect();
    let Some(emerge_cell) = nomads::scan_for_emergence(
        &rs.nomad_pops,
        &rs.nomad_observations,
        &rs.nomad_pressure_streak,
        rs.species.cognition,
        rs.species.sociality,
        &rs.state,
        &rs.planet,
        rs.species.habitat,
        &centroids,
        claim_union,
    ) else {
        return Ok(());
    };
    rs.last_emergent_tick = tick;
    let mut new_civ = Civ::with_species(
        rs.next_civ_id,
        tick,
        sim_arith::Pop::from_int(200),
        rs.species.cognition,
        rs.species.seed,
        &rs.species_modality_kinds,
        rs.species.initial_cosmology,
    );
    new_civ.name = sim_civ::civ_name_from_seed(cfg.seed, rs.next_civ_id);
    if let Some(parent_civ) = rs
        .civs
        .iter()
        .filter(|c| c.collapsed_tick.is_some())
        .max_by_key(|c| c.collapsed_tick.unwrap_or(0))
    {
        new_civ.inherit_species_drift_with_environment(
            parent_civ,
            rs.planet.seed,
            rs.planet.metabolic_substrate.metabolism(),
            rs.planet.biosphere,
        );
        new_civ.inherit_lineage_from(parent_civ);
    }
    new_civ.dynamics = sim_civ::dynamics_for_civ(&new_civ, &rs.species, &rs.planet);
    new_civ.configure_substrate_with_topology(
        rs.species.habitat,
        new_civ.effective_cognition(&rs.species),
        new_civ.effective_sociality(&rs.species),
        rs.planet.metabolic_substrate.metabolism(),
        rs.state.grid().width().saturating_mul(rs.state.grid().height()),
        rs.species.cognition_topology,
    );
    new_civ.configure_lifecycle_state(&rs.species.lifecycle);
    new_civ.territory_centroid = emerge_cell;
    let target = target_cell_count(&new_civ, rs.state.grid().n_cells());
    let cells = compute_territory(
        new_civ.territory_centroid,
        target,
        rs.state.grid(),
        &rs.state,
        &rs.planet,
        &new_civ,
        rs.species.habitat,
        claim_union,
    );
    let gained: Vec<u32> = cells.iter().copied().collect();
    new_civ.claim_cells(&cells);
    let founder_loss =
        sim_arith::Real::from_ratio(nomads::FOUNDING_ABSORB_LOSS.0, nomads::FOUNDING_ABSORB_LOSS.1);
    nomads::absorb_into_civ(
        &mut rs.nomad_pops,
        &mut new_civ,
        gained.clone(),
        &rs.species.biology,
        founder_loss,
    );
    let _drained = nomads::drain_observations_for_cells(&mut rs.nomad_observations, gained);
    new_civ.refresh_available_forms_with_modalities(
        &rs.species_baseline,
        &rs.recognition,
        &rs.species_modality_kinds,
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
                .cell_capacity(&rs.state, c, tick, &rs.planet)
                .raw()
                .to_bits()
        })
        .collect();
    emitter.emit(&Event::CivFounded(CivFounded {
        tick,
        civ_id: new_civ.id,
        parent_civ_id: None,
        name: new_civ.name.clone(),
        initial_population_q32: initial_pop_q32,
        founding_figure_count: band,
        claimed_cells: claimed,
        cell_capacities_q32: claimed_caps,
    }))?;
    new_civ.last_territory_emit_tick = tick;
    emit_species_drift_if_meaningful(emitter, &new_civ)?;
    rs.civs.push(new_civ);
    rs.next_civ_id += 1;
    Ok(())
}

