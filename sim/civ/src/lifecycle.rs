//! Collapse triggers + cohesion drift. `check_collapse` is the legacy
//! planet-less form retained for unit tests;
//! `check_collapse_with_terrain` is the production path called by
//! sim/core.

use crate::demographics::streak_ticks_for_metabolism;
use crate::{
    Civ, CollapseReason, CIVIL_WAR_COHESION_FLOOR, CIVIL_WAR_STREAK_TICKS,
    COHESION_BREAKAWAY_TRIGGER, CULTURAL_LOCK_DOGMA, CULTURAL_LOCK_STREAK_TICKS,
    DEPOPULATION_FLOOR_POP, DEPOPULATION_STREAK_TICKS, FOOD_CRISIS_STREAK_TICKS,
    FOOD_CRISIS_THRESHOLD, PLATEAU_WINDOW_TICKS, TINY_TERRITORY_CELLS, TINY_TERRITORY_STREAK_TICKS,
};
use sim_arith::{Pop, Real};

impl Civ {
    /// Per-tick collapse evaluation. Updates the food-security
    /// streak and checks both triggers. Returns the reason the
    /// civ should collapse this tick, or `None`. Caller invokes
    /// `collapse(tick, reason)` to apply.
    ///
    /// Pre- this used the planet-less `carrying_capacity` so
    /// food security ignored terrain habitability. Production now
    /// goes through `check_collapse_with_terrain`; this overload
    /// stays for legacy unit tests that don't thread a `Planet`
    /// through.
    pub fn check_collapse(
        &mut self,
        tick: u64,
        state: &sim_physics::PhysicsState,
    ) -> Option<CollapseReason> {
        if !self.is_active() {
            return None;
        }
        let demand = self
            .cohort
            .weighted_demand_from_multipliers(&self.dynamics.food_multipliers);
        let security = sim_population::food_security(demand, self.carrying_capacity(state));
        let crisis_floor = Real::from(FOOD_CRISIS_THRESHOLD);
        if security <= crisis_floor {
            self.low_food_streak = self.low_food_streak.saturating_add(1);
        } else {
            self.low_food_streak = 0;
        }
        // cultural-lock streak. High dogmatism + no
        // recent refinement → frozen civ.
        let dogma = self.cosmology.dogmatism();
        let dogma_floor = Real::from(CULTURAL_LOCK_DOGMA);
        let no_recent_refinement =
            tick.saturating_sub(self.last_refinement_tick) >= CULTURAL_LOCK_STREAK_TICKS;
        if dogma >= dogma_floor && no_recent_refinement {
            self.cultural_lock_streak = self.cultural_lock_streak.saturating_add(1);
        } else {
            self.cultural_lock_streak = 0;
        }

        // territory-too-small streak. A civ squeezed to
        // exactly `TINY_TERRITORY_CELLS` for
        // `TINY_TERRITORY_STREAK_TICKS` consecutive ticks
        // collapses with `territory_too_small`. Closes the case
        // where a successor carves the parent down to
        // one cell but the parent then lingers on forever because
        // no other trigger fires (food security can stay healthy
        // on a single cell with low population).
        //
        // Bound the count below to a non-empty claim: a civ with
        // zero claimed cells is the brand-new pre-territory state
        // (e.g. legacy callers, fresh `Civ::new` before sim/core
        // runs `claim_cells`). Counting that as "tiny" would fire
        // TerritoryTooSmall on every civ before any other trigger
        // could ever run.
        let len = self.claimed_cells.len();
        if (1..=TINY_TERRITORY_CELLS).contains(&len) {
            self.tiny_territory_streak = self.tiny_territory_streak.saturating_add(1);
        } else {
            self.tiny_territory_streak = 0;
        }

        // depopulation streak. Independent of food security
        // (which uses `<=` against a 0.3 ratio of demand/capacity);
        // this just checks whether anyone is left. Catches the case
        // where catastrophe / combat drains the cohort to ~0 but
        // other streak triggers haven't crossed threshold yet, so
        // the viewport sidebar reads "0p" for a "live" civ.
        let depop_floor = sim_arith::Pop::from_int(DEPOPULATION_FLOOR_POP);
        if self.aggregate_population() <= depop_floor {
            self.depopulation_streak = self.depopulation_streak.saturating_add(1);
        } else {
            self.depopulation_streak = 0;
        }

        if self.low_food_streak >= FOOD_CRISIS_STREAK_TICKS {
            return Some(CollapseReason::FoodCrisis);
        }
        if tick.saturating_sub(self.last_discovery_tick) >= PLATEAU_WINDOW_TICKS {
            return Some(CollapseReason::KnowledgePlateau);
        }
        if self.cultural_lock_streak >= CULTURAL_LOCK_STREAK_TICKS {
            return Some(CollapseReason::CulturalLock);
        }
        if self.tiny_territory_streak >= TINY_TERRITORY_STREAK_TICKS {
            return Some(CollapseReason::TerritoryTooSmall);
        }
        if self.depopulation_streak >= DEPOPULATION_STREAK_TICKS {
            return Some(CollapseReason::Depopulation);
        }
        None
    }

    /// terrain-aware collapse evaluation. Identical to
    /// `check_collapse` except food security is computed from
    /// `carrying_capacity_with_terrain` rather than the
    /// planet-less aggregate. A civ whose claimed cells are all
    /// peaks / shallow sea (multiplier ≤ 0.10) starves much
    /// faster than its raw fuel column would suggest.
    ///
    /// sim/core's per-tick lifecycle phase calls this; legacy
    /// `check_collapse` stays for unit tests that don't thread a
    /// `Planet` through.
    pub fn check_collapse_with_terrain(
        &mut self,
        tick: u64,
        state: &sim_physics::PhysicsState,
        planet: &sim_world::Planet,
    ) -> Option<CollapseReason> {
        if !self.is_active() {
            return None;
        }
        // Every streak / cooldown is denominated in "baseline-tick"
        // units (Aqueous calibration). Stretch them by the inverse of
        // the substrate metabolism so a silicate civ's "75-year
        // civil-war streak" lasts the same number of generations as
        // an aqueous civ's, just over more ticks.
        let metabolism = planet.metabolic_substrate.metabolism();
        let food_crisis_streak = streak_ticks_for_metabolism(FOOD_CRISIS_STREAK_TICKS, metabolism);
        let plateau_window = streak_ticks_for_metabolism(PLATEAU_WINDOW_TICKS, metabolism);
        let cultural_lock_streak =
            streak_ticks_for_metabolism(CULTURAL_LOCK_STREAK_TICKS, metabolism);
        let tiny_territory_streak =
            streak_ticks_for_metabolism(TINY_TERRITORY_STREAK_TICKS, metabolism);
        let civil_war_streak = streak_ticks_for_metabolism(CIVIL_WAR_STREAK_TICKS, metabolism);
        let depopulation_streak =
            streak_ticks_for_metabolism(DEPOPULATION_STREAK_TICKS, metabolism);

        let demand = self
            .cohort
            .weighted_demand_from_multipliers(&self.dynamics.food_multipliers);
        let raw_security = sim_population::food_security(
            demand,
            self.carrying_capacity_with_terrain(state, planet),
        );
        // M8: surplus buffer adds to the raw security score so a
        // civ with stored reserves rides out lean ticks without
        // tipping into crisis. Bounded at `SURPLUS_FOOD_BUFFER_BONUS`
        // (currently +0.20) so an infinite-surplus civ is still
        // *eventually* killable.
        let surplus_buffer = crate::economy::surplus_food_buffer(self.surplus, demand);
        let security = raw_security + surplus_buffer;
        // tools that improve food security (BulkStorage,
        // OrganizedHunting, FluidGathering, BulkCultivation,
        // OrganicSynthesis) lower the crisis floor by their
        // additive bonus, so a civ with food-resilience tools
        // survives leaner runs without tipping into collapse.
        let base_floor = Real::from(FOOD_CRISIS_THRESHOLD);
        let crisis_floor = (base_floor - self.tool_food_crisis_bonus()).max(Real::ZERO);
        if security <= crisis_floor {
            self.low_food_streak = self.low_food_streak.saturating_add(1);
        } else {
            self.low_food_streak = 0;
        }
        let dogma = self.cosmology.dogmatism();
        let dogma_floor = Real::from(CULTURAL_LOCK_DOGMA);
        let no_recent_refinement =
            tick.saturating_sub(self.last_refinement_tick) >= cultural_lock_streak;
        if dogma >= dogma_floor && no_recent_refinement {
            self.cultural_lock_streak = self.cultural_lock_streak.saturating_add(1);
        } else {
            self.cultural_lock_streak = 0;
        }
        let len = self.claimed_cells.len();
        if (1..=TINY_TERRITORY_CELLS).contains(&len) {
            self.tiny_territory_streak = self.tiny_territory_streak.saturating_add(1);
        } else {
            self.tiny_territory_streak = 0;
        }
        // depopulation streak. Same rationale as the planet-less
        // overload above — collapses civs whose aggregate population
        // has been ≤ the rendering floor for the streak window.
        let depop_floor = Pop::from_int(DEPOPULATION_FLOOR_POP);
        if self.aggregate_population() <= depop_floor {
            self.depopulation_streak = self.depopulation_streak.saturating_add(1);
        } else {
            self.depopulation_streak = 0;
        }
        // cohesion drift. Each tick, cohesion moves toward
        // an equilibrium determined by:
        //  * size pressure: more cells → lower equilibrium
        //  * food security: < 0.5 pulls cohesion down sharply
        //  * dogmatism: shared belief holds the polity together
        //  * literacy: shared canon and institutions hold longer
        // Drift rate is small per tick (1/200) so meaningful shifts
        // take many sim-years; on slow substrates it scales by
        // metabolism so societal change unfolds at the same
        // per-generation rate.
        self.update_cohesion(security, tick, metabolism);
        let cw_floor = Real::from(CIVIL_WAR_COHESION_FLOOR);
        let breakaway_trigger = Real::from(COHESION_BREAKAWAY_TRIGGER);
        if self.cohesion < cw_floor {
            self.civil_war_streak = self.civil_war_streak.saturating_add(1);
        } else {
            self.civil_war_streak = 0;
        }
        // breakaway-fragmentation zone is between civil-war
        // floor and breakaway-trigger. Streak resets if cohesion
        // recovers above trigger OR drops below floor (the latter
        // hands off to civil-war collapse instead of breakaway).
        if self.cohesion >= cw_floor && self.cohesion <= breakaway_trigger {
            self.cohesion_breakaway_streak = self.cohesion_breakaway_streak.saturating_add(1);
        } else {
            self.cohesion_breakaway_streak = 0;
        }
        if self.low_food_streak >= food_crisis_streak {
            return Some(CollapseReason::FoodCrisis);
        }
        if tick.saturating_sub(self.last_discovery_tick) >= plateau_window {
            return Some(CollapseReason::KnowledgePlateau);
        }
        if self.cultural_lock_streak >= cultural_lock_streak {
            return Some(CollapseReason::CulturalLock);
        }
        if self.tiny_territory_streak >= tiny_territory_streak {
            return Some(CollapseReason::TerritoryTooSmall);
        }
        if self.civil_war_streak >= civil_war_streak {
            return Some(CollapseReason::CivilWar);
        }
        if self.depopulation_streak >= depopulation_streak {
            return Some(CollapseReason::Depopulation);
        }
        None
    }

    /// per-tick cohesion update. Pure function of
    /// `(food_security, current_state)`. Computes an equilibrium
    /// target and drifts `self.cohesion` toward it at 1/200 per tick
    /// (~half-life ~12 baseline-years toward equilibrium).
    ///
    /// Equilibrium model (final value clamped to `[0, 1]`):
    ///  * Base: 1.0
    ///  * Size penalty: −0.30 × (`claimed_cells` / 30) clamped to ≤ 0.30
    ///  * Food penalty: −0.50 × (1 − security) when security < 0.5
    ///  * Dogmatism bonus: +0.20 × dogmatism
    ///  * Literacy bonus: +0.20 × literacy
    ///
    /// `tick` is the current simulation tick; used to compute
    /// `literacy_score(tick)` for the bonus term.
    pub fn update_cohesion(&mut self, security: Real, tick: u64, metabolism: Real) {
        let cells = i64::try_from(self.claimed_cells.len()).unwrap_or(i64::MAX);
        let size_factor = (Real::from_int(cells) / Real::from_int(30)).clamp01();
        let size_penalty = Real::from_ratio(30, 100) * size_factor;
        let food_penalty = if security < Real::from_ratio(50, 100) {
            Real::from_ratio(50, 100) * (Real::ONE - security)
        } else {
            Real::ZERO
        };
        let dogma_bonus = Real::from_ratio(20, 100) * self.cosmology.dogmatism();
        let literacy_bonus = Real::from_ratio(20, 100) * self.literacy_score(tick);
        // Tools that bind the polity together (canonised law, mass
        // literacy, network identity, urban anchors, defensive
        // institutions) lift the cohesion equilibrium directly.
        // `tool_cohesion_bonus` is capped at +0.40 so the equilibrium
        // can climb past 1.0 internally and clamp on the next line.
        let tool_bonus = self.tool_cohesion_bonus();
        let target =
            (Real::ONE - size_penalty - food_penalty + dogma_bonus + literacy_bonus + tool_bonus)
                .clamp01();
        // Drift toward target at 1/200 per tick on Aqueous; scaled
        // by metabolism so slow chemistries drift proportionally
        // slower and the streak thresholds (also stretched by
        // metabolism) still cover the same fraction of generations.
        let drift_rate = Real::from_ratio(1, 200) * metabolism;
        self.cohesion = (self.cohesion + (target - self.cohesion) * drift_rate).clamp01();
    }
}
