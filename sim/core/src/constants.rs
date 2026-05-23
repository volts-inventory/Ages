//! Per-run constants surfaced from the old `lib.rs` monolith.
//!
//! `PHASE_ORDER` is the canonical per-tick phase walk.
//! `STAGNATION_THRESHOLD_TICKS`, `TRANSCENDENCE_SUSTAINED_TICKS`,
//! and `SNAPSHOT_INTERVAL_TICKS` are sampled by the tick loop in
//! `run_tick` to drive run-end checks and snapshot cadence.

use protocol::Phase;

/// Walk the per-tick phases in their fixed order. Returning a slice
/// keeps the order canonical and unchangeable from outside.
pub const PHASE_ORDER: &[Phase] = &[
    Phase::TickStart,
    Phase::PhysicsIntegration,
    Phase::PatternRecognition,
    Phase::CohortObservations,
    Phase::FigureObservations,
    Phase::PatternDetection,
    Phase::HypothesisTesting,
    Phase::Discovery,
    Phase::AdoptionAndDecay,
    Phase::CapabilityEvaluation,
    Phase::PopulationDynamics,
    Phase::CivLifecycle,
    Phase::CulturalDrift,
    Phase::TickEnd,
];

/// Stagnation threshold: how many consecutive ticks without an
/// active civ qualifies as a species-level stagnation run-end.
/// 2× the breakaway-cooldown (500) to avoid ending mid-bounce
/// when a v2/v3 founding is imminent.
pub const STAGNATION_THRESHOLD_TICKS: u64 = 1000 * protocol::MONTHS_PER_YEAR;

/// Transcendence sustainment threshold: how many ticks the
/// species must hold at-least-one-civ-with-all-tier-5 status
/// before transcendence fires. The transcendence arc is the
/// species' tech-tree summit and should take thousands of years
/// to reach AND a sustained mature operation at that capability
/// level; the threshold encodes "this is a tradition, not a
/// one-time peak." Combined with `species_maturity_floor` (3000
/// confirmed relations needed before any tier-5 tool unlocks)
/// this typically pushes transcendence to year 10000+.
pub const TRANSCENDENCE_SUSTAINED_TICKS: u64 = 2000 * protocol::MONTHS_PER_YEAR;

/// Periodic Snapshot emission cadence (vision's fourth output
/// channel). Every 500 ticks the run digest emits as a `Snapshot`
/// event — active civ ids, total population, running totals for
/// confirmed relations / refinements / catastrophes / tech /
/// transmissions / diffusions. Cheap enough to keep without
/// bloating the event log (1 event per 500 ticks vs. the thousands
/// of recognition / tick / discovery events between).
pub const SNAPSHOT_INTERVAL_TICKS: u64 = 500 * protocol::MONTHS_PER_YEAR;
