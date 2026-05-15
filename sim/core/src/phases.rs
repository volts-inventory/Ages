//! Per-tick phase helpers extracted from the `run()` tick loop.
//! Each fn corresponds to one stretch of the loop body — they take
//! exactly the locals they touch and emit the protocol events for
//! their phase. The tick loop in `lib.rs` calls them in fixed order;
//! see `PHASE_ORDER` for the canonical sequence.

use crate::events::{claimed_cells_for_event, relation_to_event};
use crate::laws::Laws;
use crate::nomads;
// `compute_territory` / `target_cell_count` still drive the
// founding code paths in `lib.rs`; the per-tick territory loop in
// this module now uses `expand_via_overflow` + `prune_empty_cells`
// instead and no longer imports them.
use crate::RunConfig;
use protocol::{
    CivTerritoryChanged, CosmologyShifted, Event, Phase, RecognitionFiring, RefinementConfirmed,
    RefinementProposed, RefinementRejected, ReligionShifted, TechUnlocked, TickEvent,
};
use sim_arith::Real;
use sim_civ::{cosmology, discovery::HypothesisEvent, tech, Civ};
use sim_events::Emitter;
use sim_physics::{integrate_civ_step, PhysicsState};
use sim_recognition::{ChannelKind, Firing, RecognitionLibrary};
use sim_species::{ManipulationKind, Species};
use sim_world::Planet;
use std::collections::{BTreeMap, BTreeSet};

/// Phase A: emit `TickStart` + `PhysicsIntegration` markers,
/// snapshot pre-integration state for `MeasurementChannel::TemporalDelta`
/// (snapshot pre-state), then run the physics-law integrator.
///
/// Before snapshotting prev-state, walk every active civ and
/// write any apparatus clamps into the cell-channel that apparatus
/// occupies. The snapshot then captures the clamped state so this
/// tick's `TemporalDelta` reads the relaxation response, and the
/// post-physics apparatus sampling (in `cohort_and_figure_phase`)
/// reads the cell's post-physics value — pairing `(clamp_value,
/// post_phys_value)` for the civ's hypothesizer.
pub(crate) fn physics_phase<E: Emitter>(
    emitter: &mut E,
    tick: u64,
    state: &mut PhysicsState,
    cfg: &RunConfig,
    laws: &Laws,
    civs: &[Civ],
) -> Result<PhysicsState, E::Error> {
    emitter.emit(&Event::Tick(TickEvent {
        tick,
        phase: Phase::TickStart,
    }))?;
    emitter.emit(&Event::Tick(TickEvent {
        tick,
        phase: Phase::PhysicsIntegration,
    }))?;
    // Write apparatus clamps into the physics state before
    // any per-tick integration runs.
    sim_civ::apparatus::write_apparatus_clamps(state, civs, tick);
    // Snapshot pre-integration state so this tick's
    // measurement observations can read `(curr - prev)` for
    // `MeasurementChannel::TemporalDelta`. The snapshot lives
    // for one tick — refreshed each pass.
    let prev_state_for_measurements = state.clone();
    integrate_civ_step(
        state,
        &cfg.orchestration,
        &laws.fluid,
        &laws.heat,
        &laws.em,
        &laws.chemistry,
        Some(&laws.radiation),
        Some(&laws.wind),
        Some(&laws.hydrology),
        Some(&laws.tides),
        Some(&laws.magnetism),
        Some(&laws.lorentz),
        Some(&laws.coriolis),
        Some(&laws.vertical),
    );
    Ok(prev_state_for_measurements)
}

/// Phase B: emit `PatternRecognition` marker, scan the planet
/// for recognition firings (authored templates + species-
/// discovered ones), accumulate nomad-cell observation pressure
/// (per-template observation counters), and emit one `Recognition` event per firing.
/// Returns the firings so subsequent phases (cohort observation,
/// hypothesis testing) can consume them.
#[allow(clippy::too_many_arguments)]
pub(crate) fn recognition_phase<E: Emitter>(
    emitter: &mut E,
    tick: u64,
    state: &PhysicsState,
    recognition: &RecognitionLibrary,
    planet_ctx: &sim_recognition::PlanetContext,
    species: &Species,
    civs: &[Civ],
    nomad_pops: &BTreeMap<u32, Real>,
    nomad_observations: &mut BTreeMap<u32, BTreeMap<u32, u64>>,
) -> Result<Vec<Firing>, E::Error> {
    emitter.emit(&Event::Tick(TickEvent {
        tick,
        phase: Phase::PatternRecognition,
    }))?;
    // Include any species-discovered templates in the
    // scan so emergent recognition fires alongside the authored
    // library. The discovered set grows across the run as civs
    // confirm `ThresholdStep` laws.
    let discovered_vec: Vec<sim_recognition::DiscoveredTemplate> =
        species.discovered_templates.values().cloned().collect();
    let firings = recognition.scan_with_discovered(&discovered_vec, state, tick, planet_ctx);
    // Accumulate nomadic tech from recognition firings
    // *before* emission / civ-pipeline processing. Each
    // firing whose template the species can perceive AND
    // whose cell has nomadic presence increments the cell's
    // tech accumulator by `cognition × sociality`. Civs draw
    // their own observations from the per-civ Hypothesizer
    // pipeline; this tech accumulator is for nomadic
    // pre-civ population only.
    let claim_union_for_tech: BTreeSet<u32> = civs
        .iter()
        .filter(|c| c.is_active())
        .flat_map(|c| c.claimed_cells.iter().copied())
        .collect();
    for firing in &firings {
        if !species.perceivable_templates.contains(&firing.template_id) {
            continue;
        }
        nomads::accumulate_observation(
            nomad_observations,
            nomad_pops,
            &claim_union_for_tech,
            firing.cell,
            firing.template_id,
        );
    }
    for firing in &firings {
        // Map back to the template name for the event payload.
        let name = recognition
            .templates
            .iter()
            .find(|t| t.id == firing.template_id)
            .map(|t| t.name.to_string())
            .unwrap_or_default();
        emitter.emit(&Event::Recognition(RecognitionFiring {
            tick,
            template_id: firing.template_id,
            template_name: name,
            cell: firing.cell,
        }))?;
    }
    Ok(firings)
}

/// Phases C-D-E: cohort-level observation pool + per-figure
/// observation + per-figure hypothesis stepping. Each active civ
/// folds the tick's perceivable firings into its summary counts
/// (taboo-attenuated), then samples physics state per figure,
/// then steps each figure's hypothesizer. Returns the
/// `(civ_id, figure_id, event)` tuples for the discovery / event-
/// emission phase that follows.
pub(crate) fn cohort_and_figure_phase<E: Emitter>(
    emitter: &mut E,
    tick: u64,
    state: &PhysicsState,
    prev_state_for_measurements: &PhysicsState,
    civs: &mut [Civ],
    species_baseline: &BTreeSet<u32>,
    firings: &[Firing],
) -> Result<Vec<(u32, u32, HypothesisEvent)>, E::Error> {
    // Cohort-level observation pool: the civ folds the
    // tick's recognition firings into its summary observation
    // counts. Sensorium gating: only firings whose
    // recognition channels intersect the species' modality vector
    // are perceivable; latent firings stay unobservable until
    // sensorium-extending tech lands.
    emitter.emit(&Event::Tick(TickEvent {
        tick,
        phase: Phase::CohortObservations,
    }))?;
    // M5: every active civ runs its own observation/step
    // pipeline this tick. Each civ has its own sensorium
    // (different unlocked tools) so perceivable firings are
    // computed per civ.
    let mut perceivable_per_civ: BTreeMap<u32, Vec<Firing>> = BTreeMap::new();
    for civ in civs.iter_mut().filter(|c| c.is_active()) {
        let p = civ.perceivable_firings(species_baseline, firings);
        civ.observe(&p);
        // Taboo-driven attenuation of observation accumulation.
        // A civ with high mystical + communitarian cosmology
        // ("taboo culture") credits fewer firings to its
        // template-counter, slowing its obs-pressure
        // threshold and so slowing its sensorium-tech path.
        // Uses a deterministic hash over (tick, civ.id,
        // template, cell) — no RNG threading, bit-exact.
        // Accumulator stride matters: `taboo_strength` near 1
        // skips most firings; near 0 credits all of them.
        let taboo_strength = civ.cosmology.mystical.clamp01();
        for f in &p {
            let hash = tick ^ u64::from(civ.id) ^ u64::from(f.template_id) ^ u64::from(f.cell);
            let bucket = hash % 100;
            let bucket_real =
                Real::from_int(i64::try_from(bucket).unwrap_or(0)) / Real::from_int(100);
            if bucket_real >= taboo_strength {
                *civ.firings_by_template.entry(f.template_id).or_insert(0) += 1;
            }
        }
        perceivable_per_civ.insert(civ.id, p);
    }

    emitter.emit(&Event::Tick(TickEvent {
        tick,
        phase: Phase::FigureObservations,
    }))?;
    for civ in civs.iter_mut().filter(|c| c.is_active()) {
        let p = perceivable_per_civ
            .get(&civ.id)
            .cloned()
            .unwrap_or_default();
        civ.observe_per_figure(state, Some(prev_state_for_measurements), &p);
        // Feed apparatus samples into the civ's first active
        // figure's hypothesizer. The apparatus is a civ-level
        // institution but produces samples one figure has to fit;
        // assigning them to the first active figure keeps the
        // attribution stable. A civ with no apparatus yet (the
        // tool hasn't unlocked) skips through the empty Vec.
        if !civ.apparatus_cells.is_empty() {
            let apparatus = civ.apparatus_cells.clone();
            if let Some(fig) = civ.figures.iter_mut().find(|f| f.retired_tick.is_none()) {
                sim_civ::apparatus::record_apparatus_samples(
                    state,
                    &mut fig.hypothesizer,
                    &apparatus,
                    tick,
                );
            }
        }
    }

    emitter.emit(&Event::Tick(TickEvent {
        tick,
        phase: Phase::HypothesisTesting,
    }))?;
    // M5: collect per-civ hypothesis events tagged with civ_id.
    let mut hypothesis_events: Vec<(u32, u32, HypothesisEvent)> = Vec::new();
    for civ in civs.iter_mut().filter(|c| c.is_active()) {
        let civ_id = civ.id;
        for (figure_id, ev) in civ.step_per_figure(tick) {
            hypothesis_events.push((civ_id, figure_id, ev));
        }
    }
    Ok(hypothesis_events)
}

/// Phase F: emit `Discovery` marker, apply the plateau detector
/// plus cosmology drift per civ, translate each `HypothesisEvent`
/// variant into its protocol event, and run the emergence
/// passes that graduate templates / dynamic tools into the species
/// canon.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(crate) fn discovery_emission_phase<E: Emitter>(
    emitter: &mut E,
    tick: u64,
    recognition: &RecognitionLibrary,
    species: &mut Species,
    civs: &mut [Civ],
    hypothesis_events: &[(u32, u32, HypothesisEvent)],
    total_confirmed_relations: &mut u32,
    total_refinements: &mut u32,
    state: &PhysicsState,
    planet: &Planet,
) -> Result<(), E::Error> {
    emitter.emit(&Event::Tick(TickEvent {
        tick,
        phase: Phase::Discovery,
    }))?;
    // Plateau detector + cosmology drift. Bump
    // last_discovery_tick once on any knowledge event;
    // apply per-event cosmology pushes scaled by figure
    // charisma.
    // Plateau detector + cosmology drift, per civ.
    // Group events by civ_id so each civ's drift / discovery
    // bookkeeping stays scoped.
    let mut had_discovery_by_civ: BTreeMap<u32, bool> = BTreeMap::new();
    let mut had_refinement_by_civ: BTreeMap<u32, bool> = BTreeMap::new();
    for (civ_id, figure_id, ev) in hypothesis_events {
        // Every hypothesis event pushes both cosmology
        // (slow-drift deep worldview) AND religion (fast-divergent
        // cultural / religious layer) in the same conceptual
        // direction. Cosmology magnitudes were halved when religion
        // was introduced; religion's `push_for_*` table is 3× the
        // cosmology magnitude so the fast layer absorbs most of the
        // drift signal.
        let pushes = match ev {
            HypothesisEvent::Confirmed(_) => {
                *had_discovery_by_civ.entry(*civ_id).or_insert(false) = true;
                Some((
                    cosmology::push_for_relation_confirmed(),
                    sim_civ::religion::push_for_relation_confirmed(),
                ))
            }
            HypothesisEvent::RefinementProposed { .. } => Some((
                cosmology::push_for_refinement_proposed(),
                sim_civ::religion::push_for_refinement_proposed(),
            )),
            HypothesisEvent::RefinementConfirmed { .. } => {
                *had_discovery_by_civ.entry(*civ_id).or_insert(false) = true;
                *had_refinement_by_civ.entry(*civ_id).or_insert(false) = true;
                Some((
                    cosmology::push_for_refinement_confirmed(),
                    sim_civ::religion::push_for_refinement_confirmed(),
                ))
            }
            HypothesisEvent::RefinementRejected { .. } => Some((
                cosmology::push_for_refinement_rejected(),
                sim_civ::religion::push_for_refinement_rejected(),
            )),
            HypothesisEvent::MeasurementConfirmed(_) => {
                // A measurement confirmation is a discovery
                // event for plateau detection and pushes
                // cosmology the same direction as a firing-relation
                // confirmation.
                *had_discovery_by_civ.entry(*civ_id).or_insert(false) = true;
                Some((
                    cosmology::push_for_relation_confirmed(),
                    sim_civ::religion::push_for_relation_confirmed(),
                ))
            }
            HypothesisEvent::Falsified { .. } => {
                // Prediction-drift event. Same cosmology
                // direction as a refinement-proposed event (the
                // civ's previously-held law has come under
                // scrutiny); doesn't count as a discovery for
                // plateau detection.
                Some((
                    cosmology::push_for_refinement_proposed(),
                    sim_civ::religion::push_for_refinement_proposed(),
                ))
            }
            HypothesisEvent::Revalidated { .. } => {
                // A successor civ verified an inherited law
                // on its own data. Counts as a discovery for
                // plateau detection (the civ has just demonstrated
                // a real fit) and pushes cosmology like a
                // confirmation.
                *had_discovery_by_civ.entry(*civ_id).or_insert(false) = true;
                Some((
                    cosmology::push_for_relation_confirmed(),
                    sim_civ::religion::push_for_relation_confirmed(),
                ))
            }
            HypothesisEvent::Lapsed { .. } => {
                // A successor civ rejected an inherited
                // law. Pushes cosmology like a refinement-rejected
                // event (a piece of inherited theory no longer
                // holds; not a new finding).
                Some((
                    cosmology::push_for_refinement_rejected(),
                    sim_civ::religion::push_for_refinement_rejected(),
                ))
            }
        };
        if let Some((cp, rp)) = pushes {
            if let Some(civ) = civs.iter_mut().find(|c| c.id == *civ_id) {
                let charisma = civ.figure_charisma(*figure_id);
                civ.apply_cosmology_push(&cp, charisma);
                civ.apply_religion_push(&rp, charisma);
            }
        }
    }
    for civ in civs.iter_mut().filter(|c| c.is_active()) {
        if had_discovery_by_civ.get(&civ.id).copied().unwrap_or(false) {
            civ.note_discovery(tick);
        }
        if had_refinement_by_civ.get(&civ.id).copied().unwrap_or(false) {
            civ.note_refinement(tick);
        }
    }
    for (civ_id, figure_id, ev) in hypothesis_events {
        match ev {
            HypothesisEvent::Confirmed(c) => {
                emitter.emit(&Event::RelationConfirmed(relation_to_event(
                    tick,
                    c,
                    *figure_id,
                    recognition,
                )))?;
                *total_confirmed_relations = total_confirmed_relations.saturating_add(1);
            }
            HypothesisEvent::RefinementProposed {
                relation_id,
                template_id: _,
                old_form,
                new_form,
                old_confidence,
                n_samples,
            } => {
                emitter.emit(&Event::RefinementProposed(RefinementProposed {
                    tick,
                    figure_id: *figure_id,
                    relation_id: *relation_id,
                    old_form: old_form.tag().to_string(),
                    new_form: new_form.tag().to_string(),
                    old_confidence_q32: old_confidence.raw().to_bits(),
                    n_samples: u32::try_from(*n_samples).unwrap_or(u32::MAX),
                }))?;
                *total_refinements = total_refinements.saturating_add(1);
            }
            HypothesisEvent::RefinementConfirmed {
                relation_id,
                template_id: _,
                channel,
                old_form,
                new_form,
                new_params,
                new_residual,
                new_confidence,
                n_samples,
            } => {
                let real_params = new_form.rescale_params(new_params, channel.scale());
                emitter.emit(&Event::RefinementConfirmed(RefinementConfirmed {
                    tick,
                    figure_id: *figure_id,
                    relation_id: *relation_id,
                    old_form: old_form.tag().to_string(),
                    new_form: new_form.tag().to_string(),
                    new_params_q32: real_params.iter().map(|p| p.raw().to_bits()).collect(),
                    new_residual_q32: new_residual.raw().to_bits(),
                    new_confidence_q32: new_confidence.raw().to_bits(),
                    n_samples: u32::try_from(*n_samples).unwrap_or(u32::MAX),
                }))?;
                *total_refinements = total_refinements.saturating_add(1);
            }
            HypothesisEvent::RefinementRejected {
                relation_id,
                template_id: _,
                old_form,
                attempted_form,
                reason,
            } => {
                emitter.emit(&Event::RefinementRejected(RefinementRejected {
                    tick,
                    figure_id: *figure_id,
                    relation_id: *relation_id,
                    old_form: old_form.tag().to_string(),
                    attempted_form: attempted_form.tag().to_string(),
                    reason: reason.clone(),
                }))?;
                *total_refinements = total_refinements.saturating_add(1);
            }
            HypothesisEvent::Falsified {
                relation_id,
                template_id: _,
                old_form,
                streak_ticks,
            } => {
                emitter.emit(&Event::RelationFalsified(protocol::RelationFalsified {
                    tick,
                    civ_id: *civ_id,
                    figure_id: *figure_id,
                    relation_id: *relation_id,
                    old_form: old_form.tag().to_string(),
                    streak_ticks: *streak_ticks,
                }))?;
                *total_refinements = total_refinements.saturating_add(1);
            }
            HypothesisEvent::Revalidated {
                relation_id,
                template_id: _,
                from_civ_id,
                new_residual,
                new_confidence,
            } => {
                emitter.emit(&Event::RelationRevalidated(protocol::RelationRevalidated {
                    tick,
                    civ_id: *civ_id,
                    figure_id: *figure_id,
                    relation_id: *relation_id,
                    from_civ_id: *from_civ_id,
                    new_residual_q32: new_residual.raw().to_bits(),
                    new_confidence_q32: new_confidence.raw().to_bits(),
                }))?;
                *total_confirmed_relations = total_confirmed_relations.saturating_add(1);
            }
            HypothesisEvent::Lapsed {
                relation_id,
                template_id: _,
                from_civ_id,
                attempted_form,
            } => {
                emitter.emit(&Event::RelationLapsed(protocol::RelationLapsed {
                    tick,
                    civ_id: *civ_id,
                    figure_id: *figure_id,
                    relation_id: *relation_id,
                    from_civ_id: *from_civ_id,
                    attempted_form: attempted_form.tag().to_string(),
                }))?;
                *total_refinements = total_refinements.saturating_add(1);
            }
            HypothesisEvent::MeasurementConfirmed(m) => {
                let real_params = m.params_in_real_units();
                emitter.emit(&Event::MeasurementConfirmed(
                    protocol::MeasurementConfirmed {
                        tick,
                        civ_id: *civ_id,
                        figure_id: *figure_id,
                        relation_id: m.relation_id,
                        y_channel: m.y_channel.tag(),
                        x_channel: m.x_channel.tag(),
                        form: m.form.tag().to_string(),
                        params_q32: real_params.iter().map(|p| p.raw().to_bits()).collect(),
                        residual_q32: m.residual.raw().to_bits(),
                        confidence_q32: m.confidence.raw().to_bits(),
                        n_samples: u32::try_from(m.n_samples).unwrap_or(u32::MAX),
                        is_experimental: m.is_experimental,
                    },
                ))?;
                *total_confirmed_relations = total_confirmed_relations.saturating_add(1);
            }
        }
    }

    // Emergence pass — every `EMERGENCE_CHECK_PERIOD_TICKS`
    // ticks, scan each civ's confirmed `ThresholdStep` relations
    // for new species-recognition-template proposals. Templates
    // graduate into the species canon and start firing on
    // subsequent scans. Iteration order: civs sorted by id;
    // confirmed relations sorted by relation_id (BTreeMap order)
    // — preserves the determinism contract.
    if sim_civ::discovery::emergence::is_emergence_tick_for_metabolism(
        tick,
        planet.metabolic_substrate.metabolism(),
    ) {
        let mut new_proposals: Vec<sim_civ::discovery::emergence::EmergentTemplateProposal> =
            Vec::new();
        // Borrow species immutably while scanning so the proposal
        // function reads the current discovered set; insert into
        // species after the scan.
        let mut working_next_id = species.next_discovered_template_id;
        for civ in civs.iter().filter(|c| c.is_active()) {
            let civ_proposals = sim_civ::discovery::emergence::propose_discovered_templates(
                civ,
                species,
                recognition,
                tick,
            );
            for mut p in civ_proposals {
                // Re-key against the running next_id allocator so
                // proposals from later civs in this same scan
                // don't collide with earlier ones.
                p.template.id = working_next_id;
                working_next_id = working_next_id.saturating_add(1);
                new_proposals.push(p);
            }
        }
        for proposal in new_proposals {
            let threshold_si = match &proposal.template.signature {
                sim_recognition::Signature::Above(_, t) => t.to_f64_for_display(),
                _ => 0.0,
            };
            emitter.emit(&Event::TemplateDiscovered(protocol::TemplateDiscovered {
                tick,
                civ_id: proposal.proposing_civ_id,
                template_id: proposal.template.id,
                template_name: proposal.template.name.clone(),
                origin_template_id: proposal.origin_template_id,
                threshold_si,
            }))?;
            species
                .discovered_templates
                .insert(proposal.template.id, proposal.template);
        }
        species.next_discovered_template_id = working_next_id;
    }

    // Dynamic-tool emergence — same cadence as relation emergence.
    // Each civ scans its confirmed-relation clusters and
    // proposes new tools when a single channel has ≥
    // EMERGENT_TOOL_CLUSTER_SIZE confirmed relations. Tools
    // graduate into the species' dynamic_tool_registry; the
    // proposing civ unlocks them immediately.
    if sim_civ::discovery::tool_emergence::is_tool_emergence_tick_for_metabolism(
        tick,
        planet.metabolic_substrate.metabolism(),
    ) {
        let mut new_tool_proposals: Vec<sim_civ::discovery::tool_emergence::EmergentToolProposal> =
            Vec::new();
        let mut working_next_tool_id = species.next_dynamic_tool_id;
        for civ in civs.iter().filter(|c| c.is_active()) {
            let civ_proposals =
                sim_civ::discovery::tool_emergence::propose_dynamic_tools(civ, species, tick);
            for mut p in civ_proposals {
                // Resource gate: a dynamic tool proposed from a
                // substance-channel cluster (Fuel / Fossil / etc.)
                // only graduates if the proposing civ's territory
                // actually carries the substance. Abstract-channel
                // dynamic tools (Temperature / Charge / …) carry
                // an empty `resource_prereqs` so this is a no-op
                // for them.
                let resource_ok = p.tool.resource_prereqs.iter().all(|(idx, threshold)| {
                    let densities = state.substance(*idx as usize);
                    let mut sum = sim_arith::Real::ZERO;
                    for cell in &civ.claimed_cells {
                        if let Some(v) = densities.get(*cell as usize) {
                            sum = sum + *v;
                        }
                    }
                    sum >= *threshold
                });
                if !resource_ok {
                    continue;
                }
                // Re-key against the running id allocator so
                // multi-civ scans in the same tick don't collide.
                p.tool.id = working_next_tool_id;
                working_next_tool_id = working_next_tool_id.saturating_add(1);
                new_tool_proposals.push(p);
            }
        }
        // Track which civs unlock which tools so we can
        // copy them into civ.unlocked_dynamic_tools after
        // the immutable scan finishes.
        for proposal in new_tool_proposals {
            emitter.emit(&Event::ToolDiscovered(protocol::ToolDiscovered {
                tick,
                civ_id: proposal.proposing_civ_id,
                tool_id: proposal.tool.id,
                tool_name: proposal.tool.name.clone(),
                channel_focus: format!("{:?}", proposal.tool.channel_focus).to_lowercase(),
                cluster_size: u32::try_from(proposal.cluster_size).unwrap_or(u32::MAX),
                tier: proposal.tool.tier,
                capacity_multiplier_q32: proposal.tool.effects.capacity_multiplier.raw().to_bits(),
                literacy_bonus_q32: proposal.tool.effects.literacy_bonus.raw().to_bits(),
                transmission_fidelity_bonus_q32: proposal
                    .tool
                    .effects
                    .transmission_fidelity_bonus
                    .raw()
                    .to_bits(),
            }))?;
            // Copy onto the proposing civ + species registry.
            if let Some(civ) = civs.iter_mut().find(|c| c.id == proposal.proposing_civ_id) {
                civ.unlocked_dynamic_tools.push(proposal.tool.clone());
            }
            species
                .dynamic_tool_registry
                .insert(proposal.tool.id, proposal.tool);
        }
        species.next_dynamic_tool_id = working_next_tool_id;
    }
    Ok(())
}

/// Phase G: emit `CapabilityEvaluation` marker, run
/// production unlock checks per civ (with serendipity-path
/// fallback), and emit one `TechUnlocked` event per unlocked tool.
#[allow(clippy::too_many_arguments)]
pub(crate) fn tech_unlock_phase<E: Emitter>(
    emitter: &mut E,
    tick: u64,
    planet: &Planet,
    recognition: &RecognitionLibrary,
    species_channels: &BTreeSet<ChannelKind>,
    species_manipulations: &BTreeSet<ManipulationKind>,
    species_baseline: &BTreeSet<u32>,
    has_magnetosphere: bool,
    has_em_medium: bool,
    civs: &mut [Civ],
    total_confirmed_relations: u32,
    total_tech_unlocks: &mut u32,
    state: &PhysicsState,
) -> Result<(), E::Error> {
    emitter.emit(&Event::Tick(TickEvent {
        tick,
        phase: Phase::CapabilityEvaluation,
    }))?;
    // Sensorium-extending tech for the active civ. Collect
    // unlocks first (mutable borrow on civ), then emit events.
    // Production gate: tools unlock when manipulation +
    // native-channel + planet-feature prereqs hold AND the civ
    // itself has fit enough confirmed (and experimentally-confirmed)
    // relations AND civ literacy clears the floor. M5: per-civ
    // tool unlock checks.
    let mut unlocks_to_emit: Vec<(tech::ToolKind, Vec<u32>, u32, bool)> = Vec::new();
    for civ in civs.iter_mut().filter(|c| c.is_active()) {
        let civ_literacy = civ.literacy_score(tick);
        // Union of confirmed firing relations across the civ's
        // active figures, keyed on relation_id. Used both for the
        // per-tool `relation_prereqs` template-match check and as
        // an input to the civ-maturity total count below. The clone
        // is shallow (BTreeMap of references would require lifetime
        // shenanigans through `is_unlocked`'s many call sites);
        // the per-tick cost is negligible relative to the
        // candidate-fit work the hypothesizer already does.
        let civ_confirmed: BTreeMap<u32, sim_civ::discovery::ConfirmedRelation> = civ
            .figures
            .iter()
            .filter(|f| f.retired_tick.is_none())
            .flat_map(|f| f.hypothesizer.confirmed.iter())
            .map(|(k, v)| (*k, v.clone()))
            .collect();
        // Civ-maturity counts. `civ_confirmed_count` totals
        // confirmed firing-relations + confirmed measurement-
        // relations the civ has fit; gates the per-tier
        // `min_civ_confirmed_relations` floor. `civ_experimental_count`
        // is the subset of measurement relations whose fit-pool
        // included at least one apparatus sample
        // (`ConfirmedMeasurement.is_experimental`); gates
        // `min_civ_experimental_relations`. Replaces the previous
        // observation-threshold gate, which counted raw template
        // firings (environment) rather than confirmed laws (work).
        let civ_measurement_count: u32 = civ
            .figures
            .iter()
            .filter(|f| f.retired_tick.is_none())
            .map(|f| u32::try_from(f.hypothesizer.confirmed_measurements.len()).unwrap_or(u32::MAX))
            .fold(0u32, u32::saturating_add);
        let civ_confirmed_count: u32 = u32::try_from(civ_confirmed.len())
            .unwrap_or(u32::MAX)
            .saturating_add(civ_measurement_count);
        let civ_experimental_count: u32 = u32::try_from(
            civ.figures
                .iter()
                .filter(|f| f.retired_tick.is_none())
                .flat_map(|f| f.hypothesizer.confirmed_measurements.values())
                .filter(|m| m.is_experimental)
                .count(),
        )
        .unwrap_or(u32::MAX);
        for tool in tech::ToolKind::ALL {
            if civ.unlocked_tools.contains(&tool) {
                continue;
            }
            // Material-resource gate: tools with non-empty
            // `resource_prereqs` require the civ's territory to
            // carry the substance(s) at threshold. Hard gate —
            // serendipity does not bypass it. Skip the tool
            // entirely (neither strict nor serendipitous unlock)
            // when the substrate is unavailable.
            if !tech::resource_prereqs_satisfied(tool, state, &civ.claimed_cells) {
                continue;
            }
            let strict_pass = tech::is_unlocked(
                tool,
                species_channels,
                species_manipulations,
                has_magnetosphere,
                has_em_medium,
                planet.crust,
                civ_confirmed_count,
                civ_experimental_count,
                civ_literacy,
                &civ_confirmed,
                &civ.unlocked_tools,
            );
            // Serendipity path. If the strict prereq DAG
            // fails, check whether the civ is *one prereq away*
            // and roll a per-tick deterministic dice. Rare but
            // possible — Newton-saw-an-apple-style leaps. Real
            // innovation isn't pure-prereq. Keyed on
            // (planet_seed, civ_id, tool_id, tick) so byte-replay
            // holds.
            let serendipitous = if strict_pass {
                false
            } else if tech::serendipity_missing_prereqs(
                tool,
                species_channels,
                species_manipulations,
                has_magnetosphere,
                has_em_medium,
                planet.crust,
                civ_confirmed_count,
                civ_experimental_count,
                civ_literacy,
                &civ_confirmed,
                &civ.unlocked_tools,
            )
            .is_some()
                && tech::serendipity_roll(
                    planet.seed,
                    civ.id,
                    tool.id(),
                    tick,
                    civ_literacy,
                    total_confirmed_relations,
                )
            {
                true
            } else {
                continue;
            };
            // Species-cumulative maturity gate: tier-5 tools
            // require the species to have accumulated enough
            // science across all its civs in the run. Pushes
            // tier-5 unlocks naturally into the late-game
            // (~thousands of years) without authoring a
            // tier-progression rule. Serendipity does NOT bypass
            // this — even a lucky breakthrough needs the species
            // to have done the cumulative work.
            if total_confirmed_relations < tool.species_maturity_floor() {
                continue;
            }
            let newly = civ.apply_tool_unlock(tool, species_baseline, recognition);
            unlocks_to_emit.push((tool, newly, civ.id, serendipitous));
        }
    }
    for (tool, newly, civ_id, serendipitous) in unlocks_to_emit {
        emitter.emit(&Event::TechUnlocked(TechUnlocked {
            tick,
            civ_id,
            tool_id: tool.id(),
            tool_name: tool.name().to_string(),
            tier: tool.tier(),
            granted_channels: tool
                .granted_channels()
                .iter()
                .map(|c| format!("{c:?}"))
                .collect(),
            newly_perceivable_template_ids: newly,
            serendipitous,
        }))?;
        *total_tech_unlocks = total_tech_unlocks.saturating_add(1);
    }
    Ok(())
}

/// Per-tick resource consumption: each unlocked tool with a
/// burnable `resource_prereqs` entry (`Fuel` or `Fossil`) draws
/// down the substance across the civ's claimed territory,
/// mirroring the combustion stoichiometry
/// (1 fuel + 1 oxidiser → 2 ash) so the cycle stays mass-
/// conservative and feeds the regrowth loop. Non-burnable
/// prereqs (water / ice / vapour) are read-only.
///
/// Runs after the unlock phase so a tool unlocked this tick draws
/// resources next tick onward, never on the unlock tick itself.
/// Skipping the emit on this phase — consumption is silent state
/// mutation; the post-run report inspects substance fields
/// directly to surface tool-driven depletion.
pub(crate) fn resource_consumption_phase(state: &mut PhysicsState, civs: &[Civ]) {
    sim_civ::tech::apply_tool_consumption(state, civs);
}

/// Phase H: emit `PopulationDynamics` marker, run per-cell
/// population steps + inter-cell migration, then reshape each
/// active civ's territory by BFS from its centroid and emit
/// `CivTerritoryChanged` events whenever the claim set changes.
/// Newly-gained cells absorb their nomadic populations into the
/// civ's cohort.
pub(crate) fn population_phase<E: Emitter>(
    emitter: &mut E,
    tick: u64,
    state: &PhysicsState,
    planet: &Planet,
    species: &Species,
    civs: &mut [Civ],
    nomad_pops: &mut BTreeMap<u32, Real>,
) -> Result<(), E::Error> {
    emitter.emit(&Event::Tick(TickEvent {
        tick,
        phase: Phase::PopulationDynamics,
    }))?;
    // M5: per-civ population step using per-cell
    // dynamics, followed by gradient-driven migration.
    // Each `region_cohort` evolves independently with cell-
    // local seasonal capacity (seasonal factor applied
    // per-cell). Then high-pressure cells shed pop to
    // adjacent claimed cells with headroom — pre-emptive
    // migration before food crisis bites. Aggregate
    // cohort.count is rederived as the sum.
    let grid_w = state.grid().width();
    let grid_h = state.grid().height();
    for civ in civs.iter_mut().filter(|c| c.is_active()) {
        civ.step_population_per_cell(state, tick, planet, species);
        civ.migrate_inter_cell(state, tick, planet, grid_w, grid_h);
    }

    // Per-cell-capacity-driven territory updates.
    //
    // Old model: `target_cell_count(pop, max)` divided aggregate
    // population by a flat people-per-cell constant, then
    // `compute_territory` BFS-claimed that many cells from the
    // centroid. Density was uniform across the civ's territory and
    // the shape was a round BFS disk regardless of which cells
    // were actually fertile. (`target_cell_count` now reads the
    // civ's dynamic per-cell capacity, but it's only used at
    // founding — ongoing expansion runs through the overflow path
    // below.)
    //
    // New model: `cell_capacity` (already per-tech, per-terrain,
    // per-season, per-biosphere) sets each cell's population
    // ceiling. The per-cell logistic step grows each cell toward
    // its own ceiling; high-pressure cells leak migrants outward
    // via `migrate_inter_cell`; remaining overflow at the frontier
    // claims the highest-cap unclaimed habitable neighbour through
    // `expand_via_overflow`. Empty cells (count < 0.1) are pruned
    // each tick via `prune_empty_cells`. Territory shape now
    // follows fertile-cell topology, not BFS rings.
    let _ = species; // BFS path took `species.habitat`; the
                     // overflow path reads `cell_capacity` directly,
                     // which already gates on terrain habitability.
                     // Compute each civ's "claimed by others" set once before the
                     // mutable loop so expansion never trespasses on another civ.
    let claimed_by_civ: BTreeMap<u32, BTreeSet<u32>> = civs
        .iter()
        .filter(|c| c.is_active())
        .map(|c| (c.id, c.claimed_cells.clone()))
        .collect();
    let mut territory_events: Vec<CivTerritoryChanged> = Vec::new();
    for civ in civs.iter_mut().filter(|c| c.is_active()) {
        let mut others: BTreeSet<u32> = BTreeSet::new();
        for (&id, cells) in &claimed_by_civ {
            if id != civ.id {
                others.extend(cells.iter().copied());
            }
        }
        let prev_cells = civ.claimed_cells.clone();
        let gained = civ.expand_via_overflow(state, tick, planet, grid_w, grid_h, &others);
        let _removed = civ.prune_empty_cells();
        let cells_changed = civ.claimed_cells != prev_cells;
        // Re-emit `CivTerritoryChanged` whenever the claim set
        // changes (the natural trigger) and also every
        // `TERRITORY_REFRESH_TICKS` even for stable territory.
        // Without the periodic refresh, per-cell caps go stale as
        // tech multipliers, seasonal factors, and biosphere fuel
        // drift — the viewport's pop-digit scale keeps reading
        // against the at-founding cap until the civ next expands
        // or contracts, which can be many decades on dense seeds.
        let stale = tick.saturating_sub(civ.last_territory_emit_tick) >= TERRITORY_REFRESH_TICKS;
        if !cells_changed && !stale {
            continue;
        }
        if cells_changed {
            // Territory-expansion absorb: existing civ; no founder
            // reorg tax.
            nomads::absorb_into_civ(
                nomad_pops,
                civ,
                gained,
                &species.biology,
                sim_arith::Real::ZERO,
            );
        }
        let claimed_sorted = claimed_cells_for_event(civ);
        let cell_populations_q32: Vec<i128> = claimed_sorted
            .iter()
            .map(|c| {
                civ.region_cohorts
                    .get(c)
                    .map_or(0i128, |cohort| cohort.total().raw().to_bits())
            })
            .collect();
        // Per-cell carrying capacity (cell_capacity in
        // Pop/Q96.32). Mirrors `cell_populations_q32` order so
        // the viewport renderer can read each cell's pop / cap
        // ratio for the pop-digit scale (digit 9 = saturated,
        // 0 = empty). Same `cell_capacity` formula used by
        // `step_population_per_cell` upstream — tech × terrain ×
        // seasonal × biosphere — so the digit reflects the cap
        // the population step actually evolves toward.
        let cell_capacities_q32: Vec<i128> = claimed_sorted
            .iter()
            .map(|&c| civ.cell_capacity(state, c, tick, planet).raw().to_bits())
            .collect();
        civ.last_territory_emit_tick = tick;
        territory_events.push(CivTerritoryChanged {
            tick,
            civ_id: civ.id,
            claimed_cells: claimed_sorted,
            population_q32: civ.cohort.total().raw().to_bits(),
            cell_populations_q32,
            cell_capacities_q32,
        });
    }
    for ev in territory_events {
        emitter.emit(&Event::CivTerritoryChanged(ev))?;
    }
    Ok(())
}

/// Re-emit cadence for `CivTerritoryChanged` on civs whose claim
/// set hasn't changed since the last emission. ~50 baseline years
/// at the default 9–12 ticks/year — frequent enough that
/// pop-digit + cap drift stays visible in the viewport, infrequent
/// enough to keep total event volume bounded (≤ N_civs × ticks/500
/// extra emissions across a run).
const TERRITORY_REFRESH_TICKS: u64 = 500;

/// Phase J: emit `CulturalDrift` marker, then emit one
/// `CosmologyShifted` event per civ whose cosmology has drifted
/// past the gate threshold since its last emission. Also
/// emit `ReligionShifted` for any civ whose religion vector has
/// drifted past its (lower) gate threshold.
pub(crate) fn cultural_drift_phase<E: Emitter>(
    emitter: &mut E,
    tick: u64,
    civs: &mut [Civ],
) -> Result<(), E::Error> {
    emitter.emit(&Event::Tick(TickEvent {
        tick,
        phase: Phase::CulturalDrift,
    }))?;
    let mut cosmology_to_emit: Vec<(u32, [i64; 5], i64)> = Vec::new();
    let mut religion_to_emit: Vec<(u32, [i64; 3], i64)> = Vec::new();
    for civ in civs.iter_mut() {
        if civ.cosmology_should_emit() {
            let axes = civ.cosmology.axes_q32();
            let dog = civ.cosmology.dogmatism().raw().to_bits();
            cosmology_to_emit.push((civ.id, axes, dog));
            civ.note_cosmology_emitted();
        }
        if civ.religion_should_emit() {
            let axes = civ.religion.axes_q32();
            let dog = civ.religion.dogmatism().raw().to_bits();
            religion_to_emit.push((civ.id, axes, dog));
            civ.note_religion_emitted();
        }
    }
    for (civ_id, axes, dogmatism_q32) in cosmology_to_emit {
        emitter.emit(&Event::CosmologyShifted(CosmologyShifted {
            tick,
            civ_id,
            axes_q32: axes.to_vec(),
            dogmatism_q32,
        }))?;
    }
    for (civ_id, axes, dogmatism_q32) in religion_to_emit {
        emitter.emit(&Event::ReligionShifted(ReligionShifted {
            tick,
            civ_id,
            axes_q32: axes.to_vec(),
            dogmatism_q32,
        }))?;
    }
    Ok(())
}

/// How often to scan border cells for cultural-flip pressure.
/// Once per sim-year on the 1-tick = 1-month cadence — slow
/// enough that flips read as a deliberate "this region drifted
/// into a neighbour's orbit" beat rather than a per-month
/// jitter, fast enough that an ailing civ visibly hemorrhages
/// territory before collapse rather than holding every cell
/// until the moment the cohort finally hits zero.
pub(crate) const CULTURE_FLIP_CHECK_TICKS: u64 = 12;

/// Border-cell flip mechanics. A claimed cell at the boundary
/// between two civs can switch allegiance to a neighbour when
/// the owner is structurally weak (low cohesion) and the
/// neighbour pulls hard (high cohesion + culturally-similar
/// cosmology + larger pop). Models the historical pattern of
/// frontier regions drifting into a stronger neighbour's orbit
/// — Alsace-Lorraine ping-ponging, Crimea flipping, etc — as
/// distinct from outright war: no military event fires, the
/// cell just changes hands at the next yearly check.
///
/// Gates (must all hold for any flip to fire):
///
/// - Owner cohesion ≤ `OWNER_COHESION_CEIL` (0.50). Strong civs
///   don't lose territory to passive cultural pressure; the
///   only way to take a cohesive civ's land is the war path.
/// - Neighbour cohesion ≥ `NEIGHBOUR_COHESION_FLOOR` (0.65).
///   The pulling civ has to be in better internal shape than
///   the loser.
/// - Cosmology distance ≤ `COSMOLOGY_DISTANCE_CEIL` (2.0).
///   Wildly-different cultures don't flip cells; you need
///   *some* shared worldview for the populace to accept the
///   transition. (Max possible L2 distance over 5 axes in
///   `[-1, 1]` is ≈ 4.47, so 2.0 ≈ "noticeably similar.")
/// - Pull score ≥ `FLIP_THRESHOLD` (0.30). Composite metric
///   blending the cohesion gap and cosmology proximity.
///
/// Per-civ throttling: at most one cell flips OUT of any civ
/// per check (sorted by cell id for determinism). Prevents a
/// single weak civ from disintegrating in one tick when
/// surrounded by stronger neighbours.
const OWNER_COHESION_CEIL: (i64, i64) = (50, 100);
const NEIGHBOUR_COHESION_FLOOR: (i64, i64) = (65, 100);
const COSMOLOGY_DISTANCE_CEIL: (i64, i64) = (2, 1);
const FLIP_THRESHOLD: (i64, i64) = (30, 100);

/// Phase J': border-cell cultural flips. Runs after
/// `cultural_drift_phase` so the just-updated cosmology /
/// cohesion drives the flip decision. Fires once per
/// `CULTURE_FLIP_CHECK_TICKS` ticks; on off-cycle ticks it's
/// a noop. Emits `CivTerritoryChanged` for both the losing and
/// gaining civ so viewport / report consumers see the
/// transition immediately.
pub(crate) fn culture_flip_phase<E: Emitter>(
    emitter: &mut E,
    tick: u64,
    state: &PhysicsState,
    planet: &Planet,
    civs: &mut [Civ],
) -> Result<(), E::Error> {
    if !tick.is_multiple_of(CULTURE_FLIP_CHECK_TICKS) {
        return Ok(());
    }
    let owner_ceil = Real::from(OWNER_COHESION_CEIL);
    let nbr_floor = Real::from(NEIGHBOUR_COHESION_FLOOR);
    let cosmology_ceil = Real::from(COSMOLOGY_DISTANCE_CEIL);
    let flip_threshold = Real::from(FLIP_THRESHOLD);
    // Build a cell → owning civ_id map for O(1) neighbour
    // lookup. Only active civs participate; collapsed husks
    // can't pull or be pulled from.
    let mut owner: BTreeMap<u32, u32> = BTreeMap::new();
    for civ in civs.iter().filter(|c| c.is_active()) {
        for &cell in &civ.claimed_cells {
            owner.insert(cell, civ.id);
        }
    }
    // Pre-compute per-civ snapshots so we can mutate inside
    // the loop without re-borrowing.
    struct CivSnap {
        idx: usize,
        id: u32,
        cohesion: Real,
        cosmology: sim_civ::cosmology::Cosmology,
    }
    let snaps: Vec<CivSnap> = civs
        .iter()
        .enumerate()
        .filter(|(_, c)| c.is_active())
        .map(|(idx, c)| CivSnap {
            idx,
            id: c.id,
            cohesion: c.cohesion,
            cosmology: c.cosmology,
        })
        .collect();
    let snap_by_id: BTreeMap<u32, &CivSnap> = snaps.iter().map(|s| (s.id, s)).collect();
    // Score each potential flip; keep at most one per losing civ.
    // Score = (nbr.cohesion - owner.cohesion) - 0.5 * cosmology_distance
    // — strong-neighbour vs weak-owner gap minus a culture-distance
    // penalty. The half-weight on distance is calibrated so a
    // cohesion gap of 0.4 dominates a typical drift distance of
    // ~1.0, but a culturally-alien neighbour (distance ≈ 3) needs
    // a much larger gap to flip the cell.
    let half = Real::from_ratio(1, 2);
    let mut flips: Vec<(u32 /* cell */, u32 /* from */, u32 /* to */)> = Vec::new();
    for losing in &snaps {
        if losing.cohesion > owner_ceil {
            continue;
        }
        let owner_civ = &civs[losing.idx];
        let mut best: Option<(u32, u32, Real)> = None;
        // Iterate cells in sorted order so the per-civ tie-break
        // is deterministic. BTreeSet iteration is already sorted.
        for &cell in &owner_civ.claimed_cells {
            let axial = state.grid().axial_of(sim_physics::CellId(cell));
            for nbr in state.grid().neighbours(axial) {
                let Some(&nbr_civ_id) = owner.get(&nbr.0) else {
                    continue;
                };
                if nbr_civ_id == losing.id {
                    continue;
                }
                let Some(nbr_snap) = snap_by_id.get(&nbr_civ_id) else {
                    continue;
                };
                if nbr_snap.cohesion < nbr_floor {
                    continue;
                }
                let dist = losing.cosmology.distance_to(&nbr_snap.cosmology);
                if dist > cosmology_ceil {
                    continue;
                }
                let cohesion_gap = nbr_snap.cohesion - losing.cohesion;
                let score = cohesion_gap - half * dist;
                if score < flip_threshold {
                    continue;
                }
                if best.as_ref().is_none_or(|(_, _, s)| score > *s) {
                    best = Some((cell, nbr_civ_id, score));
                }
            }
        }
        if let Some((cell, to_id, _)) = best {
            flips.push((cell, losing.id, to_id));
        }
    }
    if flips.is_empty() {
        return Ok(());
    }
    // Apply flips: move cell ownership + region cohort from
    // loser to winner. Resync each touched civ's aggregate, then
    // emit CivTerritoryChanged so the viewport sees both sides.
    let mut touched: BTreeSet<u32> = BTreeSet::new();
    for (cell, from_id, to_id) in flips {
        let from_idx = match civs.iter().position(|c| c.id == from_id) {
            Some(i) => i,
            None => continue,
        };
        let to_idx = match civs.iter().position(|c| c.id == to_id) {
            Some(i) => i,
            None => continue,
        };
        civs[from_idx].claimed_cells.remove(&cell);
        let cohort = civs[from_idx].region_cohorts.remove(&cell);
        civs[from_idx].resync_aggregate_from_regions();
        if let Some(mut c) = cohort {
            c.civ_membership = Some(to_id);
            civs[to_idx].claimed_cells.insert(cell);
            civs[to_idx].region_cohorts.insert(cell, c);
            civs[to_idx].resync_aggregate_from_regions();
        } else {
            civs[to_idx].claimed_cells.insert(cell);
            civs[to_idx].resync_aggregate_from_regions();
        }
        touched.insert(from_id);
        touched.insert(to_id);
    }
    // Emit CivTerritoryChanged for each touched civ so consumers
    // see both sides of the transfer in the same tick.
    let mut events: Vec<CivTerritoryChanged> = Vec::new();
    for civ in civs.iter_mut().filter(|c| touched.contains(&c.id)) {
        let claimed_sorted = claimed_cells_for_event(civ);
        let cell_pops: Vec<i128> = claimed_sorted
            .iter()
            .map(|c| {
                civ.region_cohorts
                    .get(c)
                    .map_or(0i128, |cohort| cohort.total().raw().to_bits())
            })
            .collect();
        let cell_caps: Vec<i128> = claimed_sorted
            .iter()
            .map(|&c| civ.cell_capacity(state, c, tick, planet).raw().to_bits())
            .collect();
        civ.last_territory_emit_tick = tick;
        events.push(CivTerritoryChanged {
            tick,
            civ_id: civ.id,
            claimed_cells: claimed_sorted,
            population_q32: civ.cohort.total().raw().to_bits(),
            cell_populations_q32: cell_pops,
            cell_capacities_q32: cell_caps,
        });
    }
    for ev in events {
        emitter.emit(&Event::CivTerritoryChanged(ev))?;
    }
    Ok(())
}
