//! Per-planet ecosystem state + the per-tick step.
//!
//! Split out of `lib.rs` in CA2. Holds [`PlanetEcosystem`], every
//! mutation pass that runs each tick (producer growth, chemoautotroph
//! partition, pairwise interactions, syntrophy, consumer decay,
//! extinction detection), and the catastrophe + per-cell biomass
//! helpers. The Lindeman invariant *check* lives in
//! [`crate::invariants`]; the per-habitat assimilation efficiency
//! does too.

use protocol::{ExtinctionCause, SpeciesExtinct};
use sim_arith::Real;
use sim_physics::chemistry::{oxidiser_ladder, partition_chemoautotroph_growth, Oxidiser};
use sim_physics::{PhysicsState, Substance};
use sim_species::{
    EcosystemRole, Habitat, InteractionKind, InteractionMatrix, MutualismKind, ParasiteKind,
    ProducerMetabolism, SpeciesId,
};
use std::collections::{BTreeMap, BTreeSet};

use crate::constants::{
    CHEMOAUTOTROPH_GROWTH_RATE, CONSUMER_DECAY_RATE, DECOMPOSITION_RATE, ENGINEER_MATCH_BOOST,
    EXTINCTION_CONFIRMATION_TICKS, EXTINCTION_THRESHOLD_FRAC, K_HALF_SAT_DEFAULT,
    KEYSTONE_CENTRALITY_THRESHOLD, MACRO_FERTILITY_MULTIPLIER, MICRO_CROWDING_THRESHOLD,
    MICRO_SURVIVAL_PENALTY, POLLINATOR_BIOMASS_COUPLING, PRODUCER_GROWTH_RATE, RESPIRATION_RATE,
    SEED_DISPERSER_BIOMASS_THRESHOLD, SEED_DISPERSER_RANGE_BOOST, SYNTROPHY_COLLAPSE_RATE,
    SYNTROPHY_MIN_PARTNER_BIOMASS, VIRUS_OUTBREAK_HOST_LOSS, VIRUS_OUTBREAK_PERIOD,
};
use crate::functional::functional_response;
use crate::invariants::lindeman_assimilation_for_habitat;
use crate::species::EcoSpecies;

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
    /// F2 — planet grid cell count. Populated by
    /// [`PlanetEcosystem::initialise_cell_biomass`] /
    /// [`crate::sampling::sample_ecosystem_with_substrate_for_grid`]; left at `0` for
    /// the legacy aggregate-only construction path so existing
    /// fixtures keep their bit-for-bit numerics. When `> 0`, every
    /// `EcoSpecies.cell_biomass` Vec inside `species` has this
    /// length and the per-cell biomass invariant
    /// (`sum(cell_biomass) == biomass`) is maintained by the step
    /// loop.
    pub n_cells: usize,
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
            n_cells: 0,
        }
    }

    /// F2 (xeno N2) — install the per-cell biomass distribution. For
    /// each species, split its aggregate `biomass` across the
    /// `n_cells` cells. Sets `self.n_cells = n_cells` so the per-tick
    /// step + catastrophe path know the planet has a grid attached.
    /// Idempotent: calling twice with the same `n_cells` rebuilds the
    /// distribution from the *current* aggregate. Pass `0` to disable
    /// (clears every species' `cell_biomass`).
    ///
    /// **Distribution mode**:
    /// - `per_cell_weights = None` — uniform split (`aggregate /
    ///   n_cells` per cell). Legacy fallback; preserves the bit-exact
    ///   behaviour aggregate-only fixtures depend on.
    /// - `per_cell_weights = Some(weights)` — biome-class-weighted
    ///   split (T9). Each cell gets `aggregate × weights[i] /
    ///   sum(weights)`, so a planet's lush rainforest cells start
    ///   with more producer biomass than its deserts. `weights.len()`
    ///   must equal `n_cells`; negative weights are clamped to zero.
    ///   If `sum(weights) <= 0` (every cell uninhabitable), falls
    ///   back to the uniform split so the aggregate survives.
    ///
    /// The aggregate `biomass` stays the truth-source for the
    /// initial split; once per-cell evolution kicks in the step loop
    /// maintains the invariant `sum(cell_biomass) == biomass` by
    /// proportional redistribution. Catastrophes drain a single cell
    /// via [`PlanetEcosystem::reduce_at_cell`], which also keeps the
    /// aggregate in sync.
    pub fn initialise_cell_biomass(
        &mut self,
        n_cells: usize,
        per_cell_weights: Option<&[Real]>,
    ) {
        self.n_cells = n_cells;
        if n_cells == 0 {
            for s in self.species.values_mut() {
                s.cell_biomass.clear();
            }
            return;
        }
        // T9 — decide between weighted and uniform split. The
        // weighted path needs `weights.len() == n_cells` and a
        // strictly positive sum; otherwise we fall back to uniform so
        // the aggregate is preserved even on degenerate inputs (e.g.
        // every cell uninhabitable, or caller passed the wrong slice).
        let normalised: Option<Vec<Real>> = per_cell_weights.and_then(|w| {
            if w.len() != n_cells {
                return None;
            }
            // Clamp negative weights to zero so a buggy caller can't
            // turn an unhabitable cell into a biomass sink.
            let clamped: Vec<Real> = w
                .iter()
                .map(|x| if *x < Real::ZERO { Real::ZERO } else { *x })
                .collect();
            let sum: Real = clamped.iter().copied().fold(Real::ZERO, |a, b| a + b);
            if sum <= Real::ZERO {
                None
            } else {
                Some(clamped.into_iter().map(|x| x / sum).collect())
            }
        });
        let n_real = Real::from_int(n_cells as i64);
        for s in self.species.values_mut() {
            if let Some(ref shares) = normalised {
                // Weighted: each cell gets aggregate × share[i].
                s.cell_biomass = shares.iter().map(|share| s.biomass * *share).collect();
            } else {
                let per_cell = if n_real > Real::ZERO {
                    s.biomass / n_real
                } else {
                    Real::ZERO
                };
                s.cell_biomass = vec![per_cell; n_cells];
            }
            // Pin the aggregate to the actual cell sum so the
            // invariant `sum(cell_biomass) == biomass` is exact from
            // tick 0. Q32.32 division-then-multiplication loses up
            // to 1 ulp per cell; recomputing from the cells anchors
            // the truth-source to the per-cell slice.
            s.biomass = s
                .cell_biomass
                .iter()
                .copied()
                .fold(Real::ZERO, |a, b| a + b);
        }
    }

    /// F2 — reduce a single species' biomass at one specific cell by
    /// `fraction ∈ [0, 1]`. Used by the catastrophe path: a volcanic
    /// eruption on cell `c` drains the local producer pool *only*,
    /// without crashing the planet-wide aggregate. Updates both
    /// `cell_biomass[cell]` and the aggregate `biomass` so the
    /// invariant `sum(cell_biomass) == biomass` is preserved.
    ///
    /// No-op if `n_cells == 0` (legacy aggregate-only fixtures), if
    /// the species id is missing, if the species is already extinct,
    /// or if the cell index is out of range. Fraction is clamped to
    /// `[0, 1]` so a buggy caller can't increase biomass via a
    /// negative fraction or eat past zero.
    pub fn reduce_at_cell(&mut self, species_id: SpeciesId, cell: usize, fraction: Real) {
        if self.n_cells == 0 {
            return;
        }
        let frac = if fraction < Real::ZERO {
            Real::ZERO
        } else if fraction > Real::ONE {
            Real::ONE
        } else {
            fraction
        };
        let Some(s) = self.species.get_mut(&species_id) else {
            return;
        };
        if !s.is_extant {
            return;
        }
        if cell >= s.cell_biomass.len() {
            return;
        }
        let before = s.cell_biomass[cell];
        if before <= Real::ZERO {
            return;
        }
        let loss = before * frac;
        let after = before - loss;
        s.cell_biomass[cell] = if after < Real::ZERO { Real::ZERO } else { after };
        // Recompute the aggregate from cells so rounding drift stays
        // bounded. `biomass` is a cached value; the cell slice is the
        // truth-source once `n_cells > 0`.
        s.biomass = s
            .cell_biomass
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
    }

    /// F2 — internal: redistribute the post-step aggregate delta
    /// proportionally back to each cell so the invariant
    /// `sum(cell_biomass) == biomass` holds. Called at the end of
    /// every step pass that mutated `biomass`. Skips species whose
    /// `cell_biomass` is empty (legacy fixtures); skips when the
    /// pre-step aggregate was zero (no proportional reference —
    /// fall back to uniform reseed of the new total).
    fn rescale_cell_biomass(&mut self, prev_biomass: &BTreeMap<SpeciesId, Real>) {
        if self.n_cells == 0 {
            return;
        }
        let n_real = Real::from_int(self.n_cells as i64);
        for (id, s) in self.species.iter_mut() {
            if s.cell_biomass.is_empty() {
                continue;
            }
            let before = prev_biomass.get(id).copied().unwrap_or(Real::ZERO);
            let after = s.biomass;
            if before > Real::ZERO {
                // Proportional rescale: every cell scales by
                // `after / before`. Preserves heterogeneity introduced
                // by per-cell catastrophe pokes — a cell already
                // drained by a volcanic event stays proportionally
                // depressed after the planet-wide aggregate evolves.
                let scale = after / before;
                let mut total = Real::ZERO;
                for c in s.cell_biomass.iter_mut() {
                    *c = *c * scale;
                    if *c < Real::ZERO {
                        *c = Real::ZERO;
                    }
                    total = total + *c;
                }
                // Pin the aggregate to the actual cell sum so any
                // rounding drift is reflected back into `biomass`
                // (truth-source = cells).
                s.biomass = total;
            } else if after > Real::ZERO && n_real > Real::ZERO {
                // Reviving from zero — uniform reseed of the new
                // total.
                let per_cell = after / n_real;
                for c in s.cell_biomass.iter_mut() {
                    *c = per_cell;
                }
            } else {
                // Both zero — already in sync.
                for c in s.cell_biomass.iter_mut() {
                    *c = Real::ZERO;
                }
            }
        }
    }

    /// F2 — snapshot every species' aggregate biomass before a step
    /// pass mutates it. Used by [`Self::rescale_cell_biomass`] to
    /// proportionally redistribute the per-species delta back to
    /// the per-cell vectors.
    fn snapshot_biomass(&self) -> BTreeMap<SpeciesId, Real> {
        self.species.iter().map(|(id, s)| (*id, s.biomass)).collect()
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
        // F2 — snapshot pre-step aggregates for the per-cell rescale
        // pass below.
        let prev_biomass = self.snapshot_biomass();
        let co2_consumed = self.grow_producers_with_co2(state, solar_irradiance);
        // Item 9 paths: Chemoautotroph oxidiser-ladder partition and
        // syntrophy enforcement still run alongside the biogeochem
        // coupling so a planet with both layers gets the full stack.
        self.partition_chemoautotrophs();
        self.apply_interactions(tick);
        self.enforce_syntrophy();
        self.decay_consumers();
        let respired = self.respire_consumers();
        let decomposed = self.decomposer_chain();
        // P2.5: no post-step Lindeman cap; per-habitat assimilation
        // is the physical mechanism.
        self.clamp_biomasses();
        // F2 — proportionally redistribute the per-species aggregate
        // delta back to each cell. Catastrophe pokes inside the same
        // tick (which run *before* this step in the orchestrator)
        // have already been folded into the aggregate via
        // `reduce_at_cell`, so the rescale preserves their per-cell
        // heterogeneity.
        self.rescale_cell_biomass(&prev_biomass);
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

    fn apply_interactions(&mut self, tick: u64) {
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

    /// F3 — apply a catastrophe to every extant species in the
    /// ecosystem, scaling per-species biomass loss by
    /// `(1 - tolerance.match_score(cell_T, cell_pH, cell_sal,
    /// cell_rad, cell_p))`. Mirrors the P0.4 pattern used by
    /// `sim_civ::catastrophe::apply_resistance_and_dormancy` on the
    /// civ-bearing `Species`, extended into the trophic web so
    /// extremophile producers + consumers survive radiation bursts /
    /// thermal pulses that would otherwise wipe out narrow-envelope
    /// peers uniformly.
    ///
    /// `raw_loss_frac` is the headline severity in `[0, 1]` (the same
    /// fraction the civ-side path receives from its catastrophe
    /// pipeline); the tolerance term softens it to `raw_loss_frac ×
    /// (1 - match_score)` so:
    /// - `match_score = 1` (cell sits at envelope centre) ⇒ zero
    ///   biomass loss.
    /// - `match_score = 0` (cell outside envelope) ⇒ full
    ///   `raw_loss_frac` biomass loss.
    ///
    /// Cell conditions are passed as the local conditions during the
    /// catastrophe — for instance a radiation burst supplies `rad`
    /// near or above the typical species' `radiation_max`. The
    /// ecosystem currently runs as a single planet-wide aggregate
    /// (per-cell biota is a deferred refactor — see the post-fix
    /// xeno review N2), so the cell conditions are treated as the
    /// planet-wide event signature.
    pub fn apply_catastrophe_at_cell(
        &mut self,
        raw_loss_frac: Real,
        cell_t: Real,
        cell_ph: Real,
        cell_sal: Real,
        cell_rad: Real,
        cell_p: Real,
    ) {
        if raw_loss_frac <= Real::ZERO {
            return;
        }
        for s in self.species.values_mut() {
            if !s.is_extant {
                continue;
            }
            let survival_match =
                s.tolerance.match_score(cell_t, cell_ph, cell_sal, cell_rad, cell_p);
            let loss_frac = raw_loss_frac * (Real::ONE - survival_match);
            if loss_frac <= Real::ZERO {
                continue;
            }
            let loss = s.biomass * loss_frac;
            s.biomass = (s.biomass - loss).max(Real::ZERO);
        }
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

/// P3.1 helper — look up the `MutualismKind` of whichever side of the
/// `(a, b)` pair carries the `Mutualist { kind }` role payload. Returns
/// `None` if neither side does (back-compat fixtures with hand-built
/// matrices that use `InteractionKind::Mutualism` on non-mutualist
/// pairs, or fixtures that don't tag the role at all). When both sides
/// happen to be mutualists (uncommon but valid — two mutualist species
/// cooperating), returns the affector's kind so the per-direction
/// dispatch stays deterministic.
fn lookup_mutualism_kind(
    roles: &BTreeMap<SpeciesId, EcosystemRole>,
    affector: SpeciesId,
    affected: SpeciesId,
) -> Option<MutualismKind> {
    if let Some(EcosystemRole::Mutualist { kind }) = roles.get(&affector) {
        return Some(*kind);
    }
    if let Some(EcosystemRole::Mutualist { kind }) = roles.get(&affected) {
        return Some(*kind);
    }
    None
}

/// P3.1 helper — look up the `ParasiteKind` of whichever side of the
/// `(a, b)` pair carries the `Parasite { kind }` role payload. Returns
/// `None` if neither side does (back-compat fixtures with hand-built
/// matrices that use `InteractionKind::Parasitism` on non-parasite
/// pairs). Affector takes precedence — the typical wiring has the
/// parasite as the affector preying on its host.
fn lookup_parasite_kind(
    roles: &BTreeMap<SpeciesId, EcosystemRole>,
    affector: SpeciesId,
    affected: SpeciesId,
) -> Option<ParasiteKind> {
    if let Some(EcosystemRole::Parasite { kind }) = roles.get(&affector) {
        return Some(*kind);
    }
    if let Some(EcosystemRole::Parasite { kind }) = roles.get(&affected) {
        return Some(*kind);
    }
    None
}

/// P3.1 helper — SplitMix64-style hash of `(tick, affector_id,
/// affected_id)`. Used by the virus-parasite branch to derive a
/// deterministic tie-break order when multiple virus parasites fire
/// on the same outbreak tick. The cadence (period gate) is the firing
/// condition; this hash exists so future extensions (e.g. random
/// host-shopping among multiple candidates) have a deterministic
/// stream available without revisiting the call site.
#[must_use]
pub fn virus_outbreak_hash(tick: u64, affector: u32, affected: u32) -> u64 {
    let mut z = tick
        .wrapping_add((affector as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
        .wrapping_add((affected as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}
