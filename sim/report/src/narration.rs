//! Real-time narrator. Transforms protocol `Event` variants into
//! single-sentence human-readable lines. Used by two paths:
//!
//!   1. **Streaming**: `NarratingEmitter` wraps an inner `Emitter` and
//!      writes one narration line per event to any `Write` sink
//!      (stdout in the CLI) as the sim runs. Inner emitter still
//!      receives the canonical event so the NDJSON file is intact.
//!
//!   2. **Replay**: `replay_narration` reads a previously-recorded
//!      NDJSON event log and emits narration retroactively. Useful for
//!      post-hoc storytelling without re-running the sim.
//!
//! Narrator state is intentionally small — it tracks names seen for
//! civs / species / figures so later lines like "civ Karnan founded …"
//! can render names instead of bare ids. Year accounting reads
//! `orbital_period_months` off the `Planet` event so tick → year
//! display matches the rest of the report.

use crate::parse::{events_from_reader, ParseError};
use crate::q32::{fmt_pop, pop_q32_to_f64, q32_to_f64};
use protocol::{Event, ExtinctionCause, SpeciationTriggerKind, TraitName};
use sim_events::Emitter;
use std::collections::BTreeMap;
use std::io::{Read, Write};

/// Mutable narrator state. Threads through every `narrate_event`
/// call so per-event lines can resolve civ / figure / species names
/// against earlier events without each callsite carrying its own
/// lookup tables.
#[derive(Debug, Default)]
pub struct NarratorState {
    /// Planet orbital period (months per planet-year). Default
    /// `BASELINE_MONTHS_PER_YEAR` (12) until the `Planet` event lands.
    pub period_months: u32,
    /// Planet display name (e.g. `Vela-c`).
    pub planet_name: Option<String>,
    /// Species display name (e.g. `Kelvars`).
    pub species_name: Option<String>,
    /// `civ_id` → kingdom-style civ name. Populated from
    /// `CivFounded` events as they fire.
    pub civ_names: BTreeMap<u32, String>,
    /// `figure_id` → figure name. Populated from `FigureBorn`.
    pub figure_names: BTreeMap<u32, String>,
    /// `species_id` (dense ecosystem id) → display label. Sprint 2/3
    /// species ids are dense per-planet integers; the persistent
    /// host species lives at id 0 and shares the name in
    /// `species_name`.
    pub species_labels: BTreeMap<u32, String>,
}

impl NarratorState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            period_months: protocol::BASELINE_MONTHS_PER_YEAR as u32,
            ..Self::default()
        }
    }

    /// Planet-relative year of `tick` (rounds down).
    fn year_of(&self, tick: u64) -> u64 {
        protocol::year_of_tick_for_period(tick, self.period_months.max(1))
    }

    /// Civ display label — `"Karnan"` if a name was seen, else
    /// `"civ 3"` as a fallback.
    fn civ_label(&self, civ_id: u32) -> String {
        self.civ_names
            .get(&civ_id)
            .cloned()
            .unwrap_or_else(|| format!("civ {civ_id}"))
    }

    /// Figure display label — `"Solveth"` if seen, else
    /// `"figure 12"`.
    fn figure_label(&self, figure_id: u32) -> String {
        self.figure_names
            .get(&figure_id)
            .cloned()
            .unwrap_or_else(|| format!("figure {figure_id}"))
    }

    /// Species display label — `"Tellarius"` if known, else
    /// the bare id (`"5"`). Callers prefix `"species "` themselves so
    /// known-name lines read naturally (`"species Tellarius extinct"`)
    /// without doubling up (`"species species 5 extinct"`) when the
    /// id is anonymous.
    fn species_label(&self, species_id: u32) -> String {
        self.species_labels
            .get(&species_id)
            .cloned()
            .unwrap_or_else(|| species_id.to_string())
    }

    /// Update state from an event before narrating it. Lets later
    /// lines reference names introduced by earlier events in the same
    /// stream.
    fn ingest(&mut self, event: &Event) {
        match event {
            Event::Planet(p) => {
                self.period_months = p.orbital_period_months;
                self.planet_name = Some(p.name.clone());
            }
            Event::Species(s) => {
                self.species_name = Some(s.name.clone());
                // Sprint 2+ ecosystem species ids are dense per
                // planet; the host species typically holds id 0.
                // Storing under id 0 keeps `species_label(0)` clean
                // for downstream extinction / HGT / speciation lines.
                self.species_labels.insert(0, s.name.clone());
            }
            Event::CivFounded(c) => {
                if !c.name.is_empty() {
                    self.civ_names.insert(c.civ_id, c.name.clone());
                }
            }
            Event::FigureBorn(f) => {
                if !f.name.is_empty() {
                    self.figure_names.insert(f.figure_id, f.name.clone());
                }
            }
            _ => {}
        }
    }
}

/// Produce a one-sentence narration line for `event`, updating
/// `state` as a side effect. Returns `None` for events that have no
/// meaningful prose form (per-tick phase markers, snapshot dumps,
/// per-cell territory updates that are too noisy for the narrator).
///
/// The line is returned without a trailing newline; the emitter /
/// replay path appends one before writing.
pub fn narrate_event(state: &mut NarratorState, event: &Event) -> Option<String> {
    state.ingest(event);
    match event {
        Event::RunStart(r) => Some(format!(
            "Run started — seed {} (schema v{}, ages {}).",
            r.seed, r.schema_version, r.ages_version
        )),
        Event::Planet(p) => {
            let mean_temp_k = q32_to_f64(p.mean_temperature_q32);
            Some(format!(
                "Planet sampled — \"{}\" ({} substrate, {} atmosphere, mean {:.0} K, {}-month year).",
                p.name,
                p.metabolic_substrate,
                p.atmosphere,
                mean_temp_k,
                p.orbital_period_months,
            ))
        }
        Event::Species(s) => Some(format!(
            "Species derived — \"{}\" ({} cognition, {} modalities).",
            s.name,
            cog_tier(q32_to_f64(s.cognition_q32)),
            s.modalities.len(),
        )),
        Event::ArchetypeDerived(a) => Some(format!(
            "Developmental archetype — {} (dominant lever {}, {} cognition).",
            a.label, a.dominant_lever, a.cognition_mode,
        )),
        Event::ArchetypeEndpoint(e) => Some(format!(
            "{} reaches its endpoint — {}",
            if e.civ_name.is_empty() {
                "The civilization"
            } else {
                e.civ_name.as_str()
            },
            e.description,
        )),
        Event::CivFounded(c) => {
            let year = state.year_of(c.tick);
            let label = if c.name.is_empty() {
                format!("civ {}", c.civ_id)
            } else {
                format!("\"{}\"", c.name)
            };
            let pop = fmt_pop(pop_q32_to_f64(c.initial_population_q32));
            let parent = c
                .parent_civ_id
                .map(|id| format!(" (successor to {})", state.civ_label(id)))
                .unwrap_or_default();
            Some(format!(
                "Year {year}: civ {label} founded with {} figures, initial pop {pop}{parent}.",
                c.founding_figure_count
            ))
        }
        Event::CivCollapsed(c) => {
            let year = state.year_of(c.tick);
            let pop = fmt_pop(pop_q32_to_f64(c.final_population_q32));
            Some(format!(
                "Year {year}: civ {} collapsed ({}), final pop {pop}.",
                state.civ_label(c.civ_id),
                c.reason
            ))
        }
        Event::FigureBorn(f) => {
            let year = state.year_of(f.tick);
            let charisma = q32_to_f64(f.charisma_q32);
            let curiosity = q32_to_f64(f.curiosity_q32);
            let traits = describe_figure_traits(charisma, curiosity);
            Some(format!(
                "Year {year}: figure \"{}\" born in {}{}.",
                f.name,
                state.civ_label(f.civ_id),
                traits
            ))
        }
        Event::TechUnlocked(t) => {
            let year = state.year_of(t.tick);
            let seren = if t.serendipitous {
                " (serendipitous)"
            } else {
                ""
            };
            Some(format!(
                "Year {year}: civ {} unlocked tech \"{}\" — tier {}{seren}.",
                state.civ_label(t.civ_id),
                t.tool_name,
                t.tier
            ))
        }
        Event::CatastropheFired(c) => {
            let year = state.year_of(c.tick);
            let pct = q32_to_f64(c.fraction_lost_q32) * 100.0;
            Some(format!(
                "Year {year}: {} catastrophe on civ {} — {:.1}% population loss.",
                c.catastrophe_kind,
                state.civ_label(c.civ_id),
                pct
            ))
        }
        Event::RelationConfirmed(r) => {
            let year = state.year_of(r.tick);
            Some(format!(
                "Year {year}: {} confirmed relation \"{}\" on channel {} ({} form, n={}).",
                state.figure_label(r.figure_id),
                r.template_name,
                r.channel,
                r.form,
                r.n_samples
            ))
        }
        Event::MeasurementConfirmed(m) => {
            let year = state.year_of(m.tick);
            let mode = if m.is_experimental {
                "experimental"
            } else {
                "observational"
            };
            Some(format!(
                "Year {year}: civ {} measured {} vs {} ({} fit, {mode}).",
                state.civ_label(m.civ_id),
                m.y_channel,
                m.x_channel,
                m.form
            ))
        }
        Event::KnowledgeTransmitted(k) => {
            let year = state.year_of(k.tick);
            let comp = q32_to_f64(k.comprehension_q32);
            Some(format!(
                "Year {year}: relation {} transmitted from civ {} to civ {} (comprehension {:.2}).",
                k.relation_id,
                state.civ_label(k.source_civ_id),
                state.civ_label(k.dest_civ_id),
                comp
            ))
        }
        Event::KnowledgeDiffused(k) => {
            let year = state.year_of(k.tick);
            let comp = q32_to_f64(k.comprehension_q32);
            Some(format!(
                "Year {year}: civ {} learned relation {} from civ {} (comprehension {:.2}).",
                state.civ_label(k.dest_civ_id),
                k.relation_id,
                state.civ_label(k.source_civ_id),
                comp
            ))
        }
        Event::CivContact(c) => {
            let year = state.year_of(c.tick);
            Some(format!(
                "Year {year}: civs {} and {} made contact.",
                state.civ_label(c.civ_a),
                state.civ_label(c.civ_b)
            ))
        }
        Event::ConflictResolved(c) => {
            let year = state.year_of(c.tick);
            let outcome = if c.loser_defeated {
                "decisive defeat"
            } else {
                "skirmish"
            };
            Some(format!(
                "Year {year}: {outcome} — {} prevailed over {} across {} cells.",
                state.civ_label(c.winner_civ_id),
                state.civ_label(c.loser_civ_id),
                c.disputed_cell_count
            ))
        }
        Event::WarDeclared(w) => {
            let year = state.year_of(w.tick);
            Some(format!(
                "Year {year}: war declared — {} attacked {}.",
                state.civ_label(w.aggressor_civ_id),
                state.civ_label(w.defender_civ_id)
            ))
        }
        Event::PeaceConcluded(p) => {
            let year = state.year_of(p.tick);
            Some(format!(
                "Year {year}: peace concluded between {} and {} after {} ticks ({:?}).",
                state.civ_label(p.civ_a),
                state.civ_label(p.civ_b),
                p.duration_ticks,
                p.reason
            ))
        }
        Event::AllianceFormed(a) => {
            let year = state.year_of(a.tick);
            Some(format!(
                "Year {year}: alliance formed between {} and {}.",
                state.civ_label(a.civ_a),
                state.civ_label(a.civ_b)
            ))
        }
        Event::AllianceDissolved(a) => {
            let year = state.year_of(a.tick);
            Some(format!(
                "Year {year}: alliance between {} and {} dissolved ({:?}).",
                state.civ_label(a.civ_a),
                state.civ_label(a.civ_b),
                a.reason
            ))
        }
        Event::SpeciesExtinct(s) => {
            let year = state.year_of(s.tick);
            let cause = match s.cause {
                ExtinctionCause::PopulationCollapse => "population collapse",
                ExtinctionCause::KeystoneCascade => "keystone-cascade",
                ExtinctionCause::Catastrophe => "catastrophe",
            };
            Some(format!(
                "Year {year}: species {} extinct via {cause}.",
                state.species_label(s.species_id)
            ))
        }
        Event::SpeciationOccurred(s) => {
            let year = state.year_of(s.tick);
            let trigger = match s.trigger {
                SpeciationTriggerKind::Allopatric { isolation_ticks } => {
                    format!("allopatric ({isolation_ticks} ticks isolated)")
                }
                SpeciationTriggerKind::Sympatric => "sympatric".to_string(),
                SpeciationTriggerKind::Polyploid => "polyploid".to_string(),
                SpeciationTriggerKind::FounderEffect => "founder effect".to_string(),
                SpeciationTriggerKind::PostExtinctionRadiation { generation } => {
                    format!("post-extinction radiation (gen {generation})")
                }
            };
            Some(format!(
                "Year {year}: speciation — daughter species {} split from parent {} via {trigger}.",
                state.species_label(s.daughter_id),
                state.species_label(s.parent_id),
            ))
        }
        Event::HorizontalGeneTransfer(h) => {
            let year = state.year_of(h.tick);
            let trait_name = match h.trait_swapped {
                TraitName::DormancyCapability => "dormancy capability",
                TraitName::TemperatureToleranceLow => "temperature tolerance (low)",
                TraitName::TemperatureToleranceHigh => "temperature tolerance (high)",
                TraitName::RadiationMax => "radiation tolerance",
            };
            Some(format!(
                "Year {year}: HGT — donor {} → recipient {}, trait {trait_name}.",
                state.species_label(h.donor_id),
                state.species_label(h.recipient_id),
            ))
        }
        Event::CivResilienceTick(c) => {
            let year = state.year_of(c.tick);
            let resilience = q32_to_f64(c.resilience_q32);
            let previous = q32_to_f64(c.previous_q32);
            let dir = if resilience > previous {
                "rose"
            } else {
                "fell"
            };
            Some(format!(
                "Year {year}: civ {} resilience {dir} to {resilience:.2} (from {previous:.2}).",
                state.civ_label(c.civ_id)
            ))
        }
        Event::CohesionShifted(c) => {
            let year = state.year_of(c.tick);
            let cohesion = q32_to_f64(c.cohesion_q32);
            let previous = q32_to_f64(c.previous_q32);
            let dir = if cohesion > previous { "rose" } else { "fell" };
            Some(format!(
                "Year {year}: civ {} cohesion {dir} to {cohesion:.2}.",
                state.civ_label(c.civ_id)
            ))
        }
        Event::CivLifeExpectancyChanged(e) => {
            let year = state.year_of(e.tick);
            let months = q32_to_f64(e.life_expectancy_months_q32);
            let years = months / 12.0;
            Some(format!(
                "Year {year}: civ {} life expectancy shifted to {years:.1}y.",
                state.civ_label(e.civ_id)
            ))
        }
        Event::TemplateDiscovered(t) => {
            let year = state.year_of(t.tick);
            Some(format!(
                "Year {year}: civ {} discovered new template \"{}\" (threshold {:.3}).",
                state.civ_label(t.civ_id),
                t.template_name,
                t.threshold_si
            ))
        }
        Event::ToolDiscovered(t) => {
            let year = state.year_of(t.tick);
            Some(format!(
                "Year {year}: civ {} invented dynamic tool \"{}\" — focus {}, tier {}.",
                state.civ_label(t.civ_id),
                t.tool_name,
                t.channel_focus,
                t.tier
            ))
        }
        Event::CosmologyShifted(c) => {
            let year = state.year_of(c.tick);
            Some(format!(
                "Year {year}: civ {} cosmology shifted.",
                state.civ_label(c.civ_id)
            ))
        }
        Event::ReligionShifted(r) => {
            let year = state.year_of(r.tick);
            Some(format!(
                "Year {year}: civ {} religion drifted.",
                state.civ_label(r.civ_id)
            ))
        }
        Event::RelationFalsified(r) => {
            let year = state.year_of(r.tick);
            Some(format!(
                "Year {year}: civ {} found relation {} falsified after {} ticks of misprediction.",
                state.civ_label(r.civ_id),
                r.relation_id,
                r.streak_ticks
            ))
        }
        Event::RelationRevalidated(r) => {
            let year = state.year_of(r.tick);
            Some(format!(
                "Year {year}: civ {} revalidated inherited relation {} from civ {}.",
                state.civ_label(r.civ_id),
                r.relation_id,
                state.civ_label(r.from_civ_id)
            ))
        }
        Event::RelationLapsed(r) => {
            let year = state.year_of(r.tick);
            Some(format!(
                "Year {year}: civ {} dropped inherited relation {} (failed to re-fit).",
                state.civ_label(r.civ_id),
                r.relation_id
            ))
        }
        Event::RefinementProposed(r) => {
            let year = state.year_of(r.tick);
            Some(format!(
                "Year {year}: {} proposed refinement of relation {} ({} → {}).",
                state.figure_label(r.figure_id),
                r.relation_id,
                r.old_form,
                r.new_form
            ))
        }
        Event::RefinementConfirmed(r) => {
            let year = state.year_of(r.tick);
            Some(format!(
                "Year {year}: refinement confirmed — relation {} now {} (was {}).",
                r.relation_id, r.new_form, r.old_form
            ))
        }
        Event::RefinementRejected(r) => {
            let year = state.year_of(r.tick);
            Some(format!(
                "Year {year}: refinement of relation {} rejected ({}); stayed on {}.",
                r.relation_id, r.reason, r.old_form
            ))
        }
        Event::TradeRouteEstablished(t) => {
            let year = state.year_of(t.tick);
            Some(format!(
                "Year {year}: trade route opened between {} and {}.",
                state.civ_label(t.civ_a),
                state.civ_label(t.civ_b)
            ))
        }
        Event::TradeRouteClosed(t) => {
            let year = state.year_of(t.tick);
            Some(format!(
                "Year {year}: trade route closed between {} and {} ({}).",
                state.civ_label(t.civ_a),
                state.civ_label(t.civ_b),
                t.reason
            ))
        }
        Event::RelationMythologized(r) => {
            let year = state.year_of(r.tick);
            Some(format!(
                "Year {year}: civ {} mythologized relation {} from civ {} (axis {}).",
                state.civ_label(r.dest_civ_id),
                r.relation_id,
                state.civ_label(r.source_civ_id),
                r.axis
            ))
        }
        Event::RivalHypothesisProposed(r) => {
            let year = state.year_of(r.tick);
            Some(format!(
                "Year {year}: civ {} proposed rival hypothesis for relation {} ({} vs primary {}).",
                state.civ_label(r.civ_id),
                r.relation_id,
                r.rival_form,
                r.primary_form
            ))
        }
        Event::PrimaryHypothesisDisplaced(p) => {
            let year = state.year_of(p.tick);
            Some(format!(
                "Year {year}: civ {} primary hypothesis displaced for relation {} ({} → {}).",
                state.civ_label(p.civ_id),
                p.relation_id,
                p.old_form,
                p.new_form
            ))
        }
        Event::CivSurplusChanged(c) => {
            let year = state.year_of(c.tick);
            let surplus = q32_to_f64(c.surplus_q32);
            Some(format!(
                "Year {year}: civ {} surplus shifted to {surplus:.0} pop-equivalents.",
                state.civ_label(c.civ_id)
            ))
        }
        Event::SpeciesCosmologyBias(_) => Some(
            "Species cosmology bias declared — initial cultural-axis position recorded."
                .to_string(),
        ),
        Event::SpeciesDrift(d) => {
            let year = state.year_of(d.tick);
            Some(format!(
                "Year {year}: civ {} trait drift recorded at founding.",
                state.civ_label(d.civ_id)
            ))
        }
        Event::RunEnd { tick, reason } => {
            let year = state.year_of(*tick);
            Some(format!("Year {year}: run ended — {reason}."))
        }
        // Quiet variants: per-tick phase markers and bulk snapshot
        // events that would drown the narration in noise. The post-run
        // report covers these in aggregate.
        Event::Tick(_)
        | Event::Snapshot(_)
        | Event::PlanetMap(_)
        | Event::CellBiomass(_)
        | Event::ClimateSample(_)
        | Event::SpeciesNomadsChanged(_)
        | Event::CivTerritoryChanged(_)
        | Event::Recognition(_)
        | Event::RunMetadata(_) => None,
    }
}

/// Coarse 3-bin cognition tier label so the species-derived line
/// reads more naturally ("low" vs "0.34"). Uses the same bins the
/// `sim_report::labels::cog_tier` helper uses; reimplemented here so
/// `narration` stays a single-file module without a label-table
/// dependency.
fn cog_tier(c: f64) -> &'static str {
    if c < 0.34 {
        "low"
    } else if c < 0.67 {
        "medium"
    } else {
        "high"
    }
}

/// Flavour suffix for `FigureBorn`. Highlights the two most
/// commonly narrative-relevant scalars (charisma, curiosity); the
/// other figure traits stay in the structured event for the report.
fn describe_figure_traits(charisma: f64, curiosity: f64) -> String {
    let mut traits: Vec<&'static str> = Vec::new();
    if charisma > 0.66 {
        traits.push("high charisma");
    }
    if curiosity > 0.66 {
        traits.push("high curiosity");
    }
    if traits.is_empty() {
        String::new()
    } else {
        format!(" ({})", traits.join(" + "))
    }
}

/// Emitter wrapper that narrates each event to a `Write` sink and
/// forwards the canonical event to an inner emitter. Used for the
/// `--narration` CLI flag: pair with `JsonLinesEmitter<File>` so the
/// NDJSON file still receives the full event stream while the
/// terminal sees real-time prose.
///
/// `W` is generic over the sink so tests can capture lines into a
/// `Vec<u8>` and the binary can use `std::io::Stdout`.
pub struct NarratingEmitter<E, W> {
    pub inner: E,
    pub sink: W,
    pub state: NarratorState,
}

impl<E, W> NarratingEmitter<E, W> {
    pub fn new(inner: E, sink: W) -> Self {
        Self {
            inner,
            sink,
            state: NarratorState::new(),
        }
    }
}

/// Errors from `NarratingEmitter`. The inner emitter's error and
/// IO writes to the narration sink share this enum so the parent
/// `run()` signature stays a single `?` chain. The `Inner` variant
/// boxes the upstream error to keep the type generic-parameter-free
/// at the binary boundary (anyhow upcasts cleanly).
#[derive(Debug, thiserror::Error)]
pub enum NarrateError<E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    #[error("inner emitter: {0}")]
    Inner(#[source] E),
    #[error("narration sink io: {0}")]
    Sink(#[from] std::io::Error),
}

impl<E, W> Emitter for NarratingEmitter<E, W>
where
    E: Emitter,
    E::Error: std::error::Error + Send + Sync + 'static,
    W: Write,
{
    type Error = NarrateError<E::Error>;

    fn emit(&mut self, event: &Event) -> Result<(), Self::Error> {
        // Inner first so the canonical NDJSON write happens
        // regardless of narration sink state.
        self.inner.emit(event).map_err(NarrateError::Inner)?;
        if let Some(line) = narrate_event(&mut self.state, event) {
            self.sink.write_all(line.as_bytes())?;
            self.sink.write_all(b"\n")?;
            // Flush per line so the terminal shows narration live
            // rather than buffering it for the end of the run.
            self.sink.flush()?;
        }
        Ok(())
    }
}

/// Replay narration from a previously-recorded NDJSON event log.
/// Reads `reader` line by line, parses each line into an `Event`, and
/// writes one narration line per non-quiet event to `sink`. Returns
/// the number of narrated lines written (events without a prose form
/// are silently skipped).
pub fn replay_narration_from_reader<R: Read, W: Write>(
    reader: R,
    mut sink: W,
) -> Result<usize, ParseError> {
    let events = events_from_reader(reader)?;
    let mut state = NarratorState::new();
    let mut written: usize = 0;
    for ev in &events {
        if let Some(line) = narrate_event(&mut state, ev) {
            sink.write_all(line.as_bytes()).map_err(ParseError::Io)?;
            sink.write_all(b"\n").map_err(ParseError::Io)?;
            written += 1;
        }
    }
    Ok(written)
}

/// Replay narration from a file on disk. Convenience wrapper for the
/// `--replay-narration` CLI flag. Opens `path` (buffered), narrates
/// every event in the log to `sink`, and returns the line count.
pub fn replay_narration<W: Write>(path: &std::path::Path, sink: W) -> Result<usize, ParseError> {
    let file = std::fs::File::open(path).map_err(ParseError::Io)?;
    let reader = std::io::BufReader::new(file);
    replay_narration_from_reader(reader, sink)
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{
        CatastropheFired, CivCollapsed, CivFounded, ExtinctionCause, FigureBorn, HgtEvent,
        PlanetDerived, RelationConfirmed, RunHeader, SpeciationEvent, SpeciationTriggerKind,
        SpeciesDerived, SpeciesExtinct, TechUnlocked, TraitName, SCHEMA_VERSION,
    };
    use sim_events::JsonLinesEmitter;

    fn planet_event() -> Event {
        Event::Planet(PlanetDerived {
            seed: 1,
            name: "Vela-c".into(),
            gravity_q32: 0,
            composition: "rocky".into(),
            mean_temperature_q32: 290 * (1_i64 << 32),
            temperature_gradient_q32: 0,
            terrain_peak_q32: 0,
            sea_level_q32: 0,
            atmosphere: "oxidising".into(),
            surface_pressure_q32: 0,
            biosphere: "verdant".into(),
            magnetosphere: "active".into(),
            crust: "basaltic".into(),
            stellar_luminosity_q32: 0,
            moon_count: 1,
            axial_tilt_deg_q32: 0,
            day_length_hours_q32: 0,
            orbital_period_months: 12,
            metabolic_substrate: "aqueous".into(),
            substrate_perturbation_q32: 0,
            effective_boil_k_q32: 0,
            atmospheric_n2_q32: 0,
            atmospheric_o2_q32: 0,
            atmospheric_co2_q32: 0,
            atmospheric_ch4_q32: 0,
            atmospheric_nh3_q32: 0,
            atmospheric_h2o_q32: 0,
            atmospheric_h2_q32: 0,
            atmospheric_ar_q32: 0,
            atmospheric_other_q32: 0,
            biosphere_density_q32: 0,
            crustal_silicate_q32: 0,
            crustal_hydrocarbon_q32: 0,
            crustal_piezoelectric_q32: 0,
            crustal_ferrous_q32: 0,
            crustal_rare_earth_q32: 0,
            crustal_ice_q32: 0,
            crustal_other_q32: 0,
        })
    }

    fn species_event() -> Event {
        Event::Species(SpeciesDerived {
            seed: 1,
            name: "Kelvars".into(),
            cognition_q32: (3_i64 << 32) / 4,
            sociality_q32: 0,
            communication_fidelity_q32: 0,
            lifespan_years_q32: 0,
            t0_loss_q32: 0,
            modalities: vec!["visual".into(), "audio".into()],
            manipulation_modes: vec!["prehensile".into()],
            perceivable_template_ids: vec![],
            cognition_topology: "centralized".into(),
            habitat: "terrestrial".into(),
        })
    }

    fn civ_founded(tick: u64, id: u32, name: &str) -> Event {
        Event::CivFounded(CivFounded {
            tick,
            civ_id: id,
            parent_civ_id: None,
            name: name.into(),
            initial_population_q32: 200_i128 * (1_i128 << 32),
            founding_figure_count: 2,
            claimed_cells: vec![],
            cell_capacities_q32: vec![],
        })
    }

    #[test]
    fn narrates_run_start_with_seed() {
        let mut state = NarratorState::new();
        let ev = Event::RunStart(RunHeader {
            schema_version: SCHEMA_VERSION,
            seed: 42,
            ages_version: "0.0.1".into(),
        });
        let line = narrate_event(&mut state, &ev).unwrap();
        assert!(line.contains("seed 42"), "got {line:?}");
    }

    #[test]
    fn narrates_civ_founded_with_year_and_name() {
        let mut state = NarratorState::new();
        // Plant the planet event first so year_of picks up the 12-
        // month period and reads the tick as year 23.
        narrate_event(&mut state, &planet_event());
        let line = narrate_event(&mut state, &civ_founded(23 * 12, 1, "Karnan")).unwrap();
        assert!(line.contains("Year 23"), "year missing: {line:?}");
        assert!(line.contains("Karnan"), "name missing: {line:?}");
        assert!(line.contains("founded"), "verb missing: {line:?}");
    }

    #[test]
    fn narrates_civ_collapsed_with_civ_name_from_state() {
        let mut state = NarratorState::new();
        narrate_event(&mut state, &planet_event());
        narrate_event(&mut state, &civ_founded(0, 1, "Karnan"));
        let ev = Event::CivCollapsed(CivCollapsed {
            tick: 50 * 12,
            civ_id: 1,
            reason: "food_crisis".into(),
            final_population_q32: 10_i128 * (1_i128 << 32),
            final_figure_count: 0,
        });
        let line = narrate_event(&mut state, &ev).unwrap();
        assert!(line.contains("Year 50"), "{line:?}");
        assert!(line.contains("Karnan"), "{line:?}");
        assert!(line.contains("food_crisis"), "{line:?}");
    }

    #[test]
    fn narrates_figure_with_high_trait_callout() {
        let mut state = NarratorState::new();
        narrate_event(&mut state, &planet_event());
        let one_q32: i64 = 1 << 32;
        let ev = Event::FigureBorn(FigureBorn {
            tick: 156 * 12,
            civ_id: 1,
            figure_id: 7,
            name: "Solveth".into(),
            charisma_q32: (one_q32 * 9) / 10,
            curiosity_q32: (one_q32 * 8) / 10,
            doubt_q32: 0,
            communicativeness_q32: 0,
            cell_assignment: 0,
        });
        let line = narrate_event(&mut state, &ev).unwrap();
        assert!(line.contains("Year 156"), "{line:?}");
        assert!(line.contains("Solveth"), "{line:?}");
        assert!(
            line.contains("high charisma") && line.contains("high curiosity"),
            "expected both trait callouts; got {line:?}"
        );
    }

    #[test]
    fn narrates_tech_unlocked() {
        let mut state = NarratorState::new();
        narrate_event(&mut state, &planet_event());
        let ev = Event::TechUnlocked(TechUnlocked {
            tick: 401 * 12,
            civ_id: 2,
            tool_id: 5,
            tool_name: "Bronze Smithing".into(),
            tier: 2,
            granted_channels: vec![],
            newly_perceivable_template_ids: vec![],
            serendipitous: false,
        });
        let line = narrate_event(&mut state, &ev).unwrap();
        assert!(line.contains("Year 401"), "{line:?}");
        assert!(line.contains("Bronze Smithing"), "{line:?}");
        assert!(line.contains("tier 2"), "{line:?}");
    }

    #[test]
    fn narrates_catastrophe_with_percentage() {
        let mut state = NarratorState::new();
        narrate_event(&mut state, &planet_event());
        narrate_event(&mut state, &civ_founded(0, 1, "Karnan"));
        let ev = Event::CatastropheFired(CatastropheFired {
            tick: 1023 * 12,
            civ_id: 1,
            catastrophe_kind: "volcanic".into(),
            // 5% loss: 0.05 in Q32.32
            fraction_lost_q32: (1_i64 << 32) / 20,
        });
        let line = narrate_event(&mut state, &ev).unwrap();
        assert!(line.contains("Year 1023"), "{line:?}");
        assert!(line.contains("volcanic"), "{line:?}");
        assert!(line.contains("5.0%"), "fraction missing: {line:?}");
    }

    #[test]
    fn narrates_species_extinction() {
        let mut state = NarratorState::new();
        narrate_event(&mut state, &planet_event());
        narrate_event(&mut state, &species_event());
        let ev = Event::SpeciesExtinct(SpeciesExtinct {
            tick: 2100 * 12,
            species_id: 0,
            cause: ExtinctionCause::PopulationCollapse,
        });
        let line = narrate_event(&mut state, &ev).unwrap();
        assert!(line.contains("Year 2100"), "{line:?}");
        assert!(line.contains("Kelvars"), "{line:?}");
        assert!(line.contains("population collapse"), "{line:?}");
    }

    #[test]
    fn narrates_speciation_with_trigger_kind() {
        let mut state = NarratorState::new();
        narrate_event(&mut state, &planet_event());
        narrate_event(&mut state, &species_event());
        let ev = Event::SpeciationOccurred(SpeciationEvent {
            tick: 3000 * 12,
            parent_id: 0,
            daughter_id: 1,
            trigger: SpeciationTriggerKind::Sympatric,
        });
        let line = narrate_event(&mut state, &ev).unwrap();
        assert!(line.contains("Year 3000"), "{line:?}");
        assert!(line.contains("sympatric"), "{line:?}");
    }

    #[test]
    fn narrates_hgt_with_trait_label() {
        let mut state = NarratorState::new();
        narrate_event(&mut state, &planet_event());
        let ev = Event::HorizontalGeneTransfer(HgtEvent {
            tick: 3200 * 12,
            donor_id: 5,
            recipient_id: 12,
            trait_swapped: TraitName::DormancyCapability,
        });
        let line = narrate_event(&mut state, &ev).unwrap();
        assert!(line.contains("Year 3200"), "{line:?}");
        assert!(line.contains("dormancy"), "{line:?}");
    }

    #[test]
    fn narrates_relation_confirmed() {
        let mut state = NarratorState::new();
        narrate_event(&mut state, &planet_event());
        let ev = Event::RelationConfirmed(RelationConfirmed {
            tick: 100 * 12,
            relation_id: 1,
            figure_id: 7,
            template_id: 1,
            template_name: "WaterFreezing".into(),
            channel: "temperature".into(),
            form: "threshold_step".into(),
            params_q32: vec![],
            residual_q32: 0,
            confidence_q32: 0,
            n_samples: 10,
        });
        let line = narrate_event(&mut state, &ev).unwrap();
        assert!(line.contains("Year 100"), "{line:?}");
        assert!(line.contains("WaterFreezing"), "{line:?}");
    }

    #[test]
    fn ignores_per_tick_phase_markers() {
        let mut state = NarratorState::new();
        let ev = Event::Tick(protocol::TickEvent {
            tick: 1,
            phase: protocol::Phase::TickStart,
        });
        assert!(narrate_event(&mut state, &ev).is_none());
    }

    #[test]
    fn narrating_emitter_writes_inner_and_narration() {
        let mut narration_sink: Vec<u8> = Vec::new();
        let mut ndjson: Vec<u8> = Vec::new();
        {
            let inner = JsonLinesEmitter::new(&mut ndjson);
            let mut em = NarratingEmitter::new(inner, &mut narration_sink);
            em.emit(&Event::RunStart(RunHeader {
                schema_version: SCHEMA_VERSION,
                seed: 99,
                ages_version: "t".into(),
            }))
            .unwrap();
            em.emit(&planet_event()).unwrap();
            em.emit(&civ_founded(12, 1, "Karnan")).unwrap();
            // Tick events are quiet → narration sink unchanged but
            // inner emitter still records them.
            em.emit(&Event::Tick(protocol::TickEvent {
                tick: 12,
                phase: protocol::Phase::TickStart,
            }))
            .unwrap();
        }
        let narration = String::from_utf8(narration_sink).unwrap();
        let ndjson_text = String::from_utf8(ndjson).unwrap();
        // Three narration lines (run_start, planet, civ_founded);
        // Tick was filtered out by `narrate_event`.
        assert_eq!(
            narration.lines().count(),
            3,
            "narration:\n{narration}"
        );
        // NDJSON file should carry all four events.
        assert_eq!(
            ndjson_text.lines().count(),
            4,
            "ndjson:\n{ndjson_text}"
        );
        assert!(narration.contains("Karnan"));
        assert!(narration.contains("Vela-c"));
    }

    #[test]
    fn narration_replay_consumes_log_file() {
        // Build a small NDJSON log in memory, then run the replay
        // path against it and assert narration content.
        let mut buf: Vec<u8> = Vec::new();
        {
            let mut em = JsonLinesEmitter::new(&mut buf);
            em.emit(&Event::RunStart(RunHeader {
                schema_version: SCHEMA_VERSION,
                seed: 7,
                ages_version: "t".into(),
            }))
            .unwrap();
            em.emit(&planet_event()).unwrap();
            em.emit(&species_event()).unwrap();
            em.emit(&civ_founded(23 * 12, 1, "Karnan")).unwrap();
            em.emit(&Event::TechUnlocked(TechUnlocked {
                tick: 401 * 12,
                civ_id: 1,
                tool_id: 1,
                tool_name: "Bronze Smithing".into(),
                tier: 2,
                granted_channels: vec![],
                newly_perceivable_template_ids: vec![],
                serendipitous: false,
            }))
            .unwrap();
            em.emit(&Event::RunEnd {
                tick: 500 * 12,
                reason: "civ_collapsed".into(),
            })
            .unwrap();
        }

        let mut out: Vec<u8> = Vec::new();
        let written = replay_narration_from_reader(buf.as_slice(), &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(written >= 5, "expected ≥5 narration lines, got {written}");
        assert!(text.contains("seed 7"), "{text}");
        assert!(text.contains("Vela-c"), "{text}");
        assert!(text.contains("Kelvars"), "{text}");
        assert!(text.contains("Year 23"), "{text}");
        assert!(text.contains("Karnan"), "{text}");
        assert!(text.contains("Bronze Smithing"), "{text}");
        assert!(text.contains("run ended"), "{text}");
    }

    #[test]
    fn replay_from_path_round_trips_through_disk() {
        use std::io::Write as _;
        let tmp = std::env::temp_dir().join(format!(
            "ages-narration-replay-{}-{}.ndjson",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        {
            let mut file = std::fs::File::create(&tmp).unwrap();
            let evs: Vec<Event> = vec![
                Event::RunStart(RunHeader {
                    schema_version: SCHEMA_VERSION,
                    seed: 11,
                    ages_version: "t".into(),
                }),
                planet_event(),
                civ_founded(12, 1, "Veridia"),
            ];
            for ev in &evs {
                let line = serde_json::to_string(ev).unwrap();
                writeln!(file, "{line}").unwrap();
            }
        }

        let mut out: Vec<u8> = Vec::new();
        let written = replay_narration(&tmp, &mut out).unwrap();
        let _ = std::fs::remove_file(&tmp);

        let text = String::from_utf8(out).unwrap();
        assert_eq!(written, 3, "{text}");
        assert!(text.contains("Veridia"), "{text}");
    }
}
