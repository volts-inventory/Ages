//! Quantitative discovery pipeline.
//!
//! `Hypothesizer` accumulates per-cell observation samples for a
//! fixed set of `(template, channel)` candidate relations, periodically
//! attempts fits across the form vocabulary, and confirms a
//! relation when a fit clears the confidence threshold
//! `exp(-1)`. Confirmed relations are first-class state on the civ;
//! refinement lands as a follow-up.
//!
//! M3 attribution: the hypothesizer is civ-scoped (cohort-level) and
//! emits `figure_id = 0` on its events. Named figures will plug into
//! the same lifecycle in a follow-up commit.

mod channels;
mod events;
mod hypothesizer;
mod types;

pub mod emergence;
pub mod tool_emergence;

pub use channels::{
    channels_for_modality, measurement_relation_id, perceivable_channels,
    perceivable_channels_from_kinds, relation_id_for, Channel, MeasurementChannel,
};
pub use events::HypothesisEvent;
pub use hypothesizer::Hypothesizer;
pub use types::{
    CandidateRelation, ConfirmedMeasurement, ConfirmedRelation, MeasurementCandidate,
    RefinementState, ResidualBasis,
};

#[cfg(test)]
mod tests;
