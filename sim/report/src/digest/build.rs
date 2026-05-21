//! `impl Digest` — the walker that folds an event stream into the
//! aggregated per-civ chapters plus cross-civ event lists.

use super::types::{
    CatastropheRecord, CivChapter, CollapseRecord, ConflictRecord, Contact, CosmologyRecord,
    Digest, DiscoveredTemplateRecord, DiscoveryRecord, FigureRecord, InventedToolRecord,
    RefinementOutcome, RefinementRecord, RelationLabel, RunEnd, SurplusSnapshot, TechRecord,
    TerritorySnapshot, TradeRouteRecord, TransferRecord,
};
use protocol::Event;
use std::collections::BTreeMap;

impl Digest {
    /// Derive keyframe `WorldFrame`s at fixed-tick intervals from
    /// the per-civ `territory_history`. For each interval boundary
    /// `T`, includes every civ that was founded at `tick ≤ T` and
    /// not yet collapsed at `tick ≤ T`, snapshotting each civ's
    /// most-recent territory state at-or-before `T`.
    ///
    /// Used by the post-run report to render spatial-over-time
    /// keyframes (year 1000, 2000, …, end). The same shape feeds
    /// the `--cli=viewport` live mode, but the live path mirrors
    /// state from arriving events rather than replaying history.
    pub fn keyframes(&self, every_ticks: u64) -> Vec<crate::frame::WorldFrame> {
        if every_ticks == 0 {
            return Vec::new();
        }
        let last_tick = self
            .run_end
            .as_ref()
            .map(|e| e.tick)
            .or_else(|| {
                self.civs
                    .values()
                    .flat_map(|c| c.territory_history.iter().map(|s| s.tick))
                    .max()
            })
            .unwrap_or(0);
        if last_tick == 0 {
            return Vec::new();
        }
        let mut frames: Vec<crate::frame::WorldFrame> = Vec::new();
        let mut t = every_ticks;
        loop {
            let tick = t.min(last_tick);
            let mut civs: Vec<crate::frame::CivClaim> = Vec::new();
            for chap in self.civs.values() {
                if chap.founded_tick > tick {
                    continue;
                }
                if let Some(c) = &chap.collapsed {
                    if c.tick <= tick {
                        continue;
                    }
                }
                let snapshot = chap.territory_history.iter().rev().find(|s| s.tick <= tick);
                let claims: std::collections::BTreeSet<u32> = match snapshot {
                    Some(s) => s.claimed_cells.iter().copied().collect(),
                    None => chap.claimed_cells.iter().copied().collect(),
                };
                if claims.is_empty() {
                    continue;
                }
                let centroid = match chap.figures.first() {
                    Some(f) => f.cell_assignment,
                    None => claims.iter().next().copied().unwrap_or(0),
                };
                // Per-cell pops from the snapshot, if the
                // event log included them. Older logs leave this
                // empty and the density renderer falls back.
                let cell_populations_q32: std::collections::BTreeMap<u32, i128> = match snapshot {
                    Some(s) if s.cell_populations_q32.len() == s.claimed_cells.len() => s
                        .claimed_cells
                        .iter()
                        .copied()
                        .zip(s.cell_populations_q32.iter().copied())
                        .collect(),
                    _ => std::collections::BTreeMap::new(),
                };
                let cell_capacities_q32: std::collections::BTreeMap<u32, i128> = match snapshot {
                    Some(s) if s.cell_capacities_q32.len() == s.claimed_cells.len() => s
                        .claimed_cells
                        .iter()
                        .copied()
                        .zip(s.cell_capacities_q32.iter().copied())
                        .collect(),
                    _ => std::collections::BTreeMap::new(),
                };
                civs.push(crate::frame::CivClaim {
                    civ_id: chap.civ_id,
                    claimed_cells: claims,
                    centroid,
                    cell_populations_q32,
                    cell_capacities_q32,
                });
            }
            frames.push(crate::frame::WorldFrame {
                tick,
                civs,
                nomad_cells: Vec::new(),
            });
            if tick >= last_tick {
                break;
            }
            t = t.saturating_add(every_ticks);
        }
        frames
    }

    /// Single-allocation aggregator. Two passes: `relation_names`
    /// first, then the main fold. Events are cloned into the digest
    /// so the highlights / renderer layers can re-walk the stream
    /// without re-parsing the NDJSON.
    pub fn from_events(events: &[Event]) -> Self {
        let mut digest = Self {
            schema_version: 0,
            seed: 0,
            ages_version: String::new(),
            planet: None,
            planet_map: None,
            species: None,
            run_end: None,
            civs: BTreeMap::new(),
            founding_order: Vec::new(),
            contacts: Vec::new(),
            conflicts: Vec::new(),
            wars: Vec::new(),
            trade_routes: Vec::new(),
            diffusions: Vec::new(),
            transmissions: Vec::new(),
            relation_names: BTreeMap::new(),
            firing_counts: BTreeMap::new(),
            template_names: BTreeMap::new(),
            events: events.to_vec(),
        };

        // Pass 1: relation_id → label, plus template name index.
        for ev in events {
            match ev {
                Event::RelationConfirmed(r) => {
                    digest
                        .relation_names
                        .entry(r.relation_id)
                        .or_insert_with(|| RelationLabel {
                            template_id: r.template_id,
                            template_name: r.template_name.clone(),
                            channel: r.channel.clone(),
                        });
                    digest
                        .template_names
                        .entry(r.template_id)
                        .or_insert_with(|| r.template_name.clone());
                }
                Event::Recognition(f) => {
                    digest
                        .template_names
                        .entry(f.template_id)
                        .or_insert_with(|| f.template_name.clone());
                }
                _ => {}
            }
        }

        // Pass 2: main fold.
        for ev in events {
            digest.absorb(ev);
        }

        digest
    }

    fn absorb(&mut self, ev: &Event) {
        match ev {
            Event::RunStart(h) => {
                self.schema_version = h.schema_version;
                self.seed = h.seed;
                self.ages_version.clone_from(&h.ages_version);
            }
            Event::Planet(p) => self.planet = Some(p.clone()),
            Event::PlanetMap(m) => self.planet_map = Some(m.clone()),
            Event::Species(s) => self.species = Some(s.clone()),
            Event::Recognition(f) => {
                *self.firing_counts.entry(f.template_id).or_insert(0) += 1;
            }
            Event::FigureBorn(f) => {
                self.civ_or_pending(f.civ_id, f.tick)
                    .figures
                    .push(FigureRecord {
                        tick: f.tick,
                        id: f.figure_id,
                        name: f.name.clone(),
                        charisma_q32: f.charisma_q32,
                        curiosity_q32: f.curiosity_q32,
                        doubt_q32: f.doubt_q32,
                        communicativeness_q32: f.communicativeness_q32,
                        cell_assignment: f.cell_assignment,
                    });
            }
            Event::CivFounded(c) => {
                let chapter = self.civs.entry(c.civ_id).or_insert_with(|| CivChapter {
                    civ_id: c.civ_id,
                    parent_civ_id: c.parent_civ_id,
                    founded_tick: c.tick,
                    founding_figure_count: c.founding_figure_count,
                    initial_population_q32: c.initial_population_q32,
                    collapsed: None,
                    discoveries: Vec::new(),
                    refinements: Vec::new(),
                    techs: Vec::new(),
                    catastrophes: Vec::new(),
                    cosmology_shifts: Vec::new(),
                    religion_shifts: Vec::new(),
                    figures: Vec::new(),
                    claimed_cells: Vec::new(),
                    territory_history: Vec::new(),
                    revalidated_count: 0,
                    lapsed_count: 0,
                    falsified_count: 0,
                    discovered_templates: Vec::new(),
                    invented_tools: Vec::new(),
                    life_expectancy_history: Vec::new(),
                    surplus_history: Vec::new(),
                });
                // CivFounded carries the authoritative claimed
                // cells; overwrite whatever the stub above seeded.
                chapter.claimed_cells.clone_from(&c.claimed_cells);
                chapter.territory_history.push(TerritorySnapshot {
                    tick: c.tick,
                    claimed_cells: c.claimed_cells.clone(),
                    population_q32: c.initial_population_q32,
                    cell_populations_q32: Vec::new(),
                    cell_capacities_q32: Vec::new(),
                });
                // If we created a stub via FigureBorn-before-CivFounded
                // (shouldn't happen in well-formed logs but be defensive),
                // overwrite the founding metadata we now know.
                chapter.parent_civ_id = c.parent_civ_id;
                chapter.founded_tick = c.tick;
                chapter.founding_figure_count = c.founding_figure_count;
                chapter.initial_population_q32 = c.initial_population_q32;
                if !self.founding_order.contains(&c.civ_id) {
                    self.founding_order.push(c.civ_id);
                }
            }
            Event::CivTerritoryChanged(t) => {
                let chapter = self.civ_or_pending(t.civ_id, t.tick);
                chapter.claimed_cells.clone_from(&t.claimed_cells);
                chapter.territory_history.push(TerritorySnapshot {
                    tick: t.tick,
                    claimed_cells: t.claimed_cells.clone(),
                    population_q32: t.population_q32,
                    cell_populations_q32: t.cell_populations_q32.clone(),
                    cell_capacities_q32: t.cell_capacities_q32.clone(),
                });
            }
            Event::CivCollapsed(c) => {
                self.civ_or_pending(c.civ_id, c.tick).collapsed = Some(CollapseRecord {
                    tick: c.tick,
                    reason: c.reason.clone(),
                    final_population_q32: c.final_population_q32,
                    final_figure_count: c.final_figure_count,
                });
            }
            Event::RelationConfirmed(r) => {
                let civ_id = self.civ_owning_figure(r.figure_id);
                if let Some(cid) = civ_id {
                    self.civ_or_pending(cid, r.tick)
                        .discoveries
                        .push(DiscoveryRecord {
                            tick: r.tick,
                            relation_id: r.relation_id,
                            figure_id: r.figure_id,
                            template_id: r.template_id,
                            template_name: r.template_name.clone(),
                            channel: r.channel.clone(),
                            form: r.form.clone(),
                            params_q32: r.params_q32.clone(),
                            residual_q32: r.residual_q32,
                            confidence_q32: r.confidence_q32,
                            n_samples: r.n_samples,
                        });
                }
            }
            Event::RefinementProposed(r) => {
                if let Some(cid) = self.civ_owning_figure(r.figure_id) {
                    self.civ_or_pending(cid, r.tick)
                        .refinements
                        .push(RefinementRecord {
                            tick: r.tick,
                            figure_id: r.figure_id,
                            relation_id: r.relation_id,
                            old_form: r.old_form.clone(),
                            new_form: r.new_form.clone(),
                            outcome: RefinementOutcome::Proposed,
                        });
                }
            }
            Event::RefinementConfirmed(r) => {
                if let Some(cid) = self.civ_owning_figure(r.figure_id) {
                    self.civ_or_pending(cid, r.tick)
                        .refinements
                        .push(RefinementRecord {
                            tick: r.tick,
                            figure_id: r.figure_id,
                            relation_id: r.relation_id,
                            old_form: r.old_form.clone(),
                            new_form: r.new_form.clone(),
                            outcome: RefinementOutcome::Confirmed {
                                new_params_q32: r.new_params_q32.clone(),
                                new_residual_q32: r.new_residual_q32,
                                new_confidence_q32: r.new_confidence_q32,
                            },
                        });
                }
            }
            Event::RefinementRejected(r) => {
                if let Some(cid) = self.civ_owning_figure(r.figure_id) {
                    self.civ_or_pending(cid, r.tick)
                        .refinements
                        .push(RefinementRecord {
                            tick: r.tick,
                            figure_id: r.figure_id,
                            relation_id: r.relation_id,
                            old_form: r.old_form.clone(),
                            new_form: r.attempted_form.clone(),
                            outcome: RefinementOutcome::Rejected {
                                reason: r.reason.clone(),
                            },
                        });
                }
            }
            Event::TechUnlocked(t) => {
                self.civ_or_pending(t.civ_id, t.tick)
                    .techs
                    .push(TechRecord {
                        tick: t.tick,
                        tool_id: t.tool_id,
                        tool_name: t.tool_name.clone(),
                        tier: t.tier,
                        granted_channels: t.granted_channels.clone(),
                        newly_perceivable_template_ids: t.newly_perceivable_template_ids.clone(),
                    });
            }
            Event::CatastropheFired(c) => {
                self.civ_or_pending(c.civ_id, c.tick)
                    .catastrophes
                    .push(CatastropheRecord {
                        tick: c.tick,
                        kind: c.catastrophe_kind.clone(),
                        fraction_lost_q32: c.fraction_lost_q32,
                    });
            }
            Event::CosmologyShifted(c) => {
                let mut axes = [0_i64; 5];
                for (i, v) in c.axes_q32.iter().take(5).enumerate() {
                    axes[i] = *v;
                }
                self.civ_or_pending(c.civ_id, c.tick)
                    .cosmology_shifts
                    .push(CosmologyRecord {
                        tick: c.tick,
                        axes_q32: axes,
                        dogmatism_q32: c.dogmatism_q32,
                    });
            }
            Event::ReligionShifted(r) => {
                let mut axes = [0_i64; 3];
                for (i, v) in r.axes_q32.iter().take(3).enumerate() {
                    axes[i] = *v;
                }
                self.civ_or_pending(r.civ_id, r.tick).religion_shifts.push(
                    crate::digest::ReligionRecord {
                        tick: r.tick,
                        axes_q32: axes,
                        dogmatism_q32: r.dogmatism_q32,
                    },
                );
            }
            Event::CivContact(c) => {
                self.contacts.push(Contact {
                    tick: c.tick,
                    civ_a: c.civ_a,
                    civ_b: c.civ_b,
                });
            }
            Event::ConflictResolved(c) => {
                self.conflicts.push(ConflictRecord {
                    tick: c.tick,
                    winner_civ_id: c.winner_civ_id,
                    loser_civ_id: c.loser_civ_id,
                    disputed_cell_count: c.disputed_cell_count,
                    loss_fraction_q32: c.loss_fraction_q32,
                    loser_defeated: c.loser_defeated,
                });
            }
            Event::WarDeclared(w) => {
                let (civ_a, civ_b) = if w.aggressor_civ_id < w.defender_civ_id {
                    (w.aggressor_civ_id, w.defender_civ_id)
                } else {
                    (w.defender_civ_id, w.aggressor_civ_id)
                };
                self.wars.push(crate::digest::WarRecord {
                    start_tick: w.tick,
                    end_tick: None,
                    aggressor_civ_id: w.aggressor_civ_id,
                    defender_civ_id: w.defender_civ_id,
                    civ_a,
                    civ_b,
                    start_belligerence_q32: w.belligerence_q32,
                    start_drive_q32: w.drive_q32,
                    start_kinship_q32: w.kinship_q32,
                    peace_reason: None,
                });
            }
            Event::PeaceConcluded(p) => {
                let pair = (p.civ_a, p.civ_b);
                let reason = match p.reason {
                    protocol::PeaceReason::Defeated => "defeated",
                    protocol::PeaceReason::BelligerenceDropped => "belligerence_dropped",
                    protocol::PeaceReason::TerritoryResolved => "territory_resolved",
                };
                if let Some(war) = self
                    .wars
                    .iter_mut()
                    .rev()
                    .find(|w| (w.civ_a, w.civ_b) == pair && w.end_tick.is_none())
                {
                    war.end_tick = Some(p.tick);
                    war.peace_reason = Some(reason.to_string());
                }
            }
            Event::KnowledgeDiffused(k) => {
                self.diffusions.push(TransferRecord {
                    tick: k.tick,
                    source_civ_id: k.source_civ_id,
                    dest_civ_id: k.dest_civ_id,
                    dest_figure_id: k.dest_figure_id,
                    relation_id: k.relation_id,
                    source_form: k.source_form.clone(),
                    comprehension_q32: k.comprehension_q32,
                });
            }
            Event::KnowledgeTransmitted(k) => {
                self.transmissions.push(TransferRecord {
                    tick: k.tick,
                    source_civ_id: k.source_civ_id,
                    dest_civ_id: k.dest_civ_id,
                    dest_figure_id: k.dest_figure_id,
                    relation_id: k.relation_id,
                    source_form: k.source_form.clone(),
                    comprehension_q32: k.comprehension_q32,
                });
            }
            Event::RunEnd { tick, reason } => {
                self.run_end = Some(RunEnd {
                    tick: *tick,
                    reason: reason.clone(),
                });
            }
            // Periodic Tick / Snapshot events are sequencing markers
            // and state-digest checkpoints respectively; the report
            // renders from the per-feature events between them, so
            // both are absorbed as no-ops here. Snapshot can become
            // a cross-check surface in a later iteration.
            //
            // Measurement relations land in the report through a
            // dedicated measurement-section follow-up; no-op for the
            // M3 digest pipeline.
            Event::Tick(_)
            | Event::Snapshot(_)
            | Event::MeasurementConfirmed(_)
            | Event::RunMetadata(_)
            | Event::SpeciesNomadsChanged(_)
            | Event::SpeciesCosmologyBias(_)
            | Event::SpeciesDrift(_)
            | Event::CohesionShifted(_)
            | Event::RelationMythologized(_)
            | Event::RivalHypothesisProposed(_)
            | Event::PrimaryHypothesisDisplaced(_) => {}
            // M8: per-civ surplus shifts pin into the civ's
            // chapter so the renderer can timeline the economic
            // arc alongside life expectancy / cohesion.
            Event::CivSurplusChanged(s) => {
                let chapter = self.civ_or_pending(s.civ_id, s.tick);
                chapter.surplus_history.push(SurplusSnapshot {
                    tick: s.tick,
                    surplus_q32: s.surplus_q32,
                    previous_q32: s.previous_q32,
                });
            }
            // M8: open routes pin to the top-level trade_routes
            // list. Close events update the matching open entry.
            Event::TradeRouteEstablished(t) => {
                self.trade_routes.push(TradeRouteRecord {
                    start_tick: t.tick,
                    end_tick: None,
                    civ_a: t.civ_a,
                    civ_b: t.civ_b,
                    close_reason: None,
                });
            }
            Event::TradeRouteClosed(t) => {
                // Match the most recent open route for this pair.
                if let Some(route) = self
                    .trade_routes
                    .iter_mut()
                    .rev()
                    .find(|r| r.civ_a == t.civ_a && r.civ_b == t.civ_b && r.end_tick.is_none())
                {
                    route.end_tick = Some(t.tick);
                    route.close_reason = Some(t.reason.clone());
                }
            }
            // Per-civ life expectancy timeline: pin every shift
            // (founding + every >=24-month delta) into the civ's
            // history so the renderer can show the demographic
            // transition.
            Event::CivLifeExpectancyChanged(e) => {
                let chapter = self.civ_or_pending(e.civ_id, e.tick);
                chapter.life_expectancy_history.push(
                    crate::digest::types::LifeExpectancySnapshot {
                        tick: e.tick,
                        life_expectancy_months_q32: e.life_expectancy_months_q32,
                    },
                );
            }
            // Surface emergent templates and tools in the
            // discovering civ's chapter. The civ might not exist
            // in `self.civs` yet if the event arrives before its
            // `CivFounded` (shouldn't happen — emergence
            // requires the civ already have confirmed relations);
            // `civ_or_pending` defaults safely if so.
            Event::TemplateDiscovered(t) => {
                let chapter = self.civ_or_pending(t.civ_id, t.tick);
                chapter.discovered_templates.push(DiscoveredTemplateRecord {
                    tick: t.tick,
                    template_id: t.template_id,
                    template_name: t.template_name.clone(),
                    origin_template_id: t.origin_template_id,
                    threshold_si: t.threshold_si,
                });
            }
            Event::ToolDiscovered(t) => {
                let chapter = self.civ_or_pending(t.civ_id, t.tick);
                chapter.invented_tools.push(InventedToolRecord {
                    tick: t.tick,
                    tool_id: t.tool_id,
                    tool_name: t.tool_name.clone(),
                    channel_focus: t.channel_focus.clone(),
                    cluster_size: t.cluster_size,
                    tier: t.tier,
                    capacity_multiplier_q32: t.capacity_multiplier_q32,
                    literacy_bonus_q32: t.literacy_bonus_q32,
                    transmission_fidelity_bonus_q32: t.transmission_fidelity_bonus_q32,
                });
            }
            Event::RelationFalsified(f) => {
                if let Some(c) = self.civs.get_mut(&f.civ_id) {
                    c.falsified_count = c.falsified_count.saturating_add(1);
                }
            }
            Event::RelationRevalidated(r) => {
                if let Some(c) = self.civs.get_mut(&r.civ_id) {
                    c.revalidated_count = c.revalidated_count.saturating_add(1);
                }
            }
            Event::RelationLapsed(l) => {
                if let Some(c) = self.civs.get_mut(&l.civ_id) {
                    c.lapsed_count = c.lapsed_count.saturating_add(1);
                }
            }
            // PR4: alliance bookkeeping is currently surfaced via
            // the viewport log line; no per-civ digest aggregation
            // yet (the chapter could grow `alliances_formed` /
            // `alliances_dissolved` counters in a follow-up).
            Event::AllianceFormed(_) | Event::AllianceDissolved(_) => {}
            // Sprint 2 Item 6a: extinction events surface in the
            // event log + highlights stream. Per-civ digest
            // aggregation is not in scope for the rule itself; a
            // follow-up can grow ecosystem-side chapters (planet
            // species census) without changing the wire format.
            Event::SpeciesExtinct(_) => {}
        }
    }

    /// Get-or-create a chapter. Used when an event names a `civ_id`
    /// but `CivFounded` for it hasn't landed yet (should be rare;
    /// happens in malformed logs or in tests that only emit a slice).
    /// Founding metadata is corrected when `CivFounded` arrives.
    fn civ_or_pending(&mut self, civ_id: u32, tick: u64) -> &mut CivChapter {
        let entry = self.civs.entry(civ_id).or_insert_with(|| CivChapter {
            civ_id,
            parent_civ_id: None,
            founded_tick: tick,
            founding_figure_count: 0,
            initial_population_q32: 0,
            collapsed: None,
            discoveries: Vec::new(),
            refinements: Vec::new(),
            techs: Vec::new(),
            catastrophes: Vec::new(),
            cosmology_shifts: Vec::new(),
            religion_shifts: Vec::new(),
            figures: Vec::new(),
            claimed_cells: Vec::new(),
            territory_history: Vec::new(),
            revalidated_count: 0,
            lapsed_count: 0,
            falsified_count: 0,
            discovered_templates: Vec::new(),
            invented_tools: Vec::new(),
            life_expectancy_history: Vec::new(),
            surplus_history: Vec::new(),
        });
        if !self.founding_order.contains(&civ_id) {
            self.founding_order.push(civ_id);
        }
        entry
    }

    /// Reverse-lookup a `figure_id` → its civ. Used to attribute
    /// per-figure events (relation confirmed, refinement) back to
    /// their civ chapter. Returns `None` when the figure is unknown.
    fn civ_owning_figure(&self, figure_id: u32) -> Option<u32> {
        self.civs
            .iter()
            .find(|(_, c)| c.figures.iter().any(|f| f.id == figure_id))
            .map(|(id, _)| *id)
    }

    /// Total population at run-end across every civ that didn't
    /// collapse, plus any active civ. Used in the run summary.
    pub fn discovery_count(&self) -> usize {
        self.civs.values().map(|c| c.discoveries.len()).sum()
    }

    pub fn refinement_count(&self) -> usize {
        self.civs.values().map(|c| c.refinements.len()).sum()
    }

    pub fn figure_count(&self) -> usize {
        self.civs.values().map(|c| c.figures.len()).sum()
    }

    pub fn collapsed_civ_count(&self) -> usize {
        self.civs.values().filter(|c| c.collapsed.is_some()).count()
    }
}
