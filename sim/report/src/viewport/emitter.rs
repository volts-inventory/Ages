//! `ViewportEmitter` — the `Emitter` wrapper that mirrors per-civ
//! state from the event stream and re-renders the world to a
//! `Write` (typically stdout) at the configured frame cadence.
//!
//! The implementation is split across sibling modules to keep each
//! responsibility on its own page:
//!
//! - this file owns the struct + `Emitter` trait dispatch +
//!   alt-screen lifecycle (`ensure_initialised` / `shutdown`).
//! - `state.rs` mirrors incoming events into the snapshots the
//!   renderer consumes (`apply_state` + the `should_render` gate).
//! - `log.rs` classifies events into scrolling-log lines
//!   (`log_message` + small `civ_label` / `relation_label` helpers
//!   + the `COSMOLOGY_AXIS_NAMES` table).
//! - `cards.rs` formats the planet / species info-cards.
//! - `sidebar.rs` composes the per-civ sidebar panels.
//! - `layout.rs` orchestrates the three-region frame composition
//!   and the per-row absolute-positioning output strategy.

use super::ansi::{ANSI_ALT_SCREEN_OFF, ANSI_ALT_SCREEN_ON, ANSI_HIDE_CURSOR, ANSI_SHOW_CURSOR};
use super::config::ViewportConfig;
use crate::frame::CivClaim;
use protocol::{Event, PlanetDerived, PlanetMap, RunMetadata, SpeciesDerived};
use sim_events::{EmitError, Emitter};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::Write;

/// `out` periodically. See module docs for state rules.
pub struct ViewportEmitter<W: Write> {
    pub(super) out: W,
    pub(super) cfg: ViewportConfig,
    pub(super) planet_map: Option<PlanetMap>,
    pub(super) planet: Option<PlanetDerived>,
    /// Latest Species event captured. The species is
    /// derived once at run start; this stores it so
    /// the planet card can show name + cognition + primary
    /// modality + primary manipulation.
    pub(super) species: Option<SpeciesDerived>,
    /// Presentation metadata captured from the
    /// `RunMetadata` event. Carries substrate freeze/boil ranges
    /// (formerly hardcoded in `host_species_status`) plus the
    /// label tables that downstream Python consumers read. The
    /// viewport reads only the substrate ranges; everything else
    /// is sourced via `crate::labels` directly.
    pub(super) metadata: Option<RunMetadata>,
    pub(super) civs: BTreeMap<u32, CivClaim>,
    /// Cells where the species has nomadic presence (no
    /// civ claim, but population > `NOMAD_DISPLAY_FLOOR_POP`).
    /// Total nomadic population summed across all cells in
    /// the most recent `SpeciesNomadsChanged` snapshot. Surfaced
    /// in the caption alongside civ count so the user can see the
    /// species' nomadic mass shrink as civs absorb cells.
    pub(super) nomad_total_pop: f64,
    /// Mirrored from `SpeciesNomadsChanged` events. Rendered as
    /// `0` glyphs in the viewport map.
    pub(super) nomad_cells: BTreeSet<u32>,
    /// Per-cell producer-life index in `[0, 1]`, mirrored from the
    /// latest `CellBiomass` event (row-major, `PlanetMap` cell order).
    /// Passed into each `WorldFrame` so the coloured map tints land by
    /// vegetation rather than elevation. Empty until the first
    /// `CellBiomass` arrives.
    pub(super) producer_index: Vec<f64>,
    /// Live area-mean surface temperature (Kelvin), mirrored from the
    /// latest `ClimateSample`. The planet card, scorched/habitable
    /// badge, and surface-phase rendering read this so a world that has
    /// drifted from its sampled mean shows its *current* climate.
    /// `None` until the first sample (then the card falls back to the
    /// sampled `Planet` mean).
    pub(super) live_mean_temperature_k: Option<f64>,
    /// Per-civ sidebar / log state — name, founding year, cosmology
    /// + religion snapshots, tech tier, tools, cohesion, life
    /// expectancy, last unlocked tool. All entries cleared together
    /// on `CivCollapsed` so a re-emergent civ starts fresh. Read
    /// via `Option<&CivState>` with field-level fallbacks where the
    /// renderer needs "civ exists but X never set" semantics.
    pub(super) civ_state: BTreeMap<u32, CivState>,
    /// Latest tick observed (from `Tick` events). Frame caption
    /// reads "Year N" from here.
    pub(super) current_tick: u64,
    /// Latch so the alt-screen prologue writes exactly once on
    /// the first frame, regardless of when state becomes
    /// renderable. Final cleanup is keyed on `RunEnd`.
    pub(super) initialised: bool,
    /// Snapshot counters for the status line.
    pub(super) civ_founded_count: u64,
    pub(super) civ_collapsed_count: u64,
    /// Rolling tail of significant events for the
    /// scrolling log section. Each entry is a pre-formatted
    /// `[year N] message` string. Older events drop off the
    /// front when the deque exceeds `cfg.log_lines`.
    pub(super) recent_events: VecDeque<String>,
    /// Track which (winner, loser) pairs have already had a
    /// "conflict resolved" line emitted to the log. Multi-tick
    /// wars produce one `ConflictResolved` event per cell-flip-
    /// with-loser-defeated-true, which would flood the log. We
    /// emit only the first defeat per pair and reset after the
    /// loser collapses (so a re-emerged civ can have its war
    /// re-emitted later).
    pub(super) wars_logged: BTreeSet<(u32, u32)>,
    /// First-only filter for `RelationConfirmed`. The science
    /// heartbeat fires thousands of times per run (every civ
    /// confirming every law). Dedup on `template_id` — `relation_id`
    /// is per-civ, so the same `fire` template confirmed by five
    /// civs would still fire five log lines under a `relation_id`
    /// filter. Per-template gives the species-level beat once per
    /// phenomenon.
    pub(super) templates_confirmed_logged: BTreeSet<u32>,
    /// First-only filter for `RelationFalsified` (per `relation_id`
    /// — falsification is per-civ news, distinct civs falsifying
    /// the same template are separate beats).
    pub(super) relations_falsified_logged: BTreeSet<u32>,
    /// First-only filter for `RelationLapsed` (per `relation_id`).
    pub(super) relations_lapsed_logged: BTreeSet<u32>,
    /// Per-pair coalescing for `KnowledgeTransmitted`. After two
    /// civs first establish contact, dozens of relations stream
    /// across in a burst; we collapse the burst into one log line
    /// per (source, dest) pair for the lifetime of the run.
    pub(super) transmissions_logged: BTreeSet<(u32, u32)>,
    /// Civ pairs currently at war. Mirrors core's
    /// `war_state` map so the per-civ sidebar panels can show
    /// `⚔ war: civ X` lines without re-deriving from
    /// `ConflictResolved` cell-overlap noise. Pair key is
    /// normalised `(min, max)`.
    pub(super) wars_active: BTreeSet<(u32, u32)>,
    /// `relation_id` → `template_name` map populated on the first
    /// `RelationConfirmed` we see for each relation. Log lines for
    /// downstream relation events (falsified / lapsed / rival /
    /// displaced / mythologized) look up the template name here
    /// so they read as `lost "water flows downhill"` instead of
    /// the cryptic `lost r438`.
    pub(super) relation_template_names: BTreeMap<u32, String>,
    /// Per-civ snapshot of total population (raw Q96.32 bits)
    /// captured at the end of the previous render. Compared
    /// against the new total at render time so the sidebar's pop
    /// line gets a ↑ / ↓ / → trend arrow. ±0.5% threshold keeps
    /// the arrow stable under sub-tick noise. Cleared on
    /// `CivCollapsed`.
    pub(super) civ_last_emitted_pop_q32: BTreeMap<u32, i128>,
}

/// Per-civ snapshot state surfaced by the sidebar panels and
/// log lines. One entry per active civ; cleared on `CivCollapsed`.
/// Fields that have natural defaults (empty `String`, `0` tier,
/// empty `BTreeSet`) use them directly; the rest are `Option` so
/// "civ exists but field never observed" reads distinctly from
/// "field has its default value".
#[derive(Debug, Clone, Default)]
pub(super) struct CivState {
    pub(super) name: String,
    pub(super) founded_year: u64,
    pub(super) cosmology: Option<[f64; 5]>,
    pub(super) religion: Option<[f64; 3]>,
    pub(super) tech_tier: u8,
    pub(super) tools_unlocked: BTreeSet<String>,
    pub(super) cohesion: Option<f64>,
    pub(super) life_expectancy_months: Option<f64>,
    pub(super) last_unlocked_tool: Option<String>,
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
            producer_index: Vec::new(),
            live_mean_temperature_k: None,
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

    pub(super) fn ensure_initialised(&mut self) -> std::io::Result<()> {
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

    pub(super) fn shutdown(&mut self) -> std::io::Result<()> {
        if self.initialised && self.cfg.use_alt_screen {
            self.out.write_all(ANSI_SHOW_CURSOR.as_bytes())?;
            self.out.write_all(ANSI_ALT_SCREEN_OFF.as_bytes())?;
        }
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
