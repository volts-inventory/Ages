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
use sim_arith::{Pop, Real};
use sim_population::LifecycleState;
use sim_species::{CasteRole, Lifecycle};
use sim_world::BiosphereClass;

/// Quorum population below which a `CognitionTopology::Collective`
/// species suffers the isolation penalty on its effective
/// cognition. Tuned so a typical founding cohort
/// (~100-1000 individuals) stays above quorum while a deeply
/// collapsed civ (sub-100) collapses to near-zero cognition —
/// matching the spec's "single individuals cannot think" framing.
pub const COLLECTIVE_QUORUM_POP: Pop = Pop::from_int(100);

/// P3.4 — per-caste minimum population fractions for a
/// `Lifecycle::Eusocial` Collective civ. A colony of 10 000
/// queens with 0 workers reads "above quorum" under a single
/// total-head-count check but is functionally extinct; real
/// eusocial colonies need each caste to clear its own minimum.
///
/// Fractions are expressed as `(numerator, denominator)` so the
/// per-caste threshold = `COLLECTIVE_QUORUM_POP × num / den`.
/// Defaults follow the xeno spec:
///
/// - `Reproductive`:  1 % of total quorum (queens / drones — a
///   small fraction by definition).
/// - `Worker`:       50 % of total quorum (the load-bearing caste
///   in a healthy colony).
/// - `Soldier`:      10 % of total quorum (defensive caste; absent
///   from peaceful queen-only colonies but required for collective
///   cognition under the spec).
/// - `Nurse`:        10 % of total quorum (rears young; cognition
///   in real colonies degrades sharply when nurses fail to tend
///   the brood).
///
/// A colony is above quorum only when **all four** caste minimums
/// are met simultaneously. Falling below on any single caste
/// trips the isolation penalty exactly like a total-head-count
/// collapse — failure in any limb of the colony degrades the
/// hive-mind.
pub const CASTE_QUORUM_REPRODUCTIVE_NUM: i64 = 1;
pub const CASTE_QUORUM_REPRODUCTIVE_DEN: i64 = 100;
pub const CASTE_QUORUM_WORKER_NUM: i64 = 50;
pub const CASTE_QUORUM_WORKER_DEN: i64 = 100;
pub const CASTE_QUORUM_SOLDIER_NUM: i64 = 10;
pub const CASTE_QUORUM_SOLDIER_DEN: i64 = 100;
pub const CASTE_QUORUM_NURSE_NUM: i64 = 10;
pub const CASTE_QUORUM_NURSE_DEN: i64 = 100;

/// Per-caste population minimum derived from
/// `COLLECTIVE_QUORUM_POP` and the per-caste fraction constants.
/// `Pop::saturating_mul_real` keeps this in Q32.32 and bounded.
#[must_use]
fn caste_minimum(role: CasteRole) -> Pop {
    let (num, den) = match role {
        CasteRole::Reproductive => {
            (CASTE_QUORUM_REPRODUCTIVE_NUM, CASTE_QUORUM_REPRODUCTIVE_DEN)
        }
        CasteRole::Worker => (CASTE_QUORUM_WORKER_NUM, CASTE_QUORUM_WORKER_DEN),
        CasteRole::Soldier => (CASTE_QUORUM_SOLDIER_NUM, CASTE_QUORUM_SOLDIER_DEN),
        CasteRole::Nurse => (CASTE_QUORUM_NURSE_NUM, CASTE_QUORUM_NURSE_DEN),
    };
    COLLECTIVE_QUORUM_POP.saturating_mul_real(Real::from_ratio(num, den))
}

/// P3.4 — caste-aware Collective quorum check. Returns `true`
/// iff the civ's population is *functionally* above the
/// Collective swarm-quorum.
///
/// Dispatch:
/// - For `Lifecycle::Eusocial` species with an `Eusocial`
///   `lifecycle_state`, each of the four canonical castes
///   (Reproductive / Worker / Soldier / Nurse) must clear its
///   own per-caste minimum from `caste_minimum`. A single caste
///   below threshold fails the colony — 10 000 queens with 0
///   workers is functionally extinct.
/// - For every other lifecycle (Vertebrate, Microbial,
///   Modular, …) or when the `lifecycle_state` doesn't match
///   the declared lifecycle, fall back to the legacy
///   total-cohort check against `COLLECTIVE_QUORUM_POP`.
///
/// The function is `pub` so external callers (sim/core,
/// debug-dumpers) can query the caste-aware quorum directly
/// rather than re-implementing the gate.
#[must_use]
pub fn meets_collective_quorum(civ: &Civ, lifecycle: &Lifecycle) -> bool {
    if let (Lifecycle::Eusocial { .. }, LifecycleState::Eusocial(colony)) =
        (lifecycle, &civ.lifecycle_state)
    {
        // Every canonical caste must clear its per-caste minimum.
        // Castes absent from the colony's roster read as zero
        // headcount (`EusocialColony::caste`) — so a colony that
        // forgot to seed Soldiers will fail Soldier minimum and
        // drop to isolation_penalty.
        let castes = [
            CasteRole::Reproductive,
            CasteRole::Worker,
            CasteRole::Soldier,
            CasteRole::Nurse,
        ];
        castes
            .iter()
            .all(|role| colony.caste(*role) >= caste_minimum(*role))
    } else {
        // Legacy total-head-count check for non-Eusocial Collective
        // species (microbial swarms, modular colonies that map
        // onto a single biomass, etc.) and for partially-wired
        // call sites that haven't configured a matching
        // `LifecycleState` yet.
        civ.cohort.total() >= COLLECTIVE_QUORUM_POP
    }
}

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
        let trait_step = Real::from(SUCCESSOR_DRIFT_TRAIT_STEP);
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
        let trait_step = Real::from(SUCCESSOR_DRIFT_TRAIT_STEP) * env_factor;
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
    ///
    /// For `CognitionTopology::Collective` species, the cognition
    /// is multiplied by `isolation_penalty()` (~0.05) when the
    /// civ fails the caste-aware quorum check (see
    /// `meets_collective_quorum`):
    ///
    /// - `Lifecycle::Eusocial` colonies require **all four**
    ///   canonical castes (Reproductive, Worker, Soldier, Nurse)
    ///   to clear their per-caste minimums. A colony of 10 000
    ///   queens with 0 workers is functionally extinct and
    ///   collapses to the isolation penalty even though the total
    ///   head-count is huge.
    /// - Non-Eusocial Collective species (microbial swarms,
    ///   modular colonies) fall back to the legacy
    ///   `total >= COLLECTIVE_QUORUM_POP` gate.
    ///
    /// Other cognition topologies are unaffected by the gate.
    #[must_use]
    pub fn effective_cognition(&self, species: &sim_species::Species) -> Real {
        let base = (species.cognition + self.cognition_delta).clamp01();
        if matches!(
            species.cognition_topology,
            sim_species::CognitionTopology::Collective
        ) && !meets_collective_quorum(self, &species.lifecycle)
        {
            (base * species.cognition_topology.isolation_penalty()).clamp01()
        } else {
            base
        }
    }

    /// Effective sociality, clamped to `[0, 1]`.
    #[must_use]
    pub fn effective_sociality(&self, species: &sim_species::Species) -> Real {
        (species.sociality + self.sociality_delta).clamp01()
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
        (raw * extension).max(Real::ONE).min(Real::from_int(1000))
    }

    /// Effective communication fidelity, clamped to `[0, 1]`.
    #[must_use]
    pub fn effective_communication_fidelity(&self, species: &sim_species::Species) -> Real {
        (species.communication_fidelity + self.communication_fidelity_delta).clamp01()
    }

    /// True if any drift channel has accumulated at least
    /// half a step away from zero. Used by sim/core to gate the
    /// `SpeciesDrift` event so inaugural civs (all-zero deltas)
    /// don't emit a noise line.
    #[must_use]
    pub fn has_meaningful_drift(&self) -> bool {
        let half_trait = Real::from_ratio(
            SUCCESSOR_DRIFT_TRAIT_STEP.0,
            SUCCESSOR_DRIFT_TRAIT_STEP.1 * 2,
        );
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
        civ.record_catastrophe_selection_bias(CatastropheKind::Disease, Real::percent(30));
        assert!(civ.selection_bias.cognition > Real::ZERO);
        assert!(civ.selection_bias.sociality > Real::ZERO);
        // Drop the bias by repeating beyond the ceiling: 20 disease
        // catastrophes at 100% loss would otherwise add up to
        // 20 × 0.08 = 1.6 on cognition.
        for _ in 0..20 {
            civ.record_catastrophe_selection_bias(CatastropheKind::Disease, Real::ONE);
        }
        let ceiling = Real::percent(15);
        assert!(civ.selection_bias.cognition <= ceiling);
        let life_ceiling = Real::from_int(10);
        assert!(civ.selection_bias.lifespan_years <= life_ceiling);
    }

    #[test]
    fn inherit_species_drift_with_environment_folds_parent_bias() {
        let mut parent = fresh(1);
        // Parent survived a disease catastrophe — accumulate a
        // concrete bias.
        parent.record_catastrophe_selection_bias(CatastropheKind::Disease, Real::percent(30));
        let parent_bias_cog = parent.selection_bias.cognition;
        let mut child = fresh(2);
        child.inherit_species_drift_with_environment(
            &parent,
            42,        // planet_seed
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

    // ---- P3.4: caste-aware Collective quorum -----------------------

    /// Build a `Lifecycle::Eusocial { castes }` with all four
    /// canonical castes declared. The caller seeds per-caste
    /// headcount on the returned colony state.
    fn eusocial_lifecycle() -> Lifecycle {
        Lifecycle::Eusocial {
            castes: vec![
                CasteRole::Reproductive,
                CasteRole::Worker,
                CasteRole::Soldier,
                CasteRole::Nurse,
            ],
        }
    }

    /// Apply a per-caste seed (caste, headcount) directly to the
    /// civ's `LifecycleState::Eusocial` colony. Panics if the
    /// civ's lifecycle_state isn't already configured for
    /// Eusocial — call `configure_lifecycle_state` first.
    fn seed_castes(civ: &mut Civ, seed: &[(CasteRole, Pop)]) {
        let colony = civ
            .lifecycle_state
            .eusocial_mut()
            .expect("expected Eusocial lifecycle_state");
        for (role, n) in seed {
            colony.castes.insert(*role, *n);
        }
    }

    /// Build a `Collective` Eusocial species via the standard
    /// derivation path, then override the cognition + topology +
    /// lifecycle fields so the gate's all four conditions are
    /// observable from a deterministic baseline.
    fn collective_eusocial_species() -> sim_species::Species {
        use sim_recognition::RecognitionLibrary;
        use sim_world::sample_planet;
        let planet = sample_planet(1);
        let lib = RecognitionLibrary::earth_like_default();
        let mut species = sim_species::derive(&planet, &lib);
        species.cognition_topology = sim_species::CognitionTopology::Collective;
        species.cognition = Real::from_ratio(8, 10); // 0.8 base
        species.lifecycle = eusocial_lifecycle();
        species
    }

    /// P3.4 — a colony with 10 000 Reproductives but **zero**
    /// Workers fails the caste-aware quorum gate. Total
    /// head-count is enormous; under the legacy single-threshold
    /// check this would read "above quorum." Under the
    /// caste-aware check, missing Workers — the load-bearing
    /// caste — collapses the colony's effective cognition to the
    /// `isolation_penalty` (~5 % of base).
    #[test]
    fn collective_civ_with_no_workers_fails_quorum() {
        let species = collective_eusocial_species();

        let mut civ = Civ::new(1, 0, Pop::from_int(10_000));
        civ.configure_lifecycle_state(&species.lifecycle);
        // Reproductive=10 000 (massively above its 1 % minimum
        // of 1), Worker=0, Soldier+Nurse seeded above-min so the
        // *only* failing limb is the Worker caste — proves the
        // ALL-castes-must-pass logic.
        seed_castes(
            &mut civ,
            &[
                (CasteRole::Reproductive, Pop::from_int(10_000)),
                (CasteRole::Worker, Pop::ZERO),
                (CasteRole::Soldier, Pop::from_int(20)),
                (CasteRole::Nurse, Pop::from_int(20)),
            ],
        );

        // The caste-aware quorum gate must reject this colony.
        assert!(
            !meets_collective_quorum(&civ, &species.lifecycle),
            "colony with 0 Workers must fail caste-aware quorum"
        );
        // ...and the cognition value lands at the isolation
        // penalty multiplier.
        let cog = civ.effective_cognition(&species);
        let expected = Real::from_ratio(8, 10) * Real::from_ratio(5, 100);
        assert_eq!(
            cog, expected,
            "no-Worker colony must drop to isolation_penalty cognition"
        );
    }

    /// P3.4 — a colony with all four castes seeded above their
    /// per-caste minimums (Reproductive=10, Worker=60, Soldier=15,
    /// Nurse=15 against minimums of 1, 50, 10, 10) passes the
    /// caste-aware quorum and runs at full base cognition.
    #[test]
    fn collective_civ_with_balanced_castes_passes_quorum() {
        let species = collective_eusocial_species();

        let mut civ = Civ::new(1, 0, Pop::from_int(100));
        civ.configure_lifecycle_state(&species.lifecycle);
        // Per-caste minimums (against COLLECTIVE_QUORUM_POP=100):
        //   Reproductive:  1, Worker: 50, Soldier: 10, Nurse: 10.
        // Seed strictly above each.
        seed_castes(
            &mut civ,
            &[
                (CasteRole::Reproductive, Pop::from_int(10)),
                (CasteRole::Worker, Pop::from_int(60)),
                (CasteRole::Soldier, Pop::from_int(15)),
                (CasteRole::Nurse, Pop::from_int(15)),
            ],
        );

        assert!(
            meets_collective_quorum(&civ, &species.lifecycle),
            "balanced colony must pass caste-aware quorum"
        );
        // No isolation penalty applied — cognition equals the
        // unmodified species baseline.
        let cog = civ.effective_cognition(&species);
        assert_eq!(
            cog,
            Real::from_ratio(8, 10),
            "balanced colony must read full base cognition"
        );
    }

    /// P3.4 — pinning the "Worker below 50 % minimum" branch
    /// directly. Start a colony above quorum with Worker=60
    /// (above the 50 minimum), then drop Worker to 40 (below the
    /// 50 minimum) leaving every other caste untouched. The
    /// quorum check must flip from pass → fail and effective
    /// cognition must demote to isolation_penalty even though
    /// Reproductive / Soldier / Nurse are unchanged.
    #[test]
    fn worker_caste_underflow_demotes_cognition() {
        let species = collective_eusocial_species();

        let mut civ = Civ::new(1, 0, Pop::from_int(100));
        civ.configure_lifecycle_state(&species.lifecycle);
        seed_castes(
            &mut civ,
            &[
                (CasteRole::Reproductive, Pop::from_int(10)),
                (CasteRole::Worker, Pop::from_int(60)),
                (CasteRole::Soldier, Pop::from_int(15)),
                (CasteRole::Nurse, Pop::from_int(15)),
            ],
        );
        // Sanity: above quorum, full base cognition.
        assert!(meets_collective_quorum(&civ, &species.lifecycle));
        let base_cog = civ.effective_cognition(&species);
        assert_eq!(base_cog, Real::from_ratio(8, 10));

        // Drop Worker below its 50 % minimum (50 / 100 = 50).
        // Worker = 40 is below the threshold; every other caste
        // is still above its own minimum.
        seed_castes(&mut civ, &[(CasteRole::Worker, Pop::from_int(40))]);

        assert!(
            !meets_collective_quorum(&civ, &species.lifecycle),
            "Worker below 50 % minimum must fail caste-aware quorum"
        );
        let demoted = civ.effective_cognition(&species);
        let expected = Real::from_ratio(8, 10) * Real::from_ratio(5, 100);
        assert_eq!(
            demoted, expected,
            "Worker underflow must demote cognition to isolation_penalty"
        );
        assert!(demoted < base_cog);
    }

    /// Defence-in-depth: every single caste underflow (not just
    /// Worker) must fail the quorum. Mirrors the spec's
    /// "ALL of (Reproductive, Worker, Soldier, Nurse)" clause.
    #[test]
    fn any_single_caste_underflow_fails_quorum() {
        let species = collective_eusocial_species();
        let base = |civ: &mut Civ| {
            civ.configure_lifecycle_state(&species.lifecycle);
            seed_castes(
                civ,
                &[
                    (CasteRole::Reproductive, Pop::from_int(10)),
                    (CasteRole::Worker, Pop::from_int(60)),
                    (CasteRole::Soldier, Pop::from_int(15)),
                    (CasteRole::Nurse, Pop::from_int(15)),
                ],
            );
        };

        // Reproductive at zero.
        let mut c = Civ::new(1, 0, Pop::from_int(100));
        base(&mut c);
        seed_castes(&mut c, &[(CasteRole::Reproductive, Pop::ZERO)]);
        assert!(!meets_collective_quorum(&c, &species.lifecycle));

        // Soldier at zero.
        let mut c = Civ::new(2, 0, Pop::from_int(100));
        base(&mut c);
        seed_castes(&mut c, &[(CasteRole::Soldier, Pop::ZERO)]);
        assert!(!meets_collective_quorum(&c, &species.lifecycle));

        // Nurse at zero.
        let mut c = Civ::new(3, 0, Pop::from_int(100));
        base(&mut c);
        seed_castes(&mut c, &[(CasteRole::Nurse, Pop::ZERO)]);
        assert!(!meets_collective_quorum(&c, &species.lifecycle));
    }

    /// Non-Eusocial Collective species (Vertebrate / Microbial /
    /// Modular) fall back to the legacy total-head-count check
    /// against `COLLECTIVE_QUORUM_POP`. This pins that path so
    /// the caste-aware logic doesn't accidentally regress
    /// non-eusocial Collective hive minds.
    #[test]
    fn non_eusocial_collective_uses_legacy_total_quorum() {
        use sim_recognition::RecognitionLibrary;
        use sim_world::sample_planet;
        let planet = sample_planet(1);
        let lib = RecognitionLibrary::earth_like_default();
        let mut species = sim_species::derive(&planet, &lib);
        species.cognition_topology = sim_species::CognitionTopology::Collective;
        species.cognition = Real::from_ratio(8, 10);
        species.lifecycle = Lifecycle::Vertebrate;

        // Above threshold: passes.
        let above = Civ::new(1, 0, Pop::from_int(500));
        assert!(meets_collective_quorum(&above, &species.lifecycle));
        // Below threshold: fails.
        let below = Civ::new(2, 0, Pop::from_int(10));
        assert!(!meets_collective_quorum(&below, &species.lifecycle));
    }
}
