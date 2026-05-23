//! Main per-tick ecosystem step (no biogeochem coupling). Split out
//! of `planet.rs` in CB4.
//!
//! Hosts the deterministic six-pass `step` / `step_at_tick`
//! entrypoints plus the per-pass helpers: producer logistic growth,
//! Chemoautotroph oxidiser-ladder partition, pairwise interaction
//! application (Predation / Parasitism / Mutualism / Commensalism /
//! Competition / HabitatModification), syntrophy enforcement, passive
//! consumer decay, and the biomass clamp. The carbon-coupled
//! producer-growth + respiration + decomposer passes live in
//! [`super::biogeochem`].

use protocol::SpeciesExtinct;
use sim_arith::Real;
use sim_physics::chemistry::{oxidiser_ladder, partition_chemoautotroph_growth};
use sim_species::{
    EcosystemRole, Habitat, InteractionKind, MutualismKind, ParasiteKind, ProducerMetabolism,
    SpeciesId,
};
use std::collections::{BTreeMap, BTreeSet};

use crate::constants::{
    CHEMOAUTOTROPH_GROWTH_RATE, CONSUMER_DECAY_RATE, ENGINEER_MATCH_BOOST, K_HALF_SAT_DEFAULT,
    MACRO_FERTILITY_MULTIPLIER, MICRO_CROWDING_THRESHOLD, MICRO_SURVIVAL_PENALTY,
    POLLINATOR_BIOMASS_COUPLING, PRODUCER_GROWTH_RATE, SEED_DISPERSER_BIOMASS_THRESHOLD,
    SEED_DISPERSER_RANGE_BOOST, SYNTROPHY_COLLAPSE_RATE, SYNTROPHY_MIN_PARTNER_BIOMASS,
    VIRUS_OUTBREAK_HOST_LOSS, VIRUS_OUTBREAK_PERIOD,
};
use crate::functional::functional_response;
use crate::invariants::lindeman_assimilation_for_habitat;

use super::helpers::{lookup_mutualism_kind, lookup_parasite_kind, virus_outbreak_hash};
use super::PlanetEcosystem;

impl PlanetEcosystem {
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
        // F2 — snapshot the pre-step aggregate so we can
        // proportionally redistribute the post-step delta back to
        // each species' per-cell biomass at the end of the tick.
        let prev_biomass = self.snapshot_biomass();
        self.grow_producers();
        self.partition_chemoautotrophs();
        self.apply_interactions(tick);
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
        // F2 — keep the per-cell distribution in sync with the new
        // aggregate. Proportional rescale preserves heterogeneity
        // introduced by per-cell catastrophe pokes.
        self.rescale_cell_biomass(&prev_biomass);
        self.detect_extinctions(tick)
    }

    pub(super) fn grow_producers(&mut self) {
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
    pub(crate) fn partition_chemoautotrophs(&mut self) {
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

    pub(super) fn apply_interactions(&mut self, tick: u64) {
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
        // Per-species role snapshot — used by the P3.1 differentiated
        // MutualismKind / ParasiteKind step to look up which side of
        // an interaction pair is the mutualist / parasite (the role
        // payload carries the variant).
        let role_snapshot: BTreeMap<SpeciesId, EcosystemRole> = self
            .species
            .iter()
            .map(|(id, s)| (*id, s.role))
            .collect();
        let mut deltas: BTreeMap<SpeciesId, Real> = BTreeMap::new();

        // P3.1 Engineer mutualism — collected during the main loop
        // and applied in a second pass so the +10% match-score boost
        // hits *all* cohabitors (same Habitat tag), not just direct
        // interaction partners. Stored as `(engineer_id,
        // engineer_biomass, host_habitat)`.
        let mut engineer_events: Vec<(SpeciesId, Real, Habitat)> = Vec::new();

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
                InteractionKind::Predation => {
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
                InteractionKind::Parasitism => {
                    // P3.1: branch on `ParasiteKind`. The *parasite*
                    // is whichever side of the pair has the
                    // `Parasite { kind }` role — typically the
                    // affector, but we look it up rather than assume.
                    let parasite_kind = lookup_parasite_kind(
                        &role_snapshot,
                        *affector,
                        *affected,
                    );
                    let predator_habitat = habitat_snapshot
                        .get(affector)
                        .copied()
                        .unwrap_or(Habitat::Terrestrial);
                    let assim = lindeman_assimilation_for_habitat(predator_habitat);
                    match parasite_kind {
                        Some(ParasiteKind::Macro) => {
                            // -10% extra host fertility hit. Models
                            // the chronic reproductive cost of worms
                            // / fleas: above the generic flux the
                            // host loses an additional 10% of the
                            // flux to suppressed fertility. The
                            // parasite gains the standard assimilated
                            // share of the *base* flux only — extra
                            // fertility damage is "respired-as-heat"
                            // from the host's growth budget, not
                            // assimilated tissue.
                            let fertility_penalty =
                                flux * Real::from(MACRO_FERTILITY_MULTIPLIER);
                            *deltas.entry(*affector).or_insert(Real::ZERO) =
                                *deltas.entry(*affector).or_insert(Real::ZERO)
                                    + flux * assim;
                            *deltas.entry(*affected).or_insert(Real::ZERO) =
                                *deltas.entry(*affected).or_insert(Real::ZERO)
                                    - flux
                                    - fertility_penalty;
                        }
                        Some(ParasiteKind::Micro) => {
                            // Crowding-disease scaling — extra -5%
                            // hit when the host biomass exceeds the
                            // crowding threshold. Below threshold the
                            // host is sparse enough that the
                            // density-dependent transmission rate
                            // doesn't bite.
                            let crowding_threshold = Real::from(MICRO_CROWDING_THRESHOLD)
                                * self.producer_capacity;
                            let extra = if prey >= crowding_threshold {
                                prey * Real::from(MICRO_SURVIVAL_PENALTY)
                            } else {
                                Real::ZERO
                            };
                            *deltas.entry(*affector).or_insert(Real::ZERO) =
                                *deltas.entry(*affector).or_insert(Real::ZERO)
                                    + flux * assim;
                            *deltas.entry(*affected).or_insert(Real::ZERO) =
                                *deltas.entry(*affected).or_insert(Real::ZERO)
                                    - flux
                                    - extra;
                        }
                        Some(ParasiteKind::Virus) => {
                            // Episodic — every VIRUS_OUTBREAK_PERIOD
                            // ticks the virus fires a deterministic
                            // hit at -30% host biomass. Between
                            // outbreaks the pair is inert (no flux,
                            // no assimilation). The SplitMix64 step
                            // mixes (tick, affector, affected) so
                            // multiple virus parasites firing on the
                            // same tick still produce a stable
                            // ordering for tie-breaking; the firing
                            // *condition* is the period gate.
                            if VIRUS_OUTBREAK_PERIOD != 0
                                && tick > 0
                                && tick % VIRUS_OUTBREAK_PERIOD == 0
                            {
                                let _ = virus_outbreak_hash(
                                    tick,
                                    affector.0,
                                    affected.0,
                                );
                                let host_loss =
                                    prey * Real::from(VIRUS_OUTBREAK_HOST_LOSS);
                                // The parasite gains a small share
                                // (Lindeman-assimilated) of the
                                // outbreak biomass — viruses
                                // amplify their own population on a
                                // successful hit.
                                *deltas.entry(*affector).or_insert(Real::ZERO) =
                                    *deltas.entry(*affector).or_insert(Real::ZERO)
                                        + host_loss * assim;
                                *deltas.entry(*affected).or_insert(Real::ZERO) =
                                    *deltas.entry(*affected).or_insert(Real::ZERO)
                                        - host_loss;
                            }
                            // Otherwise inert — no biomass change.
                        }
                        None => {
                            // Pair tagged as Parasitism but neither
                            // side has a Parasite role payload — fall
                            // through to the generic
                            // predation-equivalent path so back-compat
                            // fixtures don't regress.
                            *deltas.entry(*affector).or_insert(Real::ZERO) =
                                *deltas.entry(*affector).or_insert(Real::ZERO)
                                    + flux * assim;
                            *deltas.entry(*affected).or_insert(Real::ZERO) =
                                *deltas.entry(*affected).or_insert(Real::ZERO) - flux;
                        }
                    }
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
                    // P3.1: branch on `MutualismKind`. The *mutualist*
                    // side carries the variant; the *partner* side
                    // (typically a Producer) receives the
                    // differentiated boost.
                    let affected_habitat = habitat_snapshot
                        .get(affected)
                        .copied()
                        .unwrap_or(Habitat::Terrestrial);
                    let assim = lindeman_assimilation_for_habitat(affected_habitat);
                    let mut effective_flux = flux;

                    // Identify the mutualist side. Either side could
                    // carry the role (the symmetric pair is stored as
                    // both directions); the variant we apply to *this*
                    // direction is determined by whichever side is the
                    // Mutualist when the affector→affected is into a
                    // non-Mutualist (e.g. into the producer).
                    let mutualist_kind = lookup_mutualism_kind(
                        &role_snapshot,
                        *affector,
                        *affected,
                    );
                    let recipient_is_mutualist = matches!(
                        role_snapshot.get(affected),
                        Some(EcosystemRole::Mutualist { .. })
                    );

                    match mutualist_kind {
                        Some(MutualismKind::Pollinator) if !recipient_is_mutualist => {
                            // Pollinator boosts the producer flux —
                            // scaling with pollinator biomass relative
                            // to producer capacity. The pollinator
                            // side is the *affector* in this branch
                            // (we already established the recipient
                            // is not the mutualist), so `pred` is the
                            // pollinator biomass.
                            if self.producer_capacity > Real::ZERO {
                                let coupling = Real::from_int(POLLINATOR_BIOMASS_COUPLING)
                                    * (pred / self.producer_capacity);
                                effective_flux = flux + flux * coupling;
                            }
                        }
                        Some(MutualismKind::SeedDisperser) if !recipient_is_mutualist => {
                            // SeedDisperser extends producer range —
                            // multiply the flux by 1.20 once the
                            // disperser's biomass clears the
                            // threshold. The "extended range" is the
                            // physical mechanism: seeds reach more
                            // cells per tick.
                            let threshold = Real::from(SEED_DISPERSER_BIOMASS_THRESHOLD)
                                * self.producer_capacity;
                            if pred >= threshold {
                                effective_flux = flux * Real::from(SEED_DISPERSER_RANGE_BOOST);
                            }
                        }
                        Some(MutualismKind::Engineer) if !recipient_is_mutualist => {
                            // Engineer effect — book a cohabitor-wide
                            // match-score boost for the second pass.
                            // The recipient (the species we just
                            // identified as not-mutualist) is the
                            // engineer's primary host and inherits the
                            // normal Mutualism flux; cohabitors then
                            // get the +10% boost applied at the end.
                            let host_habitat = habitat_snapshot
                                .get(affected)
                                .copied()
                                .unwrap_or(Habitat::Terrestrial);
                            engineer_events.push((*affector, pred, host_habitat));
                        }
                        _ => {
                            // Generic (or symmetric reverse direction
                            // where the recipient *is* the mutualist):
                            // fall through to the standard symmetric
                            // mutualism flux.
                        }
                    }

                    *deltas.entry(*affected).or_insert(Real::ZERO) =
                        *deltas.entry(*affected).or_insert(Real::ZERO)
                            + effective_flux * assim;
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

        // P3.1 Engineer mutualism second pass — apply the +10%
        // match-score boost to *all* cohabitors (species sharing the
        // engineer's host's habitat). The boost is a small additive
        // biomass gain proportional to the cohabitor's biomass and
        // the engineer's biomass: `Δ = cohabitor × engineer ×
        // ENGINEER_MATCH_BOOST / producer_capacity`. Scaling by
        // engineer biomass and dividing by capacity keeps the boost
        // bounded — a small engineer cohort produces a small bump,
        // and a planet with a tiny capacity (test fixtures) doesn't
        // amplify it into a runaway.
        if self.producer_capacity > Real::ZERO {
            for (engineer_id, engineer_biomass, host_habitat) in &engineer_events {
                for (id, role) in &role_snapshot {
                    if *id == *engineer_id {
                        continue;
                    }
                    // Skip the engineer's own role payload.
                    if matches!(role, EcosystemRole::Mutualist { .. }) {
                        continue;
                    }
                    let cohabitor_habitat = habitat_snapshot
                        .get(id)
                        .copied()
                        .unwrap_or(Habitat::Terrestrial);
                    if cohabitor_habitat != *host_habitat {
                        continue;
                    }
                    let cohabitor_biomass = match biomass_snapshot.get(id) {
                        Some(b) if *b > Real::ZERO => *b,
                        _ => continue,
                    };
                    let boost = cohabitor_biomass
                        * *engineer_biomass
                        * Real::from(ENGINEER_MATCH_BOOST)
                        / self.producer_capacity;
                    *deltas.entry(*id).or_insert(Real::ZERO) =
                        *deltas.entry(*id).or_insert(Real::ZERO) + boost;
                }
            }
        }

        for (id, delta) in deltas {
            if let Some(s) = self.species.get_mut(&id) {
                s.biomass = s.biomass + delta;
            }
        }
    }

    pub(super) fn decay_consumers(&mut self) {
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

    pub(super) fn clamp_biomasses(&mut self) {
        for s in self.species.values_mut() {
            if s.biomass < Real::ZERO {
                s.biomass = Real::ZERO;
            }
        }
    }
}
