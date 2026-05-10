//! `sim-population` — 4-bracket cohort step + biology-derived rates.
//!
//! Pop is tracked per civ as a `Cohort` split into four brackets:
//! `infant / juvenile / fertile / elder`. Only the fertile bracket
//! produces births; only the fertile bracket carries economic /
//! military weight. Each tick, individuals age out at a rate
//! determined by their bracket's duration in months
//! (= `lifespan_years × bracket_fraction × 12`), die at a rate
//! derived from the species' per-bracket survival fraction over
//! that duration, and (for fertile) produce births = `clutch_size /
//! fertile_window_months × fertile.count`.
//!
//! No homo-sapiens calibration baseline. Rates fall out of biology:
//! a clutch=200, lifespan=4yr, maturity=10% species lands on
//! ~14 births/fertile-adult/month inherently; a clutch=1,
//! lifespan=200yr, maturity=20% species lands on ~0.0005/month.
//! Both numerically stable, both derived from the same formulas.
//!
//! Per-bracket food multipliers in `PopulationBiology::food_multipliers`
//! mean a cell's effective demand under N infants + M juveniles +
//! K fertile + L elders is `0.3N + 0.6M + 1.0K + 0.9L`, not just
//! `N + M + K + L`. The food-security ratio compares this weighted
//! demand to capacity — so an age-skewed cohort (lots of
//! dependents) feels stress harder than a fertile-heavy one.

#![allow(clippy::module_name_repetitions)]

use sim_arith::transcendental::{exp, ln};
use sim_arith::Real;
use sim_species::PopulationBiology;

/// 4-bracket population cohort. Replaces the earlier scalar
/// `count` with explicit age structure: infants, juveniles,
/// fertile adults, and post-reproductive elders. Only the fertile
/// bracket reproduces; brackets age forward via per-tick
/// transition rates derived from the species' bracket fractions
/// times its lifespan.
#[derive(Debug, Clone)]
pub struct Cohort {
    pub infant: Real,
    pub juvenile: Real,
    pub fertile: Real,
    pub elder: Real,
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
    pub fn new(initial_count: Real) -> Self {
        Self {
            infant: Real::ZERO,
            juvenile: Real::ZERO,
            fertile: initial_count,
            elder: Real::ZERO,
            civ_membership: None,
        }
    }

    pub fn with_civ(initial_count: Real, civ_id: u32) -> Self {
        let mut c = Self::new(initial_count);
        c.civ_membership = Some(civ_id);
        c
    }

    /// Empty cohort. Useful for incrementally accumulating a
    /// per-cell breakdown via `add_to_fertile` and friends.
    pub fn empty() -> Self {
        Self {
            infant: Real::ZERO,
            juvenile: Real::ZERO,
            fertile: Real::ZERO,
            elder: Real::ZERO,
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
    pub fn total(&self) -> Real {
        self.infant + self.juvenile + self.fertile + self.elder
    }

    /// Food-weighted demand: `Σ bracket × food_multiplier`. The
    /// per-cell capacity formula compares this to capacity rather
    /// than raw `total()`, so an age-skewed cohort (lots of
    /// dependents) feels stress harder.
    pub fn weighted_demand(&self, biology: &PopulationBiology) -> Real {
        self.weighted_demand_from_multipliers(&biology.food_multipliers)
    }

    /// Same as `weighted_demand` but takes the multiplier array
    /// directly. Lets callers that hold `PopulationDynamics` (which
    /// mirrors `biology.food_multipliers`) compute demand without
    /// a second `PopulationBiology` lookup.
    pub fn weighted_demand_from_multipliers(&self, m: &[Real; 4]) -> Real {
        self.infant * m[0] + self.juvenile * m[1] + self.fertile * m[2] + self.elder * m[3]
    }

    /// Add a population delta to the fertile bracket. Used by
    /// callers that just have a scalar pop (e.g. nomad absorption
    /// at civ founding) and want to deposit it as adult founders.
    pub fn add_fertile(&mut self, delta: Real) {
        self.fertile = self.fertile + delta;
    }

    /// Distribute a scalar pop across all four brackets per the
    /// species' bracket fractions. Used when a cohort's worth of
    /// pop arrives without age structure (e.g. nomad absorption
    /// post-founding, where the absorbed pop was itself a mixed-age
    /// nomadic group). The infant/juvenile/elder splits are
    /// deposited at full count, not survival-discounted, since the
    /// per-tick step will apply the next-tick mortality.
    pub fn deposit_distributed(&mut self, count: Real, biology: &PopulationBiology) {
        if count <= Real::ZERO {
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
        let f = fraction.max(Real::ZERO).min(Real::ONE);
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
    pub fn migrate_family_to(&mut self, dst: &mut Cohort, fertile_to_move: Real) -> Real {
        let move_f = fertile_to_move.min(self.fertile).max(Real::ZERO);
        if move_f <= Real::ZERO || self.fertile <= Real::ZERO {
            return Real::ZERO;
        }
        let infant_ratio = self.infant / self.fertile;
        let juvenile_ratio = self.juvenile / self.fertile;
        let move_i = (move_f * infant_ratio).min(self.infant).max(Real::ZERO);
        let move_j = (move_f * juvenile_ratio).min(self.juvenile).max(Real::ZERO);
        self.fertile = self.fertile - move_f;
        self.infant = self.infant - move_i;
        self.juvenile = self.juvenile - move_j;
        dst.fertile = dst.fertile + move_f;
        dst.infant = dst.infant + move_i;
        dst.juvenile = dst.juvenile + move_j;
        move_f + move_i + move_j
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
    pub fn shrink_to(&mut self, target: Real) -> Real {
        let before = self.total();
        if before <= target || before <= Real::ZERO {
            return Real::ZERO;
        }
        let scale = target / before;
        self.scale_in_place(scale);
        before - target
    }

    /// Floor every bracket at zero. Defensive helper for code
    /// paths that subtract before checking sign (e.g. war
    /// casualties).
    pub fn floor_at_zero(&mut self) {
        if self.infant < Real::ZERO {
            self.infant = Real::ZERO;
        }
        if self.juvenile < Real::ZERO {
            self.juvenile = Real::ZERO;
        }
        if self.fertile < Real::ZERO {
            self.fertile = Real::ZERO;
        }
        if self.elder < Real::ZERO {
            self.elder = Real::ZERO;
        }
    }
}

/// Per-tick transition + survival rates for a 4-bracket step,
/// derived from `PopulationBiology` + `lifespan_years`. Replaces
/// the homo-sapiens-calibrated 3%/yr birth, 2.8%/yr death heuristic
/// with biology-derived rates: `birth = clutch_size /
/// fertile_window_months`, per-bracket survival from the species'
/// per-bracket survival fractions converted to per-tick mortality
/// via `mortality_per_tick = -ln(survival) / bracket_months`.
///
/// All rates are per-month (1 tick = 1 month), pinned to the
/// `BASELINE_MONTHS_PER_YEAR = 12` calibration baseline. Planet
/// orbital period drives display (year-of-tick) but not
/// per-tick rate calibration — a 9-month planet's tick is still
/// the same biological "month" as a 12-month planet's.
#[derive(Debug, Clone, Copy)]
pub struct PopulationDynamics {
    /// Births per fertile adult per tick. `clutch_size /
    /// fertile_window_months`.
    pub birth_rate: Real,
    /// Per-tick survival probability per bracket.
    pub infant_survival_per_tick: Real,
    pub juvenile_survival_per_tick: Real,
    pub fertile_survival_per_tick: Real,
    pub elder_survival_per_tick: Real,
    /// Per-tick aging-out rate per bracket (fraction of bracket
    /// that promotes to the next stage). `1 / bracket_duration_months`.
    pub infant_to_juvenile: Real,
    pub juvenile_to_fertile: Real,
    pub fertile_to_elder: Real,
    /// Stress amplification factor for mortality under food
    /// shortfall. Multiplies the fraction-dying-per-tick by
    /// `(1 + stress × stress_factor)`. Range [2, 5].
    pub stress_factor: Real,
    /// Mirror of `biology.food_multipliers` so the step doesn't
    /// have to re-thread biology.
    pub food_multipliers: [Real; 4],
    /// Per-bracket per-tick mortality reduction from tech
    /// (sanitation, healing, medicine). Each entry is a fraction
    /// in `[0, 1]` that scales the bracket's per-tick mortality
    /// down by `(1 - reduction)`. A reduction of 0.20 cuts that
    /// bracket's deaths-per-tick by 20%. Defaults to all zeros so
    /// pre-tech civs (and test callers) pass through unchanged.
    /// Order: `[infant, juvenile, fertile, elder]`. Refreshed by
    /// the civ from its unlocked tools each tick before stepping.
    pub mortality_reduction: [Real; 4],
    /// Multiplier on per-tick births from tech (nutrition,
    /// healing, modern medicine). `Real::ONE` = no change;
    /// `Real::from_ratio(150, 100)` = 50% more births per fertile.
    /// Distinct from `mortality_reduction[0]` (infant deaths
    /// *after* birth) — this is the conception-through-viable-
    /// birth gate. Defaults to `1.0` so pre-tech civs and test
    /// callers pass through unchanged. Refreshed by the civ from
    /// its unlocked tools each tick before stepping.
    pub birth_rate_multiplier: Real,
}

impl PopulationDynamics {
    /// Derive per-tick rates from species biology. Pure function
    /// of `(biology, lifespan_years, cognition, sociality)` — same
    /// inputs always produce the same dynamics struct.
    ///
    /// Bracket durations come from `lifespan_years × fraction × 12`
    /// (BASELINE months). Per-tick survival comes from
    /// `exp(ln(survival) / months)` — equivalent to the
    /// time-resampling identity `S = s^(1/months)` but stable in
    /// Q32.32. Per-tick aging is `1 / months`. Birth rate is
    /// `clutch_size / fertile_window_months` directly, no extra
    /// scaling.
    ///
    /// Fertile-bracket survival: high baseline (0.85-0.95) modulated
    /// by cognition (smarter species lose fewer adults to
    /// medicine + agriculture). Elder bracket: senescent baseline
    /// 0.30 over the elder window, no cognition discount (death is
    /// programmed, not preventable by hygiene).
    pub fn for_species(
        biology: &PopulationBiology,
        lifespan_years: Real,
        cognition: Real,
        sociality: Real,
    ) -> Self {
        let baseline_months_per_year =
            Real::from_int(i64::try_from(protocol::BASELINE_MONTHS_PER_YEAR).unwrap_or(12));
        // Defensive: lifespan can't be sub-monthly.
        let lifespan = lifespan_years.max(Real::ONE);
        let infant_months = (lifespan * biology.infant_fraction * baseline_months_per_year)
            .max(Real::ONE);
        let juvenile_months = (lifespan * biology.maturity_fraction * baseline_months_per_year)
            .max(Real::ONE);
        let fertile_months =
            (lifespan * biology.fertile_fraction() * baseline_months_per_year).max(Real::ONE);
        // Eldership can be zero by construction; clamp the
        // attrition denominator to >= 1 so divides don't blow up.
        let elder_months_raw = lifespan * biology.eldership_fraction * baseline_months_per_year;
        let elder_months = elder_months_raw.max(Real::ONE);
        // Birth rate: clutch_size offspring per fertile lifespan,
        // averaged across the fertile window. births / fertile-adult
        // / tick = clutch / fertile_months.
        let birth_rate = biology.clutch_size / fertile_months;
        // Per-tick survival via per_tick = exp(ln(window_survival)
        // / months). For window_survival in (0, 1] this returns a
        // per-tick fraction in (0, 1].
        let to_per_tick = |window_survival: Real, months: Real| -> Real {
            let s = window_survival.max(Real::from_ratio(1, 1000)).min(Real::ONE);
            let log_s = ln(s);
            let exponent = log_s / months;
            exp(exponent).max(Real::ZERO).min(Real::ONE)
        };
        // Fertile baseline survival over the entire fertile window:
        // 0.85 + 0.10 * cognition (range [0.85, 0.95]).
        let cog_clamped = cognition.max(Real::ZERO).min(Real::ONE);
        let fertile_window_survival =
            Real::from_ratio(85, 100) + cog_clamped * Real::from_ratio(10, 100);
        // Elder window survival: baseline 0.30 (senescence dominates;
        // most species kill their elders within the elder window).
        let elder_window_survival = Real::from_ratio(30, 100);
        let infant_survival_per_tick = to_per_tick(biology.infant_survival, infant_months);
        let juvenile_survival_per_tick = to_per_tick(biology.juvenile_survival, juvenile_months);
        let fertile_survival_per_tick = to_per_tick(fertile_window_survival, fertile_months);
        let elder_survival_per_tick = if biology.eldership_fraction > Real::ZERO {
            to_per_tick(elder_window_survival, elder_months)
        } else {
            // No elder bracket: anyone who lands in elder dies the
            // same tick (legacy from fertile_to_elder = 0 anyway).
            Real::ZERO
        };
        // Aging-out rates: 1 / months_in_bracket.
        let infant_to_juvenile = Real::ONE / infant_months;
        let juvenile_to_fertile = Real::ONE / juvenile_months;
        let fertile_to_elder = if biology.eldership_fraction > Real::ZERO {
            Real::ONE / fertile_months
        } else {
            // No elder bracket: fertile -> dead directly via the
            // fertile_survival_per_tick rate; no aging promotion.
            Real::ZERO
        };
        // Stress factor (carried over from earlier model). Mutual
        // aid + adaptive behaviour buffer the death amplification.
        // Centred at 4.0; range [2, 5].
        let soc_clamped = sociality.max(Real::ZERO).min(Real::ONE);
        let stress_factor =
            (Real::from_int(5) - soc_clamped - cog_clamped).max(Real::from_int(2));
        Self {
            birth_rate,
            infant_survival_per_tick,
            juvenile_survival_per_tick,
            fertile_survival_per_tick,
            elder_survival_per_tick,
            infant_to_juvenile,
            juvenile_to_fertile,
            fertile_to_elder,
            stress_factor,
            food_multipliers: biology.food_multipliers,
            mortality_reduction: [Real::ZERO; 4],
            birth_rate_multiplier: Real::ONE,
        }
    }

    /// Test-only neutral defaults. A cohort with these rates and
    /// adequate capacity stays roughly stable. Used by tests that
    /// don't care about per-species realism.
    pub fn earth_like_default() -> Self {
        Self {
            birth_rate: Real::from_ratio(3, 100 * 12),
            infant_survival_per_tick: Real::from_ratio(995, 1000),
            juvenile_survival_per_tick: Real::from_ratio(998, 1000),
            fertile_survival_per_tick: Real::from_ratio(999, 1000),
            elder_survival_per_tick: Real::from_ratio(990, 1000),
            infant_to_juvenile: Real::from_ratio(1, 24),
            juvenile_to_fertile: Real::from_ratio(1, 200),
            fertile_to_elder: Real::from_ratio(1, 600),
            stress_factor: Real::from_int(4),
            food_multipliers: [
                Real::from_ratio(30, 100),
                Real::from_ratio(60, 100),
                Real::ONE,
                Real::from_ratio(90, 100),
            ],
            mortality_reduction: [Real::ZERO; 4],
            birth_rate_multiplier: Real::ONE,
        }
    }

    /// Step the cohort one tick under capacity-coupled dynamics.
    /// Order of operations:
    /// 1. Births: `fertile × birth_rate` newborns enter `infant`.
    /// 2. Survival: each bracket multiplied by its per-tick
    ///    survival. Stress (food shortfall) amplifies mortality
    ///    by `(1 + stress × stress_factor)` applied as a
    ///    multiplicative penalty on the survival fraction.
    /// 3. Aging: a fraction of each non-fertile bracket promotes
    ///    to the next stage. Elder bracket has no destination
    ///    (attrition is already in survival).
    ///
    /// `food_security ∈ [0, 1]` is computed against weighted demand
    /// (per-bracket food multipliers × counts) vs capacity. `0`
    /// drives all brackets toward extinction via the stress-
    /// amplified death term and zero births.
    pub fn step_with_capacity(&self, cohort: &mut Cohort, capacity: Real) {
        let demand = self.food_multipliers[0] * cohort.infant
            + self.food_multipliers[1] * cohort.juvenile
            + self.food_multipliers[2] * cohort.fertile
            + self.food_multipliers[3] * cohort.elder;
        let security = food_security(demand, capacity);
        let stress = Real::ONE - security;
        // Two mortality terms:
        //   (1) Multiplicative stress amplification on the baseline
        //       per-tick mortality. (1 + stress × stress_factor).
        //       Models "an already-vulnerable bracket gets hit
        //       harder when food is short".
        //   (2) Additive starvation mortality: `stress ×
        //       STARVATION_PER_TICK`. Models "no food kills you in
        //       months regardless of how healthy you were". Without
        //       this, the high-baseline-survival fertile bracket
        //       takes years to collapse even at zero capacity, which
        //       isn't biologically realistic.
        // Per-bracket starvation severity scales with bracket
        // vulnerability — infants and juveniles can't withstand
        // food shortfall as well as adults; elders are fragile too.
        let amp = Real::ONE + stress * self.stress_factor;
        let starvation = stress * Real::from_ratio(10, 100);
        let starvation_infant = starvation * Real::from_ratio(20, 10);
        let starvation_juvenile = starvation * Real::from_ratio(15, 10);
        let starvation_fertile = starvation;
        let starvation_elder = starvation * Real::from_ratio(15, 10);
        // Tech mortality reduction: cuts the per-tick mortality
        // (= 1 - survival) by `(1 - reduction)` before stress
        // amplification + starvation are layered on. Sanitation /
        // healing tools reduce the *baseline* deaths-per-tick that
        // a bracket would otherwise see, and the cuts compound with
        // (rather than replacing) the stress-and-starvation terms.
        let combine = |s: Real, extra: Real, reduction: Real| -> Real {
            let r = reduction.max(Real::ZERO).min(Real::ONE);
            let reduced_baseline = (Real::ONE - s) * (Real::ONE - r) * amp;
            let total = (reduced_baseline + extra).min(Real::ONE);
            (Real::ONE - total).max(Real::ZERO)
        };
        let infant_s = combine(
            self.infant_survival_per_tick,
            starvation_infant,
            self.mortality_reduction[0],
        );
        let juvenile_s = combine(
            self.juvenile_survival_per_tick,
            starvation_juvenile,
            self.mortality_reduction[1],
        );
        let fertile_s = combine(
            self.fertile_survival_per_tick,
            starvation_fertile,
            self.mortality_reduction[2],
        );
        let elder_s = combine(
            self.elder_survival_per_tick,
            starvation_elder,
            self.mortality_reduction[3],
        );
        // Births: only the fertile bracket reproduces, and
        // food_security suppresses births under shortfall. Tech
        // (nutrition, healing, modern medicine) lifts the
        // conception-through-viable-birth gate via
        // `birth_rate_multiplier`; defaults to `1.0` for pre-tech
        // civs.
        let births = cohort.fertile * self.birth_rate * self.birth_rate_multiplier * security;
        // Apply survival first, then aging. The order matters:
        // applying survival first means a starving bracket loses
        // people who would have aged up.
        let infant_after_survival = cohort.infant * infant_s + births;
        let juvenile_after_survival = cohort.juvenile * juvenile_s;
        let fertile_after_survival = cohort.fertile * fertile_s;
        let elder_after_survival = cohort.elder * elder_s;
        // Aging promotions: fraction of each bracket transitions up.
        let infant_to_juv_count = infant_after_survival * self.infant_to_juvenile;
        let juv_to_fert_count = juvenile_after_survival * self.juvenile_to_fertile;
        let fert_to_eld_count = fertile_after_survival * self.fertile_to_elder;
        cohort.infant = infant_after_survival - infant_to_juv_count;
        cohort.juvenile = juvenile_after_survival + infant_to_juv_count - juv_to_fert_count;
        cohort.fertile = fertile_after_survival + juv_to_fert_count - fert_to_eld_count;
        cohort.elder = elder_after_survival + fert_to_eld_count;
        cohort.floor_at_zero();
    }

    /// Uncoupled step: capacity is treated as effectively infinite.
    /// Births and survival apply at their unstressed rates. Used
    /// by cohorts whose region has no biological-stock-driven
    /// capacity, and by tests.
    pub fn step(&self, cohort: &mut Cohort) {
        // Pass a capacity well above any plausible cohort size so
        // food_security == 1 and the stress term is zero.
        let big = (cohort.total() * Real::from_int(1_000)).max(Real::from_int(1_000_000));
        self.step_with_capacity(cohort, big);
    }

    /// Expected life span at birth, in months, under unstressed
    /// (well-fed) conditions and currently-applied tech mortality
    /// reduction. Pure function of the dynamics struct — no
    /// allocation, no mutation, deterministic in `(survivals,
    /// aging rates, mortality_reduction)`.
    ///
    /// Model: each bracket is a competing-hazards problem where
    /// per tick an individual either dies (rate `1 - s`),
    /// promotes to the next bracket (rate `s × r`), or stays
    /// (rate `s × (1 - r)`). The expected sojourn time in a
    /// bracket is `1 / (1 - s × (1 - r))`; the probability of
    /// exiting alive (= reaching the next bracket) is
    /// `s × r / (1 - s × (1 - r))`.
    ///
    /// Total life expectancy at birth is then the sum over
    /// brackets of `P(reach this bracket) × E[time in bracket]`.
    /// Stress / starvation terms are excluded — this is the
    /// neutral-environment life expectancy a Civ would converge
    /// to with adequate food. Callers wanting the stressed
    /// version can multiply mortality terms by `(1 + stress ×
    /// stress_factor)` first.
    ///
    /// Tech effects flow through `mortality_reduction`: per-tick
    /// mortality `(1 - s)` is scaled by `(1 - reduction)` before
    /// the sojourn / probability formulas, so a high-tier
    /// medical civ's expectancy reflects sanitation + medicine
    /// directly.
    #[must_use]
    pub fn life_expectancy_months(&self) -> Real {
        // Apply tech mortality reduction to the per-tick
        // survivals — same shape the step's `combine` uses
        // without stress amplification (so this is the neutral-
        // environment expectancy).
        let apply_reduction = |s: Real, r: Real| -> Real {
            let red = r.max(Real::ZERO).min(Real::ONE);
            Real::ONE - (Real::ONE - s) * (Real::ONE - red)
        };
        let s_i = apply_reduction(self.infant_survival_per_tick, self.mortality_reduction[0]);
        let s_j = apply_reduction(self.juvenile_survival_per_tick, self.mortality_reduction[1]);
        let s_f = apply_reduction(self.fertile_survival_per_tick, self.mortality_reduction[2]);
        let s_e = apply_reduction(self.elder_survival_per_tick, self.mortality_reduction[3]);
        // Per-bracket sojourn + reach-next-bracket probabilities.
        // For brackets with a non-zero aging-out rate `r`:
        //   stay_alive_per_tick = s × (1 - r)
        //   exit_alive_per_tick = s × r
        //   E[time in bracket] = 1 / (1 - stay_alive_per_tick)
        //   P(reach next bracket | enter this) = exit_alive_per_tick × E[time]
        // For the elder bracket (no destination): P(exit alive) = 0,
        //   E[time in elder | enter] = 1 / (1 - s_e).
        let bracket_stats = |s: Real, r: Real| -> (Real, Real) {
            let stay_alive = s * (Real::ONE - r);
            let one_minus_stay = (Real::ONE - stay_alive).max(Real::from_ratio(1, 1_000_000));
            let mean_time = Real::ONE / one_minus_stay;
            let exit_alive = s * r;
            let p_reach_next = exit_alive * mean_time;
            (mean_time, p_reach_next.min(Real::ONE))
        };
        let (t_i, p_to_juv) = bracket_stats(s_i, self.infant_to_juvenile);
        let (t_j, p_to_fert) = bracket_stats(s_j, self.juvenile_to_fertile);
        let (t_f, p_to_elder) = bracket_stats(s_f, self.fertile_to_elder);
        // Elder bracket: no aging-out; mean time = 1 / (1 - s_e).
        let t_e = if s_e < Real::ONE {
            Real::ONE / (Real::ONE - s_e).max(Real::from_ratio(1, 1_000_000))
        } else {
            Real::ZERO
        };
        // Compose: each bracket contributes P(reach it) × E[time in it].
        // P(reach infant) = 1 by construction (life starts there).
        // P(reach juv) = p_to_juv. P(reach fert) = p_to_juv × p_to_fert.
        // P(reach elder) = p_to_juv × p_to_fert × p_to_elder.
        let p_juv = p_to_juv;
        let p_fert = p_juv * p_to_fert;
        let p_elder = p_fert * p_to_elder;
        t_i + p_juv * t_j + p_fert * t_f + p_elder * t_e
    }
}

/// food security: `1 − max(0, demand / capacity − 1)`, clamped
/// to `[0, 1]`. Returns 0 when capacity ≤ 0 so an uninhabitable
/// region drives the cohort to extinction through the dynamics. The
/// formula penalises overshoot symmetrically — a cohort whose
/// weighted demand equals capacity gets `food_security = 1`; one
/// at 2× capacity gets 0.
pub fn food_security(demand: Real, capacity: Real) -> Real {
    if capacity <= Real::ZERO {
        return Real::ZERO;
    }
    let ratio = demand / capacity;
    let overshoot = ratio - Real::ONE;
    let stress = if overshoot > Real::ZERO {
        overshoot
    } else {
        Real::ZERO
    };
    let raw = Real::ONE - stress;
    raw.max(Real::ZERO).min(Real::ONE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_biology() -> PopulationBiology {
        // Mid-r/K species: clutch=10, lifespan=40yr,
        // infant=5%, maturity=20%, elder=15%, fertile=60%.
        // Survivals: infant=0.5, juvenile=0.85.
        PopulationBiology {
            clutch_size: Real::from_int(10),
            infant_fraction: Real::from_ratio(5, 100),
            maturity_fraction: Real::from_ratio(20, 100),
            eldership_fraction: Real::from_ratio(15, 100),
            infant_survival: Real::from_ratio(50, 100),
            juvenile_survival: Real::from_ratio(85, 100),
            food_multipliers: [
                Real::from_ratio(30, 100),
                Real::from_ratio(60, 100),
                Real::ONE,
                Real::from_ratio(90, 100),
            ],
        }
    }

    #[test]
    fn cohort_total_sums_brackets() {
        let mut c = Cohort::empty();
        c.infant = Real::from_int(10);
        c.juvenile = Real::from_int(20);
        c.fertile = Real::from_int(50);
        c.elder = Real::from_int(15);
        assert_eq!(c.total(), Real::from_int(95));
    }

    #[test]
    fn cohort_new_seeds_fertile_only() {
        let c = Cohort::new(Real::from_int(100));
        assert_eq!(c.infant, Real::ZERO);
        assert_eq!(c.juvenile, Real::ZERO);
        assert_eq!(c.fertile, Real::from_int(100));
        assert_eq!(c.elder, Real::ZERO);
    }

    #[test]
    fn for_species_birth_rate_matches_clutch_over_fertile_window() {
        let biology = test_biology();
        let lifespan = Real::from_int(40);
        let dyn_ = PopulationDynamics::for_species(
            &biology,
            lifespan,
            Real::from_ratio(50, 100),
            Real::from_ratio(50, 100),
        );
        // fertile_window_months = 40 × 0.60 × 12 = 288.
        // birth_rate = 10 / 288 ≈ 0.0347.
        let expected = Real::from_int(10) / Real::from_int(288);
        let diff = if dyn_.birth_rate > expected {
            dyn_.birth_rate - expected
        } else {
            expected - dyn_.birth_rate
        };
        assert!(
            diff < Real::from_ratio(1, 10_000),
            "birth_rate {:?} != clutch/fertile_window {:?}",
            dyn_.birth_rate,
            expected
        );
    }

    #[test]
    fn step_grows_under_neutral_conditions() {
        let biology = test_biology();
        let dyn_ = PopulationDynamics::for_species(
            &biology,
            Real::from_int(40),
            Real::from_ratio(50, 100),
            Real::from_ratio(50, 100),
        );
        let mut c = Cohort::new(Real::from_int(100));
        // Big capacity so no stress.
        for _ in 0..120 {
            dyn_.step_with_capacity(&mut c, Real::from_int(100_000));
        }
        // After 10 years (120 ticks), a clutch=10, mid-survival
        // species starting from 100 fertile founders should grow
        // and have all four brackets populated.
        assert!(c.total() > Real::from_int(100), "should grow: {c:?}");
        assert!(c.infant > Real::ZERO, "infants should appear: {c:?}");
        assert!(c.juvenile > Real::ZERO, "juveniles should appear: {c:?}");
    }

    #[test]
    fn step_drives_to_zero_at_zero_capacity() {
        let biology = test_biology();
        let dyn_ = PopulationDynamics::for_species(
            &biology,
            Real::from_int(40),
            Real::from_ratio(50, 100),
            Real::from_ratio(50, 100),
        );
        let mut c = Cohort::new(Real::from_int(100));
        for _ in 0..200 {
            dyn_.step_with_capacity(&mut c, Real::ZERO);
        }
        assert!(
            c.total() < Real::from_int(5),
            "should collapse under zero capacity: {c:?}"
        );
    }

    #[test]
    fn weighted_demand_uses_food_multipliers() {
        let biology = test_biology();
        let mut c = Cohort::empty();
        c.infant = Real::from_int(10);
        c.juvenile = Real::from_int(10);
        c.fertile = Real::from_int(10);
        c.elder = Real::from_int(10);
        // 0.3*10 + 0.6*10 + 1.0*10 + 0.9*10 = 28. Q32.32 has small
        // rounding error since 0.3, 0.6, 0.9 aren't binary-exact;
        // tolerate within 0.001.
        let d = c.weighted_demand(&biology);
        let expected = Real::from_int(28);
        let diff = if d > expected { d - expected } else { expected - d };
        assert!(diff < Real::from_ratio(1, 1_000), "demand {d:?} != 28 within tol");
    }

    #[test]
    fn food_security_one_at_or_below_capacity() {
        assert_eq!(
            food_security(Real::from_int(50), Real::from_int(100)),
            Real::ONE
        );
        assert_eq!(
            food_security(Real::from_int(100), Real::from_int(100)),
            Real::ONE
        );
    }

    #[test]
    fn food_security_drops_above_capacity() {
        let s = food_security(Real::from_int(150), Real::from_int(100));
        assert_eq!(s, Real::from_ratio(5, 10));
    }

    #[test]
    fn food_security_zero_at_or_above_double_capacity() {
        assert_eq!(
            food_security(Real::from_int(200), Real::from_int(100)),
            Real::ZERO
        );
        assert_eq!(
            food_security(Real::from_int(500), Real::from_int(100)),
            Real::ZERO
        );
    }

    #[test]
    fn food_security_zero_when_capacity_zero() {
        assert_eq!(food_security(Real::from_int(10), Real::ZERO), Real::ZERO);
    }

    #[test]
    fn step_is_deterministic() {
        let biology = test_biology();
        let dyn_ = PopulationDynamics::for_species(
            &biology,
            Real::from_int(40),
            Real::from_ratio(50, 100),
            Real::from_ratio(50, 100),
        );
        let mut a = Cohort::new(Real::from_int(100));
        let mut b = Cohort::new(Real::from_int(100));
        for _ in 0..50 {
            dyn_.step_with_capacity(&mut a, Real::from_int(10_000));
            dyn_.step_with_capacity(&mut b, Real::from_int(10_000));
        }
        assert_eq!(a.total(), b.total());
        assert_eq!(a.fertile, b.fertile);
    }

    #[test]
    fn r_strategist_grows_faster_than_k_strategist() {
        // r-strategist: clutch=200, lifespan=4yr, low survival.
        let r_bio = PopulationBiology {
            clutch_size: Real::from_int(200),
            infant_fraction: Real::from_ratio(2, 100),
            maturity_fraction: Real::from_ratio(8, 100),
            eldership_fraction: Real::ZERO,
            infant_survival: Real::from_ratio(8, 100),
            juvenile_survival: Real::from_ratio(40, 100),
            food_multipliers: [
                Real::from_ratio(30, 100),
                Real::from_ratio(60, 100),
                Real::ONE,
                Real::from_ratio(90, 100),
            ],
        };
        // K-strategist: clutch=1, lifespan=80yr, high survival.
        let k_bio = PopulationBiology {
            clutch_size: Real::ONE,
            infant_fraction: Real::from_ratio(3, 100),
            maturity_fraction: Real::from_ratio(20, 100),
            eldership_fraction: Real::from_ratio(20, 100),
            infant_survival: Real::from_ratio(90, 100),
            juvenile_survival: Real::from_ratio(95, 100),
            food_multipliers: [
                Real::from_ratio(30, 100),
                Real::from_ratio(60, 100),
                Real::ONE,
                Real::from_ratio(90, 100),
            ],
        };
        let r_dyn = PopulationDynamics::for_species(
            &r_bio,
            Real::from_int(4),
            Real::from_ratio(30, 100),
            Real::from_ratio(20, 100),
        );
        let k_dyn = PopulationDynamics::for_species(
            &k_bio,
            Real::from_int(80),
            Real::from_ratio(80, 100),
            Real::from_ratio(80, 100),
        );
        let mut r_cohort = Cohort::new(Real::from_int(100));
        let mut k_cohort = Cohort::new(Real::from_int(100));
        // 60 ticks (5 sim-years).
        for _ in 0..60 {
            r_dyn.step_with_capacity(&mut r_cohort, Real::from_int(1_000_000));
            k_dyn.step_with_capacity(&mut k_cohort, Real::from_int(1_000_000));
        }
        assert!(
            r_cohort.total() > k_cohort.total(),
            "r-strategist {:?} should grow faster than K-strategist {:?}",
            r_cohort.total(),
            k_cohort.total()
        );
    }

    /// Life expectancy at birth scales positively with bracket
    /// survival rates (more durable bracket means more time spent
    /// in it) and inversely with stress / mortality. This test
    /// pins the basic monotonicity: bumping every survival rate
    /// without changing aging-out rates strictly increases the
    /// computed expectancy.
    #[test]
    fn life_expectancy_increases_with_survival_rates() {
        let biology = test_biology();
        let baseline = PopulationDynamics::for_species(
            &biology,
            Real::from_int(40),
            Real::from_ratio(50, 100),
            Real::from_ratio(50, 100),
        );
        let baseline_le = baseline.life_expectancy_months();
        // Apply 50% mortality reduction across every bracket — a
        // fully-equipped medicine + sanitation civ.
        let mut buffed = baseline;
        buffed.mortality_reduction = [
            Real::from_ratio(50, 100),
            Real::from_ratio(50, 100),
            Real::from_ratio(50, 100),
            Real::from_ratio(50, 100),
        ];
        let buffed_le = buffed.life_expectancy_months();
        assert!(
            buffed_le > baseline_le,
            "tech-buffed LE {buffed_le:?} should exceed baseline LE {baseline_le:?}"
        );
    }

    /// Tech mortality reduction (fed via
    /// `PopulationDynamics::mortality_reduction`) cuts per-tick
    /// deaths so a civ with sanitation + medicine accumulates
    /// more pop than a tech-naive baseline under identical
    /// biology + capacity.
    #[test]
    fn mortality_reduction_lifts_population() {
        let biology = test_biology();
        let cog = Real::from_ratio(50, 100);
        let soc = Real::from_ratio(50, 100);
        let lifespan = Real::from_int(40);
        let baseline_dyn = PopulationDynamics::for_species(&biology, lifespan, cog, soc);
        let mut tech_dyn = baseline_dyn;
        // Generous reduction across all brackets — simulates a
        // tier-4/5 medicine + sanitation civ.
        tech_dyn.mortality_reduction = [
            Real::from_ratio(40, 100),
            Real::from_ratio(40, 100),
            Real::from_ratio(40, 100),
            Real::from_ratio(40, 100),
        ];
        let mut baseline_cohort = Cohort::new(Real::from_int(200));
        let mut tech_cohort = Cohort::new(Real::from_int(200));
        // Modest capacity — both runs feel mild stress so the
        // baseline-mortality cut shows up in the diff.
        let cap = Real::from_int(500);
        for _ in 0..240 {
            baseline_dyn.step_with_capacity(&mut baseline_cohort, cap);
            tech_dyn.step_with_capacity(&mut tech_cohort, cap);
        }
        assert!(
            tech_cohort.total() > baseline_cohort.total(),
            "tech-equipped cohort {:?} should outpace baseline {:?}",
            tech_cohort.total(),
            baseline_cohort.total()
        );
    }
}
