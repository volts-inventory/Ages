//! Inter-civ knowledge transmission. When a successor civ
//! founds after a collapse, a fraction of the predecessor's
//! confirmed relations transmit into the successor's knowledge —
//! gated by linguistic distance (`NameGrammar` atom overlap), age
//! decay, a flat tier factor, the parent's communicativeness
//! ceiling, and the parent's settlement-scale persistence
//! multiplier. The cultural-distance term still defaults to 1.0
//! until cultural distance is wired in.

use crate::cosmology;
use crate::discovery::ConfirmedRelation;
use crate::figures::NameGrammar;
use crate::Civ;
use sim_arith::transcendental::exp;
use sim_arith::Real;
use std::collections::BTreeMap;

/// Placeholders under tuning. Threshold tuned
/// empirically: at the M4-minimum atom-pool sizes, same-strategy
/// civs typically reach `comprehension ≈ 0.2–0.4` (Jaccard overlap
/// of ~0.3–0.6 of randomly-sampled atom subsets, × tier 0.7).
/// `0.15` lets most same-strategy successors recover a useful
/// fraction; cross-strategy stays at 0 because `linguistic_distance
/// = 1` zeros the term.
/// scaled ×12 (year-meant: e-fold over 1000 years).
pub const DECAY_CONSTANT_TICKS: u64 = 1000 * protocol::MONTHS_PER_YEAR;
pub const TRANSMIT_THRESHOLD: (i64, i64) = (15, 100);

/// mythologization floor. Transmissions whose comprehension
/// score falls in `(MYTH_FLOOR, TRANSMIT_THRESHOLD]` don't carry
/// the relation as confirmed knowledge but they don't disappear
/// either — they survive as a *cultural fingerprint*, perturbing
/// the successor civ's cosmology along one of the five axes. Real
/// historical analogue: a society that lost the original physics
/// of a phenomenon may retain the *meaning* (taboo, ritual, sacred
/// reverence) without the form. Below this floor the transmission
/// is too garbled to leave any trace.
pub const MYTH_FLOOR: (i64, i64) = (3, 100);

/// Mythologization magnitude. The cosmology perturbation a
/// single mythologized transmission applies is `MYTH_PUSH_BASE ×
/// (1 - score)` — the more lost the transmission, the more its
/// residue distorts the receiving civ's worldview. Pinned small so
/// no single mythologization swings the cosmology dramatically;
/// many mythologized transmissions in a chain do.
pub const MYTH_PUSH_BASE: (i64, i64) = (5, 100);
pub const TIER_FACTOR: (i64, i64) = (7, 10);

/// Linguistic distance between two civs' name grammars. Atom
/// set Jaccard distance: `1 − |intersection| / |union|`. Same
/// strategy + same atoms → 0; disjoint atom sets → 1.
pub fn linguistic_distance(a: &NameGrammar, b: &NameGrammar) -> Real {
    let union_size: usize = {
        let mut s: std::collections::BTreeSet<&str> = a.atoms.iter().copied().collect();
        for atom in &b.atoms {
            s.insert(atom);
        }
        s.len()
    };
    if union_size == 0 {
        return Real::ONE;
    }
    let intersection_size: usize = a.atoms.iter().filter(|atom| b.atoms.contains(atom)).count();
    let intersection = Real::from_int(i64::try_from(intersection_size).unwrap_or(i64::MAX));
    let union = Real::from_int(i64::try_from(union_size).unwrap_or(i64::MAX));
    Real::ONE - (intersection / union)
}

/// Age decay: `exp(-age_ticks / decay_ticks)`. Approaches 1 for
/// young artifacts, 0 for very old ones. : `decay_ticks` is
/// per-species (was the global `DECAY_CONSTANT_TICKS`); long-lived
/// social species preserve oral tradition longer.
pub fn age_decay(age_ticks: u64, decay_ticks: u64) -> Real {
    let age = Real::from_int(i64::try_from(age_ticks).unwrap_or(i64::MAX));
    let decay = Real::from_int(i64::try_from(decay_ticks).unwrap_or(i64::MAX));
    if decay <= Real::ZERO {
        return Real::ZERO;
    }
    exp(-(age / decay))
}

/// Combined comprehension score. `decay_ticks` is
/// per-species (see `Species::transmission_decay_ticks`).
pub fn comprehension(linguistic_dist: Real, age_ticks: u64, decay_ticks: u64) -> Real {
    let ling = (Real::ONE - linguistic_dist).max(Real::ZERO);
    let age = age_decay(age_ticks, decay_ticks);
    let tier = Real::from_ratio(TIER_FACTOR.0, TIER_FACTOR.1);
    // Cultural-distance term defaults to 0 in v1 (factor = 1).
    let cult = Real::ONE;
    ling * age * tier * cult
}

/// One transmitted relation: the predecessor's `ConfirmedRelation`
/// plus the comprehension score that brought it across.
#[derive(Debug, Clone)]
pub struct TransmissionRecord {
    pub relation: ConfirmedRelation,
    pub comprehension: Real,
}

/// mythologization record. A relation that *almost* crossed
/// the comprehension gate but didn't quite leaves a residual mark
/// on the successor civ's cosmology rather than being lost
/// outright. The receiving civ's cosmology axis shifts by
/// `magnitude` (always positive, applied to the chosen axis).
#[derive(Debug, Clone)]
pub struct MythologizationRecord {
    /// Predecessor civ's relation that got mythologized.
    pub relation_id: u32,
    /// Cosmology axis index (0=empirical, 1=communitarian,
    /// 2=reformist, 3=mystical, 4=hierarchical).
    pub axis: u8,
    /// Magnitude of the cosmology perturbation, in `[0, 0.05]`.
    pub magnitude: Real,
    /// Comprehension score that fell in the myth band — for the
    /// post-run report's narrative ("relation X comprehended at
    /// 0.08 — mythologized rather than lost").
    pub comprehension: Real,
}

/// pick a cosmology axis for a relation that's about to be
/// mythologized. Deterministic hash on `(template_id, relation_id,
/// source_civ_id)` so byte-replay holds. Returns 0..=4 indexing
/// `(empirical, communitarian, reformist, mystical, hierarchical)`.
///
/// Heuristic intent: relations about thresholds and step-changes
/// — the rest is just a deterministic 5-way fold so the residue
/// is consistent per (relation, source-civ) pair. Mystical bias
/// (axis 3) is the modal default for unrecognised relation
/// signatures since lost-meaning transmissions historically
/// become sacred / ineffable.
#[must_use]
pub fn mythologization_axis(template_id: u32, relation_id: u32, source_civ_id: u32) -> u8 {
    // 60% mystical bias; 40% spread across the other four axes
    // via a deterministic mixing function. Replay-stable.
    let mix = template_id
        .wrapping_mul(0x9E37_79B9)
        .wrapping_add(relation_id.wrapping_mul(0xBF58_476D))
        .wrapping_add(source_civ_id.wrapping_mul(0x94D0_49BB));
    let bucket = mix % 10;
    match bucket {
        0..=5 => 3, // mystical
        6 => 0,     // empirical
        7 => 1,     // communitarian
        8 => 2,     // reformist
        _ => 4,     // hierarchical
    }
}

/// cross-civ peaceful diffusion. When two concurrent civs are
/// peaceful (`conflict::is_peaceful_pair`), each tick they
/// exchange a fraction of their confirmed relations — same
/// comprehension formula but without age decay (direct contact,
/// not artifacts in transit). Returns the records that crossed
/// from `source` into `dest` so the caller can emit events.
/// The `dest`'s first figure receives the relations.
pub fn diffuse_between(
    source: &Civ,
    dest: &mut Civ,
    transmitted_at_tick: u64,
) -> Vec<TransmissionRecord> {
    if dest.figures.is_empty() {
        return Vec::new();
    }
    let drift = linguistic_distance(&source.grammar, &dest.grammar);
    let threshold = Real::from_ratio(TRANSMIT_THRESHOLD.0, TRANSMIT_THRESHOLD.1);

    let mut best: BTreeMap<u32, ConfirmedRelation> = BTreeMap::new();
    for fig in &source.figures {
        for (rid, rel) in &fig.hypothesizer.confirmed {
            best.entry(*rid)
                .and_modify(|cur| {
                    if rel.confidence > cur.confidence {
                        *cur = rel.clone();
                    }
                })
                .or_insert_with(|| rel.clone());
        }
    }

    let mut records = Vec::new();
    let target_idx = 0;
    // transmission-fidelity tools (CulturalEncoding,
    // WrittenJurisprudence, AbstractMathematics, MassLiteracy,
    // LongRangeCommunication, InformationNetworking,
    // DigitalComputation, etc.) lift comprehension multiplicatively.
    // The source civ's tools determine how clearly its knowledge
    // crosses the linguistic gap; clamped at ×1.5 so a fully-
    // equipped civ still has to earn comprehension above the
    // threshold rather than getting it for free.
    let fidelity =
        (Real::ONE + source.tool_transmission_fidelity_bonus()).min(Real::from_ratio(150, 100));
    for (rid, mut rel) in best {
        // Skip relations the destination has already confirmed —
        // direct contact doesn't overwrite local knowledge.
        if dest.figures[target_idx]
            .hypothesizer
            .confirmed
            .contains_key(&rid)
        {
            continue;
        }
        // diffusion comprehension: linguistic-distance term +
        // tier factor; no age decay (live contact). fidelity
        // multiplier folds in.
        let ling = (Real::ONE - drift).max(Real::ZERO);
        let tier = Real::from_ratio(TIER_FACTOR.0, TIER_FACTOR.1);
        let score = (ling * tier * fidelity).min(Real::ONE);
        if score <= threshold {
            continue;
        }
        rel.confidence = rel.confidence * score;
        rel.confirmed_at_tick = transmitted_at_tick;
        records.push(TransmissionRecord {
            relation: rel.clone(),
            comprehension: score,
        });
        dest.figures[target_idx]
            .hypothesizer
            .confirmed
            .insert(rid, rel);
    }
    records
}

/// Run transmission from predecessor to successor. Aggregates
/// the predecessor's per-figure confirmed relations (dedupe by
/// `relation_id`, keeping the highest-confidence version), applies
/// the comprehension gate, and injects survivors into the
/// successor's first-figure hypothesizer with confidence scaled by
/// comprehension. Returns one record per transmitted relation so
/// the caller can emit events.
///
/// also returns mythologization records for relations whose
/// comprehension score fell in `(MYTH_FLOOR, TRANSMIT_THRESHOLD]`
/// — these don't transfer as confirmed knowledge but they
/// perturb the successor civ's cosmology so the residue isn't
/// fully lost.
pub fn transmit_from_parent(
    successor: &mut Civ,
    parent: &Civ,
    transmitted_at_tick: u64,
    decay_ticks: u64,
) -> (Vec<TransmissionRecord>, Vec<MythologizationRecord>) {
    if successor.figures.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let dist = linguistic_distance(&parent.grammar, &successor.grammar);
    let threshold = Real::from_ratio(TRANSMIT_THRESHOLD.0, TRANSMIT_THRESHOLD.1);

    // communicativeness boost: the parent civ's most-communicative
    // figure scales the comprehension score on every transmitted
    // relation. A parent whose canon was carried by figures with
    // strong narrative voices passes more across the boundary.
    // `bonus = 1 + 0.3 × max(communicativeness)` — at the trait
    // ceiling of 1.0 this gives a 30% comprehension boost.
    let max_communicativeness = parent
        .figures
        .iter()
        .map(|f| f.communicativeness)
        .fold(Real::ZERO, |a, b| if a > b { a } else { b });
    let comm_bonus = Real::ONE + Real::from_ratio(3, 10) * max_communicativeness;

    // settlement-tier persistence: a civ that grew to a
    // multi-tier settlement network (capital + towns + villages
    // + hamlets) distributed knowledge archives across its
    // territory; successor civs inherit more of it across the
    // collapse boundary than they would from a hamlet-scale
    // predecessor. Multiplier composes with comm_bonus before
    // the final `min(1.0)` clip.
    let settlement_mult = parent.settlement_persistence_multiplier();

    // transmission-fidelity tools held by the *parent* lift
    // comprehension across the collapse boundary — a civ that
    // wrote things down (CulturalEncoding) preserves them better
    // for successors than one that relied on oral tradition.
    // Capped at ×1.5 so the gate threshold still has to be cleared.
    let fidelity =
        (Real::ONE + parent.tool_transmission_fidelity_bonus()).min(Real::from_ratio(150, 100));

    // Dedupe parent relations by relation_id, keep best confidence.
    let mut best: BTreeMap<u32, ConfirmedRelation> = BTreeMap::new();
    for fig in &parent.figures {
        for (rid, rel) in &fig.hypothesizer.confirmed {
            best.entry(*rid)
                .and_modify(|cur| {
                    if rel.confidence > cur.confidence {
                        *cur = rel.clone();
                    }
                })
                .or_insert_with(|| rel.clone());
        }
    }

    let mut records = Vec::new();
    let mut myths = Vec::new();
    let myth_floor = Real::from_ratio(MYTH_FLOOR.0, MYTH_FLOOR.1);
    let myth_push_base = Real::from_ratio(MYTH_PUSH_BASE.0, MYTH_PUSH_BASE.1);
    let target_idx = 0; // First figure receives all inherited knowledge in v1.
    for (_rid, mut rel) in best {
        let parent_collapsed = parent.collapsed_tick.unwrap_or(transmitted_at_tick);
        let age = transmitted_at_tick.saturating_sub(parent_collapsed);
        let score =
            (comprehension(dist, age, decay_ticks) * comm_bonus * settlement_mult * fidelity)
                .min(Real::ONE);
        if score <= threshold {
            // sub-threshold but above the myth floor →
            // mythologize. Pick a cosmology axis deterministically
            // and apply a small push proportional to (1 − score)
            // (more lost = more cultural distortion).
            if score > myth_floor {
                let axis = mythologization_axis(rel.template_id, rel.relation_id, parent.id);
                let magnitude = myth_push_base * (Real::ONE - score);
                let mut push = cosmology::Cosmology::NEUTRAL;
                match axis {
                    0 => push.empirical = magnitude,
                    1 => push.communitarian = magnitude,
                    2 => push.reformist = magnitude,
                    3 => push.mystical = magnitude,
                    _ => push.hierarchical = magnitude,
                }
                successor.apply_cosmology_push(&push, Real::ONE);
                myths.push(MythologizationRecord {
                    relation_id: rel.relation_id,
                    axis,
                    magnitude,
                    comprehension: score,
                });
            }
            continue;
        }
        // Scale confidence by comprehension to reflect partial
        // recovery; form + params copy verbatim.
        rel.confidence = rel.confidence * score;
        rel.confirmed_at_tick = transmitted_at_tick;
        // tag with inheritance metadata so the successor's
        // hypothesizer re-validates the inherited fit on its own
        // samples after `REVALIDATION_WINDOW_TICKS`. Streaks reset
        // — the predecessor's drift state shouldn't bias the
        // successor's prediction-tracking.
        rel.inherited_from_tick = Some(transmitted_at_tick);
        rel.inherited_from_civ_id = Some(parent.id);
        rel.falsification_streak = 0;
        rel.low_confidence_streak = 0;
        records.push(TransmissionRecord {
            relation: rel.clone(),
            comprehension: score,
        });
        successor.figures[target_idx]
            .hypothesizer
            .confirmed
            .insert(rel.relation_id, rel);
    }
    (records, myths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::figures::{NameGrammar, NameStrategy};
    use sim_species::ModalityKind;

    fn make_grammar(strategy_seed: ModalityKind, civ_id: u32, species_seed: u64) -> NameGrammar {
        NameGrammar::derive(&[strategy_seed], civ_id, species_seed)
    }

    #[test]
    fn linguistic_distance_zero_for_identical_grammar() {
        let g = make_grammar(ModalityKind::AcousticAir, 1, 42);
        let same = g.clone();
        assert_eq!(linguistic_distance(&g, &same), Real::ZERO);
    }

    #[test]
    fn linguistic_distance_one_for_disjoint_strategies() {
        // Acoustic vs Chemical pools share no atoms.
        let a = make_grammar(ModalityKind::AcousticAir, 1, 42);
        let c = make_grammar(ModalityKind::ChemicalPheromone, 1, 42);
        assert_eq!(a.strategy, NameStrategy::Acoustic);
        assert_eq!(c.strategy, NameStrategy::Chemical);
        assert_eq!(linguistic_distance(&a, &c), Real::ONE);
    }

    #[test]
    fn linguistic_distance_partial_for_same_strategy_diff_civs() {
        // Same Acoustic strategy, different civ ids → atom subsets
        // partially overlap but rarely fully match.
        let a = make_grammar(ModalityKind::AcousticAir, 1, 42);
        let b = make_grammar(ModalityKind::AcousticAir, 99, 42);
        let d = linguistic_distance(&a, &b);
        assert!(d >= Real::ZERO);
        assert!(d <= Real::ONE);
    }

    #[test]
    fn age_decay_monotonic_decreasing() {
        let decay = DECAY_CONSTANT_TICKS;
        let young = age_decay(100, decay);
        let old = age_decay(2000, decay);
        let ancient = age_decay(10_000, decay);
        assert!(young > old);
        assert!(old > ancient);
        assert!(young <= Real::ONE);
        assert!(ancient >= Real::ZERO);
    }

    #[test]
    fn comprehension_zero_for_total_linguistic_drift() {
        let score = comprehension(Real::ONE, 0, DECAY_CONSTANT_TICKS);
        assert_eq!(score, Real::ZERO);
    }

    #[test]
    fn comprehension_high_for_zero_distance_zero_age() {
        let score = comprehension(Real::ZERO, 0, DECAY_CONSTANT_TICKS);
        // ling = 1, age = 1, tier = 0.7 → 0.7
        let tier = Real::from_ratio(7, 10);
        assert_eq!(score, tier);
    }

    #[test]
    fn settlement_persistence_multiplier_buckets() {
        // ladder maps lifetime peak claimed_cells → comprehension
        // multiplier. A civ that grew to 16+ cells leaves more
        // knowledge for successors than one that died at hamlet
        // scale. Boundaries match the BFS-ring tier labels in the
        // post-run report (capital / town / village / hamlet).
        let mut civ = Civ::new(1, 0, Real::from_int(1000));
        assert_eq!(civ.peak_claimed_cells, 0);
        assert_eq!(
            civ.settlement_persistence_multiplier(),
            Real::from_ratio(85, 100)
        );

        civ.peak_claimed_cells = 1;
        assert_eq!(
            civ.settlement_persistence_multiplier(),
            Real::from_ratio(85, 100)
        );

        civ.peak_claimed_cells = 2;
        assert_eq!(civ.settlement_persistence_multiplier(), Real::ONE);

        civ.peak_claimed_cells = 5;
        assert_eq!(civ.settlement_persistence_multiplier(), Real::ONE);

        civ.peak_claimed_cells = 6;
        assert_eq!(
            civ.settlement_persistence_multiplier(),
            Real::from_ratio(115, 100)
        );

        civ.peak_claimed_cells = 15;
        assert_eq!(
            civ.settlement_persistence_multiplier(),
            Real::from_ratio(115, 100)
        );

        civ.peak_claimed_cells = 16;
        assert_eq!(
            civ.settlement_persistence_multiplier(),
            Real::from_ratio(130, 100)
        );

        civ.peak_claimed_cells = 1000;
        assert_eq!(
            civ.settlement_persistence_multiplier(),
            Real::from_ratio(130, 100)
        );
    }

    #[test]
    fn claim_cells_tracks_lifetime_peak() {
        // peak_claimed_cells is a high-water mark: it grows with
        // expansion but never shrinks during contraction. A civ
        // that filled 12 cells at peak then shrank back to 1 still
        // reads as "grew to a town network" for transmission
        // purposes — the archives don't vaporise during decline.
        let mut civ = Civ::new(1, 0, Real::from_int(1000));
        let small: std::collections::BTreeSet<u32> = (0..3u32).collect();
        civ.claim_cells(&small);
        assert_eq!(civ.peak_claimed_cells, 3);

        let bigger: std::collections::BTreeSet<u32> = (0..12u32).collect();
        civ.claim_cells(&bigger);
        assert_eq!(civ.peak_claimed_cells, 12);

        let shrunk: std::collections::BTreeSet<u32> = (0..1u32).collect();
        civ.claim_cells(&shrunk);
        assert_eq!(civ.peak_claimed_cells, 12);

        let empty: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
        civ.claim_cells(&empty);
        assert_eq!(civ.peak_claimed_cells, 12);
    }
}
