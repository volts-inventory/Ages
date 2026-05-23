//! Per-tick population dynamics: birth/survival/aging rates and
//! the capacity-coupled step.
//!
//! `PopulationDynamics` is a derived rate struct (pure function of
//! species biology + lifespan + cognition + sociality) and a
//! per-tick `step_with_capacity` that applies births, survival
//! (with stress + starvation + tech mortality reduction), and
//! aging-out promotions to a `Cohort`. Also exposes
//! `food_security` — the demand/capacity-derived stress signal
//! the step consumes.

use sim_arith::transcendental::{exp, ln};
use sim_arith::{Pop, Real};
use sim_species::PopulationBiology;

use crate::cohort::Cohort;

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
    /// `Real::percent(150)` = 50% more births per fertile.
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
        let infant_months =
            (lifespan * biology.infant_fraction * baseline_months_per_year).max(Real::ONE);
        let juvenile_months =
            (lifespan * biology.maturity_fraction * baseline_months_per_year).max(Real::ONE);
        let fertile_months =
            (lifespan * biology.fertile_fraction() * baseline_months_per_year).max(Real::ONE);
        // Eldership can be zero by construction; clamp the
        // attrition denominator to >= 1 so divides don't blow up.
        let elder_months_raw = lifespan * biology.eldership_fraction * baseline_months_per_year;
        let elder_months = elder_months_raw.max(Real::ONE);
        // Birth rate: (clutch_size × events_per_window) /
        // fertile_months. The `events_per_window` factor distinguishes
        // semelparous from iteroparous strategies — a salmon
        // (clutch=5000, events=1) and a rat (clutch=8, events=24)
        // both with total-lifetime offspring in the thousands
        // produce wildly different per-month rates under the new
        // formula (~83/mo vs ~8/mo), where the legacy `clutch /
        // fertile_months` collapsed them to identical dynamics.
        //
        // Back-compat: `events_per_fertile_window <= 0` is read as
        // "legacy biology literal" and falls through to the original
        // formula. The deriving sampler always sets a positive value
        // (range [2, 30]); only hand-built test fixtures pass `0`.
        //
        // Use `saturating_mul` so the K-strategist long-fertile-window
        // / r-strategist large-clutch tails can't push the Q32.32
        // `Real` underlying past its ±2.1e9 ceiling. The product is
        // capped before division, then the per-fertile per-month rate
        // is well within range; this is the first of the two
        // overflow guards that together keep `fertile × birth_rate`
        // bounded for the whole derived rate chain.
        // birth_rate now layers a `reproductive_success` factor on
        // top of `clutch × events` so the per-month rate calibrates
        // against real demography. K-strategist mammals land at
        // ~0.001-0.01 births/fertile/month (real human ≈ 0.0005);
        // r-strategist broadcast-spawners land at ~5-90 (real
        // salmon spawn ~83 spread over their pre-death fertile
        // window). Without success, the prior calibration
        // overshot K rates by ~500× and the recruit-ceiling clamp
        // was the load-bearing limiter.
        //
        // Three back-compat tiers:
        //   - new biology (events>0 && success>0): full formula
        //   - mid-tier (events>0, success=0): clutch × events / fertile_months
        //     (matches PR #29 behaviour for hand-built test fixtures
        //     that opt-in to events but haven't been migrated to
        //     success yet)
        //   - legacy (events=0): clutch / fertile_months
        let birth_rate = if biology.events_per_fertile_window > Real::ZERO
            && biology.reproductive_success > Real::ZERO
        {
            biology
                .clutch_size
                .saturating_mul(biology.events_per_fertile_window)
                .saturating_mul(biology.reproductive_success)
                / fertile_months
        } else if biology.events_per_fertile_window > Real::ZERO {
            biology
                .clutch_size
                .saturating_mul(biology.events_per_fertile_window)
                / fertile_months
        } else {
            biology.clutch_size / fertile_months
        };
        // Per-tick survival via per_tick = exp(ln(window_survival)
        // / months). For window_survival in (0, 1] this returns a
        // per-tick fraction in (0, 1].
        let to_per_tick = |window_survival: Real, months: Real| -> Real {
            let s = window_survival
                .max(Real::from_ratio(1, 1000))
                .min(Real::ONE);
            let log_s = ln(s);
            let exponent = log_s / months;
            exp(exponent).clamp01()
        };
        // Fertile baseline survival over the entire fertile window:
        // 0.85 + 0.10 * cognition (range [0.85, 0.95]).
        let cog_clamped = cognition.clamp01();
        let fertile_window_survival = Real::percent(85) + cog_clamped * Real::percent(10);
        // Elder window survival: baseline 0.30 (senescence dominates;
        // most species kill their elders within the elder window).
        let elder_window_survival = Real::percent(30);
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
        let soc_clamped = sociality.clamp01();
        let stress_factor = (Real::from_int(5) - soc_clamped - cog_clamped).max(Real::from_int(2));
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
                Real::percent(30),
                Real::percent(60),
                Real::ONE,
                Real::percent(90),
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
    pub fn step_with_capacity(&self, cohort: &mut Cohort, capacity: Pop) {
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
        let starvation = stress * Real::percent(10);
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
            let r = reduction.clamp01();
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
        //
        // Q32.32 overflow guard (defensive): a hyper-r seed with
        // clutch ≈ 500 over a 1-yr fertile window derived
        // `birth_rate ≈ 417 births/fertile/month` under the legacy
        // (pre-`events_per_window`) formula; with a billion fertile
        // and a planet/tech multiplier the product `fertile_pop ×
        // birth_rate × multiplier` could push the Q96.32 `Pop` near
        // its ceiling within two ticks and panic at the next
        // multiplication. We compute the births via saturating
        // multiplies and then hard-clamp to the biological ceiling
        // of `fertile × 5` (no real species recruits more than 5×
        // its fertile population in a single month — even mass
        // broadcast spawners hit ecological brick walls well below
        // that). Together with the `events_per_window` reformulation
        // this keeps the recruit term bounded across the whole r/K
        // axis.
        let rate_with_tech = self.birth_rate.saturating_mul(self.birth_rate_multiplier);
        let raw_births = cohort
            .fertile
            .saturating_mul_real(rate_with_tech)
            .saturating_mul_real(security);
        // Per-tick recruit ceiling: 5× fertile. Models the biology
        // constraint that ecological feedback bounds in-tick recruits
        // even when the formula would suggest otherwise.
        let recruit_ceiling = cohort.fertile.saturating_mul_real(Real::from_int(5));
        let births = raw_births.min(recruit_ceiling);
        // Debug-only assert: the per-tick births can't overrun
        // i64::MAX/2 (a comfortable margin below the i96 ceiling
        // that Pop's internals carry). If this fires, the upstream
        // sampler emitted a rate that should have been clamped at
        // derivation time.
        debug_assert!(
            births.to_f64_for_display() < (i64::MAX / 2) as f64,
            "births {births:?} exceeds the i64::MAX/2 overflow threshold"
        );
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
        let big = (cohort.total() * Real::from_int(1_000)).max(Pop::from_int(1_000_000));
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
            let red = r.clamp01();
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
pub fn food_security(demand: Pop, capacity: Pop) -> Real {
    if capacity <= Pop::ZERO {
        return Real::ZERO;
    }
    let ratio: Real = demand / capacity;
    let overshoot = ratio - Real::ONE;
    let stress = if overshoot > Real::ZERO {
        overshoot
    } else {
        Real::ZERO
    };
    let raw = Real::ONE - stress;
    raw.clamp01()
}
