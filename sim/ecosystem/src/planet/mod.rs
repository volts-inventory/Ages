//! Per-planet ecosystem state + the per-tick step.
//!
//! Split out of `lib.rs` in CA2 and further partitioned into
//! per-concern submodules in CB4. Holds [`PlanetEcosystem`], every
//! mutation pass that runs each tick (producer growth, chemoautotroph
//! partition, pairwise interactions, syntrophy, consumer decay,
//! extinction detection), and the catastrophe + per-cell biomass
//! helpers. The Lindeman invariant *check* lives in
//! [`crate::invariants`]; the per-habitat assimilation efficiency
//! does too.
//!
//! ## CB4 submodule layout
//!
//! - [`step`] — main per-tick step (no biogeochem): producer
//!   logistic growth, Chemoautotroph oxidiser-ladder partition,
//!   pairwise interaction application, syntrophy enforcement, passive
//!   consumer decay, biomass clamp.
//! - [`biogeochem`] — CO2 coupling + biomass deltas:
//!   `step_with_biogeochem` / `step_with_biogeochem_at_tick`,
//!   `grow_producers_with_co2`, `respire_consumers`,
//!   `decomposer_chain`.
//! - [`extinction`] — extinction sweep (`detect_extinctions`).
//! - [`catastrophe`] — per-cell biomass reduction
//!   (`reduce_at_cell`) and the catastrophe-with-tolerance path
//!   (`apply_catastrophe_at_cell`).
//! - [`centrality`] — keystone-betweenness + Brandes' algorithm.
//! - [`helpers`] — `sum_substance`, `apply_co2_delta`,
//!   `lookup_mutualism_kind`, `lookup_parasite_kind`,
//!   `virus_outbreak_hash`.

use sim_arith::Real;
use sim_physics::chemistry::{oxidiser_ladder, Oxidiser};
use sim_species::{InteractionMatrix, SpeciesId};
use std::collections::BTreeMap;

use crate::species::EcoSpecies;

mod biogeochem;
mod catastrophe;
mod centrality;
mod extinction;
mod helpers;
mod step;

pub use helpers::virus_outbreak_hash;

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

    /// F2 — internal: redistribute the post-step aggregate delta
    /// proportionally back to each cell so the invariant
    /// `sum(cell_biomass) == biomass` holds. Called at the end of
    /// every step pass that mutated `biomass`. Skips species whose
    /// `cell_biomass` is empty (legacy fixtures); skips when the
    /// pre-step aggregate was zero (no proportional reference —
    /// fall back to uniform reseed of the new total).
    pub(super) fn rescale_cell_biomass(&mut self, prev_biomass: &BTreeMap<SpeciesId, Real>) {
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
    pub(super) fn snapshot_biomass(&self) -> BTreeMap<SpeciesId, Real> {
        self.species.iter().map(|(id, s)| (*id, s.biomass)).collect()
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
}
