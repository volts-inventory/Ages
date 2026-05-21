//! World-side events: the per-tick recognition firing payload, the
//! one-shot planet/species/nomad records emitted at run start (and
//! when state changes), and the per-civ species-drift snapshot.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A recognised-phenomenon firing — the `PatternRecognition` phase
/// emits one of these per cell × template match per civ-sim tick.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RecognitionFiring {
    pub tick: u64,
    pub template_id: u32,
    pub template_name: String,
    pub cell: u32,
}

/// Planet-sampled event — emitted once at run start, immediately
/// after `RunStart`. Carries the bulk planet properties drawn from
/// the seed. The post-run report uses these to render
/// the planet card; downstream consumers can also reproduce the
/// run's law coefficients from these values.
///
/// Real-valued scalars are emitted as `Q32.32` raw bits (`raw_q32`)
/// for bit-exact event-log determinism; divide by `2^32` to recover
/// the underlying SI value. Enums (composition, atmosphere,
/// biosphere, magnetosphere) are emitted as snake-case strings.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PlanetDerived {
    pub seed: u64,
    /// Deterministic planet name from the seed (e.g.
    /// `Vela-c`, `Aleph-h`). Pure flavour; no physics depends
    /// on it. Used by viewport / report layers for human-readable
    /// identification.
    pub name: String,
    pub gravity_q32: i64,
    pub composition: String,
    pub mean_temperature_q32: i64,
    pub temperature_gradient_q32: i64,
    pub terrain_peak_q32: i64,
    pub sea_level_q32: i64,
    pub atmosphere: String,
    pub surface_pressure_q32: i64,
    pub biosphere: String,
    pub magnetosphere: String,
    /// Crust mineral profile snake-case tag — one of
    /// `basaltic` / `hydrocarbon` / `piezoelectric` / `ferrous`
    /// / `rare_earth`. Drives fuel availability and which
    /// sensorium-extending tech tracks the species can reach.
    pub crust: String,
    pub stellar_luminosity_q32: i64,
    pub moon_count: u8,
    /// Axial tilt in degrees (Earth ≈ 23.4°), Q32.32 raw bits.
    pub axial_tilt_deg_q32: i64,
    /// Sidereal day length in hours (Earth ≈ 24), Q32.32 raw bits.
    pub day_length_hours_q32: i64,
    /// Months per orbital period. 8-16 across habitable
    /// worlds. The sim cadence is still 1 tick = 1 species-month;
    /// year-bearing constants like
    /// `STAGNATION_THRESHOLD_TICKS` keep the 12-month standard.
    /// What this drives: seasonal templates' `MonthIn` modulo
    /// fires on the planet's actual orbital fraction.
    pub orbital_period_months: u32,
    /// Metabolic substrate snake-case tag — one of
    /// `aqueous` / `ammoniacal` / `hydrocarbon` / `silicate`.
    /// Determines which biochemistry life on this planet runs on;
    /// the sampler picks this first and constrains every other
    /// field to the substrate's tolerance window so every seed
    /// produces a habitable world.
    pub metabolic_substrate: String,
    /// Per-seed substrate-chemistry
    /// perturbation in `[-0.05, +0.05]`, Q32.32 raw bits. Shifts
    /// the substrate's nominal freeze + boil points by
    /// `nominal × perturbation`. Consumers can recover the
    /// effective freeze point as
    /// `RunMetadata::substrate_freeze_k[substrate] × (1 + perturbation)`.
    /// Defaults to 0 for legacy event logs that pre-date the field.
    #[serde(default)]
    pub substrate_perturbation_q32: i64,
    /// Continuous atmospheric composition (mass fractions).
    /// Nine channels — N₂, O₂, CO₂, CH₄, NH₃, H₂O, H₂, Ar, other.
    /// Each Q32.32 raw bits; consumers convert via
    /// `i64 as f64 / 2^32`. Sum approaches 1.0 (within fixed-point rounding)
    /// for any non-vacuum atmosphere; sums to 0 for `Atmosphere::None`.
    /// Older event logs default each channel to 0 — compatible
    /// with vacuum, so consumers don't crash but the categorical
    /// `atmosphere` label is the only signal available.
    #[serde(default)]
    pub atmospheric_n2_q32: i64,
    #[serde(default)]
    pub atmospheric_o2_q32: i64,
    #[serde(default)]
    pub atmospheric_co2_q32: i64,
    #[serde(default)]
    pub atmospheric_ch4_q32: i64,
    #[serde(default)]
    pub atmospheric_nh3_q32: i64,
    #[serde(default)]
    pub atmospheric_h2o_q32: i64,
    #[serde(default)]
    pub atmospheric_h2_q32: i64,
    #[serde(default)]
    pub atmospheric_ar_q32: i64,
    #[serde(default)]
    pub atmospheric_other_q32: i64,
    /// Continuous biosphere richness in `[0, 1]`. Q32.32 raw
    /// bits. Sampled from `BiosphereClass` baseline + ±0.10 jitter.
    /// 0 = lifeless, 1 = hyper-biodiverse. Older logs default to 0.
    #[serde(default)]
    pub biosphere_density_q32: i64,
    /// Continuous crustal composition (mass fractions). Seven
    /// channels — silicate, hydrocarbon, piezoelectric, ferrous,
    /// `rare_earth`, ice, other. Each Q32.32 raw bits. Sum approaches
    /// 1.0 for any sampled crust; 0 for `empty()`. Older logs
    /// default each channel to 0.
    #[serde(default)]
    pub crustal_silicate_q32: i64,
    #[serde(default)]
    pub crustal_hydrocarbon_q32: i64,
    #[serde(default)]
    pub crustal_piezoelectric_q32: i64,
    #[serde(default)]
    pub crustal_ferrous_q32: i64,
    #[serde(default)]
    pub crustal_rare_earth_q32: i64,
    #[serde(default)]
    pub crustal_ice_q32: i64,
    #[serde(default)]
    pub crustal_other_q32: i64,
}

/// Per-cell elevation + water-depth map of the sampled planet.
/// Emitted once at run start after `init_planet` populates the
/// physics grid. Lets the post-run report draw an ASCII map of the
/// world (vision: "a spatial grid"); also useful for offline tools
/// that want to overlay civ activity on the planet.
///
/// Real-valued scalars are emitted as `Q32.32` raw bits for bit-
/// exact event-log determinism. Cell ordering matches
/// `HexGrid::cells()` row-major order: index = `r * grid_width + q`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PlanetMap {
    pub grid_width: u32,
    pub grid_height: u32,
    pub elevation_q32: Vec<i64>,
    pub water_depth_q32: Vec<i64>,
}

/// Species-derivation event — emitted once at run start after the
/// physics warm-up and the recognition library are in place. Carries
/// the derived traits for the run's persistent species.
///
/// Real-valued trait scalars are emitted as `Q32.32` raw bits
/// (`raw_q32`) so the event log stays bit-exact deterministic across
/// platforms; divide by `2^32` to recover the underlying value.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SpeciesDerived {
    pub seed: u64,
    /// Deterministic species name from the seed (e.g.
    /// `Kelvars`, `Tolakites`). Pure flavour; no behaviour
    /// depends on it. Used by viewport / report layers.
    pub name: String,
    pub cognition_q32: i64,
    pub sociality_q32: i64,
    pub communication_fidelity_q32: i64,
    pub lifespan_years_q32: i64,
    pub t0_loss_q32: i64,
    /// Modality kinds in the species' sensorium.
    pub modalities: Vec<String>,
    /// Manipulation modes available to the species.
    pub manipulation_modes: Vec<String>,
    /// Recognition template ids the species can perceive natively.
    /// Latent templates (no native channel) are not in this list and
    /// stay unobservable until sensorium-extending tech lands.
    pub perceivable_template_ids: Vec<u32>,
    /// Cognition topology — `centralized` (vertebrate-equivalent,
    /// single brain) or `distributed` (cephalopod-equivalent, many
    /// processing centres). Drives reporting flavour and reserves
    /// space for behavioural forks in later passes.
    pub cognition_topology: String,
}

/// Species trait-drift snapshot at civ founding.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SpeciesDrift {
    pub tick: u64,
    pub civ_id: u32,
    pub parent_civ_id: Option<u32>,
    pub cognition_delta_q32: i64,
    pub sociality_delta_q32: i64,
    pub lifespan_delta_years_q32: i64,
    pub communication_fidelity_delta_q32: i64,
}

/// Per-seed cosmology bias event payload. Q32.32 raw bits
/// for each of the five axes; consumers display via
/// `i64 as f64 / 2^32` (the standard Q32.32 → display-f64 path).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SpeciesCosmologyBias {
    pub tick: u64,
    pub empirical_q32: i64,
    pub communitarian_q32: i64,
    pub reformist_q32: i64,
    pub mystical_q32: i64,
    pub hierarchical_q32: i64,
}

/// Cause of a `SpeciesExtinct` event. Sprint 2 Item 6a emits
/// `PopulationCollapse` only — biomass dropped below the threshold
/// for the confirmation window. `KeystoneCascade` and `Catastrophe`
/// wire up in later items (keystone removal cascades; catastrophe
/// kill triggers); declared here so the wire schema is stable and
/// downstream consumers can switch on the cause without a schema
/// migration when the later causes start emitting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExtinctionCause {
    /// Biomass / population pool fell below
    /// `EXTINCTION_THRESHOLD` for
    /// `EXTINCTION_CONFIRMATION_TICKS` consecutive ticks.
    PopulationCollapse,
    /// Extinction propagated from the loss of a keystone the
    /// species depended on (food web disconnection). Reserved for
    /// future wiring; not emitted by Sprint 2 Item 6a.
    KeystoneCascade,
    /// Single-tick wipe from a catastrophe whose lethality cleared
    /// the species' tolerance envelope. Reserved for future
    /// wiring; not emitted by Sprint 2 Item 6a.
    Catastrophe,
}

/// A species was flagged extinct by the ecosystem step. The species
/// record stays in the per-planet registry for history / replay
/// determinism but is skipped by subsequent ecosystem ticks. Sprint
/// 2 Item 6a always emits `cause = PopulationCollapse`; future
/// items wire in the other causes.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SpeciesExtinct {
    pub tick: u64,
    /// Dense per-planet species id (`SpeciesId.0`). Matches the
    /// ids used in the ecosystem step / interaction matrix.
    pub species_id: u32,
    pub cause: ExtinctionCause,
}

/// Per-trait identifier for the trait actually swapped by a
/// `HorizontalGeneTransfer` event. Sprint 3 Item 11a swaps one of
/// these four scalar trait axes between two co-located Microbial
/// species per HGT trial. Extensible — additional axes can be
/// appended without breaking the wire schema, since the variants
/// are serialised as snake-case strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TraitName {
    /// `Species::dormancy_capability` — tardigrade-grade dormancy
    /// scalar in `[0, 1]`.
    DormancyCapability,
    /// `Species::tolerance.temp_range.0` — low end of the
    /// temperature tolerance envelope.
    TemperatureToleranceLow,
    /// `Species::tolerance.temp_range.1` — high end of the
    /// temperature tolerance envelope.
    TemperatureToleranceHigh,
    /// `Species::tolerance.radiation_max` — radiation-tolerance
    /// ceiling.
    RadiationMax,
}

/// Sprint 3 Item 11a — a horizontal-gene-transfer trial succeeded
/// between two co-located `Lifecycle::Microbial` species. The
/// recipient's `trait_swapped` axis was nudged toward the donor's
/// value by a small fraction (the step does
/// `recipient = recipient × 0.95 + donor × 0.05`). The species
/// records keep the swap; this event carries the audit trail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HgtEvent {
    pub tick: u64,
    /// Donor species' dense per-planet id (`SpeciesId.0`).
    pub donor_id: u32,
    /// Recipient species' dense per-planet id (`SpeciesId.0`).
    pub recipient_id: u32,
    pub trait_swapped: TraitName,
}

/// Trigger kind for a `SpeciationOccurred` event. Sprint 3 Item 11
/// emits one of five trigger kinds, mapped from the
/// `sim_ecosystem::speciation::SpeciationTrigger` enum. The wire
/// schema enumerates all five up front so downstream consumers can
/// switch exhaustively without a schema migration as new triggers
/// (HGT-driven, sexual-selection-driven, …) are wired in later items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SpeciationTriggerKind {
    /// Population split into geographically disconnected groups for
    /// more than the configured isolation window. The daughter
    /// species inherits the isolated subpopulation.
    Allopatric {
        /// Number of consecutive ticks the two subpopulations were
        /// disconnected before speciation fired.
        isolation_ticks: u64,
    },
    /// Sympatric — two species competed intensely on overlapping
    /// resources for longer than the configured sympatric pressure
    /// window, and the parent drifted into a distinct niche.
    Sympatric,
    /// Polyploidy — instant chromosome-duplication event. Only
    /// emitted for `Lifecycle::Plant` parents.
    Polyploid,
    /// Founder effect — a small bottleneck population (< 1% of the
    /// parent's normal pool) seeded new territory and drifted toward
    /// fixation differently from the parent stock.
    FounderEffect,
    /// Post-extinction adaptive radiation — speciation rate is
    /// boosted 5× for 100 generations after a mass-extinction event.
    /// `generation` identifies which post-extinction cohort the
    /// daughter species belongs to (0 = first, growing monotonically
    /// through the boosted window).
    PostExtinctionRadiation {
        generation: u64,
    },
}

/// A daughter species was generated from a parent by the speciation
/// step. Sprint 3 Item 11 emits one event per speciation. The
/// daughter id is the newly allocated `SpeciesId` (one past the
/// current registry max); the parent id is the species the daughter
/// drifted from. Trait drift is correlated via the allometry helper
/// — see `sim_ecosystem::speciation::divergence_pull`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SpeciationEvent {
    pub tick: u64,
    /// Dense per-planet parent species id (`SpeciesId.0`).
    pub parent_id: u32,
    /// Dense per-planet daughter species id (`SpeciesId.0`).
    /// Allocated as `max(existing_id) + 1` at speciation time so the
    /// id space stays monotonic across the run.
    pub daughter_id: u32,
    /// Trigger kind. See `SpeciationTriggerKind`.
    pub trigger: SpeciationTriggerKind,
}

/// Snapshot of the species' nomadic population per cell.
/// Emitted on tick boundaries when the nomad pool's per-cell
/// distribution changes meaningfully (births, civ absorption,
/// migration). Cells with population above
/// `NOMAD_DISPLAY_FLOOR_POP` get rendered as `0` in the viewport
/// — nomadic populations the species occupies but no civ has
/// coalesced from yet.
#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, schemars::JsonSchema,
)]
pub struct SpeciesNomadsChanged {
    pub tick: u64,
    /// Cells (sorted ascending) with non-trivial nomadic
    /// population. Civ-claimed cells are *excluded* — civs
    /// absorb nomads on claim, so per-cell population there is
    /// represented in `CivTerritoryChanged` instead.
    pub cells: Vec<u32>,
    /// Q32.32 raw bits per nomadic cell, in the same order as
    /// `cells`. Lets the renderer pick a `0` / `▒0` density
    /// shading and the post-run report compute total nomad
    /// pop as a sanity counter on species.cohort.
    pub population_q32: Vec<i64>,
}
