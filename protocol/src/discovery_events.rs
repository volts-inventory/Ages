//! Discovery-pipeline events: relation confirmation/refinement/
//! falsification/revalidation/lapsing, measurement confirmation,
//! and the rival-hypothesis lifecycle plus mythology.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A relation a named figure has confirmed: a recognition channel
/// (the independent variable) ↔ recognition template (the dependent
/// variable) bound to a fitted functional form. The
/// `params` are emitted as Q32.32 raw bits for bit-exact event-log
/// determinism.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RelationConfirmed {
    pub tick: u64,
    /// Stable id assigned by the civ when the relation is registered.
    pub relation_id: u32,
    /// The figure (named individual) who confirmed it. M2-era
    /// scaffold has no figures yet; emit `0` for cohort-level
    /// confirmations until figures land.
    pub figure_id: u32,
    /// Recognition template id observed (`y`).
    pub template_id: u32,
    /// Recognition template name (`y`'s human-readable label). Repeated
    /// here so the post-run report and any other consumer can render
    /// relations without joining against the recognition library.
    pub template_name: String,
    /// Channel (independent variable, `x`) snake-case tag.
    pub channel: String,
    /// `Form::tag()` snake-case identifier.
    pub form: String,
    /// Q32.32 raw bits of each fitted parameter, in canonical order.
    pub params_q32: Vec<i64>,
    pub residual_q32: i64,
    pub confidence_q32: i64,
    pub n_samples: u32,
}

/// A figure has triggered a refinement on a previously confirmed
/// relation. The old form remains the civ-visible truth during
/// probation; a `RefinementConfirmed` or `RefinementRejected` lands
/// later.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RefinementProposed {
    pub tick: u64,
    pub figure_id: u32,
    pub relation_id: u32,
    pub old_form: String,
    pub new_form: String,
    /// Confidence on the old form at the moment of trigger
    /// (`exp(-residual/tolerance)`), encoded as Q32.32 raw bits.
    pub old_confidence_q32: i64,
    pub n_samples: u32,
}

/// Measurement relation: a continuous-y law the civ has
/// fitted from its surroundings. Distinct from `RelationConfirmed`
/// (firing-binary y); samples here are
/// `(x = continuous channel reading, y = continuous channel
/// reading)` so `params` recover *coefficients*, not thresholds.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MeasurementConfirmed {
    pub tick: u64,
    pub civ_id: u32,
    pub figure_id: u32,
    pub relation_id: u32,
    /// y-axis selector tag (e.g. `temperature`,
    /// `neighbour_mean_temperature`).
    pub y_channel: String,
    /// x-axis selector tag.
    pub x_channel: String,
    /// `Form::tag()` snake-case identifier.
    pub form: String,
    /// Q32.32 raw bits of each fitted parameter, rescaled to real
    /// (SI) units from the fit-space normalisation on both axes.
    pub params_q32: Vec<i64>,
    pub residual_q32: i64,
    pub confidence_q32: i64,
    pub n_samples: u32,
    /// At least one sample in this relation's fit pool came
    /// from a controlled-conditions apparatus the civ built — a
    /// clamped channel value paired with the post-physics response
    /// in the apparatus cell. Distinguishes Galileo-style intervention
    /// from passive observation; `false` = pure observation, `true`
    /// = at least one experimental sample contributed. Older
    /// event logs (and runs without an unlocked apparatus) decode
    /// this as `false` via the serde default.
    #[serde(default)]
    pub is_experimental: bool,
}

/// Probation succeeded: the proposed form fits well enough to
/// supersede the old one.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RefinementConfirmed {
    pub tick: u64,
    pub figure_id: u32,
    pub relation_id: u32,
    pub old_form: String,
    pub new_form: String,
    pub new_params_q32: Vec<i64>,
    pub new_residual_q32: i64,
    pub new_confidence_q32: i64,
    pub n_samples: u32,
}

/// Probation expired without confirmation; the relation reverts to
/// the old form and starts cooldown.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RefinementRejected {
    pub tick: u64,
    pub figure_id: u32,
    pub relation_id: u32,
    pub old_form: String,
    pub attempted_form: String,
    pub reason: String,
}

/// A confirmed relation has mispredicted fresh observations
/// for a sustained streak of ticks. The civ's law is no longer
/// holding up against new data; refinement force-triggers in the
/// same tick. Pure narrative signal — refinement still flows
/// through `RefinementProposed` after this fires.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RelationFalsified {
    pub tick: u64,
    pub civ_id: u32,
    pub figure_id: u32,
    pub relation_id: u32,
    /// `Form::tag()` of the falsified form (the one that fit at
    /// confirm time but no longer predicts well).
    pub old_form: String,
    /// How many consecutive ticks of drift triggered the fall —
    /// a fixed `FALSIFICATION_TRIGGER_TICKS` from `sim_civ` for
    /// now; threading the value through the event payload keeps
    /// the protocol self-describing for downstream consumers.
    pub streak_ticks: u64,
}

/// An inherited relation re-fit successfully on the successor
/// civ's own observations after the revalidation window. The
/// relation graduates to native confirmed status — the successor
/// has now *measured* the parent's law on its own data. Distinct
/// from the original transmission event, which carried the
/// inheritance event but not the verification.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RelationRevalidated {
    pub tick: u64,
    pub civ_id: u32,
    pub figure_id: u32,
    pub relation_id: u32,
    pub from_civ_id: u32,
    pub new_residual_q32: i64,
    pub new_confidence_q32: i64,
}

/// An inherited relation failed to re-fit on the successor's
/// observations and has been dropped from `confirmed`. The civ has
/// rejected the parent's theory. Emits the form that was attempted
/// so the report can show "civ X tried Y's lightning law and could
/// not reproduce it on their own observations."
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RelationLapsed {
    pub tick: u64,
    pub civ_id: u32,
    pub figure_id: u32,
    pub relation_id: u32,
    pub from_civ_id: u32,
    pub attempted_form: String,
}

/// Rival-hypothesis proposal payload. `primary_form` and
/// `rival_form` are the snake-case form tags (`linear`,
/// `threshold_step`, etc.). Confidence values are Q32.32 raw bits.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RivalHypothesisProposed {
    pub tick: u64,
    pub civ_id: u32,
    pub figure_id: u32,
    pub relation_id: u32,
    pub primary_form: String,
    pub rival_form: String,
    pub primary_confidence_q32: i64,
    pub rival_confidence_q32: i64,
}

/// Primary-hypothesis displacement payload. Emitted when a
/// rival's confidence exceeded the primary's and they swapped.
/// `old_form` / `new_form` are the snake-case form tags before
/// and after the swap.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PrimaryHypothesisDisplaced {
    pub tick: u64,
    pub civ_id: u32,
    pub relation_id: u32,
    pub old_form: String,
    pub new_form: String,
    pub old_confidence_q32: i64,
    pub new_confidence_q32: i64,
}

/// Relation mythologization payload. Emitted when a
/// transmission's comprehension score fell in the
/// `(MYTH_FLOOR, TRANSMIT_THRESHOLD]` band. `axis` is `0..=4`
/// indexing `(empirical, communitarian, reformist, mystical,
/// hierarchical)`. `magnitude_q32` is the cosmology push
/// magnitude in `[0, 0.05]`. `comprehension_q32` is the
/// score that fell short of the transmit threshold.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RelationMythologized {
    pub tick: u64,
    pub source_civ_id: u32,
    pub dest_civ_id: u32,
    pub relation_id: u32,
    pub axis: u8,
    pub magnitude_q32: i64,
    pub comprehension_q32: i64,
}
