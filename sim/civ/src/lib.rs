//! `sim-civ` — civilizations and their lifecycle.
//!
//! The species is the persistent unit and civilizations are
//! bounded collectivities within it. M2 scaffold: a single civ
//! exists from run start; cohort-level observation pool tracks
//! recognition firings per template per civ-sim tick. M3 adds the
//! quantitative discovery pipeline — see
//! the `forms` and `fit` modules below; named figures and the
//! refinement lifecycle land in a follow-up commit. M4 adds the
//! founding/collapse/succession lifecycle.
//!
//! `Civ` impl methods are split across role-focused sibling modules:
//! `state` (constructors / accessors / collapse marker), `drift`
//! (species drift), `founding` (refound + literacy), `tools`
//! (effect aggregators + sensorium gating), `lifecycle`
//! (collapse evaluation + cohesion), `territory`
//! (per-cell pop + migration), `capacity` (carrying
//! capacity), `observation` (per-figure observe + step + cosmology +
//! form refresh).

#![allow(clippy::module_name_repetitions)]

pub mod apparatus;
pub mod catastrophe;
pub mod conflict;
pub mod cosmology;
pub mod culture_hooks;
mod demographics;
pub mod discovery;
pub mod figures;
pub mod fit;
pub mod forms;
mod naming;
pub mod religion;
mod succession;
pub mod tech;
pub mod transmission;

mod capacity;
mod drift;
mod founding;
mod lifecycle;
mod observation;
mod state;
mod territory;
mod tools;

pub use demographics::dynamics_for_civ;
pub use demographics::{
    attempt_period_for_cognition, biosphere_birth_factor, biosphere_birth_factor_for_planet,
    carrying_capacity_per_unit, dynamics_for, founding_min_population,
    migration_pressure_threshold, scale_attempt_period_for_metabolism,
    streak_ticks_for_metabolism, tech_augmented_migration_threshold,
};
pub use naming::civ_name_from_seed;
pub use succession::pick_successor_centroid;

use figures::{NameGrammar, NamedFigure};
use sim_arith::Real;
pub use sim_population::Cohort;
use sim_population::PopulationDynamics;
use sim_recognition::ChannelKind;
use std::collections::{BTreeMap, BTreeSet};
use tech::ToolKind;

// Demographics helpers live in `mod demographics`.
// Successor centroid pick lives in `mod succession`.

// Deterministic civ-name pool lives in `mod naming`.

/// A civilization. M2 scaffold — single cohort, no figures, no
/// culture. Later milestones refine the shape of this struct.
#[derive(Debug, Clone)]
pub struct Civ {
    pub id: u32,
    /// Deterministic civilisation name from `(seed, civ_id)`.
    /// Sampled once at founding via `civ_name_from_seed`; same
    /// `(seed, civ_id)` produces the same string byte-for-byte.
    /// Plumbs into `CivFounded` on emission so the narrator can
    /// say "Eldoria took root" instead of "Civilization 1 took
    /// root". Defaults to empty string for legacy callers
    /// (`Civ::new`, `Civ::with_intelligence`, tests) that don't
    /// thread a seed; production sites in sim/core assign the
    /// name immediately after the constructor returns.
    pub name: String,
    pub founded_tick: u64,
    pub cohort: Cohort,
    pub dynamics: PopulationDynamics,
    /// Cohort-level observation summary. For each recognition
    /// template id, a running count of firings observed across the
    /// civ's lifetime. Cohort tracking is summary-only —
    /// per-figure data points belong to named figures (M3+).
    pub observations: BTreeMap<u32, u64>,
    /// Cached intelligence factor — passed into per-figure
    /// `Hypothesizer` instances at founding and as new figures join.
    pub intelligence: Real,
    /// Per-civ name grammar. Strategy chosen from the species'
    /// communication modalities — acoustic species get syllabic
    /// names, visual species get brightness-pattern names, chemical
    /// species get compound names, etc. Sampled deterministically
    /// at founding.
    pub grammar: NameGrammar,
    /// Active and retired named figures within the civ. The
    /// founding band lands at `founded_tick`; M4 grows the roster
    /// as population × literacy × institution count crosses
    /// thresholds.
    pub figures: Vec<NamedFigure>,
    /// Next figure id to assign. Stable for the run.
    pub next_figure_id: u32,
    /// Sensorium-extending tools the civ has unlocked.
    pub unlocked_tools: BTreeSet<ToolKind>,
    /// Dynamic-tool unlocks. Owned copies of the species-
    /// registry records — keeps the effect-aggregator methods
    /// `&self` (no need to thread `&Species` everywhere). Sorted
    /// by tool id for deterministic effect-fold order.
    pub unlocked_dynamic_tools: Vec<sim_species::DynamicTool>,
    /// Channels granted by unlocked tools. Unioned with species-
    /// native modalities for sensorium gating; new templates whose
    /// channels intersect become perceivable.
    pub unlocked_channels: BTreeSet<ChannelKind>,
    /// Recognition templates the civ now perceives via tool grants
    /// that the species couldn't perceive natively. Refreshed each
    /// time a new tool unlocks; consumed by `perceivable_firings`
    /// alongside the species' baseline perceivable set.
    pub extra_perceivable_templates: BTreeSet<u32>,
    /// Tech multiplier — product of cultivation / domestication
    /// / irrigation / storage / industrial-agriculture multipliers
    /// (M4 grows the list). M3 placeholder pinned at `1.0`; the
    /// civ's carrying capacity scales with this. **Orthogonal to
    /// sensorium tools** — those extend perception, not
    /// productivity. M4 productivity tech populates this slot.
    pub tech_multiplier: Real,
    /// Substrate-derived per-fuel-unit individual count for
    /// `carrying_capacity`. Replaces the flat 50 placeholder. Cached
    /// at founding via `configure_substrate`; default 50 (Earth-
    /// equivalent) for legacy callers that construct without
    /// substrate context.
    pub carrying_capacity_per_unit: Real,
    /// Substrate-derived migration pressure threshold. Replaces
    /// the flat 0.85 in `apply_migration`. Cached at founding via
    /// `configure_substrate`; default 0.85 for legacy callers.
    pub migration_pressure_threshold: Real,
    /// Tick at which the civ collapsed; `None` while active.
    /// Set by `check_collapse` when food-security or knowledge-
    /// plateau streaks cross threshold.
    pub collapsed_tick: Option<u64>,
    /// Tick of the most recent confirmed or refinement-confirmed
    /// hypothesis event. Knowledge-plateau collapse fires when
    /// `tick - last_discovery_tick >= PLATEAU_WINDOW_TICKS`.
    /// Initialised to the founding tick.
    pub last_discovery_tick: u64,
    /// Tick of the most recent `CivTerritoryChanged` emission for
    /// this civ. The territory phase re-emits when the claim set
    /// changes (the natural trigger) and also forces a refresh
    /// every `TERRITORY_REFRESH_TICKS`, so the viewport's
    /// per-cell pop/cap digits stay current as tech, seasonal,
    /// and biosphere multipliers drift even during long stretches
    /// of stable territory. Seeded to `founded_tick` at every
    /// founding site (refound, breakaway, emergent).
    pub last_territory_emit_tick: u64,
    /// Consecutive ticks `food_security <= 0.3`. Reset on
    /// recovery; collapse fires at `FOOD_CRISIS_STREAK_TICKS`.
    pub low_food_streak: u64,
    /// Id of the predecessor civ (the one whose collapse left
    /// the stateless population this civ refounded from), or `None`
    /// for the first civ in the run.
    pub parent_civ_id: Option<u32>,
    /// Cumulative observed-firing counter, keyed by the
    /// recognition `template_id`. Drives the observation-pressure
    /// gate on tool unlocks. Updated each tick from the civ's
    /// perceivable firings; never decreases.
    pub firings_by_template: BTreeMap<u32, u64>,
    /// Five-axis cosmology vector. Drifts on hypothesis +
    /// lifecycle events. Read by culture hooks.
    /// This is the *slow-drift* worldview layer (the
    /// deep cosmology shared across civs of one species via the
    /// species bias). The fast-divergent religion / customs
    /// layer lives in `religion` below.
    pub cosmology: cosmology::Cosmology,
    /// Last cosmology snapshot emitted as a `CosmologyShifted`
    /// event. Used by sim/core to gate event emission so only
    /// non-trivial drifts show up in the log.
    pub last_emitted_cosmology: cosmology::Cosmology,
    /// Three-axis religion / customs vector. Drifts on the
    /// same hypothesis + lifecycle events as cosmology but at
    /// 3× magnitude — this is the fast layer that absorbs schism
    /// dynamics (Reformation, sectarian conflict) and drives
    /// intra-species war via the kinship weighting.
    pub religion: religion::Religion,
    /// Last religion snapshot emitted as a `ReligionShifted`
    /// event. Same gating semantics as `last_emitted_cosmology`.
    pub last_emitted_religion: religion::Religion,
    /// Cultural-lock streak: consecutive ticks
    /// `dogmatism > CULTURAL_LOCK_DOGMA` with no refinement-
    /// confirmed events.
    pub cultural_lock_streak: u64,
    /// Last refinement tick: bumped on
    /// `RefinementConfirmed`; used to verify "no refinements
    /// during the cultural-lock window."
    pub last_refinement_tick: u64,
    /// Catastrophe bookkeeping — last tick each kind fired.
    pub last_volcanic_tick: Option<u64>,
    pub last_disease_tick: Option<u64>,
    pub last_asteroid_tick: Option<u64>,
    pub last_solar_flare_tick: Option<u64>,
    pub last_ice_age_tick: Option<u64>,
    /// Most recent catastrophe of any kind. Drives
    /// the post-catastrophe founding trigger.
    pub last_catastrophe_tick: Option<u64>,
    /// Per-cell cohort breakdown. Sums to `cohort.count`
    /// (the aggregate). M5 v1 distributes `initial_population`
    /// evenly across `claimed_cells`; v2 adds migration between
    /// cells under food-security pressure.
    pub region_cohorts: BTreeMap<u32, Cohort>,
    /// Set of cell ids this civ claims as its territory.
    /// Tracks population: target count is
    /// `ceil(population / PEOPLE_PER_CELL)`, computed by sim/core
    /// each tick. Cells are added/dropped from `territory_centroid`
    /// outward via deterministic BFS so contraction sheds the most
    /// distant cells first. Volcanic catastrophe targets the
    /// cell's region cohort directly.
    pub claimed_cells: BTreeSet<u32>,
    /// Anchor for territory growth/contraction. Set at
    /// founding to the first figure's `cell_assignment` (the
    /// founders' attention focus); never moves. BFS from this
    /// cell defines which cells the civ claims first as it grows
    /// and which it keeps as it shrinks.
    pub territory_centroid: u32,
    /// High-water mark of `claimed_cells.len()` over the civ's
    /// lifetime. Updated each time `claim_cells` runs. Drives
    /// `settlement_persistence_multiplier` — a civ that grew large
    /// at peak left distributed archives across its territory, so
    /// successor civs inherit more knowledge from it via inter-civ
    /// transmission even if the civ shrank back to one cell before
    /// collapse.
    pub peak_claimed_cells: u32,
    /// Consecutive ticks the civ has held
    /// `claimed_cells.len() <= TINY_TERRITORY_CELLS`. Reset whenever
    /// territory recovers above the floor; collapse fires at
    /// `TINY_TERRITORY_STREAK_TICKS`. Lets successor pressure
    /// finish off a parent that's been squeezed to a
    /// single cell rather than letting it linger indefinitely.
    pub tiny_territory_streak: u64,
    /// Consecutive ticks the civ has held
    /// `aggregate_population <= DEPOPULATION_FLOOR_POP`. Reset
    /// whenever pop recovers above the floor; collapse fires at
    /// `DEPOPULATION_STREAK_TICKS` with reason
    /// `CollapseReason::Depopulation`. Closes the "civ exists with
    /// 0 people" gap that the streak-only triggers leave open
    /// after a catastrophe / war / starvation drains cohorts below
    /// the rendering precision floor.
    pub depopulation_streak: u64,
    /// Internal cohesion in `[0, 1]`. 1.0 = unified populace
    /// (founding state); 0.0 = fully fragmented. Drifts each tick
    /// toward an equilibrium that depends on civ size, food
    /// security, dogmatism, and literacy:
    ///
    /// - Larger civs (more cells) drift toward lower cohesion —
    ///   regional divergence and authority-stretch.
    /// - Low food security accelerates fragmentation (hungry
    ///   regions break from the centre).
    /// - High dogmatism *holds together* longer (shared belief).
    /// - High literacy *also* holds together (shared canon, common
    ///   institutions).
    ///
    /// Drops below `CIVIL_WAR_COHESION_FLOOR` for
    /// `CIVIL_WAR_STREAK_TICKS` ticks → `CollapseReason::CivilWar`.
    /// Future PRs can add a sub-population breakaway path keyed on
    /// cohesion (regional faction → new civ).
    pub cohesion: Real,
    /// Consecutive ticks `cohesion < CIVIL_WAR_COHESION_FLOOR`.
    /// Reset whenever cohesion recovers above the floor.
    pub civil_war_streak: u64,
    /// Consecutive ticks the civ sits in the breakaway-fragmentation
    /// zone (`cohesion ∈ [CIVIL_WAR_COHESION_FLOOR,
    /// COHESION_BREAKAWAY_TRIGGER]`). When this exceeds
    /// `COHESION_BREAKAWAY_STREAK_TICKS` and the global breakaway
    /// cooldown is clear, sim/core forks a new civ off this one
    /// — a regional faction succeeds and takes ~30% of the
    /// parent's population. Reset to zero when cohesion exits
    /// the zone (recovers above trigger or drops below floor).
    pub cohesion_breakaway_streak: u64,
    /// Last cohesion value emitted as a `CohesionShifted`
    /// event. Used by sim/core to gate event emission so only
    /// non-trivial shifts (≥ 0.05 absolute change) show in the log.
    pub last_emitted_cohesion: Real,
    /// Last life expectancy (months) emitted as a
    /// `CivLifeExpectancyChanged` event. Used by sim/core to
    /// gate emission so only meaningful shifts (≥ 24 months,
    /// i.e. 2 years at the `BASELINE_MONTHS_PER_YEAR` baseline)
    /// hit the log. Initialized to a sentinel zero at founding;
    /// the founding-tick emission populates it with the actual
    /// expectancy.
    pub last_emitted_life_expectancy_months: Real,
    /// Per-civ species drift on `cognition`. Each successor
    /// civ inherits its parent's delta and adds a small
    /// deterministic perturbation in `[-SUCCESSOR_DRIFT_TRAIT_STEP,
    /// +SUCCESSOR_DRIFT_TRAIT_STEP]`. The civ's *effective* cognition
    /// is `species.cognition + cognition_delta`, clamped to `[0, 1]`.
    /// Inaugural civs start at zero. Drives gradual subspecies
    /// divergence over a long civ chain.
    pub cognition_delta: Real,
    /// Per-civ species drift on `sociality`. See
    /// `cognition_delta`.
    pub sociality_delta: Real,
    /// Per-civ species drift on `lifespan_years`. Same
    /// inherit-+-perturb mechanic but the perturbation is in years
    /// (`±SUCCESSOR_DRIFT_LIFESPAN_STEP_YEARS`); effective lifespan is
    /// clamped to `[1, 1000]`. Drives the lifespan-relative rate
    /// scaling in `dynamics_for_civ` so a long civ chain can shift
    /// the species toward longer or shorter generations.
    pub lifespan_delta_years: Real,
    /// Per-civ species drift on `communication_fidelity`.
    /// See `cognition_delta`.
    pub communication_fidelity_delta: Real,
    /// Experiment-apparatus cells the civ has built. Empty
    /// until `ToolKind::ExperimentApparatus` unlocks; one apparatus
    /// is allocated at unlock time inside the civ's claimed cells.
    /// Each apparatus clamps a physics channel pre-tick and reads
    /// the post-physics response — controlled-conditions intervention
    /// alongside the passive observation track. Stable order; a
    /// successor civ does *not* inherit apparatus (a successor
    /// rebuilds its own when it re-unlocks the tool).
    pub apparatus_cells: Vec<apparatus::Apparatus>,
}

/// Collapse triggers. Multiple may compound; whichever streak
/// crosses threshold first wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollapseReason {
    FoodCrisis,
    KnowledgePlateau,
    CulturalLock,
    /// Civ has been squeezed to ≤ `TINY_TERRITORY_CELLS` for
    /// `TINY_TERRITORY_STREAK_TICKS` consecutive ticks.
    TerritoryTooSmall,
    /// Cohesion has stayed below `CIVIL_WAR_COHESION_FLOOR`
    /// for `CIVIL_WAR_STREAK_TICKS` consecutive ticks. The civ has
    /// fragmented internally — factions, regional divergence,
    /// language drift between separated regions — and the central
    /// authority can no longer hold the polity together. Fires
    /// alongside the other collapse reasons; whichever streak
    /// crosses threshold first wins.
    CivilWar,
    /// Aggregate population has stayed at or below
    /// `DEPOPULATION_FLOOR_POP` for `DEPOPULATION_STREAK_TICKS`
    /// consecutive ticks. Catches the "zombie civ" case where a
    /// civ's cells have been drained near zero by catastrophe /
    /// starvation / combat but none of the streak-based triggers
    /// (food crisis, territory-too-small, etc.) have crossed
    /// threshold yet. Without this gate the viewport sidebar reads
    /// "0p" (or worse, "-0p" from f64 noise) for civs that are
    /// physically empty but still nominally alive.
    Depopulation,
}

impl CollapseReason {
    pub fn tag(self) -> &'static str {
        match self {
            CollapseReason::FoodCrisis => "food_crisis",
            CollapseReason::KnowledgePlateau => "knowledge_plateau",
            CollapseReason::CulturalLock => "cultural_lock",
            CollapseReason::TerritoryTooSmall => "territory_too_small",
            CollapseReason::CivilWar => "civil_war",
            CollapseReason::Depopulation => "depopulation",
        }
    }
}

/// Lifecycle constants — placeholders. Reference-run calibration
/// in M4 follow-up. Scaled ×12 so "100 years of food crisis" stays
/// 100 years under the 1 tick = 1 month cadence.
pub const FOOD_CRISIS_THRESHOLD: (i64, i64) = (3, 10);
pub const FOOD_CRISIS_STREAK_TICKS: u64 = 100 * protocol::MONTHS_PER_YEAR;
pub const PLATEAU_WINDOW_TICKS: u64 = 500 * protocol::MONTHS_PER_YEAR;
/// Cultural-lock thresholds.
pub const CULTURAL_LOCK_DOGMA: (i64, i64) = (85, 100);
pub const CULTURAL_LOCK_STREAK_TICKS: u64 = 250 * protocol::MONTHS_PER_YEAR;
/// Territory-too-small collapse trigger. A civ whose
/// `claimed_cells.len() <= TINY_TERRITORY_CELLS` for
/// `TINY_TERRITORY_STREAK_TICKS` consecutive ticks (~2 sim-years
/// at 1 tick = 1 month) collapses with reason `territory_too_small`.
/// Lets successor pressure actually finish off a parent
/// that has been squeezed to a single cell rather than letting it
/// linger indefinitely.
pub const TINY_TERRITORY_CELLS: usize = 1;
pub const TINY_TERRITORY_STREAK_TICKS: u64 = 2 * protocol::MONTHS_PER_YEAR;

/// Depopulation collapse trigger. `aggregate_population <=
/// DEPOPULATION_FLOOR_POP` (in people, Real-int comparable) for
/// `DEPOPULATION_STREAK_TICKS` consecutive ticks fires
/// `CollapseReason::Depopulation`. Floor of 1 person is the
/// rendering precision floor — `{:.0}p` renders anything below
/// that as "0p" anyway, so a civ in this state has no visible
/// populace. Streak matches the tiny-territory window
/// (2 baseline-years) so brief catastrophe-driven dips that
/// recover within a couple of years don't pre-empt other
/// triggers.
pub const DEPOPULATION_FLOOR_POP: i64 = 1;
pub const DEPOPULATION_STREAK_TICKS: u64 = 2 * protocol::MONTHS_PER_YEAR;

/// Cohesion floor for the civil-war collapse trigger. A civ
/// whose `cohesion < CIVIL_WAR_COHESION_FLOOR` for
/// `CIVIL_WAR_STREAK_TICKS` consecutive ticks collapses with
/// reason `civil_war`. 0.10 is "near-fragmented" — the civ has
/// been on the brink for a sustained span. Calibration: tunable.
pub const CIVIL_WAR_COHESION_FLOOR: (i64, i64) = (1, 10);
/// Sustained-low-cohesion duration before civil war fires.
/// 75 baseline-years — long enough that brief crises don't break
/// the civ but a chronic fracture does. Calibration: tunable.
pub const CIVIL_WAR_STREAK_TICKS: u64 = 75 * protocol::MONTHS_PER_YEAR;
/// Cohesion-shifted event emission threshold. Sim/core only
/// emits a `CohesionShifted` when the absolute change since the
/// last emission ≥ this amount, so the log isn't noisy from
/// per-tick microdrift.
pub const COHESION_EMIT_THRESHOLD: (i64, i64) = (5, 100);

/// Cohesion ceiling for the breakaway-fragmentation zone.
/// A civ whose cohesion stays in `[CIVIL_WAR_COHESION_FLOOR,
/// COHESION_BREAKAWAY_TRIGGER]` (0.10 – 0.35) for the streak
/// duration spawns a regional-faction breakaway civ — not yet
/// fragmented enough to collapse but no longer holding together.
pub const COHESION_BREAKAWAY_TRIGGER: (i64, i64) = (35, 100);
/// Sustained-fragmentation duration before the cohesion
/// breakaway fires. 40 baseline-years — shorter than civil-war
/// streak (75y) so a falling-apart civ has a chance to fork off
/// a faction before the polity actually collapses.
pub const COHESION_BREAKAWAY_STREAK_TICKS: u64 = 40 * protocol::MONTHS_PER_YEAR;
/// Fraction of the parent's cohort that follows the
/// breakaway faction. 30% — enough to seat a viable new civ
/// without collapsing the parent.
pub const COHESION_BREAKAWAY_SHARE: (i64, i64) = (3, 10);
/// Cohesion the breakaway civ starts at — fresh authority,
/// shared cause, smaller scale. Higher than the parent's current
/// cohesion so the new civ has runway to stabilise.
pub const COHESION_BREAKAWAY_INITIAL: (i64, i64) = (85, 100);
/// Cohesion recovery the parent gets when the breakaway
/// fires — the disgruntled faction left. Capped so it can't push
/// the parent above 1.0.
pub const COHESION_PARENT_RECOVERY: (i64, i64) = (15, 100);

/// Literacy weights — placeholders.
pub const LITERACY_DISCOVERY_WEIGHT: (i64, i64) = (4, 100);
pub const LITERACY_TIER_WEIGHT: (i64, i64) = (20, 100);
pub const LITERACY_LIFESPAN_WEIGHT: (i64, i64) = (10, 100);
pub const LITERACY_LIFESPAN_DENOM: i64 = 500;

/// Founding constants. Stateless population must be ≥ this
/// for the remnant trigger to fire; the recent-collapse window
/// caps how stale a remnant can be; and the minimum-dark-age
/// gate enforces a visible gap between civs so M4 acceptance's
/// "dark age" reads as more than a same-tick refound.
pub const FOUNDING_MIN_POPULATION: i64 = 100;
pub const RECENT_REMNANT_WINDOW_TICKS: u64 = 250 * protocol::MONTHS_PER_YEAR;
pub const FOUNDING_MIN_DARK_AGE_TICKS: u64 = 50 * protocol::MONTHS_PER_YEAR;

/// Per-generation drift step on the unit-range traits
/// (cognition / sociality / `communication_fidelity`). Sampled in
/// `[-SUCCESSOR_DRIFT_TRAIT_STEP, +SUCCESSOR_DRIFT_TRAIT_STEP]` per civ
/// founding so a long civ chain (10+ successors) can accumulate
/// enough drift to meaningfully reshape the species' effective
/// traits without any single founding moving the needle a lot.
/// 2/100 = 0.02 per generation; ~ ±20% trait shift over 10 civs
/// in the worst case (random-walk variance-bound is smaller).
pub const SUCCESSOR_DRIFT_TRAIT_STEP: (i64, i64) = (2, 100);

/// Per-generation drift step on lifespan, in years. Sampled
/// in `[-SUCCESSOR_DRIFT_LIFESPAN_STEP_YEARS, +...]` per civ founding.
/// Pinned at 1 year because the species lifespan range is 5-200
/// years, so a 200-year species drifts by ≤ 0.5% per
/// generation, a 20-year species by ≤ 5%.
pub const SUCCESSOR_DRIFT_LIFESPAN_STEP_YEARS: i64 = 1;

#[cfg(test)]
mod tests;
