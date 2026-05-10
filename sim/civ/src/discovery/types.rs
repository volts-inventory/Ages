//! Data types ferried between the discovery pipeline's pieces:
//! `CandidateRelation`, `MeasurementCandidate`, `ResidualBasis`,
//! `ConfirmedMeasurement`, `ConfirmedRelation`, `RefinementState`.

use crate::forms::Form;
use sim_arith::Real;

use super::channels::{measurement_relation_id, Channel, MeasurementChannel};

/// One candidate relation: how does template `template_id`'s firing
/// behaviour depend on `channel`? The pipeline accumulates per-cell
/// `(channel reading, fired? 0/1)` samples and tries to fit a form.
#[derive(Debug, Clone)]
pub struct CandidateRelation {
    pub relation_id: u32,
    pub template_id: u32,
    pub channel: Channel,
}

/// candidate measurement relation. Continuous `y` and `x` from
/// physics state — fits recover continuous coefficients (e.g. heat-
/// diffusion equilibrium slope), not template firing thresholds.
///
/// an optional `residual_basis` lets a candidate fit the
/// *residual* of an earlier confirmed measurement. When base
/// relation R has been confirmed (e.g. `T = a × neighbour_mean(T) +
/// b`) and the civ wants to explain *what's left*, the residual
/// candidate's effective y becomes `direct_y - R.predict(x_R at
/// cell)`. The fit then determines what other channel correlates
/// with that residual — building hierarchical theories instead of
/// independent atomic relations.
#[derive(Debug, Clone)]
pub struct MeasurementCandidate {
    pub relation_id: u32,
    pub y_channel: MeasurementChannel,
    pub x_channel: MeasurementChannel,
    pub residual_basis: Option<ResidualBasis>,
    /// how deep in the residual hierarchy this candidate sits.
    /// 0 = base ( default catalogue); 1 = first-level residual
    /// child; 2 = grandchild; etc. Auto-generation stops
    /// at depth `MAX_RESIDUAL_DEPTH` so the candidate space stays
    /// bounded.
    pub residual_depth: u32,
}

/// snapshot of a confirmed measurement relation, used by a
/// child candidate to compute residuals at observe time. Carries
/// the form + params at compose time so the basis is *frozen* —
/// future refinement of the source relation doesn't retroactively
/// invalidate child fits. The trade-off is that the hierarchy
/// captures the civ's *understanding at the time it built the
/// composition*, which is a more faithful biography than a live
/// re-evaluation.
#[derive(Debug, Clone)]
pub struct ResidualBasis {
    pub source_relation_id: u32,
    pub source_form: Form,
    pub source_params: Vec<Real>,
    pub source_x_channel: MeasurementChannel,
}

impl MeasurementCandidate {
    pub fn new(y_channel: MeasurementChannel, x_channel: MeasurementChannel) -> Self {
        Self {
            relation_id: measurement_relation_id(y_channel, x_channel),
            y_channel,
            x_channel,
            residual_basis: None,
            residual_depth: 0,
        }
    }

    /// Residual candidate. Same y/x channels as a regular
    /// candidate but the y axis subtracts the basis's prediction
    /// before fitting. `relation_id` is offset into a separate
    /// `2_000_000`+ namespace to avoid colliding with both firing
    /// (max ~500) and direct measurement (`1_000_000`+) ids.
    /// `residual_depth` is the parent's depth + 1, so the
    /// auto-generation can cap recursion (grandchildren and
    /// beyond stay bounded).
    pub fn residual(
        y_channel: MeasurementChannel,
        x_channel: MeasurementChannel,
        basis: ResidualBasis,
        residual_depth: u32,
    ) -> Self {
        let base_id = measurement_relation_id(y_channel, x_channel);
        // Mix the source relation + depth into the id so the same
        // (y, x) pair against different bases / depths gets distinct
        // relation_ids.
        let mixed = base_id
            .wrapping_add(basis.source_relation_id.wrapping_mul(7919))
            .wrapping_add(residual_depth.wrapping_mul(31_337));
        Self {
            relation_id: 2_000_000 + (mixed % 1_000_000),
            y_channel,
            x_channel,
            residual_basis: Some(basis),
            residual_depth,
        }
    }
}

/// A confirmed measurement relation. Same quality readings as
/// `ConfirmedRelation`; `params` are stored in fit-space and
/// rescaled to real units on emit via the underlying channels'
/// `scale()`.
#[derive(Debug, Clone)]
pub struct ConfirmedMeasurement {
    pub relation_id: u32,
    pub y_channel: MeasurementChannel,
    pub x_channel: MeasurementChannel,
    pub form: Form,
    pub params: Vec<Real>,
    pub residual: Real,
    pub confidence: Real,
    pub n_samples: usize,
    pub confirmed_at_tick: u64,
    /// at least one sample contributing to this relation's
    /// fit pool came from a controlled-conditions apparatus the
    /// civ built (`Hypothesizer::record_experimental_measurement`).
    /// Distinguishes Galileo-style intervention (clamp x, measure
    /// y) from passive observation (sample whatever the planet
    /// happens to produce). Persists with the relation so successor
    /// civs and the post-run report can distinguish theories the
    /// parent confirmed experimentally from theories it merely
    /// observed.
    pub is_experimental: bool,
}

impl ConfirmedMeasurement {
    /// Rescale params from fit-space (normalised by `x_channel.scale`)
    /// into real-unit space. y is *also* in fit-space (divided by
    /// `y_channel.scale`); the SI-real slope is therefore
    /// `fit_slope × y_scale / x_scale`. This mirrors
    /// `Form::rescale_params` for firing relations but accounts for
    /// the y-axis normalisation that firing relations don't have
    /// (binary y has no scale).
    pub fn params_in_real_units(&self) -> Vec<Real> {
        let x_scale = self.x_channel.channel().scale();
        let y_scale = self.y_channel.channel().scale();
        // For Linear / Constant the SI-space conversion is just
        // y_scale / x_scale on slope-bearing terms; intercept is in
        // y-space so multiply by y_scale. Defer per-form correctness
        // to `Form::rescale_params` where it already exists, then
        // post-multiply by y_scale to lift y from fit-space.
        let xspace = self.form.rescale_params(&self.params, x_scale);
        xspace.into_iter().map(|p| p * y_scale).collect()
    }
}

/// A confirmed relation — the form, parameters, and quality
/// readings as of the last fit. Refinement tracks the sustained-
/// low-confidence streak and any active probation in place; on a
/// confirmed refinement, the form / params / residuals update.
///
/// `params` are stored in *fit-space* (over the channel's normalised
/// `x = x_real / channel.scale()`) so refinement and the in-pipeline
/// fit comparisons stay numerically consistent. Use
/// `params_in_real_units()` to convert to SI-consistent coefficients
/// for event emission and reporting.
#[derive(Debug, Clone)]
pub struct ConfirmedRelation {
    pub relation_id: u32,
    pub template_id: u32,
    pub channel: Channel,
    pub form: Form,
    pub params: Vec<Real>,
    pub residual: Real,
    pub confidence: Real,
    pub n_samples: usize,
    pub confirmed_at_tick: u64,
    /// How many consecutive `step()` ticks the relation has measured
    /// `confidence ≤ exp(-2)` against the current form. Trigger fires
    /// at `SUSTAINED_TRIGGER_TICKS`; resets when confidence recovers.
    pub low_confidence_streak: u64,
    /// Earliest tick at which a new refinement may be triggered.
    /// Set after a rejected probation; `0` while no cooldown active.
    pub cooldown_until: u64,
    /// Set when a refinement is in probation. Holds the candidate
    /// form proposed against this relation.
    pub refinement: Option<RefinementState>,
    /// confirm-time residual snapshot. Used to detect
    /// *prediction drift* — when the relation's residual on the
    /// latest sample window exceeds 1.5× this initial value for
    /// `FALSIFICATION_TRIGGER_TICKS` ticks, the law is mispredicting
    /// and the civ force-triggers refinement (faster than the
    /// confidence-based path). Stored separately from the live
    /// `residual` so refinement can compare *current* fit quality
    /// against the *confirm-time* baseline.
    pub initial_residual: Real,
    /// consecutive ticks the relation's RMSE on fresh samples
    /// has exceeded 1.5× `initial_residual`. Fires `RelationFalsified`
    /// and force-triggers refinement at
    /// `FALSIFICATION_TRIGGER_TICKS`; resets when RMSE recovers.
    pub falsification_streak: u64,
    /// tick at which this relation was transmitted from a
    /// parent civ (`None` if the civ confirmed it natively). The
    /// successor re-validates the inherited fit against its own
    /// observations during the
    /// `REVALIDATION_WINDOW_TICKS` window starting at this tick;
    /// passes graduate to native confirmed status, failures emit
    /// `RelationLapsed` and drop the relation.
    pub inherited_from_tick: Option<u64>,
    /// civ id the relation was inherited from. Carried through
    /// to the post-run report ("civ X falsified Y's law from civ Z").
    pub inherited_from_civ_id: Option<u32>,
}

impl ConfirmedRelation {
    /// Return the relation's parameters in real-unit (SI) space.
    /// Internal `params` are fit-space (normalised by
    /// `channel.scale()`); this conversion is what events should
    /// carry so external consumers see SI-consistent coefficients.
    pub fn params_in_real_units(&self) -> Vec<Real> {
        self.form.rescale_params(&self.params, self.channel.scale())
    }
}

/// Probation state for a relation under refinement. The old
/// form remains the civ-visible truth while the new form is on
/// probation.
#[derive(Debug, Clone)]
pub struct RefinementState {
    pub new_form: Form,
    pub proposed_at: u64,
    pub deadline: u64,
}
