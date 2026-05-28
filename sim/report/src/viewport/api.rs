//! Public read API over the viewport's accumulated state. The
//! interactive TUI (`crate::tui`) reuses `ViewportEmitter` purely as
//! an event→state mirror — it feeds events through [`apply`] and
//! renders ratatui widgets from these accessors — so the rich
//! event-classification + per-civ snapshot logic in the sibling
//! modules has a single home and never drifts between the legacy
//! ANSI viewport and the TUI.
//!
//! [`apply`]: ViewportEmitter::apply

use super::emitter::{CivState, ViewportEmitter};
use crate::frame::{centroid_symbol, civ_color_code, CivClaim, WorldFrame};
use crate::q32::pop_q32_to_f64;
use crate::render::SurfacePhase;
use protocol::{Event, PlanetDerived, PlanetMap};
use std::collections::VecDeque;
use std::io::Write;

/// Read-only per-civ snapshot for the TUI civ list + detail panes.
/// Built fresh on demand from the mirrored `civs` / `civ_state`
/// maps; one entry per currently-active civ.
#[derive(Debug, Clone)]
pub struct CivPanel {
    pub id: u32,
    pub name: String,
    /// xterm-256 palette index for this civ (matches the map glyphs).
    pub color_idx: u8,
    /// Capital marker letter (`A`..`Z`) shown on the map centroid.
    pub centroid_letter: char,
    pub founded_year: u64,
    pub tier: u8,
    pub tool_count: usize,
    pub last_tool: Option<String>,
    pub pop: f64,
    pub cells: usize,
    pub cohesion_pct: Option<i64>,
    pub life_years: Option<f64>,
    pub at_war: bool,
    pub rivals: Vec<String>,
    pub cosmology_axis: Option<(&'static str, f64)>,
    pub religion_axis: Option<(&'static str, f64)>,
}

/// Names of the three religion axes, in `axes_q32` index order.
const RELIGION_AXIS_NAMES: [&str; 3] = ["theology", "ritual", "afterlife"];

/// Pick the strongest-magnitude axis above a `0.20` deadband.
fn dominant_axis(values: &[f64], names: &[&'static str]) -> Option<(&'static str, f64)> {
    let mut best: Option<(usize, f64)> = None;
    for (i, &v) in values.iter().enumerate() {
        if v.abs() < 0.20 {
            continue;
        }
        if best.is_none_or(|(_, b): (usize, f64)| v.abs() > b.abs()) {
            best = Some((i, v));
        }
    }
    best.map(|(i, v)| (names[i], v))
}

impl<W: Write> ViewportEmitter<W> {
    /// Mirror one event into the accumulated state without rendering.
    /// The TUI calls this for every event drained from the channel;
    /// it then draws from the accessors below at its own frame rate.
    pub fn apply(&mut self, ev: &Event) {
        self.apply_state(ev);
    }

    #[must_use]
    pub fn planet_map(&self) -> Option<&PlanetMap> {
        self.planet_map.as_ref()
    }

    #[must_use]
    pub fn planet(&self) -> Option<&PlanetDerived> {
        self.planet.as_ref()
    }

    #[must_use]
    pub fn planet_name(&self) -> Option<&str> {
        self.planet.as_ref().map(|p| p.name.as_str())
    }

    /// Planet mean surface temperature formatted in the configured
    /// unit (e.g. `221F`), or `None` until the `Planet` event lands.
    /// Mirrors the planet card's climate line so the status bar and
    /// card agree.
    #[must_use]
    pub fn mean_temp_display(&self) -> Option<String> {
        let p = self.planet.as_ref()?;
        let k = crate::q32::q32_to_f64(p.mean_temperature_q32);
        let value = self.cfg.temperature_unit.from_kelvin(k);
        Some(format!("{value:.0}{}", self.cfg.temperature_unit.suffix()))
    }

    #[must_use]
    pub fn species_name(&self) -> Option<&str> {
        self.species.as_ref().map(|s| s.name.as_str())
    }

    /// Coarse surface-physics state for the active planet — used to
    /// pick the terrain glyph set (`Earthlike` / `Lava` / `IceCap`).
    #[must_use]
    pub fn phase(&self) -> SurfacePhase {
        self.surface_phase()
    }

    /// Two/three-line planet stat card (reuses the ANSI viewport's
    /// formatter so the TUI shows identical text).
    #[must_use]
    pub fn planet_card_text(&self) -> Option<String> {
        self.planet_card()
    }

    /// Three-line species recap (cognition / senses / biology).
    #[must_use]
    pub fn species_card_text(&self) -> Option<String> {
        self.species_card()
    }

    #[must_use]
    pub fn current_tick(&self) -> u64 {
        self.current_tick
    }

    /// Planet orbital period in months (= ticks per year).
    #[must_use]
    pub fn orbital_period(&self) -> u32 {
        self.planet
            .as_ref()
            .map_or(protocol::BASELINE_MONTHS_PER_YEAR as u32, |p| {
                p.orbital_period_months
            })
    }

    #[must_use]
    pub fn active_civ_count(&self) -> usize {
        self.civs.len()
    }

    #[must_use]
    pub fn founded_count(&self) -> u64 {
        self.civ_founded_count
    }

    #[must_use]
    pub fn collapsed_count(&self) -> u64 {
        self.civ_collapsed_count
    }

    #[must_use]
    pub fn nomad_pop(&self) -> f64 {
        self.nomad_total_pop
    }

    #[must_use]
    pub fn nomad_cell_count(&self) -> usize {
        self.nomad_cells.len()
    }

    /// Total species mass: summed per-cell civ populations plus the
    /// running nomadic total. Clamped at zero (sub-integer f64 noise).
    #[must_use]
    pub fn total_pop(&self) -> f64 {
        let civ_pop: f64 = self
            .civs
            .values()
            .flat_map(|c| c.cell_populations_q32.values())
            .map(|p| pop_q32_to_f64(*p))
            .sum();
        (civ_pop + self.nomad_total_pop).max(0.0)
    }

    /// The rolling tail of significant-event log lines (oldest first).
    #[must_use]
    pub fn log_lines(&self) -> &VecDeque<String> {
        &self.recent_events
    }

    /// Names of the tools a civ has unlocked, sorted (the `BTreeSet`
    /// order). Borrowed so the detail pane can list them for the
    /// selected civ without cloning every civ's tool set per frame.
    #[must_use]
    pub fn civ_tools(&self, id: u32) -> Vec<&str> {
        self.civ_state
            .get(&id)
            .map(|s| s.tools_unlocked.iter().map(String::as_str).collect())
            .unwrap_or_default()
    }

    /// Snapshot of all active civs at this tick. Order doesn't
    /// matter to the renderer; the TUI re-sorts as it sees fit.
    #[must_use]
    pub fn world_frame(&self) -> WorldFrame {
        WorldFrame {
            tick: self.current_tick,
            civs: self.civs.values().cloned().collect(),
            nomad_cells: self.nomad_cells.iter().copied().collect(),
        }
    }

    /// Per-civ panel snapshots, sorted by population (largest first),
    /// civ id ascending on ties — the order the TUI lists them in.
    #[must_use]
    pub fn civ_panels(&self) -> Vec<CivPanel> {
        let period = self.orbital_period();
        let mut panels: Vec<CivPanel> = self
            .civs
            .iter()
            .map(|(&id, claim)| self.build_panel(id, claim, period))
            .collect();
        panels.sort_by(|a, b| {
            b.pop
                .partial_cmp(&a.pop)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });
        panels
    }

    fn build_panel(&self, id: u32, claim: &CivClaim, period: u32) -> CivPanel {
        let state: Option<&CivState> = self.civ_state.get(&id);
        let name = state
            .map(|s| s.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| format!("civ {id}"));
        let pop: f64 = claim
            .cell_populations_q32
            .values()
            .map(|p| pop_q32_to_f64(*p))
            .sum();
        let cohesion_pct = state
            .and_then(|s| s.cohesion)
            .map(|c| (c * 100.0).round().clamp(0.0, 100.0) as i64);
        let life_years = state
            .and_then(|s| s.life_expectancy_months)
            .map(|m| m / f64::from(period))
            .filter(|y| *y > 0.5);
        let mut rivals: Vec<u32> = self
            .wars_active
            .iter()
            .filter_map(|(a, b)| {
                if *a == id {
                    Some(*b)
                } else if *b == id {
                    Some(*a)
                } else {
                    None
                }
            })
            .collect();
        rivals.sort_unstable();
        rivals.dedup();
        let cosmology_axis = state
            .and_then(|s| s.cosmology)
            .and_then(|c| dominant_axis(&c, &Self::COSMOLOGY_AXIS_NAMES));
        let religion_axis = state
            .and_then(|s| s.religion)
            .and_then(|r| dominant_axis(&r, &RELIGION_AXIS_NAMES));
        CivPanel {
            id,
            color_idx: civ_color_code(id),
            centroid_letter: centroid_symbol(id),
            founded_year: state.map_or(0, |s| s.founded_year),
            tier: state.map_or(0, |s| s.tech_tier),
            tool_count: state.map_or(0, |s| s.tools_unlocked.len()),
            last_tool: state.and_then(|s| s.last_unlocked_tool.clone()),
            pop,
            cells: claim.claimed_cells.len(),
            cohesion_pct,
            life_years,
            at_war: !rivals.is_empty(),
            rivals: rivals.iter().map(|r| self.civ_label(*r)).collect(),
            name,
            cosmology_axis,
            religion_axis,
        }
    }
}
