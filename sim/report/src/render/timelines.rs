//! Time-series sections: population timeline (births/deaths over
//! time), migration patterns (cell-population flux), and world
//! keyframes (snapshot frames at fixed cadences).

use super::{digest_period, tick_to_year};
use crate::digest::Digest;
use crate::q32::q32_to_f64;
use std::fmt::Write;

/// Species population timeline — declared in `docs/architecture.md`
/// as a deliverable for the post-run report. The sim doesn't
/// emit per-tick population (would drown the event log), so we
/// derive a sparse arc from the inflection events that *do* land:
/// `CivFounded` (population added), `CivCollapsed` (cohort transitions
/// to stateless at the recorded final population), `CatastropheFired`
/// (fractional population loss applied to one civ). Rows show the
/// running active-civ count and rough species-aggregate population
/// (active + stateless cohorts that haven't been reabsorbed yet).
pub(super) fn render_population_timeline(s: &mut String, d: &Digest) {
    use protocol::Event;
    let period = digest_period(d);
    let _ = writeln!(s, "## Species population timeline");
    let _ = writeln!(s);

    // Walk every population-affecting event in tick order, keep a
    // running per-civ map; emit a row per inflection.
    let mut per_civ: std::collections::BTreeMap<u32, f64> = std::collections::BTreeMap::new();
    let mut active: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    let mut rows: Vec<(u64, String, usize, f64)> = Vec::new();

    for ev in &d.events {
        match ev {
            Event::CivFounded(c) => {
                let pop = q32_to_f64(c.initial_population_q32);
                // Successor population is sourced from the breakaway
                // parent (halved) or stateless cohort (drained); the
                // event log doesn't emit counterpart "drained" events
                // so the per-civ map can't perfectly conserve total
                // population. Decision: insert the new civ at its
                // recorded initial; leave parent/stateless entries
                // alone. The aggregate column then reflects "last
                // known per-civ population" rather than a true
                // species total — fine as a narrative arc.
                per_civ.insert(c.civ_id, pop);
                active.insert(c.civ_id);
                let label = c.parent_civ_id.map_or_else(
                    || format!("civ {} founded (inaugural)", c.civ_id),
                    |p| format!("civ {} founded (from civ {p})", c.civ_id),
                );
                rows.push((c.tick, label, active.len(), per_civ.values().sum::<f64>()));
            }
            Event::CivCollapsed(c) => {
                // Final pop becomes the stateless residual; reduce
                // the per-civ entry to that and drop from active.
                let final_pop = q32_to_f64(c.final_population_q32);
                per_civ.insert(c.civ_id, final_pop);
                active.remove(&c.civ_id);
                rows.push((
                    c.tick,
                    format!("civ {} collapsed ({})", c.civ_id, c.reason),
                    active.len(),
                    per_civ.values().sum::<f64>(),
                ));
            }
            Event::CatastropheFired(cf) => {
                let frac = q32_to_f64(cf.fraction_lost_q32).clamp(0.0, 1.0);
                if let Some(p) = per_civ.get_mut(&cf.civ_id) {
                    *p *= 1.0 - frac;
                }
                rows.push((
                    cf.tick,
                    format!("catastrophe ({}) on civ {}", cf.catastrophe_kind, cf.civ_id),
                    active.len(),
                    per_civ.values().sum::<f64>(),
                ));
            }
            _ => {}
        }
    }

    if rows.is_empty() {
        let _ = writeln!(s, "_No population inflections recorded._");
        let _ = writeln!(s);
        return;
    }

    let _ = writeln!(s, "Sparse arc from population-affecting events. *Recorded* sums each civ's last-known population (initial founding, end-of-life collapse, post-catastrophe). Population dynamics between events are not emitted, so this underestimates true peaks.");
    let _ = writeln!(s);
    let _ = writeln!(s, "| Year | Event | Active civs | Recorded pop |");
    let _ = writeln!(s, "|---:|---|---:|---:|");
    // Up to 30 rows; if longer, show the first and last 15 with an ellipsis row.
    if rows.len() <= 30 {
        for (tick, label, n_active, pop) in &rows {
            let _ = writeln!(
                s,
                "| {} | {label} | {n_active} | {pop:.0} |",
                tick_to_year(*tick, period)
            );
        }
    } else {
        for (tick, label, n_active, pop) in rows.iter().take(15) {
            let _ = writeln!(
                s,
                "| {} | {label} | {n_active} | {pop:.0} |",
                tick_to_year(*tick, period)
            );
        }
        let _ = writeln!(s, "| … | _({} omitted)_ | | |", rows.len() - 30);
        for (tick, label, n_active, pop) in rows.iter().rev().take(15).rev() {
            let _ = writeln!(
                s,
                "| {} | {label} | {n_active} | {pop:.0} |",
                tick_to_year(*tick, period)
            );
        }
    }
    let _ = writeln!(s);
}

/// Spatial-over-time keyframes: one ASCII world frame per
/// `interval` years, showing all active civs' territories at that
/// moment. Complements the population timeline (which shows civ
/// count + total pop as a 1D arc) by showing *where* civs grew,
/// where successors picked up the cells of the fallen, and where
/// civs collided spatially in time.
///
/// Frame interval is adaptive: aim for ~6 frames across the run,
/// rounded to a clean 500-year step. A 5000-year run → 1000-year
/// frames (5 frames); a 50000-year run → 8000-year frames (~6
/// frames). Calls into `frame::render_world_frame` so the per-frame
/// look matches the live `--cli=viewport` mode exactly.
/// Migration patterns: derive per-civ aggregate
/// migration totals from per-cell pop deltas across consecutive
/// `CivTerritoryChanged` snapshots. No new event types — uses
/// the per-cell pops already in the `territory_history` payload.
/// Reports the top movers: civs that shifted the
/// most population around between cells over the run.
pub(super) fn render_migration_patterns(s: &mut String, d: &Digest) {
    // Aggregate per-civ flow: total pop that ever moved into a
    // cell while the same civ was claiming it (immigration), and
    // total that ever moved out (emigration). Cell flips between
    // civs aren't counted here — that's a war / contraction
    // story, covered by the conflict + territory sections.
    struct CivFlow {
        civ_id: u32,
        immigration: f64,
        emigration: f64,
        most_volatile_cell: Option<(u32, f64)>,
    }
    let mut flows: Vec<CivFlow> = Vec::new();
    for civ in d.civs.values() {
        if civ.territory_history.len() < 2 {
            continue;
        }
        let mut immigration = 0.0_f64;
        let mut emigration = 0.0_f64;
        let mut volatile_cell: Option<(u32, f64)> = None;
        // Walk consecutive snapshots; for cells in both, diff
        // the per-cell pop. Skip snapshots without per-cell data
        // (older event logs).
        for window in civ.territory_history.windows(2) {
            let prev = &window[0];
            let next = &window[1];
            if prev.cell_populations_q32.len() != prev.claimed_cells.len()
                || next.cell_populations_q32.len() != next.claimed_cells.len()
            {
                continue;
            }
            let prev_map: std::collections::BTreeMap<u32, f64> = prev
                .claimed_cells
                .iter()
                .copied()
                .zip(prev.cell_populations_q32.iter().copied().map(q32_to_f64))
                .collect();
            for (i, &cell) in next.claimed_cells.iter().enumerate() {
                let next_pop = q32_to_f64(next.cell_populations_q32[i]);
                if let Some(&prev_pop) = prev_map.get(&cell) {
                    let delta = next_pop - prev_pop;
                    let abs_delta = delta.abs();
                    if delta > 0.0 {
                        immigration += delta;
                    } else {
                        emigration += -delta;
                    }
                    let prev_volatile = volatile_cell.map_or(0.0, |(_, v)| v);
                    if abs_delta > prev_volatile {
                        volatile_cell = Some((cell, abs_delta));
                    }
                }
            }
        }
        if immigration + emigration > 0.0 {
            flows.push(CivFlow {
                civ_id: civ.civ_id,
                immigration,
                emigration,
                most_volatile_cell: volatile_cell,
            });
        }
    }
    if flows.is_empty() {
        // Older event log without per-cell pops, or no
        // multi-snapshot civs.
        return;
    }
    // Rank by total flow (immigration + emigration).
    flows.sort_by(|a, b| {
        let sum_a = a.immigration + a.emigration;
        let sum_b = b.immigration + b.emigration;
        sum_b
            .partial_cmp(&sum_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let _ = writeln!(s, "## Migration patterns");
    let _ = writeln!(s);
    let _ = writeln!(
        s,
        "Aggregate per-cell population flow within each civ \
         (gradient migration + birth/death + catastrophe \
         redistribution). Top movers ranked by total flow:"
    );
    let _ = writeln!(s);
    let _ = writeln!(
        s,
        "| Civ | Immigration | Emigration | Net | Most-volatile cell |"
    );
    let _ = writeln!(s, "|---:|---:|---:|---:|:---|");
    for f in flows.iter().take(15) {
        let net = f.immigration - f.emigration;
        let cell_str = f.most_volatile_cell.map_or_else(
            || "—".to_string(),
            |(cell, mag)| format!("cell {cell} (±{mag:.0})"),
        );
        let _ = writeln!(
            s,
            "| {} | {:.0} | {:.0} | {:+.0} | {} |",
            f.civ_id, f.immigration, f.emigration, net, cell_str,
        );
    }
    if flows.len() > 15 {
        let _ = writeln!(s, "_(showing 15 of {})_", flows.len());
    }
    let _ = writeln!(s);
}

pub(super) fn render_world_keyframes(s: &mut String, d: &Digest) {
    let Some(pm) = &d.planet_map else {
        return;
    };
    let last_tick = d.run_end.as_ref().map_or(0, |e| e.tick);
    if last_tick == 0 {
        return;
    }
    let period = digest_period(d);
    // Target ~6 frames, snapped to a clean 500-year
    // boundary in *planet years* (orbital_period_months ticks per
    // year). On a planet with a 16-month year, 500 years = 8000 ticks.
    let last_year = tick_to_year(last_tick, period);
    let raw_year = last_year / 6;
    let interval_years = raw_year.div_ceil(500).max(1) * 500;
    let interval_ticks = interval_years * u64::from(period.max(1));
    let frames = d.keyframes(interval_ticks);
    if frames.is_empty() {
        return;
    }
    let _ = writeln!(s, "## Spatial timeline");
    let _ = writeln!(s);
    let _ = writeln!(
        s,
        "World map every {interval_years} years. \
         **Capital:** uppercase letter (`A`=civ 1, `B`=civ 2, …); \
         **claimed cells:** civ digit (`1`–`9`, `*` for civ ≥ 10); \
         **disputed cells:** `#` (claimed by multiple civs); \
         **terrain:** ▲ peak / △ mtn / ▒ inland / ░ coast / ~ shallow / ≈ deep."
    );
    let _ = writeln!(s);
    for frame in &frames {
        let active = frame.civs.len();
        let caption = format!(
            "Year {} — {active} active civ(s)",
            tick_to_year(frame.tick, period)
        );
        let body = crate::frame::render_world_frame(pm, d.planet.as_ref(), frame, &caption);
        s.push_str(&body);
        let _ = writeln!(s);
        // Paired density map showing per-cell cohort
        // population shaded as Unicode blocks (` ░ ▒ ▓ █`).
        // Empty for older event logs that don't carry per-cell
        // pops; the function returns "" and we skip rendering.
        let density = crate::frame::render_density_frame(
            pm,
            d.planet.as_ref(),
            frame,
            &format!(
                "Year {} density (capital letter; ` ░ ▒ ▓ █` shading)",
                tick_to_year(frame.tick, period)
            ),
        );
        if !density.is_empty() {
            s.push_str(&density);
            let _ = writeln!(s);
        }
    }
}
