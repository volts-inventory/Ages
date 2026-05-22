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

#![allow(clippy::module_name_repetitions)]

pub mod hgt;
pub use hgt::{step_hgt, HGT_BASE_RATE, HGT_INTERPOLATION};

use protocol::{ExtinctionCause, SpeciesExtinct};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use sim_arith::Real;
use sim_physics::chemistry::{oxidiser_ladder, partition_chemoautotroph_growth, Oxidiser};
use sim_physics::{PhysicsState, Substance};
use sim_species::{
    EcosystemRole, FunctionalResponse, Habitat, Interaction, InteractionKind, InteractionMatrix,
    MutualismKind, ParasiteKind, ProducerMetabolism, SpeciesId,
};
use std::collections::{BTreeMap, BTreeSet};

pub mod speciation;
pub use speciation::{
    clamp_cosmic_ray_multiplier, daughter_eco_role, derive_daughter_species, divergence_pull,
    next_species_id, polyploid_check, step_speciation, SpeciationTracker, SpeciationTrigger,
    ALLOPATRIC_ISOLATION_TICKS, COSMIC_RAY_MULTIPLIER_CEILING, COSMIC_RAY_MULTIPLIER_FLOOR,
    FOUNDER_BIOMASS_FRAC, POLYPLOID_PER_TICK_PROB_RECIP, POST_EXTINCTION_BOOST_TICKS,
    POST_EXTINCTION_RADIATION_MULTIPLIER, SYMPATRIC_COMPETITION_BIOMASS_FRAC,
    SYMPATRIC_PRESSURE_TICKS,
};

/// Default Lindeman 10:1 assimilation ratio — back-compat fallback
/// used for fixtures that don't pin a habitat. The canonical
/// terrestrial value; per-habitat overrides live in
/// [`lindeman_assimilation_for_habitat`].
///
/// **Note (P2.5):** the ecosystem step used to *both* assimilate at
/// 10% during predation *and* run a post-step `enforce_lindeman_pyramid`
/// scaling pass that re-clamped each tier to ≤ 0.1× the lower tier.
/// That was double-bookkeeping: a calibrated assimilation efficiency
/// is the *physical* mechanism that produces the pyramid at
/// steady state, so the post-step cap is redundant. The cap is gone;
/// the per-habitat assimilation ratio carries the whole load.
pub const LINDEMAN_RATIO: (i64, i64) = (1, 10);

/// Per-habitat assimilation efficiency (P2.5).
#[must_use]
pub fn lindeman_assimilation_for_habitat(habitat: Habitat) -> Real {
    match habitat {
        Habitat::Aquatic => Real::from_ratio(1, 30),
        Habitat::Terrestrial | Habitat::Subterranean | Habitat::Endolithic => {
            Real::from_ratio(1, 10)
        }
        Habitat::Amphibious | Habitat::Airborne => Real::from_ratio(15, 100),
    }
}

pub const LINDEMAN_OVERSHOOT_DEBUG_MAX: i64 = 5;

/// Half-saturation default (P2.6 — was K_HALF_SAT; renamed _DEFAULT
/// since per-pair `Interaction::half_saturation` is now the production
/// path). Consumers only reach this when `half_saturation = ZERO`.
pub const K_HALF_SAT_DEFAULT: (i64, i64) = (1, 2);

/// Canonical per-pair half-saturation fractions (P2.6).
pub const HALF_SAT_APEX_PREDATOR: (i64, i64) = (1, 10);
pub const HALF_SAT_SPECIALIST_PREDATOR: (i64, i64) = (3, 10);
pub const HALF_SAT_MUTUALISM: (i64, i64) = (5, 10);
pub const HALF_SAT_HABITAT_MOD: (i64, i64) = (2, 10);

/// Per-tick base growth rate for producers (fraction of carrying
/// capacity). The producer pool drifts toward
/// `producer_capacity` at this fraction per tick when not grazed.
pub const PRODUCER_GROWTH_RATE: (i64, i64) = (2, 100);

/// Per-tick passive mortality for any non-producer species. Without
/// this, predator pools never decay between feedings and oscillations
/// collapse into monotonic ramps.
pub const CONSUMER_DECAY_RATE: (i64, i64) = (1, 100);

/// Betweenness-centrality threshold above which a species is flagged
/// as a keystone. Tuned for the 8-20 species per-planet target where
/// the producer hubs naturally accumulate centrality of order
/// `n_species × n_consumers`. Expressed as a fraction of the maximum
/// possible centrality (n × (n-1)).
pub const KEYSTONE_CENTRALITY_THRESHOLD: (i64, i64) = (15, 100);

/// Syntrophy partner-biomass floor (Sprint 2 Item 9). Mutualism
/// pairs whose smaller partner falls below this absolute biomass
/// drag *both* sides toward extinction at
/// `SYNTROPHY_COLLAPSE_RATE` per tick. The floor is calibrated as a
/// small absolute number rather than as a fraction of capacity so a
/// pair with biomass `(1, 0.01)` reads "the 0.01 side is below the
/// floor → the pair collapses" regardless of the producer pool size.
pub const SYNTROPHY_MIN_PARTNER_BIOMASS: (i64, i64) = (1, 100);

/// Per-tick fractional collapse applied to *both* sides of a
/// Mutualism pair when one partner falls below
/// `SYNTROPHY_MIN_PARTNER_BIOMASS`. 25% per tick is fast enough that
/// the test's "within a few ticks" assertion holds, and slow enough
/// that a transient dip below the floor (e.g. due to a single
/// catastrophic predation event) doesn't trip the cascade on a single
/// tick.
pub const SYNTROPHY_COLLAPSE_RATE: (i64, i64) = (25, 100);

/// Per-Chemoautotroph-species growth-demand baseline used by
/// `partition_chemoautotrophs`. A Chemoautotroph wants to add up to
/// this fraction of the producer carrying capacity per tick, scaled
/// by its current biomass / capacity ratio so empty pools fill fast
/// and saturated pools coast. Identical in shape to
/// `PRODUCER_GROWTH_RATE` (which drives Photoautotrophs) but routed
/// through `oxidiser_ladder` so the per-tick growth is also capped
/// by oxidiser availability — a chemolithotroph on a CO2-poor
/// hydrocarbon world can't grow even if biomass demand says it
/// should.
pub const CHEMOAUTOTROPH_GROWTH_RATE: (i64, i64) = (2, 100);

/// Biomass floor below which a species is considered to be
/// collapsing. Expressed as a fraction of the planet's
/// `producer_capacity` so the threshold scales with planet size:
/// `0.001 × capacity`. Sprint 2 Item 6a — paired with
/// `EXTINCTION_CONFIRMATION_TICKS` so a single bad tick can't kill
/// a species, but a sustained collapse does.
pub const EXTINCTION_THRESHOLD_FRAC: (i64, i64) = (1, 1000);

/// Number of consecutive ticks the per-species biomass must sit
/// below `EXTINCTION_THRESHOLD_FRAC × producer_capacity` before the
/// species is flagged extinct. `12` on monthly cadence ≈ one
/// sim-year — long enough that a single seasonal trough doesn't
/// trigger extinction, short enough that an actual collapse converts
/// to an extinction event within the run.
pub const EXTINCTION_CONFIRMATION_TICKS: u64 = 12;

/// Per-tick consumer respiration rate — fraction of consumer biomass
/// returned to atmospheric `CO2` each tick (Sprint 2 Item 6b). 1%/tick.
///
/// Mirror of the carbon side of the biogeochem loop: every consumer
/// (PrimaryConsumer, SecondaryConsumer, ApexConsumer, Detritivore,
/// Saprotroph, Mutualist, Parasite) respires a small fraction of its
/// biomass back to atmospheric `CO2` each tick. Producers don't
/// respire here — they're net carbon sinks (photosynthesis /
/// chemosynthesis) over the daily-averaged tick budget.
pub const RESPIRATION_RATE: (i64, i64) = (1, 100);

/// Per-tick decomposition rate — fraction of all extant species'
/// biomass that decomposers (Detritivore + Saprotroph) liberate to
/// atmospheric `CO2` each tick (Sprint 2 Item 6b). 0.5%/tick.
///
/// This represents the dead-biomass channel: at any given moment a
/// small fraction of every species' standing biomass is dead matter
/// being broken down. The decomposer chain returns that carbon to
/// the atmosphere. Drawn from total biomass (producers included);
/// rate is gated on the presence of at least one Detritivore or
/// Saprotroph.
pub const DECOMPOSITION_RATE: (i64, i64) = (1, 200);

/// A Lindeman pyramid invariant violation reported by
/// [`PlanetEcosystem::check_lindeman_invariant`] (P2.5). Names the
/// upper tier whose biomass blew past
/// `LINDEMAN_OVERSHOOT_DEBUG_MAX × per-habitat-ratio × lower-tier
/// biomass`. Returned (not panicked) so tests can decide what to do
/// with it; the production step loop doesn't check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LindemanViolation {
    /// Tier index (1 = primary, 2 = secondary, 3 = apex) that
    /// overshot.
    pub upper_tier: u8,
    /// Biomass total of the offending upper tier.
    pub upper_biomass: Real,
    /// Biomass total of the lower tier feeding it.
    pub lower_biomass: Real,
    /// The maximum-allowed ratio
    /// (`LINDEMAN_OVERSHOOT_DEBUG_MAX × max_assimilation_ratio`) the
    /// upper-over-lower ratio crossed.
    pub allowed_slack: Real,
}

/// One species' per-planet record. `species_id` is the dense per-
/// planet index; `biomass` is the live pool (always ≥ 0); the
/// other fields are configuration carried for the step.
#[derive(Debug, Clone, Copy)]
pub struct EcoSpecies {
    pub species_id: SpeciesId,
    pub role: EcosystemRole,
    /// Live biomass pool. Same units as `producer_capacity`.
    pub biomass: Real,
    /// True iff the species can still participate in the per-tick
    /// step. Extinction (Item 6a) flips this off without removing
    /// the record.
    pub is_extant: bool,
    /// Consecutive-tick counter for the extinction rule. Each
    /// `step` increments this when `biomass <
    /// EXTINCTION_THRESHOLD_FRAC × producer_capacity` and resets it
    /// otherwise; when the streak reaches
    /// `EXTINCTION_CONFIRMATION_TICKS` the species is flagged
    /// extinct (`is_extant = false`) and emits a `SpeciesExtinct`
    /// event with `cause = PopulationCollapse`. Resets to `0` once
    /// extinction fires so the field stays bounded and can be
    /// re-used if a future rewilding rule restores `is_extant`.
    pub low_biomass_streak: u64,
    /// Primary habitat — used to look up the per-habitat Lindeman
    /// assimilation efficiency at predation time (P2.5). Defaults to
    /// `Habitat::Terrestrial` (10% assimilation) so back-compat
    /// fixtures that don't set it preserve the canonical Lindeman
    /// 10:1 behaviour.
    pub habitat: Habitat,
}

/// Per-planet ecosystem state. Owned by the higher-level world
/// loop; constructed once at worldgen and stepped each tick.
#[derive(Debug, Clone)]
pub struct PlanetEcosystem {
    pub species: BTreeMap<SpeciesId, EcoSpecies>,
    pub interactions: InteractionMatrix,
    /// Producer-tier carrying capacity. Producers grow logistically
    /// toward this value at `PRODUCER_GROWTH_RATE` per tick.
    pub producer_capacity: Real,
    /// Substrate tag (`"aqueous" | "ammoniacal" | "hydrocarbon" |
    /// "silicate"`) used to look up the oxidiser ladder when
    /// partitioning Chemoautotroph growth. Defaults to `"aqueous"`
    /// for back-compat with hand-built test fixtures.
    pub substrate_tag: &'static str,
    /// Live per-tick oxidiser ladder. Rebuilt each tick from
    /// `oxidiser_ladder(substrate_tag)` so density depletions reset
    /// on the next tick — the per-tick budget is a "what's available
    /// this turn" pool, not a slow-leak persistent reservoir.
    /// Exposed for tests that need to introspect the ladder
    /// pre/post-tick.
    pub current_oxidisers: Vec<Oxidiser>,
}

impl PlanetEcosystem {
    /// Construct a new ecosystem from a sampled species list and
    /// pre-built interaction matrix. Useful for tests that need a
    /// hand-tuned matrix; production code goes through
    /// `sample_ecosystem`. Defaults to the Aqueous oxidiser ladder.
    #[must_use]
    pub fn new(
        species: Vec<EcoSpecies>,
        interactions: InteractionMatrix,
        producer_capacity: Real,
    ) -> Self {
        Self::new_with_substrate(species, interactions, producer_capacity, "aqueous")
    }

    /// Like `new` but lets the caller pick the substrate that drives
    /// the Chemoautotroph oxidiser-ladder partition. Required for
    /// the Sprint 2 Item 9 hydrocarbon / silicate / ammoniacal
    /// chemoautotroph tests.
    #[must_use]
    pub fn new_with_substrate(
        species: Vec<EcoSpecies>,
        interactions: InteractionMatrix,
        producer_capacity: Real,
        substrate_tag: &'static str,
    ) -> Self {
        let map: BTreeMap<_, _> = species.into_iter().map(|s| (s.species_id, s)).collect();
        let current_oxidisers = oxidiser_ladder(substrate_tag);
        Self {
            species: map,
            interactions,
            producer_capacity,
            substrate_tag,
            current_oxidisers,
        }
    }

    /// Run one ecosystem tick without atmospheric coupling.
    /// Convenience wrapper that discards any extinction events;
    /// callers that want the event stream should use `step_at_tick`.
    /// Six passes (each over `BTreeMap`):
    ///
    /// 1. Producer logistic growth toward `producer_capacity`
    ///    (Photoautotroph + Mixotroph).
    /// 2. Chemoautotroph partition through the substrate oxidiser
    ///    ladder (Sprint 2 Item 9) — strongest-acceptor first.
    /// 3. Pairwise interactions: apply the per-pair delta computed
    ///    by the pair's `FunctionalResponse`.
    /// 4. Syntrophy enforcement on Mutualism pairs (Sprint 2 Item 9):
    ///    if either side falls below `SYNTROPHY_MIN_PARTNER_BIOMASS`,
    ///    drag *both* sides toward extinction.
    /// 5. Passive consumer decay. The Lindeman pyramid is *not*
    ///    enforced as a post-step cap (P2.5 dropped the corrective
    ///    scaling); per-habitat assimilation efficiency applied in
    ///    pass 3 is the physical mechanism that produces the pyramid
    ///    at steady state. Tests that want to assert
    ///    "no Lindeman runaway" can call
    ///    [`PlanetEcosystem::check_lindeman_invariant`] explicitly.
    /// 6. Extinction sweep (Sprint 2 Item 6a): each species whose
    ///    biomass sits below `EXTINCTION_THRESHOLD_FRAC ×
    ///    producer_capacity` for `EXTINCTION_CONFIRMATION_TICKS`
    ///    consecutive ticks is flagged `is_extant = false` and the
    ///    matching `SpeciesExtinct` event is returned by
    ///    `step_at_tick`.
    ///
    /// Kept for tests + callers that don't need biogeochemical
    /// coupling. Production callers should prefer
    /// [`PlanetEcosystem::step_with_biogeochem`] (Sprint 2 Item 6b)
    /// so producer growth is rate-limited by atmospheric CO2 +
    /// available energy and consumer/decomposer respiration returns
    /// CO2 to the air.
    pub fn step(&mut self) {
        let _ = self.step_at_tick(0);
    }

    /// Same as `step` but carries the current sim tick and returns
    /// the list of `SpeciesExtinct` events that fired this tick.
    /// The caller is expected to forward these to its `Emitter`.
    /// Returned in `SpeciesId` order (the underlying `BTreeMap`
    /// iteration) so the event sequence is deterministic.
    pub fn step_at_tick(&mut self, tick: u64) -> Vec<SpeciesExtinct> {
        self.grow_producers();
        self.partition_chemoautotrophs();
        self.apply_interactions();
        self.enforce_syntrophy();
        self.decay_consumers();
        // P2.5: no post-step Lindeman cap — per-habitat assimilation
        // efficiency (applied in `apply_interactions`) is the physical
        // mechanism that produces the pyramid at steady state.
        // Short-term overshoots are allowed; the debug-only
        // `check_lindeman_invariant` method exists for callers + tests
        // that want to assert no runaway growth, but it isn't part of
        // the per-tick step (a hand-built fixture that starts with an
        // inverted pyramid shouldn't trip it).
        self.clamp_biomasses();
        self.detect_extinctions(tick)
    }

    /// Run one ecosystem tick *and* exchange carbon with the
    /// atmosphere via the supplied `PhysicsState` (Sprint 2 Item 6b).
    ///
    /// - Each Producer consumes atmospheric `CO2`, rate-limited by
    ///   `min(co2_available, energy_available, base_potential)`.
    ///   Energy comes from `solar_irradiance` for Photoautotrophs,
    ///   from the planet-wide `Oxidiser` pool for Chemoautotrophs,
    ///   and from their sum for Mixotrophs.
    /// - Each Consumer (anything not a Producer) respires a fixed
    ///   fraction of its biomass back to atmospheric `CO2`.
    /// - When at least one Detritivore or Saprotroph is present, a
    ///   small fraction of *all* species' biomass passes through the
    ///   decomposer chain back to `CO2`.
    ///
    /// CO2 deltas are applied uniformly across grid cells (consumed
    /// per-cell as `consumed / n_cells`, returned the same way), so
    /// the per-tick mass change matches the aggregate-level budget
    /// the biogeochem model balances at.
    pub fn step_with_biogeochem(
        &mut self,
        state: &mut PhysicsState,
        solar_irradiance: Real,
    ) {
        let _ = self.step_with_biogeochem_at_tick(state, solar_irradiance, 0);
    }

    /// Same as `step_with_biogeochem` but carries the current sim
    /// tick and returns any `SpeciesExtinct` events that fired this
    /// tick. The extinction sweep (Item 6a) runs after the
    /// biogeochem-coupled passes so a species that lost biomass to
    /// respiration / decomposition can be flagged extinct in the
    /// same tick.
    pub fn step_with_biogeochem_at_tick(
        &mut self,
        state: &mut PhysicsState,
        solar_irradiance: Real,
        tick: u64,
    ) -> Vec<SpeciesExtinct> {
        let co2_consumed = self.grow_producers_with_co2(state, solar_irradiance);
        // Item 9 paths: Chemoautotroph oxidiser-ladder partition and
        // syntrophy enforcement still run alongside the biogeochem
        // coupling so a planet with both layers gets the full stack.
        self.partition_chemoautotrophs();
        self.apply_interactions();
        self.enforce_syntrophy();
        self.decay_consumers();
        let respired = self.respire_consumers();
        let decomposed = self.decomposer_chain();
        // P2.5: no post-step Lindeman cap; per-habitat assimilation
        // is the physical mechanism.
        self.clamp_biomasses();
        let co2_returned = respired + decomposed;
        apply_co2_delta(state, co2_returned - co2_consumed);
        self.detect_extinctions(tick)
    }

    fn grow_producers(&mut self) {
        let growth_rate = Real::from(PRODUCER_GROWTH_RATE);
        let cap = self.producer_capacity;
        for s in self.species.values_mut() {
            if !s.is_extant {
                continue;
            }
            // Only Photoautotroph + Mixotroph drive off the logistic;
            // Chemoautotroph growth runs through the oxidiser ladder.
            if let EcosystemRole::Producer { metabolism } = s.role {
                if matches!(metabolism, ProducerMetabolism::Chemoautotroph) {
                    continue;
                }
                // Logistic: dB = r × B × (1 - B / K).
                if cap > Real::ZERO {
                    let ratio = s.biomass / cap;
                    let slack = Real::ONE - ratio;
                    let delta = growth_rate * s.biomass * slack;
                    s.biomass = s.biomass + delta;
                }
            }
        }
    }

    /// Per-tick Chemoautotroph partition. Rebuilds the planet's
    /// per-tick oxidiser ladder, collects each Chemoautotroph
    /// species' growth demand (logistic shape, same coefficient as
    /// Photoautotrophs), and walks the ladder greedy-strongest-first.
    /// Iteration order is `BTreeMap`-stable so the first
    /// Chemoautotroph (lowest `SpeciesId`) gets the strongest
    /// available oxidiser. After this pass `current_oxidisers`
    /// reflects the post-tick residual densities for diagnostics.
    fn partition_chemoautotrophs(&mut self) {
        // Reset the per-tick ladder.
        self.current_oxidisers = oxidiser_ladder(self.substrate_tag);
        let growth_rate = Real::from(CHEMOAUTOTROPH_GROWTH_RATE);
        let cap = self.producer_capacity;

        // Collect Chemoautotrophs in deterministic order, paired with
        // their growth demand.
        let mut species_indices: Vec<SpeciesId> = Vec::new();
        let mut demands: Vec<Real> = Vec::new();
        for (id, s) in &self.species {
            if !s.is_extant {
                continue;
            }
            if let EcosystemRole::Producer { metabolism } = s.role {
                if matches!(metabolism, ProducerMetabolism::Chemoautotroph) {
                    species_indices.push(*id);
                    let demand = if cap > Real::ZERO {
                        let ratio = s.biomass / cap;
                        let slack = Real::ONE - ratio;
                        growth_rate * s.biomass * slack
                    } else {
                        Real::ZERO
                    };
                    demands.push(demand);
                }
            }
        }

        if species_indices.is_empty() {
            return;
        }

        let shares =
            partition_chemoautotroph_growth(&mut self.current_oxidisers, &demands);
        for share in &shares {
            let id = species_indices[share.species_index];
            if let Some(s) = self.species.get_mut(&id) {
                s.biomass = s.biomass + share.growth_units;
            }
        }
    }

    /// Syntrophy enforcement (Sprint 2 Item 9). For each unordered
    /// Mutualism pair (we treat `(a, b)` and `(b, a)` as one
    /// relationship), if either side's biomass falls below
    /// `SYNTROPHY_MIN_PARTNER_BIOMASS`, multiply *both* sides by
    /// `(1 - SYNTROPHY_COLLAPSE_RATE)` for this tick. The
    /// asymmetric form models the biology faithfully: a methanogen
    /// without its H2-producer partner can't survive the niche even
    /// if its own biomass started high.
    ///
    /// Pairs are deduplicated through a `BTreeSet<(min, max)>` so
    /// the canonical interaction matrix's symmetric two-direction
    /// storage doesn't double-apply the collapse.
    pub fn enforce_syntrophy(&mut self) {
        let floor = Real::from(SYNTROPHY_MIN_PARTNER_BIOMASS);
        let collapse = Real::from(SYNTROPHY_COLLAPSE_RATE);
        let survival = Real::ONE - collapse;

        // Deduplicate symmetric pairs.
        let mut pairs: BTreeSet<(SpeciesId, SpeciesId)> = BTreeSet::new();
        for ((a, b), interaction) in &self.interactions.pairs {
            if interaction.kind != InteractionKind::Mutualism {
                continue;
            }
            let pair = if a <= b { (*a, *b) } else { (*b, *a) };
            pairs.insert(pair);
        }

        for (a, b) in &pairs {
            let ba = self
                .species
                .get(a)
                .filter(|s| s.is_extant)
                .map(|s| s.biomass)
                .unwrap_or(Real::ZERO);
            let bb = self
                .species
                .get(b)
                .filter(|s| s.is_extant)
                .map(|s| s.biomass)
                .unwrap_or(Real::ZERO);
            let weak_side_below = ba < floor || bb < floor;
            if !weak_side_below {
                continue;
            }
            // Drag both sides down. A side already at zero stays at
            // zero (zero * survival = zero).
            if let Some(s) = self.species.get_mut(a) {
                s.biomass = s.biomass * survival;
            }
            if let Some(s) = self.species.get_mut(b) {
                s.biomass = s.biomass * survival;
            }
        }
    }

    /// Producer growth coupled to atmospheric `CO2` + an energy
    /// source (Sprint 2 Item 6b). Returns the *total* CO2 actually
    /// consumed across all producers this tick — the caller
    /// subtracts it from the atmosphere.
    ///
    /// For each producer:
    ///   base_potential = r × B × (1 - B/K)      (same logistic shape)
    ///   gated_growth   = min(co2_available_share,
    ///                        energy_available_share,
    ///                        base_potential)
    ///   B' = B + gated_growth
    ///
    /// CO2 + energy are split equally between the producers that
    /// would otherwise grow this tick — every producer "competes"
    /// for the same atmosphere + sunlight pool, and the
    /// equal-split prevents one species from greedily monopolising
    /// the entire CO2 budget in one tick. Chemoautotroph growth via
    /// the multi-oxidiser ladder (Item 9) is layered on top by
    /// `partition_chemoautotrophs`; this path provides the carbon-
    /// budgeted baseline shared by all three metabolism kinds.
    fn grow_producers_with_co2(
        &mut self,
        state: &PhysicsState,
        solar_irradiance: Real,
    ) -> Real {
        let growth_rate = Real::from(PRODUCER_GROWTH_RATE);
        let cap = self.producer_capacity;
        if cap <= Real::ZERO {
            return Real::ZERO;
        }
        let total_co2 = sum_substance(state, Substance::CO2);
        let total_oxidiser = sum_substance(state, Substance::Oxidiser);

        // Producers that could grow this tick (extant, non-zero
        // biomass, room under K). Equal-split of CO2 + energy
        // across this set so a small atmosphere can't be drained
        // entirely by the largest producer in one shot.
        let producer_ids: Vec<SpeciesId> = self
            .species
            .iter()
            .filter_map(|(id, s)| {
                if !s.is_extant {
                    return None;
                }
                if matches!(s.role, EcosystemRole::Producer { .. }) {
                    Some(*id)
                } else {
                    None
                }
            })
            .collect();
        if producer_ids.is_empty() {
            return Real::ZERO;
        }
        let n_prod = Real::from_int(producer_ids.len() as i64);
        let co2_share = total_co2 / n_prod;
        let solar_share = solar_irradiance / n_prod;
        let oxidiser_share = total_oxidiser / n_prod;

        let mut total_consumed = Real::ZERO;
        for id in producer_ids {
            let s = match self.species.get_mut(&id) {
                Some(s) => s,
                None => continue,
            };
            let metabolism = match s.role {
                EcosystemRole::Producer { metabolism } => metabolism,
                _ => continue,
            };
            let ratio = s.biomass / cap;
            let slack = Real::ONE - ratio;
            let base_potential = growth_rate * s.biomass * slack;
            if base_potential <= Real::ZERO {
                continue;
            }
            let energy_share = match metabolism {
                ProducerMetabolism::Photoautotroph => solar_share,
                ProducerMetabolism::Chemoautotroph => oxidiser_share,
                ProducerMetabolism::Mixotroph => solar_share + oxidiser_share,
            };
            let gated = base_potential.min(co2_share).min(energy_share);
            if gated <= Real::ZERO {
                continue;
            }
            s.biomass = s.biomass + gated;
            total_consumed = total_consumed + gated;
        }
        total_consumed
    }

    /// Apply `RESPIRATION_RATE` to every extant consumer (anything
    /// not a Producer). Returns total CO2 returned to the atmosphere.
    /// Consumers lose biomass; that biomass becomes atmospheric CO2.
    fn respire_consumers(&mut self) -> Real {
        let rate = Real::from(RESPIRATION_RATE);
        let mut total = Real::ZERO;
        for s in self.species.values_mut() {
            if !s.is_extant {
                continue;
            }
            if let EcosystemRole::Producer { .. } = s.role {
                continue;
            }
            let respired = s.biomass * rate;
            if respired <= Real::ZERO {
                continue;
            }
            s.biomass = s.biomass - respired;
            total = total + respired;
        }
        total
    }

    /// Decomposer chain — when at least one Detritivore or
    /// Saprotroph is extant, free `DECOMPOSITION_RATE` × total
    /// biomass back to atmospheric CO2 *and* deduct that mass
    /// proportionally from every extant species pool.
    ///
    /// Closes the carbon budget: each unit of CO2 released to the
    /// atmosphere is balanced by a unit of biomass removed from
    /// the living pool. Models the steady-state dead-matter
    /// pipeline — even healthy populations are shedding some
    /// carbon through the decomposer compartment each tick, and
    /// the carbon that ends up in the atmosphere came from
    /// somebody's biomass.
    fn decomposer_chain(&mut self) -> Real {
        let has_decomposer = self.species.values().any(|s| {
            s.is_extant
                && matches!(
                    s.role,
                    EcosystemRole::Detritivore | EcosystemRole::Saprotroph
                )
        });
        if !has_decomposer {
            return Real::ZERO;
        }
        let rate = Real::from(DECOMPOSITION_RATE);
        let mut total_released = Real::ZERO;
        for s in self.species.values_mut() {
            if !s.is_extant {
                continue;
            }
            let released = s.biomass * rate;
            if released <= Real::ZERO {
                continue;
            }
            s.biomass = s.biomass - released;
            total_released = total_released + released;
        }
        total_released
    }

    fn apply_interactions(&mut self) {
        // Snapshot biomasses pre-step so deltas reference a
        // consistent state. Two-pass: build deltas into a separate
        // BTreeMap, apply at end.
        let biomass_snapshot: BTreeMap<SpeciesId, Real> = self
            .species
            .iter()
            .map(|(id, s)| (*id, if s.is_extant { s.biomass } else { Real::ZERO }))
            .collect();
        // Per-species habitat snapshot — used to pick the right
        // Lindeman assimilation efficiency at predation time (P2.5).
        let habitat_snapshot: BTreeMap<SpeciesId, Habitat> = self
            .species
            .iter()
            .map(|(id, s)| (*id, s.habitat))
            .collect();
        let mut deltas: BTreeMap<SpeciesId, Real> = BTreeMap::new();

        // Iterate pairs in sorted order — BTreeMap iterator is
        // deterministic.
        for ((affector, affected), interaction) in &self.interactions.pairs {
            let prey = match biomass_snapshot.get(affected) {
                Some(b) => *b,
                None => continue,
            };
            let pred = match biomass_snapshot.get(affector) {
                Some(b) => *b,
                None => continue,
            };
            if prey <= Real::ZERO || pred <= Real::ZERO {
                continue;
            }

            // Per-pair half-saturation (Sprint 2 Item P2.6). The
            // pair carries its calibrated fraction of producer
            // capacity; a back-compat literal with `half_saturation
            // = 0` falls through to the legacy 0.5 default so old
            // fixtures keep their numerics. Fraction × capacity
            // converts to the absolute `k` the functional response
            // expects.
            let half_sat_frac = if interaction.half_saturation > Real::ZERO {
                interaction.half_saturation
            } else {
                Real::from(K_HALF_SAT_DEFAULT)
            };
            let k = half_sat_frac * self.producer_capacity;

            // Functional-response: per-capita consumption per
            // predator unit, multiplied by predator biomass to get
            // the gross flux.
            let per_pred = functional_response(interaction.functional_response, prey, k);
            let flux = interaction.strength * pred * per_pred;

            match interaction.kind {
                InteractionKind::Predation | InteractionKind::Parasitism => {
                    // Predator gains a fraction of the flux —
                    // per-habitat Lindeman assimilation (P2.5).
                    // Aquatic predators run ~30:1; flying/amphibious
                    // ~6.7:1; terrestrial 10:1. Prey loses the full
                    // flux regardless: the dropped fraction is the
                    // "respired-as-heat / lost-to-decomposers"
                    // share that doesn't make it into predator tissue.
                    let predator_habitat = habitat_snapshot
                        .get(affector)
                        .copied()
                        .unwrap_or(Habitat::Terrestrial);
                    let assim = lindeman_assimilation_for_habitat(predator_habitat);
                    *deltas.entry(*affector).or_insert(Real::ZERO) =
                        *deltas.entry(*affector).or_insert(Real::ZERO) + flux * assim;
                    *deltas.entry(*affected).or_insert(Real::ZERO) =
                        *deltas.entry(*affected).or_insert(Real::ZERO) - flux;
                }
                InteractionKind::Competition => {
                    // Affector reduces the affected. Symmetric
                    // interactions live in the matrix as two
                    // entries so each side experiences a hit
                    // proportional to the other's biomass.
                    *deltas.entry(*affected).or_insert(Real::ZERO) =
                        *deltas.entry(*affected).or_insert(Real::ZERO) - flux;
                }
                InteractionKind::Mutualism => {
                    // Both sides benefit. Stored as two entries
                    // (a→b and b→a); each step adds a small
                    // benefit to the affected side proportional to
                    // the affector's biomass. The conversion uses
                    // the *recipient's* habitat — what its
                    // metabolism turns the gross mutualistic flux
                    // into biomass.
                    let affected_habitat = habitat_snapshot
                        .get(affected)
                        .copied()
                        .unwrap_or(Habitat::Terrestrial);
                    let assim = lindeman_assimilation_for_habitat(affected_habitat);
                    *deltas.entry(*affected).or_insert(Real::ZERO) =
                        *deltas.entry(*affected).or_insert(Real::ZERO) + flux * assim;
                }
                InteractionKind::Commensalism => {
                    // One-way benefit, no effect on the affector.
                    // Recipient's habitat governs assimilation.
                    let affected_habitat = habitat_snapshot
                        .get(affected)
                        .copied()
                        .unwrap_or(Habitat::Terrestrial);
                    let assim = lindeman_assimilation_for_habitat(affected_habitat);
                    *deltas.entry(*affected).or_insert(Real::ZERO) =
                        *deltas.entry(*affected).or_insert(Real::ZERO) + flux * assim;
                }
                InteractionKind::HabitatModification => {
                    // Engineer effect — small positive on the
                    // affected side, no draw on the affector. Kept
                    // at a flat 5% — the engineer's contribution is
                    // a niche-restructuring nudge, not a direct
                    // metabolic transfer, so the per-habitat
                    // Lindeman ratio doesn't apply.
                    let assim = Real::from((5, 100));
                    *deltas.entry(*affected).or_insert(Real::ZERO) =
                        *deltas.entry(*affected).or_insert(Real::ZERO) + flux * assim;
                }
            }
        }

        for (id, delta) in deltas {
            if let Some(s) = self.species.get_mut(&id) {
                s.biomass = s.biomass + delta;
            }
        }
    }

    fn decay_consumers(&mut self) {
        let decay = Real::from(CONSUMER_DECAY_RATE);
        for s in self.species.values_mut() {
            if !s.is_extant {
                continue;
            }
            if let EcosystemRole::Producer { .. } = s.role {
                continue;
            }
            s.biomass = s.biomass - s.biomass * decay;
        }
    }

    /// Lindeman pyramid invariant check (P2.5). Returns `Ok(())` if
    /// each consumer tier sits at no more than
    /// `LINDEMAN_OVERSHOOT_DEBUG_MAX × max_assimilation_ratio` times
    /// the lower tier; otherwise returns a `LindemanViolation` naming
    /// the offending tier pair and the magnitude of the overshoot.
    ///
    /// Replaces the corrective `enforce_lindeman_pyramid` from before
    /// the P2.5 fix — that one *scaled biomasses down* on every tick,
    /// which was double-bookkeeping the per-habitat assimilation
    /// efficiency already applied during the predation step. This
    /// function is a *read-only* invariant: it never modifies state.
    ///
    /// Skipped when the lower tier is below
    /// `producer_capacity × 1%` — a tier collapse isn't a Lindeman
    /// runaway, it's a cascade-extinction case the extinction rule
    /// handles, and the ratio diverges meaninglessly there.
    ///
    /// Intended for use in test invariants + debug assertions; the
    /// production step loop *does not* call this on every tick
    /// because hand-built fixtures that start with an inverted
    /// pyramid (e.g. the keystone-cascade test) would trip it
    /// before the dynamics had any chance to play out. Tests that
    /// want to assert "the pyramid held throughout the run" should
    /// call this themselves at the end of the simulated period.
    #[must_use]
    pub fn check_lindeman_invariant(&self) -> Result<(), LindemanViolation> {
        let max_ratio = self.max_consumer_assimilation();
        let slack = Real::from_int(LINDEMAN_OVERSHOOT_DEBUG_MAX) * max_ratio;
        let collapse_floor = self.producer_capacity * Real::from_ratio(1, 100);

        let producer_total = self.tier_biomass(0);
        if producer_total <= collapse_floor {
            return Ok(());
        }
        let primary_total = self.tier_biomass(1);
        if primary_total > producer_total * slack {
            return Err(LindemanViolation {
                upper_tier: 1,
                upper_biomass: primary_total,
                lower_biomass: producer_total,
                allowed_slack: slack,
            });
        }

        if primary_total > collapse_floor {
            let secondary_total = self.tier_biomass(2);
            if secondary_total > primary_total * slack {
                return Err(LindemanViolation {
                    upper_tier: 2,
                    upper_biomass: secondary_total,
                    lower_biomass: primary_total,
                    allowed_slack: slack,
                });
            }

            if secondary_total > collapse_floor {
                let apex_total = self.tier_biomass(3);
                if apex_total > secondary_total * slack {
                    return Err(LindemanViolation {
                        upper_tier: 3,
                        upper_biomass: apex_total,
                        lower_biomass: secondary_total,
                        allowed_slack: slack,
                    });
                }
            }
        }
        Ok(())
    }

    /// Largest per-habitat Lindeman assimilation ratio held by any
    /// extant non-Producer species in the ecosystem. Used as the
    /// conservative bound for the debug invariant (the higher the
    /// efficiency the higher the legitimate steady-state ratio).
    ///
    /// Falls back to the canonical terrestrial ratio (1/10) when no
    /// consumer is present so an empty-consumer planet still gets a
    /// sensible bound.
    fn max_consumer_assimilation(&self) -> Real {
        let mut best = Real::from_ratio(1, 10);
        for s in self.species.values() {
            if !s.is_extant {
                continue;
            }
            if matches!(s.role, EcosystemRole::Producer { .. }) {
                continue;
            }
            let r = lindeman_assimilation_for_habitat(s.habitat);
            if r > best {
                best = r;
            }
        }
        best
    }

    fn clamp_biomasses(&mut self) {
        for s in self.species.values_mut() {
            if s.biomass < Real::ZERO {
                s.biomass = Real::ZERO;
            }
        }
    }

    /// Per-species low-biomass streak counter. Once a species has
    /// been below the absolute threshold (`EXTINCTION_THRESHOLD_FRAC
    /// × producer_capacity`) for `EXTINCTION_CONFIRMATION_TICKS` in
    /// a row, flip `is_extant = false` and emit a
    /// `SpeciesExtinct { cause = PopulationCollapse }`. The species
    /// stays in `self.species` for history / replay determinism;
    /// later passes of `apply_interactions` / `grow_producers` /
    /// `decay_consumers` skip it via the `is_extant` guard.
    ///
    /// Iteration order is `BTreeMap`-deterministic so the event
    /// stream is byte-stable across rebuilds.
    fn detect_extinctions(&mut self, tick: u64) -> Vec<SpeciesExtinct> {
        let threshold =
            Real::from(EXTINCTION_THRESHOLD_FRAC) * self.producer_capacity;
        let mut events = Vec::new();
        for s in self.species.values_mut() {
            if !s.is_extant {
                // Already extinct — keep the streak at zero so a
                // future rewilding (not implemented this PR) starts
                // fresh.
                s.low_biomass_streak = 0;
                continue;
            }
            if s.biomass < threshold {
                s.low_biomass_streak = s.low_biomass_streak.saturating_add(1);
                if s.low_biomass_streak >= EXTINCTION_CONFIRMATION_TICKS {
                    s.is_extant = false;
                    s.low_biomass_streak = 0;
                    events.push(SpeciesExtinct {
                        tick,
                        species_id: s.species_id.0,
                        cause: ExtinctionCause::PopulationCollapse,
                    });
                }
            } else {
                s.low_biomass_streak = 0;
            }
        }
        events
    }

    /// Sum of biomasses for all extant species whose role tier
    /// matches `tier`.
    #[must_use]
    pub fn tier_biomass(&self, tier: u8) -> Real {
        let mut sum = Real::ZERO;
        for s in self.species.values() {
            if !s.is_extant {
                continue;
            }
            if let Some(t) = s.role.tier() {
                if t == tier {
                    sum = sum + s.biomass;
                }
            }
        }
        sum
    }

    /// Compute betweenness centrality over the interaction graph
    /// (treated undirected for keystone detection) and return any
    /// species whose normalised centrality exceeds the configured
    /// threshold.
    #[must_use]
    pub fn keystone_species(&self) -> BTreeSet<SpeciesId> {
        let centralities = self.betweenness_centrality();
        let n = self.species.len();
        if n < 3 {
            return BTreeSet::new();
        }
        // Maximum centrality for an undirected graph is
        // (n-1)(n-2)/2. Normalise then compare against threshold.
        let max_c = Real::from_int(((n - 1) * (n - 2) / 2) as i64);
        let threshold = Real::from(KEYSTONE_CENTRALITY_THRESHOLD);
        let mut out = BTreeSet::new();
        for (id, c) in centralities {
            if max_c > Real::ZERO {
                let normed = c / max_c;
                if normed >= threshold {
                    out.insert(id);
                }
            }
        }
        out
    }

    /// Compute betweenness centrality for every species via
    /// Brandes' algorithm on the unweighted, undirected interaction
    /// graph. Returns a `BTreeMap` so iteration order is stable.
    #[must_use]
    pub fn betweenness_centrality(&self) -> BTreeMap<SpeciesId, Real> {
        let mut adjacency: BTreeMap<SpeciesId, BTreeSet<SpeciesId>> = BTreeMap::new();
        for id in self.species.keys() {
            adjacency.insert(*id, BTreeSet::new());
        }
        for (a, b) in self.interactions.pairs.keys() {
            if !self.species.contains_key(a) || !self.species.contains_key(b) {
                continue;
            }
            adjacency.entry(*a).or_default().insert(*b);
            adjacency.entry(*b).or_default().insert(*a);
        }

        let ids: Vec<SpeciesId> = self.species.keys().copied().collect();
        let mut centrality: BTreeMap<SpeciesId, Real> =
            ids.iter().map(|id| (*id, Real::ZERO)).collect();

        // Brandes: for each source, do BFS, then back-accumulate.
        for s in &ids {
            // Predecessors of v on shortest paths from s.
            let mut preds: BTreeMap<SpeciesId, Vec<SpeciesId>> =
                ids.iter().map(|id| (*id, Vec::new())).collect();
            // sigma[v] = number of shortest paths from s to v.
            let mut sigma: BTreeMap<SpeciesId, i64> =
                ids.iter().map(|id| (*id, 0)).collect();
            sigma.insert(*s, 1);
            // dist[v] = shortest-path length s..v (negative = unset).
            let mut dist: BTreeMap<SpeciesId, i64> =
                ids.iter().map(|id| (*id, -1)).collect();
            dist.insert(*s, 0);

            let mut queue: std::collections::VecDeque<SpeciesId> =
                std::collections::VecDeque::new();
            queue.push_back(*s);
            let mut stack: Vec<SpeciesId> = Vec::new();

            while let Some(v) = queue.pop_front() {
                stack.push(v);
                let v_dist = *dist.get(&v).unwrap_or(&-1);
                if let Some(neighbours) = adjacency.get(&v) {
                    for w in neighbours {
                        let w_dist = *dist.get(w).unwrap_or(&-1);
                        if w_dist < 0 {
                            dist.insert(*w, v_dist + 1);
                            queue.push_back(*w);
                        }
                        if *dist.get(w).unwrap_or(&-1) == v_dist + 1 {
                            let new_sigma =
                                *sigma.get(w).unwrap_or(&0) + *sigma.get(&v).unwrap_or(&0);
                            sigma.insert(*w, new_sigma);
                            preds.entry(*w).or_default().push(v);
                        }
                    }
                }
            }

            // Back-accumulate dependencies.
            let mut delta: BTreeMap<SpeciesId, Real> =
                ids.iter().map(|id| (*id, Real::ZERO)).collect();
            while let Some(w) = stack.pop() {
                let sigma_w = *sigma.get(&w).unwrap_or(&1);
                let delta_w = *delta.get(&w).unwrap_or(&Real::ZERO);
                if let Some(pred_list) = preds.get(&w) {
                    for v in pred_list {
                        let sigma_v = *sigma.get(v).unwrap_or(&0);
                        if sigma_w > 0 {
                            let contribution = Real::from_ratio(sigma_v, sigma_w)
                                * (Real::ONE + delta_w);
                            let cur = *delta.get(v).unwrap_or(&Real::ZERO);
                            delta.insert(*v, cur + contribution);
                        }
                    }
                }
                if w != *s {
                    let cur = *centrality.get(&w).unwrap_or(&Real::ZERO);
                    centrality.insert(w, cur + delta_w);
                }
            }
        }

        // Undirected — divide by 2.
        for v in centrality.values_mut() {
            *v = *v / Real::from_int(2);
        }
        centrality
    }
}

/// Aggregate per-substance density across every cell of the planet.
fn sum_substance(state: &PhysicsState, substance: Substance) -> Real {
    state
        .substance(substance.idx())
        .iter()
        .copied()
        .fold(Real::ZERO, |a, b| a + b)
}

/// Apply a planet-wide CO2 delta — positive = add to atmosphere,
/// negative = remove from atmosphere. Distributes the change
/// uniformly across cells (per-cell delta = total / n_cells). When
/// the requested removal exceeds the per-cell stock, the per-cell
/// value clamps at zero — the *available* CO2 was already gated by
/// the producer-growth path so this clamp protects against rounding
/// drift only.
fn apply_co2_delta(state: &mut PhysicsState, delta: Real) {
    if delta == Real::ZERO {
        return;
    }
    let co2 = state.substance_mut(Substance::CO2.idx());
    let n = co2.len();
    if n == 0 {
        return;
    }
    let per_cell = delta / Real::from_int(n as i64);
    for c in co2.iter_mut() {
        let next = *c + per_cell;
        *c = if next < Real::ZERO { Real::ZERO } else { next };
    }
}

/// Evaluate a functional response. `prey` is the affected species'
/// biomass; `k` is the half-saturation constant in the same units.
///
/// - `Linear` (Type I): `prey`.
/// - `Saturating` (Type II): `prey / (k + prey)`.
/// - `Sigmoidal` (Type III): `prey² / (k² + prey²)`.
///
/// The pair's `strength` and the predator biomass multiply this
/// number in the caller — keeping the function unit-free (per
/// per-predator per-strength unit) makes the per-pair branch in
/// `apply_interactions` readable.
#[must_use]
pub fn functional_response(response: FunctionalResponse, prey: Real, k: Real) -> Real {
    match response {
        FunctionalResponse::Linear => prey,
        FunctionalResponse::Saturating => {
            let denom = k + prey;
            if denom <= Real::ZERO {
                Real::ZERO
            } else {
                prey / denom
            }
        }
        FunctionalResponse::Sigmoidal => {
            let prey_sq = prey * prey;
            let k_sq = k * k;
            let denom = k_sq + prey_sq;
            if denom <= Real::ZERO {
                Real::ZERO
            } else {
                prey_sq / denom
            }
        }
    }
}

/// Sample an 8-20 species ecosystem honoring the role-distribution
/// spec (≥2 Producers, ≥3 PrimaryConsumers, ≥2 SecondaryConsumers,
/// ≥1 ApexConsumer, ≥1 Detritivore, ≥1 Saprotroph, 1-3 Mutualists,
/// 1-5 Parasites). Biomasses are seeded at Lindeman-respecting
/// totals so the first step doesn't need to scrub a violation.
///
/// Determinism: derived solely from `(planet_seed, producer_capacity)`
/// via a dedicated `ChaCha20` stream keyed off the seed.
#[must_use]
pub fn sample_ecosystem(planet_seed: u64, producer_capacity: Real) -> PlanetEcosystem {
    let mut rng = ChaCha20Rng::seed_from_u64(planet_seed ^ 0xEC05_5751_1F00_0BA1);
    let mut species: Vec<EcoSpecies> = Vec::new();
    let mut next_id: u32 = 0;

    // Sample role counts within spec bounds. Producer biomass is
    // split evenly across `n_producers`, primary across `n_primary`,
    // etc., so each tier total is *exact* (producer = capacity,
    // primary = 0.1 × capacity, secondary = 0.01 × capacity, apex =
    // 0.001 × capacity) regardless of how the role count happened
    // to land. Off-pyramid roles (detritivore / saprotroph /
    // mutualist / parasite) get small fixed per-species biomasses.
    let n_producers = rng.gen_range(2..=4);
    let n_primary = rng.gen_range(3..=5);
    let n_secondary = rng.gen_range(2..=3);
    let n_apex = rng.gen_range(1..=2);
    let n_mutualists = rng.gen_range(1..=3);
    let n_parasites = rng.gen_range(1..=5);

    let producer_tier = producer_capacity;
    let primary_tier = producer_capacity * Real::from(LINDEMAN_RATIO);
    let secondary_tier =
        primary_tier * Real::from(LINDEMAN_RATIO);
    let apex_tier = secondary_tier * Real::from(LINDEMAN_RATIO);

    let producer_per = producer_tier / Real::from_int(n_producers);
    let primary_per = primary_tier / Real::from_int(n_primary);
    let secondary_per = secondary_tier / Real::from_int(n_secondary);
    let apex_per = apex_tier / Real::from_int(n_apex);

    // Off-pyramid biomasses (small constants). Detritivores +
    // saprotrophs work on dead matter (not capped by Lindeman);
    // mutualists + parasites depend on a single host species, so a
    // small biomass keeps the network coupled without warping the
    // pyramid.
    let detritivore_per = producer_capacity * Real::from((2, 100));
    let saprotroph_per = producer_capacity * Real::from((1, 100));
    let mutualist_per = producer_capacity * Real::from((1, 100));
    let parasite_per = producer_capacity * Real::from((5, 1000));

    let push = |species: &mut Vec<EcoSpecies>,
                    next_id: &mut u32,
                    role: EcosystemRole,
                    biomass: Real| {
        species.push(EcoSpecies {
            species_id: SpeciesId(*next_id),
            role,
            biomass,
            is_extant: true,
            low_biomass_streak: 0,
            // Legacy sampling stream defaults to Terrestrial — the
            // canonical 10:1 Lindeman ratio — so existing fixtures
            // get bit-for-bit identical numerics. The
            // substrate-aware path
            // (`sample_ecosystem_with_substrate`) can override
            // habitat to match the planet's solvent chemistry.
            habitat: Habitat::Terrestrial,
        });
        *next_id += 1;
    };

    // Producers — metabolism cycles through the three variants.
    for i in 0..n_producers {
        let metabolism = match i % 3 {
            0 => ProducerMetabolism::Photoautotroph,
            1 => ProducerMetabolism::Chemoautotroph,
            _ => ProducerMetabolism::Mixotroph,
        };
        push(
            &mut species,
            &mut next_id,
            EcosystemRole::Producer { metabolism },
            producer_per,
        );
    }
    for _ in 0..n_primary {
        push(
            &mut species,
            &mut next_id,
            EcosystemRole::PrimaryConsumer,
            primary_per,
        );
    }
    for _ in 0..n_secondary {
        push(
            &mut species,
            &mut next_id,
            EcosystemRole::SecondaryConsumer,
            secondary_per,
        );
    }
    for _ in 0..n_apex {
        push(
            &mut species,
            &mut next_id,
            EcosystemRole::ApexConsumer,
            apex_per,
        );
    }
    push(
        &mut species,
        &mut next_id,
        EcosystemRole::Detritivore,
        detritivore_per,
    );
    push(
        &mut species,
        &mut next_id,
        EcosystemRole::Saprotroph,
        saprotroph_per,
    );
    for i in 0..n_mutualists {
        let kind = match i % 4 {
            0 => MutualismKind::Pollinator,
            1 => MutualismKind::SeedDisperser,
            2 => MutualismKind::Engineer,
            _ => MutualismKind::Generic,
        };
        push(
            &mut species,
            &mut next_id,
            EcosystemRole::Mutualist { kind },
            mutualist_per,
        );
    }
    for i in 0..n_parasites {
        let kind = match i % 3 {
            0 => ParasiteKind::Macro,
            1 => ParasiteKind::Micro,
            _ => ParasiteKind::Virus,
        };
        push(
            &mut species,
            &mut next_id,
            EcosystemRole::Parasite { kind },
            parasite_per,
        );
    }

    // Cap at 20 species total. Max draw is
    // 4+5+3+2+1+1+3+5 = 24 — trim parasites from the tail.
    while species.len() > 20 {
        species.pop();
    }

    let interactions = build_interaction_matrix(&species);

    PlanetEcosystem::new(species, interactions, producer_capacity)
}

/// Same as [`sample_ecosystem`] but lets the caller pin the substrate
/// tag that drives the Chemoautotroph oxidiser-ladder partition. The
/// production callsite in `sim-core::run` derives the tag from
/// `planet.metabolic_substrate.tag()` so the per-planet ecosystem
/// matches the planet's solvent chemistry; tests + back-compat go
/// through the existing `sample_ecosystem` (which pins to `"aqueous"`).
///
/// The seed XOR uses a different discriminator
/// (`0xEC05_0001_5751_1F00`) than the legacy `sample_ecosystem`
/// (`0xEC05_5751_1F00_0BA1`) so the two namespaces don't alias each
/// other — sim-core's production stream gets its own deterministic
/// draw that won't collide with the legacy unit-test stream.
#[must_use]
pub fn sample_ecosystem_with_substrate(
    planet_seed: u64,
    substrate_tag: &'static str,
    producer_capacity: Real,
) -> PlanetEcosystem {
    let mut eco = sample_ecosystem(planet_seed ^ 0xEC05_0001_5751_1F00, producer_capacity);
    eco.substrate_tag = substrate_tag;
    eco.current_oxidisers = oxidiser_ladder(substrate_tag);
    // P2.5: substrate-derived habitat. An aqueous (water-solvent)
    // world is implicitly aquatic — the per-habitat Lindeman
    // assimilation drops to ~3.3% (30:1) so producer-heavy pyramids
    // emerge. Non-aqueous substrates default to Terrestrial; a
    // hydrocarbon-lake species *could* be aquatic too but the
    // calibration data is thinner there, so the conservative default
    // is the 10:1 terrestrial value.
    let habitat = habitat_for_substrate(substrate_tag);
    for s in eco.species.values_mut() {
        s.habitat = habitat;
    }
    eco
}

/// Derive the dominant per-species habitat from a planet's solvent
/// substrate tag (P2.5). Used by
/// [`sample_ecosystem_with_substrate`] to pin the per-habitat
/// Lindeman assimilation ratio without requiring callers to set it
/// per-species.
///
/// Mapping:
/// - `"aqueous"` → `Aquatic` (30:1 Lindeman ratio — water-solvent
///   life is overwhelmingly fish-equivalent for trophic
///   accounting).
/// - All other substrates (`"ammoniacal"`, `"hydrocarbon"`,
///   `"silicate"`, fallback) → `Terrestrial` (10:1). The
///   canonical default; off-Earth substrates that *might* warrant a
///   different ratio aren't calibrated well enough to pin
///   confidently.
#[must_use]
pub fn habitat_for_substrate(substrate_tag: &str) -> Habitat {
    match substrate_tag {
        "aqueous" => Habitat::Aquatic,
        _ => Habitat::Terrestrial,
    }
}

/// Build a canonical interaction matrix from a sampled species list.
///
/// Wiring rules:
/// - Every primary consumer preys on every producer (Saturating).
/// - Every secondary consumer preys on every primary consumer
///   (Saturating).
/// - Every apex consumer preys on every secondary consumer
///   (Saturating).
/// - Same-tier consumers compete (Competition, symmetric).
/// - Mutualists pair with the first Producer (Mutualism, symmetric).
/// - Parasites prey on the first PrimaryConsumer host (Parasitism).
/// - Detritivore + Saprotroph have HabitatModification edges to all
///   producers (they enable the recycling loop).
fn insert_competition(matrix: &mut InteractionMatrix, ids: &[SpeciesId], strength: Real) {
    // Competition uses a Linear functional response so the
    // half-saturation value never enters the per-tick math; carry the
    // neutral default (0.5) for forward compatibility.
    let half_saturation = Real::from(HALF_SAT_MUTUALISM);
    for (i, a) in ids.iter().enumerate() {
        for b in &ids[i + 1..] {
            matrix.insert(
                *a,
                *b,
                Interaction {
                    kind: InteractionKind::Competition,
                    strength,
                    functional_response: FunctionalResponse::Linear,
                    half_saturation,
                },
            );
            matrix.insert(
                *b,
                *a,
                Interaction {
                    kind: InteractionKind::Competition,
                    strength,
                    functional_response: FunctionalResponse::Linear,
                    half_saturation,
                },
            );
        }
    }
}

fn build_interaction_matrix(species: &[EcoSpecies]) -> InteractionMatrix {
    let mut matrix = InteractionMatrix::new();

    let producers: Vec<_> = species
        .iter()
        .filter(|s| matches!(s.role, EcosystemRole::Producer { .. }))
        .map(|s| s.species_id)
        .collect();
    let primary: Vec<_> = species
        .iter()
        .filter(|s| matches!(s.role, EcosystemRole::PrimaryConsumer))
        .map(|s| s.species_id)
        .collect();
    let secondary: Vec<_> = species
        .iter()
        .filter(|s| matches!(s.role, EcosystemRole::SecondaryConsumer))
        .map(|s| s.species_id)
        .collect();
    let apex: Vec<_> = species
        .iter()
        .filter(|s| matches!(s.role, EcosystemRole::ApexConsumer))
        .map(|s| s.species_id)
        .collect();
    let detritivores: Vec<_> = species
        .iter()
        .filter(|s| matches!(s.role, EcosystemRole::Detritivore))
        .map(|s| s.species_id)
        .collect();
    let saprotrophs: Vec<_> = species
        .iter()
        .filter(|s| matches!(s.role, EcosystemRole::Saprotroph))
        .map(|s| s.species_id)
        .collect();
    let mutualists: Vec<_> = species
        .iter()
        .filter(|s| matches!(s.role, EcosystemRole::Mutualist { .. }))
        .map(|s| s.species_id)
        .collect();
    let parasites: Vec<_> = species
        .iter()
        .filter(|s| matches!(s.role, EcosystemRole::Parasite { .. }))
        .map(|s| s.species_id)
        .collect();

    let predation_strength = Real::from((2, 100));
    let competition_strength = Real::from((1, 100));
    let mutualism_strength = Real::from((1, 100));
    let parasite_strength = Real::from((1, 100));
    let habmod_strength = Real::from((1, 100));

    // Per-pair half-saturation calibration (Sprint 2 Item P2.6).
    // Specialist predators (primary → producer, secondary → primary)
    // get the lynx-hare 0.30 — small predators saturate slowly. Apex
    // predators get the wolf-deer 0.10 — large apex predators
    // saturate fast on big prey items. Parasites inherit the
    // specialist baseline because micro-/macro-parasites depend on
    // host availability rather than apex-style satiation. Mutualism +
    // engineering effects get their own per-kind calibration.
    let half_sat_specialist = Real::from(HALF_SAT_SPECIALIST_PREDATOR);
    let half_sat_apex = Real::from(HALF_SAT_APEX_PREDATOR);
    let half_sat_mutualism = Real::from(HALF_SAT_MUTUALISM);
    let half_sat_habmod = Real::from(HALF_SAT_HABITAT_MOD);

    // Predation up the tier ladder.
    for c in &primary {
        for p in &producers {
            matrix.insert(
                *c,
                *p,
                Interaction {
                    kind: InteractionKind::Predation,
                    strength: predation_strength,
                    functional_response: FunctionalResponse::Saturating,
                    half_saturation: half_sat_specialist,
                },
            );
        }
    }
    for c in &secondary {
        for p in &primary {
            matrix.insert(
                *c,
                *p,
                Interaction {
                    kind: InteractionKind::Predation,
                    strength: predation_strength,
                    functional_response: FunctionalResponse::Saturating,
                    half_saturation: half_sat_specialist,
                },
            );
        }
    }
    for a in &apex {
        for s in &secondary {
            matrix.insert(
                *a,
                *s,
                Interaction {
                    kind: InteractionKind::Predation,
                    strength: predation_strength,
                    functional_response: FunctionalResponse::Saturating,
                    half_saturation: half_sat_apex,
                },
            );
        }
    }

    // Same-tier competition (symmetric: store both directions).
    insert_competition(&mut matrix, &primary, competition_strength);
    insert_competition(&mut matrix, &secondary, competition_strength);
    insert_competition(&mut matrix, &apex, competition_strength);

    // Mutualism (symmetric) with the first producer.
    if let Some(host) = producers.first() {
        for m in &mutualists {
            matrix.insert(
                *m,
                *host,
                Interaction {
                    kind: InteractionKind::Mutualism,
                    strength: mutualism_strength,
                    functional_response: FunctionalResponse::Saturating,
                    half_saturation: half_sat_mutualism,
                },
            );
            matrix.insert(
                *host,
                *m,
                Interaction {
                    kind: InteractionKind::Mutualism,
                    strength: mutualism_strength,
                    functional_response: FunctionalResponse::Saturating,
                    half_saturation: half_sat_mutualism,
                },
            );
        }
    }

    // Parasites on the first primary host.
    if let Some(host) = primary.first() {
        for p in &parasites {
            matrix.insert(
                *p,
                *host,
                Interaction {
                    kind: InteractionKind::Parasitism,
                    strength: parasite_strength,
                    functional_response: FunctionalResponse::Saturating,
                    half_saturation: half_sat_specialist,
                },
            );
        }
    }

    // Detritivore + Saprotroph engineering effects.
    for d in detritivores.iter().chain(saprotrophs.iter()) {
        for p in &producers {
            matrix.insert(
                *d,
                *p,
                Interaction {
                    kind: InteractionKind::HabitatModification,
                    strength: habmod_strength,
                    functional_response: FunctionalResponse::Linear,
                    half_saturation: half_sat_habmod,
                },
            );
        }
    }

    matrix
}

#[cfg(test)]
mod tests;
