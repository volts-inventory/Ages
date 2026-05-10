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
