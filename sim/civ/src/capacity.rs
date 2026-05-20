//! Carrying-capacity computations and capacity-coupled population
//! steps. Per-cell and aggregate capacity formulas share the same
//! fuel × per-unit × tech-multiplier × tool-multiplier × seasonal
//! × terrain stack.

use crate::Civ;
use sim_arith::{Pop, Real};
use sim_species::Habitat;

/// Per-cell habitability scaled to this civ's species habitat.
/// Terrestrial / Airborne see water cells as marginal (shallow sea
/// at 0.05, deep ocean at 0.00) and land cells as native (0.10
/// peak through 1.20 coast). Aquatic flip the relationship —
/// water is native, land cells drop to 0.05-0.10. Amphibious see
/// both as native. Coast stays universally habitable (transition
/// zone, valued by every habitat for the same reason).
fn species_habitability(glyph: char, habitat: Habitat) -> Real {
    let water_native = matches!(habitat, Habitat::Aquatic | Habitat::Amphibious);
    let land_native = matches!(
        habitat,
        Habitat::Terrestrial
            | Habitat::Airborne
            | Habitat::Amphibious
            | Habitat::Subterranean
            | Habitat::Endolithic
    );
    let water_bonus = Real::percent(120);
    let water_marginal = Real::percent(5);
    let land_marginal = Real::percent(5);
    match glyph {
        // ≈ deep ocean — only Aquatic / Amphibious can live here.
        '\u{2248}' => {
            if water_native {
                Real::ONE
            } else {
                Real::ZERO
            }
        }
        // ≡ gas band — uninhabitable for everyone.
        '\u{2261}' => Real::ZERO,
        // ~ shallow sea — native for aquatic, marginal for land.
        '~' => {
            if water_native {
                water_bonus
            } else {
                water_marginal
            }
        }
        // ░ coast — both habitats value the transition zone.
        '\u{2591}' => water_bonus,
        // ▒ inland — native for land, marginal for aquatic.
        '\u{2592}' => {
            if land_native {
                Real::percent(90)
            } else {
                land_marginal * Real::from_int(2)
            }
        }
        // △ hill / low mountain.
        '\u{25B3}' => {
            if land_native {
                Real::percent(60)
            } else {
                land_marginal
            }
        }
        // ▲ peak.
        '\u{25B2}' => {
            if land_native {
                Real::percent(10)
            } else {
                land_marginal
            }
        }
        // · plain / featureless / wildcard.
        _ => {
            if land_native {
                Real::ONE
            } else {
                land_marginal * Real::from_int(2)
            }
        }
    }
}

impl Civ {
    /// Carrying capacity: derived from the civ's claimed cells'
    /// biological-stock proxy (summed `Substance::Fuel` density
    /// across `claimed_cells`), scaled by `tech_multiplier`. Fuel
    /// stands in for ecosystem productivity in the M3 chemistry
    /// model; richer biospheres support larger civs. The per-cell
    /// aggregation closes territory's negative-feedback loop
    /// (small claim → less fuel → smaller pop → smaller claim) so
    /// dark ages are mechanical, not just narrative. Pre-territory
    /// civs (empty `claimed_cells`) fall back to the planetary
    /// total — keeps the M2-era unit tests valid.
    ///
    /// Calibration: per-unit retained at 50; sum is over claimed
    /// cells only. A 5-cell founding civ on a fuel-rich seed gets
    /// ~5 × 1.5 × 50 = 375 capacity (7× the founding pop of 50,
    /// so growth headroom is fine). A fully-territorial civ scales
    /// linearly to 7000+ capacity. Decline reverses it: shedding
    /// cells drops capacity, which throttles regrowth.
    pub fn carrying_capacity(&self, state: &sim_physics::PhysicsState) -> Pop {
        let fuel = state.substance(sim_physics::Substance::Fuel.idx());
        let fuel_total = if self.claimed_cells.is_empty() {
            // Pre-territory case: fall back to the planet-wide sum.
            // Preserves M2/M3 unit-test behaviour for civs created
            // without `claim_cells`.
            fuel.iter().copied().fold(Real::ZERO, |a, b| a + b)
        } else {
            self.claimed_cells
                .iter()
                .filter_map(|c| fuel.get(*c as usize).copied())
                .fold(Real::ZERO, |a, b| a + b)
        };
        // tool-derived multiplicative tech multiplier folds
        // in alongside the existing `self.tech_multiplier`
        // placeholder. Tier-1 tools (LocalisedCombustion +15%,
        // FoodProcessing +15%, StoneWorking +5%) stack
        // multiplicatively; tiers 2-5 add larger factors as the
        // tech tree fills.
        let mult = fuel_total * self.tech_multiplier * self.tool_capacity_multiplier();
        Pop::from_real(self.carrying_capacity_per_unit) * mult
    }

    /// terrain-aware carrying capacity. Same fuel-summed
    /// formula as `carrying_capacity`, but each per-cell
    /// contribution is multiplied by the cell's terrain
    /// habitability multiplier (`sim_world::cell_habitability`).
    /// Deep ocean and gas-band cells contribute zero; coast cells
    /// get a 1.20× boost; peaks contribute almost nothing.
    ///
    /// Pre-territory civs (`claimed_cells.is_empty()`) fall back
    /// to the planet-wide untouched fuel sum so legacy unit tests
    /// that don't run `claim_cells` still see non-zero capacity.
    pub fn carrying_capacity_with_terrain(
        &self,
        state: &sim_physics::PhysicsState,
        planet: &sim_world::Planet,
    ) -> Pop {
        let fuel = state.substance(sim_physics::Substance::Fuel.idx());
        let fuel_total = if self.claimed_cells.is_empty() {
            fuel.iter().copied().fold(Real::ZERO, |a, b| a + b)
        } else {
            self.claimed_cells
                .iter()
                .map(|&c| {
                    let f = fuel.get(c as usize).copied().unwrap_or(Real::ZERO);
                    let glyph = sim_world::terrain_glyph_at(state, planet, c);
                    let h = species_habitability(glyph, self.species_habitat);
                    f * h
                })
                .fold(Real::ZERO, |a, b| a + b)
        };
        let mult = fuel_total * self.tech_multiplier * self.tool_capacity_multiplier();
        Pop::from_real(self.carrying_capacity_per_unit) * mult
    }

    /// season-aware carrying capacity. Sums per-cell capacity
    /// where each cell's contribution is its fuel-derived base
    /// scaled by the seasonal factor for `(tick, cell, planet)`.
    /// Pre-territory civs fall back to base `carrying_capacity`
    /// (no seasonal modulation; preserves legacy unit tests that
    /// don't thread tick/planet through).
    pub fn seasonal_carrying_capacity(
        &self,
        state: &sim_physics::PhysicsState,
        tick: u64,
        planet: &sim_world::Planet,
    ) -> Pop {
        if self.claimed_cells.is_empty() {
            return self.carrying_capacity(state);
        }
        let fuel = state.substance(sim_physics::Substance::Fuel.idx());
        let temps = state.temperature();
        let pop_per_unit = Pop::from_real(self.carrying_capacity_per_unit);
        let grid = state.grid();
        self.claimed_cells
            .iter()
            .map(|&c| {
                let base_fuel = fuel.get(c as usize).copied().unwrap_or(Real::ZERO);
                let base_temp = temps.get(c as usize).copied().unwrap_or(Real::ZERO);
                let offset = sim_world::seasonal_temperature_offset(tick, c, planet, grid);
                let cell_temp = base_temp + offset;
                let raw_factor = sim_world::seasonal_capacity_factor(cell_temp, planet);
                // tools that lift the seasonal floor (SimpleShelter,
                // BasicTextiles, PermanentMasonry, FluidControl,
                // UrbanConstruction, EnergyStorage) shrink the per-cell
                // seasonal penalty. A SimpleShelter civ's worst-season
                // cells hold ~10% more capacity than they would
                // pre-shelter.
                let factor = self.effective_seasonal_factor(raw_factor);
                // terrain habitability — multiply by the cell's
                // glyph-derived multiplier so deep ocean / gas band
                // contribute zero, coast gets a 1.20× boost, peaks
                // contribute ~10%.
                let glyph = sim_world::terrain_glyph_at(state, planet, c);
                let hab = species_habitability(glyph, self.species_habitat);
                let mult = base_fuel
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
    pub fn step_population_with_capacity(&mut self, state: &sim_physics::PhysicsState) {
        let cap = self.carrying_capacity(state);
        self.dynamics.mortality_reduction = self.tool_mortality_reduction_per_bracket();
        self.dynamics.birth_rate_multiplier = Real::ONE + self.tool_fertility_bonus();
        self.dynamics.step_with_capacity(&mut self.cohort, cap);
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
        self.dynamics.step_with_capacity(&mut self.cohort, cap);
    }

    /// per-cell capacity. Same fuel-density formula as the
    /// aggregate `carrying_capacity` restricted to one cell;
    /// applies the seasonal factor so winter cells throttle
    /// in their bite.
    pub fn cell_capacity(
        &self,
        state: &sim_physics::PhysicsState,
        cell: u32,
        tick: u64,
        planet: &sim_world::Planet,
    ) -> Pop {
        let fuel = state.substance(sim_physics::Substance::Fuel.idx());
        let base_fuel = fuel.get(cell as usize).copied().unwrap_or(Real::ZERO);
        let temps = state.temperature();
        let base_temp = temps.get(cell as usize).copied().unwrap_or(Real::ZERO);
        let offset = sim_world::seasonal_temperature_offset(tick, cell, planet, state.grid());
        let cell_temp = base_temp + offset;
        let raw_factor = sim_world::seasonal_capacity_factor(cell_temp, planet);
        // same seasonal-floor lift as in
        // `seasonal_carrying_capacity`. Per-cell migration via
        // sees the lifted factor so winter cells with shelter shed
        // less population.
        let factor = self.effective_seasonal_factor(raw_factor);
        // Terrain habitability — gates per-cell capacity by
        // the cell's glyph-derived multiplier (zero for deep ocean
        // or gas, ~10% for peaks, 1.20 for coast). Per-cell
        // migration sees the gated capacity, so high-
        // pressure cells naturally shed pop toward better terrain.
        let glyph = sim_world::terrain_glyph_at(state, planet, cell);
        let hab = species_habitability(glyph, self.species_habitat);
        let mult =
            base_fuel * self.tech_multiplier * self.tool_capacity_multiplier() * factor * hab;
        Pop::from_real(self.carrying_capacity_per_unit) * mult
    }

    /// per-cell population step. Each `region_cohort` evolves
    /// independently, but with a *smoothed* capacity-share rather
    /// than its raw `cell_capacity`. Pre-smoothing, low-fuel cells
    /// in heterogeneous worlds (ocean planets, sparse biospheres)
    /// produced runaway local stress: cell pop crashed on
    /// founding, the aggregate fell below the food-crisis floor,
    /// and the civ went extinct at tick ≈ `FOOD_CRISIS_STREAK_TICKS`
    /// regardless of the *aggregate* food story being fine.
    ///
    /// Each cell evolves toward its own `cell_capacity` (tech ×
    /// terrain × seasonal × biosphere). Earlier model used
    /// `share_capacity = aggregate_capacity / n_cells` to spread
    /// stress uniformly — but that made expansion self-defeating
    /// in the per-cell-cap territorial model: claiming a new cell
    /// dropped each cell's `share_cap` proportionally, which
    /// removed the very pressure that triggered the expansion.
    /// Civs ended up stuck at the founding territory size.
    ///
    /// New behaviour: each cell steps with its real per-cell cap.
    /// Heterogeneous terrain now produces heterogeneous density —
    /// fertile valleys fill, peaks and deserts stay sparse, and
    /// expansion (overflow + claim) keeps producing lasting
    /// pressure as the frontier finds richer ground. Low-fuel
    /// cells that die out get pruned via `prune_empty_cells` rather
    /// than dragging the civ-aggregate stress reading down.
    pub fn step_population_per_cell(
        &mut self,
        state: &sim_physics::PhysicsState,
        tick: u64,
        planet: &sim_world::Planet,
        species: &sim_species::Species,
    ) {
        if self.region_cohorts.is_empty() {
            self.step_population_with_seasonal_capacity(state, tick, planet, species);
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
                self.dynamics.step_with_capacity(cohort, cap);
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
    }
}
