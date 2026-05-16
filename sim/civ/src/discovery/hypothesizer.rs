//! `Hypothesizer` — the per-civ quantitative-discovery state machine
//! plus the private constants and helper functions (`exp_neg_two`,
//! `occam_lambda`, `switch_margin`, `falsification_drift_ratio`,
//! `is_trivial_measurement`, `best_confirmable_fit`,
//! `form_priority_tiebreak`) that drive the discovery pipeline.

use super::channels::{relation_id_for, Channel, MeasurementChannel};
use super::events::HypothesisEvent;
use super::types::{
    CandidateRelation, ConfirmedMeasurement, ConfirmedRelation, MeasurementCandidate,
    RefinementState, ResidualBasis,
};
use crate::fit::{fit, rmse, FitResult, Sample};
use crate::forms::{available_forms, Form};
use sim_arith::transcendental::exp;
use sim_arith::Real;
use sim_physics::PhysicsState;
use sim_recognition::Firing;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// refinement-trigger threshold: confidence ≤ exp(-2). The
/// alternative form must beat the active form by at least
/// `score_advantage` on Occam-adjusted score for a refinement to be
/// proposed; the lifecycle constants land directly from.
fn exp_neg_two() -> Real {
    exp(-Real::from_int(2))
}

/// Lifecycle constants (placeholders pending tuning).
const SUSTAINED_TRIGGER_TICKS: u64 = 50;
const REFINEMENT_COOLDOWN_TICKS: u64 = 100;
/// inherited-knowledge revalidation window. Transmitted
/// relations enter the successor's `confirmed` registry tagged
/// with `inherited_from_tick`; after this many ticks the
/// successor evaluates the inherited form on its own samples.
/// Passes graduate to native status, failures emit `RelationLapsed`
/// and drop the relation. 50 ticks (~4 years monthly cadence) is
/// long enough for the successor to accumulate a representative
/// sample of its own physics.
const REVALIDATION_WINDOW_TICKS: u64 = 50;

/// maximum residual-chain depth. auto-generates child
/// candidates when a measurement confirms; lets the chain
/// continue past one level (residual-of-residual). Capping at 3
/// keeps the candidate space bounded — without the cap a
/// single base measurement can spawn unbounded descendants over a
/// long run.
const MAX_RESIDUAL_DEPTH: u32 = 3;

/// prediction-drift trigger. When `falsification_streak` hits
/// this many consecutive ticks (samples-with-RMSE > 1.5× confirm-
/// time residual), the law is mispredicting fast enough that the
/// civ skips the slower confidence-streak path and force-triggers
/// refinement immediately.
///
/// Used as the default for `Hypothesizer::falsification_trigger_ticks`
/// when civ-aware callers haven't yet rescaled by planet metabolism.
/// At 30 ticks = 2.5 sim-yr the trigger was short enough to flap
/// under sample-window noise; civ-aware callers (`configure_substrate`)
/// overwrite this with `streak_ticks_for_metabolism(120, metabolism)`
/// so slow-substrate worlds get correspondingly longer windows.
const DEFAULT_FALSIFICATION_TRIGGER_TICKS: u64 = 30;
/// prediction-drift residual ratio. Streak increments when the
/// RMSE on the latest sample window exceeds this multiple of the
/// fit's confirm-time residual. 1.5× catches genuine drift while
/// tolerating the natural sample-window noise that a tight 1.0×
/// would falsely flag.
fn falsification_drift_ratio() -> Real {
    Real::from_ratio(15, 10)
}
const PROBATION_WINDOW_TICKS: u64 = 200;

fn occam_lambda() -> Real {
    Real::percent(2)
}

fn switch_margin() -> Real {
    Real::percent(5)
}

/// Civ-scoped quantitative discovery pipeline. Holds a per-relation
/// rolling sample window and the set of confirmed relations. Cheap
/// enough that one Hypothesizer per civ is reasonable for M3.
#[derive(Debug, Clone)]
pub struct Hypothesizer {
    /// Candidate relations. Stable order; each holds a unique
    /// `relation_id` assigned at construction.
    pub candidates: Vec<CandidateRelation>,
    /// Rolling per-relation sample window. Capped at `max_window`
    /// per relation so memory stays bounded over a long run.
    pub(super) samples: BTreeMap<u32, VecDeque<Sample>>,
    /// Confirmed-relation registry. Keyed by `relation_id` for the
    /// stable `(template, channel)` identity.
    pub confirmed: BTreeMap<u32, ConfirmedRelation>,
    /// competing-hypothesis registry. Keyed by `relation_id`,
    /// each entry holds zero-or-more *alternative* fits for the
    /// same template-channel pair — different forms (Linear vs
    /// `ThresholdStep`, etc.) that compete with the primary entry
    /// in `confirmed[relation_id]`.
    ///
    /// Real-history analogue: phlogiston vs oxygen, geocentric vs
    /// heliocentric, miasma vs germ — multiple theories can coexist
    /// in a society's canon before one displaces the other. This
    /// data structure makes that coexistence representable in the
    /// sim without rewriting the per-figure fitter.
    ///
    /// Sim/core (or downstream callers) populate via
    /// `add_rival_hypothesis` and resolve via
    /// `displace_primary_with_best_rival`. ships the storage
    /// plus the API; auto-triggering logic (which fits to propose,
    /// when to swap) is intentionally deferred to a follow-up so
    /// the proposal cadence can be tuned without re-shipping the
    /// data-shape change.
    pub rivals: BTreeMap<u32, Vec<ConfirmedRelation>>,
    /// Last fit-attempt tick per relation; throttle the next attempt
    /// to `attempt_period` ticks after.
    pub(super) next_attempt: BTreeMap<u32, u64>,
    pub intelligence: Real,
    pub max_window: usize,
    pub attempt_period: u64,
    /// Form vocabulary the hypothesizer may propose. Defaults
    /// to all 12 forms; civ owners narrow this via
    /// `set_available_forms` to the derivation over perceivable-
    /// template tags.
    pub available_forms: Vec<Form>,
    /// measurement relations — continuous-y candidates that
    /// recover physical-law coefficients (e.g. heat-diffusion
    /// equilibrium slope) from spatial channel pairings. Stable
    /// order; ids in the `1_000_000+` namespace via
    /// `measurement_relation_id`.
    pub measurements: Vec<MeasurementCandidate>,
    /// Per-measurement rolling sample window. Same `max_window` cap
    /// as firing samples.
    pub(super) measurement_samples: BTreeMap<u32, VecDeque<Sample>>,
    /// Confirmed measurement relations.
    pub confirmed_measurements: BTreeMap<u32, ConfirmedMeasurement>,
    /// Last measurement-fit-attempt tick per measurement; throttled
    /// to `attempt_period` like firing relations.
    pub(super) measurement_next_attempt: BTreeMap<u32, u64>,
    /// per-relation experimental-sample contribution count.
    /// Bumped by `record_experimental_measurement` (apparatus cells
    /// from `crate::apparatus`); read at confirm time to set
    /// `ConfirmedMeasurement.is_experimental`. A relation that has
    /// received any apparatus sample at all marks its eventual fit
    /// as experimental — even if observation samples dominate the
    /// pool, the civ's epistemology was intervention-supported.
    pub experimental_count_by_relation: BTreeMap<u32, u32>,
    /// Force-refinement threshold for the prediction-drift streak,
    /// measured in ticks of sustained RMSE > 1.5× confirm-time
    /// residual. Defaults to `DEFAULT_FALSIFICATION_TRIGGER_TICKS`
    /// (30) so legacy tests + planet-agnostic constructions keep
    /// their existing behaviour. Civ-aware founders rewrite this
    /// via `set_falsification_trigger_ticks` from
    /// `streak_ticks_for_metabolism(120, metabolism)` — slow
    /// substrates get correspondingly longer windows so a single
    /// noise burst can't thrash a confirmed relation.
    pub falsification_trigger_ticks: u64,
}

impl Hypothesizer {
    /// Cross-product candidate set for an arbitrary perceivable-
    /// template list. Every (template, channel) pair becomes a
    /// candidate; fits will fail (return `None` from
    /// `crate::fit::fit`) on combinations with no real signal, and
    /// confirm `Constant` / `Linear` / `ThresholdStep` etc. depending
    /// on what the physics produces. Less authored than the M3-era
    /// hand-picked 17-pair list — civs explore the full physically-
    /// possible space and the fit module decides which pairings
    /// carry information.
    pub fn candidates_for(perceivable_template_ids: &[u32]) -> Vec<CandidateRelation> {
        let mut out = Vec::with_capacity(perceivable_template_ids.len() * Channel::ALL.len());
        for tid in perceivable_template_ids {
            for ch in Channel::ALL {
                out.push(CandidateRelation {
                    relation_id: relation_id_for(*tid, ch),
                    template_id: *tid,
                    channel: ch,
                });
            }
        }
        out
    }

    /// Construct a hypothesizer for an explicit perceivable-template
    /// set. Production callers pass the species baseline
    /// here; tests pass whatever subset they're exercising.
    /// `attempt_period` defaults to 20 (Earth-like baseline);
    /// `with_attempt_period` lets thread species-derived
    /// cadence through.
    pub fn new(intelligence: Real, perceivable_template_ids: &[u32]) -> Self {
        Self::with_attempt_period(intelligence, perceivable_template_ids, 20)
    }

    /// construct a hypothesizer with an explicit
    /// `attempt_period` (in ticks per candidate). High-cognition
    /// species pass shorter periods, accumulating discoveries
    /// faster across their candidate space.
    pub fn with_attempt_period(
        intelligence: Real,
        perceivable_template_ids: &[u32],
        attempt_period: u64,
    ) -> Self {
        let candidates = Self::candidates_for(perceivable_template_ids);
        let mut samples = BTreeMap::new();
        let mut next_attempt = BTreeMap::new();
        for c in &candidates {
            samples.insert(c.relation_id, VecDeque::with_capacity(64));
            next_attempt.insert(c.relation_id, 0);
        }
        let measurements = Self::default_measurements(perceivable_template_ids);
        let mut measurement_samples = BTreeMap::new();
        let mut measurement_next_attempt = BTreeMap::new();
        for m in &measurements {
            measurement_samples.insert(m.relation_id, VecDeque::with_capacity(64));
            measurement_next_attempt.insert(m.relation_id, 0);
        }
        Self {
            candidates,
            samples,
            confirmed: BTreeMap::new(),
            rivals: BTreeMap::new(),
            next_attempt,
            intelligence,
            max_window: 200,
            attempt_period: attempt_period.max(5),
            available_forms: available_forms().to_vec(),
            measurements,
            measurement_samples,
            confirmed_measurements: BTreeMap::new(),
            measurement_next_attempt,
            experimental_count_by_relation: BTreeMap::new(),
            falsification_trigger_ticks: DEFAULT_FALSIFICATION_TRIGGER_TICKS,
        }
    }

    /// Rewrite the prediction-drift refinement trigger. Civ-aware
    /// callers use `streak_ticks_for_metabolism(120, metabolism)`
    /// from `sim_civ::demographics` so slow-substrate worlds
    /// (silicate metabolism ≈ 0.2) get ~5× longer falsification
    /// windows than the aqueous baseline. Floors at 1 tick;
    /// passing zero falls back to the existing default.
    pub fn set_falsification_trigger_ticks(&mut self, ticks: u64) {
        if ticks > 0 {
            self.falsification_trigger_ticks = ticks;
        }
    }

    /// record one apparatus sample. Called per-tick by
    /// `crate::apparatus::record_apparatus_samples` after the
    /// physics integrator has run and the apparatus cell holds
    /// its post-physics response. The sample is pushed into the
    /// measurement-relation buffer **twice** to express the
    /// information-density advantage of controlled samples (the
    /// same-bits-from-fewer-samples that real-world experiments
    /// have over passive observation).
    ///
    /// If the relation isn't already in `measurements` (the
    /// apparatus pair may not be in the default catalogue),
    /// adds a fresh `MeasurementCandidate` so subsequent
    /// `step_with_cosmology_and_doubt` calls walk it. Same `relation_id`
    /// from `measurement_relation_id` so naturally-observed samples
    /// for the same `(y, x)` pair already in the buffer continue
    /// to flow into the same fit pool — apparatus and observation
    /// data co-fit.
    pub fn record_experimental_measurement(
        &mut self,
        y_channel: MeasurementChannel,
        x_channel: MeasurementChannel,
        x: Real,
        y: Real,
    ) {
        use super::types::MeasurementCandidate;
        let relation_id = super::channels::measurement_relation_id(y_channel, x_channel);
        // Lazily add the candidate / buffer / next_attempt entries
        // so apparatus pairs not in the default catalogue still
        // flow into the same fit-attempt loop.
        let buf = self
            .measurement_samples
            .entry(relation_id)
            .or_insert_with(|| VecDeque::with_capacity(64));
        for _ in 0..2 {
            if buf.len() == self.max_window {
                buf.pop_front();
            }
            buf.push_back(Sample { x, y });
        }
        self.measurement_next_attempt
            .entry(relation_id)
            .or_insert(0);
        if !self
            .measurements
            .iter()
            .any(|m| m.relation_id == relation_id)
        {
            self.measurements
                .push(MeasurementCandidate::new(y_channel, x_channel));
        }
        let count = self
            .experimental_count_by_relation
            .entry(relation_id)
            .or_insert(0);
        *count = count.saturating_add(2);
    }

    /// default measurement set. Spatial-coupling pairs that
    /// reliably converge on physics-grounded laws when the relevant
    /// channel carries signal:
    ///
    /// - `Temperature` ↔ `NeighbourMean(Temperature)` — heat-
    ///   diffusion equilibrium → linear slope ≈ 1.
    /// - `WaterDepth` ↔ `NeighbourMean(WaterDepth)` — gravity-flow
    ///   smoothing.
    /// - `ChargeMagnitude` ↔ `NeighbourMean(ChargeMagnitude)` — EM
    ///   diffusion smoothing.
    /// - `Temperature` ↔ `Elevation` — thermal lapse-rate proxy.
    ///
    /// `perceivable_template_ids` is reserved for future
    /// sensorium-gated filtering of the measurement set; M3 emits
    /// the full default catalogue and lets fits fail naturally on
    /// archetypes where the channel carries no signal.
    fn default_measurements(_perceivable_template_ids: &[u32]) -> Vec<MeasurementCandidate> {
        vec![
            // spatial-equilibrium baselines.
            MeasurementCandidate::new(
                MeasurementChannel::Direct(Channel::Temperature),
                MeasurementChannel::NeighbourMean(Channel::Temperature),
            ),
            MeasurementCandidate::new(
                MeasurementChannel::Direct(Channel::WaterDepth),
                MeasurementChannel::NeighbourMean(Channel::WaterDepth),
            ),
            MeasurementCandidate::new(
                MeasurementChannel::Direct(Channel::ChargeMagnitude),
                MeasurementChannel::NeighbourMean(Channel::ChargeMagnitude),
            ),
            MeasurementCandidate::new(
                MeasurementChannel::Direct(Channel::Temperature),
                MeasurementChannel::Direct(Channel::Elevation),
            ),
            // temporal-derivative measurements: civ recovers the
            // diffusion coefficient by fitting `dT/dt = α × ∇²T` and
            // analogues for charge / water. These are the actual
            // physical-law coefficients (heat-conduction `α`, EM
            // `conductivity`, gravity-flow `k`), not equilibrium
            // properties.
            MeasurementCandidate::new(
                MeasurementChannel::TemporalDelta(Channel::Temperature),
                MeasurementChannel::Laplacian(Channel::Temperature),
            ),
            MeasurementCandidate::new(
                MeasurementChannel::TemporalDelta(Channel::ChargeMagnitude),
                MeasurementChannel::Laplacian(Channel::ChargeMagnitude),
            ),
            MeasurementCandidate::new(
                MeasurementChannel::TemporalDelta(Channel::WaterDepth),
                MeasurementChannel::Laplacian(Channel::Elevation),
            ),
        ]
    }

    /// Refresh the candidate list against a new perceivable-template
    /// set (e.g. after a tool unlock makes a latent template
    /// perceivable). Stable `relation_id` from `relation_id_for`
    /// preserves existing samples / confirmations for templates
    /// already present; new (template, channel) pairs get fresh
    /// empty buffers; templates removed from the perceivable set
    /// (rare in M3) drop their state.
    pub fn refresh_perceivable(&mut self, perceivable_template_ids: &[u32]) {
        let new_candidates = Self::candidates_for(perceivable_template_ids);
        let new_ids: BTreeSet<u32> = new_candidates.iter().map(|c| c.relation_id).collect();
        // Drop state for relations no longer in scope.
        self.samples.retain(|rid, _| new_ids.contains(rid));
        self.confirmed.retain(|rid, _| new_ids.contains(rid));
        self.next_attempt.retain(|rid, _| new_ids.contains(rid));
        // Add fresh state for new relations.
        for c in &new_candidates {
            self.samples
                .entry(c.relation_id)
                .or_insert_with(|| VecDeque::with_capacity(64));
            self.next_attempt.entry(c.relation_id).or_insert(0);
        }
        self.candidates = new_candidates;
    }

    /// Replace the form vocabulary the hypothesizer can propose.
    /// civ owners derive this from their currently perceivable
    /// recognition templates' structural tags via
    /// `crate::forms::derive_available_forms` and call this whenever
    /// the perceivable set grows (sensorium-extending tool unlock).
    pub fn set_available_forms(&mut self, forms: Vec<Form>) {
        self.available_forms = forms;
    }

    /// Per-tick sample collection over every cell. Equivalent to
    /// `observe_cells(state, None, firings, |_| true)`; tests use
    /// this.
    pub fn observe(&mut self, state: &PhysicsState, firings: &[Firing]) {
        self.observe_cells(state, None, firings, |_| true);
    }

    /// Per-tick sample collection over a cell subset. Each named
    /// figure observes only the cells assigned to them via
    /// their `cell_assignment` (PR C alignment), so figures
    /// accumulate genuinely different sample distributions and
    /// confirm relations they personally observed. Caps the
    /// rolling window at `max_window` per relation.
    ///
    /// `prev_state` is the previous tick's `PhysicsState`,
    /// supplied by the caller (sim/core snapshots before the
    /// integration phase). Measurement channels of variant
    /// `TemporalDelta` skip samples when `prev_state.is_none()`;
    /// other channels ignore the parameter.
    pub fn observe_cells<F>(
        &mut self,
        state: &PhysicsState,
        prev_state: Option<&PhysicsState>,
        firings: &[Firing],
        accept: F,
    ) where
        F: Fn(usize) -> bool,
    {
        // Build a per-template fired-cell set once, deterministic
        // since `firings` is sorted by `(template_id, cell)` upstream.
        let mut fired: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
        for f in firings {
            fired.entry(f.template_id).or_default().insert(f.cell);
        }

        let n_cells = state.grid().n_cells();
        for c in &self.candidates {
            let buf = self
                .samples
                .get_mut(&c.relation_id)
                .expect("samples buffer for known relation");
            let fired_set = fired.get(&c.template_id);
            for cell_i in 0..n_cells {
                if !accept(cell_i) {
                    continue;
                }
                let cell_u32 = u32::try_from(cell_i).unwrap_or(u32::MAX);
                let y = if fired_set.is_some_and(|s| s.contains(&cell_u32)) {
                    Real::ONE
                } else {
                    Real::ZERO
                };
                let x = c.channel.read(state, cell_i);
                if buf.len() == self.max_window {
                    buf.pop_front();
                }
                buf.push_back(Sample { x, y });
            }
        }

        // continuous-y measurement samples in parallel. Same
        // cell-assignment filter applies so each figure's measurement
        // pool is genuinely their own subset.
        // TemporalDelta channels return None when prev_state is
        // unavailable; samples skip the buffer when either axis
        // can't be read.
        // residual-basis candidates subtract the basis's
        // prediction from y before fitting, so the fit operates on
        // *what's left* after the base law explains its share.
        for m in &self.measurements {
            let buf = self
                .measurement_samples
                .get_mut(&m.relation_id)
                .expect("measurement buffer for known relation");
            for cell_i in 0..n_cells {
                if !accept(cell_i) {
                    continue;
                }
                let Some(mut y) = m.y_channel.read(state, prev_state, cell_i) else {
                    continue;
                };
                let Some(x) = m.x_channel.read(state, prev_state, cell_i) else {
                    continue;
                };
                if let Some(basis) = &m.residual_basis {
                    let Some(basis_x) = basis.source_x_channel.read(state, prev_state, cell_i)
                    else {
                        continue;
                    };
                    let predicted = basis.source_form.evaluate(&basis.source_params, basis_x);
                    y = y - predicted;
                }
                if buf.len() == self.max_window {
                    buf.pop_front();
                }
                buf.push_back(Sample { x, y });
            }
        }
    }

    /// Per-tick hypothesizer step. Walks every candidate relation:
    /// - Unconfirmed and due: tries the form vocabulary; emits
    ///   `Confirmed` on first fit clearing `exp(-1)`.
    /// - Confirmed without active refinement: re-evaluates the
    ///   active form's confidence on the latest samples; tracks the
    ///   sustained-low-confidence streak; on trigger, finds the best
    ///   Occam-adjusted alternative and emits `RefinementProposed`
    ///   if it beats the current by `switch_margin`.
    /// - In probation: re-fits the candidate; on confirm emits
    ///   `RefinementConfirmed` and switches the active form; on
    ///   deadline expiry emits `RefinementRejected`, reverts, and
    ///   starts cooldown.
    pub fn step(&mut self, tick: u64) -> Vec<HypothesisEvent> {
        self.step_with_cosmology(tick, &crate::cosmology::Cosmology::NEUTRAL)
    }

    /// register an alternative fit as a rival hypothesis
    /// for `relation_id`. Returns `true` if accepted, `false` if
    /// rejected because the same form already exists in
    /// `confirmed[relation_id]` or in the rivals list (no
    /// duplicates). The primary fit in `confirmed[relation_id]`
    /// is never modified — call `displace_primary_with_best_rival`
    /// to swap.
    pub fn add_rival_hypothesis(&mut self, relation_id: u32, rival: ConfirmedRelation) -> bool {
        if let Some(primary) = self.confirmed.get(&relation_id) {
            if primary.form == rival.form {
                return false;
            }
        }
        let entry = self.rivals.entry(relation_id).or_default();
        if entry.iter().any(|r| r.form == rival.form) {
            return false;
        }
        entry.push(rival);
        true
    }

    /// if any rival for `relation_id` has higher confidence
    /// than the primary, swap them — the primary becomes a rival,
    /// the highest-confidence rival becomes primary. Returns
    /// `Some((displaced_form, new_form))` on swap, `None` if no
    /// rival qualifies or no primary exists. The displaced primary
    /// re-enters the rivals list so future swaps can flip back.
    pub fn displace_primary_with_best_rival(
        &mut self,
        relation_id: u32,
    ) -> Option<(crate::forms::Form, crate::forms::Form)> {
        let rivals = self.rivals.get(&relation_id)?;
        let best_idx = rivals
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.confidence.cmp(&b.1.confidence))
            .map(|(i, _)| i)?;
        let primary = self.confirmed.get(&relation_id)?.clone();
        if rivals[best_idx].confidence <= primary.confidence {
            return None;
        }
        // SAFETY: `best_idx` is in-bounds by construction above.
        let new_primary = self
            .rivals
            .get_mut(&relation_id)
            .expect("rival entry present")
            .remove(best_idx);
        let old_form = primary.form;
        let new_form = new_primary.form;
        self.confirmed.insert(relation_id, new_primary);
        // The displaced primary returns to the rivals pool so a
        // future flip can restore it. Stable order (push_back).
        self.rivals.entry(relation_id).or_default().push(primary);
        Some((old_form, new_form))
    }

    /// variant that applies cosmology-driven confidence
    /// suppression to candidate fits before the
    /// `is_confirmed (>= exp(-1))` check. A neutral cosmology
    /// makes this equivalent to `step(tick)`.
    pub fn step_with_cosmology(
        &mut self,
        tick: u64,
        cosmology: &crate::cosmology::Cosmology,
    ) -> Vec<HypothesisEvent> {
        // Default: doubt = 0.5 (neutral). Owning-figure callers
        // use `step_with_cosmology_and_doubt` to thread a per-
        // figure doubt scalar into 's switch_margin.
        self.step_with_cosmology_and_doubt(tick, cosmology, Real::from_ratio(5, 10))
    }

    /// Variant that applies cosmology-driven confidence
    /// suppression AND per-figure doubt to refinement-readiness
    /// gating. High-doubt figures push their relations toward
    /// refinement faster (effective `switch_margin` reduced); low-
    /// doubt figures stick with confirmed forms longer.
    /// `doubt` is the figure's `[0, 1]` doubt scalar; 0.5 is
    /// neutral (matches the default `step_with_cosmology`).
    pub fn step_with_cosmology_and_doubt(
        &mut self,
        tick: u64,
        cosmology: &crate::cosmology::Cosmology,
        doubt: Real,
    ) -> Vec<HypothesisEvent> {
        // Default discovery-rate multiplier of `1.0` — no cadence
        // change. Civ-aware callers thread a `(1 + tool bonus)`
        // multiplier via `step_with_cosmology_doubt_and_rate`.
        self.step_with_cosmology_doubt_and_rate(tick, cosmology, doubt, Real::ONE)
    }

    /// Variant that further accepts a discovery-rate multiplier.
    /// Tools that accelerate the science loop (analytical engines,
    /// digital computation, experiment apparatus) lift this above
    /// `1.0`; the hypothesizer's `attempt_period` is divided by it
    /// (clamped to `≥ 1` tick) when scheduling the next fit
    /// attempt for each candidate / measurement, so faster civs
    /// propose and confirm more often. Multiplier `≤ 1.0` falls
    /// back to the unscaled period so pre-tech civs and existing
    /// tests pass through unchanged. The raw `attempt_period` stays
    /// canonical for the streak-threshold formula
    /// (`SUSTAINED_TRIGGER_TICKS / attempt_period`) so calibration
    /// holds.
    #[allow(clippy::too_many_lines)]
    pub fn step_with_cosmology_doubt_and_rate(
        &mut self,
        tick: u64,
        cosmology: &crate::cosmology::Cosmology,
        doubt: Real,
        discovery_rate: Real,
    ) -> Vec<HypothesisEvent> {
        let next_period = if discovery_rate <= Real::ONE {
            self.attempt_period
        } else {
            let raw = self.attempt_period.max(1);
            let raw_real = Real::from_int(i64::try_from(raw).unwrap_or(1));
            let scaled = raw_real / discovery_rate;
            let bits = scaled.raw().to_bits();
            let floor = u64::try_from((bits >> 32).max(0)).unwrap_or(0);
            floor.max(1)
        };
        let mut events = Vec::new();
        // Snapshot relation_ids first so we can mutate `self.confirmed`
        // inside the loop without borrow conflicts.
        let candidate_ids: Vec<u32> = self.candidates.iter().map(|c| c.relation_id).collect();

        for relation_id in candidate_ids {
            let due = self.next_attempt.get(&relation_id).copied().unwrap_or(0);
            if tick < due {
                continue;
            }
            self.next_attempt.insert(relation_id, tick + next_period);

            let samples: Vec<Sample> = self
                .samples
                .get(&relation_id)
                .map(|b| b.iter().copied().collect())
                .unwrap_or_default();

            if self.confirmed.contains_key(&relation_id) {
                self.step_confirmed(relation_id, &samples, tick, doubt, &mut events);
            } else {
                self.step_unconfirmed(relation_id, &samples, tick, cosmology, &mut events);
            }
        }

        // parallel fit pass for measurement relations. M3
        // measurements are confirm-once; refinement (sustained-low-
        // confidence drift, probation) lands when temporal-derivative
        // measurements arrive and the law actually has a coefficient
        // worth re-fitting against new data.
        let measurement_ids: Vec<u32> = self.measurements.iter().map(|m| m.relation_id).collect();
        for relation_id in measurement_ids {
            if self.confirmed_measurements.contains_key(&relation_id) {
                continue;
            }
            let due = self
                .measurement_next_attempt
                .get(&relation_id)
                .copied()
                .unwrap_or(0);
            if tick < due {
                continue;
            }
            self.measurement_next_attempt
                .insert(relation_id, tick + next_period);

            let samples: Vec<Sample> = self
                .measurement_samples
                .get(&relation_id)
                .map(|b| b.iter().copied().collect())
                .unwrap_or_default();
            let Some(m) = self
                .measurements
                .iter()
                .find(|m| m.relation_id == relation_id)
                .cloned()
            else {
                continue;
            };
            let Some(res) = best_confirmable_fit(
                &samples,
                self.intelligence,
                &self.available_forms,
                cosmology,
            ) else {
                continue;
            };
            // trivial-fit filter: a measurement fit is degenerate
            // when the underlying field is uniform across the sample
            // window — every cell holds essentially the same value, so
            // any (x, y) channel pairing reduces to a constant or an
            // identity (`y = x`) with residual ~ 0. The fit "confirms"
            // but the species learned nothing about the law. Examples:
            // sub-surface-ocean planets where the ice-shell column is
            // uniform; gaseous-shell worlds early-tick before EM
            // diffusion produces a gradient. Reject when sample
            // variance on either axis is below the trivial threshold.
            if is_trivial_measurement(&samples) {
                continue;
            }
            let confirmed = ConfirmedMeasurement {
                relation_id: m.relation_id,
                y_channel: m.y_channel,
                x_channel: m.x_channel,
                form: res.form,
                params: res.params,
                residual: res.residual,
                confidence: res.confidence,
                n_samples: res.n_samples,
                confirmed_at_tick: tick,
                // any apparatus contribution flips the flag.
                // The fit may include observation samples too, but
                // the civ's epistemology was intervention-supported.
                is_experimental: self
                    .experimental_count_by_relation
                    .get(&m.relation_id)
                    .copied()
                    .unwrap_or(0)
                    > 0,
            };
            self.confirmed_measurements
                .insert(m.relation_id, confirmed.clone());
            events.push(HypothesisEvent::MeasurementConfirmed(confirmed.clone()));

            // auto-generate residual candidates that try to
            // explain what's left after this base relation. For each
            // other measurement channel, add a candidate fitting the
            // residual y against that channel as x. Lets civs build
            // hierarchical theories — once temperature is explained
            // by neighbour-mean, the *residual* from that fit might
            // correlate with elevation (lapse rate), with charge
            // (joule heating), etc.
            //
            // The basis is *frozen* (form + params snapshotted at
            // compose time) so refinement of the source doesn't
            // retroactively invalidate the child. The trade-off is
            // documented in q76.md: the hierarchy captures the civ's
            // understanding at the moment of composition, which is
            // a more faithful biography than live re-evaluation.
            // cap the auto-generation depth so the candidate
            // space stays bounded. A confirming candidate at depth D
            // spawns children at depth D+1; depth >= MAX_RESIDUAL_DEPTH
            // skips generation entirely.
            if m.residual_depth >= MAX_RESIDUAL_DEPTH {
                continue;
            }
            let child_depth = m.residual_depth + 1;
            let basis = ResidualBasis {
                source_relation_id: m.relation_id,
                source_form: confirmed.form,
                source_params: confirmed.params.clone(),
                source_x_channel: m.x_channel,
            };
            // The y-axis stays the source's direct y (so the
            // subtraction yields a real residual); the x-axis
            // is `Elevation` only, the most physically meaningful
            // secondary channel for temperature / water dynamics.
            // Keeping the residual catalogue small keeps the
            // hypothesizer's per-tick fit cost bounded — every
            // additional residual child is another 96-cell sample
            // pool and a periodic least-squares fit attempt.
            let secondary_x = [MeasurementChannel::Direct(Channel::Elevation)];
            for x_ch in secondary_x {
                let candidate =
                    MeasurementCandidate::residual(m.y_channel, x_ch, basis.clone(), child_depth);
                if self
                    .measurement_samples
                    .contains_key(&candidate.relation_id)
                {
                    continue; // already added (idempotent)
                }
                self.measurement_samples
                    .insert(candidate.relation_id, VecDeque::with_capacity(64));
                self.measurement_next_attempt
                    .insert(candidate.relation_id, tick);
                self.measurements.push(candidate);
            }
        }

        events
    }

    fn step_unconfirmed(
        &mut self,
        relation_id: u32,
        samples: &[Sample],
        tick: u64,
        cosmology: &crate::cosmology::Cosmology,
        events: &mut Vec<HypothesisEvent>,
    ) {
        let Some(c) = self
            .candidates
            .iter()
            .find(|c| c.relation_id == relation_id)
            .cloned()
        else {
            return;
        };
        let Some(res) =
            best_confirmable_fit(samples, self.intelligence, &self.available_forms, cosmology)
        else {
            return;
        };
        let confirmed = ConfirmedRelation {
            relation_id: c.relation_id,
            template_id: c.template_id,
            channel: c.channel,
            form: res.form,
            params: res.params,
            residual: res.residual,
            confidence: res.confidence,
            n_samples: res.n_samples,
            confirmed_at_tick: tick,
            low_confidence_streak: 0,
            cooldown_until: 0,
            refinement: None,
            initial_residual: res.residual,
            falsification_streak: 0,
            inherited_from_tick: None,
            inherited_from_civ_id: None,
        };
        self.confirmed.insert(c.relation_id, confirmed.clone());
        events.push(HypothesisEvent::Confirmed(confirmed));
    }

    #[allow(clippy::too_many_lines)]
    fn step_confirmed(
        &mut self,
        relation_id: u32,
        samples: &[Sample],
        tick: u64,
        doubt: Real,
        events: &mut Vec<HypothesisEvent>,
    ) {
        // revalidation window expiry. Inherited relations
        // graduate or lapse when the window closes; the rest of
        // step_confirmed continues only on graduated / native
        // relations.
        let intelligence = self.intelligence;
        let revalidation_outcome = {
            let Some(rel) = self.confirmed.get(&relation_id) else {
                return;
            };
            rel.inherited_from_tick.and_then(|tick0| {
                if tick.saturating_sub(tick0) < REVALIDATION_WINDOW_TICKS {
                    return None;
                }
                let from_civ_id = rel.inherited_from_civ_id.unwrap_or(0);
                let template_id = rel.template_id;
                let attempted_form = rel.form;
                Some((from_civ_id, template_id, attempted_form))
            })
        };
        if let Some((from_civ_id, template_id, attempted_form)) = revalidation_outcome {
            // Re-fit the inherited form against the successor's
            // own samples. Pass: graduate to native confirmed.
            // Fail: emit Lapsed, drop from confirmed.
            let refit = fit(attempted_form, samples, intelligence);
            match refit {
                Some(res) if res.is_confirmed() => {
                    if let Some(rel) = self.confirmed.get_mut(&relation_id) {
                        rel.params = res.params;
                        rel.residual = res.residual;
                        rel.initial_residual = res.residual;
                        rel.confidence = res.confidence;
                        rel.n_samples = res.n_samples;
                        rel.inherited_from_tick = None;
                        rel.inherited_from_civ_id = None;
                        rel.confirmed_at_tick = tick;
                    }
                    events.push(HypothesisEvent::Revalidated {
                        relation_id,
                        template_id,
                        from_civ_id,
                        new_residual: self
                            .confirmed
                            .get(&relation_id)
                            .map_or(Real::ZERO, |r| r.residual),
                        new_confidence: self
                            .confirmed
                            .get(&relation_id)
                            .map_or(Real::ZERO, |r| r.confidence),
                    });
                }
                _ => {
                    self.confirmed.remove(&relation_id);
                    events.push(HypothesisEvent::Lapsed {
                        relation_id,
                        template_id,
                        from_civ_id,
                        attempted_form,
                    });
                    return;
                }
            }
        }

        // Re-evaluate against the active form. Without enough samples
        // to support the current form, leave state untouched.
        let active_form;
        let active_params;
        let in_probation;
        let probation_deadline;
        let cooldown_until;
        {
            let Some(rel) = self.confirmed.get(&relation_id) else {
                return;
            };
            active_form = rel.form;
            active_params = rel.params.clone();
            in_probation = rel.refinement.is_some();
            probation_deadline = rel.refinement.as_ref().map_or(0, |r| r.deadline);
            cooldown_until = rel.cooldown_until;
        }

        let active_residual = rmse(active_form, &active_params, samples);
        let active_tolerance =
            crate::fit::compute_tolerance(active_form, intelligence, samples.len());
        let active_confidence = if active_tolerance > Real::ZERO {
            exp(-(active_residual / active_tolerance))
        } else {
            Real::ZERO
        };

        // track prediction-drift streak. The active law's RMSE
        // against the rolling sample window is what the law would
        // produce as a per-cell prediction error if used to forecast
        // y from x. Sustained drift > 1.5× the confirm-time residual
        // means the law is mispredicting reliably; force-trigger
        // refinement faster than the confidence-streak path. Treat
        // a near-zero confirm-time residual as 1e-3 so trivially-
        // perfect fits don't emit nan ratios.
        let mut falsified_now = false;
        if let Some(rel) = self.confirmed.get_mut(&relation_id) {
            rel.residual = active_residual;
            rel.confidence = active_confidence;
            rel.n_samples = samples.len();

            let drift_floor = rel.initial_residual.max(Real::from_ratio(1, 1000));
            let drift_threshold = drift_floor * falsification_drift_ratio();
            if active_residual > drift_threshold {
                rel.falsification_streak = rel.falsification_streak.saturating_add(1);
            } else {
                rel.falsification_streak = 0;
            }
            if rel.falsification_streak >= self.falsification_trigger_ticks && !in_probation {
                falsified_now = true;
                // Force the confidence-streak path to fire by lifting
                // the streak counter to its trigger value; the
                // existing refinement-proposed branch then runs the
                // form-search and emits RefinementProposed.
                rel.low_confidence_streak = SUSTAINED_TRIGGER_TICKS;
                // Reset the streak so the event fires once per drift
                // episode, not once per tick of sustained drift.
                // Refinement runs on the next iteration; if it
                // confirms, the new fit's residual resets the
                // baseline.
                rel.falsification_streak = 0;
            }
        }
        if falsified_now {
            events.push(HypothesisEvent::Falsified {
                relation_id,
                template_id: self
                    .confirmed
                    .get(&relation_id)
                    .map_or(0, |r| r.template_id),
                old_form: active_form,
                streak_ticks: self.falsification_trigger_ticks,
            });
        }

        // Probation handling : on each step, re-fit the candidate
        // form. Confirm if it clears `exp(-1)`; expire on deadline.
        if in_probation {
            let new_form = self
                .confirmed
                .get(&relation_id)
                .and_then(|r| r.refinement.as_ref().map(|s| s.new_form))
                .expect("probation state present");

            if let Some(new_fit) = fit(new_form, samples, intelligence) {
                if new_fit.is_confirmed() {
                    if let Some(rel) = self.confirmed.get_mut(&relation_id) {
                        let template_id = rel.template_id;
                        let old_form = rel.form;
                        rel.form = new_form;
                        rel.params.clone_from(&new_fit.params);
                        rel.residual = new_fit.residual;
                        rel.confidence = new_fit.confidence;
                        rel.refinement = None;
                        rel.low_confidence_streak = 0;
                        rel.cooldown_until = 0;
                        events.push(HypothesisEvent::RefinementConfirmed {
                            relation_id,
                            template_id,
                            channel: rel.channel,
                            old_form,
                            new_form,
                            new_params: new_fit.params,
                            new_residual: new_fit.residual,
                            new_confidence: new_fit.confidence,
                            n_samples: new_fit.n_samples,
                        });
                        return;
                    }
                }
            }

            if tick >= probation_deadline {
                if let Some(rel) = self.confirmed.get_mut(&relation_id) {
                    rel.refinement = None;
                    rel.low_confidence_streak = 0;
                    rel.cooldown_until = tick + REFINEMENT_COOLDOWN_TICKS;
                    events.push(HypothesisEvent::RefinementRejected {
                        relation_id,
                        template_id: rel.template_id,
                        old_form: rel.form,
                        attempted_form: new_form,
                        reason: "probation_deadline_expired".to_string(),
                    });
                }
            }
            return;
        }

        // Outside probation. Are we below the trigger threshold?
        if active_confidence <= exp_neg_two() {
            if let Some(rel) = self.confirmed.get_mut(&relation_id) {
                rel.low_confidence_streak = rel.low_confidence_streak.saturating_add(1);
            }
        } else if let Some(rel) = self.confirmed.get_mut(&relation_id) {
            rel.low_confidence_streak = 0;
        }

        // Cooldown gates a new trigger.
        if tick < cooldown_until {
            return;
        }

        // Streak threshold: trigger refinement candidate selection.
        // Note `step` runs at `attempt_period` cadence; the streak is
        // measured in step counts. Convert 's tick-based 50 by
        // dividing by attempt_period (rounded up), so the wall-clock
        // dynamics roughly match the spec.
        let streak = self
            .confirmed
            .get(&relation_id)
            .map_or(0, |r| r.low_confidence_streak);
        let streak_threshold = SUSTAINED_TRIGGER_TICKS.div_ceil(self.attempt_period.max(1));

        if streak < streak_threshold {
            return;
        }

        // candidate selection: re-fit every available form,
        // score = confidence - λ × param_count, propose the new form
        // only if score_best > score_current + switch_margin.
        let active_score = active_confidence
            - occam_lambda()
                * Real::from_int(i64::try_from(active_form.param_count()).unwrap_or(0));
        let mut best: Option<(Form, FitResult, Real)> = None;
        let active_forms = self.available_forms.clone();
        for form in &active_forms {
            if *form == active_form {
                continue;
            }
            let Some(res) = fit(*form, samples, intelligence) else {
                continue;
            };
            let score = res.confidence
                - occam_lambda() * Real::from_int(i64::try_from(form.param_count()).unwrap_or(0));
            if best
                .as_ref()
                .is_none_or(|(_, _, prev_score)| score > *prev_score)
            {
                best = Some((*form, res, score));
            }
        }

        if let Some((new_form, _new_fit, score)) = best {
            // doubt: scale the switch margin per-figure.
            // doubt = 0.5 (neutral) → 1.0 × margin; doubt = 1.0 →
            // 0.5 × margin (more aggressive); doubt = 0.0 → 1.5 ×
            // margin (more conservative).
            let doubt_scale = Real::from_ratio(15, 10) - doubt;
            let scaled_margin = switch_margin() * doubt_scale;
            if score > active_score + scaled_margin {
                let proposed_at = tick;
                let deadline = tick + PROBATION_WINDOW_TICKS;
                if let Some(rel) = self.confirmed.get_mut(&relation_id) {
                    rel.refinement = Some(RefinementState {
                        new_form,
                        proposed_at,
                        deadline,
                    });
                    rel.low_confidence_streak = 0;
                    let template_id = rel.template_id;
                    let old_form = rel.form;
                    let old_confidence = rel.confidence;
                    let n_samples = rel.n_samples;
                    events.push(HypothesisEvent::RefinementProposed {
                        relation_id,
                        template_id,
                        old_form,
                        new_form,
                        old_confidence,
                        n_samples,
                    });
                    return;
                }
            }
        }

        // No proposal landed but the streak fired — reset streak
        // anyway and start cooldown to avoid retry-storms.
        if let Some(rel) = self.confirmed.get_mut(&relation_id) {
            rel.low_confidence_streak = 0;
            rel.cooldown_until = tick + REFINEMENT_COOLDOWN_TICKS;
        }
    }
}

/// Try every form in priority order; return the first confirmed
/// fit, or `None`. Priority is low-arity first so simpler
/// explanations are preferred where they fit ('s Occam spirit
/// at initial-confirmation time). The `available_forms` slice is
/// the per-civ derivation; passing it explicitly keeps the
/// helper independent of the placeholder `forms::available_forms()`.
/// trivial-measurement filter. A measurement fit is degenerate
/// when the sample window has near-zero variance on either axis —
/// the channel pairing carries no information for the fitter and any
/// confirmed law is an artefact of sampling identical points. Reject
/// these so the species canon only carries fits over genuinely
/// varying data (the meaningful diffusion-equilibrium slope on a
/// gradient, not the trivial `y = x` on a uniform field).
///
/// Threshold rationale: samples are stored in fit-space (each axis
/// normalised by `Channel::scale()`), so meaningful variations land
/// in O(1). `1e-4` in Q32.32 ≈ 429497 raw bits — well above numerical
/// noise from the fitter, well below any real signal worth fitting.
pub(super) fn is_trivial_measurement(samples: &[Sample]) -> bool {
    if samples.len() < 2 {
        return true;
    }
    let n = Real::from_int(i64::try_from(samples.len()).unwrap_or(i64::MAX));
    let mean = |project: fn(&Sample) -> Real| -> Real {
        let sum = samples.iter().map(project).fold(Real::ZERO, |a, b| a + b);
        sum / n
    };
    let variance = |project: fn(&Sample) -> Real| -> Real {
        let m = mean(project);
        let sum_sq = samples
            .iter()
            .map(|s| {
                let d = project(s) - m;
                d * d
            })
            .fold(Real::ZERO, |a, b| a + b);
        sum_sq / n
    };
    let var_x = variance(|s| s.x);
    let var_y = variance(|s| s.y);
    let threshold = Real::from_ratio(1, 10_000);
    var_x < threshold || var_y < threshold
}

fn best_confirmable_fit(
    samples: &[Sample],
    intelligence: Real,
    available_forms: &[Form],
    cosmology: &crate::cosmology::Cosmology,
) -> Option<FitResult> {
    let mut sorted: Vec<Form> = available_forms.to_vec();
    sorted.sort_by_key(|f| (f.param_count(), form_priority_tiebreak(*f)));
    for form in sorted {
        if let Some(mut res) = fit(form, samples, intelligence) {
            // cosmology suppression: dogmatic civs find
            // heretical forms harder to confirm.
            let suppress = crate::culture_hooks::suppress_confidence_for(form, cosmology);
            // focus weight: empirical/reformist civs get a
            // confidence boost; mystical/communitarian civs see
            // the inverse. Both factors compound on the raw fit
            // confidence so the cosmology vector drives the
            // *direction* of who confirms what, not just whether
            // confirmation gates open.
            let focus = crate::culture_hooks::focus_weight_for(0, cosmology);
            res.confidence = (res.confidence * suppress * focus).min(Real::ONE);
            if res.is_confirmed() {
                return Some(res);
            }
        }
    }
    None
}

/// Stable tie-break ordering when two forms share `param_count`.
/// Prefers more "structural" forms (`Linear` before `InverseSquare`
/// for 2-arity ties; `ThresholdStep` before `Polynomial2` at 3-arity).
/// Match arms enumerated per form for clarity.
#[allow(clippy::match_same_arms)]
fn form_priority_tiebreak(f: Form) -> u8 {
    match f {
        Form::Constant => 0,
        Form::Linear => 0,
        Form::InverseSquare => 1,
        Form::Logarithmic => 2,
        Form::ExpDecay => 3,
        Form::ExpGrowth => 4,
        Form::PowerLaw => 5,
        Form::ThresholdStep => 0,
        Form::Polynomial2 => 1,
        Form::Logistic => 2,
        Form::Polynomial3 => 0,
        Form::PeriodicSine => 1,
    }
}
