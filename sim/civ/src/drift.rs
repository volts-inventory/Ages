//! Per-civ species drift. Each successor civ inherits its
//! parent's deltas and adds a small deterministic perturbation;
//! effective trait values fold the delta into the species baseline.

use crate::{Civ, SUCCESSOR_DRIFT_LIFESPAN_STEP_YEARS, SUCCESSOR_DRIFT_TRAIT_STEP};
use sim_arith::Real;

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
