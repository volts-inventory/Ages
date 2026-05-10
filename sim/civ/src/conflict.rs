//! conflict resolution between two civs whose `claimed_cells`
//! overlap. Periodic per-pair check (every
//! `CONFLICT_CHECK_TICKS = 75` ticks); strength weighted by
//! population × literacy × Hierarchical-cosmology bonus; loser
//! takes a population hit and surrenders cells if defeated below
//! `CONFLICT_DEFEAT_FLOOR`.

use crate::Civ;
use sim_arith::Real;
use std::collections::BTreeSet;

pub const CONFLICT_CHECK_TICKS: u64 = 75;
pub const CONFLICT_DEFEAT_FLOOR: i64 = 50;
pub const CONFLICT_MIN_LOSS: (i64, i64) = (10, 100);
pub const CONFLICT_HIERARCHY_BONUS: (i64, i64) = (30, 100);
/// Hierarchical-axis ceiling above which both civs are
/// considered conflict-prone (no peaceful diffusion).
pub const PEACEFUL_HIERARCHY_FLOOR: (i64, i64) = (40, 100);

/// `strength = aggregate_pop × (1 + literacy) × (1 + Hierarchical/2) × tool_war_multiplier`.
///
/// the per-tool war-strength contribution (`ContactWeapon`
/// +0.10, `RangedMomentumWeapon` +0.10, `StoneWorking` +0.05,
/// `OrganizedHunting` +0.05, plus tier-2+ fortification / chemical-
/// projectile / mechanisation contributions) folds in
/// multiplicatively via `Civ::tool_war_strength_multiplier`.
pub fn strength(civ: &Civ, tick: u64) -> Real {
    let pop = civ.aggregate_population();
    let literacy = civ.literacy_score(tick);
    let hier = civ.cosmology.hierarchical;
    let war_bonus = Real::ONE + hier / Real::from_int(2);
    pop * (Real::ONE + literacy) * war_bonus * civ.tool_war_strength_multiplier()
}

/// Cells the two civs both claim.
pub fn overlap(a: &Civ, b: &Civ) -> BTreeSet<u32> {
    a.claimed_cells
        .intersection(&b.claimed_cells)
        .copied()
        .collect()
}

/// Outcome of one conflict check between civs `a` and `b`.
#[derive(Debug, Clone)]
pub struct ConflictOutcome {
    pub winner_id: u32,
    pub loser_id: u32,
    pub disputed_cells: Vec<u32>,
    pub loss_fraction: Real,
    pub loser_defeated: bool,
}

/// per-cell flip threshold. Each conflict check, any
/// disputed cell whose loser-side cohort drops below this many
/// individuals THIS CHECK flips ownership immediately. Set to
/// half the aggregate `CONFLICT_DEFEAT_FLOOR` so individual
/// cells can flip without the loser's whole civ collapsing.
pub const CELL_FLIP_FLOOR: i64 = CONFLICT_DEFEAT_FLOOR / 2;

/// Resolve a conflict between `a` and `b` at `tick`.
/// changed this from a single-tick total resolution (drop all
/// pop, surrender all cells if defeated) to a per-cell skirmish
/// round: pop drops in each disputed cell at `loss_frac`, and
/// each cell flips to the winner only if THAT cell's loser
/// cohort drops below `CELL_FLIP_FLOOR`. Multi-cell wars now
/// span multiple checks (every `CONFLICT_CHECK_TICKS` = 75
/// ticks ≈ 6 years) — a war over 5 disputed cells reads as a
/// 30-90 year campaign rather than a single instantaneous
/// resolution.
///
/// `loser_defeated` semantics shifted: it now means "this is
/// the last round of the war — loser has no remaining overlap
/// with winner after this check." Single-cell wars still
/// resolve in one check.
///
/// Returns `None` when there's no overlap to fight over.
pub fn resolve(a: &mut Civ, b: &mut Civ, tick: u64) -> Option<ConflictOutcome> {
    let disputed: Vec<u32> = overlap(a, b).into_iter().collect();
    if disputed.is_empty() {
        return None;
    }
    let s_a = strength(a, tick);
    let s_b = strength(b, tick);
    let (winner_id, loser_id) = if s_a > s_b {
        (a.id, b.id)
    } else if s_b > s_a {
        (b.id, a.id)
    } else if a.id < b.id {
        (a.id, b.id)
    } else {
        (b.id, a.id)
    };
    let winner_hier = if winner_id == a.id {
        a.cosmology.hierarchical
    } else {
        b.cosmology.hierarchical
    };
    let min_loss = Real::from_ratio(CONFLICT_MIN_LOSS.0, CONFLICT_MIN_LOSS.1);
    let hier_bonus = Real::from_ratio(CONFLICT_HIERARCHY_BONUS.0, CONFLICT_HIERARCHY_BONUS.1);
    // Tech-asymmetry term. The winner's tool-derived
    // war-strength multiplier already determined who wins via
    // `strength()`; here we let the tech *gap* between the
    // belligerents amplify the per-cell casualty fraction so a
    // gunpowder civ defeating a stone-age neighbour inflicts
    // dramatically more casualties than two parity civs grinding
    // each other down. Ratio = winner_war_mult / loser_war_mult,
    // clamped to `[1, 4]` so a 10× tech gap doesn't extrapolate
    // to 90% per-cell loss. At parity (ratio = 1) the term is 0;
    // at the cap (ratio ≥ 4) it adds +0.30 to `loss_frac` —
    // enough to feel like "the modern army cuts through the
    // pre-modern levy" without one-shotting the loser.
    let (winner_civ, loser_civ_for_ratio) = if winner_id == a.id {
        (&*a, &*b)
    } else {
        (&*b, &*a)
    };
    let winner_mult = winner_civ.tool_war_strength_multiplier();
    let loser_mult = loser_civ_for_ratio.tool_war_strength_multiplier();
    let raw_ratio = if loser_mult > Real::ZERO {
        winner_mult / loser_mult
    } else {
        Real::from_int(4)
    };
    let clamped = raw_ratio
        .max(Real::ONE)
        .min(Real::from_int(4));
    let tech_gap = (clamped - Real::ONE) * Real::from_ratio(10, 100);
    let loss_frac = (min_loss + winner_hier * hier_bonus + tech_gap)
        .max(Real::ZERO)
        .min(Real::from_ratio(60, 100));

    let (loser, winner) = if loser_id == a.id { (a, b) } else { (b, a) };
    let flip_floor = Real::from_int(CELL_FLIP_FLOOR);

    // drop pop AND check per-cell flip in one pass.
    let mut flipped_this_check: Vec<u32> = Vec::new();
    for cell in &disputed {
        // War casualties: take the loss out of the fertile bracket
        // first (combat-age adults bear the brunt), then any
        // remainder spills into juveniles. Infants and elders are
        // not directly killed by war contact in this model — they
        // die through follow-on famine/displacement which is
        // already handled by the per-tick step under degraded
        // capacity. We achieve the bracket-targeted policy by
        // calling `drop_cell_pop` on a temporary scaled fraction
        // applied only to fertile + juvenile via direct mutation.
        if let Some(c) = loser.region_cohorts.get_mut(cell) {
            let fertile_loss = c.fertile * loss_frac;
            c.fertile = (c.fertile - fertile_loss).max(Real::ZERO);
            // Spillover to juveniles: 30% of the headline fraction
            // hits juveniles too — adolescents dragooned into the
            // levy take a smaller hit than fertile adults but more
            // than infants/elders.
            let juvenile_loss = c.juvenile * loss_frac * Real::from_ratio(30, 100);
            c.juvenile = (c.juvenile - juvenile_loss).max(Real::ZERO);
        }
        loser.resync_aggregate_from_regions();
        let post_count = loser
            .region_cohorts
            .get(cell)
            .map_or(Real::ZERO, sim_population::Cohort::total);
        if post_count <= flip_floor {
            // Cell flips to winner.
            loser.claimed_cells.remove(cell);
            loser.region_cohorts.remove(cell);
            winner.claimed_cells.insert(*cell);
            winner
                .region_cohorts
                .entry(*cell)
                .or_insert_with(|| crate::Cohort::with_civ(Real::ZERO, winner.id));
            flipped_this_check.push(*cell);
        }
    }

    // War "ends this check" when no overlap remains. That covers
    // both the single-cell-flip case and the multi-cell-cleanup
    // case where the last contested cells flipped at once.
    let remaining = overlap(loser, winner);
    let defeated = remaining.is_empty();

    Some(ConflictOutcome {
        winner_id,
        loser_id,
        disputed_cells: disputed,
        loss_fraction: loss_frac,
        loser_defeated: defeated,
    })
}

/// Whether two civs are peaceful enough for cross-civ knowledge
/// diffusion (both Hierarchical axes below the floor).
pub fn is_peaceful_pair(a: &Civ, b: &Civ) -> bool {
    let floor = Real::from_ratio(PEACEFUL_HIERARCHY_FLOOR.0, PEACEFUL_HIERARCHY_FLOOR.1);
    a.cosmology.hierarchical < floor && b.cosmology.hierarchical < floor
}

// === Belligerence model ===========================================
//
// Q-war: replaces "any cell overlap → continuous war" with a per-
// pair belligerence score that decides whether a 75-tick conflict
// check actually inflicts losses. Overlap remains the *opportunity*
// (you can only fight cells you both claim) but no longer is the
// sole *cause* — material drive (population pressure + defender
// slack capacity + attacker strength share) competes against
// cultural kinship (cosmology proximity + literacy proximity) as a
// multiplicative dampener.

/// Headroom factor on capacity used by `pressure`. With factor
/// 1.25, a civ at exactly capacity reads as `pressure = 0.8`
/// rather than 1.0, preventing late-game pinning when capacity-
/// driven expansion saturates every claimed cell.
pub const PRESSURE_HEADROOM: (i64, i64) = (5, 4);

/// Drive component weights (sum to 1).
pub const DRIVE_W_PRESSURE: (i64, i64) = (45, 100);
pub const DRIVE_W_OPPORTUNITY: (i64, i64) = (25, 100);
pub const DRIVE_W_DOMINANCE: (i64, i64) = (30, 100);

/// Multiplicative dampener: `belligerence = drive ×
/// (1 − KINSHIP_DAMPENER · kinship)`. Identical civs (kinship = 1)
/// see drive multiplied by `1 − 0.2 = 0.8`; totally alien civs
/// (kinship = 0) see drive at full strength. Calibrated against
/// the seed-100 / 8000-tick smoke run: with single-species seeds
/// where every civ inherits the same species cosmology bias,
/// kinship sits near 1.0 the whole run. Higher dampener values
/// (0.4, 0.6) crushed every war; 0.2 lets pressure + dominance
/// breakthroughs fire without erasing kinship's effect.
pub const KINSHIP_DAMPENER: (i64, i64) = (20, 100);

/// At-peace pair becomes at-war once `belligerence ≥` this.
/// Lowered from the initial 0.55 because realistic drive values
/// in capacity-driven-expansion runs cap around 0.5 even in
/// strong-vs-weak matchups; 0.55 produced zero wars at seed 100.
pub const WAR_DECLARE_THRESHOLD: (i64, i64) = (35, 100);

/// At-war pair becomes at-peace once `belligerence <` this.
/// Hysteresis (declare 0.35 / end 0.20) prevents flapping when
/// pop crosses capacity tick-to-tick.
pub const WAR_END_THRESHOLD: (i64, i64) = (20, 100);

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

fn clamp01(x: Real) -> Real {
    x.max(Real::ZERO).min(Real::ONE)
}

fn pressure(
    civ: &Civ,
    state: &sim_physics::PhysicsState,
    planet: &sim_world::Planet,
) -> Real {
    let cap = civ.carrying_capacity_with_terrain(state, planet);
    if cap <= Real::ZERO {
        return Real::ZERO;
    }
    let headroom = Real::from_ratio(PRESSURE_HEADROOM.0, PRESSURE_HEADROOM.1);
    clamp01(civ.aggregate_population() / (cap * headroom))
}

fn opportunity(
    attacker: &Civ,
    defender: &Civ,
    state: &sim_physics::PhysicsState,
    planet: &sim_world::Planet,
) -> Real {
    let pop_a = attacker.aggregate_population();
    if pop_a <= Real::ZERO {
        return Real::ZERO;
    }
    let cap_d = defender.carrying_capacity_with_terrain(state, planet);
    let pop_d = defender.aggregate_population();
    let slack = (cap_d - pop_d).max(Real::ZERO);
    clamp01(slack / pop_a)
}

fn dominance(a: &Civ, b: &Civ, tick: u64) -> Real {
    let s_a = strength(a, tick);
    let s_b = strength(b, tick);
    let total = s_a + s_b;
    if total <= Real::ZERO {
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
    let w_p = Real::from_ratio(DRIVE_W_PRESSURE.0, DRIVE_W_PRESSURE.1);
    let w_o = Real::from_ratio(DRIVE_W_OPPORTUNITY.0, DRIVE_W_OPPORTUNITY.1);
    let w_d = Real::from_ratio(DRIVE_W_DOMINANCE.0, DRIVE_W_DOMINANCE.1);
    clamp01(w_p * p + w_o * o + w_d * d)
}

/// Q-war kinship weights (sum to 1.0). added religion as
/// the dominant term — single-species runs have near-zero
/// cosmology gap throughout (every civ inherits the same
/// species bias), so the old equal-weighted three-term mean
/// produced kinship ≈ 1 across every pair. Religion absorbs the
/// fast-divergent cultural signal at civ founding + drift, so
/// weighting it highest gives intra-species pairs a real
/// dispersion in kinship.
pub const KINSHIP_W_HIER: (i64, i64) = (10, 100);
pub const KINSHIP_W_COSMO: (i64, i64) = (15, 100);
pub const KINSHIP_W_TECH: (i64, i64) = (15, 100);
pub const KINSHIP_W_RELIGION: (i64, i64) = (60, 100);

/// Kinship ∈ [0, 1] — weighted closeness across hierarchical
/// axis, the four non-hierarchical cosmology axes (averaged),
/// literacy, and 's three-axis religion vector. Religion
/// dominates (weight 0.60) so the kinship lever survives in
/// single-species runs where cosmology stays clustered.
fn kinship_pair(a: &Civ, b: &Civ, tick: u64) -> Real {
    let hier_gap = clamp01(abs_diff(a.cosmology.hierarchical, b.cosmology.hierarchical));
    let four = Real::from_int(4);
    let cosmo_gap = clamp01(
        (abs_diff(a.cosmology.empirical, b.cosmology.empirical)
            + abs_diff(a.cosmology.communitarian, b.cosmology.communitarian)
            + abs_diff(a.cosmology.reformist, b.cosmology.reformist)
            + abs_diff(a.cosmology.mystical, b.cosmology.mystical))
            / four,
    );
    let tech_gap = clamp01(abs_diff(a.literacy_score(tick), b.literacy_score(tick)));
    let three = Real::from_int(3);
    let religion_gap = clamp01(
        (abs_diff(a.religion.theology, b.religion.theology)
            + abs_diff(a.religion.ritual, b.religion.ritual)
            + abs_diff(a.religion.sacred_time, b.religion.sacred_time))
            / three,
    );
    let w_h = Real::from_ratio(KINSHIP_W_HIER.0, KINSHIP_W_HIER.1);
    let w_c = Real::from_ratio(KINSHIP_W_COSMO.0, KINSHIP_W_COSMO.1);
    let w_t = Real::from_ratio(KINSHIP_W_TECH.0, KINSHIP_W_TECH.1);
    let w_r = Real::from_ratio(KINSHIP_W_RELIGION.0, KINSHIP_W_RELIGION.1);
    clamp01(
        w_h * (Real::ONE - hier_gap)
            + w_c * (Real::ONE - cosmo_gap)
            + w_t * (Real::ONE - tech_gap)
            + w_r * (Real::ONE - religion_gap),
    )
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
    if a.aggregate_population() <= Real::ZERO || b.aggregate_population() <= Real::ZERO {
        return None;
    }
    let kin = kinship_pair(a, b, tick);
    let dampener = Real::from_ratio(KINSHIP_DAMPENER.0, KINSHIP_DAMPENER.1);
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
    let declare = Real::from_ratio(WAR_DECLARE_THRESHOLD.0, WAR_DECLARE_THRESHOLD.1);
    let end = Real::from_ratio(WAR_END_THRESHOLD.0, WAR_END_THRESHOLD.1);
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

    fn civ_with_id(id: u32, pop: i64) -> Civ {
        Civ::new(id, 0, Real::from_int(pop))
    }

    #[test]
    fn no_outcome_when_no_overlap() {
        let mut a = civ_with_id(1, 100);
        let mut b = civ_with_id(2, 100);
        // Disjoint claims.
        let mut a_cells = BTreeSet::new();
        a_cells.insert(0);
        a.claim_cells(&a_cells);
        let mut b_cells = BTreeSet::new();
        b_cells.insert(1);
        b.claim_cells(&b_cells);
        let r = resolve(&mut a, &mut b, 100);
        assert!(r.is_none());
    }

    #[test]
    fn larger_civ_wins_when_overlap_exists() {
        let mut a = civ_with_id(1, 1000);
        let mut b = civ_with_id(2, 100);
        let mut shared = BTreeSet::new();
        shared.insert(0);
        a.claim_cells(&shared);
        b.claim_cells(&shared);
        let r = resolve(&mut a, &mut b, 100).unwrap();
        assert_eq!(r.winner_id, 1);
        assert_eq!(r.loser_id, 2);
    }

    #[test]
    fn defeated_loser_surrenders_cells() {
        // per-cell flip threshold is `CELL_FLIP_FLOOR = 25`.
        // Loser starts at 30 in the disputed cell; max loss_frac
        // is 0.50, so post-check the cell holds 15 < 25 and
        // flips. With only one disputed cell, the flip leaves no
        // overlap → loser_defeated = true.
        let mut a = civ_with_id(1, 1_000_000);
        let mut b = civ_with_id(2, 30);
        let mut cells = BTreeSet::new();
        cells.insert(0);
        a.claim_cells(&cells);
        b.claim_cells(&cells);
        a.cosmology.hierarchical = Real::ONE;
        let r = resolve(&mut a, &mut b, 100).unwrap();
        assert!(r.loser_defeated);
        assert!(!b.claimed_cells.contains(&0));
        assert!(a.claimed_cells.contains(&0));
    }

    #[test]
    fn peaceful_pair_requires_both_low_hierarchy() {
        let mut a = civ_with_id(1, 100);
        let mut b = civ_with_id(2, 100);
        a.cosmology = Cosmology::NEUTRAL;
        b.cosmology = Cosmology::NEUTRAL;
        assert!(is_peaceful_pair(&a, &b));
        a.cosmology.hierarchical = Real::from_ratio(7, 10);
        assert!(!is_peaceful_pair(&a, &b));
    }

    // ─── Q-war belligerence model ───

    #[test]
    fn decide_war_respects_declare_threshold_when_at_peace() {
        // Just below 0.35 → still peaceful.
        let just_below = Real::from_ratio(34, 100);
        assert_eq!(decide_war(false, just_below), WarDecision::StayPeaceful);
        // At 0.35 → declare.
        let at_threshold = Real::from_ratio(35, 100);
        assert_eq!(decide_war(false, at_threshold), WarDecision::DeclareWar);
        // Above 0.35 → declare.
        let above = Real::from_ratio(70, 100);
        assert_eq!(decide_war(false, above), WarDecision::DeclareWar);
    }

    #[test]
    fn decide_war_respects_end_threshold_when_at_war() {
        // Above 0.20 → keep fighting (covers the 0.20–0.35
        // hysteresis band).
        let mid_band = Real::from_ratio(28, 100);
        assert_eq!(decide_war(true, mid_band), WarDecision::ContinueWar);
        // Just below 0.20 → conclude.
        let just_below = Real::from_ratio(19, 100);
        assert_eq!(decide_war(true, just_below), WarDecision::ConcludePeace);
        // At 0.20 (boundary) → still fighting.
        let at_threshold = Real::from_ratio(20, 100);
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
        assert!(kin < Real::from_ratio(95, 100));
        assert!(kin > Real::from_ratio(90, 100));
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
        assert!(kin < Real::from_ratio(20, 100));
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

        let kin_score = assess_pair(&kin_a, &kin_b, &state, &planet, 100)
            .expect("non-zero pop pair");
        let alien_score = assess_pair(&alien_a, &alien_b, &state, &planet, 100)
            .expect("non-zero pop pair");

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

    /// Tech gap between belligerents amplifies the per-cell loss
    /// fraction inflicted on the loser. Two parity-tech civs of
    /// equal pop produce a small `loss_fraction`; the same civ
    /// pair where the winner has gunpowder + advanced materials
    /// produces a substantially larger one.
    #[test]
    fn tech_gap_amplifies_loss_fraction() {
        // Run 1: parity tech (both pre-tech).
        let mut a = civ_with_id(1, 5000);
        let mut b = civ_with_id(2, 1000);
        let mut shared = BTreeSet::new();
        shared.insert(0);
        a.claim_cells(&shared);
        b.claim_cells(&shared);
        let r_parity = resolve(&mut a, &mut b, 100).expect("overlap should fight");
        let parity_loss = r_parity.loss_fraction;

        // Run 2: same pops + cells but winner has tier-3 + tier-4
        // weapon tools unlocked. Loser stays pre-tech.
        let mut a = civ_with_id(1, 5000);
        let mut b = civ_with_id(2, 1000);
        a.claim_cells(&shared);
        b.claim_cells(&shared);
        a.unlocked_tools.insert(crate::tech::ToolKind::ContactWeapon);
        a.unlocked_tools
            .insert(crate::tech::ToolKind::RangedMomentumWeapon);
        a.unlocked_tools
            .insert(crate::tech::ToolKind::ChemicalProjectile);
        a.unlocked_tools
            .insert(crate::tech::ToolKind::AdvancedMaterials);
        let r_tech = resolve(&mut a, &mut b, 100).expect("overlap should fight");
        assert_eq!(r_tech.winner_id, 1);
        assert!(
            r_tech.loss_fraction > parity_loss,
            "tech-asymmetric loss {:?} should exceed parity loss {:?}",
            r_tech.loss_fraction,
            parity_loss
        );
    }
}
