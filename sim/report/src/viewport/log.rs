//! Event → log-line classifier for the viewport. Owns the
//! `log_message` rules plus the small `civ_label` / `relation_label`
//! helpers that format civ + relation references inside log lines.
//!
//! The viewport's scrolling log surfaces a curated subset of the
//! event stream — founding, collapse, conflicts, first-of-kind
//! scientific milestones, cosmology drifts, etc. — and suppresses
//! the high-volume per-tick chatter that would otherwise flood the
//! 3-line log strip. Per-pair / per-kind dedup latches live on
//! `ViewportEmitter` (`wars_logged`, `transmissions_logged`,
//! `templates_confirmed_logged`, …) so this module is a pure
//! `&mut self` formatter that reads + updates those sets.

use super::emitter::ViewportEmitter;
use protocol::Event;
use std::io::Write;

impl<W: Write> ViewportEmitter<W> {
    /// Cosmology axis names, in `axes_q32` vector index
    /// order. Index 0 is the empirical axis, index 4 is
    /// hierarchical. Used by `log_message` to name the dominant
    /// axis on a `CosmologyShifted` event.
    pub(super) const COSMOLOGY_AXIS_NAMES: [&'static str; 5] = [
        "empirical",
        "communitarian",
        "reformist",
        "mystical",
        "hierarchical",
    ];

    /// Resolve a relation_id to a human-readable name. Falls back
    /// to the bare `r{id}` form if the relation hasn't fired a
    /// `RelationConfirmed` yet (transient — most downstream events
    /// arrive after confirmation).
    pub(super) fn relation_label(&self, relation_id: u32) -> String {
        match self.relation_template_names.get(&relation_id) {
            Some(name) => format!("`{name}`"),
            None => format!("r{relation_id}"),
        }
    }

    /// Resolve a civ_id to its human-readable name for log lines.
    /// Falls back to `civ N` when the name hasn't arrived yet (the
    /// first events on a freshly-founded civ may race the
    /// `CivFounded` name capture). The post-collapse cleanup
    /// removes the name, so log lines for a recently-collapsed civ
    /// also fall back — fine, the collapse log line itself logged
    /// the name while it was still present.
    pub(super) fn civ_label(&self, civ_id: u32) -> String {
        self.civ_state
            .get(&civ_id)
            .map(|s| s.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| format!("civ {civ_id}"))
    }

    /// Classify an event for the scrolling log. Returns the
    /// formatted message (without the `[year N]` prefix) when the
    /// event is "significant" enough to surface to a viewer;
    /// returns `None` for routine per-tick chatter (most events).
    ///
    /// Reads `self.civ_state` (the per-civ cosmology snapshot of the
    /// previous `axes_q32` vector, as 5-vec of f64) — when a
    /// `CosmologyShifted` arrives, the delta vector is computed
    /// against the prior snapshot so the log line can name the
    /// dominant axis with signed magnitude (`empirical+0.30`).
    ///
    /// Takes `&mut self` so it can also mutate `wars_logged` to
    /// dedup `ConflictResolved` lines — multi-tick wars emit one
    /// event per cell flipped, so without the per-(winner, loser)
    /// pair latch the log floods with duplicate "civ X defeated"
    /// lines.
    pub(super) fn log_message(&mut self, ev: &Event) -> Option<String> {
        use crate::q32::q32_to_f64;
        match ev {
            Event::CivFounded(f) => {
                // CivFounded races the name capture — fall back to
                // `civ N` if the event arrives with an empty name
                // (some test fixtures do, and the deterministic
                // founding-band reseed could in principle too).
                let label = if f.name.is_empty() {
                    format!("civ {}", f.civ_id)
                } else {
                    f.name.clone()
                };
                Some(format!(
                    "{label} founded ({} cell{})",
                    f.claimed_cells.len(),
                    if f.claimed_cells.len() == 1 { "" } else { "s" }
                ))
            }
            Event::CivCollapsed(c) => Some(format!(
                "{} collapsed ({})",
                self.civ_label(c.civ_id),
                c.reason
            )),
            Event::CatastropheFired(c) => Some(format!(
                "{} hit by {} catastrophe",
                self.civ_label(c.civ_id),
                c.catastrophe_kind
            )),
            // Per-pair coalescing: after two civs first establish
            // contact, dozens of relations stream across in a burst.
            // Surface the burst as a single line per (source, dest)
            // pair for the lifetime of the run.
            Event::KnowledgeTransmitted(k) => {
                let pair = (k.source_civ_id, k.dest_civ_id);
                if !self.transmissions_logged.insert(pair) {
                    return None;
                }
                Some(format!(
                    "{} → {} sharing knowledge",
                    self.civ_label(k.source_civ_id),
                    self.civ_label(k.dest_civ_id)
                ))
            }
            // Suppress mid-war skirmishes. Multi-tick wars emit a
            // `ConflictResolved` per skirmish; only the final one
            // (`loser_defeated = true`) is the "the war ended"
            // beat the viewer cares about. Even with
            // `loser_defeated = true`, the same war can fire one
            // event per cell the loser drops below the defeat
            // floor — so dedup on the (winner, loser) pair via
            // `wars_logged` and emit only the first.
            Event::ConflictResolved(c) => {
                if !c.loser_defeated {
                    return None;
                }
                let pair = (c.winner_civ_id, c.loser_civ_id);
                if !self.wars_logged.insert(pair) {
                    return None;
                }
                Some(format!(
                    "{} defeated {}",
                    self.civ_label(c.winner_civ_id),
                    self.civ_label(c.loser_civ_id),
                ))
            }
            Event::WarDeclared(w) => Some(format!(
                "{} declared war on {}",
                self.civ_label(w.aggressor_civ_id),
                self.civ_label(w.defender_civ_id)
            )),
            Event::PeaceConcluded(p) => {
                let reason = match p.reason {
                    protocol::PeaceReason::Defeated => "defeated",
                    protocol::PeaceReason::BelligerenceDropped => "tensions eased",
                    protocol::PeaceReason::TerritoryResolved => "borders settled",
                };
                Some(format!(
                    "peace: {} and {} ({reason})",
                    self.civ_label(p.civ_a),
                    self.civ_label(p.civ_b)
                ))
            }
            Event::AllianceFormed(a) => Some(format!(
                "{} + {} allied",
                self.civ_label(a.civ_a),
                self.civ_label(a.civ_b)
            )),
            Event::AllianceDissolved(a) => {
                let reason = match a.reason {
                    protocol::AllianceDissolveReason::CosmologyDrift => "drift",
                    protocol::AllianceDissolveReason::WarMisalignment => "war misalignment",
                    protocol::AllianceDissolveReason::TrustEroded => "trust eroded",
                };
                Some(format!(
                    "{} \u{2A2F} {} alliance broke ({reason})",
                    self.civ_label(a.civ_a),
                    self.civ_label(a.civ_b)
                ))
            }
            Event::TechUnlocked(t) => {
                // Serendipitous unlocks read as "stumbled onto"
                // (one prereq waived by lucky-discovery roll). Strict
                // prereq-met unlocks keep the prior verb.
                let verb = if t.serendipitous {
                    "stumbled onto"
                } else {
                    "unlocked"
                };
                Some(format!(
                    "{} {} {}",
                    self.civ_label(t.civ_id),
                    verb,
                    t.tool_name
                ))
            }
            // Surface first-contact + cosmology-shift events
            // in the log too. Both fire rarely (a few times per
            // run) and add narrative beats without the per-tick
            // flooding that filters out the other event kinds.
            Event::CivContact(c) => Some(format!(
                "{} met {}",
                self.civ_label(c.civ_a),
                self.civ_label(c.civ_b)
            )),
            // Name the dominant axis with signed magnitude.
            // The `axes_q32` vector packs 5 axes; we compute
            // the delta against the previous snapshot, find the
            // axis with the largest absolute change, and emit
            // `civ N cosmology shift: {axis}{+/-}{|delta|:.2}`.
            // First shift on a civ has no prior snapshot — treat
            // the prior as zero so the line still names the
            // dominant axis (the absolute position).
            Event::CosmologyShifted(s) => {
                if s.axes_q32.len() < 5 {
                    return Some(format!("{} worldview shifting", self.civ_label(s.civ_id)));
                }
                let mut curr = [0.0_f64; 5];
                for (i, raw) in s.axes_q32.iter().take(5).enumerate() {
                    curr[i] = q32_to_f64(*raw);
                }
                let prev = self
                    .civ_state
                    .get(&s.civ_id)
                    .and_then(|c| c.cosmology)
                    .unwrap_or([0.0; 5]);
                let mut best_idx = 0usize;
                let mut best_abs = 0.0_f64;
                let mut best_delta = 0.0_f64;
                for i in 0..5 {
                    let d = curr[i] - prev[i];
                    if d.abs() > best_abs {
                        best_abs = d.abs();
                        best_idx = i;
                        best_delta = d;
                    }
                }
                let sign = if best_delta >= 0.0 { "+" } else { "-" };
                Some(format!(
                    "{} drifts {}{}{:.2}",
                    self.civ_label(s.civ_id),
                    Self::COSMOLOGY_AXIS_NAMES[best_idx],
                    sign,
                    best_abs
                ))
            }
            // Surface emergent recognition templates + emergent
            // dynamic tools in the live log feed. Both fire at
            // most a handful of times per run on the 600-tick
            // emergence cadence, so they don't flood the log;
            // both are narratively significant beats (the
            // species' first new template; a civ's first invented
            // tool) that deserve mention alongside founding /
            // collapse / contact / cosmology-shift events.
            Event::TemplateDiscovered(t) => Some(format!(
                "{} discovered `{}`",
                self.civ_label(t.civ_id),
                t.template_name
            )),
            Event::ToolDiscovered(t) => Some(format!(
                "{} invented `{}`",
                self.civ_label(t.civ_id),
                t.tool_name
            )),
            // Surface the species starting cosmology bias as a
            // one-shot tick-0 log line. Names the dominant axis
            // with signed magnitude — same shape as the
            // cosmology-shift formatter but reading the bias
            // straight off the event payload (no prior snapshot
            // since the bias IS the starting point).
            Event::SpeciesCosmologyBias(b) => {
                let axes = [
                    ("empirical", q32_to_f64(b.empirical_q32)),
                    ("communitarian", q32_to_f64(b.communitarian_q32)),
                    ("reformist", q32_to_f64(b.reformist_q32)),
                    ("mystical", q32_to_f64(b.mystical_q32)),
                    ("hierarchical", q32_to_f64(b.hierarchical_q32)),
                ];
                let mut best_idx = 0usize;
                let mut best_abs = 0.0_f64;
                for (i, (_, v)) in axes.iter().enumerate() {
                    if v.abs() > best_abs {
                        best_abs = v.abs();
                        best_idx = i;
                    }
                }
                if best_abs < 0.01 {
                    // Neutral starting position — no narrative
                    // beat, suppress.
                    return None;
                }
                let sign = if axes[best_idx].1 >= 0.0 { "+" } else { "-" };
                Some(format!(
                    "species starts {}{}{:.2} {}",
                    sign, "", best_abs, axes[best_idx].0
                ))
            }
            // A rival hypothesis was proposed for an already-
            // confirmed relation. Surfaces the form contest in
            // plain language ("Linear vs ThresholdStep") + the
            // relation's template name when known so the reader
            // sees a paradigm shift in progress rather than a
            // cryptic `r42` id.
            Event::RivalHypothesisProposed(r) => Some(format!(
                "{} debates {} vs {} for {}",
                self.civ_label(r.civ_id),
                r.primary_form,
                r.rival_form,
                self.relation_label(r.relation_id),
            )),
            // Primary-hypothesis displacement — a rival's
            // confidence exceeded the primary's, swapped.
            Event::PrimaryHypothesisDisplaced(p) => Some(format!(
                "{} replaced {} with {} for {}",
                self.civ_label(p.civ_id),
                p.old_form,
                p.new_form,
                self.relation_label(p.relation_id),
            )),
            // Mythologization line. A relation didn't quite
            // make it across the comprehension gate but left a
            // cosmological residue. Names the axis the new civ's
            // cosmology shifted along.
            Event::RelationMythologized(m) => {
                let axis_name = match m.axis {
                    0 => "empirical",
                    1 => "communitarian",
                    2 => "reformist",
                    3 => "mystical",
                    _ => "hierarchical",
                };
                Some(format!(
                    "{} mythologized {} → {axis_name}",
                    self.civ_label(m.dest_civ_id),
                    self.relation_label(m.relation_id),
                ))
            }
            // Cohesion-shift line. Lets the user see when a
            // civ's internal stability degrades toward civil war.
            Event::CohesionShifted(c) => {
                let cur = q32_to_f64(c.cohesion_q32);
                let prev = q32_to_f64(c.previous_q32);
                let dir = if cur > prev { "rising" } else { "falling" };
                Some(format!(
                    "{} cohesion {dir} ({:.0}%)",
                    self.civ_label(c.civ_id),
                    cur * 100.0
                ))
            }
            // Per-civ species drift line. Shows the largest-
            // magnitude delta channel so a long civ chain reveals
            // gradual subspecies divergence.
            Event::SpeciesDrift(d) => {
                let channels = [
                    ("cognition", q32_to_f64(d.cognition_delta_q32)),
                    ("sociality", q32_to_f64(d.sociality_delta_q32)),
                    ("lifespan", q32_to_f64(d.lifespan_delta_years_q32)),
                    ("comms", q32_to_f64(d.communication_fidelity_delta_q32)),
                ];
                let mut best_idx = 0usize;
                let mut best_abs = 0.0_f64;
                for (i, (_, v)) in channels.iter().enumerate() {
                    if v.abs() > best_abs {
                        best_abs = v.abs();
                        best_idx = i;
                    }
                }
                let sign = if channels[best_idx].1 >= 0.0 { "+" } else { "" };
                let unit = if channels[best_idx].0 == "lifespan" {
                    "y"
                } else {
                    ""
                };
                Some(format!(
                    "{} drifted {} {sign}{:.2}{unit}",
                    self.civ_label(d.civ_id),
                    channels[best_idx].0,
                    channels[best_idx].1,
                ))
            }
            // Per-kind first-only: each `relation_id` logs once when
            // the species first confirms it. `RelationConfirmed` is
            // the heartbeat of scientific progress and fires
            // thousands of times per run (every civ confirming every
            // law), so the raw stream would flood the 3-line log.
            // First-confirmation-per-relation gives one line per
            // discovery beat — typically ~50–100 per run.
            Event::RelationConfirmed(r) => {
                if !self.templates_confirmed_logged.insert(r.template_id) {
                    return None;
                }
                Some(format!("species confirmed `{}`", r.template_name))
            }
            // Per-kind first-only on `relation_id`: the species'
            // first refutation of each law. Quotes the template
            // name (the *what*) rather than the cryptic `r42` id.
            Event::RelationFalsified(f) => {
                if !self.relations_falsified_logged.insert(f.relation_id) {
                    return None;
                }
                Some(format!(
                    "{} rejected {} as {}",
                    self.civ_label(f.civ_id),
                    self.relation_label(f.relation_id),
                    f.old_form
                ))
            }
            // Per-kind first-only: surface the first time each law
            // lapses (a transmitted form failed to take hold in the
            // receiving civ).
            Event::RelationLapsed(l) => {
                if !self.relations_lapsed_logged.insert(l.relation_id) {
                    return None;
                }
                Some(format!(
                    "{} lost {} (from {})",
                    self.civ_label(l.civ_id),
                    self.relation_label(l.relation_id),
                    self.civ_label(l.from_civ_id)
                ))
            }
            // Magnitude / threshold: only surface high-charisma
            // figures so the log doesn't fill with every founding
            // band member. Q32.32 charisma in `[0.0, 1.0]`; the
            // 0.7 floor matches "top ~30%" — distinctive figures
            // worth a narrative beat.
            Event::FigureBorn(f) => {
                let charisma = q32_to_f64(f.charisma_q32);
                if charisma < 0.7 {
                    return None;
                }
                Some(format!(
                    "{} born in {} (charisma {:.2})",
                    f.name,
                    self.civ_label(f.civ_id),
                    charisma
                ))
            }
            _ => None,
        }
    }
}
