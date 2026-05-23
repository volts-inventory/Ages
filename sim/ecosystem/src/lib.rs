//! `sim-ecosystem` — multi-species ecosystem layer (Sprint 2 Item 6).
//!
//! Builds a per-planet biota of 8-20 typed species, a typed
//! pairwise `InteractionMatrix`, and runs the per-tick step that
//! couples them. Three pieces:
//!
//! 1. **Sampling** (`sample_ecosystem`) — draws the role
//!    distribution required by the spec (≥2 Producers, ≥3
//!    PrimaryConsumers, etc.), assigns a starting biomass that
//!    already respects the Lindeman 10:1 pyramid, and wires up a
//!    deterministic interaction matrix (predation along the
//!    tier ladder, competition within tier, parasitism for the
//!    parasite cohort, etc.).
//! 2. **Step** (`PlanetEcosystem::step`) — for each pair in the
//!    matrix, computes a per-species biomass delta via the pair's
//!    `FunctionalResponse` (Linear: `s × prey`; Saturating Type-II:
//!    `s × prey / (k + prey)`; Sigmoidal Type-III: `s × prey² /
//!    (k² + prey²)`). Predation/parasitism assimilates the gross
//!    flux through a per-habitat Lindeman efficiency
//!    ([`lindeman_assimilation_for_habitat`]: 30:1 aquatic, 10:1
//!    terrestrial, 6.7:1 amphibious/airborne) — the pyramid emerges
//!    from this single calibrated ratio, *not* from a post-step
//!    corrective cap. Tests + debug callers can assert
//!    "no Lindeman runaway" via
//!    [`PlanetEcosystem::check_lindeman_invariant`].
//! 3. **Keystone detection** (`PlanetEcosystem::keystone_species`)
//!    — computes betweenness centrality on the interaction graph
//!    (treated undirected for centrality) and returns species whose
//!    centrality exceeds the configured threshold.
//!
//! **Determinism**: every collection iterated by the step is a
//! `BTreeMap` or `BTreeSet`, never `HashMap`. Half-saturation
//! is per-pair (Sprint 2 Item P2.6) — each `Interaction` carries
//! its calibrated fraction of producer capacity (wolf-deer apex
//! 0.10, lynx-hare specialist 0.30, habitat engineers 0.20,
//! generic mutualism 0.50) so the canonical Lotka-Volterra cycle
//! periods match published values across pair types. Back-compat
//! literals with `half_saturation = Real::ZERO` fall through to
//! the legacy 0.5× default via `K_HALF_SAT_DEFAULT`.
//!
//! ## Module layout (CA2 split)
//!
//! - [`constants`] — calibrated rates, thresholds, fractions.
//! - [`functional`] — Holling Type I/II/III evaluators.
//! - [`species`] — `EcoSpecies` per-planet record.
//! - [`planet`] — `PlanetEcosystem` state + per-tick step.
//! - [`invariants`] — Lindeman pyramid invariants + per-habitat
//!   assimilation efficiency.
//! - [`sampling`] — seed-driven `sample_*` builders + canonical
//!   interaction wiring.
//! - [`hgt`] — horizontal gene transfer (Sprint 2 Item 12).
//! - [`speciation`] — speciation triggers + daughter-species
//!   derivation (Sprint 3).

#![allow(clippy::module_name_repetitions)]

pub mod constants;
pub mod functional;
pub mod hgt;
pub mod invariants;
pub mod planet;
pub mod sampling;
pub mod species;
pub mod speciation;

// ── Constants: every calibrated rate / threshold the per-tick step
//    reads from. Re-exported flat so callers can keep using
//    `sim_ecosystem::PRODUCER_GROWTH_RATE` without the `constants::`
//    prefix.
pub use constants::{
    CHEMOAUTOTROPH_GROWTH_RATE, CONSUMER_DECAY_RATE, DECOMPOSITION_RATE, ENGINEER_MATCH_BOOST,
    EXTINCTION_CONFIRMATION_TICKS, EXTINCTION_THRESHOLD_FRAC, HALF_SAT_APEX_PREDATOR,
    HALF_SAT_HABITAT_MOD, HALF_SAT_MUTUALISM, HALF_SAT_SPECIALIST_PREDATOR, K_HALF_SAT_DEFAULT,
    KEYSTONE_CENTRALITY_THRESHOLD, LINDEMAN_OVERSHOOT_DEBUG_MAX, LINDEMAN_RATIO,
    MACRO_FERTILITY_MULTIPLIER, MICRO_CROWDING_THRESHOLD, MICRO_SURVIVAL_PENALTY,
    POLLINATOR_BIOMASS_COUPLING, PRODUCER_GROWTH_RATE, RESPIRATION_RATE,
    SEED_DISPERSER_BIOMASS_THRESHOLD, SEED_DISPERSER_RANGE_BOOST, SYNTROPHY_COLLAPSE_RATE,
    SYNTROPHY_MIN_PARTNER_BIOMASS, VIRUS_OUTBREAK_HOST_LOSS, VIRUS_OUTBREAK_PERIOD,
};

pub use functional::functional_response;
pub use invariants::{lindeman_assimilation_for_habitat, LindemanViolation};
pub use planet::{virus_outbreak_hash, PlanetEcosystem};
pub use sampling::{
    habitat_for_substrate, sample_ecosystem, sample_ecosystem_with_substrate,
    sample_ecosystem_with_substrate_for_grid,
};
pub use species::EcoSpecies;

pub use hgt::{step_hgt, LocalConditions, HGT_BASE_RATE, SWEEP_THRESHOLD};
pub use speciation::{
    apply_character_displacement, clamp_cosmic_ray_multiplier, daughter_eco_role,
    derive_daughter_species, divergence_pull, next_species_id, polyploid_check, step_speciation,
    SpeciationTracker, SpeciationTrigger, ALLOPATRIC_ISOLATION_TICKS,
    COSMIC_RAY_MULTIPLIER_CEILING, COSMIC_RAY_MULTIPLIER_FLOOR, FOUNDER_BIOMASS_FRAC,
    INHERITED_INTERACTION_STRENGTH_FRAC, POLYPLOID_PER_TICK_PROB_RECIP,
    POST_EXTINCTION_BOOST_TICKS, POST_EXTINCTION_RADIATION_MULTIPLIER,
    RADIATION_DISPLACEMENT_REFERENCE, SISTER_COMPETITION_STRENGTH,
    SYMPATRIC_COMPETITION_BIOMASS_FRAC, SYMPATRIC_PRESSURE_TICKS,
    TEMPERATURE_DISPLACEMENT_REFERENCE_K, TOLERANCE_DISPLACEMENT_FRAC,
};

#[cfg(test)]
mod tests;
