//! `ViewportEmitter` — the `Emitter` wrapper that mirrors per-civ
//! state from the event stream and re-renders the world to a
//! `Write` (typically stdout) at the configured frame cadence.

use super::ansi::{
    divider, pad_to, split_divider, visible_width, write_centered_line, ANSI_ALT_SCREEN_OFF,
    ANSI_ALT_SCREEN_ON, ANSI_ERASE_LINE, ANSI_ERASE_TO_END, ANSI_HIDE_CURSOR, ANSI_SHOW_CURSOR,
    MAP_WIDTH,
};
use super::config::ViewportConfig;
use crate::frame::{centroid_symbol, civ_color_code, claim_symbol, CivClaim, WorldFrame};
use crate::labels::{
    atmosphere_descriptor, cog_tier, comm_label, format_atmospheric_composition, friendly_badge,
    host_species_status, planet_type, short_manip, short_modality, sociality_label,
    substrate_biochem,
};
use protocol::{Event, Phase, PlanetDerived, PlanetMap, RunMetadata, SpeciesDerived};
use sim_events::{EmitError, Emitter};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Write as FmtWrite;
use std::io::Write;
use std::time::Duration;

use crate::q32::fmt_pop;

/// `out` periodically. See module docs for state rules.
pub struct ViewportEmitter<W: Write> {
    out: W,
    cfg: ViewportConfig,
    planet_map: Option<PlanetMap>,
    planet: Option<PlanetDerived>,
    /// Latest Species event captured. The species is
    /// derived once at run start; this stores it so
    /// the planet card can show name + cognition + primary
    /// modality + primary manipulation.
    species: Option<SpeciesDerived>,
    /// Presentation metadata captured from the
    /// `RunMetadata` event. Carries substrate freeze/boil ranges
    /// (formerly hardcoded in `host_species_status`) plus the
    /// label tables that downstream Python consumers read. The
    /// viewport reads only the substrate ranges; everything else
    /// is sourced via `crate::labels` directly.
    metadata: Option<RunMetadata>,
    civs: BTreeMap<u32, CivClaim>,
    /// Cells where the species has nomadic presence (no
    /// civ claim, but population > `NOMAD_DISPLAY_FLOOR_POP`).
    /// Total nomadic population summed across all cells in
    /// the most recent `SpeciesNomadsChanged` snapshot. Surfaced
    /// in the caption alongside civ count so the user can see the
    /// species' nomadic mass shrink as civs absorb cells.
    nomad_total_pop: f64,
    /// Mirrored from `SpeciesNomadsChanged` events. Rendered as
    /// `0` glyphs in the viewport map.
    nomad_cells: BTreeSet<u32>,
    /// Per-civ sidebar / log state — name, founding year, cosmology
    /// + religion snapshots, tech tier, tools, cohesion, life
    /// expectancy, last unlocked tool. All entries cleared together
    /// on `CivCollapsed` so a re-emergent civ starts fresh. Read
    /// via `Option<&CivState>` with field-level fallbacks where the
    /// renderer needs "civ exists but X never set" semantics.
    civ_state: BTreeMap<u32, CivState>,
    /// Latest tick observed (from `Tick` events). Frame caption
    /// reads "Year N" from here.
    current_tick: u64,
    /// Latch so the alt-screen prologue writes exactly once on
    /// the first frame, regardless of when state becomes
    /// renderable. Final cleanup is keyed on `RunEnd`.
    initialised: bool,
    /// Snapshot counters for the status line.
    civ_founded_count: u64,
    civ_collapsed_count: u64,
    /// Rolling tail of significant events for the
    /// scrolling log section. Each entry is a pre-formatted
    /// `[year N] message` string. Older events drop off the
    /// front when the deque exceeds `cfg.log_lines`.
    recent_events: VecDeque<String>,
    /// Track which (winner, loser) pairs have already had a
    /// "conflict resolved" line emitted to the log. Multi-tick
    /// wars produce one `ConflictResolved` event per cell-flip-
    /// with-loser-defeated-true, which would flood the log. We
    /// emit only the first defeat per pair and reset after the
    /// loser collapses (so a re-emerged civ can have its war
    /// re-emitted later).
    wars_logged: BTreeSet<(u32, u32)>,
    /// First-only filter for `RelationConfirmed`. The science
    /// heartbeat fires thousands of times per run (every civ
    /// confirming every law). Dedup on `template_id` — `relation_id`
    /// is per-civ, so the same `fire` template confirmed by five
    /// civs would still fire five log lines under a `relation_id`
    /// filter. Per-template gives the species-level beat once per
    /// phenomenon.
    templates_confirmed_logged: BTreeSet<u32>,
    /// First-only filter for `RelationFalsified` (per `relation_id`
    /// — falsification is per-civ news, distinct civs falsifying
    /// the same template are separate beats).
    relations_falsified_logged: BTreeSet<u32>,
    /// First-only filter for `RelationLapsed` (per `relation_id`).
    relations_lapsed_logged: BTreeSet<u32>,
    /// Per-pair coalescing for `KnowledgeTransmitted`. After two
    /// civs first establish contact, dozens of relations stream
    /// across in a burst; we collapse the burst into one log line
    /// per (source, dest) pair for the lifetime of the run.
    transmissions_logged: BTreeSet<(u32, u32)>,
    /// Civ pairs currently at war. Mirrors core's
    /// `war_state` map so the per-civ sidebar panels can show
    /// `⚔ war: civ X` lines without re-deriving from
    /// `ConflictResolved` cell-overlap noise. Pair key is
    /// normalised `(min, max)`.
    wars_active: BTreeSet<(u32, u32)>,
    /// `relation_id` → `template_name` map populated on the first
    /// `RelationConfirmed` we see for each relation. Log lines for
    /// downstream relation events (falsified / lapsed / rival /
    /// displaced / mythologized) look up the template name here
    /// so they read as `lost "water flows downhill"` instead of
    /// the cryptic `lost r438`.
    relation_template_names: BTreeMap<u32, String>,
    /// Per-civ snapshot of total population (raw Q96.32 bits)
    /// captured at the end of the previous render. Compared
    /// against the new total at render time so the sidebar's pop
    /// line gets a ↑ / ↓ / → trend arrow. ±0.5% threshold keeps
    /// the arrow stable under sub-tick noise. Cleared on
    /// `CivCollapsed`.
    civ_last_emitted_pop_q32: BTreeMap<u32, i128>,
}

/// Per-civ snapshot state surfaced by the sidebar panels and
/// log lines. One entry per active civ; cleared on `CivCollapsed`.
/// Fields that have natural defaults (empty `String`, `0` tier,
/// empty `BTreeSet`) use them directly; the rest are `Option` so
/// "civ exists but field never observed" reads distinctly from
/// "field has its default value".
#[derive(Debug, Clone, Default)]
struct CivState {
    name: String,
    founded_year: u64,
    cosmology: Option<[f64; 5]>,
    religion: Option<[f64; 3]>,
    tech_tier: u8,
    tools_unlocked: BTreeSet<String>,
    cohesion: Option<f64>,
    life_expectancy_months: Option<f64>,
    last_unlocked_tool: Option<String>,
}

impl<W: Write> ViewportEmitter<W> {
    pub fn new(out: W, cfg: ViewportConfig) -> Self {
        Self {
            out,
            cfg,
            planet_map: None,
            planet: None,
            species: None,
            metadata: None,
            civs: BTreeMap::new(),
            nomad_cells: BTreeSet::new(),
            nomad_total_pop: 0.0,
            civ_state: BTreeMap::new(),
            current_tick: 0,
            initialised: false,
            civ_founded_count: 0,
            civ_collapsed_count: 0,
            recent_events: VecDeque::new(),
            wars_logged: BTreeSet::new(),
            templates_confirmed_logged: BTreeSet::new(),
            relations_falsified_logged: BTreeSet::new(),
            relations_lapsed_logged: BTreeSet::new(),
            transmissions_logged: BTreeSet::new(),
            wars_active: BTreeSet::new(),
            relation_template_names: BTreeMap::new(),
            civ_last_emitted_pop_q32: BTreeMap::new(),
        }
    }

    /// Resolve a relation_id to a human-readable name. Falls back
    /// to the bare `r{id}` form if the relation hasn't fired a
    /// `RelationConfirmed` yet (transient — most downstream events
    /// arrive after confirmation).
    fn relation_label(&self, relation_id: u32) -> String {
        match self.relation_template_names.get(&relation_id) {
            Some(name) => format!("`{name}`"),
            None => format!("r{relation_id}"),
        }
    }

    /// Cosmology axis names, in `axes_q32` vector index
    /// order. Index 0 is the empirical axis, index 4 is
    /// hierarchical. Used by `log_message` to name the dominant
    /// axis on a `CosmologyShifted` event.
    const COSMOLOGY_AXIS_NAMES: [&'static str; 5] = [
        "empirical",
        "communitarian",
        "reformist",
        "mystical",
        "hierarchical",
    ];

    /// Resolve a civ_id to its human-readable name for log lines.
    /// Falls back to `civ N` when the name hasn't arrived yet (the
    /// first events on a freshly-founded civ may race the
    /// `CivFounded` name capture). The post-collapse cleanup
    /// removes the name, so log lines for a recently-collapsed civ
    /// also fall back — fine, the collapse log line itself logged
    /// the name while it was still present.
    fn civ_label(&self, civ_id: u32) -> String {
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
    fn log_message(&mut self, ev: &Event) -> Option<String> {
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
                use crate::q32::q32_to_f64;
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

    /// Format the planet card for the top of the viewport. Two
    /// short lines of compact stats, each ≤ 32 chars so the card
    /// fits on portrait phone terminals (iPhone Termius narrowest
    /// column is ~30; this leaves 2 chars margin). Static for the
    /// run since `Planet` is emitted once.
    fn planet_card(&self) -> Option<String> {
        use crate::q32::q32_to_f64;
        let p = self.planet.as_ref()?;
        let mut s = String::new();
        // Card layout groups thematically-related fields per
        // line. Planet name lives in the section divider (rendered
        // by `render()`); species + bio info lives in a dedicated
        // species section (see `species_card()`). The planet card
        // covers only the *world* — type, climate, orbital
        // mechanics.
        let mean_t_k = q32_to_f64(p.mean_temperature_q32);
        // Substrate freeze/boil come from the captured
        // `RunMetadata` event (sourced upstream from
        // `sim_physics::chemistry::substrate_properties`).
        // Apply the per-seed perturbation
        // (`p.substrate_perturbation_q32`) so the displayed values
        // match what `Chemistry::for_planet_with_perturbation`
        // actually wired into the run's physics. Without this the
        // card showed water freezing at 273.15 K every seed even
        // though seed-42's effective freeze point might be 271.7 K.
        let perturb = q32_to_f64(p.substrate_perturbation_q32);
        let (freeze_k, boil_k) = self.metadata.as_ref().map_or((0.0, 0.0), |m| {
            let nominal_freeze = m
                .substrate_freeze_k
                .get(&p.metabolic_substrate)
                .copied()
                .unwrap_or(0.0);
            let nominal_boil = m
                .substrate_boil_k
                .get(&p.metabolic_substrate)
                .copied()
                .unwrap_or(0.0);
            (
                nominal_freeze * (1.0 + perturb),
                nominal_boil * (1.0 + perturb),
            )
        });
        let badge = host_species_status(
            &p.metabolic_substrate,
            &p.atmosphere,
            mean_t_k,
            freeze_k,
            boil_k,
        );
        let badge_friendly = friendly_badge(badge);
        let ptype = planet_type(p.metabolic_substrate.as_str());
        // Line 1: planet-type noun · friendly badge — leads with
        // the type archetype and follows with a one-word
        // habitability descriptor (e.g. `ocean world · scorching`).
        let _ = writeln!(s, "{ptype} · {badge_friendly}");
        // Line 2 (climate): atmosphere · temperature ·
        // magnetosphere — the three "what's the air / sky like"
        // fields. `none` magnetosphere collapses to `no`
        // (reads better than "none mag").
        let mean_t_display = self.cfg.temperature_unit.from_kelvin(mean_t_k);
        let temp_suffix = self.cfg.temperature_unit.suffix();
        let mag_label = if p.magnetosphere == "none" {
            "no"
        } else {
            p.magnetosphere.as_str()
        };
        let atm_desc = atmosphere_descriptor(p.atmosphere.as_str());
        let _ = writeln!(
            s,
            "{atm_desc} · {mean_t_display:.0}{temp_suffix} · {mag_label} mag",
        );
        // Atmospheric composition — top three channels by
        // mass fraction, e.g. `78%N₂ 21%O₂ 1%Ar`. Skipped on
        // vacuum (sum ≈ 0). Older event logs default all
        // composition channels to 0 and fall through to vacuum.
        if let Some(line) = format_atmospheric_composition(p) {
            let _ = writeln!(s, "{line}");
        }
        // Line 3 (orbital): day · year · tilt · moons —
        // the rotation/orbit/satellite fields a reader would
        // associate with "what does the sky cycle look like".
        let _ = writeln!(
            s,
            "{:.0}h · {}mo · {:.0}° · {} moon{}",
            q32_to_f64(p.day_length_hours_q32),
            p.orbital_period_months,
            q32_to_f64(p.axial_tilt_deg_q32),
            p.moon_count,
            if p.moon_count == 1 { "" } else { "s" },
        );
        Some(s)
    }

    /// Species card body. Returns `None` until both `Planet`
    /// and `Species` events have arrived (the biochem axis needs
    /// the planet's substrate). Three lines:
    ///
    /// 1. *Cognition* — full-word topology + tier phrase
    ///    (`centralized medium cognition`).
    /// 2. *Senses + manipulation* — primary modality and primary
    ///    manipulation mode, prefixed with `sense:` / `manip:`
    ///    so the reader doesn't have to know the order convention.
    /// 3. *Biology* — lifespan years + sociality tier + comm tier
    ///    + substrate-implied biochemistry.
    ///
    /// The species name is *not* repeated here — `render()` writes
    /// it as the section divider label (`---- Cyranites ----`).
    fn species_card(&self) -> Option<String> {
        use crate::q32::q32_to_f64;
        let p = self.planet.as_ref()?;
        let sp = self.species.as_ref()?;
        let mut s = String::new();
        // Line 1: cognition phrase. `{topology} {tier} cognition`
        // — a noun phrase that reads naturally. Tier bucket comes
        // from the shared `labels::cog_tier` so the boundaries
        // match every other consumer.
        let cog = q32_to_f64(sp.cognition_q32);
        let cog_tier_word = cog_tier(cog);
        let topo_full = match sp.cognition_topology.as_str() {
            "centralized" => "centralized",
            "distributed" => "distributed",
            _ => "unknown",
        };
        let _ = writeln!(s, "{topo_full} {cog_tier_word} cognition");
        // Line 2: senses + manipulation, labeled.
        let primary_modality = sp.modalities.first().map_or("?", String::as_str);
        let primary_manip = sp.manipulation_modes.first().map_or("?", String::as_str);
        let _ = writeln!(
            s,
            "sense: {} · manip: {}",
            short_modality(primary_modality),
            short_manip(primary_manip),
        );
        // Line 3: biology — lifespan, sociality, comm, biochem.
        let lifespan_years = q32_to_f64(sp.lifespan_years_q32) as i64;
        let soc_word = sociality_label(q32_to_f64(sp.sociality_q32));
        let comm_word = comm_label(q32_to_f64(sp.communication_fidelity_q32));
        let biochem = substrate_biochem(p.metabolic_substrate.as_str());
        let _ = writeln!(
            s,
            "{lifespan_years}y · {soc_word} · {comm_word} · {biochem}",
        );
        Some(s)
    }

    fn ensure_initialised(&mut self) -> std::io::Result<()> {
        if self.initialised {
            return Ok(());
        }
        if self.cfg.use_alt_screen {
            self.out.write_all(ANSI_ALT_SCREEN_ON.as_bytes())?;
            self.out.write_all(ANSI_HIDE_CURSOR.as_bytes())?;
        }
        self.initialised = true;
        Ok(())
    }

    fn shutdown(&mut self) -> std::io::Result<()> {
        if self.initialised && self.cfg.use_alt_screen {
            self.out.write_all(ANSI_SHOW_CURSOR.as_bytes())?;
            self.out.write_all(ANSI_ALT_SCREEN_OFF.as_bytes())?;
        }
        Ok(())
    }

    /// Apply state-mirroring rules. Returns true if a rendering-
    /// relevant change occurred (so the caller knows the next
    /// frame would differ); the actual frame cadence is still
    /// gated on `frame_every`.
    fn apply_state(&mut self, ev: &Event) {
        // Run log_message *before* the state-mutation match below.
        // Some events (notably CivCollapsed) clear identifying state
        // — civ_state.remove drops the civ's name — and the log line
        // then falls back to "civ {id}" instead of the civ's actual
        // name. Logging first reads the still-present name, then the
        // match block does the cleanup.
        if self.cfg.log_lines > 0 {
            if let Some(msg) = self.log_message(ev) {
                let period = self
                    .planet
                    .as_ref()
                    .map_or(protocol::BASELINE_MONTHS_PER_YEAR as u32, |p| {
                        p.orbital_period_months
                    });
                let year = protocol::year_of_tick_for_period(self.current_tick, period);
                // Same-tick contact coalescing: when several civs
                // contact the same partner in one tick (typically a
                // newly-founded civ being met by all neighbours at
                // once), merge into one line — "Karnan, Goran met
                // Yothan" instead of two separate "X met Y" lines.
                if let Event::CivContact(c) = ev {
                    let partner = self.civ_label(c.civ_b);
                    let initiator = self.civ_label(c.civ_a);
                    let suffix = format!(" met {partner}");
                    let line_to_push = if let Some(last) = self.recent_events.back_mut() {
                        let prefix = format!("y{year} ");
                        if last.starts_with(&prefix) && last.ends_with(&suffix) {
                            let head = &last[prefix.len()..last.len() - suffix.len()];
                            *last = format!("{prefix}{head}, {initiator}{suffix}");
                            None
                        } else {
                            Some(format!("y{year} {msg}"))
                        }
                    } else {
                        Some(format!("y{year} {msg}"))
                    };
                    if let Some(line) = line_to_push {
                        self.recent_events.push_back(line);
                    }
                } else {
                    self.recent_events.push_back(format!("y{year} {msg}"));
                }
                while self.recent_events.len() > self.cfg.log_lines {
                    self.recent_events.pop_front();
                }
            }
        }
        match ev {
            Event::PlanetMap(pm) => {
                self.planet_map = Some(pm.clone());
            }
            Event::Planet(p) => {
                self.planet = Some(p.clone());
            }
            Event::Species(s) => {
                self.species = Some(s.clone());
            }
            Event::RunMetadata(m) => {
                // Capture once per run; the freeze/boil
                // ranges drive `host_species_status`.
                self.metadata = Some(m.clone());
            }
            Event::CivFounded(f) => {
                self.civ_founded_count = self.civ_founded_count.saturating_add(1);
                let claims: std::collections::BTreeSet<u32> =
                    f.claimed_cells.iter().copied().collect();
                let centroid = claims.iter().next().copied().unwrap_or(0);
                // Seed per-cell pop with an even split of the
                // founding population across claimed cells. Mirrors
                // what sim/core's M5 distribution does at founding,
                // and keeps the sidebar from rendering "0p" for the
                // many ticks between founding and the first
                // CivTerritoryChanged event (which only fires when
                // claimed_cells actually changes — typically tens of
                // sim-years after founding). The next
                // CivTerritoryChanged refines per-cell totals.
                let n = f.claimed_cells.len() as i128;
                let per_cell: i128 = if n > 0 {
                    f.initial_population_q32 / n
                } else {
                    0
                };
                let cell_populations_q32: std::collections::BTreeMap<u32, i128> = f
                    .claimed_cells
                    .iter()
                    .copied()
                    .map(|c| (c, per_cell))
                    .collect();
                // Per-cell caps now arrive in `CivFounded` itself
                // (paired with `claimed_cells`). Without this, the
                // frame renderer hits the `frame_max_pop` fallback
                // for any civ that hasn't yet seen a
                // `CivTerritoryChanged` — and since founding seeds
                // each cell with the same `pop / n`, every founder
                // cell ties for digit `9`. With the cap map seeded
                // here, pop/cap ratios read correctly from tick 0.
                let cell_capacities_q32: std::collections::BTreeMap<u32, i128> =
                    if f.cell_capacities_q32.len() == f.claimed_cells.len() {
                        f.claimed_cells
                            .iter()
                            .copied()
                            .zip(f.cell_capacities_q32.iter().copied())
                            .collect()
                    } else {
                        std::collections::BTreeMap::new()
                    };
                self.civs.insert(
                    f.civ_id,
                    CivClaim {
                        civ_id: f.civ_id,
                        claimed_cells: claims,
                        centroid,
                        cell_populations_q32,
                        cell_capacities_q32,
                    },
                );
                // Capture civ name and founding year for the
                // per-civ sidebar panel. Year derives from the
                // founding tick (sim ticks = months).
                let period = self
                    .planet
                    .as_ref()
                    .map_or(protocol::BASELINE_MONTHS_PER_YEAR as u32, |p| {
                        p.orbital_period_months
                    });
                let s = self.civ_state.entry(f.civ_id).or_default();
                s.name = f.name.clone();
                s.founded_year = protocol::year_of_tick_for_period(f.tick, period);
            }
            Event::SpeciesNomadsChanged(n) => {
                // Replace the nomad-cell set with the new tick's
                // snapshot. Cells with population above the
                // protocol-pinned floor render as `0` in the map.
                // Also accumulate the total population across
                // all nomad cells for the caption display.
                use crate::q32::q32_to_f64;
                self.nomad_cells.clear();
                let mut total = 0.0_f64;
                for (cell, pop_q32) in n.cells.iter().zip(n.population_q32.iter()) {
                    let pop = q32_to_f64(*pop_q32);
                    total += pop;
                    if pop > protocol::NOMAD_DISPLAY_FLOOR_POP {
                        self.nomad_cells.insert(*cell);
                    }
                }
                self.nomad_total_pop = total;
            }
            Event::CivTerritoryChanged(t) => {
                if let Some(c) = self.civs.get_mut(&t.civ_id) {
                    c.claimed_cells = t.claimed_cells.iter().copied().collect();
                    if t.cell_populations_q32.len() == t.claimed_cells.len() {
                        c.cell_populations_q32 = t
                            .claimed_cells
                            .iter()
                            .copied()
                            .zip(t.cell_populations_q32.iter().copied())
                            .collect();
                    }
                    if t.cell_capacities_q32.len() == t.claimed_cells.len() {
                        c.cell_capacities_q32 = t
                            .claimed_cells
                            .iter()
                            .copied()
                            .zip(t.cell_capacities_q32.iter().copied())
                            .collect();
                    }
                }
            }
            Event::CivCollapsed(c) => {
                self.civ_collapsed_count = self.civ_collapsed_count.saturating_add(1);
                self.civs.remove(&c.civ_id);
                // Drop sidebar-only state for the collapsed civ
                // so its panel disappears on the next frame. A
                // refound civ gets a fresh `CivFounded` and
                // re-populates these entries. The cosmology
                // snapshot drops too, so a re-emergent civ_id
                // compares against zero, not stale state.
                self.civ_state.remove(&c.civ_id);
                self.civ_last_emitted_pop_q32.remove(&c.civ_id);
                // Drop any war pairs touching this civ so a
                // re-emerged civ_id can re-trigger a fresh
                // "conflict resolved" line. Pairs are stored as
                // (winner, loser); both sides referencing the
                // collapsing civ_id need to clear.
                self.wars_logged
                    .retain(|(w, l)| *w != c.civ_id && *l != c.civ_id);
                // Drop any active-war entries touching the
                // collapsed civ. Core also emits PeaceConcluded for
                // these on the next conflict check, but sidebar
                // panels stop rendering for collapsed civs anyway,
                // so the early prune just keeps the set tidy.
                self.wars_active
                    .retain(|(a, b)| *a != c.civ_id && *b != c.civ_id);
            }
            Event::WarDeclared(w) => {
                let pair = if w.aggressor_civ_id < w.defender_civ_id {
                    (w.aggressor_civ_id, w.defender_civ_id)
                } else {
                    (w.defender_civ_id, w.aggressor_civ_id)
                };
                self.wars_active.insert(pair);
            }
            Event::PeaceConcluded(p) => {
                self.wars_active.remove(&(p.civ_a, p.civ_b));
            }
            Event::Tick(t) => {
                self.current_tick = t.tick;
            }
            Event::TechUnlocked(t) => {
                let s = self.civ_state.entry(t.civ_id).or_default();
                if t.tier > s.tech_tier {
                    s.tech_tier = t.tier;
                }
                s.tools_unlocked.insert(t.tool_name.clone());
                s.last_unlocked_tool = Some(t.tool_name.clone());
            }
            Event::CohesionShifted(c) => {
                use crate::q32::q32_to_f64;
                self.civ_state.entry(c.civ_id).or_default().cohesion =
                    Some(q32_to_f64(c.cohesion_q32));
            }
            Event::CivLifeExpectancyChanged(l) => {
                use crate::q32::q32_to_f64;
                self.civ_state
                    .entry(l.civ_id)
                    .or_default()
                    .life_expectancy_months = Some(q32_to_f64(l.life_expectancy_months_q32));
            }
            Event::RelationConfirmed(r) => {
                // First-confirmation wins; later civs confirming
                // the same relation_id (across collapse boundaries)
                // would just re-insert the same name, but a stable
                // first-write avoids needless map churn on every
                // confirmation.
                self.relation_template_names
                    .entry(r.relation_id)
                    .or_insert_with(|| r.template_name.clone());
            }
            _ => {}
        }
        // Log line emission happens *before* the match block above
        // so events that drop identifying state (e.g. CivCollapsed
        // removing civ_state) don't strip names before they're
        // logged.
        // Update the per-civ cosmology snapshot *after*
        // `log_message` reads it, so the next shift's delta is
        // computed against this just-arrived axes vector.
        if let Event::CosmologyShifted(s) = ev {
            if s.axes_q32.len() >= 5 {
                use crate::q32::q32_to_f64;
                let mut snap = [0.0_f64; 5];
                for (i, raw) in s.axes_q32.iter().take(5).enumerate() {
                    snap[i] = q32_to_f64(*raw);
                }
                self.civ_state.entry(s.civ_id).or_default().cosmology = Some(snap);
            }
        }
        // Mirror the religion vector so the per-civ sidebar
        // panel can name the dominant axis.
        if let Event::ReligionShifted(r) = ev {
            if r.axes_q32.len() >= 3 {
                use crate::q32::q32_to_f64;
                let mut snap = [0.0_f64; 3];
                for (i, raw) in r.axes_q32.iter().take(3).enumerate() {
                    snap[i] = q32_to_f64(*raw);
                }
                self.civ_state.entry(r.civ_id).or_default().religion = Some(snap);
            }
        }
    }

    fn should_render(&self, ev: &Event) -> bool {
        match ev {
            Event::Tick(t) if matches!(t.phase, Phase::TickEnd) => {
                if self.cfg.frame_every == 0 {
                    return false;
                }
                t.tick % self.cfg.frame_every == 0
            }
            Event::RunEnd { .. } => true,
            _ => false,
        }
    }

    /// Build the right-hand sidebar lines as a `Vec<String>`.
    /// Three sub-blocks separated by blank lines:
    ///
    /// 1. **Legend** — extended glyph reference (covers all
    ///    fallback glyphs `·` / `≡` plus the existing civ +
    ///    terrain glyphs). 4 lines so each line stays ≤ 28 cols.
    /// 2. **Species** — re-uses `species_card()`'s 3-line body so
    ///    the cognition / sense / biology summary stays visible
    ///    alongside the map.
    /// 3. **Per-civ panels** — one block per currently-active civ.
    ///    Each block: `─── {Civ name} ───`, then an identity line
    ///    that doubles as a colour swatch. In colored mode the
    ///    civ's name + `{centroid_letter}=cap · 0-9=pop` glyphs
    ///    render in the civ's palette colour so a reader can match
    ///    the panel to its cells on the map at a glance. In mono
    ///    mode the identity line falls back to the legacy
    ///    `{centroid_letter}=cap · {claim_digit}=civ` since the
    ///    monochrome map still uses civ-id digits for territory.
    ///    The block then ends with `y{founded_year} · {N} cells`.
    ///    Civs auto-add on `CivFounded` and auto-drop on
    ///    `CivCollapsed` since this just iterates `self.civs`.
    ///
    /// Sub-blocks are separated by an empty `String` for visual
    /// breathing room. Returns the line vector unwrapped — caller
    /// pads each to `SIDEBAR_WIDTH` cols when zipping with the
    /// map rows.
    fn build_sidebar_lines(&mut self) -> Vec<String> {
        // Legend: all fallback glyphs included; split across 3
        // lines so each fits in ~28 cols. The redundant
        // `A=cap · 1=civ` entries were dropped — every per-civ
        // panel already carries its own colored identity line, so
        // the global key only needs to cover glyphs that aren't
        // surfaced per-civ. `#=war` stays (it's a global glyph
        // for disputed cells, not tied to any one civ). In colored
        // mode line 1 picks up a `1-9=fill` hint so the reader
        // knows the per-civ digit reads as cap-relative density
        // on a linear scale (digit 9 = ≥90% of cap, digit 1 ≈ 10%
        // of cap; cells below 10% show terrain in civ colour).
        // Civ identity is carried by colour. `0` stays mapped to
        // unclaimed nomadic presence. Mono mode keeps the legacy
        // line — there the per-civ digit is the civ-id, not pop.
        // Colored mode: per-cell digit is pop fill-%; civ identity
        // is carried by colour. Mono mode (markdown / no ANSI): the
        // digit is the civ-id, `*` covers civ ids ≥ 10. Same
        // glyphs surface for nomads + disputes in both. The
        // line-1 disambiguation matters because the digit reads
        // *very* differently between the two modes.
        // In density mode the per-cell symbol stays as the terrain
        // glyph (so land/coast/peak/plain remains readable) but
        // brightness encodes pop fill — bold = dense, normal =
        // mid, dim = sparse. In digit mode (legacy) the colored
        // variant uses `1-9=fill%`.
        let mut lines: Vec<String> = if self.cfg.use_color {
            // Nomads share the terrain-glyph shape with unclaimed
            // terrain; they're distinguished only by colour (bold
            // white vs the muted terrain palette), so the legend
            // notes "white=nomad" rather than a glyph mapping.
            let density_line = if self.cfg.density_mode {
                "dim/bold=fill% · white=nomad · #=war"
            } else {
                "1-9=fill% · white=nomad · #=war"
            };
            vec![
                density_line.to_string(),
                "~sea · ≈deep · ▲peak · △hill".to_string(),
                "▒land · ░coast · ·=plain".to_string(),
            ]
        } else {
            vec![
                "1-9=civ-id · *=civ≥10".to_string(),
                "0=nomad · #=war · ~sea · ≈deep".to_string(),
                "▲peak · △hill · ▒land · ░coast · ·=plain".to_string(),
            ]
        };
        // Species sub-block (3 lines from species_card()).
        if let Some(species_body) = self.species_card() {
            lines.push(String::new());
            // Section header so the reader knows what these 3
            // lines are; species name from the captured Species
            // event.
            let species_label = self.species.as_ref().map_or("species", |s| s.name.as_str());
            lines.push(format!("─── {species_label} ───"));
            for line in species_body.lines() {
                lines.push(line.to_string());
            }
        }
        // Per-civ panels: one block per currently-active civ.
        // Order panels by total population (largest first) so the
        // dominant civ surfaces at the top of the sidebar; on ties
        // fall back to civ_id ascending for deterministic output.
        // Sums use i64 saturating add on the raw Q32.32 fixed-point
        // values so ranking is exact rather than depending on f64
        // round-off across many cells.
        //
        // Capture per-civ Q32 sums into a local map up-front: the
        // same total drives the panel sort *and* the ↑/↓ trend
        // arrow on the pop line (compared against the previous
        // render's snapshot in `civ_last_emitted_pop_q32`). The
        // snapshot map is replaced at the end of this method.
        let civ_pop_q32: BTreeMap<u32, i128> = self
            .civs
            .iter()
            .map(|(id, claim)| {
                let sum = claim
                    .cell_populations_q32
                    .values()
                    .copied()
                    .fold(0i128, i128::saturating_add);
                (*id, sum)
            })
            .collect();
        let mut civ_order: Vec<(&u32, &CivClaim)> = self.civs.iter().collect();
        civ_order.sort_by(|a, b| {
            let pa = civ_pop_q32.get(a.0).copied().unwrap_or(0);
            let pb = civ_pop_q32.get(b.0).copied().unwrap_or(0);
            pb.cmp(&pa).then_with(|| a.0.cmp(b.0))
        });
        for (civ_id, claim) in civ_order {
            lines.push(String::new());
            let name = self.civ_state.get(civ_id).map_or("", |s| s.name.as_str());
            // In colored mode, paint the civ's name (or `civ N`
            // fallback) in its palette colour so the sidebar panel
            // doubles as a colour swatch — the user can match the
            // legend entry to the cells on the map at a glance.
            // `\x1b[22;39m` resets bold + foreground without
            // touching the surrounding dim wrap applied at render
            // time, so the rest of the panel stays dim-styled.
            let (open, close) = if self.cfg.use_color {
                (
                    format!("\x1b[1;38;5;{}m", civ_color_code(*civ_id)),
                    "\x1b[22;39m".to_string(),
                )
            } else {
                (String::new(), String::new())
            };
            let header = if name.is_empty() {
                format!("─── {open}civ {civ_id}{close} ───")
            } else {
                format!("─── {open}{name}{close} ───")
            };
            lines.push(header);
            // Tech tier sits on the identity line next to the
            // capital-letter / pop-digit swatch so the reader can
            // scan "what marker, which population, what era" in
            // one row. Tier defaults to 0 until the first
            // `TechUnlocked` event arrives.
            let state = self.civ_state.get(civ_id);
            let tier = state.map_or(0, |s| s.tech_tier);
            let tool_count = state.map_or(0, |s| s.tools_unlocked.len());
            // Identity line: capital letter (still A..Z by civ_id)
            // is the on-map marker for this civ's centroid; the
            // `0-9` is a colored swatch standing in for the
            // pop-scaled digits the civ's territory cells render
            // as. In monochrome mode there's no colour, so fall
            // back to the legacy `{letter}=cap · {digit}=civ` line.
            // Tier + tool count surface era + breadth-of-tech-tree
            // in one row alongside the cap/pop swatch.
            let identity = if self.cfg.use_color {
                // In density mode territory cells render as the
                // terrain glyph in the civ's colour, with brightness
                // scaled by population (bold ≥ 60%, normal ≥ 30%,
                // dim < 30%). The legend row shows the ladder with
                // the civ's actual ANSI attributes applied so the
                // reader can match swatch ↔ density at a glance.
                // Digit mode keeps the legacy `0-9` swatch.
                let pop_swatch = if self.cfg.density_mode {
                    let code = crate::frame::civ_color_code(*civ_id);
                    format!(
                        "\x1b[2;38;5;{code}m▒\x1b[0m\x1b[38;5;{code}m▒\x1b[0m\x1b[1;38;5;{code}m▒\x1b[0m",
                    )
                } else {
                    format!("{open}0-9{close}")
                };
                format!(
                    "{open}{cap}{close}=cap · {pop_swatch}=pop · t{tier} · {tool_count} tools",
                    open = open,
                    close = close,
                    cap = centroid_symbol(*civ_id),
                )
            } else {
                format!(
                    "{}=cap · {}=civ · t{tier} · {tool_count} tools",
                    centroid_symbol(*civ_id),
                    claim_symbol(*civ_id),
                )
            };
            lines.push(identity);
            // Most-recent unlock surfaces underneath the identity
            // line when present. Skipped on civs that haven't
            // unlocked anything yet so brand-new founders stay
            // visually compact. The tool count above already says
            // "0 tools" in that case, so the missing `last:` line
            // is unambiguous.
            if let Some(tool) = state.and_then(|s| s.last_unlocked_tool.as_ref()) {
                lines.push(format!("last: {tool}"));
            }
            let founded_year = state.map_or(0, |s| s.founded_year);
            // Per-civ population count alongside founding
            // year + cell count. Sum the per-cell Q32.32
            // populations from `cell_populations_q32` so the
            // sidebar surfaces "civ Foo has 247 people across 5
            // cells" — the user can see civs grow / shrink from
            // the panel without having to read the NDJSON.
            let civ_pop: f64 = claim
                .cell_populations_q32
                .values()
                .map(|p| crate::q32::pop_q32_to_f64(*p))
                .sum();
            // Sub-integer floating-point noise can sum to -0.0 or a
            // tiny negative value, which `{:.0}` then renders as
            // "-0p" in the sidebar. Population can't be negative —
            // clamp before formatting.
            //
            // Trend arrow against the previous render's snapshot.
            // ±0.5% deadband suppresses jitter from monthly
            // food-cycle noise; first-frame civs get `→` (no prior
            // sample). Reads `civ_last_emitted_pop_q32`; the
            // snapshot is rewritten at the end of this method.
            let cur_q32 = civ_pop_q32.get(civ_id).copied().unwrap_or(0);
            let trend = match self.civ_last_emitted_pop_q32.get(civ_id).copied() {
                None => '\u{2192}', // → (newly founded, no prior)
                Some(prev) => {
                    // 0.5% of |prev| as a threshold; saturating to
                    // avoid overflow when prev is near i128::MAX.
                    let band = prev.saturating_abs() / 200;
                    let delta = cur_q32.saturating_sub(prev);
                    if delta > band {
                        '\u{2191}' // ↑
                    } else if delta < -band {
                        '\u{2193}' // ↓
                    } else {
                        '\u{2192}' // →
                    }
                }
            };
            lines.push(format!(
                "y{} · {} cells · {}p {}",
                founded_year,
                claim.claimed_cells.len(),
                fmt_pop(civ_pop),
                trend,
            ));
            // Stats line: cohesion + life expectancy, both pulled
            // from the running per-civ snapshots. Cohesion shown as
            // a 0-100 percentage so the reader can read it against
            // the civil-war floor (~10) and breakaway band
            // (10-35). Life expectancy from months → years using
            // the planet's actual orbital period (matches the
            // year display in the caption). Always emitted so the
            // panel keeps a fixed line count per civ.
            let cohesion_pct = state
                .and_then(|s| s.cohesion)
                .map_or(100, |c| (c * 100.0).round().clamp(0.0, 100.0) as i64);
            let period_months = self
                .planet
                .as_ref()
                .map_or(protocol::BASELINE_MONTHS_PER_YEAR as u32, |p| {
                    p.orbital_period_months
                });
            let life_y = state
                .and_then(|s| s.life_expectancy_months)
                .map_or(0.0, |m| m / f64::from(period_months));
            // Quick-scan war status: a single `· at war ⚔` /
            // `· peace` token rides the cohesion line so the
            // reader can see "is this civ fighting *anyone*" at
            // a glance, even when scrolling past dozens of civ
            // panels. The detail "war: civ X, civ Y" line below
            // names rivals when there are any.
            let in_war = self
                .wars_active
                .iter()
                .any(|(a, b)| *a == *civ_id || *b == *civ_id);
            let war_tag = if in_war { "at war \u{2694}" } else { "peace" };
            if life_y > 0.5 {
                lines.push(format!(
                    "cohesion {cohesion_pct}% · life {life_y:.0}y · {war_tag}"
                ));
            } else {
                lines.push(format!("cohesion {cohesion_pct}% · {war_tag}"));
            }
            // Religion + cosmology dominant-axis line. Names the
            // strongest-magnitude axis on each side with signed
            // magnitude so the reader sees what this civ's belief
            // system *is*, not just abbreviated suffixes. Hidden
            // entirely when both vectors are still neutral
            // (newborn civ, no drift yet).
            let cosmo_axis: Option<(usize, f64)> = state.and_then(|s| s.cosmology).and_then(|c| {
                let mut best = None;
                for (i, v) in c.iter().enumerate() {
                    if v.abs() < 0.20 {
                        continue;
                    }
                    if best.map_or(true, |(_, ba): (usize, f64)| v.abs() > ba.abs()) {
                        best = Some((i, *v));
                    }
                }
                best
            });
            let rel_axis: Option<(usize, f64)> = state.and_then(|s| s.religion).and_then(|r| {
                let mut best = None;
                for (i, v) in r.iter().enumerate() {
                    if v.abs() < 0.20 {
                        continue;
                    }
                    if best.map_or(true, |(_, ba): (usize, f64)| v.abs() > ba.abs()) {
                        best = Some((i, *v));
                    }
                }
                best
            });
            let rel_labels = ["theology", "ritual", "afterlife"];
            let mut belief_parts: Vec<String> = Vec::new();
            if let Some((i, v)) = cosmo_axis {
                let sign = if v >= 0.0 { '+' } else { '-' };
                belief_parts.push(format!(
                    "{}{}{:.1}",
                    Self::COSMOLOGY_AXIS_NAMES[i],
                    sign,
                    v.abs()
                ));
            }
            if let Some((i, v)) = rel_axis {
                let sign = if v >= 0.0 { '+' } else { '-' };
                belief_parts.push(format!("{}{}{:.1}", rel_labels[i], sign, v.abs()));
            }
            if !belief_parts.is_empty() {
                lines.push(belief_parts.join(" · "));
            }
            // List active wars this civ is in (rivals listed by
            // id ascending). The pair set is mirrored from
            // `WarDeclared` / `PeaceConcluded` events.
            let mut rivals: Vec<u32> = self
                .wars_active
                .iter()
                .filter_map(|(a, b)| {
                    if *a == *civ_id {
                        Some(*b)
                    } else if *b == *civ_id {
                        Some(*a)
                    } else {
                        None
                    }
                })
                .collect();
            rivals.sort_unstable();
            rivals.dedup();
            if !rivals.is_empty() {
                let label = rivals
                    .iter()
                    .map(|id| self.civ_label(*id))
                    .collect::<Vec<_>>()
                    .join(", ");
                lines.push(format!("\u{2694} war: {label}"));
            }
        }
        // Replace the previous-render snapshot with the totals
        // we just rendered, so the next frame's trend arrow
        // compares against this frame. Collapsed civs were pruned
        // from `self.civs` on `CivCollapsed`, so the new map
        // already excludes them.
        self.civ_last_emitted_pop_q32 = civ_pop_q32;
        lines
    }

    fn render(&mut self) -> std::io::Result<()> {
        let Some(pm) = &self.planet_map else {
            return Ok(());
        };
        let frame = WorldFrame {
            tick: self.current_tick,
            civs: self.civs.values().cloned().collect(),
            nomad_cells: self.nomad_cells.iter().copied().collect(),
        };
        let active = frame.civs.len();
        // Tick is in months; display year + month-in-year.
        // Compact format so the caption fits in ~30 cols
        // (portrait phone terminals). Year/month derives from
        // the planet's actual orbital period — a 16-month-year
        // planet shows months 0..=15, not wraps at 11 like Earth.
        let period = self
            .planet
            .as_ref()
            .map_or(protocol::BASELINE_MONTHS_PER_YEAR as u32, |p| {
                p.orbital_period_months
            });
        let year = protocol::year_of_tick_for_period(frame.tick, period);
        let month = protocol::month_of_tick_for_period(frame.tick, period);
        // Include total species population (civ pop + nomadic pop)
        // in the caption so the user can see the species' overall
        // mass at a glance. Civ pop sums the per-civ cell
        // populations across `self.civs`; nomadic pop is the
        // running total from the most recent
        // `SpeciesNomadsChanged` snapshot.
        let civ_pop: f64 = self
            .civs
            .values()
            .flat_map(|c| c.cell_populations_q32.values())
            .map(|p| crate::q32::pop_q32_to_f64(*p))
            .sum();
        let total_pop = (civ_pop + self.nomad_total_pop).max(0.0);
        let caption = format!(
            "Y{} M{} · {} civ · {}F/{}C · {}p",
            year,
            month,
            active,
            self.civ_founded_count,
            self.civ_collapsed_count,
            fmt_pop(total_pop),
        );
        // An empty caption is passed in — the caption
        // (`Y{n} M{n} · {civ} civ · {F}F/{C}C`) is rendered
        // separately, either in the planet section's tail or at
        // the top of the map section when the planet card is hidden.
        let body = crate::frame::render_world_frame_styled(
            pm,
            self.planet.as_ref(),
            &frame,
            "",
            crate::frame::FrameStyle {
                use_color: self.cfg.use_color,
                compact: self.cfg.compact,
                density: self.cfg.density_mode,
            },
        );
        // Build the entire frame into an in-memory Vec<u8>, then
        // prepend `\x1b[H\x1b[2J` (home + full-screen clear) and
        // write the whole thing in one pass. The terminal sees
        // clear and content as one chunk and paints both as a
        // single pass — flicker-free and stale-row-free.
        let mut buf: Vec<u8> = Vec::with_capacity(4096);
        // Box-section layout. ANSI dim escape wraps non-map
        // content when `use_color = true` so the map stays full
        // brightness and the surrounding sections recede.
        let (dim_on, dim_off) = if self.cfg.use_color {
            ("\x1b[2m", "\x1b[0m")
        } else {
            ("", "")
        };
        // ===== Top: planet section (left, left-aligned) + log (right) =====
        // When the planet card is shown, the planet card lines + caption
        // render left-aligned in the left zone (`MAP_WIDTH` cols), with
        // the recent-event log riding alongside in the right zone — same
        // `+`-cornered split-divider geometry as the map/key middle row.
        // The bottom log section drops out in that case (the log moves
        // up). Bare-map test configs (`show_planet_card = false`) keep
        // the original full-width bottom log section.
        let planet_section_shown = self.cfg.show_planet_card && self.planet.is_some();
        let log_rides_top = planet_section_shown && self.cfg.log_lines > 0;
        if planet_section_shown {
            if let Some(card) = self.planet_card() {
                let label = self.planet.as_ref().map_or("planet", |p| p.name.as_str());
                buf.write_all(dim_on.as_bytes())?;
                if log_rides_top {
                    buf.write_all(split_divider(label, "log").as_bytes())?;
                } else {
                    buf.write_all(divider(label).as_bytes())?;
                }
                let mut planet_lines: Vec<String> = card.lines().map(str::to_string).collect();
                planet_lines.push(caption.clone());
                // Block-center: every line gets the same left
                // padding computed from the widest line, so the
                // card reads as one centered block where every
                // entry starts at the same column. Independent
                // per-line centering put each line at its own
                // offset, which looked off-center even though each
                // line was technically balanced.
                let widest = planet_lines
                    .iter()
                    .map(|l| visible_width(l))
                    .max()
                    .unwrap_or(0);
                let block_left = MAP_WIDTH.saturating_sub(widest) / 2;
                // Compose each line as `<block_left spaces><line><right pad>`
                // so it pads out to MAP_WIDTH while sharing a single
                // left margin with every other line of the card.
                let render_line = |line: &str| -> String {
                    let mut s = String::with_capacity(MAP_WIDTH + 8);
                    for _ in 0..block_left {
                        s.push(' ');
                    }
                    s.push_str(line);
                    let used = block_left + visible_width(line);
                    for _ in used..MAP_WIDTH {
                        s.push(' ');
                    }
                    s
                };
                if log_rides_top {
                    let log_lines: Vec<&str> =
                        self.recent_events.iter().map(String::as_str).collect();
                    let row_count = planet_lines.len().max(log_lines.len());
                    for i in 0..row_count {
                        let p_row = planet_lines.get(i).map_or("", String::as_str);
                        let p_padded = render_line(p_row);
                        let l_row = log_lines.get(i).copied().unwrap_or("");
                        buf.write_all(p_padded.as_bytes())?;
                        buf.write_all(b" |")?;
                        if !l_row.is_empty() {
                            buf.write_all(b" ")?;
                            buf.write_all(l_row.as_bytes())?;
                        }
                        buf.write_all(ANSI_ERASE_LINE.as_bytes())?;
                        buf.write_all(b"\n")?;
                    }
                } else {
                    for line in &planet_lines {
                        buf.write_all(render_line(line).as_bytes())?;
                        buf.write_all(ANSI_ERASE_LINE.as_bytes())?;
                        buf.write_all(b"\n")?;
                    }
                }
                buf.write_all(dim_off.as_bytes())?;
            }
        }
        // Strip the markdown ```text...``` fences from the
        // body — post-run-report ornament that reads as ASCII
        // noise in the live viewport.
        let stripped = body.strip_prefix("```text\n").unwrap_or(&body);
        let stripped = stripped.strip_suffix("```\n").unwrap_or(stripped);
        // ===== Middle: side-by-side map + sidebar =====
        // Split divider — `--- map ---+--- key ---` — with
        // the `+` corner at `RULE_COL` so the vertical `|` rule
        // connects cleanly through the dashes.
        let middle_divider_shown = self.cfg.show_planet_card;
        if middle_divider_shown {
            buf.write_all(dim_on.as_bytes())?;
            buf.write_all(split_divider("map", "key").as_bytes())?;
            buf.write_all(dim_off.as_bytes())?;
        } else {
            buf.write_all(divider("map").as_bytes())?;
        }
        if !planet_section_shown {
            write_centered_line(&mut buf, &caption)?;
        }
        // Collect the map rows so we can pair them with sidebar
        // rows. Trim trailing blank lines that
        // `render_world_frame_inner` leaves after the bottom
        // `+----+` border.
        let map_lines: Vec<String> = stripped.lines().map(str::to_string).collect();
        // build sidebar lines (legend + species + per-civ).
        // Only rendered when the planet card is shown — the test
        // configs that disable it expect bare-map output.
        let sidebar_lines: Vec<String> = if self.cfg.show_planet_card {
            self.build_sidebar_lines()
        } else {
            Vec::new()
        };
        let row_count = map_lines.len().max(sidebar_lines.len());
        for i in 0..row_count {
            let map_row = map_lines.get(i).map_or("", String::as_str);
            let map_padded = pad_to(map_row, MAP_WIDTH);
            if self.cfg.show_planet_card {
                let side_row = sidebar_lines.get(i).map_or("", String::as_str);
                // each middle row reads as
                // `{map_padded} | {sidebar_padded}` — the `|` rule
                // sits at `RULE_COL` (== MAP_WIDTH + 1).
                buf.write_all(map_padded.as_bytes())?;
                buf.write_all(b" ")?;
                buf.write_all(dim_on.as_bytes())?;
                buf.write_all(b"|")?;
                buf.write_all(dim_off.as_bytes())?;
                buf.write_all(b" ")?;
                buf.write_all(dim_on.as_bytes())?;
                buf.write_all(side_row.as_bytes())?;
                buf.write_all(dim_off.as_bytes())?;
                buf.write_all(ANSI_ERASE_LINE.as_bytes())?;
                buf.write_all(b"\n")?;
            } else {
                // No sidebar — centre the map row within
                // `VIEWPORT_WIDTH` so the bare-map test path looks
                // sensible.
                write_centered_line(&mut buf, map_row)?;
            }
        }
        // ===== Bottom: log section (full-width, bare-map only) =====
        // The log moves up next to the planet card when the card is
        // shown (see the top section), so this bottom block only fires
        // for the bare-map path (`show_planet_card = false`) — the test
        // configs that exercise the log assertions.
        if self.cfg.log_lines > 0 && !log_rides_top {
            buf.write_all(dim_on.as_bytes())?;
            buf.write_all(divider("log").as_bytes())?;
            for line in &self.recent_events {
                write_centered_line(&mut buf, line)?;
            }
            for _ in self.recent_events.len()..self.cfg.log_lines {
                buf.write_all(b"\n")?;
            }
            buf.write_all(dim_off.as_bytes())?;
        }
        // Per-row absolute cursor positioning. The previous
        // variants either prepended `\x1b[H\x1b[2J` (visible
        // erase-then-paint flicker) or `\x1b[H` + body + `\x1b[J`
        // (still flickered on iOS Termius — content past the
        // terminal viewport's last row scrolls the alt-screen,
        // and the next frame's home + paint reads as flashing).
        // Both relied on `\n` line endings, which advance the
        // cursor and scroll once we hit the bottom row.
        //
        // Instead, position the cursor absolutely at row 1 col 1
        // before each line and append `\x1b[K` (erase to EOL).
        // The terminal never scrolls because we never use `\n`;
        // each frame paints in-place at fixed coordinates. Final
        // `\x1b[J` (erase to end of screen) cleans up rows below
        // the new frame's bottom.
        //
        // Per-row write + flush rather than one big buffered
        // write: a coloured frame is ~25 KiB, well past the
        // kernel's PTY chunk size (~4 KiB) and SSH/Termius
        // packetisation. When a multi-byte glyph (`▲` = 3 bytes,
        // `⚔` = 3 bytes, …) straddles a chunk boundary, the
        // terminal's UTF-8 parser sees a lone lead/continuation
        // byte and paints `?` until the next full frame redraws.
        // Per-row writes keep each row ≤ ~300 bytes (atomic on
        // any sane PTY and well within a single SSH segment), so
        // a multi-byte glyph is always delivered intact alongside
        // the ASCII that surrounds it.
        if self.cfg.use_alt_screen {
            // Convert the `\n`-separated body into per-row
            // absolute-positioning chunks. `\x1b[<row>;1H` sets
            // cursor to (row, col 1); rows are 1-indexed in ANSI.
            let body = std::str::from_utf8(&buf).unwrap_or("");
            let mut row_buf: Vec<u8> = Vec::with_capacity(512);
            for (row_idx, line) in body.split('\n').enumerate() {
                // Skip the synthetic empty trailing line that
                // `split('\n')` produces when the buffer ends in
                // `\n` — otherwise we'd emit a stray cursor-move
                // past the last real row.
                if row_idx > 0 && line.is_empty() && body.ends_with('\n') {
                    let next_split = body.split('\n').count() - 1;
                    if row_idx == next_split {
                        break;
                    }
                }
                let row_num = row_idx + 1; // ANSI rows are 1-indexed
                row_buf.clear();
                row_buf.extend_from_slice(format!("\x1b[{row_num};1H").as_bytes());
                row_buf.extend_from_slice(line.as_bytes());
                row_buf.extend_from_slice(ANSI_ERASE_LINE.as_bytes());
                self.out.write_all(&row_buf)?;
                self.out.flush()?;
                // Brief pause between row flushes. Some mobile
                // terminals (Termius / iOS over SSH is the
                // documented case) have a UTF-8 parser that drops
                // continuation bytes when rows arrive back-to-back
                // in tight succession — the symptom is multi-byte
                // glyphs flickering as `?` until the next full
                // frame repaints. A sub-millisecond pause is
                // enough for the parser to drain between rows;
                // ~1.5 ms × ~40 rows = ~60 ms per frame, well
                // under the configured tick cadence.
                std::thread::sleep(Duration::from_micros(1500));
            }
            self.out.write_all(ANSI_ERASE_TO_END.as_bytes())?;
        } else {
            self.out.write_all(&buf)?;
        }
        self.out.flush()?;
        Ok(())
    }
}

impl<W: Write> Emitter for ViewportEmitter<W> {
    type Error = EmitError;

    fn emit(&mut self, event: &Event) -> Result<(), Self::Error> {
        self.apply_state(event);
        if self.should_render(event) {
            self.ensure_initialised()?;
            self.render()?;
        }
        if matches!(event, Event::RunEnd { .. }) {
            self.shutdown()?;
        }
        Ok(())
    }
}
