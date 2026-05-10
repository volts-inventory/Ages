//! Per-figure observation + hypothesis stepping, cosmology
//! drift emission gating, and perceivable-form refresh after
//! sensorium changes.

use crate::cosmology;
use crate::discovery::HypothesisEvent;
use crate::religion;
use crate::{forms, Civ};
use sim_arith::Real;
use sim_recognition::{Firing, RecognitionLibrary};
use std::collections::BTreeSet;

impl Civ {
    /// Drive each active figure's hypothesizer with their assigned
    /// cell subset (`cell_idx % n_active == cell_assignment`). PR C
    /// alignment: real per-figure observation pools, not
    /// round-robin labels.
    pub fn observe_per_figure(
        &mut self,
        state: &sim_physics::PhysicsState,
        prev_state: Option<&sim_physics::PhysicsState>,
        firings: &[Firing],
    ) {
        let n_active = self.n_active_figures().max(1);
        for fig in &mut self.figures {
            if fig.retired_tick.is_some() {
                continue;
            }
            let assignment = fig.cell_assignment as usize;
            fig.hypothesizer
                .observe_cells(state, prev_state, firings, |i| i % n_active == assignment);
        }
    }

    /// Advance every active figure's hypothesizer; collect the
    /// `(figure_id, event)` pairs the caller emits to the protocol.
    /// Each figure's fit pipeline runs through cosmology
    /// suppression — dogmatic civs find heretical forms
    /// harder to confirm. The civ's `tool_discovery_rate_bonus`
    /// folds into a `1 + bonus` cadence multiplier so tools that
    /// accelerate the science loop (analytical engines, digital
    /// computation, experiment apparatus) propose and confirm more
    /// often per unit time.
    pub fn step_per_figure(&mut self, tick: u64) -> Vec<(u32, HypothesisEvent)> {
        let mut all = Vec::new();
        let cosmology = self.cosmology;
        let discovery_rate = Real::ONE + self.tool_discovery_rate_bonus();
        for fig in &mut self.figures {
            if fig.retired_tick.is_some() {
                continue;
            }
            // doubt threads into switch_margin scaling —
            // high-doubt figures revise their theories sooner.
            let events = fig.hypothesizer.step_with_cosmology_doubt_and_rate(
                tick,
                &cosmology,
                fig.doubt,
                discovery_rate,
            );
            for ev in events {
                all.push((fig.id, ev));
            }
        }
        all
    }

    /// cosmology drift: apply a push-vector scaled by
    /// `magnitude` (typically figure charisma for hypothesis
    /// events; `1.0` for collapse). Mutates the civ's cosmology
    /// and clamps each axis to `[-1, 1]`.
    pub fn apply_cosmology_push(&mut self, push: &cosmology::Cosmology, magnitude: Real) {
        self.cosmology.push(push, magnitude);
    }

    /// emission gate: returns `true` if the cosmology has
    /// drifted at least `COSMOLOGY_EMIT_THRESHOLD` from the last
    /// emitted snapshot. sim/core checks this each tick after
    /// applying drift.
    pub fn cosmology_should_emit(&self) -> bool {
        let dist = self.cosmology.distance_to(&self.last_emitted_cosmology);
        dist >= Real::from_ratio(
            cosmology::COSMOLOGY_EMIT_THRESHOLD.0,
            cosmology::COSMOLOGY_EMIT_THRESHOLD.1,
        )
    }

    /// Update the last-emitted snapshot after sim/core has emitted
    /// a `CosmologyShifted` event for the current state.
    pub fn note_cosmology_emitted(&mut self) {
        self.last_emitted_cosmology = self.cosmology;
    }

    /// religion drift: apply a push-vector scaled by
    /// `magnitude`. Mutates the civ's religion vector and clamps
    /// each axis to `[-1, 1]`. Mirrors `apply_cosmology_push`.
    pub fn apply_religion_push(&mut self, push: &religion::Religion, magnitude: Real) {
        self.religion.push(push, magnitude);
    }

    /// emission gate: true when religion has drifted at least
    /// `RELIGION_EMIT_THRESHOLD` from the last emitted snapshot.
    pub fn religion_should_emit(&self) -> bool {
        let dist = self.religion.distance_to(&self.last_emitted_religion);
        dist >= Real::from_ratio(
            religion::RELIGION_EMIT_THRESHOLD.0,
            religion::RELIGION_EMIT_THRESHOLD.1,
        )
    }

    /// update the last-emitted religion snapshot after
    /// sim/core has emitted a `ReligionShifted` event.
    pub fn note_religion_emitted(&mut self) {
        self.last_emitted_religion = self.religion;
    }

    /// Filter firings to those the civ can perceive. Union of
    /// the species' baseline perceivable set (sensorium
    /// gating) and any templates the civ has unlocked via tools.
    /// Replaces the M2-era species-only filter once tech
    /// unlocks become possible mid-run.
    pub fn perceivable_firings(
        &self,
        species_baseline: &BTreeSet<u32>,
        firings: &[Firing],
    ) -> Vec<Firing> {
        firings
            .iter()
            .copied()
            .filter(|f| {
                species_baseline.contains(&f.template_id)
                    || self.extra_perceivable_templates.contains(&f.template_id)
            })
            .collect()
    }

    /// Recompute every figure's hypothesizer candidate set
    /// (cross-product over `Channel::ALL`) AND available form
    /// vocabulary ( derivation over perceivable-template tags).
    /// Idempotent. Call at civ founding and after any sensorium-
    /// extending tool unlock that changes the perceivable set.
    pub fn refresh_available_forms(
        &mut self,
        species_baseline: &BTreeSet<u32>,
        recognition_lib: &RecognitionLibrary,
    ) {
        let perceived: BTreeSet<u32> = species_baseline
            .iter()
            .copied()
            .chain(self.extra_perceivable_templates.iter().copied())
            .collect();
        let perceived_vec: Vec<u32> = perceived.iter().copied().collect();
        let tags: Vec<sim_recognition::FormTag> = recognition_lib
            .templates
            .iter()
            .filter(|t| perceived.contains(&t.id))
            .flat_map(|t| t.tags.iter().copied())
            .collect();
        let derived = forms::derive_available_forms(tags.iter().copied());
        for fig in &mut self.figures {
            if fig.retired_tick.is_some() {
                continue;
            }
            fig.hypothesizer.refresh_perceivable(&perceived_vec);
            fig.hypothesizer.set_available_forms(derived.clone());
        }
    }
}
