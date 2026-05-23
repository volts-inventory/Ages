//! Q-war per-pair belligerence assessment.
//!
//! Replaces "any cell overlap → continuous war" with a per-pair
//! belligerence score that decides whether a 75-tick conflict check
//! actually inflicts losses. Overlap remains the *opportunity* (you
//! can only fight cells you both claim) but no longer is the sole
//! *cause* — material drive (population pressure + defender slack
//! capacity + attacker strength share) competes against cultural
//! kinship (cosmology proximity + literacy proximity) as a
//! multiplicative dampener.

use super::grudge::{decayed_grudge, GRUDGE_DECAY_PER_TICK_LOSER, GRUDGE_DECAY_PER_TICK_WINNER};
use super::war::strength;
use crate::Civ;
use sim_arith::{Pop, Real};

/// Headroom factor on capacity used by `pressure`. With factor
/// 1.25, a civ at exactly capacity reads as `pressure = 0.8`
/// rather than 1.0, preventing late-game pinning when capacity-
/// driven expansion saturates every claimed cell.
pub(crate) const PRESSURE_HEADROOM: (i64, i64) = (5, 4);

/// Drive component weights (sum to 1).
pub(crate) const DRIVE_W_PRESSURE: (i64, i64) = (45, 100);
pub(crate) const DRIVE_W_OPPORTUNITY: (i64, i64) = (25, 100);
pub(crate) const DRIVE_W_DOMINANCE: (i64, i64) = (30, 100);

/// Multiplicative dampener: `belligerence = drive ×
/// (1 − KINSHIP_DAMPENER · kinship)`. Identical civs (kinship = 1)
/// see drive multiplied by `1 − 0.1 = 0.9`; totally alien civs
/// (kinship = 0) see drive at full strength.
///
/// Lowered from 0.2 → 0.1 after the 5× cap raise. In single-species
/// runs kinship sits ~0.98 the whole simulation (no second species
/// to dilute it via cosmology / religion / tech distance), so the
/// dampener acts at near-full strength on every pair forever.
/// Real history says same-species civs war constantly; the 0.2
/// value was over-pacifying single-species runs in particular.
pub(crate) const KINSHIP_DAMPENER: (i64, i64) = (10, 100);

/// At-peace pair becomes at-war once `belligerence ≥` this.
///
/// Lowered 0.35 → 0.25 after the 5× cap raise. `pressure = pop /
/// (cap × 1.25)` is the dominant drive term; with cap at 2500
/// instead of 500, an established civ at ~70% of cap reads
/// pressure ~0.56 instead of the much higher saturation it'd hit
/// under the old cap, so belligerence lands ~0.31 even for
/// well-matched pairs in contact — just below the prior 0.35 line.
/// 0.25 clears those marginal pairs into war without crushing the
/// pressure → dominance → opportunity signal mix.
pub(crate) const WAR_DECLARE_THRESHOLD: (i64, i64) = (25, 100);

/// At-war pair becomes at-peace once `belligerence <` this.
/// Hysteresis (declare 0.25 / end 0.15) prevents flapping when
/// pop crosses capacity tick-to-tick.
pub(crate) const WAR_END_THRESHOLD: (i64, i64) = (15, 100);

/// Generation-distance decay constant: kinship loses 1/e of its
/// magnitude per `GENERATION_KIN_DECAY_GENERATIONS` of lineage
/// distance between the pair. Same-species cousins ~8 generations
/// apart get ~0.37 generation-closeness; the full kinship is then
/// multiplied by this factor so deep-cousin pairs read as much
/// more distantly related than direct-line successors.
pub(crate) const GENERATION_KIN_DECAY_GENERATIONS: i64 = 8;

/// Q-war kinship weights (sum to 1.0). added religion as
/// the dominant term — single-species runs have near-zero
/// cosmology gap throughout (every civ inherits the same
/// species bias), so the old equal-weighted three-term mean
/// produced kinship ≈ 1 across every pair. Religion absorbs the
/// fast-divergent cultural signal at civ founding + drift, so
/// weighting it highest gives intra-species pairs a real
/// dispersion in kinship.
pub(crate) const KINSHIP_W_HIER: (i64, i64) = (10, 100);
pub(crate) const KINSHIP_W_COSMO: (i64, i64) = (15, 100);
pub(crate) const KINSHIP_W_TECH: (i64, i64) = (15, 100);
pub(crate) const KINSHIP_W_RELIGION: (i64, i64) = (60, 100);

/// Per-pair belligerence assessment. Computed each conflict check
/// for every overlapping pair that has met contact prerequisites;
/// the core loop uses `belligerence` against the
/// declare/end thresholds to decide whether `resolve()` runs.
#[derive(Debug, Clone)]
pub struct PairAssessment {
    pub aggressor_id: u32,
    pub defender_id: u32,
    pub belligerence: Real,
    pub drive: Real,
    pub kinship: Real,
}

/// Decision the core loop makes for one (overlapping, in-contact)
/// pair this check, given current war state + assessment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarDecision {
    /// Peaceful → at war this check. Emit `WarDeclared` and run
    /// `resolve()`.
    DeclareWar,
    /// Already at war and still belligerent. Run `resolve()`.
    ContinueWar,
    /// Already at war but belligerence dropped below end
    /// threshold. Emit `PeaceConcluded { BelligerenceDropped }`,
    /// no losses this check.
    ConcludePeace,
    /// Peaceful, below declare threshold. Border friction with no
    /// losses.
    StayPeaceful,
}

fn abs_diff(a: Real, b: Real) -> Real {
    let d = a - b;
    if d < Real::ZERO {
        -d
    } else {
        d
    }
}

fn pressure(civ: &Civ, state: &sim_physics::PhysicsState, planet: &sim_world::Planet) -> Real {
    let cap = civ.carrying_capacity_with_terrain(state, planet);
    if cap <= Pop::ZERO {
        return Real::ZERO;
    }
    let headroom = Real::from(PRESSURE_HEADROOM);
    (civ.aggregate_population() / (cap * headroom)).clamp01()
}

fn opportunity(
    attacker: &Civ,
    defender: &Civ,
    state: &sim_physics::PhysicsState,
    planet: &sim_world::Planet,
) -> Real {
    let pop_a = attacker.aggregate_population();
    if pop_a <= Pop::ZERO {
        return Real::ZERO;
    }
    let cap_d = defender.carrying_capacity_with_terrain(state, planet);
    let pop_d = defender.aggregate_population();
    let slack = (cap_d - pop_d).max(Pop::ZERO);
    (slack / pop_a).clamp01()
}

fn dominance(a: &Civ, b: &Civ, tick: u64) -> Real {
    let s_a = strength(a, tick);
    let s_b = strength(b, tick);
    let total = s_a + s_b;
    if total <= Pop::ZERO {
        return Real::from_ratio(1, 2);
    }
    s_a / total
}

fn drive(
    attacker: &Civ,
    defender: &Civ,
    state: &sim_physics::PhysicsState,
    planet: &sim_world::Planet,
    tick: u64,
) -> Real {
    let p = pressure(attacker, state, planet);
    let o = opportunity(attacker, defender, state, planet);
    let d = dominance(attacker, defender, tick);
    let w_p = Real::from(DRIVE_W_PRESSURE);
    let w_o = Real::from(DRIVE_W_OPPORTUNITY);
    let w_d = Real::from(DRIVE_W_DOMINANCE);
    (w_p * p + w_o * o + w_d * d).clamp01()
}

/// Kinship ∈ [0, 1] — weighted closeness across hierarchical
/// axis, the four non-hierarchical cosmology axes (averaged),
/// literacy, and 's three-axis religion vector. Religion
/// dominates (weight 0.60) so the kinship lever survives in
/// single-species runs where cosmology stays clustered.
///
/// Two M6-era modifiers attenuate the base score so intra-
/// species rivalries don't freeze at 0.98:
///   1. Generation closeness: `exp(-|gen_a - gen_b| / 8)`.
///      Same-species cousins 8 lineages apart end up at ~0.37 of
///      the otherwise-computed kinship. Restores the
///      Italian-republics dynamic — same religion, same lineage
///      stem, but enough generational drift to make them fight.
///   2. War-history grudge: per-pair accumulator (asymmetric;
///      loser holds longer) subtracted from the result. Decays
///      lazily via `decayed_grudge`. A pair that's been at war
///      for decades reads as fundamentally hostile in kinship
///      space even if they share religion.
pub(crate) fn kinship_pair(a: &Civ, b: &Civ, tick: u64) -> Real {
    let hier_gap = abs_diff(a.cosmology.hierarchical, b.cosmology.hierarchical).clamp01();
    let four = Real::from_int(4);
    let cosmo_gap = ((abs_diff(a.cosmology.empirical, b.cosmology.empirical)
        + abs_diff(a.cosmology.communitarian, b.cosmology.communitarian)
        + abs_diff(a.cosmology.reformist, b.cosmology.reformist)
        + abs_diff(a.cosmology.mystical, b.cosmology.mystical))
        / four)
        .clamp01();
    let tech_gap = abs_diff(a.literacy_score(tick), b.literacy_score(tick)).clamp01();
    let three = Real::from_int(3);
    let religion_gap = ((abs_diff(a.religion.theology, b.religion.theology)
        + abs_diff(a.religion.ritual, b.religion.ritual)
        + abs_diff(a.religion.sacred_time, b.religion.sacred_time))
        / three)
        .clamp01();
    let w_h = Real::from(KINSHIP_W_HIER);
    let w_c = Real::from(KINSHIP_W_COSMO);
    let w_t = Real::from(KINSHIP_W_TECH);
    let w_r = Real::from(KINSHIP_W_RELIGION);
    let base = w_h * (Real::ONE - hier_gap)
        + w_c * (Real::ONE - cosmo_gap)
        + w_t * (Real::ONE - tech_gap)
        + w_r * (Real::ONE - religion_gap);
    // Generation closeness: 1.0 at zero distance, decays toward 0
    // as lineage depth diverges. exp(-|d| / 8).
    let gen_a = i64::from(a.lineage_depth);
    let gen_b = i64::from(b.lineage_depth);
    let gen_distance = (gen_a - gen_b).abs();
    let decay_arg = Real::from_int(gen_distance) / Real::from_int(GENERATION_KIN_DECAY_GENERATIONS);
    let gen_factor = sim_arith::transcendental::exp(-decay_arg);
    // War-history grudge: average the two asymmetric sides
    // (winner-style decay for `a`'s grudge against `b`, loser-
    // style decay for `b`'s grudge against `a` — and vice versa
    // since `kinship_pair` is symmetric in its arguments; the
    // asymmetry between the two civs is captured in *which side
    // had which decay rate when it accumulated*, not in this
    // call's order).
    let decay_w = Real::from_ratio(
        GRUDGE_DECAY_PER_TICK_WINNER.0,
        GRUDGE_DECAY_PER_TICK_WINNER.1,
    );
    let decay_l = Real::from_ratio(GRUDGE_DECAY_PER_TICK_LOSER.0, GRUDGE_DECAY_PER_TICK_LOSER.1);
    // For each side, use the slower decay rate so the longer-
    // memory side of the pair sets the floor; this conservatively
    // treats both sides as "the loser remembers" when we don't
    // know which side lost the last round.
    let g_a = decayed_grudge(&a.grudges, b.id, tick, decay_l.min(decay_w));
    let g_b = decayed_grudge(&b.grudges, a.id, tick, decay_l.min(decay_w));
    let grudge_avg = (g_a + g_b) / Real::from_int(2);
    (base * gen_factor - grudge_avg).clamp01()
}

/// Compute the per-pair belligerence assessment. The aggressor is
/// the side whose `belligerence` score is higher (ties broken by
/// lower `civ_id`). Returns `None` if either civ has zero population.
pub fn assess_pair(
    a: &Civ,
    b: &Civ,
    state: &sim_physics::PhysicsState,
    planet: &sim_world::Planet,
    tick: u64,
) -> Option<PairAssessment> {
    if a.aggregate_population() <= Pop::ZERO || b.aggregate_population() <= Pop::ZERO {
        return None;
    }
    let kin = kinship_pair(a, b, tick);
    let dampener = Real::from(KINSHIP_DAMPENER);
    let dampener_factor = (Real::ONE - dampener * kin).max(Real::ZERO);
    let drive_ab = drive(a, b, state, planet, tick);
    let drive_ba = drive(b, a, state, planet, tick);
    let bell_ab = drive_ab * dampener_factor;
    let bell_ba = drive_ba * dampener_factor;
    let (aggressor_id, defender_id, belligerence, drive_val) = if bell_ab > bell_ba {
        (a.id, b.id, bell_ab, drive_ab)
    } else if bell_ba > bell_ab {
        (b.id, a.id, bell_ba, drive_ba)
    } else if a.id < b.id {
        (a.id, b.id, bell_ab, drive_ab)
    } else {
        (b.id, a.id, bell_ba, drive_ba)
    };
    Some(PairAssessment {
        aggressor_id,
        defender_id,
        belligerence,
        drive: drive_val,
        kinship: kin,
    })
}

/// Apply the declare/end thresholds with hysteresis to decide what
/// the core loop should do this check for one (overlapping,
/// in-contact) pair.
pub fn decide_war(currently_at_war: bool, belligerence: Real) -> WarDecision {
    let declare = Real::from(WAR_DECLARE_THRESHOLD);
    let end = Real::from(WAR_END_THRESHOLD);
    if currently_at_war {
        if belligerence < end {
            WarDecision::ConcludePeace
        } else {
            WarDecision::ContinueWar
        }
    } else if belligerence >= declare {
        WarDecision::DeclareWar
    } else {
        WarDecision::StayPeaceful
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cosmology::Cosmology;
    use std::collections::BTreeSet;

    fn civ_with_id(id: u32, pop: i64) -> Civ {
        Civ::new(id, 0, Pop::from_int(pop))
    }

    #[test]
    fn decide_war_respects_declare_threshold_when_at_peace() {
        // Just below 0.25 → still peaceful.
        let just_below = Real::percent(24);
        assert_eq!(decide_war(false, just_below), WarDecision::StayPeaceful);
        // At 0.25 → declare.
        let at_threshold = Real::percent(25);
        assert_eq!(decide_war(false, at_threshold), WarDecision::DeclareWar);
        // Above 0.25 → declare.
        let above = Real::percent(70);
        assert_eq!(decide_war(false, above), WarDecision::DeclareWar);
    }

    #[test]
    fn decide_war_respects_end_threshold_when_at_war() {
        // Above 0.15 → keep fighting (covers the 0.15–0.25
        // hysteresis band).
        let mid_band = Real::percent(20);
        assert_eq!(decide_war(true, mid_band), WarDecision::ContinueWar);
        // Just below 0.15 → conclude.
        let just_below = Real::percent(14);
        assert_eq!(decide_war(true, just_below), WarDecision::ConcludePeace);
        // At 0.15 (boundary) → still fighting.
        let at_threshold = Real::percent(15);
        assert_eq!(decide_war(true, at_threshold), WarDecision::ContinueWar);
    }

    #[test]
    fn kinship_is_one_for_identical_civs() {
        let mut a = civ_with_id(1, 100);
        let mut b = civ_with_id(2, 100);
        a.cosmology = Cosmology::NEUTRAL;
        b.cosmology = Cosmology::NEUTRAL;
        // civ_with_id picks up a founding-figure-driven
        // religion vector that differs across civ_ids; force both
        // to NEUTRAL so the kinship test isolates the cosmology
        // path.
        a.religion = crate::religion::Religion::NEUTRAL;
        b.religion = crate::religion::Religion::NEUTRAL;
        let kin = kinship_pair(&a, &b, 100);
        // Q32.32 weighted-sum loses a single ULP relative to
        // exact 1.0; require ≥ 0.999.
        assert!(
            kin > Real::from_ratio(999, 1000),
            "identical civs should have near-1 kinship; got {kin:?}",
        );
    }

    #[test]
    fn kinship_drops_with_cosmology_distance() {
        let mut a = civ_with_id(1, 100);
        let mut b = civ_with_id(2, 100);
        a.cosmology = Cosmology::NEUTRAL;
        b.cosmology = Cosmology::NEUTRAL;
        a.religion = crate::religion::Religion::NEUTRAL;
        b.religion = crate::religion::Religion::NEUTRAL;
        // Push one cosmology axis to maximum gap.
        a.cosmology.empirical = Real::ONE;
        b.cosmology.empirical = -Real::ONE;
        let kin = kinship_pair(&a, &b, 100);
        // cosmo_gap = (2.0 / 4) = 0.5; with weight 0.15, it
        // subtracts 0.075 from full kinship → kinship ≈ 0.925.
        assert!(kin < Real::percent(95));
        assert!(kin > Real::percent(90));
    }

    #[test]
    fn kinship_low_when_all_axes_diverge() {
        let mut a = civ_with_id(1, 100);
        let mut b = civ_with_id(2, 100);
        // Saturate every gap term across every layer (hierarchical,
        // 4 cosmo axes, 3 religion axes; literacy stays equal).
        a.cosmology.hierarchical = Real::ONE;
        b.cosmology.hierarchical = Real::ZERO;
        a.cosmology.empirical = Real::ONE;
        b.cosmology.empirical = -Real::ONE;
        a.cosmology.communitarian = Real::ONE;
        b.cosmology.communitarian = -Real::ONE;
        a.cosmology.reformist = Real::ONE;
        b.cosmology.reformist = -Real::ONE;
        a.cosmology.mystical = Real::ONE;
        b.cosmology.mystical = -Real::ONE;
        a.religion.theology = Real::ONE;
        b.religion.theology = -Real::ONE;
        a.religion.ritual = Real::ONE;
        b.religion.ritual = -Real::ONE;
        a.religion.sacred_time = Real::ONE;
        b.religion.sacred_time = -Real::ONE;
        let kin = kinship_pair(&a, &b, 100);
        // All cultural gaps clamp to 1; literacy gap stays 0
        // (default literacy_score ≈ 0). Weighted closeness =
        // 0.10·0 + 0.15·0 + 0.15·1 + 0.60·0 = 0.15.
        assert!(kin < Real::percent(20));
    }

    #[test]
    fn kinship_decays_with_generation_distance() {
        let mut a = civ_with_id(1, 100);
        let mut b = civ_with_id(2, 100);
        a.cosmology = Cosmology::NEUTRAL;
        b.cosmology = Cosmology::NEUTRAL;
        a.religion = crate::religion::Religion::NEUTRAL;
        b.religion = crate::religion::Religion::NEUTRAL;
        let kin_same_gen = kinship_pair(&a, &b, 100);
        // Eight generations apart → exp(-1) ≈ 0.368 factor.
        b.lineage_depth = 8;
        let kin_8_gen = kinship_pair(&a, &b, 100);
        // Should be ~36.8% of the same-gen value.
        let ratio = kin_8_gen / kin_same_gen;
        let target = Real::from_ratio(368, 1000);
        let drift = if ratio > target {
            ratio - target
        } else {
            target - ratio
        };
        assert!(
            drift < Real::percent(5),
            "8-gen kinship should be ~0.37× same-gen; got ratio {ratio:?}"
        );
    }

    #[test]
    fn kinship_subtracts_grudge_score() {
        let mut a = civ_with_id(1, 100);
        let mut b = civ_with_id(2, 100);
        a.cosmology = Cosmology::NEUTRAL;
        b.cosmology = Cosmology::NEUTRAL;
        a.religion = crate::religion::Religion::NEUTRAL;
        b.religion = crate::religion::Religion::NEUTRAL;
        let kin_no_grudge = kinship_pair(&a, &b, 100);
        // Drop a sizeable grudge on both sides at the current
        // tick (no decay yet).
        a.bump_grudge(2, Real::percent(30), 100);
        b.bump_grudge(1, Real::percent(30), 100);
        let kin_with_grudge = kinship_pair(&a, &b, 100);
        let delta = kin_no_grudge - kin_with_grudge;
        // Average of (0.30, 0.30) = 0.30 subtracted.
        let target = Real::percent(30);
        let drift = if delta > target {
            delta - target
        } else {
            target - delta
        };
        assert!(
            drift < Real::percent(2),
            "grudge should subtract ~0.30 from kinship; got delta {delta:?}"
        );
    }

    #[test]
    fn assess_pair_kin_dampener_lowers_belligerence_vs_alien() {
        // Holding material conditions constant (same population,
        // capacity, overlap, tick), the kin pair must score lower
        // belligerence than the alien pair. This pins the kinship
        // dampener as a real lever even though the absolute scores
        // depend on the ambient capacity / pressure values.
        use sim_physics::{HexGrid, PhysicsState, Substance};
        use sim_world::sample_planet;

        let planet = sample_planet(1);
        let grid = HexGrid::new(4, 4);
        let mut state = PhysicsState::new(grid);
        for v in state.substance_mut(Substance::Fuel.idx()) {
            *v = Real::from_int(10);
        }

        // --- Kin pair: identical neutral cosmology + religion ---
        let mut kin_a = civ_with_id(1, 500);
        let mut kin_b = civ_with_id(2, 500);
        kin_a.cosmology = Cosmology::NEUTRAL;
        kin_b.cosmology = Cosmology::NEUTRAL;
        kin_a.religion = crate::religion::Religion::NEUTRAL;
        kin_b.religion = crate::religion::Religion::NEUTRAL;
        let mut shared = BTreeSet::new();
        shared.insert(0);
        shared.insert(1);
        kin_a.claim_cells(&shared);
        kin_b.claim_cells(&shared);

        // --- Alien pair: opposite cosmology + religion poles ---
        let mut alien_a = civ_with_id(1, 500);
        let mut alien_b = civ_with_id(2, 500);
        alien_a.cosmology.empirical = Real::ONE;
        alien_a.cosmology.communitarian = Real::ONE;
        alien_a.cosmology.reformist = Real::ONE;
        alien_a.cosmology.mystical = Real::ONE;
        alien_a.cosmology.hierarchical = Real::ONE;
        alien_b.cosmology.empirical = -Real::ONE;
        alien_b.cosmology.communitarian = -Real::ONE;
        alien_b.cosmology.reformist = -Real::ONE;
        alien_b.cosmology.mystical = -Real::ONE;
        alien_b.cosmology.hierarchical = Real::ZERO;
        alien_a.religion.theology = Real::ONE;
        alien_a.religion.ritual = Real::ONE;
        alien_a.religion.sacred_time = Real::ONE;
        alien_b.religion.theology = -Real::ONE;
        alien_b.religion.ritual = -Real::ONE;
        alien_b.religion.sacred_time = -Real::ONE;
        alien_a.claim_cells(&shared);
        alien_b.claim_cells(&shared);

        let kin_score =
            assess_pair(&kin_a, &kin_b, &state, &planet, 100).expect("non-zero pop pair");
        let alien_score =
            assess_pair(&alien_a, &alien_b, &state, &planet, 100).expect("non-zero pop pair");

        assert!(
            kin_score.kinship > Real::from_ratio(999, 1000),
            "neutral pair should be near-1 kinship; got {:?}",
            kin_score.kinship,
        );
        assert!(
            alien_score.kinship < kin_score.kinship,
            "opposite-pole pair should be less kin than neutral pair"
        );
        assert!(
            kin_score.belligerence < alien_score.belligerence,
            "kinship dampener should reduce kin belligerence below alien belligerence: kin={:?}, alien={:?}",
            kin_score.belligerence,
            alien_score.belligerence
        );
    }

    #[test]
    fn assess_pair_picks_lower_id_aggressor_on_ties() {
        // Identical civs: bell_ab == bell_ba; tiebreak picks lower
        // id as aggressor for determinism.
        use sim_physics::{HexGrid, PhysicsState, Substance};
        use sim_world::sample_planet;

        let planet = sample_planet(1);
        let grid = HexGrid::new(4, 4);
        let mut state = PhysicsState::new(grid);
        for v in state.substance_mut(Substance::Fuel.idx()) {
            *v = Real::from_int(10);
        }

        let mut a = civ_with_id(1, 500);
        let mut b = civ_with_id(2, 500);
        let mut shared = BTreeSet::new();
        shared.insert(0);
        a.claim_cells(&shared);
        b.claim_cells(&shared);

        let assessment = assess_pair(&a, &b, &state, &planet, 100).expect("non-zero pop pair");
        assert_eq!(assessment.aggressor_id, 1);
        assert_eq!(assessment.defender_id, 2);
    }
}
