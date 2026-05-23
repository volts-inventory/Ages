//! Event → viewport-state mirroring. `apply_state` is the
//! single-method dispatch that reads the incoming event stream and
//! updates the snapshots (`planet`, `species`, `civs`, `civ_state`,
//! dedup latches, …) that the cards / sidebar / map renderers
//! consume. Also owns the small `should_render` frame-cadence gate.

use super::emitter::{CivState, ViewportEmitter};
use crate::frame::CivClaim;
use protocol::{Event, Phase};
use std::io::Write;

impl<W: Write> ViewportEmitter<W> {
    /// Apply state-mirroring rules. Returns true if a rendering-
    /// relevant change occurred (so the caller knows the next
    /// frame would differ); the actual frame cadence is still
    /// gated on `frame_every`.
    pub(super) fn apply_state(&mut self, ev: &Event) {
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
                let s: &mut CivState = self.civ_state.entry(f.civ_id).or_default();
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

    pub(super) fn should_render(&self, ev: &Event) -> bool {
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
}
