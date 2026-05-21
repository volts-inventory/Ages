//! `sim-species` — species traits, derivation, and sensorium gating.
//!
//! The species is the persistent unit of a run. Species
//! is *derived* from {planet, regions, recognized phenomena},
//! not sampled independently — its sensorium and manipulation modes
//! reflect the niche the planet provides. Modalities and
//! manipulation modes are bounded enums (15 + 12) with environment-
//! gated availability and per-channel parameters; aggregation across
//! multiple modalities is *vector* (downstream consumers query per
//! channel rather than collapsing to a scalar).
//!
//! Sensorium gating is the M2-deferred follow-up: a species perceives
//! a recognition template iff at least one of the template's natural
//! channels is in the species' modality set. Latent templates (sensed
//! by no native modality) are observable only after sensorium-
//! extending tech.

#![allow(clippy::module_name_repetitions)]

mod derive;
mod habitat_glyph;
mod sampling;
mod species;
mod types;

pub use derive::derive;
pub use habitat_glyph::habitat_glyph_multiplier;
pub use sampling::{
    apply_catastrophe, modality_supported, species_name_from_seed, template_channels,
};
pub use species::Species;
pub use types::{
    apply_catastrophe_with_dormancy, CasteRole, CognitionAxes, CognitionTopology, DormantPool,
    DynamicTool, DynamicToolEffects, EcosystemRole, Fission, FunctionalResponse, Habitat,
    Interaction, InteractionKind, InteractionMatrix, Lifecycle, Manipulation, ManipulationKind,
    Modality, ModalityKind, MutualismKind, ParasiteKind, PopulationBiology, ProducerMetabolism,
    SpeciesId, ToleranceEnvelope, DYNAMIC_TOOL_ID_START,
};

#[cfg(test)]
mod tests;
