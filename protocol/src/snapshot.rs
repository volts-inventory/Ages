//! Periodic state digest emitted into the same NDJSON stream every
//! `SNAPSHOT_INTERVAL_TICKS`. Cheap state-anchor for offline tools.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Periodic state-digest snapshot. Emitted every
/// `SNAPSHOT_INTERVAL_TICKS` from sim-core; carries the run's
/// aggregate state at the snapshot tick. Listed in the project's
/// vision as one of four output channels (alongside NDJSON events,
/// the live CLI stream, and the markdown post-run report). Snapshot
/// content is digest-level rather than full-physics-state — the
/// report consumes them as a cross-check on event-derived totals,
/// and downstream consumers (replay, debugging) can use them as
/// fast-forward checkpoints without re-walking every per-tick event.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Snapshot {
    pub tick: u64,
    /// Civ ids that were active at this tick.
    pub active_civ_ids: Vec<u32>,
    /// Civ ids that have collapsed (cohort `civ_membership = None`).
    pub collapsed_civ_ids: Vec<u32>,
    /// Total population across every cohort the species owns
    /// (active + stateless), encoded as `Q96.32` raw bits (`Pop`
    /// type, i128-backed). Wire-encoded as a JSON string (see
    /// `pop_bits_serde`).
    #[serde(with = "crate::pop_bits_serde")]
    #[schemars(with = "String")]
    pub total_population_q32: i128,
    /// Total confirmed relations across every figure across every
    /// civ that ever existed in this run.
    pub total_confirmed_relations: u32,
    /// Total refinement events landed (proposed+confirmed+rejected).
    pub total_refinements: u32,
    /// Total catastrophes fired so far.
    pub total_catastrophes: u32,
    /// Total tech unlocks landed so far.
    pub total_tech_unlocks: u32,
    /// Total inter-civ knowledge transmission events.
    pub total_knowledge_transmissions: u32,
    /// Total inter-civ knowledge diffusion events.
    pub total_knowledge_diffusions: u32,
}
