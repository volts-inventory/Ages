//! Event emission. Sim writes events here; downstream consumers
//! (NDJSON file, live CLI stream, anything later) read them.
//!
//! Multiple emitters can be combined via `TeeEmitter` so a run
//! writes to both an NDJSON file and stdout in the same pass.

use protocol::Event;
use std::io::Write;
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

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
            Event::CellBiomass(_) => "cell_biomass",
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

/// Shared run-pacing control for the interactive TUI. The sim runs
/// on a background thread driving `run()`; the UI thread mutates
/// this handle to pause, single-step, change speed, or request an
/// early stop. The sim-side [`ChannelEmitter`] consults it once per
/// tick (via [`PaceControl::wait_tick`]) and blocks / sleeps
/// accordingly. All methods take `&self` so the handle can be shared
/// behind an [`Arc`] across the two threads.
///
/// Determinism note: pacing reads wall-clock time but never feeds it
/// into sim computation — the canonical NDJSON event log written
/// upstream is bit-for-bit identical regardless of pause / speed.
#[derive(Debug)]
pub struct PaceControl {
    inner: Mutex<PaceState>,
    cv: Condvar,
}

#[derive(Debug)]
struct PaceState {
    paused: bool,
    /// Per-tick delay when running. `Duration::ZERO` = full speed.
    delay: Duration,
    /// Pending single-step ticks to release while paused.
    steps: u64,
    /// The UI asked the sim to stop; `wait_tick` returns `false` so
    /// the emitter can unwind `run()` cleanly.
    quit: bool,
}

impl PaceControl {
    #[must_use]
    pub fn new(delay: Duration, paused: bool) -> Self {
        Self {
            inner: Mutex::new(PaceState {
                paused,
                delay,
                steps: 0,
                quit: false,
            }),
            cv: Condvar::new(),
        }
    }

    /// Flip the paused state and wake the sim thread so it re-checks.
    pub fn toggle_pause(&self) {
        let mut s = self.inner.lock().unwrap();
        s.paused = !s.paused;
        self.cv.notify_all();
    }

    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.inner.lock().unwrap().paused
    }

    /// Set the per-tick delay (running speed) and wake the sim so an
    /// in-flight delay is recomputed against the new value.
    pub fn set_delay(&self, delay: Duration) {
        let mut s = self.inner.lock().unwrap();
        s.delay = delay;
        self.cv.notify_all();
    }

    #[must_use]
    pub fn delay(&self) -> Duration {
        self.inner.lock().unwrap().delay
    }

    /// Release `n` ticks while paused, then re-pause. Pressing step
    /// implies "pause and advance", so this also sets `paused`.
    pub fn step(&self, n: u64) {
        let mut s = self.inner.lock().unwrap();
        s.paused = true;
        s.steps = s.steps.saturating_add(n);
        self.cv.notify_all();
    }

    /// Ask the sim to stop at the next tick boundary. Idempotent.
    /// Doesn't tear the sim down — `wait_tick` returns promptly and the
    /// caller's tick loop breaks cleanly so a final `RunEnd` is still
    /// emitted (see `sim-core`'s `run_interruptible`).
    pub fn quit(&self) {
        let mut s = self.inner.lock().unwrap();
        s.quit = true;
        self.cv.notify_all();
    }

    /// Whether the UI has requested the sim stop. The sim's tick loop
    /// polls this to break gracefully.
    #[must_use]
    pub fn is_quit(&self) -> bool {
        self.inner.lock().unwrap().quit
    }

    /// Called by the sim-side emitter once per tick (on `TickEnd`).
    /// Blocks while paused (until resumed, a step is released, or a
    /// quit is requested); otherwise sleeps the configured delay,
    /// staying responsive to pause / speed changes via the condvar.
    /// Returns promptly when a quit is requested so the current tick
    /// completes and the run loop can break cleanly.
    pub fn wait_tick(&self) {
        let mut s = self.inner.lock().unwrap();
        loop {
            if s.quit {
                return;
            }
            if s.steps > 0 {
                s.steps -= 1;
                return;
            }
            if s.paused {
                s = self.cv.wait(s).unwrap();
                continue;
            }
            let delay = s.delay;
            if delay.is_zero() {
                return;
            }
            let (guard, res) = self.cv.wait_timeout(s, delay).unwrap();
            s = guard;
            if res.timed_out() {
                return;
            }
            // Woken early by a state change (pause / speed / quit) —
            // loop to re-evaluate rather than releasing the tick.
        }
    }
}

/// Forwards every event to a bounded channel (clone per event) and
/// applies [`PaceControl`] pacing on each tick boundary. Used to
/// drive the interactive TUI: the sim runs on a background thread
/// with this as one leg of a [`TeeEmitter`] (the other leg being the
/// canonical NDJSON file), and the UI thread drains the channel to
/// mirror state.
///
/// The bounded channel provides backpressure: if the UI falls behind,
/// `send` blocks and the sim paces itself to the UI's draining rate.
/// If the UI drops the receiver (the user quit), `send` fails and
/// this returns an error so `run()` unwinds the sim cleanly.
pub struct ChannelEmitter {
    tx: SyncSender<Event>,
    pace: Arc<PaceControl>,
}

impl ChannelEmitter {
    #[must_use]
    pub fn new(tx: SyncSender<Event>, pace: Arc<PaceControl>) -> Self {
        Self { tx, pace }
    }
}

impl Emitter for ChannelEmitter {
    type Error = EmitError;

    fn emit(&mut self, event: &Event) -> Result<(), Self::Error> {
        if self.tx.send(event.clone()).is_err() {
            return Err(EmitError::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "tui receiver closed",
            )));
        }
        if matches!(event, Event::Tick(t) if matches!(t.phase, protocol::Phase::TickEnd)) {
            self.pace.wait_tick();
        }
        Ok(())
    }
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
