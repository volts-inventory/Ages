//! Per-civ chapter rendering: founding card + key discoveries +
//! refinements + figures + tech ladder + catastrophes + cosmology
//! + collapse, plus the supporting helpers for territory maps,
//!   settlement-tier counts, and territory-history sparklines.

use super::{
    digest_period, pretty_form, terrain_symbol, tick_to_month, tick_to_year, COSMOLOGY_AXES,
};
use crate::digest::{
    CivChapter, CollapseRecord, Digest, DiscoveryRecord, RefinementOutcome, RefinementRecord,
};
use crate::q32::{fmt_pop, pop_q32_to_f64, q32_to_f64};
use std::collections::BTreeMap;
use std::fmt::Write;

fn render_territory_sparkline(
    s: &mut String,
    history: &[crate::digest::TerritorySnapshot],
    period: u32,
) {
    const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    if history.is_empty() {
        return;
    }
    let counts: Vec<usize> = history.iter().map(|t| t.claimed_cells.len()).collect();
    let max = *counts.iter().max().unwrap_or(&1).max(&1);
    let mut line = String::new();
    for c in &counts {
        let idx = ((c.saturating_mul(BARS.len() - 1)) / max).min(BARS.len() - 1);
        line.push(BARS[idx]);
    }
    let first_year = tick_to_year(history.first().map_or(0, |t| t.tick), period);
    let last_year = tick_to_year(history.last().map_or(0, |t| t.tick), period);
    let _ = writeln!(s, "```text");
    let _ = writeln!(
        s,
        "Territory over time (year {first_year}–{last_year}, peak {max} cells):"
    );
    let _ = writeln!(s);
    let _ = writeln!(s, "  {line}");
    let _ = writeln!(s, "```");
}

/// BFS-ring distance from `centroid` to every cell on the hex
/// torus, capped at `max_ring`. Used by settlement-density
/// rendering to bucket claimed cells into tiers (capital / town /
/// village / hamlet) based on how far they sit from the civ's
/// founding focus. Mirrors the sim/core territory BFS (same axial
/// neighbour offsets, same torus wrap) so the topology agrees with
/// what the sim actually computed at claim time.
fn bfs_distances(centroid: u32, width: u32, height: u32) -> BTreeMap<u32, u32> {
    const OFFSETS: [(i32, i32); 6] = [(1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)];
    let mut out: BTreeMap<u32, u32> = BTreeMap::new();
    if width == 0 || height == 0 {
        return out;
    }
    let n = (width as usize) * (height as usize);
    if (centroid as usize) >= n {
        return out;
    }
    let mut queue: std::collections::VecDeque<(u32, u32)> = std::collections::VecDeque::new();
    queue.push_back((centroid, 0));
    out.insert(centroid, 0);
    while let Some((cell, d)) = queue.pop_front() {
        let q = (cell % width) as i32;
        let r = (cell / width) as i32;
        for (dq, dr) in OFFSETS {
            let nq = (q + dq).rem_euclid(width as i32);
            let nr = (r + dr).rem_euclid(height as i32);
            let nc = ((nr as u32) * width) + (nq as u32);
            if let std::collections::btree_map::Entry::Vacant(e) = out.entry(nc) {
                e.insert(d + 1);
                queue.push_back((nc, d + 1));
            }
        }
    }
    out
}

/// Bucket a BFS ring distance into a settlement tier label.
/// Ring 0 (the centroid itself) is the civ's capital; the
/// progressively-distant rings step down through town, village,
/// hamlet. Capped at hamlet for the outer rings — past 5 rings
/// out the cells are remote frontier and lumped together.
fn settlement_tier(ring: u32) -> &'static str {
    match ring {
        0 => "capital",
        1..=2 => "town",
        3..=4 => "village",
        _ => "hamlet",
    }
}

/// Append a one-line settlement-pattern summary to `s`. Reads
/// the civ's claimed cells, ring-distances them from the founder's
/// `cell_assignment`, and counts each tier. A tight civ around its
/// homeland reads "1 capital, 6 towns" — a sprawling empire reads
/// "1 capital, 6 towns, 12 villages, 18 hamlets". Skipped silently
/// when the civ has no figures or no claimed cells.
fn render_settlement_pattern(s: &mut String, pm: &protocol::PlanetMap, civ: &CivChapter) {
    // Render in semantic order, not alphabetical — capital first,
    // then towns out to hamlets. BTreeMap doesn't preserve insertion
    // order, so walk the canonical sequence manually.
    const ORDER: [&str; 4] = ["capital", "town", "village", "hamlet"];
    if civ.claimed_cells.is_empty() || civ.figures.is_empty() {
        return;
    }
    let centroid = civ.figures[0].cell_assignment;
    let dist = bfs_distances(centroid, pm.grid_width, pm.grid_height);
    let mut counts: BTreeMap<&'static str, u32> = BTreeMap::new();
    for cell in &civ.claimed_cells {
        let ring = dist.get(cell).copied().unwrap_or(u32::MAX);
        let tier = settlement_tier(ring);
        *counts.entry(tier).or_insert(0) += 1;
    }
    let parts: Vec<String> = ORDER
        .iter()
        .filter_map(|tier| {
            counts.get(tier).map(|&n| {
                let label = if n == 1 {
                    (*tier).to_string()
                } else {
                    format!("{tier}s")
                };
                format!("{n} {label}")
            })
        })
        .collect();
    if !parts.is_empty() {
        let _ = writeln!(s, "**Settlement pattern:** {}.", parts.join(", "));
        let _ = writeln!(s);
    }
}

/// claimed cells are marked with the civ's id digit (or `*` for
/// civ ids ≥ 10) and figures' `cell_assignment` cells are marked
/// with the uppercase first letter of the figure's name. Other
/// cells fall back to the terrain symbol so context is preserved.
fn render_civ_territory_map(
    s: &mut String,
    pm: &protocol::PlanetMap,
    planet: Option<&protocol::PlanetDerived>,
    civ: &CivChapter,
) {
    if pm.grid_width == 0 || pm.grid_height == 0 {
        return;
    }
    let terrain_peak = planet.map_or(0.0, |p| q32_to_f64(p.terrain_peak_q32));
    let claimed: std::collections::BTreeSet<u32> = civ.claimed_cells.iter().copied().collect();
    // Map cell_assignment → first letter of figure's name (active
    // figures only; retired_tick logic isn't in the report's
    // FigureRecord, so all listed figures count).
    let mut figure_marker: std::collections::BTreeMap<u32, char> =
        std::collections::BTreeMap::new();
    for fig in &civ.figures {
        let initial = fig.name.chars().next().unwrap_or('?').to_ascii_uppercase();
        figure_marker.entry(fig.cell_assignment).or_insert(initial);
    }
    let w = pm.grid_width as usize;
    let h = pm.grid_height as usize;
    let _ = writeln!(s, "```text");
    let _ = writeln!(
        s,
        "Territory of civ {}: {} marks claimed cells; capitals are figures' attention foci.",
        civ.civ_id,
        if civ.civ_id < 10 {
            (b'0' + civ.civ_id as u8) as char
        } else {
            '*'
        }
    );
    let _ = writeln!(s);
    // Column header.
    let mut header = String::from("    ");
    for q in 0..w {
        let _ = write!(header, "{q:>2}");
    }
    let _ = writeln!(s, "{header}");
    for r in 0..h {
        let mut line = format!("{r:>2}  ");
        if r % 2 == 1 {
            line.push(' ');
        }
        for q in 0..w {
            let cell_idx = (r * w + q) as u32;
            let symbol = if let Some(&initial) = figure_marker.get(&cell_idx) {
                initial
            } else if claimed.contains(&cell_idx) {
                if civ.civ_id < 10 {
                    (b'0' + civ.civ_id as u8) as char
                } else {
                    '*'
                }
            } else {
                terrain_symbol(pm, r, q, terrain_peak)
            };
            line.push(symbol);
            line.push(' ');
        }
        let _ = writeln!(s, "{line}");
    }
    let _ = writeln!(s, "```");
    let _ = writeln!(s);
}

pub(super) fn render_civ_chapter(s: &mut String, civ: &CivChapter, d: &Digest) {
    let period = digest_period(d);
    let _ = writeln!(s, "### Civ {}", civ.civ_id);
    let _ = writeln!(s);
    let parent = civ.parent_civ_id.map_or_else(
        || "_inaugural — founded with the species_".to_string(),
        |p| format!("succeeded civ {p}"),
    );
    // Territory peak/end: walk the per-tick history collected
    // from CivTerritoryChanged events. Lets the chapter's first
    // line tell the arc — a civ that shrunk during a dark age
    // reads "peaked at 27 cells, ended at 13" rather than just
    // "claims 13 cells."
    let initial_cells = civ
        .territory_history
        .first()
        .map_or(civ.claimed_cells.len(), |t| t.claimed_cells.len());
    let peak_cells = civ
        .territory_history
        .iter()
        .map(|t| t.claimed_cells.len())
        .max()
        .unwrap_or(initial_cells);
    let final_cells = civ.claimed_cells.len();
    let arc = if peak_cells > final_cells && final_cells > 0 {
        format!(
            "started at {initial_cells} cells, peaked at {peak_cells}, currently {final_cells} (territory contracted)"
        )
    } else if peak_cells > initial_cells {
        format!("started at {initial_cells} cells, currently {final_cells} (peak {peak_cells})")
    } else {
        format!("claims {final_cells} cells")
    };
    let _ = writeln!(
        s,
        "Founded in year {} ({}). Founding band of {} figures; initial population {}; {}.",
        tick_to_year(civ.founded_tick, period),
        parent,
        civ.founding_figure_count,
        fmt_pop(pop_q32_to_f64(civ.initial_population_q32)),
        arc,
    );
    // Life expectancy at birth — surfaces the demographic
    // transition driven by tech (sanitation + medicine reduce
    // per-bracket mortality; senescence treatment extends the
    // biological cap). Only render if the civ has at least one
    // recorded snapshot (founded after the 4-bracket model
    // shipped).
    if let Some((founding_le, current_le)) = (|| {
        let history = &civ.life_expectancy_history;
        let first = history.first()?;
        let last = history.last()?;
        let to_years = |months_q32: i64| -> f64 {
            q32_to_f64(months_q32) / f64::from(protocol::BASELINE_MONTHS_PER_YEAR as u32)
        };
        Some((to_years(first.life_expectancy_months_q32), to_years(last.life_expectancy_months_q32)))
    })() {
        if (current_le - founding_le).abs() >= 1.0 {
            let _ = writeln!(
                s,
                "Life expectancy at birth: founded with {founding_le:.0}y, currently {current_le:.0}y."
            );
        } else {
            let _ = writeln!(
                s,
                "Life expectancy at birth: {founding_le:.0}y."
            );
        }
    }
    // M8 — economic arc: founded with N, peaked at P (year Y),
    // ending at C. Skipped when no surplus snapshots accumulated
    // (civ never reached the 50-pop emit floor on its surplus).
    if !civ.surplus_history.is_empty() {
        let first = &civ.surplus_history[0];
        let last = civ.surplus_history.last().unwrap();
        let peak = civ
            .surplus_history
            .iter()
            .max_by_key(|h| h.surplus_q32)
            .unwrap();
        let founding = pop_q32_to_f64(i128::from(first.surplus_q32));
        let peak_v = pop_q32_to_f64(i128::from(peak.surplus_q32));
        let current = pop_q32_to_f64(i128::from(last.surplus_q32));
        let peak_year = tick_to_year(peak.tick, period);
        if peak_v <= 0.0 && current <= 0.0 {
            // No meaningful surplus accumulated.
        } else if (peak_v - current).abs() < 1.0 && (peak_v - founding).abs() < 1.0 {
            let _ = writeln!(
                s,
                "Economy: held a surplus of {} pop-equivalents.",
                fmt_pop(peak_v)
            );
        } else {
            let _ = writeln!(
                s,
                "Economy: surplus peaked at {} pop-equivalents in year {}, currently {}.",
                fmt_pop(peak_v),
                peak_year,
                fmt_pop(current)
            );
        }
    }
    let _ = writeln!(s);

    // Territory size sparkline if there's enough history. Each
    // tick's row shows when the civ grew or shrank — dark ages
    // appear as the bars stepping down.
    if civ.territory_history.len() >= 3 {
        render_territory_sparkline(s, &civ.territory_history, period);
        let _ = writeln!(s);
    }

    // Per-civ territory map: shades the cells this civ claims
    // with its civ id, marks figures' attention foci with the
    // initial of their name. Renders the *current* claim (last
    // entry in territory_history); civ.claimed_cells is the
    // authoritative final state. Skip if no PlanetMap is in the
    // digest (very old logs) or if the civ claims no cells.
    if let Some(pm) = &d.planet_map {
        if !civ.claimed_cells.is_empty() {
            render_settlement_pattern(s, pm, civ);
            render_civ_territory_map(s, pm, d.planet.as_ref(), civ);
        }
    }
    let _ = writeln!(s);

    if civ.figures.is_empty() {
        let _ = writeln!(s, "_(no named figures)_");
    } else {
        let _ = writeln!(s, "**Named figures ({}):**", civ.figures.len());
        for f in &civ.figures {
            let n = civ
                .discoveries
                .iter()
                .filter(|d| d.figure_id == f.id)
                .count();
            let _ = writeln!(
                s,
                "- *{}* (id {}, born year {}) — {} confirmed relation{}",
                f.name,
                f.id,
                tick_to_year(f.tick, period),
                n,
                if n == 1 { "" } else { "s" }
            );
        }
        let _ = writeln!(s);
    }

    if civ.techs.is_empty() {
        let _ = writeln!(s, "**Tech ladder:** _no unlocks_");
    } else {
        let _ = writeln!(s, "**Tech ladder:**");
        for t in &civ.techs {
            let extra = if t.newly_perceivable_template_ids.is_empty() {
                String::new()
            } else {
                format!(
                    " — newly perceivable: {}",
                    t.newly_perceivable_template_ids
                        .iter()
                        .map(|id| d
                            .template_names
                            .get(id)
                            .cloned()
                            .map_or_else(|| format!("template_{id}"), |n| format!("`{n}`")))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            let _ = writeln!(
                s,
                "- year {}: tier-{} `{}` (channels: {}){}",
                tick_to_year(t.tick, period),
                t.tier,
                t.tool_name,
                t.granted_channels
                    .iter()
                    .map(|c| format!("`{c}`"))
                    .collect::<Vec<_>>()
                    .join(", "),
                extra,
            );
        }
    }
    let _ = writeln!(s);

    if civ.discoveries.is_empty() {
        let _ = writeln!(s, "**Discoveries:** _no relations confirmed_");
    } else {
        let _ = writeln!(s, "**Key discoveries ({}):**", civ.discoveries.len());
        for disc in discoveries_to_show(civ) {
            let figure = civ
                .figures
                .iter()
                .find(|f| f.id == disc.figure_id)
                .map_or_else(|| format!("figure {}", disc.figure_id), |f| f.name.clone());
            let _ = writeln!(
                s,
                "- year {}: *{}* fitted **{}** ↔ **{}** as `{}` → {} (residual {:.3}, confidence {:.3}, n={})",
                tick_to_year(disc.tick, period),
                figure,
                disc.template_name,
                disc.channel,
                disc.form,
                pretty_form(disc),
                q32_to_f64(disc.residual_q32),
                q32_to_f64(disc.confidence_q32),
                disc.n_samples,
            );
        }
    }
    let _ = writeln!(s);

    if !civ.refinements.is_empty() {
        let _ = writeln!(s, "**Refinements ({}):**", civ.refinements.len());
        for r in refinements_to_show(civ) {
            let label = d.relation_names.get(&r.relation_id).map_or_else(
                || format!("relation {}", r.relation_id),
                |l| format!("`{}` ↔ `{}`", l.template_name, l.channel),
            );
            let figure = civ
                .figures
                .iter()
                .find(|f| f.id == r.figure_id)
                .map_or_else(|| format!("figure {}", r.figure_id), |f| f.name.clone());
            let outcome = match &r.outcome {
                RefinementOutcome::Proposed => "proposed".to_string(),
                RefinementOutcome::Confirmed {
                    new_confidence_q32, ..
                } => format!(
                    "confirmed (new confidence {:.3})",
                    q32_to_f64(*new_confidence_q32)
                ),
                RefinementOutcome::Rejected { reason } => format!("rejected ({reason})"),
            };
            let _ = writeln!(
                s,
                "- year {}: *{}* on {} — {} → {} ({})",
                tick_to_year(r.tick, period),
                figure,
                label,
                r.old_form,
                r.new_form,
                outcome
            );
        }
        let _ = writeln!(s);
    }

    if !civ.catastrophes.is_empty() {
        let _ = writeln!(s, "**Catastrophes:**");
        for c in &civ.catastrophes {
            let _ = writeln!(
                s,
                "- year {} month {}: `{}` — population fell by {:.1}%",
                tick_to_year(c.tick, period),
                tick_to_month(c.tick, period),
                c.kind,
                q32_to_f64(c.fraction_lost_q32) * 100.0,
            );
        }
        let _ = writeln!(s);
    }

    if !civ.cosmology_shifts.is_empty() {
        let last = civ.cosmology_shifts.last().expect("non-empty");
        let _ = writeln!(
            s,
            "**Cosmology drift:** {} shifts; final dogmatism {:.3}; final axes {{{}}}",
            civ.cosmology_shifts.len(),
            q32_to_f64(last.dogmatism_q32),
            COSMOLOGY_AXES
                .iter()
                .zip(last.axes_q32.iter())
                .map(|(name, q)| format!("{name}={:.2}", q32_to_f64(*q)))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let _ = writeln!(s);
    }

    match &civ.collapsed {
        Some(CollapseRecord {
            tick,
            reason,
            final_population_q32,
            final_figure_count,
        }) => {
            let _ = writeln!(
                s,
                "**Collapse:** year {} — `{}`. Final population {}, {} figures left when the lights went out.",
                tick_to_year(*tick, period),
                reason,
                fmt_pop(pop_q32_to_f64(*final_population_q32)),
                final_figure_count
            );
        }
        None => {
            let _ = writeln!(s, "**Status at run-end:** _still alive._");
        }
    }
    // Scientific-lifecycle line. Surfaces the inherited /
    // revalidated / lapsed / falsified counters
    // so the per-civ chapter can narrate "civ X
    // graduated 11 inherited laws, lapsed 3, and falsified 9 of
    // its own across its lifetime." The line is omitted when all
    // counts are zero (e.g. inaugural civ before transmission
    // fires) to keep short-lived chapters terse.
    let total = civ.revalidated_count + civ.lapsed_count + civ.falsified_count;
    if total > 0 {
        let _ = writeln!(
            s,
            "**Knowledge dynamics:** revalidated {} inherited law{}, lapsed {}, falsified {} of its own.",
            civ.revalidated_count,
            if civ.revalidated_count == 1 { "" } else { "s" },
            civ.lapsed_count,
            civ.falsified_count,
        );
        let _ = writeln!(s);
    }

    // Emergent recognition templates this civ
    // proposed. Render as a short sub-section so the biographical
    // payoff of "civ X discovered the world's first
    // *temperature_ridge_threshold* template" is visible in the
    // chapter.
    if !civ.discovered_templates.is_empty() {
        let _ = writeln!(s, "**Emergent recognition templates:**");
        for t in &civ.discovered_templates {
            let _ = writeln!(
                s,
                "- year {}: `{}` (id {}) — derived from confirmed law on template {}, threshold ≈ {:.3}",
                tick_to_year(t.tick, period),
                t.template_name,
                t.template_id,
                t.origin_template_id,
                t.threshold_si,
            );
        }
        let _ = writeln!(s);
    }

    // Emergent dynamic tools this civ invented.
    if !civ.invented_tools.is_empty() {
        let _ = writeln!(s, "**Emergent tools invented:**");
        for t in &civ.invented_tools {
            let cap_mult = q32_to_f64(t.capacity_multiplier_q32);
            let lit = q32_to_f64(t.literacy_bonus_q32);
            let trans = q32_to_f64(t.transmission_fidelity_bonus_q32);
            let _ = writeln!(
                s,
                "- year {}: `{}` (tier {}, focus: {}) — from a cluster of {} confirmed laws; capacity ×{:.2}, literacy +{:.2}, transmission +{:.2}",
                tick_to_year(t.tick, period),
                t.tool_name,
                t.tier,
                t.channel_focus,
                t.cluster_size,
                cap_mult,
                lit,
                trans,
            );
        }
        let _ = writeln!(s);
    }

    let _ = writeln!(s);
}

/// Select up to ~10 most-significant discoveries for a civ — first
/// discovery of each (template, channel) pair this civ confirmed plus
/// the highest-confidence misses if there's room. Avoids drowning the
/// reader in 100+ entries on a long-lived civ.
fn discoveries_to_show(civ: &CivChapter) -> Vec<&DiscoveryRecord> {
    const MAX: usize = 10;
    // Two-pass: first prefer non-trivial firsts (real fits), then
    // fill remaining slots with constant-zero firsts only if there
    // aren't enough real ones — most chapters get rich (template,
    // channel) coverage and never see a trivial fit, but the very
    // first tick of an inaugural civ on a sparse-firing planet
    // sometimes only has zeros to show.
    let mut seen: std::collections::BTreeSet<(u32, String)> = std::collections::BTreeSet::new();
    let mut firsts: Vec<&DiscoveryRecord> = Vec::new();
    for d in &civ.discoveries {
        if !is_trivial_constant_discovery(d) && seen.insert((d.template_id, d.channel.clone())) {
            firsts.push(d);
        }
        if firsts.len() >= MAX {
            return firsts;
        }
    }
    if firsts.is_empty() {
        for d in &civ.discoveries {
            if seen.insert((d.template_id, d.channel.clone())) {
                firsts.push(d);
            }
            if firsts.len() >= MAX {
                break;
            }
        }
    }
    firsts
}

/// Trivial-fit filter applied at the per-civ chapter and species
/// canon layers. A fit is trivial when every y-coefficient of its
/// form is near zero — that is, the relation reduces to "y is
/// always (essentially) 0 across the sample window," which is an
/// absence rather than a discovery.
///
/// Form-specific: each form's params include some y-coefficients
/// (which scale or offset the output) and possibly an x-coordinate
/// like `threshold_step`'s `t`. Only y-coefficients are checked;
/// x-coordinates are ignored. Threshold: 16 in raw `Q32.32` bits is
/// ≈ 3.7e-9 in real units.
pub(super) fn is_trivial_constant_discovery(d: &DiscoveryRecord) -> bool {
    let y_coeff_indices: &[usize] = match d.form.as_str() {
        // Single y-coefficient at index 0.
        "constant" | "exp_decay" | "exp_growth" | "power_law" | "inverse_square" | "logistic" => {
            &[0]
        }
        // Two y-coefficients (slope + intercept). `threshold_step`
        // shares this pattern: params are [a, b, t] where a/b are
        // y-values and t (index 2) is the x-threshold.
        "linear" | "logarithmic" | "threshold_step" => &[0, 1],
        "polynomial_2" => &[0, 1, 2],
        "polynomial_3" => &[0, 1, 2, 3],
        // periodic_sine params are [a, b, c, d]: amplitude a,
        // frequency b, phase c, offset d. Trivial when a and d
        // are both 0 (output is identically zero).
        "periodic_sine" => &[0, 3],
        _ => &[0],
    };
    if y_coeff_indices.is_empty() {
        return false;
    }
    y_coeff_indices
        .iter()
        .all(|&i| d.params_q32.get(i).is_some_and(|p| p.unsigned_abs() < 16))
}

fn refinements_to_show(civ: &CivChapter) -> Vec<&RefinementRecord> {
    const MAX: usize = 10;
    civ.refinements.iter().take(MAX).collect()
}
