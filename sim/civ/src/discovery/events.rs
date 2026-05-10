//! `HypothesisEvent` — the per-tick output of the hypothesizer's
//! `step`. sim/core maps each variant to a protocol event.

use super::channels::Channel;
use super::types::{ConfirmedMeasurement, ConfirmedRelation};
use crate::forms::Form;
use sim_arith::Real;

/// Event surfaced by the hypothesizer's per-tick `step`. The caller
/// (sim/core) maps each variant to its protocol event.
#[derive(Debug, Clone)]
pub enum HypothesisEvent {
    Confirmed(ConfirmedRelation),
    RefinementProposed {
        relation_id: u32,
        template_id: u32,
        old_form: Form,
        new_form: Form,
        old_confidence: Real,
        n_samples: usize,
    },
    RefinementConfirmed {
        relation_id: u32,
        template_id: u32,
        /// Channel of the relation; emitters use this to rescale
        /// `new_params` out of fit-space into real-unit space.
        channel: Channel,
        old_form: Form,
        new_form: Form,
        new_params: Vec<Real>,
        new_residual: Real,
        new_confidence: Real,
        n_samples: usize,
    },
    RefinementRejected {
        relation_id: u32,
        template_id: u32,
        old_form: Form,
        attempted_form: Form,
        reason: String,
    },
    /// a measurement relation cleared `exp(-1)` and the civ
    /// has measured a continuous law. Distinct from `Confirmed`
    /// (which is firing-relation only) so the protocol layer can
    /// route to the dedicated `MeasurementConfirmed` event with
    /// y/x channel tags rather than a single `(template, channel)`.
    MeasurementConfirmed(ConfirmedMeasurement),
    /// a confirmed relation's RMSE on fresh samples drifted
    /// above 1.5× its confirm-time residual for
    /// `FALSIFICATION_TRIGGER_TICKS` consecutive ticks. The civ has
    /// observed the law mispredicting and force-triggers refinement
    /// faster than the slower confidence-streak path. Pure narrative
    /// signal — refinement still flows through `RefinementProposed`
    /// after this fires.
    Falsified {
        relation_id: u32,
        template_id: u32,
        old_form: Form,
        streak_ticks: u64,
    },
    /// an inherited relation re-fit successfully on the
    /// successor's own observations after the revalidation window.
    /// The relation graduates to native confirmed status — the
    /// `inherited_from_*` tags are cleared.
    Revalidated {
        relation_id: u32,
        template_id: u32,
        from_civ_id: u32,
        new_residual: Real,
        new_confidence: Real,
    },
    /// an inherited relation failed to re-fit on the successor's
    /// observations. The relation is dropped from `confirmed`. The
    /// successor has rejected the parent's theory on its own data.
    Lapsed {
        relation_id: u32,
        template_id: u32,
        from_civ_id: u32,
        attempted_form: Form,
    },
}
