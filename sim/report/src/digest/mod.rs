//! Walk an event stream and aggregate every section the renderer
//! needs into one `Digest`. Two passes over the slice:
//!
//!   1. Build `relation_names: relation_id → (template_name, channel)`
//!      from `RelationConfirmed` events. Refinement, knowledge-
//!      transmission, and knowledge-diffusion events reference
//!      relations by `relation_id` only — having the names indexed
//!      lets the renderer label them without joining at write time.
//!   2. Fold every event into the running per-civ chapters plus the
//!      cross-civ event lists.
//!
//! Chapters are indexed by civ id and emitted in founding order.

mod build;
mod types;

pub use types::*;

#[cfg(test)]
mod tests;
