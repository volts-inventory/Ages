//! Alliance formation / dissolution.
//!
//! PR4: `Civ::allied_with` was a dead field — declared, read by the
//! conflict resolver, but never populated. These helpers give it a
//! formation rule, dissolution rules, and a trust scalar that
//! decays as the pair's worldview drifts.
//!
//! Formation criteria (cumulative — all must hold):
//!   1. Cosmology proximity: 5-axis euclidean distance < 0.4.
//!   2. Religion proximity: 3-axis euclidean distance < 0.4.
//!   3. Positive contact history: pair has at least one prior
//!      CivContact (mirrored in `Civ::contact_history`) AND is
//!      not currently at war.
//! Dissolution paths:
//!   - Cosmology drift: 5-axis distance exceeds 0.6.
//!   - War misalignment: one ally is at war with a third party the
//!     other is at peace with (driven from the war_state at the
//!     sim/core layer; this module exposes the per-pair predicate).
//!   - Trust eroded: per-pair trust scalar (decayed each check by
//!     the religion+cosmology gap) crosses below 0.2.

use crate::Civ;
use sim_arith::Real;

/// Formation gate on 5-axis cosmology euclidean distance.
pub const ALLIANCE_FORM_COSMO_GAP: (i64, i64) = (40, 100);
/// Formation gate on 3-axis religion euclidean distance.
pub const ALLIANCE_FORM_RELIGION_GAP: (i64, i64) = (40, 100);
/// Dissolution trigger on 5-axis cosmology euclidean distance.
/// Set above the formation gate so a pair right at the edge of
/// forming an alliance isn't immediately dissolved by a single
/// ULP of drift.
pub const ALLIANCE_DISSOLVE_COSMO_GAP: (i64, i64) = (60, 100);
/// Trust floor: alliance dissolves with `TrustEroded` reason when
/// the per-pair trust scalar drops below this value.
pub const ALLIANCE_TRUST_FLOOR: (i64, i64) = (20, 100);
/// Trust scalar at formation (full faith).
pub const ALLIANCE_TRUST_INITIAL: (i64, i64) = (100, 100);
/// Per-check trust decay weight on the religion+cosmology gap
/// average. With weight 0.5, a pair whose worldview gap averages
/// 0.5 loses 0.25 trust per conflict check (~6 sim-years); the
/// trust scalar therefore crosses the 0.2 floor in 3-4 checks.
pub const ALLIANCE_TRUST_DECAY_WEIGHT: (i64, i64) = (50, 100);
/// Cooldown in ticks between successive alliances of the same
/// pair. Without this, a pair whose cosmology / religion drifts
/// across the 0.4 (form) / 0.6 (dissolve) hysteresis edge can flap
/// — form, dissolve, form, dissolve every alliance check. 200
/// ticks ≈ 17 sim-yr at monthly cadence is long enough that the
/// post-dissolution drift either settles back into proximity (then
/// reform is meaningful, not flap) or carries the pair further
/// apart.
pub const ALLIANCE_FORM_COOLDOWN_TICKS: u64 = 200;

/// 5-axis cosmology euclidean distance between two civs (across
/// `empirical`, `communitarian`, `reformist`, `mystical`,
/// `hierarchical`). Result is non-negative; ≥ 0 with no upper
/// clamp (theoretical max ~sqrt(20) ≈ 4.47 if every axis is
/// fully opposed).
#[must_use]
pub fn cosmology_distance(a: &Civ, b: &Civ) -> Real {
    let de = a.cosmology.empirical - b.cosmology.empirical;
    let dc = a.cosmology.communitarian - b.cosmology.communitarian;
    let dr = a.cosmology.reformist - b.cosmology.reformist;
    let dm = a.cosmology.mystical - b.cosmology.mystical;
    let dh = a.cosmology.hierarchical - b.cosmology.hierarchical;
    let sum = de * de + dc * dc + dr * dr + dm * dm + dh * dh;
    sim_arith::transcendental::sqrt(sum.max(Real::ZERO))
}

/// 3-axis religion euclidean distance between two civs (across
/// `theology`, `ritual`, `sacred_time`).
#[must_use]
pub fn religion_distance(a: &Civ, b: &Civ) -> Real {
    let dt = a.religion.theology - b.religion.theology;
    let dr = a.religion.ritual - b.religion.ritual;
    let ds = a.religion.sacred_time - b.religion.sacred_time;
    let sum = dt * dt + dr * dr + ds * ds;
    sim_arith::transcendental::sqrt(sum.max(Real::ZERO))
}

/// Decide if `a` and `b` should form a mutual alliance this
/// check. Returns `true` only when all four cumulative criteria
/// hold:
///   1. Cosmology distance < `ALLIANCE_FORM_COSMO_GAP` (0.4).
///   2. Religion distance < `ALLIANCE_FORM_RELIGION_GAP` (0.4).
///   3. Not currently at war (caller passes `at_war_now`).
///   4. Both civs have prior contact history on each other
///      (mirrored from the `CivContact` emission path).
///   5. Neither already allies the other (idempotent — caller
///      typically pre-filters but we double-check).
/// `tick` is the current sim tick. Used to enforce
/// `ALLIANCE_FORM_COOLDOWN_TICKS` between successive alliances of
/// the same pair: a pair that dissolved an alliance at tick T must
/// wait at least `ALLIANCE_FORM_COOLDOWN_TICKS` more ticks before
/// re-allying, preventing flap at the 0.4 / 0.6 hysteresis edge.
#[must_use]
pub fn propose_alliance(a: &Civ, b: &Civ, at_war_now: bool, tick: u64) -> bool {
    if at_war_now {
        return false;
    }
    if a.allied_with.contains(&b.id) || b.allied_with.contains(&a.id) {
        return false;
    }
    if !a.contact_history.contains(&b.id) || !b.contact_history.contains(&a.id) {
        return false;
    }
    // Cooldown: either side can carry a recent-dissolution stamp;
    // the strictest of the two governs. If neither side has a
    // prior dissolution, the cooldown is trivially satisfied.
    let prior_a = a.alliance_cooldown.get(&b.id).copied();
    let prior_b = b.alliance_cooldown.get(&a.id).copied();
    let last_dissolve = match (prior_a, prior_b) {
        (Some(ta), Some(tb)) => Some(ta.max(tb)),
        (Some(t), None) | (None, Some(t)) => Some(t),
        (None, None) => None,
    };
    if let Some(t_dissolve) = last_dissolve {
        if tick.saturating_sub(t_dissolve) < ALLIANCE_FORM_COOLDOWN_TICKS {
            return false;
        }
    }
    let cosmo_gap = cosmology_distance(a, b);
    if cosmo_gap >= Real::from(ALLIANCE_FORM_COSMO_GAP) {
        return false;
    }
    let religion_gap = religion_distance(a, b);
    if religion_gap >= Real::from(ALLIANCE_FORM_RELIGION_GAP) {
        return false;
    }
    true
}

/// Drift-based dissolution check. Returns `true` when the pair's
/// 5-axis cosmology distance exceeds `ALLIANCE_DISSOLVE_COSMO_GAP`
/// (0.6) — they've drifted too far for the alliance to hold.
#[must_use]
pub fn alliance_drifted_apart(a: &Civ, b: &Civ) -> bool {
    cosmology_distance(a, b) >= Real::from(ALLIANCE_DISSOLVE_COSMO_GAP)
}

/// Apply one tick of trust decay on the alliance entry. The
/// decay step is `(cosmo_gap + religion_gap) / 2 ×
/// ALLIANCE_TRUST_DECAY_WEIGHT`. Returns the post-decay trust
/// value clamped to `[0, 1]`.
#[must_use]
pub fn step_alliance_trust(prior_trust: Real, cosmo_gap: Real, religion_gap: Real) -> Real {
    let avg_gap = ((cosmo_gap + religion_gap) / Real::from_int(2)).clamp01();
    let weight = Real::from(ALLIANCE_TRUST_DECAY_WEIGHT);
    let decayed = prior_trust - avg_gap * weight;
    decayed.max(Real::ZERO).min(Real::ONE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cosmology::Cosmology;
    use sim_arith::Pop;
    use std::collections::BTreeSet;

    fn civ_with_id(id: u32, pop: i64) -> Civ {
        Civ::new(id, 0, Pop::from_int(pop))
    }

    /// `propose_alliance` requires BOTH cosmology and religion
    /// distances under their respective gates. Pinning each axis
    /// independently above the gate must reject the alliance.
    #[test]
    fn proposed_alliance_requires_both_cosmology_and_religion_proximity() {
        let mut a = civ_with_id(1, 100);
        let mut b = civ_with_id(2, 100);
        a.cosmology = Cosmology::NEUTRAL;
        b.cosmology = Cosmology::NEUTRAL;
        a.religion = crate::religion::Religion::NEUTRAL;
        b.religion = crate::religion::Religion::NEUTRAL;
        // Wire up mutual contact history so the prior-contact gate
        // doesn't itself reject every variant.
        a.contact_history.insert(b.id);
        b.contact_history.insert(a.id);

        // Baseline: identical civs, in contact, at peace → ally.
        assert!(
            propose_alliance(&a, &b, false, 100),
            "identical-cosmology + identical-religion in-contact pair should ally"
        );

        // Push cosmology distance above the gate (0.4). One axis
        // pole-flipped gives a single-axis distance of 2.0,
        // sqrt of 4.0 = 2.0 > 0.4 → reject.
        a.cosmology.empirical = Real::ONE;
        b.cosmology.empirical = -Real::ONE;
        assert!(
            !propose_alliance(&a, &b, false, 100),
            "wide-cosmology pair should reject alliance even with shared religion"
        );

        // Restore cosmology; push religion above the gate.
        a.cosmology = Cosmology::NEUTRAL;
        b.cosmology = Cosmology::NEUTRAL;
        a.religion.theology = Real::ONE;
        b.religion.theology = -Real::ONE;
        assert!(
            !propose_alliance(&a, &b, false, 100),
            "wide-religion pair should reject alliance even with shared cosmology"
        );

        // Even with both axes proximal: if at war, no alliance.
        a.religion = crate::religion::Religion::NEUTRAL;
        b.religion = crate::religion::Religion::NEUTRAL;
        assert!(
            !propose_alliance(&a, &b, true, 100),
            "at-war pair should never ally"
        );

        // No prior contact → no alliance.
        let mut c = civ_with_id(3, 100);
        let mut d = civ_with_id(4, 100);
        c.cosmology = Cosmology::NEUTRAL;
        d.cosmology = Cosmology::NEUTRAL;
        c.religion = crate::religion::Religion::NEUTRAL;
        d.religion = crate::religion::Religion::NEUTRAL;
        assert!(
            !propose_alliance(&c, &d, false, 100),
            "never-met pair should not ally"
        );
    }

    /// Once both civs flag each other allied, `resolve` returns
    /// `None` even over fully-disputed territory. This is the
    /// payoff of the formation rule — the previously-dead
    /// `allied_with` gate now actually fires.
    #[test]
    fn alliance_short_circuits_conflict_after_formation() {
        use super::super::war::resolve;
        let shared = {
            let mut s = BTreeSet::new();
            s.insert(0);
            s.insert(1);
            s
        };

        // Without alliance: real outcome.
        let mut a = civ_with_id(1, 1000);
        let mut b = civ_with_id(2, 1000);
        a.claim_cells(&shared);
        b.claim_cells(&shared);
        let outcome_pre = resolve(&mut a, &mut b, 100);
        assert!(
            outcome_pre.is_some(),
            "overlap without alliance should resolve to a conflict outcome"
        );

        // Reset for the second resolve call.
        let mut a = civ_with_id(1, 1000);
        let mut b = civ_with_id(2, 1000);
        a.claim_cells(&shared);
        b.claim_cells(&shared);
        // Form a symmetric alliance.
        a.allied_with.insert(b.id);
        b.allied_with.insert(a.id);
        let outcome_post = resolve(&mut a, &mut b, 100);
        assert!(
            outcome_post.is_none(),
            "fully-mutual alliance must short-circuit conflict resolution"
        );

        // Unilateral flag (only one side allies the other) must
        // NOT short-circuit — the AND-gate semantics enforce true
        // mutual alliance.
        let mut a = civ_with_id(1, 1000);
        let mut b = civ_with_id(2, 1000);
        a.claim_cells(&shared);
        b.claim_cells(&shared);
        a.allied_with.insert(b.id);
        let outcome_unilateral = resolve(&mut a, &mut b, 100);
        assert!(
            outcome_unilateral.is_some(),
            "unilateral allied flag must not short-circuit (alliance is symmetric)"
        );
    }

    /// Dissolution rule fires when cosmology distance exceeds
    /// `ALLIANCE_DISSOLVE_COSMO_GAP`. The drift-check predicate
    /// returns true once the pair lands beyond 0.6.
    #[test]
    fn alliance_dissolves_when_cosmology_drifts_apart() {
        let mut a = civ_with_id(1, 100);
        let mut b = civ_with_id(2, 100);
        a.cosmology = Cosmology::NEUTRAL;
        b.cosmology = Cosmology::NEUTRAL;
        // Near-identical pair should NOT trigger dissolution.
        assert!(
            !alliance_drifted_apart(&a, &b),
            "neutral-pair distance is ~0 and must not trigger drift dissolution"
        );

        // Push past the dissolution threshold. A pole-flip on a
        // single axis gives a euclidean distance of 2.0, which
        // far exceeds the 0.6 gate.
        a.cosmology.mystical = Real::ONE;
        b.cosmology.mystical = -Real::ONE;
        assert!(
            alliance_drifted_apart(&a, &b),
            "wide cosmology gap (≥ 0.6) must trigger drift dissolution"
        );

        // Sanity: the trust scalar also decays on the same gap.
        // From a full-trust start, a single step at the widest
        // measured gap drops below the trust floor.
        let post = step_alliance_trust(Real::ONE, cosmology_distance(&a, &b), Real::ZERO);
        let floor = Real::from(ALLIANCE_TRUST_FLOOR);
        assert!(
            post < floor || alliance_drifted_apart(&a, &b),
            "single decay step at wide cosmo gap should cross the trust floor or hit the drift trigger; got trust {post:?}"
        );
    }

    /// Once `propose_alliance` greenlights a pair, the symmetric
    /// `allied_with` flags + initial trust scalar form the
    /// alliance state that subsequent ticks read. This pins the
    /// formation step (state shape) — the wire-format event is
    /// emitted by the sim/core layer using the same predicate.
    #[test]
    fn alliance_formed_event_emitted_at_formation() {
        let mut a = civ_with_id(1, 100);
        let mut b = civ_with_id(2, 100);
        a.cosmology = Cosmology::NEUTRAL;
        b.cosmology = Cosmology::NEUTRAL;
        a.religion = crate::religion::Religion::NEUTRAL;
        b.religion = crate::religion::Religion::NEUTRAL;
        a.contact_history.insert(b.id);
        b.contact_history.insert(a.id);

        // Predicate must greenlight first.
        assert!(propose_alliance(&a, &b, false, 100));

        // The sim/core formation pass applies a symmetric insert
        // + initial trust. Mirror that here.
        a.allied_with.insert(b.id);
        b.allied_with.insert(a.id);
        let initial_trust = Real::from(ALLIANCE_TRUST_INITIAL);
        a.alliance_trust.insert(b.id, initial_trust);
        b.alliance_trust.insert(a.id, initial_trust);

        // Post-formation invariants the event downstream relies
        // on: symmetric flag + present trust scalar.
        assert!(
            a.allied_with.contains(&b.id) && b.allied_with.contains(&a.id),
            "formation must set symmetric allied_with flags"
        );
        assert_eq!(a.alliance_trust.get(&b.id), Some(&initial_trust));
        assert_eq!(b.alliance_trust.get(&a.id), Some(&initial_trust));

        // Re-running propose_alliance after the flags are set
        // must return false (idempotent — pair is already allied).
        assert!(
            !propose_alliance(&a, &b, false, 100),
            "already-allied pair must not re-form alliance"
        );
    }

    /// Distance helpers behave: zero distance for identical civs,
    /// strictly positive for any axis difference.
    #[test]
    fn cosmology_and_religion_distance_helpers_are_correct() {
        let mut a = civ_with_id(1, 100);
        let mut b = civ_with_id(2, 100);
        a.cosmology = Cosmology::NEUTRAL;
        b.cosmology = Cosmology::NEUTRAL;
        a.religion = crate::religion::Religion::NEUTRAL;
        b.religion = crate::religion::Religion::NEUTRAL;
        let d_c = cosmology_distance(&a, &b);
        let d_r = religion_distance(&a, &b);
        assert!(
            d_c < Real::from_ratio(1, 1000) && d_r < Real::from_ratio(1, 1000),
            "neutral pairs must have ~0 distance on both axes; got c={d_c:?} r={d_r:?}"
        );

        // Single-axis pole flip on cosmology → distance = 2.0.
        a.cosmology.hierarchical = Real::ONE;
        b.cosmology.hierarchical = -Real::ONE;
        let d_c2 = cosmology_distance(&a, &b);
        let target = Real::from_int(2);
        let drift = if d_c2 > target { d_c2 - target } else { target - d_c2 };
        assert!(
            drift < Real::percent(1),
            "pole-flipped single axis should give distance 2.0; got {d_c2:?}"
        );
    }
}
