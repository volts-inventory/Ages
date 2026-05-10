//! Run-level events + per-run metadata + the `Phase` enum + period
//! helpers + the schema-version / baseline-months consts that
//! consumers index off the run header.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Per-tick phase ordering. The sim walks these in fixed order
/// each civ-sim tick. Sub-phase ordinals can be added later without
/// renumbering top-level phases.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    TickStart,
    PhysicsIntegration,
    PatternRecognition,
    CohortObservations,
    FigureObservations,
    PatternDetection,
    HypothesisTesting,
    Discovery,
    AdoptionAndDecay,
    CapabilityEvaluation,
    PopulationDynamics,
    CivLifecycle,
    CulturalDrift,
    TickEnd,
}

/// Run header — emitted once at the start of every run.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RunHeader {
    pub schema_version: u32,
    pub seed: u64,
    pub ages_version: String,
}

/// Presentation metadata — emitted once per run, immediately
/// after `RunStart`. Carries every label table + numeric threshold
/// the viewport / narrator / future LLM consumer needs to render
/// human-readable output, so the same string ("ocean world",
/// "scorching", "centralized medium cognition") appears across
/// live and post-run views without each consumer re-implementing
/// the mappings.
///
/// Source of truth: `sim_report::labels` (label functions) +
/// `sim_physics::chemistry::substrate_properties` (substrate
/// freeze/boil ranges). `sim_core` builds this struct from those
/// upstream sources and emits it; downstream consumers read it
/// from the NDJSON event log.
///
/// All maps are keyed by the *internal* enum name ("aqueous",
/// "Tactile", `cnt`/`dst`-style topology key, …) so consumers
/// can do `metadata.planet_type_labels[planet.metabolic_substrate]`
/// without a second lookup table.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct RunMetadata {
    /// Substrate → freeze point (Kelvin). Values mirror
    /// `sim_physics::chemistry::substrate_properties().freeze_k`.
    pub substrate_freeze_k: BTreeMap<String, f64>,
    /// Substrate → boil point (Kelvin). Values mirror
    /// `sim_physics::chemistry::substrate_properties().boil_k`.
    pub substrate_boil_k: BTreeMap<String, f64>,
    /// Substrate → planet-type display label.
    /// e.g. `aqueous → "ocean world"`.
    pub planet_type_labels: BTreeMap<String, String>,
    /// Substrate → biochemistry display label.
    /// e.g. `silicate → "silicon"`, everything else → `"carbon"`.
    pub planet_biochem_labels: BTreeMap<String, String>,
    /// Atmosphere → display label. e.g.
    /// `oxidising → "oxygen-rich"`.
    pub atmosphere_labels: BTreeMap<String, String>,
    /// Host-species badge (`frozen-out`, `near-freezing`,
    /// `thriving`, `near-boiling`, `boiling-off`, `vacuum`)
    /// → friendly word. e.g. `boiling-off → "scorching"`.
    pub friendly_badge_labels: BTreeMap<String, String>,
    /// `ModalityKind` debug name → short display label.
    pub modality_short_labels: BTreeMap<String, String>,
    /// `ManipulationKind` debug name → short display label.
    pub manipulation_short_labels: BTreeMap<String, String>,
    /// Tier-bucket boundaries for 0..1 trait scalars (cognition,
    /// sociality, communication-fidelity). Default `[0.34, 0.67]`
    /// Three buckets: `[low, mid, high)`.
    pub tier_thresholds: Vec<f64>,
    /// Cognition tier labels in low→high order. Default
    /// `["low", "medium", "high"]`.
    pub cog_tier_labels: Vec<String>,
    /// Sociality tier labels in low→high order. Default
    /// `["solitary", "social", "eusocial"]`.
    pub sociality_tier_labels: Vec<String>,
    /// Communication-fidelity tier labels in low→high order.
    /// Default `["noisy", "clear", "precise"]`.
    pub comm_tier_labels: Vec<String>,
}

/// Tick boundary marker. Carries the tick index for downstream
/// consumers that want to group events.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TickEvent {
    pub tick: u64,
    pub phase: Phase,
}

/// Minimum per-cell nomad population required for a cell to render
/// as `0` in the viewport. Set well above 1 so a cell with a
/// handful of stray migrants doesn't visually claim the same
/// "occupied" weight as a saturated village. With per-cell cap
/// of `NOMAD_PER_CELL_CAP = 80`, a floor of 10 means roughly
/// "≥ 1/8 of cap" before the glyph appears — the cell has to
/// host a real settlement, not a passing-through cohort.
pub const NOMAD_DISPLAY_FLOOR_POP: f64 = 10.0;

pub const SCHEMA_VERSION: u32 = 0;

/// Rate-calibration baseline. **NOT** the universal
/// year length anymore — calendar time is per-planet via
/// `Planet::orbital_period_months`. This constant survives only as
/// the denominator for tick-rate calibrations: per-month birth/death
/// rates and `*_COOLDOWN_TICKS` constants are pinned to a 12-tick
/// reference year so a planet with a 6- or 18-month orbital period
/// runs the same physics calibration. For *display* (year-of-tick,
/// month-of-tick, seasonal cycles) use the planet's actual orbital
/// period — see `year_of_tick_for_period` /
/// `month_of_tick_for_period`.
pub const BASELINE_MONTHS_PER_YEAR: u64 = 12;

/// Backwards-compat alias for the rate-calibration baseline. Prefer
/// `BASELINE_MONTHS_PER_YEAR` in new code; this name is retained for
/// the older tick-rate constants that already reference it.
pub const MONTHS_PER_YEAR: u64 = BASELINE_MONTHS_PER_YEAR;

/// Derive the planet-relative year a tick falls in. `period`
/// is the planet's `orbital_period_months` (sampled per planet, range
/// 8..=16). A period of 0 falls back to the baseline so degraded
/// inputs don't divide by zero.
#[must_use]
pub fn year_of_tick_for_period(tick: u64, period: u32) -> u64 {
    let p = u64::from(period.max(1));
    tick / p
}

/// Derive the planet-relative month-within-year for a tick.
/// `0` = first month of year; max value is `period - 1`.
#[must_use]
pub fn month_of_tick_for_period(tick: u64, period: u32) -> u64 {
    let p = u64::from(period.max(1));
    tick % p
}

/// Baseline (12-month) year-of-tick. Retained for legacy callsites
/// that don't have a planet handle; new code should pass through
/// `year_of_tick_for_period` with the actual planet period.
#[must_use]
pub fn year_of_tick(tick: u64) -> u64 {
    tick / BASELINE_MONTHS_PER_YEAR
}

/// Baseline (12-month) month-of-tick. Retained for legacy callsites
/// that don't have a planet handle.
#[must_use]
pub fn month_of_tick(tick: u64) -> u64 {
    tick % BASELINE_MONTHS_PER_YEAR
}
