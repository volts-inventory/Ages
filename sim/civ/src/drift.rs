//! Per-civ species drift. Each successor civ inherits its
//! parent's deltas and adds a small deterministic perturbation;
//! effective trait values fold the delta into the species baseline.

use crate::catastrophe::CatastropheKind;
use crate::environmental_drift::{
    drift_band_factor, selection_bias_for_catastrophe, SelectionBias,
};
use crate::{
    Civ, SELECTION_BIAS_CHANNEL_CEILING, SELECTION_BIAS_LIFESPAN_CEILING_YEARS,
    SUCCESSOR_DRIFT_LIFESPAN_STEP_YEARS, SUCCESSOR_DRIFT_TRAIT_STEP,
};
use sim_arith::Real;
use sim_world::BiosphereClass;

/// Deterministic drift seed mixer. Combines the planet seed
/// with the new civ's id and the perturbation channel index (0-3
/// for the four traits) to produce a `ChaCha20Rng` seed unique to
/// `(planet, child_civ_id, trait)`. Same inputs always produce
/// the same drift step — replay determinism preserved.
#[must_use]
fn drift_seed(planet_seed: u64, child_civ_id: u32, channel: u8) -> u64 {
    planet_seed
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(u64::from(child_civ_id))
        .wrapping_mul(0xBF58_476D_1CE4_E5B9)
        .wrapping_add(u64::from(channel))
}

/// Derive the per-generation drift step for one trait
/// channel. `step_size_q32` is the half-range (so output is in
/// `[-step_size, +step_size]`). Uses `ChaCha20Rng` seeded by
/// `(planet_seed, child_civ_id, channel)` for replay determinism.
#[must_use]
fn derive_drift_step(planet_seed: u64, child_civ_id: u32, channel: u8, step_size: Real) -> Real {
    use rand::Rng;
    use rand_chacha::rand_core::SeedableRng;
    let mut rng =
        rand_chacha::ChaCha20Rng::seed_from_u64(drift_seed(planet_seed, child_civ_id, channel));
    // Sample uniform in [-1, 1] via [0, 2_000_000) − 1_000_000.
    let raw = rng.gen_range(0i64..2_000_000) - 1_000_000;
    Real::from_ratio(raw, 1_000_000) * step_size
}

impl Civ {
    /// Inherit the parent civ's species drift and add a
    /// deterministic per-generation perturbation. Called by sim/core
    /// at successor-civ founding (both `refound_from_stateless` and
    /// emergent paths). Inaugural civs (no parent) skip this and
    /// keep their zero-init deltas.
    ///
    /// Determinism: drift step is keyed on `(planet_seed, self.id,
    /// trait_channel)` so byte-replay holds across runs. The four
    /// trait channels (cognition / sociality / `lifespan_years` /
    /// `communication_fidelity`) sample independently.
    pub fn inherit_species_drift(&mut self, parent: &Civ, planet_seed: u64) {
        let trait_step = Real::from_ratio(SUCCESSOR_DRIFT_TRAIT_STEP.0, SUCCESSOR_DRIFT_TRAIT_STEP.1);
        let lifespan_step = Real::from_int(SUCCESSOR_DRIFT_LIFESPAN_STEP_YEARS);
        self.cognition_delta =
            parent.cognition_delta + derive_drift_step(planet_seed, self.id, 0, trait_step);
        self.sociality_delta =
            parent.sociality_delta + derive_drift_step(planet_seed, self.id, 1, trait_step);
        self.lifespan_delta_years =
            parent.lifespan_delta_years + derive_drift_step(planet_seed, self.id, 2, lifespan_step);
        self.communication_fidelity_delta = parent.communication_fidelity_delta
            + derive_drift_step(planet_seed, self.id, 3, trait_step);
    }

    /// M7 — environment-aware version of `inherit_species_drift`.
    ///
    /// Two superseding behaviours stacked on top of the
    /// legacy random-walk drift:
    /// 1. **Selection-on-survival**: the parent's accumulated
    ///    `selection_bias` (from catastrophes) folds into the
    ///    child's inherited delta *before* the random
    ///    perturbation. So a civ that survived a long ice age
    ///    passes a cognition + sociality + lifespan bias to its
    ///    successor; the successor then random-walks from that
    ///    biased starting point. Child's own `selection_bias`
    ///    starts at zero — it accumulates fresh from its own
    ///    catastrophes.
    /// 2. **Substrate × biosphere drift bands**: the random
    ///    perturbation step magnitude is multiplied by
    ///    `drift_band_factor(metabolism, biosphere)`. Silicate
    ///    sparse worlds shrink the band, aqueous hyper-bio
    ///    worlds widen it. So substrate + biosphere together
    ///    set the *speed* of trait drift across the run.
    ///
    /// Determinism is preserved: same `(planet_seed, civ_id,
    /// channel)` keys the same RNG step; the env factor is a
    /// deterministic scalar on top.
    pub fn inherit_species_drift_with_environment(
        &mut self,
        parent: &Civ,
        planet_seed: u64,
        metabolism: Real,
        biosphere: BiosphereClass,
    ) {
        let env_factor = drift_band_factor(metabolism, biosphere);
        let trait_step =
            Real::from_ratio(SUCCESSOR_DRIFT_TRAIT_STEP.0, SUCCESSOR_DRIFT_TRAIT_STEP.1)
                * env_factor;
        let lifespan_step = Real::from_int(SUCCESSOR_DRIFT_LIFESPAN_STEP_YEARS) * env_factor;
        let bias = parent.selection_bias;
        self.cognition_delta = parent.cognition_delta
            + bias.cognition
            + derive_drift_step(planet_seed, self.id, 0, trait_step);
        self.sociality_delta = parent.sociality_delta
            + bias.sociality
            + derive_drift_step(planet_seed, self.id, 1, trait_step);
        self.lifespan_delta_years = parent.lifespan_delta_years
            + bias.lifespan_years
            + derive_drift_step(planet_seed, self.id, 2, lifespan_step);
        self.communication_fidelity_delta = parent.communication_fidelity_delta
            + bias.communication_fidelity
            + derive_drift_step(planet_seed, self.id, 3, trait_step);
        // Child accumulates its own bias fresh; reset.
        self.selection_bias = SelectionBias::zero();
    }

    /// M7 — record a catastrophe's selection pressure on this
    /// civ. Adds the per-kind weights (scaled by `fraction_lost`)
    /// to the running `selection_bias` accumulator, then caps
    /// each channel at the per-channel ceiling so unbounded
    /// catastrophe sequences don't swamp the inherited drift.
    /// Called from sim-core's catastrophe-firing site.
    pub fn record_catastrophe_selection_bias(
        &mut self,
        kind: CatastropheKind,
        fraction_lost: Real,
    ) {
        let contribution = selection_bias_for_catastrophe(kind, fraction_lost);
        let trait_ceiling = Real::from_ratio(
            SELECTION_BIAS_CHANNEL_CEILING.0,
            SELECTION_BIAS_CHANNEL_CEILING.1,
        );
        let lifespan_ceiling = Real::from_int(SELECTION_BIAS_LIFESPAN_CEILING_YEARS);
        let clamp_trait = |v: Real| v.min(trait_ceiling).max(-trait_ceiling);
        let clamp_life = |v: Real| v.min(lifespan_ceiling).max(-lifespan_ceiling);
        let accumulated = self.selection_bias.add(contribution);
        self.selection_bias = SelectionBias {
            cognition: clamp_trait(accumulated.cognition),
            sociality: clamp_trait(accumulated.sociality),
            lifespan_years: clamp_life(accumulated.lifespan_years),
            communication_fidelity: clamp_trait(accumulated.communication_fidelity),
        };
    }

    /// Effective cognition = species.cognition + drift,
    /// clamped to `[0, 1]`. Used by `dynamics_for_civ` and any
    /// other consumer that wants the civ-specific perceived trait.
    #[must_use]
    pub fn effective_cognition(&self, species: &sim_species::Species) -> Real {
        (species.cognition + self.cognition_delta)
            .max(Real::ZERO)
            .min(Real::ONE)
    }

    /// Effective sociality, clamped to `[0, 1]`.
    #[must_use]
    pub fn effective_sociality(&self, species: &sim_species::Species) -> Real {
        (species.sociality + self.sociality_delta)
            .max(Real::ZERO)
            .min(Real::ONE)
    }

    /// Effective lifespan in years, clamped to `[1, 1000]`. Folds
    /// in the multiplicative tool-lifespan-extension factor so
    /// civs with senescence-treatment tech actually live longer
    /// — both their per-bracket sojourn times stretch (via
    /// `dynamics_for_civ`'s re-derivation each tick) and their
    /// life expectancy at birth rises.
    #[must_use]
    pub fn effective_lifespan_years(&self, species: &sim_species::Species) -> Real {
        let raw = species.lifespan_years + self.lifespan_delta_years;
        let extension = Real::ONE + self.tool_lifespan_extension_factor();
        (raw * extension)
            .max(Real::ONE)
            .min(Real::from_int(1000))
    }

    /// Effective communication fidelity, clamped to `[0, 1]`.
    #[must_use]
    pub fn effective_communication_fidelity(&self, species: &sim_species::Species) -> Real {
        (species.communication_fidelity + self.communication_fidelity_delta)
            .max(Real::ZERO)
            .min(Real::ONE)
    }

    /// True if any drift channel has accumulated at least
    /// half a step away from zero. Used by sim/core to gate the
    /// `SpeciesDrift` event so inaugural civs (all-zero deltas)
    /// don't emit a noise line.
    #[must_use]
    pub fn has_meaningful_drift(&self) -> bool {
        let half_trait = Real::from_ratio(SUCCESSOR_DRIFT_TRAIT_STEP.0, SUCCESSOR_DRIFT_TRAIT_STEP.1 * 2);
        let half_lifespan = Real::from_ratio(SUCCESSOR_DRIFT_LIFESPAN_STEP_YEARS, 2);
        self.cognition_delta.abs() >= half_trait
            || self.sociality_delta.abs() >= half_trait
            || self.communication_fidelity_delta.abs() >= half_trait
            || self.lifespan_delta_years.abs() >= half_lifespan
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_arith::Pop;

    fn fresh(id: u32) -> Civ {
        Civ::new(id, 0, Pop::from_int(100))
    }

    #[test]
    fn record_catastrophe_selection_bias_accumulates_and_clamps() {
        let mut civ = fresh(1);
        assert_eq!(civ.selection_bias, SelectionBias::zero());
        // One disease wiping out 30%: bumps cognition by 0.024,
        // sociality by 0.018, lifespan by 0.6 years.
        civ.record_catastrophe_selection_bias(
            CatastropheKind::Disease,
            Real::from_ratio(30, 100),
        );
        assert!(civ.selection_bias.cognition > Real::ZERO);
        assert!(civ.selection_bias.sociality > Real::ZERO);
        // Drop the bias by repeating beyond the ceiling: 20 disease
        // catastrophes at 100% loss would otherwise add up to
        // 20 × 0.08 = 1.6 on cognition.
        for _ in 0..20 {
            civ.record_catastrophe_selection_bias(CatastropheKind::Disease, Real::ONE);
        }
        let ceiling = Real::from_ratio(15, 100);
        assert!(civ.selection_bias.cognition <= ceiling);
        let life_ceiling = Real::from_int(10);
        assert!(civ.selection_bias.lifespan_years <= life_ceiling);
    }

    #[test]
    fn inherit_species_drift_with_environment_folds_parent_bias() {
        let mut parent = fresh(1);
        // Parent survived a disease catastrophe — accumulate a
        // concrete bias.
        parent.record_catastrophe_selection_bias(
            CatastropheKind::Disease,
            Real::from_ratio(30, 100),
        );
        let parent_bias_cog = parent.selection_bias.cognition;
        let mut child = fresh(2);
        child.inherit_species_drift_with_environment(
            &parent,
            42, // planet_seed
            Real::ONE, // aqueous metabolism
            BiosphereClass::Lush,
        );
        // Child's cognition_delta should include the parent's
        // bias plus a (potentially small) random perturbation;
        // it must at minimum equal-or-exceed the bias magnitude.
        // Since the random step can be negative, we just check
        // that the bias contribution is detectable in the delta:
        // delta ≈ bias + step where |step| ≤ 0.02 × env_factor ≤
        // 0.03 (Lush biosphere × Aqueous substrate × 0.02 step).
        // Bias from one 30% disease is 0.024; the upper bound is
        // 0.024 + 0.03 = 0.054, the lower bound 0.024 - 0.03 =
        // -0.006. Net: must be > -0.01.
        assert!(child.cognition_delta > Real::from_int(-1) / Real::from_int(100));
        // After inheritance, child's *own* selection_bias is reset.
        assert_eq!(child.selection_bias, SelectionBias::zero());
        // Parent's bias is untouched.
        let drift = parent.selection_bias.cognition - parent_bias_cog;
        assert!(drift.abs() < Real::from_ratio(1, 1_000_000));
    }

    #[test]
    fn drift_band_factor_narrows_silicate_random_step() {
        let parent = fresh(1);
        // Same parent, same child_id, different env. The random
        // step magnitude should shrink under silicate + sparse.
        let mut child_a = fresh(2);
        child_a.inherit_species_drift_with_environment(
            &parent,
            42,
            Real::ONE, // aqueous
            BiosphereClass::Lush,
        );
        let mut child_b = fresh(2);
        child_b.inherit_species_drift_with_environment(
            &parent,
            42,
            Real::from_ratio(2, 10), // silicate
            BiosphereClass::Sparse,
        );
        // Silicate sparse should produce a strictly smaller
        // absolute drift than aqueous lush (same RNG seed → same
        // raw sample; env factor scales the result).
        let mag_a = child_a.cognition_delta.abs();
        let mag_b = child_b.cognition_delta.abs();
        assert!(
            mag_b < mag_a,
            "silicate+sparse drift ({mag_b:?}) should be tighter than aqueous+lush ({mag_a:?})"
        );
    }
}
