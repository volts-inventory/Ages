//! Carrying-capacity computations and capacity-coupled population
//! steps.
//!
//! P0.5 — pre-refactor, capacity summed per-cell `Substance::Fuel`
//! density across `claimed_cells`. That decoupled the civ from the
//! living producer pool: `PlanetEcosystem` could collapse to zero
//! biomass via cascading extinctions and the civ would still see the
//! fuel column as full, so the food-crisis trigger never fired.
//!
//! New model: the civ's capacity equals
//! `producer_biomass × claimed_cell_fraction × carrying_capacity_per_unit
//!     × tech_multiplier × tool_capacity_multiplier`,
//! where `producer_biomass` is `PlanetEcosystem::tier_biomass(0)`
//! threaded in from sim/core's per-tick ecosystem step. Cascading
//! extinctions now starve civs — apex predator removal eventually
//! reaches the producer tier; producer crash collapses civ capacity.
//!
//! The `carrying_capacity_per_unit` multiplier is retained so the
//! existing per-substrate scaling (Aqueous vs Silicate vs Plasma,
//! cached at founding via `configure_substrate`) still applies.

use crate::Civ;
use sim_arith::{Pop, Real};
use sim_species::Habitat;

/// Per-cell habitability scaled to this civ's species habitat.
/// Pure delegate to `sim_species::habitat_glyph_multiplier` —
/// the single source of truth shared with `sim_core::territory`
/// so capacity-rewarded terrain and territory-preferred terrain
/// agree per habitat. The previous local table contradicted
/// territory's for Subterranean (peak 0.10 here vs 1.30 there);
/// a peak-founded Subterranean civ would then starve. The shared
/// function ensures the two stay aligned.
pub(crate) fn species_habitability(glyph: char, habitat: Habitat) -> Real {
    sim_species::habitat_glyph_multiplier(habitat, glyph)
}

/// P0.5 — fraction of the planet's grid this civ holds. Read
/// from `state.grid().n_cells()` so the ratio reflects the
/// actual sim grid, not a hard-coded "Earth-equivalent" cell
/// count. Pre-territory civs (`claimed_cells.is_empty()`) get
/// the full `1.0` share so the legacy unit tests (which run
/// without `claim_cells`) still see the full producer pool.
fn claimed_cell_fraction(civ: &Civ, state: &sim_physics::PhysicsState) -> Real {
    let total = state.grid().n_cells();
    if civ.claimed_cells.is_empty() || total == 0 {
        return Real::ONE;
    }
    let claimed = i64::try_from(civ.claimed_cells.len()).unwrap_or(i64::MAX);
    let total_i = i64::try_from(total).unwrap_or(i64::MAX);
    Real::from_int(claimed) / Real::from_int(total_i)
}

impl Civ {
    /// Carrying capacity in pop units. P0.5: derived from the
    /// live `PlanetEcosystem` producer-tier biomass scaled by
    /// the civ's claimed-cell share of the planet, then
    /// multiplied by `carrying_capacity_per_unit × tech_multiplier
    /// × tool_capacity_multiplier`.
    ///
    /// The producer-biomass scalar comes from
    /// `self.producer_biomass` — set each tick by sim/core via
    /// `update_producer_biomass(ecosystem.tier_biomass(0))` before
    /// any capacity-coupled step runs. Cascading extinctions
    /// (apex predator removal → mesopredator release → primary
    /// consumer collapse → producer overshoot/crash) now propagate
    /// into civ capacity: a planet whose producers have gone
    /// extinct sees civ capacity fall toward zero.
    ///
    /// Pre-P0.5 callers that don't have ecosystem context (legacy
    /// unit tests) see `self.producer_biomass = Real::ONE` (the
    /// default) and a capacity baseline that depends on tech +
    /// claim fraction only. The previous fuel-column scan is gone
    /// — `Substance::Fuel` continues to exist as a chemistry-side
    /// substance, but it no longer doubles as the food proxy.
    pub fn carrying_capacity(&self, state: &sim_physics::PhysicsState) -> Pop {
        let claimed_frac = claimed_cell_fraction(self, state);
        // P0.5 — replace the per-cell `Substance::Fuel` sum with
        // `producer_biomass × claimed_cell_frac`. The producer
        // share is what the civ can actually draw food from; the
        // claimed-cell fraction is the share of the planet it
        // farms / forages. Tools and tech multiply through the
        // same way they did pre-refactor.
        let producer_share = self.producer_biomass * claimed_frac;
        let mult = producer_share * self.tech_multiplier * self.tool_capacity_multiplier();
        Pop::from_real(self.carrying_capacity_per_unit) * mult
    }

    /// terrain-aware carrying capacity. P0.5: same producer-share
    /// formula as `carrying_capacity`, with the claimed-cell share
    /// further weighted by each claimed cell's terrain
    /// habitability multiplier. Deep ocean / gas-band cells
    /// contribute zero; coast cells get a 1.20× boost; peaks
    /// ~10%.
    ///
    /// The terrain weighting is an *additional* per-cell factor on
    /// top of the producer-tier biomass scalar — a sprawling civ
    /// with mostly coastal claims gets a larger effective producer
    /// share than the same civ on peaks, even though both read
    /// the same planet-wide producer biomass.
    pub fn carrying_capacity_with_terrain(
        &self,
        state: &sim_physics::PhysicsState,
        planet: &sim_world::Planet,
    ) -> Pop {
        let total = state.grid().n_cells();
        // Pre-territory: full producer share, no terrain weighting.
        if self.claimed_cells.is_empty() || total == 0 {
            return self.carrying_capacity(state);
        }
        let total_i = i64::try_from(total).unwrap_or(i64::MAX);
        let total_r = Real::from_int(total_i);
        // Sum the per-cell terrain multipliers, then divide by
        // `total_cells` so the result is the terrain-weighted
        // claimed-cell fraction. Equivalent to averaging the
        // multiplier across claimed cells × (claimed / total)
        // — exactly the producer share a civ on heterogeneous
        // terrain actually realises.
        let weighted: Real = self
            .claimed_cells
            .iter()
            .map(|&c| {
                let glyph = sim_world::terrain_glyph_at(state, planet, c);
                species_habitability(glyph, self.species_habitat)
            })
            .fold(Real::ZERO, |a, b| a + b);
        let weighted_share = weighted / total_r;
        let producer_share = self.producer_biomass * weighted_share;
        let mult = producer_share * self.tech_multiplier * self.tool_capacity_multiplier();
        Pop::from_real(self.carrying_capacity_per_unit) * mult
    }

    /// season-aware carrying capacity. P0.5: aggregates per-cell
    /// contributions, where each cell's share of the planet-wide
    /// producer biomass is scaled by terrain habitability ×
    /// seasonal capacity factor for `(tick, cell, planet)`.
    pub fn seasonal_carrying_capacity(
        &self,
        state: &sim_physics::PhysicsState,
        tick: u64,
        planet: &sim_world::Planet,
    ) -> Pop {
        if self.claimed_cells.is_empty() {
            return self.carrying_capacity(state);
        }
        let total = state.grid().n_cells();
        if total == 0 {
            return Pop::ZERO;
        }
        let total_i = i64::try_from(total).unwrap_or(i64::MAX);
        let producer_per_cell = self.producer_biomass / Real::from_int(total_i);
        let temps = state.temperature();
        let pop_per_unit = Pop::from_real(self.carrying_capacity_per_unit);
        let grid = state.grid();
        self.claimed_cells
            .iter()
            .map(|&c| {
                let base_temp = temps.get(c as usize).copied().unwrap_or(Real::ZERO);
                let offset = sim_world::seasonal_temperature_offset(tick, c, planet, grid);
                let cell_temp = base_temp + offset;
                let raw_factor = sim_world::seasonal_capacity_factor(cell_temp, planet);
                // tools that lift the seasonal floor (SimpleShelter,
                // BasicTextiles, PermanentMasonry, FluidControl,
                // UrbanConstruction, EnergyStorage) shrink the per-cell
                // seasonal penalty.
                let factor = self.effective_seasonal_factor(raw_factor);
                let glyph = sim_world::terrain_glyph_at(state, planet, c);
                let hab = species_habitability(glyph, self.species_habitat);
                let mult = producer_per_cell
                    * self.tech_multiplier
                    * self.tool_capacity_multiplier()
                    * factor
                    * hab;
                pop_per_unit * mult
            })
            .fold(Pop::ZERO, |a, b| a + b)
    }

    /// Advance the civ's population using the capacity-coupled
    /// dynamics. Replaces the M2-era `step_population` that ignored
    /// capacity; callers thread the current `PhysicsState` so
    /// capacity tracks ecosystem changes mid-run.
    ///
    /// Routes through `step_for_lifecycle` so a species'
    /// `Lifecycle` variant (Vertebrate, Insect, Eusocial, …) picks
    /// the matching per-tick step. `Vertebrate` falls through to
    /// the legacy `step_with_capacity` bit-for-bit so existing
    /// Vertebrate-species runs reproduce byte-for-byte.
    pub fn step_population_with_capacity(
        &mut self,
        state: &sim_physics::PhysicsState,
        species: &sim_species::Species,
    ) {
        let cap = self.carrying_capacity(state);
        self.dynamics.mortality_reduction = self.tool_mortality_reduction_per_bracket();
        self.dynamics.birth_rate_multiplier = Real::ONE + self.tool_fertility_bonus();
        sim_population::lifecycle::step_for_lifecycle(
            &species.lifecycle,
            &mut self.lifecycle_state,
            &self.dynamics,
            &mut self.cohort,
            cap,
        );
    }

    /// season-aware population step. Same dynamics as
    /// `step_population_with_capacity` but uses
    /// `seasonal_carrying_capacity` so winter cells throttle
    /// growth and summer cells unlock it.
    pub fn step_population_with_seasonal_capacity(
        &mut self,
        state: &sim_physics::PhysicsState,
        tick: u64,
        planet: &sim_world::Planet,
        species: &sim_species::Species,
    ) {
        let cap = self.seasonal_carrying_capacity(state, tick, planet);
        // Re-derive dynamics so tech changes take effect each tick.
        self.dynamics = crate::dynamics_for_civ(self, species, planet);
        sim_population::lifecycle::step_for_lifecycle(
            &species.lifecycle,
            &mut self.lifecycle_state,
            &self.dynamics,
            &mut self.cohort,
            cap,
        );
    }

    /// per-cell capacity. P0.5: each cell receives its share of
    /// the planet-wide producer biomass
    /// (`self.producer_biomass / total_cells`), then terrain +
    /// seasonal multipliers apply on top. Reads
    /// `self.producer_biomass` (cached by
    /// `step_population_per_cell` at the start of the population
    /// phase) so the territory / migration paths that call
    /// `cell_capacity` deep in the call stack don't have to
    /// re-thread the biomass scalar.
    pub fn cell_capacity(
        &self,
        state: &sim_physics::PhysicsState,
        cell: u32,
        tick: u64,
        planet: &sim_world::Planet,
    ) -> Pop {
        let total = state.grid().n_cells();
        if total == 0 {
            return Pop::ZERO;
        }
        let total_i = i64::try_from(total).unwrap_or(i64::MAX);
        let producer_per_cell = self.producer_biomass / Real::from_int(total_i);
        let temps = state.temperature();
        let base_temp = temps.get(cell as usize).copied().unwrap_or(Real::ZERO);
        let offset = sim_world::seasonal_temperature_offset(tick, cell, planet, state.grid());
        let cell_temp = base_temp + offset;
        let raw_factor = sim_world::seasonal_capacity_factor(cell_temp, planet);
        let factor = self.effective_seasonal_factor(raw_factor);
        let glyph = sim_world::terrain_glyph_at(state, planet, cell);
        let hab = species_habitability(glyph, self.species_habitat);
        let mult = producer_per_cell
            * self.tech_multiplier
            * self.tool_capacity_multiplier()
            * factor
            * hab;
        Pop::from_real(self.carrying_capacity_per_unit) * mult
    }

    /// Update the civ's cached producer-biomass + ecological-
    /// resilience scalars from the live ecosystem reading. Called
    /// once at the start of each civ tick (the per-tick population
    /// phase in sim/core) before any capacity-coupled step or
    /// territory pass runs.
    ///
    /// `producer_biomass` is `PlanetEcosystem::tier_biomass(0)` —
    /// the planet-wide primary producer pool. On the first call
    /// (when `initial_producer_biomass` is at its default
    /// `Real::ONE` sentinel), the value is captured as the civ's
    /// anchor; subsequent calls leave the anchor untouched.
    ///
    /// `ecological_resilience` is `producer_biomass /
    /// max(initial_producer_biomass, 1)` clamped to `[0, 2]`.
    /// 1.0 = baseline; < 1.0 = degraded; > 1.0 = thriving.
    pub fn update_producer_biomass(&mut self, producer_biomass: Real) {
        self.producer_biomass = producer_biomass;
        // First tick after founding: capture the anchor. Use a
        // strict `is_one` check so a deliberate `Real::ONE`
        // anchor at construction time (the default) gets
        // overwritten by the first non-1.0 live value; subsequent
        // calls (where the anchor will rarely equal exactly 1.0
        // again) stay pinned to the first live reading.
        if self.initial_producer_biomass == Real::ONE && producer_biomass != Real::ONE {
            self.initial_producer_biomass = producer_biomass;
        }
        let denom = if self.initial_producer_biomass < Real::ONE {
            Real::ONE
        } else {
            self.initial_producer_biomass
        };
        let ratio = producer_biomass / denom;
        // Clamp to [0, 2]. < 1.0 = degraded ecosystem; 1.0 =
        // baseline; > 1.0 = thriving. The upper bound prevents a
        // single tick's noise from running the scalar away on a
        // collapsed-then-rebounded ecosystem.
        let two = Real::from_int(2);
        let clamped = if ratio < Real::ZERO {
            Real::ZERO
        } else if ratio > two {
            two
        } else {
            ratio
        };
        self.ecological_resilience = clamped;
    }

    /// per-cell population step. P0.5: producer biomass threaded
    /// in once at the top of the call; cached on
    /// `self.producer_biomass` so the per-cell capacity path
    /// (`cell_capacity`, called from the territory + migration
    /// paths several frames deep) sees the live reading without
    /// explicit plumbing.
    ///
    /// Each `region_cohort` evolves independently with its own
    /// `cell_capacity` (tech × terrain × seasonal × producer-share).
    /// Heterogeneous terrain produces heterogeneous density —
    /// fertile valleys fill, peaks and deserts stay sparse, and
    /// expansion (overflow + claim) keeps producing lasting
    /// pressure as the frontier finds richer ground. Low-fuel
    /// cells that die out get pruned via `prune_empty_cells`
    /// rather than dragging the civ-aggregate stress reading down.
    pub fn step_population_per_cell(
        &mut self,
        state: &sim_physics::PhysicsState,
        tick: u64,
        planet: &sim_world::Planet,
        species: &sim_species::Species,
        producer_biomass: Real,
    ) {
        // P0.5: cache producer biomass + resilience anchor before
        // any capacity-coupled step runs.
        self.update_producer_biomass(producer_biomass);
        if self.region_cohorts.is_empty() {
            self.step_population_with_seasonal_capacity(state, tick, planet, species);
            // P1.3 — also drive the dormant-pool resurrection on
            // the legacy single-cohort path so freshly-founded
            // civs and unit-test fixtures that haven't yet
            // distributed to `region_cohorts` still see seed-bank
            // recovery between catastrophes.
            self.step_dormant_resurrection();
            return;
        }
        // Re-derive per-tick dynamics from current tools + drift
        // state. Cheap (~few divs + 4 ln/exp); ensures tool unlocks
        // (mortality reduction, lifespan extension) flow into the
        // per-tick step immediately rather than only at founding.
        self.dynamics = crate::dynamics_for_civ(self, species, planet);
        let cells: Vec<u32> = self.region_cohorts.keys().copied().collect();
        for cell in cells {
            let cap = self.cell_capacity(state, cell, tick, planet);
            if let Some(cohort) = self.region_cohorts.get_mut(&cell) {
                sim_population::lifecycle::step_for_lifecycle(
                    &species.lifecycle,
                    &mut self.lifecycle_state,
                    &self.dynamics,
                    cohort,
                    cap,
                );
            }
        }
        // Re-sum each bracket independently so the civ-level
        // aggregate matches the per-cell breakdown across all four
        // age brackets, not just the scalar total.
        self.cohort.infant = self
            .region_cohorts
            .values()
            .map(|c| c.infant)
            .fold(Pop::ZERO, |a, b| a + b);
        self.cohort.juvenile = self
            .region_cohorts
            .values()
            .map(|c| c.juvenile)
            .fold(Pop::ZERO, |a, b| a + b);
        self.cohort.fertile = self
            .region_cohorts
            .values()
            .map(|c| c.fertile)
            .fold(Pop::ZERO, |a, b| a + b);
        self.cohort.elder = self
            .region_cohorts
            .values()
            .map(|c| c.elder)
            .fold(Pop::ZERO, |a, b| a + b);
        // P1.3 — drain a tick's worth of the dormant pool back
        // into the active cohort. The seed-bank reservoir is
        // populated by catastrophe-driven dormancy in
        // `apply_resistance_and_dormancy`; this is the recovery
        // half — tardigrade-grade species that crypto-bio'd
        // through the catastrophe slowly re-emerge.
        self.step_dormant_resurrection();
    }

    /// P1.3 — drive one tick's worth of dormant-pool resurrection
    /// into the active cohort. Caps recovery at
    /// `pre_catastrophe_population` so the active pool can never
    /// overshoot the largest cohort the civ has ever held. Revived
    /// headcount is deposited into the fertile bracket (matches the
    /// spec's "just add to fertile bracket" fall-through — emerging
    /// cryptobiotic adults rejoin the reproductive pool directly).
    /// When the civ has per-cell `region_cohorts`, the deposit is
    /// distributed proportionally so the resurrection doesn't all
    /// land in cell 0; civs without per-cell breakdown (legacy
    /// tests, freshly founded civs) deposit into the aggregate
    /// fertile bracket directly. No-op when the pool is empty or
    /// the active cohort already sits at the cap.
    pub fn step_dormant_resurrection(&mut self) {
        if self.dormant_pool.population <= Real::ZERO {
            return;
        }
        let target = self.pre_catastrophe_population.to_real_nonneg();
        let mut active = self.cohort.total().to_real_nonneg();
        let revived = self.dormant_pool.resurrect_step(&mut active, target);
        if revived <= Real::ZERO {
            return;
        }
        let revived_pop = Pop::from_real(revived);
        // Distribute the revival across per-cell cohorts in
        // proportion to each cell's current fertile share so the
        // resurrection doesn't pile on top of any single cell.
        // Falls back to the civ-level cohort when no per-cell
        // breakdown exists.
        if self.region_cohorts.is_empty() {
            self.cohort.add_fertile(revived_pop);
            return;
        }
        let total_fertile = self
            .region_cohorts
            .values()
            .map(|c| c.fertile)
            .fold(Pop::ZERO, |a, b| a + b);
        if total_fertile <= Pop::ZERO {
            // No fertile pop anywhere — deposit into the civ-level
            // cohort (which the per-cell sum at the call site
            // already nominally tracks) and into the first cell
            // so the aggregate matches the sum.
            self.cohort.add_fertile(revived_pop);
            if let Some((_, c)) = self.region_cohorts.iter_mut().next() {
                c.add_fertile(revived_pop);
            }
            return;
        }
        // Per-cell proportional split. Collect keys then mutate so
        // we don't hold a borrow across the iteration.
        let cells: Vec<u32> = self.region_cohorts.keys().copied().collect();
        let mut deposited = Pop::ZERO;
        for cell in &cells {
            if let Some(cohort) = self.region_cohorts.get_mut(cell) {
                let share = cohort.fertile / total_fertile;
                let delta = revived_pop * share;
                cohort.add_fertile(delta);
                deposited = deposited + delta;
            }
        }
        // Any rounding residual goes to the largest cell so the
        // civ-level sum equals `revived_pop` exactly.
        let residual = revived_pop - deposited;
        if residual > Pop::ZERO {
            if let Some(cell) = cells
                .iter()
                .max_by_key(|&&c| self.region_cohorts.get(&c).map(|x| x.fertile))
            {
                if let Some(cohort) = self.region_cohorts.get_mut(cell) {
                    cohort.add_fertile(residual);
                }
            }
        }
        // Re-sum the civ-level fertile bracket so the aggregate
        // reflects the resurrection deposit.
        self.cohort.fertile = self
            .region_cohorts
            .values()
            .map(|c| c.fertile)
            .fold(Pop::ZERO, |a, b| a + b);
    }
}
