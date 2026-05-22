//! catastrophes — the 5-kind death-amplifier set. Each kind
//! has its own per-tick trigger predicate, its own cooldown, and
//! its own population-loss fraction; `check_and_apply` orchestrates
//! the per-civ check + apply step.
//!
//! Module layout:
//!
//! * [`kind`]      — `CatastropheKind` enum + telemetry tag.
//! * [`record`]    — `CatastropheRecord` (the per-event payload).
//! * [`triggers`]  — per-kind firing predicates.
//! * [`factors`]   — planet-driven severity/cooldown scaling.
//! * [`cells`]     — cell-targeting helpers (hex neighbours,
//!                   densest-claimed pick, deterministic per-tick
//!                   impact-site pick).
//! * [`damage`]    — per-cell damage plumbing: tolerance-gate
//!                   conditions builder + resistance/dormancy
//!                   applicator.
//! * [`apply`]     — `check_and_apply`, the per-tick orchestrator.
//!
//! Cooldown + population-loss constants for the five kinds live
//! here so the per-catastrophe applicator and tests share a single
//! source of truth.

mod apply;
mod cells;
mod damage;
mod factors;
mod kind;
mod record;
mod triggers;

pub use apply::check_and_apply;
pub use cells::{
    apply_to_cell_and_neighbors, densest_claimed_cell, deterministic_cell_pick, hex_neighbors,
};
pub use factors::{disease_severity_factor, ice_age_severity_factor, volcanic_cooldown_factor};
pub use kind::CatastropheKind;
pub use record::CatastropheRecord;

/// Per-kind cooldown (ticks). Placeholders under. : scaled
/// ×12 so the year-equivalent recurrence matches the old yearly
/// cadence under 1 tick = 1 month.
pub const VOLCANIC_COOLDOWN_TICKS: u64 = 200 * protocol::MONTHS_PER_YEAR;
pub const DISEASE_COOLDOWN_TICKS: u64 = 500 * protocol::MONTHS_PER_YEAR;
pub const ASTEROID_COOLDOWN_TICKS: u64 = 5_000 * protocol::MONTHS_PER_YEAR;
pub const SOLAR_FLARE_COOLDOWN_TICKS: u64 = 800 * protocol::MONTHS_PER_YEAR;
pub const ICE_AGE_COOLDOWN_TICKS: u64 = 4_000 * protocol::MONTHS_PER_YEAR;
pub const DISEASE_AGE_FLOOR_TICKS: u64 = 300 * protocol::MONTHS_PER_YEAR;

/// Population-fraction lost on each kind ( placeholders).
pub const VOLCANIC_POP_LOSS: (i64, i64) = (5, 100);
pub const DISEASE_POP_LOSS: (i64, i64) = (30, 100);
pub const ASTEROID_POP_LOSS: (i64, i64) = (40, 100);
pub const SOLAR_FLARE_POP_LOSS: (i64, i64) = (10, 100);
pub const ICE_AGE_POP_LOSS: (i64, i64) = (20, 100);
