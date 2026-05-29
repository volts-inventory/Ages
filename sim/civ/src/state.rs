//! `Civ` constructors + basic accessors. Lifecycle decisions
//! (collapse, cohesion, capacity, drift, tool effects) live in the
//! sibling modules `lifecycle`, `capacity`, `drift`, `tools`,
//! `territory`, `observation`, `founding`.

use crate::cosmology;
use crate::demographics::attempt_period_for_cognition;
use crate::figures::{found_band, NameGrammar};
use crate::religion;
use crate::Civ;
use sim_arith::{Pop, Real};
use sim_population::{Cohort, PopulationDynamics};
use sim_recognition::Firing;
use sim_species::ModalityKind;
use std::collections::{BTreeMap, BTreeSet};

impl Civ {
    pub fn new(id: u32, founded_tick: u64, initial_population: Pop) -> Self {
        Self::with_species(
            id,
            founded_tick,
            initial_population,
            Real::ONE,
            0,
            &[],
            [Real::ZERO; 5],
        )
    }

    /// Construct a civ with an explicit intelligence factor for the
    /// hypothesis pipeline. Used by `sim-core` to thread the species'
    /// `cognition` trait into 's tolerance / minimum-sample
    /// formulas. Defaults to no modalities — falls back to gestural
    /// naming. Production callers use `with_species` to thread
    /// species sensorium into the name grammar.
    pub fn with_intelligence(
        id: u32,
        founded_tick: u64,
        initial_population: Pop,
        intelligence: Real,
    ) -> Self {
        Self::with_species(
            id,
            founded_tick,
            initial_population,
            intelligence,
            0,
            &[],
            [Real::ZERO; 5],
        )
    }

    /// Full constructor: thread species cognition + species seed +
    /// species modalities into the founding band so figure
    /// names reflect the species' communication channel and
    /// reproduce deterministically per (species, `civ_id`).
    pub fn with_species(
        id: u32,
        founded_tick: u64,
        initial_population: Pop,
        intelligence: Real,
        species_seed: u64,
        species_modalities: &[ModalityKind],
        species_initial_cosmology: [Real; 5],
    ) -> Self {
        let grammar = NameGrammar::derive(species_modalities, id, species_seed);
        let attempt_period = attempt_period_for_cognition(intelligence);
        // Sensor-gated candidate set: figures' hypothesizers see
        // only the channels the species can actually perceive.
        // Empty modality lists fall back to the universally-
        // touchable minimum (`Temperature`+`Elevation`) inside
        // `perceivable_channels_from_kinds`.
        let species_channels =
            crate::discovery::perceivable_channels_from_kinds(species_modalities);
        let (figures, next_figure_id) = found_band(
            &grammar,
            id,
            species_seed,
            founded_tick,
            1,
            intelligence,
            &[],
            attempt_period,
            Some(&species_channels),
        );
        let centroid = figures.first().map_or(0, |f| f.cell_assignment);
        // per-seed cosmology pole-position bias. The species
        // computed an initial vector at genesis; civs of this
        // species inherit it as their starting cosmology rather
        // than NEUTRAL. Drift mechanism then operates from this
        // starting point.
        let initial_cosmology = cosmology::Cosmology {
            empirical: species_initial_cosmology[0],
            communitarian: species_initial_cosmology[1],
            reformist: species_initial_cosmology[2],
            mystical: species_initial_cosmology[3],
            hierarchical: species_initial_cosmology[4],
        };
        // religion is the fast-divergent layer; unlike
        // cosmology, it does *not* inherit a species bias —
        // every species starts at neutral and civs differentiate
        // at founding via figure traits + civ_id-derived jitter.
        // Even two civs of the same species land on distinct
        // religion vectors at birth, mimicking real religious
        // genesis (founding-figure personality + early-event
        // chance).
        let founding_figure = figures.first();
        let initial_religion = religion::founding_religion(
            id,
            founding_figure.map_or(Real::ZERO, |f| f.charisma),
            founding_figure.map_or(Real::ZERO, |f| f.doubt),
            founding_figure.map_or(Real::ZERO, |f| f.curiosity),
        );
        Self {
            id,
            name: String::new(),
            founded_tick,
            cohort: Cohort::with_civ(initial_population, id),
            dynamics: PopulationDynamics::earth_like_default(),
            lifecycle_state: sim_population::LifecycleState::None,
            observations: BTreeMap::new(),
            intelligence,
            grammar,
            figures,
            next_figure_id,
            unlocked_tools: BTreeSet::new(),
            unlocked_dynamic_tools: Vec::new(),
            unlocked_channels: BTreeSet::new(),
            extra_perceivable_templates: BTreeSet::new(),
            tech_multiplier: Real::ONE,
            // matches `demographics::carrying_capacity_per_unit`
            // baseline (50,000/fuel-unit). `configure_substrate`
            // overwrites this with a biosphere-derived value at
            // founding; the default is here for callers that
            // build a `Civ` without threading planet context (legacy
            // unit tests).
            carrying_capacity_per_unit: Real::from_int(50_000),
            // Default Earth-equivalent surface area —
            // `configure_substrate_with_topology` overwrites with
            // `radius²` at founding. Legacy / test callers without
            // planet context keep the neutral 1.0 so their capacity
            // ratios match the pre-planet-scale behaviour.
            planet_area_factor: Real::ONE,
            // Default Terrestrial — `configure_substrate` overrides
            // at founding from the species' real habitat. Legacy
            // tests without that init path see land-only behaviour.
            species_habitat: sim_species::Habitat::Terrestrial,
            allied_with: std::collections::BTreeSet::new(),
            contact_history: std::collections::BTreeSet::new(),
            alliance_trust: std::collections::BTreeMap::new(),
            alliance_cooldown: std::collections::BTreeMap::new(),
            migration_pressure_threshold: Real::percent(85),
            collapsed_tick: None,
            last_discovery_tick: founded_tick,
            last_territory_emit_tick: founded_tick,
            low_food_streak: 0,
            parent_civ_id: None,
            firings_by_template: BTreeMap::new(),
            cosmology: initial_cosmology,
            last_emitted_cosmology: initial_cosmology,
            religion: initial_religion,
            last_emitted_religion: initial_religion,
            cultural_lock_streak: 0,
            last_refinement_tick: founded_tick,
            last_volcanic_tick: None,
            last_disease_tick: None,
            last_asteroid_tick: None,
            last_solar_flare_tick: None,
            last_ice_age_tick: None,
            last_catastrophe_tick: None,
            region_cohorts: BTreeMap::new(),
            claimed_cells: BTreeSet::new(),
            territory_centroid: centroid,
            peak_claimed_cells: 0,
            tiny_territory_streak: 0,
            depopulation_streak: 0,
            cohesion: Real::ONE,
            civil_war_streak: 0,
            cohesion_breakaway_streak: 0,
            last_emitted_cohesion: Real::ONE,
            last_emitted_life_expectancy_months: Real::ZERO,
            cognition_delta: Real::ZERO,
            sociality_delta: Real::ZERO,
            lifespan_delta_years: Real::ZERO,
            communication_fidelity_delta: Real::ZERO,
            apparatus_cells: Vec::new(),
            lineage_depth: 0,
            grudges: BTreeMap::new(),
            selection_bias: crate::environmental_drift::SelectionBias::zero(),
            surplus: Real::ZERO,
            last_emitted_surplus: Real::ZERO,
            // P0.5: producer biomass defaults to 1.0 so legacy
            // callers without ecosystem context get a non-zero
            // capacity baseline. Production sim/core overwrites
            // these on the first `step_population_per_cell` call
            // each tick from the live `PlanetEcosystem::tier_biomass(0)`.
            producer_biomass: Real::ONE,
            initial_producer_biomass: Real::ONE,
            ecological_resilience: Real::ONE,
            last_emitted_resilience: Real::ONE,
            // P1.3 — dormant pool starts empty; only catastrophes
            // populate it via `apply_resistance_and_dormancy`.
            // Resurrection cap anchored at the civ's initial
            // founding population so brand-new civs without any
            // observed peak still have a sensible target if a
            // catastrophe fires before the population has grown.
            dormant_pool: sim_species::DormantPool::EMPTY,
            pre_catastrophe_population: initial_population,
        }
    }

    pub fn is_active(&self) -> bool {
        self.collapsed_tick.is_none()
    }

    /// Mark a discovery moment for the knowledge-plateau detector.
    /// sim/core calls this whenever a `Confirmed` or
    /// `RefinementConfirmed` hypothesis event fires.
    pub fn note_discovery(&mut self, tick: u64) {
        self.last_discovery_tick = tick;
    }

    /// Bumped by sim/core whenever a `RefinementConfirmed`
    /// hypothesis event fires. reads it to detect
    /// cultural-lock (high dogmatism without refinement).
    pub fn note_refinement(&mut self, tick: u64) {
        self.last_refinement_tick = tick;
    }

    /// Apply collapse. Marks `collapsed_tick`, retires every active
    /// figure, and turns the cohort stateless (`civ_membership =
    /// None`) so 's founding pipeline reads it as input pool.
    pub fn collapse(&mut self, tick: u64) {
        if !self.is_active() {
            return;
        }
        self.collapsed_tick = Some(tick);
        self.cohort.civ_membership = None;
        for fig in &mut self.figures {
            if fig.retired_tick.is_none() {
                fig.retired_tick = Some(tick);
            }
        }
    }

    /// Inherit lineage depth from a parent civ. Increments by 1
    /// so the first successor lands at depth 1, the second at 2,
    /// etc. Call this immediately after constructing a refound
    /// or breakaway civ, alongside `inherit_species_drift`.
    pub fn inherit_lineage_from(&mut self, parent: &Civ) {
        self.lineage_depth = parent.lineage_depth.saturating_add(1);
    }

    /// Bump this civ's grudge against `other_id` by `amount`,
    /// stamping the current tick so kinship-read-time decay
    /// computes from this point. Caps at `GRUDGE_CEILING` so
    /// repeat-skirmish runaway is bounded.
    pub fn bump_grudge(&mut self, other_id: u32, amount: Real, tick: u64) {
        let entry = self.grudges.entry(other_id).or_insert((Real::ZERO, tick));
        let ceiling = Real::from_ratio(
            crate::conflict::GRUDGE_CEILING.0,
            crate::conflict::GRUDGE_CEILING.1,
        );
        let new_score = (entry.0 + amount).min(ceiling).max(Real::ZERO);
        *entry = (new_score, tick);
    }

    pub(crate) fn n_active_figures(&self) -> usize {
        self.figures
            .iter()
            .filter(|f| f.retired_tick.is_none())
            .count()
    }

    /// Helper: charisma of the figure with `figure_id`, or `0.5`
    /// if no figure matches (shouldn't happen for active civs).
    pub fn figure_charisma(&self, figure_id: u32) -> Real {
        self.figures
            .iter()
            .find(|f| f.id == figure_id)
            .map_or(Real::from_ratio(5, 10), |f| f.charisma)
    }

    /// Aggregate accessor — sums per-region cohort totals
    /// (`Cohort::total()`, all four brackets summed). Falls back
    /// to the civ-level `cohort.total()` if `region_cohorts` is
    /// empty.
    pub fn aggregate_population(&self) -> Pop {
        if self.region_cohorts.is_empty() {
            return self.cohort.total();
        }
        self.region_cohorts
            .values()
            .map(sim_population::Cohort::total)
            .fold(Pop::ZERO, |a, b| a + b)
    }

    /// Advance population dynamics one civ-sim tick.
    pub fn step_population(&mut self) {
        self.dynamics.step(&mut self.cohort);
    }

    /// Fold this tick's recognition firings into the civ's
    /// observation pool. For now: count per template. Sensorium
    /// gating (, M3) filters which firings the civ's species
    /// can perceive.
    pub fn observe(&mut self, firings: &[Firing]) {
        for firing in firings {
            *self.observations.entry(firing.template_id).or_insert(0) += 1;
        }
    }

    pub fn population(&self) -> Pop {
        self.cohort.total()
    }

    /// Life expectancy at birth, in months, given the civ's
    /// current `dynamics` snapshot. The dynamics is re-derived
    /// each tick (in `step_population_per_cell`) from the civ's
    /// effective traits + currently-unlocked tools, so this
    /// reflects up-to-date tech without needing a separate
    /// recomputation. Pure read; no allocation, no mutation.
    pub fn life_expectancy_months(&self) -> Real {
        self.dynamics.life_expectancy_months()
    }

    /// Life expectancy at birth, in years.
    pub fn life_expectancy_years(&self) -> Real {
        let baseline_months_per_year =
            Real::from_int(i64::try_from(protocol::BASELINE_MONTHS_PER_YEAR).unwrap_or(12));
        self.life_expectancy_months() / baseline_months_per_year
    }

    pub fn observation_count(&self, template_id: u32) -> u64 {
        self.observations.get(&template_id).copied().unwrap_or(0)
    }
}
