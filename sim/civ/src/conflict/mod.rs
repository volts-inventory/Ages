//! Conflict resolution between two civs whose `claimed_cells`
//! overlap. Periodic per-pair check (every
//! `CONFLICT_CHECK_TICKS = 75` ticks); strength weighted by
//! population × literacy × Hierarchical-cosmology bonus; loser
//! takes a population hit and surrenders cells if defeated below
//! `CONFLICT_DEFEAT_FLOOR`.
//!
//! Split into:
//! - [`war`] — `resolve`, `ConflictOutcome`, strength + casualty
//!   math.
//! - [`alliance`] — `propose_alliance`, drift-based dissolution,
//!   trust decay.
//! - [`grudge`] — per-pair grudge accumulator with lazy decay.
//! - [`assessment`] — Q-war belligerence (`assess_pair`,
//!   `PairAssessment`, `WarDecision`, `decide_war`).

pub mod alliance;
pub mod assessment;
pub mod grudge;
pub mod war;

// Re-exports: keep the pre-split surface intact for sim/core and
// state.rs callers. Anything imported via `conflict::X` before the
// split still resolves through these names.

pub use alliance::{
    alliance_drifted_apart, cosmology_distance, propose_alliance, religion_distance,
    step_alliance_trust, ALLIANCE_DISSOLVE_COSMO_GAP, ALLIANCE_FORM_COOLDOWN_TICKS,
    ALLIANCE_FORM_COSMO_GAP, ALLIANCE_FORM_RELIGION_GAP, ALLIANCE_TRUST_DECAY_WEIGHT,
    ALLIANCE_TRUST_FLOOR, ALLIANCE_TRUST_INITIAL,
};
pub use assessment::{assess_pair, decide_war, PairAssessment, WarDecision};
pub use grudge::GRUDGE_CEILING;
pub use war::{
    hierarchical_strength, hierarchy_size_factor, is_peaceful_pair, overlap, resolve, strength,
    CELL_FLIP_FLOOR, CONFLICT_CHECK_TICKS, CONFLICT_DEFEAT_FLOOR, CONFLICT_HIERARCHY_BONUS,
    CONFLICT_MIN_LOSS, ConflictOutcome, PEACEFUL_HIERARCHY_FLOOR,
};
