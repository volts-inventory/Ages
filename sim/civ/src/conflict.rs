//! conflict resolution between two civs whose `claimed_cells`
//! overlap. Periodic per-pair check (every
//! `CONFLICT_CHECK_TICKS = 75` ticks); strength weighted by
//! population × literacy × Hierarchical-cosmology bonus; loser
//! takes a population hit and surrenders cells if defeated below
//! `CONFLICT_DEFEAT_FLOOR`.

use crate::Civ;
use sim_arith::{Pop, Real};
use std::collections::{BTreeMap, BTreeSet};

pub const CONFLICT_CHECK_TICKS: u64 = 75;
pub const CONFLICT_DEFEAT_FLOOR: i64 = 50;
pub const CONFLICT_MIN_LOSS: (i64, i64) = (10, 100);
pub const CONFLICT_HIERARCHY_BONUS: (i64, i64) = (30, 100);
/// Hierarchical-axis ceiling above which both civs are
/// considered conflict-prone (no peaceful diffusion).
pub const PEACEFUL_HIERARCHY_FLOOR: (i64, i64) = (40, 100);

/// M9 — composite "hierarchical strength" weights. The legacy
/// war-bonus formula multiplied by `(1 + cosmology.hierarchical/2)`,
/// so authority over combat was purely a worldview-axis read.
/// `hierarchical_strength` widens that to a weighted sum across
/// four channels so a civ's military advantage reflects its
/// *actual* institutional reach, not just the abstract pole
/// position of its cosmology:
///
/// - **cosmology** (40%): the original hierarchical axis. Still
///   the dominant signal — formal hierarchy in worldview maps to
///   formal hierarchy in war command.
/// - **religion** (20%): magnitude of the `theology` + `ritual`
///   axes on the religion vector. High-ritual, high-theology
///   civs project authority through religious institutions —
///   the priest-king + state-religion archetype.
/// - **kinship / cohesion** (25%): the civ's internal cohesion
///   scalar. A fragmented polity can't hierarchically project
///   force, however hierarchical its worldview claims to be.
/// - **economic** (15%): surplus / aggregate-pop ratio (saturating
///   at 1.0). Stored economic reach lets the civ pay standing
///   forces, requisition supply, and maintain command chains
///   across multi-cell campaigns.
///
/// Weights sum to 1.0; output clamped to `[0, 1]`.
pub const HIER_W_COSMOLOGY: (i64, i64) = (40, 100);
pub const HIER_W_RELIGION: (i64, i64) = (20, 100);
pub const HIER_W_KINSHIP: (i64, i64) = (25, 100);
pub const HIER_W_ECONOMIC: (i64, i64) = (15, 100);

/// Multi-component hierarchical strength scalar, replacing the
/// legacy single-axis read of `civ.cosmology.hierarchical` in
/// `strength()` + the casualty-bonus formula. See the
/// `HIER_W_*` constants for per-channel weights + rationale.
///
/// Output in `[0, 1]`. Pure-cosmology civs (no religion drift,
/// neutral cohesion, no surplus) recover roughly
/// `0.4 × cosmology.hierarchical` so the legacy war-bonus signal
/// stays preserved as a floor; institutional reach in religion +
/// cohesion + economy adds on top.
#[must_use]
pub fn hierarchical_strength(civ: &Civ) -> Real {
    let cosmology_h = civ.cosmology.hierarchical.clamp01();
    // Religion: average magnitude of the theology + ritual axes.
    // Each axis is in `[-1, 1]`; we read |v| so a high-magnitude
    // religion (whatever its sign) projects authority. Sacred-time
    // is intentionally excluded — it's a temporal-cycle axis,
    // less tied to chain-of-command.
    let religion_h =
        ((civ.religion.theology.abs() + civ.religion.ritual.abs()) / Real::from_int(2)).clamp01();
    // Kinship: read internal cohesion. Already in [0, 1].
    let kinship_h = civ.cohesion.clamp01();
    // Economic: surplus / aggregate_pop, saturating at 1.0. A
    // civ with surplus ≥ its pop has the full economic-rank
    // contribution.
    let pop = civ.aggregate_population();
    let pop_real = pop.to_real_nonneg();
    let economic_h = if pop_real > Real::ZERO {
        (civ.surplus / pop_real).clamp01()
    } else {
        Real::ZERO
    };
    let w_c = Real::from(HIER_W_COSMOLOGY);
    let w_r = Real::from(HIER_W_RELIGION);
    let w_k = Real::from(HIER_W_KINSHIP);
    let w_e = Real::from(HIER_W_ECONOMIC);
    (w_c * cosmology_h + w_r * religion_h + w_k * kinship_h + w_e * economic_h).clamp01()
}

/// Civ size factor for the hierarchy-driven casualty bonus.
/// Returns `min(1.0, log10(cells) / 2)` — a 1-cell band has no
/// hierarchical advantage (organisation costs you nothing when
/// you're a single hamlet), a 100-cell empire gets the full
/// scale (log10(100) / 2 = 1.0). At 14 cells the factor is
/// ≈ 0.57, so the empire's casualty edge over a tribe of equal
/// hierarchy reads as ~0.30 vs ~0.18 — proportional to
/// organisational reach. Falls to 0 for empty civs to avoid
/// log-of-zero blowups.
#[must_use]
pub fn hierarchy_size_factor(cells: usize) -> Real {
    if cells == 0 {
        return Real::ZERO;
    }
    // log10(x) = ln(x) / ln(10); ln(10) ≈ 2.302585.
    let n = Real::from_int(i64::try_from(cells).unwrap_or(i64::MAX));
    let ln_n = sim_arith::transcendental::ln(n);
    let ln_10 = sim_arith::transcendental::ln(Real::from_int(10));
    let factor = ln_n / (ln_10 * Real::from_int(2));
    factor.clamp01()
}

/// `strength = aggregate_pop × (1 + literacy) × (1 + hierarchical_strength/2) × tool_war_multiplier × surplus_modifier`.
///
/// the per-tool war-strength contribution (`ContactWeapon`
/// +0.10, `RangedMomentumWeapon` +0.10, `StoneWorking` +0.05,
/// `OrganizedHunting` +0.05, plus tier-2+ fortification / chemical-
/// projectile / mechanisation contributions) folds in
/// multiplicatively via `Civ::tool_war_strength_multiplier`.
///
/// M8: surplus modifier reads as "well-fed troops fight better".
/// A civ with stored surplus ≥ its aggregate pop gets the full
/// `SURPLUS_WAR_BONUS_CAP` (+0.15) multiplier; a depleted civ
/// (surplus = 0) gets the baseline 1.0. Combined with the
/// existing strength terms so an empire with broken supply lines
/// can still lose to a smaller civ with full granaries.
///
/// M9: `hierarchical_strength` is no longer a pure cosmology read.
/// See `hierarchical_strength()` for the four-channel composite
/// (cosmology 40% / religion 20% / kinship 25% / economic 15%).
/// The legacy formula `(1 + cosmology.hierarchical / 2)` is
/// recovered as a floor when the other channels are zeroed.
pub fn strength(civ: &Civ, tick: u64) -> Pop {
    let pop = civ.aggregate_population();
    let literacy = civ.literacy_score(tick);
    let hier = hierarchical_strength(civ);
    let war_bonus = Real::ONE + hier / Real::from_int(2);
    let pop_real = pop.to_real_nonneg();
    let surplus_modifier = crate::economy::surplus_war_strength_modifier(civ.surplus, pop_real);
    pop * (Real::ONE + literacy) * war_bonus * civ.tool_war_strength_multiplier() * surplus_modifier
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
    // M9: casualty bonus reads the composite hierarchical_strength
    // rather than the bare cosmology axis. A civ with religious
    // hierarchy + economic surplus + tight cohesion projects
    // command-and-control beyond what its cosmology axis alone
    // would suggest, inflicting heavier per-cell casualties on
    // the loser.
    let winner_hier = if winner_id == a.id {
        hierarchical_strength(a)
    } else {
        hierarchical_strength(b)
    };
    let min_loss = Real::from(CONFLICT_MIN_LOSS);
    let hier_bonus = Real::from(CONFLICT_HIERARCHY_BONUS);
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
    let clamped = raw_ratio.max(Real::ONE).min(Real::from_int(4));
    let tech_gap = (clamped - Real::ONE) * Real::from_ratio(10, 100);
    // Hierarchy-size factor: the +0.30 hierarchy casualty bonus
    // is gated by organisational reach. A 14-cell tribe with a
    // hierarchical chief gets ~0.18 of the bonus, a 100-cell
    // empire with the same chief gets the full 0.30. Otherwise
    // a tiny tribal band reads as effective as the empire of
    // the same hierarchical cosmology, which doesn't match
    // historical asymmetry (Macedon out-organising Persia at
    // size, not a Sumerian city-state at 6 cells).
    let size_factor = hierarchy_size_factor(winner_civ.claimed_cells.len());
    let loss_frac = (min_loss + winner_hier * hier_bonus * size_factor + tech_gap)
        .max(Real::ZERO)
        .min(Real::from_ratio(60, 100));

    let (loser, winner) = if loser_id == a.id { (a, b) } else { (b, a) };
    let flip_floor = Pop::from_int(CELL_FLIP_FLOOR);

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
            c.fertile = (c.fertile - fertile_loss).max(Pop::ZERO);
            // Spillover to juveniles: 30% of the headline fraction
            // hits juveniles too — adolescents dragooned into the
            // levy take a smaller hit than fertile adults but more
            // than infants/elders.
            let juvenile_loss = c.juvenile * loss_frac * Real::from_ratio(30, 100);
            c.juvenile = (c.juvenile - juvenile_loss).max(Pop::ZERO);
        }
        loser.resync_aggregate_from_regions();
        let post_count = loser
            .region_cohorts
            .get(cell)
            .map_or(Pop::ZERO, sim_population::Cohort::total);
        if post_count <= flip_floor {
            // Cell flips to winner.
            loser.claimed_cells.remove(cell);
            loser.region_cohorts.remove(cell);
            winner.claimed_cells.insert(*cell);
            winner
                .region_cohorts
                .entry(*cell)
                .or_insert_with(|| crate::Cohort::with_civ(Pop::ZERO, winner.id));
            flipped_this_check.push(*cell);
        }
    }

    // War "ends this check" when no overlap remains. That covers
    // both the single-cell-flip case and the multi-cell-cleanup
    // case where the last contested cells flipped at once.
    let remaining = overlap(loser, winner);
    let defeated = remaining.is_empty();

    // Bump per-pair grudges asymmetrically. Loser remembers more
    // (and its grudge decays slower; see GRUDGE_DECAY_PER_TICK_*),
    // so a defeated civ holds the grievance for decades while the
    // winner forgets within years. Net effect: the loser drives
    // future intra-pair belligerence even after religion +
    // cosmology have re-converged.
    let winner_bump = Real::from(GRUDGE_BUMP_WINNER);
    let loser_bump = Real::from(GRUDGE_BUMP_LOSER);
    loser.bump_grudge(winner_id, loser_bump, tick);
    winner.bump_grudge(loser_id, winner_bump, tick);

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
    let floor = Real::from(PEACEFUL_HIERARCHY_FLOOR);
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
/// see drive multiplied by `1 − 0.1 = 0.9`; totally alien civs
/// (kinship = 0) see drive at full strength.
///
/// Lowered from 0.2 → 0.1 after the 5× cap raise. In single-species
/// runs kinship sits ~0.98 the whole simulation (no second species
/// to dilute it via cosmology / religion / tech distance), so the
/// dampener acts at near-full strength on every pair forever.
/// Real history says same-species civs war constantly; the 0.2
/// value was over-pacifying single-species runs in particular.
pub const KINSHIP_DAMPENER: (i64, i64) = (10, 100);

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
pub const WAR_DECLARE_THRESHOLD: (i64, i64) = (25, 100);

/// At-war pair becomes at-peace once `belligerence <` this.
/// Hysteresis (declare 0.25 / end 0.15) prevents flapping when
/// pop crosses capacity tick-to-tick.
pub const WAR_END_THRESHOLD: (i64, i64) = (15, 100);

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

/// Generation-distance decay constant: kinship loses 1/e of its
/// magnitude per `GENERATION_KIN_DECAY_GENERATIONS` of lineage
/// distance between the pair. Same-species cousins ~8 generations
/// apart get ~0.37 generation-closeness; the full kinship is then
/// multiplied by this factor so deep-cousin pairs read as much
/// more distantly related than direct-line successors.
pub const GENERATION_KIN_DECAY_GENERATIONS: i64 = 8;

/// Grudge increment per skirmish (the round-level loss applied by
/// `resolve`). Asymmetric: loser holds longer. Loser's grudge
/// against winner gets `GRUDGE_BUMP_LOSER`, winner's grudge against
/// loser gets `GRUDGE_BUMP_WINNER` — both grow during a multi-round
/// war but the loser's accumulator climbs faster *and* decays
/// slower (see `GRUDGE_DECAY_PER_TICK_LOSER` /
/// `GRUDGE_DECAY_PER_TICK_WINNER`).
pub const GRUDGE_BUMP_WINNER: (i64, i64) = (5, 100);
pub const GRUDGE_BUMP_LOSER: (i64, i64) = (10, 100);
/// Per-tick lazy decay rates. Applied at kinship-read time using
/// the stored `last_update_tick` so we don't need a per-tick
/// maintenance pass. Winner forgets at ~0.001/tick, loser at half
/// that — the defeated civ remembers significantly longer (decades
/// rather than years).
pub const GRUDGE_DECAY_PER_TICK_WINNER: (i64, i64) = (1, 1000);
pub const GRUDGE_DECAY_PER_TICK_LOSER: (i64, i64) = (5, 10_000);
/// Cap on a single grudge score. Prevents an infinite-war
/// edge case (the +0.05/0.10 bumps every 75 ticks would otherwise
/// drift unbounded under no decay window).
pub const GRUDGE_CEILING: (i64, i64) = (60, 100);

/// Resolve a grudge score for the current tick, applying the
/// per-side decay rate retroactively. Returns `0` if the entry
/// is missing or the decay has driven the score to zero or below.
#[must_use]
pub fn decayed_grudge(
    grudges: &BTreeMap<u32, (Real, u64)>,
    other_id: u32,
    current_tick: u64,
    decay_per_tick: Real,
) -> Real {
    let (raw, last_tick) = match grudges.get(&other_id) {
        Some(v) => *v,
        None => return Real::ZERO,
    };
    let elapsed = current_tick.saturating_sub(last_tick);
    let elapsed_real = Real::from_int(i64::try_from(elapsed).unwrap_or(i64::MAX));
    let decayed = raw - decay_per_tick * elapsed_real;
    decayed.max(Real::ZERO)
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
fn kinship_pair(a: &Civ, b: &Civ, tick: u64) -> Real {
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

    fn civ_with_id(id: u32, pop: i64) -> Civ {
        Civ::new(id, 0, Pop::from_int(pop))
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
    fn hierarchical_strength_composes_four_channels() {
        // Pure cosmology axis (default cohesion = 1.0, no
        // religion, no surplus): formula reduces to
        // 0.4 × cosmology + 0.25 × cohesion = 0.4 + 0.25 = 0.65
        // for cosmology=1, cohesion=1.
        let mut civ = civ_with_id(1, 100);
        civ.cosmology = Cosmology::NEUTRAL;
        civ.religion = crate::religion::Religion::NEUTRAL;
        civ.cosmology.hierarchical = Real::ONE;
        let h_pure = hierarchical_strength(&civ);
        let expected = Real::from_ratio(65, 100);
        let drift = if h_pure > expected {
            h_pure - expected
        } else {
            expected - h_pure
        };
        assert!(
            drift < Real::from_ratio(1, 100),
            "pure-cosmology civ should land at ~0.65; got {h_pure:?}"
        );

        // Add religion: +0.20 × ((|theology| + |ritual|) / 2).
        // Max both religion axes → +0.20.
        civ.religion.theology = Real::ONE;
        civ.religion.ritual = -Real::ONE;
        let h_religion = hierarchical_strength(&civ);
        assert!(
            h_religion > h_pure,
            "adding religion should lift hierarchy; got {h_religion:?} vs {h_pure:?}"
        );

        // Drop cohesion to zero (fragmented). The kinship rank
        // (0.25 weight) vanishes; net should drop ~0.25 below the
        // religion-heavy reading.
        civ.cohesion = Real::ZERO;
        let h_fragmented = hierarchical_strength(&civ);
        assert!(
            h_fragmented < h_religion,
            "fragmented civ should drop the kinship contribution; got {h_fragmented:?}"
        );

        // Add economic rank: surplus ≥ pop → +0.15.
        civ.surplus = Real::from_int(10_000); // far above pop=100
        let h_full = hierarchical_strength(&civ);
        assert!(
            h_full > h_fragmented,
            "economic rank should add on top; got {h_full:?}"
        );

        // Output is clamped to [0, 1].
        assert!(h_full <= Real::ONE);
    }

    #[test]
    fn hierarchical_strength_clamps_negative_cosmology() {
        // Negative cosmology hierarchical (egalitarian pole)
        // should clamp at zero — the formula doesn't go negative.
        let mut civ = civ_with_id(1, 100);
        civ.cosmology = Cosmology::NEUTRAL;
        civ.religion = crate::religion::Religion::NEUTRAL;
        civ.cosmology.hierarchical = -Real::ONE;
        civ.cohesion = Real::ZERO;
        let h = hierarchical_strength(&civ);
        // cosmology clamped to 0, religion 0, kinship 0, economic 0.
        assert_eq!(h, Real::ZERO);
    }

    #[test]
    fn defeated_loser_surrenders_cells() {
        // per-cell flip threshold is `CELL_FLIP_FLOOR = 25`.
        // Loser starts at 30 in the disputed cell; max loss_frac
        // is 0.50, so post-check the cell holds 15 < 25 and
        // flips. With only one disputed cell, the flip leaves no
        // overlap → loser_defeated = true.
        //
        // Hierarchical casualty bonus now scales with claimed-cell
        // count (log10(cells)/2 capped at 1.0). A 100-cell winner
        // hits the cap and recovers the legacy +0.30 contribution
        // that drives loss_frac above the flip floor; we give civ
        // 1 a 100-cell claim with only cell 0 contested.
        let mut a = civ_with_id(1, 1_000_000);
        let mut b = civ_with_id(2, 30);
        let mut a_cells = BTreeSet::new();
        for i in 0..100u32 {
            a_cells.insert(i);
        }
        a.claim_cells(&a_cells);
        let mut b_cells = BTreeSet::new();
        b_cells.insert(0);
        b.claim_cells(&b_cells);
        a.cosmology.hierarchical = Real::ONE;
        let r = resolve(&mut a, &mut b, 100).unwrap();
        assert!(r.loser_defeated);
        assert!(!b.claimed_cells.contains(&0));
        assert!(a.claimed_cells.contains(&0));
    }

    #[test]
    fn hierarchy_size_factor_scales_with_civ_size() {
        // A 1-cell band gets no hierarchical organisational
        // advantage; a 100-cell empire hits the cap.
        let f1 = hierarchy_size_factor(1);
        // log10(1) = 0 → factor 0.
        assert!(f1 < Real::from_ratio(1, 1000));

        let f100 = hierarchy_size_factor(100);
        // log10(100) / 2 = 1.0 → factor 1.0.
        let drift = Real::ONE - f100;
        assert!(drift < Real::from_ratio(1, 100));

        // Between: 14-cell tribe ≈ 0.57.
        let f14 = hierarchy_size_factor(14);
        let target = Real::from_ratio(57, 100);
        let drift14 = if f14 > target {
            f14 - target
        } else {
            target - f14
        };
        assert!(
            drift14 < Real::from_ratio(5, 100),
            "14-cell factor should be ~0.57; got {f14:?}"
        );

        // 1000-cell empire still capped at 1.0 (log10/2 = 1.5).
        let f1000 = hierarchy_size_factor(1000);
        let drift1000 = Real::ONE - f1000;
        assert!(drift1000 < Real::from_ratio(1, 1000));
    }

    #[test]
    fn small_band_does_not_get_hierarchy_casualty_edge() {
        // A 1-cell band fighting a 1-cell band, even with maxed
        // hierarchical cosmology, gets only the base 10% loss
        // (no organisational bonus at this size). Loser starts
        // above the flip floor and survives the round.
        let mut a = civ_with_id(1, 10_000_000); // wins by sheer pop
        let mut b = civ_with_id(2, 200); // above CELL_FLIP_FLOOR=25
        let mut cells = BTreeSet::new();
        cells.insert(0);
        a.claim_cells(&cells);
        b.claim_cells(&cells);
        a.cosmology.hierarchical = Real::ONE;
        let r = resolve(&mut a, &mut b, 100).unwrap();
        // 1-cell winner → size_factor = 0; loss_frac = 0.10 only.
        // 200 × 0.90 = 180, well above the 25 flip floor.
        assert!(!r.loser_defeated, "1-cell winner shouldn't one-shot");
        assert!(b.claimed_cells.contains(&0));
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
        // Just below 0.25 → still peaceful.
        let just_below = Real::from_ratio(24, 100);
        assert_eq!(decide_war(false, just_below), WarDecision::StayPeaceful);
        // At 0.25 → declare.
        let at_threshold = Real::from_ratio(25, 100);
        assert_eq!(decide_war(false, at_threshold), WarDecision::DeclareWar);
        // Above 0.25 → declare.
        let above = Real::from_ratio(70, 100);
        assert_eq!(decide_war(false, above), WarDecision::DeclareWar);
    }

    #[test]
    fn decide_war_respects_end_threshold_when_at_war() {
        // Above 0.15 → keep fighting (covers the 0.15–0.25
        // hysteresis band).
        let mid_band = Real::from_ratio(20, 100);
        assert_eq!(decide_war(true, mid_band), WarDecision::ContinueWar);
        // Just below 0.15 → conclude.
        let just_below = Real::from_ratio(14, 100);
        assert_eq!(decide_war(true, just_below), WarDecision::ConcludePeace);
        // At 0.15 (boundary) → still fighting.
        let at_threshold = Real::from_ratio(15, 100);
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
            drift < Real::from_ratio(5, 100),
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
        a.bump_grudge(2, Real::from_ratio(30, 100), 100);
        b.bump_grudge(1, Real::from_ratio(30, 100), 100);
        let kin_with_grudge = kinship_pair(&a, &b, 100);
        let delta = kin_no_grudge - kin_with_grudge;
        // Average of (0.30, 0.30) = 0.30 subtracted.
        let target = Real::from_ratio(30, 100);
        let drift = if delta > target {
            delta - target
        } else {
            target - delta
        };
        assert!(
            drift < Real::from_ratio(2, 100),
            "grudge should subtract ~0.30 from kinship; got delta {delta:?}"
        );
    }

    #[test]
    fn grudge_decays_over_time() {
        let g: BTreeMap<u32, (Real, u64)> = {
            let mut m = BTreeMap::new();
            m.insert(2u32, (Real::from_ratio(20, 100), 100));
            m
        };
        // Slowest decay (loser memory): 5/10_000 per tick.
        let decay = Real::from_ratio(GRUDGE_DECAY_PER_TICK_LOSER.0, GRUDGE_DECAY_PER_TICK_LOSER.1);
        // 200 ticks later: 0.20 - 0.0005*200 = 0.10.
        let d = decayed_grudge(&g, 2, 300, decay);
        let expected = Real::from_ratio(10, 100);
        let drift = if d > expected {
            d - expected
        } else {
            expected - d
        };
        assert!(
            drift < Real::from_ratio(1, 100),
            "grudge should decay to ~0.10; got {d:?}"
        );
        // 1000 ticks later: 0.20 - 0.5 → clamped to 0.
        let d2 = decayed_grudge(&g, 2, 1100, decay);
        assert_eq!(d2, Real::ZERO);
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
        a.unlocked_tools
            .insert(crate::tech::ToolKind::ContactWeapon);
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
