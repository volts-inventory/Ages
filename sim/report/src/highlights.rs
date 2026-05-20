//! "Interesting moments" highlight scoring. Hybrid: structural
//! pins always make the cut; everything else is scored by
//! `w_n·novelty + w_m·magnitude + w_f·figure-significance + w_a·arc-coherence`
//! and the top N pulled in.
//!
//! M6 starting weights — informal calibration on seed 42 / 5000 ticks.
//! Tune in subsequent runs as the rendered report's highlight reel
//! develops a feel.

use crate::digest::Digest;
use crate::q32::q32_to_f64;
use protocol::Event;
use std::collections::BTreeSet;

/// Default top-N% of scored events to surface. The ~5% recommendation
/// is calibrated for very long runs; for the M6 5000-tick
/// shakedown this would surface noise, so the renderer caps the
/// scored long-tail at `MAX_SCORED_HIGHLIGHTS` after taking the top
/// fraction.
pub const DEFAULT_TOP_FRACTION: f64 = 0.05;
pub const MAX_SCORED_HIGHLIGHTS: usize = 25;

/// One line in the highlight reel.
#[derive(Debug, Clone)]
pub struct Highlight {
    pub tick: u64,
    pub kind: HighlightKind,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HighlightKind {
    /// Structural pin — never filtered out.
    Pin,
    /// Scored long tail. Score breaks ties between pins
    /// at the same tick; serves no other purpose.
    Scored,
}

/// Score an event against the long-tail formula. Pins return `None`
/// (they're handled separately so they don't compete for slots).
fn score(ev: &Event, digest: &Digest) -> Option<f64> {
    match ev {
        Event::RelationConfirmed(r) => {
            // novelty: first confirmation of a (template, channel) pair
            // across all civs scores high; later civs re-discovering
            // the same relation score modestly. Cheap to compute by
            // checking the digest's relation_names map (built in
            // pass 1) — but every confirmed relation is in there, so
            // distinguish by checking earliest-tick-per-relation.
            let earliest = digest
                .events
                .iter()
                .filter_map(|e| match e {
                    Event::RelationConfirmed(rc) if rc.relation_id == r.relation_id => {
                        Some(rc.tick)
                    }
                    _ => None,
                })
                .min()
                .unwrap_or(r.tick);
            let novelty = if r.tick == earliest { 1.0 } else { 0.2 };
            // magnitude: confidence — high-confidence fits are more
            // meaningful than borderline ones.
            let magnitude = q32_to_f64(r.confidence_q32).clamp(0.0, 1.0);
            // figure-significance: per-figure discovery count (more
            // prolific figures get a small boost). Capped at 1.0.
            let f_sig = digest
                .civs
                .values()
                .flat_map(|c| c.discoveries.iter())
                .filter(|d| d.figure_id == r.figure_id)
                .count();
            let figure_sig = (f_sig as f64 / 10.0).min(1.0);
            // arc-coherence: discoveries early in a civ's life or
            // late (twilight) are more narratively interesting than
            // mid-life. Small effect.
            let civ_id = digest
                .civs
                .iter()
                .find(|(_, c)| c.figures.iter().any(|f| f.id == r.figure_id))
                .map(|(id, _)| *id);
            let arc = if let Some(id) = civ_id {
                let civ = &digest.civs[&id];
                let life = civ
                    .collapsed
                    .as_ref()
                    .map_or(digest.run_end.as_ref().map_or(r.tick, |e| e.tick), |c| {
                        c.tick
                    });
                let lived = life.saturating_sub(civ.founded_tick).max(1);
                let elapsed = r.tick.saturating_sub(civ.founded_tick);
                let normed = elapsed as f64 / lived as f64;
                // U-shape: 1.0 at the ends, ~0.3 in the middle.
                ((normed - 0.5).abs() * 2.0).max(0.3)
            } else {
                0.5
            };
            Some(0.4 * novelty + 0.3 * magnitude + 0.2 * figure_sig + 0.1 * arc)
        }
        Event::RefinementConfirmed(r) => {
            // A refinement that lands is always interesting — cheap
            // way to score: pin-adjacent at 0.85.
            let _ = r;
            Some(0.85)
        }
        Event::CosmologyShifted(c) => {
            // Magnitude proxy: dogmatism. High-dogmatism civs are the
            // ones whose drift mattered (cosmology hooks fire on
            // confidence suppression).
            let dog = q32_to_f64(c.dogmatism_q32).clamp(0.0, 1.0);
            Some(0.3 + 0.4 * dog)
        }
        Event::ConflictResolved(c) => {
            // Pinned only when defeat happens; pure stalemates score.
            let loss = q32_to_f64(c.loss_fraction_q32).clamp(0.0, 1.0);
            Some(0.5 + 0.3 * loss)
        }
        Event::WarDeclared(w) => {
            // War declarations are dramatic narrative beats —
            // score by belligerence (higher = more lopsided
            // motivation, more newsworthy).
            let bell = q32_to_f64(w.belligerence_q32).clamp(0.0, 1.0);
            Some(0.7 + 0.2 * bell)
        }
        Event::PeaceConcluded(p) => {
            // Peace is the war bookend; defeated > dropped >
            // territory_resolved in narrative weight.
            let bump = match p.reason {
                protocol::PeaceReason::Defeated => 0.15,
                protocol::PeaceReason::BelligerenceDropped => 0.05,
                protocol::PeaceReason::TerritoryResolved => 0.0,
            };
            Some(0.65 + bump)
        }
        // Pins handled separately, or excluded from the scored long
        // tail by category (run-frame events, recognition firings,
        // figure births, refinement proposals/rejections, intra-tick
        // diffusions). Bundled into one arm — clippy's
        // `match_same_arms` flags the split as redundant since the
        // body is identical.
        Event::CivFounded(_)
        | Event::CivTerritoryChanged(_)
        | Event::CivCollapsed(_)
        | Event::CatastropheFired(_)
        | Event::TechUnlocked(_)
        | Event::CivContact(_)
        | Event::KnowledgeTransmitted(_)
        | Event::Planet(_)
        | Event::PlanetMap(_)
        | Event::Species(_)
        | Event::RunStart(_)
        | Event::RunEnd { .. }
        | Event::Recognition(_)
        | Event::Tick(_)
        | Event::RefinementProposed(_)
        | Event::RefinementRejected(_)
        | Event::RelationFalsified(_)
        | Event::RelationRevalidated(_)
        | Event::RelationLapsed(_)
        | Event::KnowledgeDiffused(_)
        | Event::FigureBorn(_)
        | Event::MeasurementConfirmed(_)
        | Event::Snapshot(_)
        | Event::RunMetadata(_)
        | Event::SpeciesNomadsChanged(_)
        // Species cosmology-bias one-shot is a metadata
        // event; per-civ chapters already render the resulting
        // starting cosmology, so don't pin it in the highlight
        // reel.
        | Event::SpeciesCosmologyBias(_)
        // Drift snapshot is per-civ-founding metadata;
        // shown in chapters but not pinned to highlights.
        | Event::SpeciesDrift(_)
        // Cohesion shifts are per-civ slow drifts; pinning
        // every 0.05 jump would drown the highlight reel. The
        // narrator's interest is the civil-war collapse line that
        // CivCollapsed already pins.
        | Event::CohesionShifted(_)
        // Religion drift: per-civ schism beats are interesting
        // but currently surface aggregated through war / civ-collapse
        // pins; suppress individual events from the highlight reel
        // for now.
        | Event::ReligionShifted(_)
        // Life-expectancy shifts surface in the per-civ report
        // section's demographic-transition timeline; suppress from
        // the global highlight reel to avoid noise.
        | Event::CivLifeExpectancyChanged(_)
        // Surplus shifts are slow per-civ economic drifts; the
        // dramatic beats already surface via war / catastrophe /
        // collapse pins that reference the surplus state. Trade
        // routes likewise live in the per-civ chapter + a global
        // trade-routes section; per-event reel pins would be noise.
        | Event::CivSurplusChanged(_)
        | Event::TradeRouteEstablished(_)
        | Event::TradeRouteClosed(_)
        // Alliances surface in the per-civ chapter + the
        // viewport log line; per-event reel pins would push out
        // higher-signal beats (founding / collapse / catastrophe).
        | Event::AllianceFormed(_)
        | Event::AllianceDissolved(_)
        // Per-relation mythologization residue is too granular
        // to pin individually. Aggregate effect surfaces via the
        // CosmologyShifted events the cosmology drift naturally emits.
        | Event::RelationMythologized(_)
        // Rival-hypothesis events are scientific-canon
        // texture; the primary-hypothesis displacement is the
        // narrative beat (gets pinned via cosmology-shifted /
        // refinement events).
        | Event::RivalHypothesisProposed(_)
        | Event::PrimaryHypothesisDisplaced(_) => None,
        // Emergent template / tool births are headline-worthy —
        // genuinely-new species recognition or genuinely-invented
        // civ tools. Score 0.95 puts them alongside first tech
        // unlocks and conflict resolutions in the highlight reel.
        Event::TemplateDiscovered(_) | Event::ToolDiscovered(_) => Some(0.95),
    }
}

/// Build the highlight reel from a digest. Pins always included;
/// scored events filtered to `top_fraction` of all scored candidates
/// then capped at `MAX_SCORED_HIGHLIGHTS`. Returned in tick-ascending
/// order.
pub fn highlights(digest: &Digest) -> Vec<Highlight> {
    let mut out: Vec<Highlight> = Vec::new();
    let mut seen_pin_keys: BTreeSet<String> = BTreeSet::new();

    // First-of-kind discoveries: track relation_ids we've already
    // pinned a discovery for, so subsequent civs' re-discoveries
    // don't get pinned (they fall to the scored tail).
    let mut pinned_relations: BTreeSet<u32> = BTreeSet::new();
    // Tier crossings: one pin per civ per tier transition.
    let mut pinned_tiers: BTreeSet<(u32, u8)> = BTreeSet::new();
    // Inheritance pins collapsed by (source_civ, dest_civ, tick) so a
    // civ that inherits 30 relations at founding gets one summary
    // line ("inherited 30 relations from civ X") rather than 30
    // pins that drown the highlight reel.
    let mut inheritance_pinned: BTreeSet<(u32, u32, u64)> = BTreeSet::new();
    let inheritance_counts: std::collections::BTreeMap<(u32, u32, u64), usize> = digest
        .events
        .iter()
        .filter_map(|e| match e {
            Event::KnowledgeTransmitted(k) => Some((k.source_civ_id, k.dest_civ_id, k.tick)),
            _ => None,
        })
        .fold(std::collections::BTreeMap::new(), |mut acc, key| {
            *acc.entry(key).or_insert(0) += 1;
            acc
        });

    for ev in &digest.events {
        match ev {
            Event::CivFounded(c) => {
                let key = format!("founded:{}", c.civ_id);
                if seen_pin_keys.insert(key) {
                    let parent = c.parent_civ_id.map_or_else(
                        || "the species' inaugural".to_string(),
                        |p| format!("succeeding civ {p}"),
                    );
                    out.push(Highlight {
                        tick: c.tick,
                        kind: HighlightKind::Pin,
                        text: format!(
                            "Civ {} founded ({}, {} founding figures).",
                            c.civ_id, parent, c.founding_figure_count
                        ),
                    });
                }
            }
            Event::CivCollapsed(c) => {
                out.push(Highlight {
                    tick: c.tick,
                    kind: HighlightKind::Pin,
                    text: format!(
                        "Civ {} collapsed ({}, {} figures lost).",
                        c.civ_id, c.reason, c.final_figure_count
                    ),
                });
            }
            Event::CatastropheFired(c) => {
                let frac = q32_to_f64(c.fraction_lost_q32);
                out.push(Highlight {
                    tick: c.tick,
                    kind: HighlightKind::Pin,
                    text: format!(
                        "Catastrophe ({}) struck civ {} — population fell by {:.1}%.",
                        c.catastrophe_kind,
                        c.civ_id,
                        frac * 100.0
                    ),
                });
            }
            Event::TechUnlocked(t) => {
                if pinned_tiers.insert((t.civ_id, t.tier)) {
                    let extra = if t.newly_perceivable_template_ids.is_empty() {
                        String::new()
                    } else {
                        format!(
                            " Newly perceivable templates: {}.",
                            t.newly_perceivable_template_ids
                                .iter()
                                .map(u32::to_string)
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    };
                    out.push(Highlight {
                        tick: t.tick,
                        kind: HighlightKind::Pin,
                        text: format!(
                            "Civ {} unlocked tier-{} tool `{}`.{}",
                            t.civ_id, t.tier, t.tool_name, extra
                        ),
                    });
                }
            }
            Event::CivContact(c) => {
                out.push(Highlight {
                    tick: c.tick,
                    kind: HighlightKind::Pin,
                    text: format!("Civs {} and {} first co-existed.", c.civ_a, c.civ_b),
                });
            }
            Event::KnowledgeTransmitted(k) => {
                let key = (k.source_civ_id, k.dest_civ_id, k.tick);
                if !inheritance_pinned.insert(key) {
                    continue;
                }
                let count = inheritance_counts.get(&key).copied().unwrap_or(1);
                out.push(Highlight {
                    tick: k.tick,
                    kind: HighlightKind::Pin,
                    text: format!(
                        "Civ {} inherited {count} relation{} from civ {} at founding.",
                        k.dest_civ_id,
                        if count == 1 { "" } else { "s" },
                        k.source_civ_id,
                    ),
                });
            }
            Event::RelationConfirmed(r) => {
                // Skip degenerate "constant 0" first-of-kinds: when a
                // template never fires across the sample window the
                // pipeline confirms `y = 0` with confidence 1, but
                // these are absences, not discoveries — they drown
                // out the meaningful firsts at tick 0 where every
                // (template, channel) gets one.
                if is_trivial_constant(r) {
                    continue;
                }
                if pinned_relations.insert(r.relation_id) {
                    out.push(Highlight {
                        tick: r.tick,
                        kind: HighlightKind::Pin,
                        text: format!(
                            "First-of-kind: civ {}'s figure {} confirmed `{}` ↔ `{}` ({}).",
                            digest
                                .civs
                                .iter()
                                .find(|(_, c)| c.figures.iter().any(|f| f.id == r.figure_id))
                                .map_or(0, |(id, _)| *id),
                            figure_name(digest, r.figure_id),
                            r.template_name,
                            r.channel,
                            r.form,
                        ),
                    });
                }
            }
            _ => {}
        }
    }

    // Scored long-tail.
    let mut scored: Vec<(f64, &Event)> = digest
        .events
        .iter()
        .filter_map(|e| score(e, digest).map(|s| (s, e)))
        // Skip events already pinned (e.g. first-of-kind
        // RelationConfirmed) and degenerate constant=0 confirmations
        // that the pin path also filters out.
        .filter(|(_, e)| match e {
            Event::RelationConfirmed(r) => {
                !pinned_relations.contains(&r.relation_id) && !is_trivial_constant(r)
            }
            _ => true,
        })
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let n_total = scored.len();
    let take = ((n_total as f64) * DEFAULT_TOP_FRACTION).ceil() as usize;
    let take = take.min(MAX_SCORED_HIGHLIGHTS).min(n_total);

    for (s, ev) in scored.into_iter().take(take) {
        let _ = s;
        if let Some(line) = scored_line(ev, digest) {
            out.push(line);
        }
    }

    out.sort_by_key(|h| h.tick);
    out
}

fn scored_line(ev: &Event, digest: &Digest) -> Option<Highlight> {
    match ev {
        Event::RelationConfirmed(r) => Some(Highlight {
            tick: r.tick,
            kind: HighlightKind::Scored,
            text: format!(
                "Re-discovery: civ {}'s {} fitted `{}` ↔ `{}` again as {}.",
                digest
                    .civs
                    .iter()
                    .find(|(_, c)| c.figures.iter().any(|f| f.id == r.figure_id))
                    .map_or(0, |(id, _)| *id),
                figure_name(digest, r.figure_id),
                r.template_name,
                r.channel,
                r.form,
            ),
        }),
        Event::RefinementConfirmed(r) => {
            let label = digest.relation_names.get(&r.relation_id).map_or_else(
                || format!("relation {}", r.relation_id),
                |l| format!("`{}` ↔ `{}`", l.template_name, l.channel),
            );
            Some(Highlight {
                tick: r.tick,
                kind: HighlightKind::Scored,
                text: format!(
                    "{} refined {} from {} to {}.",
                    figure_name(digest, r.figure_id),
                    label,
                    r.old_form,
                    r.new_form,
                ),
            })
        }
        Event::CosmologyShifted(c) => Some(Highlight {
            tick: c.tick,
            kind: HighlightKind::Scored,
            text: format!(
                "Cosmology of civ {} drifted (dogmatism = {:.2}).",
                c.civ_id,
                q32_to_f64(c.dogmatism_q32),
            ),
        }),
        Event::ConflictResolved(c) => {
            let loss = q32_to_f64(c.loss_fraction_q32);
            Some(Highlight {
                tick: c.tick,
                kind: HighlightKind::Scored,
                text: format!(
                    "Civ {} {} civ {} over {} cells (loss {:.1}%).",
                    c.winner_civ_id,
                    if c.loser_defeated {
                        "defeated"
                    } else {
                        "skirmished with"
                    },
                    c.loser_civ_id,
                    c.disputed_cell_count,
                    loss * 100.0,
                ),
            })
        }
        _ => None,
    }
}

/// Trivial-fit filter: a fit is trivial when every y-coefficient
/// of its form is near zero — the relation reduces to "y is always
/// 0 in the sample window," an absence rather than a discovery.
/// Generalises beyond `constant` to catch `linear` / `polynomial_2`
/// / etc. fits that landed all-zero coefficients (the broader
/// recognition vocabulary makes these more common; the discovery
/// pipeline can pick a higher-arity form when residuals tie).
fn is_trivial_constant(r: &protocol::RelationConfirmed) -> bool {
    let y_coeff_indices: &[usize] = match r.form.as_str() {
        "constant" | "exp_decay" | "exp_growth" | "power_law" | "inverse_square" | "logistic" => {
            &[0]
        }
        // `threshold_step` shares the [0,1] pattern with `linear`
        // (a/b are y-coefficients; t at index 2 is the x-threshold).
        "linear" | "logarithmic" | "threshold_step" => &[0, 1],
        "polynomial_2" => &[0, 1, 2],
        "polynomial_3" => &[0, 1, 2, 3],
        "periodic_sine" => &[0, 3],
        _ => &[0],
    };
    if y_coeff_indices.is_empty() {
        return false;
    }
    y_coeff_indices
        .iter()
        .all(|&i| r.params_q32.get(i).is_some_and(|p| p.unsigned_abs() < 16))
}

fn figure_name(digest: &Digest, figure_id: u32) -> String {
    digest
        .civs
        .values()
        .flat_map(|c| c.figures.iter())
        .find(|f| f.id == figure_id)
        .map_or_else(|| format!("figure {figure_id}"), |f| f.name.clone())
}
