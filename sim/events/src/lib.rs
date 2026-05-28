//! Event emission. Sim writes events here; downstream consumers
//! (NDJSON file, live CLI stream, anything later) read them.
//!
//! Multiple emitters can be combined via `TeeEmitter` so a run
//! writes to both an NDJSON file and stdout in the same pass.

use protocol::Event;
use std::io::Write;

#[derive(Debug, thiserror::Error)]
pub enum EmitError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub trait Emitter {
    type Error;
    fn emit(&mut self, event: &Event) -> Result<(), Self::Error>;
}

/// Writes events as newline-delimited JSON to any `Write`. The
/// canonical NDJSON event log uses this against a `File`; tests
/// use it against a `Vec<u8>` buffer.
pub struct JsonLinesEmitter<W: Write> {
    writer: W,
}

impl<W: Write> JsonLinesEmitter<W> {
    pub fn new(writer: W) -> Self {
        Self { writer }
    }

    pub fn into_inner(self) -> W {
        self.writer
    }
}

impl<W: Write> Emitter for JsonLinesEmitter<W> {
    type Error = EmitError;

    fn emit(&mut self, event: &Event) -> Result<(), Self::Error> {
        serde_json::to_writer(&mut self.writer, event)?;
        self.writer.write_all(b"\n")?;
        Ok(())
    }
}

/// Splits emission to two emitters. Used to tee the canonical NDJSON
/// log to stdout for the live CLI stream (replacement for
/// LLM narration).
pub struct TeeEmitter<A, B> {
    pub a: A,
    pub b: B,
}

impl<A: Emitter, B: Emitter<Error = A::Error>> Emitter for TeeEmitter<A, B> {
    type Error = A::Error;

    fn emit(&mut self, event: &Event) -> Result<(), Self::Error> {
        self.a.emit(event)?;
        self.b.emit(event)?;
        Ok(())
    }
}

/// Forwards events that match `predicate` to the inner emitter; drops
/// the rest. Used to wire `--cli=highlights` so the live stream only
/// carries structural-pin events instead of every per-tick phase
/// marker. The canonical NDJSON file emitter sits outside this so
/// the file always carries the full stream.
pub struct FilterEmitter<E, F> {
    pub inner: E,
    pub predicate: F,
}

impl<E: Emitter, F: FnMut(&Event) -> bool> Emitter for FilterEmitter<E, F> {
    type Error = E::Error;

    fn emit(&mut self, event: &Event) -> Result<(), Self::Error> {
        if (self.predicate)(event) {
            self.inner.emit(event)?;
        }
        Ok(())
    }
}

/// Wraps an emitter and sleeps `tick_rate` between ticks. Only
/// fires on the `Tick { phase: TickEnd }` event so the sleep
/// happens once per tick at the canonical tick boundary, not per-
/// emitted-event. Used for the live CLI stream when
/// `--tick-rate-ms > 0` so the operator (or a future UI consumer)
/// can pace the visible event flow without slowing the canonical
/// NDJSON file write.
///
/// Determinism note: `thread::sleep` reads wall-clock time but
/// doesn't feed it into any sim computation. The event log written
/// upstream of this wrapper is bit-for-bit identical regardless of
/// throttle setting; throttling only affects when downstream
/// readers see the events.
pub struct ThrottledEmitter<E> {
    pub inner: E,
    pub tick_rate: std::time::Duration,
}

impl<E: Emitter> Emitter for ThrottledEmitter<E> {
    type Error = E::Error;

    fn emit(&mut self, event: &Event) -> Result<(), Self::Error> {
        self.inner.emit(event)?;
        if matches!(event, Event::Tick(t) if matches!(t.phase, protocol::Phase::TickEnd)) {
            std::thread::sleep(self.tick_rate);
        }
        Ok(())
    }
}

/// Test helper: an Emitter that counts events by kind tag without
/// storing the full log. Lets long-run integration tests assert
/// "saw at least N `civ_founded` events" without buffering the
/// entire NDJSON stream (which can hit several GB at month-grained
/// ticks with rich civ chains).
#[derive(Debug, Default)]
pub struct CountingEmitter {
    counts: std::collections::BTreeMap<&'static str, u64>,
}

impl CountingEmitter {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn count(&self, kind: &str) -> u64 {
        self.counts.get(kind).copied().unwrap_or(0)
    }

    /// Total events seen, summed across kinds. Useful as a coarse
    /// liveness check.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.counts.values().sum()
    }
}

impl Emitter for CountingEmitter {
    type Error = EmitError;

    fn emit(&mut self, event: &Event) -> Result<(), Self::Error> {
        let key: &'static str = match event {
            Event::RunStart(_) => "run_start",
            Event::Tick(_) => "tick",
            Event::Recognition(_) => "recognition",
            Event::Planet(_) => "planet",
            Event::PlanetMap(_) => "planet_map",
            Event::Species(_) => "species",
            Event::FigureBorn(_) => "figure_born",
            Event::TechUnlocked(_) => "tech_unlocked",
            Event::CivFounded(_) => "civ_founded",
            Event::CivTerritoryChanged(_) => "civ_territory_changed",
            Event::CivCollapsed(_) => "civ_collapsed",
            Event::KnowledgeTransmitted(_) => "knowledge_transmitted",
            Event::CosmologyShifted(_) => "cosmology_shifted",
            Event::ReligionShifted(_) => "religion_shifted",
            Event::CatastropheFired(_) => "catastrophe_fired",
            Event::CivContact(_) => "civ_contact",
            Event::ConflictResolved(_) => "conflict_resolved",
            Event::WarDeclared(_) => "war_declared",
            Event::PeaceConcluded(_) => "peace_concluded",
            Event::KnowledgeDiffused(_) => "knowledge_diffused",
            Event::RelationConfirmed(_) => "relation_confirmed",
            Event::MeasurementConfirmed(_) => "measurement_confirmed",
            Event::RefinementProposed(_) => "refinement_proposed",
            Event::RefinementConfirmed(_) => "refinement_confirmed",
            Event::RefinementRejected(_) => "refinement_rejected",
            Event::RelationFalsified(_) => "relation_falsified",
            Event::RelationRevalidated(_) => "relation_revalidated",
            Event::RelationLapsed(_) => "relation_lapsed",
            Event::Snapshot(_) => "snapshot",
            Event::RunEnd { .. } => "run_end",
            Event::RunMetadata(_) => "run_metadata",
            Event::SpeciesNomadsChanged(_) => "species_nomads_changed",
            Event::TemplateDiscovered(_) => "template_discovered",
            Event::ToolDiscovered(_) => "tool_discovered",
            Event::SpeciesCosmologyBias(_) => "species_cosmology_bias",
            Event::ArchetypeDerived(_) => "archetype_derived",
            Event::ArchetypeEndpoint(_) => "archetype_endpoint",
            Event::SpeciesDrift(_) => "species_drift",
            Event::CohesionShifted(_) => "cohesion_shifted",
            Event::RelationMythologized(_) => "relation_mythologized",
            Event::RivalHypothesisProposed(_) => "rival_hypothesis_proposed",
            Event::PrimaryHypothesisDisplaced(_) => "primary_hypothesis_displaced",
            Event::CivLifeExpectancyChanged(_) => "civ_life_expectancy_changed",
            Event::CivSurplusChanged(_) => "civ_surplus_changed",
            Event::CivResilienceTick(_) => "civ_resilience_tick",
            Event::TradeRouteEstablished(_) => "trade_route_established",
            Event::TradeRouteClosed(_) => "trade_route_closed",
            Event::AllianceFormed(_) => "alliance_formed",
            Event::AllianceDissolved(_) => "alliance_dissolved",
            Event::SpeciesExtinct(_) => "species_extinct",
            Event::HorizontalGeneTransfer(_) => "horizontal_gene_transfer",
            Event::SpeciationOccurred(_) => "speciation_occurred",
        };
        *self.counts.entry(key).or_insert(0) += 1;
        Ok(())
    }
}

/// Structural-pin predicate for the live `--cli=highlights` mode.
/// Mirrors the post-run report's `highlights::highlights` pin set:
/// civ founding/collapse, catastrophe, tech unlocks, civ contact,
/// inter-civ knowledge transmission, run-end. Recognition firings,
/// per-tick phase markers, and per-relation discovery events are
/// suppressed; the post-run report renders those.
#[must_use]
pub fn is_highlight_event(event: &Event) -> bool {
    matches!(
        event,
        Event::RunStart(_)
            | Event::Planet(_)
            | Event::Species(_)
            | Event::CivFounded(_)
            | Event::CivCollapsed(_)
            | Event::CatastropheFired(_)
            | Event::TechUnlocked(_)
            | Event::CivContact(_)
            | Event::KnowledgeTransmitted(_)
            | Event::ConflictResolved(_)
            // Emergent discoveries are headline-
            // worthy — a species inventing a new recognition
            // template or a civ inventing a new dynamic tool is
            // the kind of structural-pin event the live highlights
            // stream exists to surface.
            | Event::TemplateDiscovered(_)
            | Event::ToolDiscovered(_)
            // Per-seed cosmology bias one-shot. Same
            // narrative weight as Species — it's the species-
            // level cultural-substrate declaration.
            | Event::SpeciesCosmologyBias(_)
            // The run-start archetype declaration — which of the
            // foundational levers this world+species develops along —
            // is a top-level structural pin.
            | Event::ArchetypeDerived(_)
            // The archetype endpoint is the run's climactic divergent
            // fate — always a headline pin.
            | Event::ArchetypeEndpoint(_)
            | Event::RunEnd { .. }
    )
    // PlanetMap is intentionally excluded — it's a one-shot setup
    // event with per-cell payload that bloats the live stream;
    // the post-run report renders it from the file.
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{Phase, RunHeader, TickEvent, SCHEMA_VERSION};

    #[test]
    fn json_lines_writes_one_line_per_event() {
        let mut buf = Vec::new();
        {
            let mut em = JsonLinesEmitter::new(&mut buf);
            em.emit(&Event::RunStart(RunHeader {
                schema_version: SCHEMA_VERSION,
                seed: 1,
                ages_version: "test".to_string(),
            }))
            .unwrap();
            em.emit(&Event::Tick(TickEvent {
                tick: 0,
                phase: Phase::TickStart,
            }))
            .unwrap();
        }
        let text = String::from_utf8(buf).unwrap();
        assert_eq!(text.lines().count(), 2);
    }
}
