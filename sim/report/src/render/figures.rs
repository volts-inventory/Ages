//! Species canon (run-end relations) + memorable figures (top
//! discoverers, founders, longest-lived, most-charismatic etc.).
//! Both pull cross-civ data from the digest, so they don't fit in
//! `civ.rs` (per-civ chapter) — they're species-level summaries.

use super::civ::is_trivial_constant_discovery;
use super::{digest_period, pretty_form, tick_to_year};
use crate::digest::{Digest, DiscoveryRecord};
use crate::q32::q32_to_f64;
use std::collections::BTreeMap;
use std::fmt::Write;

pub(super) fn render_species_canon(s: &mut String, d: &Digest) {
    let period = digest_period(d);
    let _ = writeln!(s, "## Species canon");
    let _ = writeln!(s);

    // For each relation_id ever confirmed, find the latest discovery
    // (any civ, any figure). That's the species' most-recent
    // understanding of that relation at run-end.
    let mut latest: BTreeMap<u32, &DiscoveryRecord> = BTreeMap::new();
    for civ in d.civs.values() {
        for disc in &civ.discoveries {
            latest
                .entry(disc.relation_id)
                .and_modify(|cur| {
                    if disc.tick > cur.tick {
                        *cur = disc;
                    }
                })
                .or_insert(disc);
        }
    }
    if latest.is_empty() {
        let _ = writeln!(s, "_The species derived no relations during this run._");
        let _ = writeln!(s);
        return;
    }
    let (non_trivial, trivial): (Vec<&DiscoveryRecord>, Vec<&DiscoveryRecord>) = latest
        .values()
        .copied()
        .partition(|d| !is_trivial_constant_discovery(d));

    let _ = writeln!(
        s,
        "{} non-trivial relation{} the species pinned down at run-end (latest fit per relation across every civ); {} trivial `y \u{2248} 0` absences listed at the end.",
        non_trivial.len(),
        if non_trivial.len() == 1 { "" } else { "s" },
        trivial.len(),
    );
    let _ = writeln!(s);

    if !non_trivial.is_empty() {
        let _ = writeln!(s, "**Non-trivial findings:**");
        let _ = writeln!(s);
        let _ = writeln!(s, "| Relation | Latest form | Last fit | First seen |");
        let _ = writeln!(s, "|---|---|---|---|");
        let mut by_template: BTreeMap<String, Vec<&DiscoveryRecord>> = BTreeMap::new();
        for disc in &non_trivial {
            by_template
                .entry(disc.template_name.clone())
                .or_default()
                .push(disc);
        }
        let mut sorted_templates: Vec<_> = by_template.keys().cloned().collect();
        sorted_templates.sort();
        for tmpl in sorted_templates {
            let mut rows = by_template.remove(&tmpl).unwrap_or_default();
            rows.sort_by(|a, b| a.channel.cmp(&b.channel));
            for disc in rows {
                let first_tick = d
                    .civs
                    .values()
                    .flat_map(|c| c.discoveries.iter())
                    .filter(|x| x.relation_id == disc.relation_id)
                    .map(|x| x.tick)
                    .min()
                    .unwrap_or(disc.tick);
                let _ = writeln!(
                    s,
                    "| **{}** \u{2194} `{}` | `{}` → {} | year {} | year {} |",
                    disc.template_name,
                    disc.channel,
                    disc.form,
                    pretty_form(disc),
                    tick_to_year(disc.tick, period),
                    tick_to_year(first_tick, period),
                );
            }
        }
        let _ = writeln!(s);
    }

    if !trivial.is_empty() {
        let _ = writeln!(s, "<details><summary>Trivial constant fits ({} relations the species observed as never firing or independent of the channel)</summary>", trivial.len());
        let _ = writeln!(s);
        let mut sorted_trivial = trivial.clone();
        sorted_trivial.sort_by(|a, b| {
            a.template_name
                .cmp(&b.template_name)
                .then(a.channel.cmp(&b.channel))
        });
        for disc in sorted_trivial {
            let _ = writeln!(
                s,
                "- **{}** \u{2194} `{}` (`{}`)",
                disc.template_name, disc.channel, disc.form
            );
        }
        let _ = writeln!(s);
        let _ = writeln!(s, "</details>");
        let _ = writeln!(s);
    }
}

/// Memorable figures — cross-civ rankings. The species' most
/// prolific scientist, longest-serving figure, deepest refiner.
/// Per-civ chapters list every figure; this section pulls the
/// notable few up so the biography has named protagonists at the
/// species level rather than only at the civ level.
pub(super) fn render_memorable_figures(s: &mut String, d: &Digest) {
    let period = digest_period(d);
    let _ = writeln!(s, "## Memorable figures");
    let _ = writeln!(s);
    let figures: Vec<(u32, &crate::digest::FigureRecord, u32)> = d
        .civs
        .iter()
        .flat_map(|(civ_id, civ)| civ.figures.iter().map(move |f| (*civ_id, f, civ.civ_id)))
        .collect();
    if figures.is_empty() {
        let _ = writeln!(s, "_No named figures emerged during this run._");
        let _ = writeln!(s);
        return;
    }

    // Most-prolific: figure with the most confirmed relations.
    let mut by_discovery_count: Vec<(u32, &crate::digest::FigureRecord, u32, usize)> = figures
        .iter()
        .map(|(civ_id, fig, _)| {
            let n = d.civs.get(civ_id).map_or(0, |c| {
                c.discoveries
                    .iter()
                    .filter(|disc| disc.figure_id == fig.id)
                    .count()
            });
            (*civ_id, *fig, *civ_id, n)
        })
        .collect();
    by_discovery_count.sort_by(|a, b| b.3.cmp(&a.3));

    // Most-refined-by: figure who lodged the most refinements.
    let mut by_refinement: BTreeMap<u32, usize> = BTreeMap::new();
    for civ in d.civs.values() {
        for r in &civ.refinements {
            *by_refinement.entry(r.figure_id).or_default() += 1;
        }
    }
    let top_refiner = by_refinement.iter().max_by_key(|(_, n)| **n);

    let _ = writeln!(s, "**Most prolific scientists (by confirmed relations):**");
    for (civ_id, fig, _, n) in by_discovery_count.iter().take(5) {
        if *n == 0 {
            continue;
        }
        let _ = writeln!(
            s,
            "- *{}* (civ {}, born year {}) — {} confirmed relation{}; charisma {:.2}, curiosity {:.2}, doubt {:.2}, communicativeness {:.2}",
            fig.name,
            civ_id,
            tick_to_year(fig.tick, period),
            n,
            if *n == 1 { "" } else { "s" },
            q32_to_f64(fig.charisma_q32),
            q32_to_f64(fig.curiosity_q32),
            q32_to_f64(fig.doubt_q32),
            q32_to_f64(fig.communicativeness_q32),
        );
    }
    let _ = writeln!(s);

    if let Some((figure_id, n_refinements)) = top_refiner {
        if let Some((_, fig, civ_id)) = figures.iter().find(|(_, f, _)| f.id == *figure_id) {
            let _ = writeln!(
                s,
                "**Deepest refiner:** *{}* of civ {} — {} refinement event{}.",
                fig.name,
                civ_id,
                n_refinements,
                if *n_refinements == 1 { "" } else { "s" }
            );
            let _ = writeln!(s);
        }
    }

    // Trait-based superlatives. Surface the figure with the
    // highest of each personality scalar so the biography names
    // its most charismatic/curious/skeptical/communicative voices.
    let pick_top = |key: fn(&crate::digest::FigureRecord) -> i64| -> Option<(u32, &crate::digest::FigureRecord)> {
        figures.iter().max_by_key(|(_, f, _)| key(f)).map(|(c, f, _)| (*c, *f))
    };
    if let Some((civ_id, fig)) = pick_top(|f| f.charisma_q32) {
        let _ = writeln!(
            s,
            "**Most charismatic:** *{}* of civ {} (charisma {:.2}).",
            fig.name,
            civ_id,
            q32_to_f64(fig.charisma_q32)
        );
    }
    if let Some((civ_id, fig)) = pick_top(|f| f.doubt_q32) {
        let _ =
            writeln!(
            s,
            "**Boldest skeptic:** *{}* of civ {} (doubt {:.2}) — pushed harder for refinements.",
            fig.name, civ_id, q32_to_f64(fig.doubt_q32)
        );
    }
    if let Some((civ_id, fig)) = pick_top(|f| f.communicativeness_q32) {
        let _ = writeln!(
            s,
            "**Most communicative:** *{}* of civ {} (communicativeness {:.2}) — preserved more of the canon for successors.",
            fig.name, civ_id, q32_to_f64(fig.communicativeness_q32)
        );
    }
    let _ = writeln!(s);

    // Founder figures: figures present at civ founding (born_tick == civ.founded_tick).
    let founders: Vec<(u32, &crate::digest::FigureRecord)> = d
        .civs
        .values()
        .flat_map(|civ| {
            civ.figures
                .iter()
                .filter(move |f| f.tick == civ.founded_tick)
                .map(move |f| (civ.civ_id, f))
        })
        .collect();
    if !founders.is_empty() {
        let _ = writeln!(
            s,
            "**Founders ({}):** the figures present at each civ's founding moment.",
            founders.len()
        );
        for (civ_id, fig) in founders.iter().take(15) {
            let _ = writeln!(
                s,
                "- *{}* — civ {} (year {})",
                fig.name,
                civ_id,
                tick_to_year(fig.tick, period)
            );
        }
        if founders.len() > 15 {
            let _ = writeln!(s, "_(showing 15 of {})_", founders.len());
        }
        let _ = writeln!(s);
    }
}
