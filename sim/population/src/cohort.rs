//! 4-bracket population cohort and per-bracket arithmetic.
//!
//! `Cohort` carries `infant / juvenile / fertile / elder` counts
//! and the civ-membership tag. All bracket-scoped operations —
//! totals, weighted demand, distribution, scaling, migration,
//! merging, shrinking, flooring — live here so the per-tick
//! `PopulationDynamics::step_with_capacity` (in `dynamics.rs`) only
//! has to talk about birth/survival/aging math.

use sim_arith::{Pop, Real};
use sim_species::PopulationBiology;

/// 4-bracket population cohort. Replaces the earlier scalar
/// `count` with explicit age structure: infants, juveniles,
/// fertile adults, and post-reproductive elders. Only the fertile
/// bracket reproduces; brackets age forward via per-tick
/// transition rates derived from the species' bracket fractions
/// times its lifespan.
#[derive(Debug, Clone)]
pub struct Cohort {
    pub infant: Pop,
    pub juvenile: Pop,
    pub fertile: Pop,
    pub elder: Pop,
    /// Civ membership tag. `Some(civ_id)` for cohorts attached
    /// to a civ; `None` for stateless population (post-collapse
    /// remnants).
    pub civ_membership: Option<u32>,
}

impl Cohort {
    /// Construct a cohort with the entire initial count placed in
    /// the fertile bracket. Founders are by definition adults; the
    /// per-tick step produces infants and ages them up over the
    /// first generation.
    pub fn new(initial_count: Pop) -> Self {
        Self {
            infant: Pop::ZERO,
            juvenile: Pop::ZERO,
            fertile: initial_count,
            elder: Pop::ZERO,
            civ_membership: None,
        }
    }

    pub fn with_civ(initial_count: Pop, civ_id: u32) -> Self {
        let mut c = Self::new(initial_count);
        c.civ_membership = Some(civ_id);
        c
    }

    /// Empty cohort. Useful for incrementally accumulating a
    /// per-cell breakdown via `add_to_fertile` and friends.
    pub fn empty() -> Self {
        Self {
            infant: Pop::ZERO,
            juvenile: Pop::ZERO,
            fertile: Pop::ZERO,
            elder: Pop::ZERO,
            civ_membership: None,
        }
    }

    pub fn empty_with_civ(civ_id: u32) -> Self {
        let mut c = Self::empty();
        c.civ_membership = Some(civ_id);
        c
    }

    /// Sum of all brackets — the bracket-agnostic total
    /// "population" of the cohort.
    pub fn total(&self) -> Pop {
        self.infant + self.juvenile + self.fertile + self.elder
    }

    /// Food-weighted demand: `Σ bracket × food_multiplier`. The
    /// per-cell capacity formula compares this to capacity rather
    /// than raw `total()`, so an age-skewed cohort (lots of
    /// dependents) feels stress harder.
    pub fn weighted_demand(&self, biology: &PopulationBiology) -> Pop {
        self.weighted_demand_from_multipliers(&biology.food_multipliers)
    }

    /// Same as `weighted_demand` but takes the multiplier array
    /// directly. Lets callers that hold `PopulationDynamics` (which
    /// mirrors `biology.food_multipliers`) compute demand without
    /// a second `PopulationBiology` lookup.
    pub fn weighted_demand_from_multipliers(&self, m: &[Real; 4]) -> Pop {
        self.infant * m[0] + self.juvenile * m[1] + self.fertile * m[2] + self.elder * m[3]
    }

    /// Add a population delta to the fertile bracket. Used by
    /// callers that just have a scalar pop (e.g. nomad absorption
    /// at civ founding) and want to deposit it as adult founders.
    pub fn add_fertile(&mut self, delta: Pop) {
        self.fertile = self.fertile + delta;
    }

    /// Distribute a scalar pop across all four brackets per the
    /// species' bracket fractions. Used when a cohort's worth of
    /// pop arrives without age structure (e.g. nomad absorption
    /// post-founding, where the absorbed pop was itself a mixed-age
    /// nomadic group). The infant/juvenile/elder splits are
    /// deposited at full count, not survival-discounted, since the
    /// per-tick step will apply the next-tick mortality.
    pub fn deposit_distributed(&mut self, count: Pop, biology: &PopulationBiology) {
        if count <= Pop::ZERO {
            return;
        }
        let i = count * biology.infant_fraction;
        let j = count * biology.maturity_fraction;
        let e = count * biology.eldership_fraction;
        let f = count - i - j - e;
        self.infant = self.infant + i;
        self.juvenile = self.juvenile + j;
        self.fertile = self.fertile + f;
        self.elder = self.elder + e;
    }

    /// In-place scalar multiply applied identically to every
    /// bracket. Used by territory contraction (lose X% of every
    /// bracket proportionally) and similar mass-conserving shrink
    /// operations.
    pub fn scale_in_place(&mut self, factor: Real) {
        self.infant = self.infant * factor;
        self.juvenile = self.juvenile * factor;
        self.fertile = self.fertile * factor;
        self.elder = self.elder * factor;
    }

    /// Split off a fraction of every bracket into a new cohort.
    /// Mass-conservative: the moved cohort's brackets are removed
    /// from self. Used by territory expansion + civ founding to
    /// seed a new cell with a slice of an existing centroid.
    #[must_use]
    pub fn split_off_fraction(&mut self, fraction: Real) -> Cohort {
        let f = fraction.clamp01();
        let moved = Cohort {
            infant: self.infant * f,
            juvenile: self.juvenile * f,
            fertile: self.fertile * f,
            elder: self.elder * f,
            civ_membership: self.civ_membership,
        };
        let keep = Real::ONE - f;
        self.scale_in_place(keep);
        moved
    }

    /// Migrate `fertile_to_move` adults from `self` into `dst`,
    /// dragging dependent infants + juveniles proportionally to
    /// the source cohort's own dependent-to-fertile ratio. Elders
    /// stay in `self`. This implements the family-unit migration
    /// policy: a productive-age adult leaving a cell takes their
    /// own dependents along, but post-reproductive elders are too
    /// rooted (or too senescent) to migrate. Returns the total
    /// number of people that moved (fertile + infants + juveniles).
    pub fn migrate_family_to(&mut self, dst: &mut Cohort, fertile_to_move: Pop) -> Pop {
        let move_f = fertile_to_move.min(self.fertile).max(Pop::ZERO);
        if move_f <= Pop::ZERO || self.fertile <= Pop::ZERO {
            return Pop::ZERO;
        }
        let infant_ratio = self.infant / self.fertile;
        let juvenile_ratio = self.juvenile / self.fertile;
        let move_i = (move_f * infant_ratio).min(self.infant).max(Pop::ZERO);
        let move_j = (move_f * juvenile_ratio).min(self.juvenile).max(Pop::ZERO);
        self.fertile = self.fertile - move_f;
        self.infant = self.infant - move_i;
        self.juvenile = self.juvenile - move_j;
        dst.fertile = dst.fertile + move_f;
        dst.infant = dst.infant + move_i;
        dst.juvenile = dst.juvenile + move_j;
        move_f + move_i + move_j
    }

    /// Migrate a proportional slice of every age bracket to `dst`,
    /// preserving the source cohort's age structure. Unlike
    /// `migrate_family_to` — which models a family unit relocating
    /// (fertile + their dependents move, elders stay rooted) — this
    /// is the right primitive for the slow, sustained intra-civ
    /// rebalancing flow between adjacent claimed cells: drain only
    /// the productive brackets and source cells demographically
    /// collapse (elders age out without fertile to replace them, the
    /// cell falls below the prune floor, and saturated cores
    /// gradually hollow into pruned holes inside contiguous
    /// territory).
    ///
    /// `total_to_move` is the target headcount across all brackets;
    /// every bracket is scaled by `total_to_move / self.total()` so
    /// the move respects the current age mix. Returns the actual
    /// total that moved.
    pub fn migrate_balanced_to(&mut self, dst: &mut Cohort, total_to_move: Pop) -> Pop {
        let total = self.total();
        if total <= Pop::ZERO || total_to_move <= Pop::ZERO {
            return Pop::ZERO;
        }
        let move_total = if total_to_move > total {
            total
        } else {
            total_to_move
        };
        let frac = move_total / total;
        let move_i = self.infant * frac;
        let move_j = self.juvenile * frac;
        let move_f = self.fertile * frac;
        let move_e = self.elder * frac;
        self.infant = (self.infant - move_i).max(Pop::ZERO);
        self.juvenile = (self.juvenile - move_j).max(Pop::ZERO);
        self.fertile = (self.fertile - move_f).max(Pop::ZERO);
        self.elder = (self.elder - move_e).max(Pop::ZERO);
        dst.infant = dst.infant + move_i;
        dst.juvenile = dst.juvenile + move_j;
        dst.fertile = dst.fertile + move_f;
        dst.elder = dst.elder + move_e;
        move_i + move_j + move_f + move_e
    }

    /// Add another cohort's brackets into self in-place. Civ
    /// membership is preserved on `self`. Used by refugee
    /// merging — when a civ sheds a cell, the shed cohort's pop
    /// is folded into a retained cell.
    pub fn merge_in(&mut self, other: &Cohort) {
        self.infant = self.infant + other.infant;
        self.juvenile = self.juvenile + other.juvenile;
        self.fertile = self.fertile + other.fertile;
        self.elder = self.elder + other.elder;
    }

    /// Shrink every bracket proportionally so the cohort's total
    /// becomes `target`. No-op if total is already <= target.
    /// Returns the number of people lost. Used by catastrophes
    /// that combine a fractional pop loss with a minimum-pop floor:
    /// `target = (total() × (1 - frac)).max(floor)` on the caller
    /// side, then this method preserves age structure.
    pub fn shrink_to(&mut self, target: Pop) -> Pop {
        let before = self.total();
        if before <= target || before <= Pop::ZERO {
            return Pop::ZERO;
        }
        let scale = target / before;
        self.scale_in_place(scale);
        before - target
    }

    /// Floor every bracket at zero. Defensive helper for code
    /// paths that subtract before checking sign (e.g. war
    /// casualties).
    pub fn floor_at_zero(&mut self) {
        if self.infant < Pop::ZERO {
            self.infant = Pop::ZERO;
        }
        if self.juvenile < Pop::ZERO {
            self.juvenile = Pop::ZERO;
        }
        if self.fertile < Pop::ZERO {
            self.fertile = Pop::ZERO;
        }
        if self.elder < Pop::ZERO {
            self.elder = Pop::ZERO;
        }
    }
}
