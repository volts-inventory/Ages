//! Markdown rendering for the post-run report. Templated prose where
//! prose helps readability — Mad-Libs-style fills from event data,
//! fully deterministic.
//!
//! Section order:
//!   1. Run header
//!   2. Planet card
//!   3. Species card
//!   4. Run summary (counts, recognition coverage)
//!   5. Per-civ chapters (founding, key discoveries, refinements,
//!      figures, tech ladder, catastrophes, cosmology, collapse)
//!   6. Inter-civ knowledge transmission table
//!   7. Inter-civ contacts / conflicts / diffusions table
//!   8. Highlight reel
//!   9. Run-end footer

use crate::ages::derive_ages;
use crate::digest::{Digest, DiscoveryRecord, TransferRecord};
use crate::highlights::{highlights, HighlightKind};
use crate::q32::q32_to_f64;
use protocol::Event;
use std::fmt::Write;

mod civ;
mod figures;
mod planet;
mod timelines;

use civ::render_civ_chapter;
use figures::{render_memorable_figures, render_species_canon};
pub use planet::{surface_phase_at, SurfacePhase};
pub(crate) use planet::{render_planet, surface_phase_for_digest, terrain_symbol};
use timelines::{render_migration_patterns, render_population_timeline, render_world_keyframes};

/// Convert a raw tick (= month) into the year-of-tick
/// for user-facing display. Year length is the planet's
/// `orbital_period_months` (sampled per planet, range 8..=16). The
/// report frames time in years; the internal protocol keeps ticks
/// (months) so per-month events carry sub-year resolution.
#[must_use]
fn tick_to_year(tick: u64, period: u32) -> u64 {
    protocol::year_of_tick_for_period(tick, period)
}

/// Month-of-year for a raw tick. Range is `0..period`
/// where `period = planet.orbital_period_months`.
#[must_use]
fn tick_to_month(tick: u64, period: u32) -> u64 {
    protocol::month_of_tick_for_period(tick, period)
}

/// Pull the orbital period off a Digest. Falls back to the
/// baseline 12-month calibration if no Planet event arrived.
#[must_use]
fn digest_period(d: &crate::Digest) -> u32 {
    d.planet
        .as_ref()
        .map_or(protocol::BASELINE_MONTHS_PER_YEAR as u32, |p| {
            p.orbital_period_months
        })
}

pub(super) const COSMOLOGY_AXES: [&str; 5] = [
    "empirical",
    "communitarian",
    "reformist",
    "mystical",
    "hierarchical",
];

/// Render the digest as a markdown report. Always succeeds — missing
/// sections (e.g. no Planet event in the log) render with a single
/// "(not emitted)" line so the structure stays consistent across runs.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn markdown(d: &Digest) -> String {
    let mut s = String::new();
    let period = digest_period(d);

    let _ = writeln!(s, "# Ages — post-run report");
    let _ = writeln!(s);
    let _ = writeln!(s, "- **Seed:** `{}`", d.seed);
    let _ = writeln!(s, "- **Schema version:** {}", d.schema_version);
    let _ = writeln!(s, "- **Sim version:** `{}`", d.ages_version);
    if let Some(end) = &d.run_end {
        // 1 sim tick = 1 species-month; user-facing report
        // measures time in *planet-years* (= tick / orbital_period_months).
        let _ = writeln!(
            s,
            "- **Ran for:** {} years ({})",
            tick_to_year(end.tick, period),
            end.reason
        );
    }
    let _ = writeln!(s);

    render_planet(&mut s, d);
    render_species(&mut s, d);
    render_archetype(&mut s, d);
    render_ages(&mut s, d);
    render_summary(&mut s, d);
    render_memorable_figures(&mut s, d);
    render_species_canon(&mut s, d);

    let _ = writeln!(s, "## Civilizations");
    let _ = writeln!(s);
    if d.founding_order.is_empty() {
        let _ = writeln!(s, "_No civs founded in this run._");
        let _ = writeln!(s);
    } else {
        for civ_id in &d.founding_order {
            if let Some(civ) = d.civs.get(civ_id) {
                render_civ_chapter(&mut s, civ, d);
            }
        }
    }

    render_transmissions(&mut s, d);
    render_contacts_and_conflicts(&mut s, d);
    render_diffusions(&mut s, d);
    render_trade_routes(&mut s, d);
    render_population_timeline(&mut s, d);
    render_migration_patterns(&mut s, d);
    render_world_keyframes(&mut s, d);
    render_highlights(&mut s, d);

    s
}

fn render_archetype(s: &mut String, d: &Digest) {
    let Some(a) = d.events.iter().find_map(|e| match e {
        Event::ArchetypeDerived(a) => Some(a),
        _ => None,
    }) else {
        return;
    };
    let _ = writeln!(s, "## Developmental archetype");
    let _ = writeln!(s);
    let _ = writeln!(s, "- **Archetype:** {}", a.label);
    let _ = writeln!(s, "- **Dominant lever:** {}", a.dominant_lever);
    let _ = writeln!(s, "- **Secondary lever:** {}", a.secondary_lever);
    let _ = writeln!(s, "- **Cognition:** {}", a.cognition_mode);
    let _ = writeln!(s);

    // Lever signature, strongest first — the peer-scored dimensions
    // this world+species lean on.
    let mut scored: Vec<(&String, f64)> = a
        .lever_names
        .iter()
        .zip(a.lever_scores_q32.iter())
        .map(|(n, &q)| (n, q32_to_f64(q)))
        .collect();
    scored.sort_by(|x, y| y.1.partial_cmp(&x.1).unwrap_or(std::cmp::Ordering::Equal));
    let _ = writeln!(s, "Lever signature (strongest first):");
    let _ = writeln!(s);
    for (name, score) in scored.iter().take(5) {
        let _ = writeln!(s, "- {name}: {score:.2}");
    }
    let _ = writeln!(s);

    if let Some(e) = d.events.iter().find_map(|e| match e {
        Event::ArchetypeEndpoint(ep) => Some(ep),
        _ => None,
    }) {
        let _ = writeln!(s, "**Endpoint reached** ({}): {}", e.endpoint_mode, e.description);
        let _ = writeln!(s);
    }
}

fn render_species(s: &mut String, d: &Digest) {
    let _ = writeln!(s, "## Species");
    let _ = writeln!(s);
    if let Some(sp) = &d.species {
        let _ = writeln!(s, "| Trait | Value |");
        let _ = writeln!(s, "|---|---|");
        let _ = writeln!(s, "| Cognition | {:.3} |", q32_to_f64(sp.cognition_q32));
        let _ = writeln!(s, "| Sociality | {:.3} |", q32_to_f64(sp.sociality_q32));
        let _ = writeln!(
            s,
            "| Communication fidelity | {:.3} |",
            q32_to_f64(sp.communication_fidelity_q32)
        );
        let _ = writeln!(
            s,
            "| Lifespan | {:.0} years |",
            q32_to_f64(sp.lifespan_years_q32)
        );
        let _ = writeln!(s, "| t0 loss | {:.3} |", q32_to_f64(sp.t0_loss_q32));
        let _ = writeln!(s, "| Cognition topology | `{}` |", sp.cognition_topology);
        let _ = writeln!(s);
        let _ = writeln!(
            s,
            "**Modalities:** {}",
            list_or_none(
                &sp.modalities
                    .iter()
                    .map(|m| format!("`{m}`"))
                    .collect::<Vec<_>>()
            )
        );
        let _ = writeln!(
            s,
            "**Manipulation modes:** {}",
            list_or_none(
                &sp.manipulation_modes
                    .iter()
                    .map(|m| format!("`{m}`"))
                    .collect::<Vec<_>>()
            )
        );
        let _ = writeln!(
            s,
            "**Native perceivable templates:** {} ({})",
            sp.perceivable_template_ids.len(),
            sp.perceivable_template_ids
                .iter()
                .map(|id| d
                    .template_names
                    .get(id)
                    .cloned()
                    .map_or_else(|| format!("template_{id}"), |n| format!("`{n}`")))
                .collect::<Vec<_>>()
                .join(", "),
        );
    } else {
        let _ = writeln!(s, "_(species event not emitted)_");
    }
    let _ = writeln!(s);
}

/// Species "Ages" timeline. The project is named `Ages`; this is
/// the section that earns the name. Each row is one emergent era —
/// triggered by a concrete event, not authored thresholds. On a
/// short run only Foundational lands; on a rich run all six.
fn render_ages(s: &mut String, d: &Digest) {
    let _ = writeln!(s, "## Ages of the species");
    let _ = writeln!(s);
    let ages = derive_ages(d);
    if ages.len() == 1 {
        let _ = writeln!(
            s,
            "_The species lived through a single age — {} — and didn't reach later milestones._",
            ages[0].name
        );
        let _ = writeln!(s);
        return;
    }
    let period = digest_period(d);
    let run_end_year = d
        .run_end
        .as_ref()
        .map_or(0, |e| tick_to_year(e.tick, period));
    for (i, age) in ages.iter().enumerate() {
        let end = age.ended_year.unwrap_or(run_end_year);
        let span = if i + 1 == ages.len() {
            format!("year {}–{} (ongoing at run-end)", age.started_year, end)
        } else {
            format!("year {}–{}", age.started_year, end)
        };
        let _ = writeln!(s, "- **{}** ({}) — {}.", age.name, span, age.trigger_text);
    }
    let _ = writeln!(s);
}

fn render_summary(s: &mut String, d: &Digest) {
    let _ = writeln!(s, "## Run summary");
    let _ = writeln!(s);
    let _ = writeln!(s, "- **Civs founded:** {}", d.founding_order.len());
    let _ = writeln!(s, "- **Civs collapsed:** {}", d.collapsed_civ_count());
    let _ = writeln!(s, "- **Named figures:** {}", d.figure_count());
    let _ = writeln!(s, "- **Confirmed relations:** {}", d.discovery_count());
    let _ = writeln!(s, "- **Refinement events:** {}", d.refinement_count());
    let _ = writeln!(
        s,
        "- **Catastrophes:** {}",
        d.civs.values().map(|c| c.catastrophes.len()).sum::<usize>()
    );
    let _ = writeln!(
        s,
        "- **Tech unlocks:** {}",
        d.civs.values().map(|c| c.techs.len()).sum::<usize>()
    );
    let _ = writeln!(
        s,
        "- **Cosmology shifts:** {}",
        d.civs
            .values()
            .map(|c| c.cosmology_shifts.len())
            .sum::<usize>()
    );
    let _ = writeln!(
        s,
        "- **Religion shifts:** {}",
        d.civs
            .values()
            .map(|c| c.religion_shifts.len())
            .sum::<usize>()
    );
    let _ = writeln!(
        s,
        "- **Knowledge transmissions (across collapse):** {}",
        d.transmissions.len()
    );
    let _ = writeln!(
        s,
        "- **Knowledge diffusions (concurrent):** {}",
        d.diffusions.len()
    );
    let _ = writeln!(s, "- **Inter-civ contacts:** {}", d.contacts.len());
    let _ = writeln!(s, "- **Trade routes opened:** {}", d.trade_routes.len());
    let _ = writeln!(s, "- **Conflicts resolved:** {}", d.conflicts.len());
    let _ = writeln!(
        s,
        "- **Recognition firings (templates × times):** {} firings across {} templates",
        d.firing_counts.values().sum::<u64>(),
        d.firing_counts.len(),
    );
    let _ = writeln!(s);
}

fn render_transmissions(s: &mut String, d: &Digest) {
    let _ = writeln!(s, "## Inter-civ knowledge transmission");
    let _ = writeln!(s);
    if d.transmissions.is_empty() {
        let _ = writeln!(s, "_No knowledge crossed a collapse boundary._");
    } else {
        let _ = writeln!(
            s,
            "{} relation{} comprehended by successor civs from their predecessors.",
            d.transmissions.len(),
            if d.transmissions.len() == 1 { "" } else { "s" }
        );
        let _ = writeln!(s);
        let _ = writeln!(
            s,
            "| Tick | Source civ | → Dest civ | Relation | Comprehension |"
        );
        let _ = writeln!(s, "|---|---|---|---|---|");
        for t in d.transmissions.iter().take(20) {
            let label = d.relation_names.get(&t.relation_id).map_or_else(
                || format!("relation {}", t.relation_id),
                |l| format!("`{}` ↔ `{}`", l.template_name, l.channel),
            );
            let _ = writeln!(
                s,
                "| {} | {} | {} | {} | {:.3} |",
                t.tick,
                t.source_civ_id,
                t.dest_civ_id,
                label,
                q32_to_f64(t.comprehension_q32)
            );
        }
        if d.transmissions.len() > 20 {
            let _ = writeln!(
                s,
                "_(showing 20 of {} — see event log for the rest)_",
                d.transmissions.len()
            );
        }
    }
    let _ = writeln!(s);
}

fn render_diffusions(s: &mut String, d: &Digest) {
    let _ = writeln!(s, "## Concurrent-civ knowledge diffusion");
    let _ = writeln!(s);
    if d.diffusions.is_empty() {
        let _ = writeln!(s, "_No diffusion between concurrent civs._");
    } else {
        let summary = summarise_transfers(&d.diffusions);
        let _ = writeln!(
            s,
            "{} relation transfer{} between concurrent peaceful civs ({}).",
            d.diffusions.len(),
            if d.diffusions.len() == 1 { "" } else { "s" },
            summary
        );
    }
    let _ = writeln!(s);
}

/// M8 — trade-route lifecycle table. One row per opened route;
/// closed routes show their end year + reason, still-open routes
/// read as `(open at year X, ongoing)`.
fn render_trade_routes(s: &mut String, d: &Digest) {
    let period = digest_period(d);
    let _ = writeln!(s, "## Trade routes");
    let _ = writeln!(s);
    if d.trade_routes.is_empty() {
        let _ = writeln!(s, "_No civs opened a trade route this run._");
        let _ = writeln!(s);
        return;
    }
    let total = d.trade_routes.len();
    let open_now = d
        .trade_routes
        .iter()
        .filter(|r| r.end_tick.is_none())
        .count();
    let closed_by_war = d
        .trade_routes
        .iter()
        .filter(|r| r.close_reason.as_deref() == Some("war_declared"))
        .count();
    let closed_by_collapse = d
        .trade_routes
        .iter()
        .filter(|r| r.close_reason.as_deref() == Some("civ_collapsed"))
        .count();
    let _ = writeln!(
        s,
        "{} trade route{} opened across the run — {} still open at sim-end; \
         {} closed by war, {} by civ collapse.",
        total,
        if total == 1 { "" } else { "s" },
        open_now,
        closed_by_war,
        closed_by_collapse,
    );
    let _ = writeln!(s);
    let _ = writeln!(s, "| Opened | Closed | Pair | Outcome |");
    let _ = writeln!(s, "|---|---|---|---|");
    for r in d.trade_routes.iter().take(30) {
        let opened = format!("year {}", tick_to_year(r.start_tick, period));
        let (closed, outcome) = match (r.end_tick, r.close_reason.as_deref()) {
            (Some(t), Some(reason)) => (
                format!("year {}", tick_to_year(t, period)),
                reason.replace('_', " "),
            ),
            (Some(t), None) => (
                format!("year {}", tick_to_year(t, period)),
                "closed".to_string(),
            ),
            (None, _) => ("—".to_string(), "ongoing".to_string()),
        };
        let _ = writeln!(
            s,
            "| {} | {} | civ {} ↔ civ {} | {} |",
            opened, closed, r.civ_a, r.civ_b, outcome
        );
    }
    if d.trade_routes.len() > 30 {
        let _ = writeln!(
            s,
            "_(showing 30 of {} — see event log for the rest)_",
            d.trade_routes.len()
        );
    }
    let _ = writeln!(s);
}

fn render_contacts_and_conflicts(s: &mut String, d: &Digest) {
    let period = digest_period(d);
    let _ = writeln!(s, "## Inter-civ contact and conflict");
    let _ = writeln!(s);
    if d.contacts.is_empty() {
        let _ = writeln!(s, "_No civs co-existed long enough to make contact._");
    } else {
        let _ = writeln!(s, "**First-contact pairs ({}):**", d.contacts.len());
        for c in d.contacts.iter().take(20) {
            let _ = writeln!(
                s,
                "- year {}: civ {} ↔ civ {}",
                tick_to_year(c.tick, period),
                c.civ_a,
                c.civ_b
            );
        }
    }
    let _ = writeln!(s);
    // Q-war: prefer the explicit `WarDeclared`/`PeaceConcluded`
    // brackets in `d.wars` when they exist (any run produced after
    // the belligerence model shipped). Fall back to the heuristic
    // "campaign" grouper over `ConflictResolved` for older logs that
    // pre-date the brackets, so historical events still render.
    if !d.wars.is_empty() {
        let total_skirmishes = d.conflicts.len();
        let _ = writeln!(
            s,
            "**Wars ({}, {} skirmish events total):**",
            d.wars.len(),
            total_skirmishes
        );
        for war in d.wars.iter().take(30) {
            let span = match war.end_tick {
                Some(end) if end > war.start_tick => format!(
                    "year {}–{}",
                    tick_to_year(war.start_tick, period),
                    tick_to_year(end, period)
                ),
                Some(_) => format!("year {}", tick_to_year(war.start_tick, period)),
                None => format!(
                    "from year {} (unresolved)",
                    tick_to_year(war.start_tick, period)
                ),
            };
            let outcome = match war.peace_reason.as_deref() {
                Some("defeated") => format!(
                    "civ {} **defeated** civ {}",
                    war.aggressor_civ_id, war.defender_civ_id
                ),
                Some("belligerence_dropped") => {
                    format!("civ {} ↔ civ {} (tensions eased)", war.civ_a, war.civ_b)
                }
                Some("territory_resolved") => {
                    format!("civ {} ↔ civ {} (borders settled)", war.civ_a, war.civ_b)
                }
                _ => format!(
                    "civ {} → civ {} (ongoing)",
                    war.aggressor_civ_id, war.defender_civ_id
                ),
            };
            let bell = crate::q32::q32_to_f64(war.start_belligerence_q32);
            let drive = crate::q32::q32_to_f64(war.start_drive_q32);
            let kin = crate::q32::q32_to_f64(war.start_kinship_q32);
            let _ = writeln!(
                s,
                "- {span}: {outcome} (belligerence {bell:.2} = drive {drive:.2} × kin {kin:.2})"
            );
        }
        if d.wars.len() > 30 {
            let _ = writeln!(s, "_(showing 30 of {} wars)_", d.wars.len());
        }
    } else if d.conflicts.is_empty() {
        let _ = writeln!(s, "_No conflicts resolved._");
    } else {
        // Legacy path: pre-Q-war logs without WarDeclared/
        // PeaceConcluded brackets. Use the heuristic campaign
        // grouper over `ConflictResolved` events.
        let campaigns = group_conflicts_into_campaigns(&d.conflicts, period);
        let _ = writeln!(
            s,
            "**Wars ({} campaigns, {} skirmish events total):**",
            campaigns.len(),
            d.conflicts.len()
        );
        for camp in campaigns.iter().take(30) {
            let outcome = if camp.ended_with_defeat {
                format!("civ {} **defeated** civ {}", camp.winner_id, camp.loser_id)
            } else {
                format!(
                    "civ {} vs civ {} (ongoing or stalemate)",
                    camp.pair_a, camp.pair_b
                )
            };
            let span = if camp.start_year == camp.end_year {
                format!("year {}", camp.start_year)
            } else {
                format!("year {}–{}", camp.start_year, camp.end_year)
            };
            let _ = writeln!(
                s,
                "- {}: {} ({} skirmish event{}, peak loss {:.1}%)",
                span,
                outcome,
                camp.skirmish_count,
                if camp.skirmish_count == 1 { "" } else { "s" },
                camp.peak_loss_pct,
            );
        }
        if campaigns.len() > 30 {
            let _ = writeln!(s, "_(showing 30 of {} campaigns)_", campaigns.len());
        }
    }
    let _ = writeln!(s);
}

/// One war campaign — a sequence of `ConflictResolved` events
/// between the same pair (regardless of which side was winner
/// each event).
struct Campaign {
    pair_a: u32,
    pair_b: u32,
    winner_id: u32,
    loser_id: u32,
    start_year: u64,
    end_year: u64,
    skirmish_count: u32,
    peak_loss_pct: f64,
    ended_with_defeat: bool,
}

/// Group `ConflictResolved` events into "campaigns".
/// A campaign is a contiguous sequence of skirmishes between the
/// same pair (orderless: civ 1 vs civ 2 == civ 2 vs civ 1). A
/// `loser_defeated = true` event ends the campaign; otherwise it
/// stays open until a different pair or run-end.
fn group_conflicts_into_campaigns(
    conflicts: &[crate::digest::ConflictRecord],
    period: u32,
) -> Vec<Campaign> {
    use std::collections::HashMap;
    let mut active: HashMap<(u32, u32), Campaign> = HashMap::new();
    let mut closed: Vec<Campaign> = Vec::new();
    for c in conflicts {
        let key = if c.winner_civ_id < c.loser_civ_id {
            (c.winner_civ_id, c.loser_civ_id)
        } else {
            (c.loser_civ_id, c.winner_civ_id)
        };
        let year = tick_to_year(c.tick, period);
        let loss_pct = q32_to_f64(c.loss_fraction_q32) * 100.0;
        let entry = active.entry(key).or_insert(Campaign {
            pair_a: key.0,
            pair_b: key.1,
            winner_id: c.winner_civ_id,
            loser_id: c.loser_civ_id,
            start_year: year,
            end_year: year,
            skirmish_count: 0,
            peak_loss_pct: 0.0,
            ended_with_defeat: false,
        });
        entry.end_year = year;
        entry.winner_id = c.winner_civ_id;
        entry.loser_id = c.loser_civ_id;
        entry.skirmish_count += 1;
        if loss_pct > entry.peak_loss_pct {
            entry.peak_loss_pct = loss_pct;
        }
        if c.loser_defeated {
            entry.ended_with_defeat = true;
            if let Some(camp) = active.remove(&key) {
                closed.push(camp);
            }
        }
    }
    for (_, camp) in active {
        closed.push(camp);
    }
    closed.sort_by_key(|c| c.start_year);
    closed
}

fn render_highlights(s: &mut String, d: &Digest) {
    let period = digest_period(d);
    let _ = writeln!(s, "## Highlights");
    let _ = writeln!(s);
    let h = highlights(d);
    if h.is_empty() {
        let _ = writeln!(s, "_No events qualified for the highlight reel._");
    } else {
        for hl in &h {
            let marker = match hl.kind {
                HighlightKind::Pin => "**•**",
                HighlightKind::Scored => "·",
            };
            let _ = writeln!(
                s,
                "- {} year {}: {}",
                marker,
                tick_to_year(hl.tick, period),
                hl.text
            );
        }
    }
    let _ = writeln!(s);
}

fn summarise_transfers(transfers: &[TransferRecord]) -> String {
    if transfers.is_empty() {
        return "no comprehension".into();
    }
    let mean = transfers
        .iter()
        .map(|t| q32_to_f64(t.comprehension_q32))
        .sum::<f64>()
        / transfers.len() as f64;
    format!("mean comprehension {mean:.3}")
}

fn list_or_none(items: &[String]) -> String {
    if items.is_empty() {
        "_none_".into()
    } else {
        items.join(", ")
    }
}

/// Pretty-print a fitted form's equation with its parameters
/// substituted in. Falls back to the raw param list for forms not
/// yet enumerated here.
fn pretty_form(d: &DiscoveryRecord) -> String {
    let p: Vec<f64> = d.params_q32.iter().map(|q| q32_to_f64(*q)).collect();
    match d.form.as_str() {
        "constant" if p.len() == 1 => format!("y = {:.3}", p[0]),
        "linear" if p.len() == 2 => format!("y = {:.3}·x + {:.3}", p[0], p[1]),
        "logarithmic" if p.len() == 2 => format!("y = {:.3}·ln(x) + {:.3}", p[0], p[1]),
        "inverse_square" if p.len() == 1 => format!("y = {:.3} / x²", p[0]),
        "exp_decay" if p.len() == 2 => format!("y = {:.3}·exp(-{:.3}·x)", p[0], p[1]),
        "exp_growth" if p.len() == 2 => format!("y = {:.3}·exp({:.3}·x)", p[0], p[1]),
        "power_law" if p.len() == 2 => format!("y = {:.3}·x^{:.3}", p[0], p[1]),
        "polynomial_2" if p.len() == 3 => {
            format!("y = {:.3}·x² + {:.3}·x + {:.3}", p[0], p[1], p[2])
        }
        "polynomial_3" if p.len() == 4 => format!(
            "y = {:.3}·x³ + {:.3}·x² + {:.3}·x + {:.3}",
            p[0], p[1], p[2], p[3]
        ),
        "logistic" if p.len() == 3 => format!(
            "y = {:.3} / (1 + exp(-{:.3}·(x - {:.3})))",
            p[0], p[1], p[2]
        ),
        "threshold_step" if p.len() == 3 => {
            format!("y = {{ {:.3} if x < {:.3} else {:.3} }}", p[0], p[2], p[1])
        }
        "periodic_sine" if p.len() == 4 => format!(
            "y = {:.3}·sin({:.3}·x + {:.3}) + {:.3}",
            p[0], p[1], p[2], p[3]
        ),
        _ => format!("params {p:?}"),
    }
}
