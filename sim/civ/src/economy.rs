//! M8 — economic dimension. Per-civ `surplus` accumulator
//! that gates food-crisis collapse, modulates war strength,
//! and absorbs catastrophe shocks.
//!
//! Surplus is a *buffer* — population above subsistence
//! that accumulates productive output, depleted by war, catastrophe,
//! and public works. A civ whose cells run at high utilisation
//! and whose tech stack multiplies production accumulates surplus
//! faster than a frontier civ scrambling to feed every cell.
//!
//! Units: dimensionless Q32.32, in the same scale as
//! `aggregate_population()` so ratios `(surplus / pop)` read as
//! "how many years of average per-capita pop has this civ saved
//! up". Capped at `SURPLUS_CEILING_FRAC × aggregate_pop` so
//! mature civs can't accumulate unbounded reserves.

use crate::Civ;
use sim_arith::Real;

/// Cell-utilisation threshold above which a civ accumulates
/// surplus. Below this floor, every productive unit is going to
/// keeping the population fed; above, the excess flows into the
/// accumulator. 0.7 = cells running at 70%+ of cap.
pub const SURPLUS_UTILIZATION_FLOOR: (i64, i64) = (70, 100);

/// Surplus accumulation per tick when utilisation is at full
/// (utilisation = 1.0). Scaled linearly by the headroom above
/// `SURPLUS_UTILIZATION_FLOOR`, so a civ exactly at the floor
/// gains nothing; a civ at full saturation gains the full rate.
/// Units: fraction of aggregate population per tick.
pub const SURPLUS_GAIN_PER_TICK: (i64, i64) = (1, 1000);

/// Surplus drain per tick per active war this civ is in.
/// Frontline supply, mobilisation, and economic disruption eat
/// stored reserves; sustained wars exhaust them.
pub const SURPLUS_WAR_DRAIN_PER_TICK: (i64, i64) = (15, 10_000);

/// Fraction of remaining surplus consumed when a catastrophe
/// fires. Stored reserves absorb shocks first; the civ trades
/// surplus for survival.
pub const SURPLUS_CATASTROPHE_DRAIN_FRAC: (i64, i64) = (40, 100);

/// Per-civ surplus ceiling, expressed as a fraction of aggregate
/// population. A civ can't store more than 5× its current pop's
/// worth of surplus — buffer is bounded by storage + perishability
/// + institutional carrying capacity for reserves.
pub const SURPLUS_CEILING_FRAC: i64 = 5;

/// Emit threshold (absolute delta) for `CivSurplusChanged` event
/// gating. Same shape as the cohesion / life-expectancy gates —
/// only emit when the surplus has moved by at least this much
/// since the last emission. Reads in units of population so a
/// 50-person change on a civ of 1000 fires; a 50-person change
/// on a civ of 1_000_000 doesn't (the relative shift is too small
/// to be narratively significant).
pub const SURPLUS_EMIT_DELTA_FLOOR: i64 = 50;

/// Maximum food-security buffer the surplus can contribute.
/// At surplus = `SURPLUS_FOOD_BUFFER_FULL × demand`, the buffer
/// adds the full `SURPLUS_FOOD_BUFFER_BONUS` to security.
/// Bounded so a civ with infinite surplus doesn't become
/// uncrashable.
pub const SURPLUS_FOOD_BUFFER_FULL: i64 = 2;
pub const SURPLUS_FOOD_BUFFER_BONUS: (i64, i64) = (20, 100);

/// Maximum war-strength modifier the surplus can contribute.
/// Adds up to +`SURPLUS_WAR_BONUS_CAP` to the `1.0` baseline
/// strength multiplier when the civ has surplus ≥ aggregate pop.
pub const SURPLUS_WAR_BONUS_CAP: (i64, i64) = (15, 100);

/// Step the civ's surplus accumulator one tick. Reads aggregate
/// pop + cap utilisation from the civ's current cell state;
/// drains by war count and clamps to the ceiling.
///
/// `at_war_count` is the number of active wars touching this
/// civ (0 = peace, 1+ = each war drains the per-war rate).
///
/// Returns the updated surplus value so the caller can decide
/// whether to emit a `CivSurplusChanged` event.
pub fn step_surplus(civ: &mut Civ, utilisation: Real, at_war_count: u64) {
    let pop = civ.aggregate_population();
    let pop_real = pop.to_real_nonneg();
    if pop_real <= Real::ZERO {
        // Empty civ: no surplus generation, and existing surplus
        // decays away over time (no one to hold it).
        civ.surplus = (civ.surplus * Real::percent(99)).max(Real::ZERO);
        return;
    }
    // Accumulation: scales by (utilisation - floor) clamped to
    // [0, 1 - floor]. So a civ at the floor gains nothing; a civ
    // at full saturation gains the full rate.
    let floor = Real::from(SURPLUS_UTILIZATION_FLOOR);
    let headroom = (utilisation - floor).max(Real::ZERO).min(Real::ONE - floor);
    let max_headroom = Real::ONE - floor;
    let scale_factor = if max_headroom > Real::ZERO {
        headroom / max_headroom
    } else {
        Real::ZERO
    };
    let gain_rate = Real::from(SURPLUS_GAIN_PER_TICK);
    let gain = pop_real * gain_rate * scale_factor;
    // Drain: per war drain rate × war count, denominated in
    // population units so it cancels the gain at parity-scale.
    let drain_rate = Real::from(SURPLUS_WAR_DRAIN_PER_TICK);
    let war_factor = Real::from_int(i64::try_from(at_war_count).unwrap_or(i64::MAX));
    let drain = pop_real * drain_rate * war_factor;
    let new_surplus = (civ.surplus + gain - drain).max(Real::ZERO);
    let ceiling = pop_real * Real::from_int(SURPLUS_CEILING_FRAC);
    civ.surplus = new_surplus.min(ceiling);
}

/// Apply a catastrophe's surplus shock. Drops the accumulator
/// by `SURPLUS_CATASTROPHE_DRAIN_FRAC`. Called from the
/// catastrophe-firing site after the cohort loss has been applied.
pub fn drain_surplus_on_catastrophe(civ: &mut Civ) {
    let drain_frac = Real::from(SURPLUS_CATASTROPHE_DRAIN_FRAC);
    civ.surplus = (civ.surplus * (Real::ONE - drain_frac)).max(Real::ZERO);
}

/// Compute the food-security buffer the civ's surplus
/// contributes. Adds to the raw security score so a civ with
/// stored reserves rides out lean ticks without tipping into
/// crisis. Returns a non-negative additive bonus in `[0, BONUS_CAP]`.
#[must_use]
pub fn surplus_food_buffer(surplus: Real, demand: sim_arith::Pop) -> Real {
    let demand_real = demand.to_real_nonneg();
    if demand_real <= Real::ZERO {
        return Real::ZERO;
    }
    let ratio = surplus / demand_real;
    let full = Real::from_int(SURPLUS_FOOD_BUFFER_FULL);
    let capped_ratio = ratio.min(full);
    let bonus_cap = Real::from(SURPLUS_FOOD_BUFFER_BONUS);
    if full > Real::ZERO {
        (capped_ratio / full) * bonus_cap
    } else {
        Real::ZERO
    }
}

/// Per-tick trade-flow fraction. Each open trade route moves
/// this fraction of the higher-surplus civ's reserve toward the
/// lower-surplus civ, smoothing the buffer across the trading
/// pair. Small enough that one route doesn't equalize in a
/// single tick; large enough that a multi-decade route brings
/// a wealthy + a poor civ into rough surplus parity.
pub const TRADE_FLOW_PER_TICK: (i64, i64) = (5, 10_000);

/// Per-tick trade flow between two civs. Computes
/// `(richer.surplus - poorer.surplus) × TRADE_FLOW_PER_TICK / 2`
/// (half each side) and shifts that magnitude from richer to
/// poorer. Returns the flow magnitude so callers can log /
/// aggregate. Capped at the richer civ's full surplus to avoid
/// drawing the donor negative.
///
/// Symmetric in argument order: the function self-sorts which
/// side is the donor. Pass either order.
pub fn trade_flow_between(civ_a: &mut Civ, civ_b: &mut Civ) -> Real {
    let s_a = civ_a.surplus;
    let s_b = civ_b.surplus;
    let gap = if s_a > s_b { s_a - s_b } else { s_b - s_a };
    let flow_rate = Real::from(TRADE_FLOW_PER_TICK);
    let raw_flow = gap * flow_rate;
    let flow = raw_flow.max(Real::ZERO);
    if flow <= Real::ZERO {
        return Real::ZERO;
    }
    let (donor, recipient) = if s_a > s_b {
        (civ_a, civ_b)
    } else {
        (civ_b, civ_a)
    };
    let actual = flow.min(donor.surplus);
    donor.surplus = donor.surplus - actual;
    recipient.surplus = recipient.surplus + actual;
    actual
}

/// Compute the war-strength multiplier from the civ's surplus.
/// Returns `1.0 + bonus` where bonus ∈ `[0, SURPLUS_WAR_BONUS_CAP]`,
/// scaled linearly with `surplus / aggregate_pop` (saturating
/// at surplus = pop).
#[must_use]
pub fn surplus_war_strength_modifier(surplus: Real, aggregate_pop: Real) -> Real {
    if aggregate_pop <= Real::ZERO {
        return Real::ONE;
    }
    let ratio = (surplus / aggregate_pop).clamp01();
    let cap = Real::from(SURPLUS_WAR_BONUS_CAP);
    Real::ONE + ratio * cap
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_arith::Pop;

    fn fresh(id: u32, pop: i64) -> Civ {
        Civ::new(id, 0, Pop::from_int(pop))
    }

    #[test]
    fn surplus_grows_above_utilisation_floor_and_caps() {
        let mut civ = fresh(1, 1_000);
        // Below the 0.7 floor: no gain.
        for _ in 0..10 {
            step_surplus(&mut civ, Real::from_ratio(5, 10), 0);
        }
        assert_eq!(civ.surplus, Real::ZERO);
        // Above floor: gain accumulates.
        for _ in 0..50 {
            step_surplus(&mut civ, Real::ONE, 0);
        }
        assert!(civ.surplus > Real::ZERO);
        // Far past ceiling? Should cap at 5× aggregate pop.
        for _ in 0..100_000 {
            step_surplus(&mut civ, Real::ONE, 0);
        }
        let pop_real = Real::from_int(1_000);
        let ceiling = pop_real * Real::from_int(SURPLUS_CEILING_FRAC);
        assert!(
            civ.surplus <= ceiling,
            "surplus {:?} should cap at 5× pop = {:?}",
            civ.surplus,
            ceiling
        );
    }

    #[test]
    fn war_drains_surplus_per_tick_per_war() {
        let mut civ = fresh(1, 1_000);
        // Pre-fill the surplus so the drain is visible.
        civ.surplus = Real::from_int(500);
        // No war: surplus holds (utilisation 0.8 just above floor).
        let before = civ.surplus;
        step_surplus(&mut civ, Real::from_ratio(8, 10), 0);
        assert!(civ.surplus >= before, "no-war frame should not drain");
        // Three-front war: drains faster than the gain at 0.8 util.
        civ.surplus = Real::from_int(500);
        for _ in 0..50 {
            step_surplus(&mut civ, Real::from_ratio(8, 10), 3);
        }
        // Three wars × 0.0015/tick × pop 1000 × 50 ticks = 225 drain;
        // gain at 0.8 util = 1000 × 0.001 × (0.1/0.3) ≈ 0.33/tick × 50 = 17;
        // net loss ≈ 208.
        assert!(
            civ.surplus < Real::from_int(500),
            "3 wars × 50 ticks should drain surplus; got {:?}",
            civ.surplus
        );
    }

    #[test]
    fn catastrophe_drain_consumes_fraction_of_surplus() {
        let mut civ = fresh(1, 1_000);
        civ.surplus = Real::from_int(500);
        drain_surplus_on_catastrophe(&mut civ);
        // 40% drained → 300 remaining.
        let expected = Real::from_int(300);
        let drift = if civ.surplus > expected {
            civ.surplus - expected
        } else {
            expected - civ.surplus
        };
        assert!(
            drift < Real::from_int(1),
            "expected ~300, got {:?}",
            civ.surplus
        );
    }

    #[test]
    fn surplus_food_buffer_saturates_at_bonus_cap() {
        let demand = Pop::from_int(100);
        // No surplus → no bonus.
        let zero = surplus_food_buffer(Real::ZERO, demand);
        assert_eq!(zero, Real::ZERO);
        // surplus = full × demand → full bonus cap.
        let full = surplus_food_buffer(Real::from_int(SURPLUS_FOOD_BUFFER_FULL * 100), demand);
        let cap = Real::from(SURPLUS_FOOD_BUFFER_BONUS);
        let drift = if full > cap { full - cap } else { cap - full };
        assert!(drift < Real::percent(1));
        // 10× full saturates at cap (no overflow).
        let huge = surplus_food_buffer(Real::from_int(10_000), demand);
        let drift_huge = if huge > cap { huge - cap } else { cap - huge };
        assert!(drift_huge < Real::percent(1));
    }

    #[test]
    fn trade_flow_moves_surplus_toward_parity() {
        let mut a = fresh(1, 1_000);
        let mut b = fresh(2, 1_000);
        a.surplus = Real::from_int(1_000);
        b.surplus = Real::from_int(0);
        let flow = trade_flow_between(&mut a, &mut b);
        // flow = (1000 - 0) × 0.0005 = 0.5.
        let expected_flow = Real::from_ratio(5, 10);
        let drift = if flow > expected_flow {
            flow - expected_flow
        } else {
            expected_flow - flow
        };
        assert!(drift < Real::percent(1));
        // Donor lost the flow; recipient gained it.
        let a_drop = Real::from_int(1_000) - a.surplus;
        assert!(a_drop > Real::ZERO);
        assert!(b.surplus > Real::ZERO);
        // Total conserved.
        let total = a.surplus + b.surplus;
        let exp_total = Real::from_int(1_000);
        let total_drift = if total > exp_total {
            total - exp_total
        } else {
            exp_total - total
        };
        assert!(total_drift < Real::percent(1));
    }

    #[test]
    fn trade_flow_self_sorts_donor() {
        // Pass in reverse order — donor should still be identified
        // correctly by surplus magnitude, not arg order.
        let mut a = fresh(1, 1_000);
        let mut b = fresh(2, 1_000);
        a.surplus = Real::ZERO;
        b.surplus = Real::from_int(1_000);
        trade_flow_between(&mut a, &mut b);
        // a was poorer → it gained.
        assert!(a.surplus > Real::ZERO);
        assert!(b.surplus < Real::from_int(1_000));
    }

    #[test]
    fn surplus_war_strength_modifier_within_bounds() {
        let pop = Real::from_int(1_000);
        // No surplus → baseline 1.0.
        let one = surplus_war_strength_modifier(Real::ZERO, pop);
        let drift = Real::ONE - one;
        assert!(drift.abs() < Real::from_ratio(1, 1000));
        // Surplus = pop → full +cap bonus.
        let full = surplus_war_strength_modifier(pop, pop);
        let cap = Real::from(SURPLUS_WAR_BONUS_CAP);
        let expected = Real::ONE + cap;
        let drift_full = if full > expected {
            full - expected
        } else {
            expected - full
        };
        assert!(drift_full < Real::percent(1));
        // Surplus > pop saturates at the cap.
        let over = surplus_war_strength_modifier(pop * Real::from_int(10), pop);
        let drift_over = if over > expected {
            over - expected
        } else {
            expected - over
        };
        assert!(drift_over < Real::percent(1));
    }
}
