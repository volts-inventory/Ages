//! Right-hand sidebar composition for the viewport: legend
//! sub-block, species recap, and one panel per active civ (name +
//! colour swatch, tech tier, founding year, population trend,
//! cohesion / war / belief axes). Pure formatter: reads snapshot
//! state from `ViewportEmitter` and returns a `Vec<String>` that
//! `render()` zips alongside the map rows.

use super::emitter::{CivState, ViewportEmitter};
use crate::frame::{centroid_symbol, civ_color_code, claim_symbol, CivClaim};
use crate::q32::fmt_pop;
use std::collections::BTreeMap;
use std::io::Write;

impl<W: Write> ViewportEmitter<W> {
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
    pub(super) fn build_sidebar_lines(&mut self) -> Vec<String> {
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
            // Terrain glyphs depend on planet surface phase: lava
            // worlds drop sea/coast entries in favour of magma,
            // ice worlds add the ice-sheet glyph.
            let (terrain1, terrain2) = match self.surface_phase() {
                crate::render::SurfacePhase::Lava => (
                    "* magma · ▲peak · △outcrop".to_string(),
                    "(rocky peaks exposed above magma sea)".to_string(),
                ),
                crate::render::SurfacePhase::IceCap => (
                    "+ ice sheet · ▲peak · △hill".to_string(),
                    "▒land · ░coast · ·=plain".to_string(),
                ),
                crate::render::SurfacePhase::Earthlike => (
                    "~sea · ≈deep · ▲peak · △hill".to_string(),
                    "▒land · ░coast · ·=plain".to_string(),
                ),
            };
            vec![density_line.to_string(), terrain1, terrain2]
        } else {
            let (line2, line3) = match self.surface_phase() {
                crate::render::SurfacePhase::Lava => (
                    "0=nomad · #=war · * magma".to_string(),
                    "▲peak · △outcrop · ·=plain".to_string(),
                ),
                crate::render::SurfacePhase::IceCap => (
                    "0=nomad · #=war · + ice sheet".to_string(),
                    "▲peak · △hill · ▒land · ░coast · ·=plain".to_string(),
                ),
                crate::render::SurfacePhase::Earthlike => (
                    "0=nomad · #=war · ~sea · ≈deep".to_string(),
                    "▲peak · △hill · ▒land · ░coast · ·=plain".to_string(),
                ),
            };
            vec!["1-9=civ-id · *=civ≥10".to_string(), line2, line3]
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
            let state: Option<&CivState> = self.civ_state.get(civ_id);
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
}
