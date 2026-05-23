//! Per-planet species record (`EcoSpecies`).
//!
//! Carved out of `lib.rs` in CA2. The struct + its trivial
//! constructors live here; the per-tick step that *mutates* a species
//! lives on [`crate::planet::PlanetEcosystem`].

use sim_arith::Real;
use sim_species::{EcosystemRole, Habitat, SpeciesId, ToleranceEnvelope};

/// One species' per-planet record. `species_id` is the dense per-
/// planet index; `biomass` is the live pool (always ≥ 0); the
/// other fields are configuration carried for the step.
///
/// **F2 (xeno N2) — per-cell biomass:** `cell_biomass` carries the
/// per-cell distribution of this species' standing biomass. Length
/// is `n_cells` when the ecosystem has been initialised against a
/// concrete planet grid (via [`crate::planet::PlanetEcosystem::initialise_cell_biomass`]
/// or [`crate::sampling::sample_ecosystem_with_substrate_for_grid`]); empty for the
/// legacy aggregate-only construction path used by hand-built test
/// fixtures. The invariant `sum(cell_biomass) == biomass` is
/// maintained by the step loop and the per-cell catastrophe poke
/// helper [`crate::planet::PlanetEcosystem::reduce_at_cell`] — `biomass` is the
/// cached aggregate, `cell_biomass` is the truth-source once
/// populated. Catastrophes that hit a single cell drain only that
/// cell's slice, enabling heterogeneous local famines (a volcanic
/// eruption no longer crashes producer biomass planet-wide).
///
/// Dropped `Copy` (P0.1 F2): the `Vec` field is heap-owned and
/// can't be bit-copied; `Clone` covers the path through species
/// snapshots and the per-step deep-copy.
#[derive(Debug, Clone)]
pub struct EcoSpecies {
    pub species_id: SpeciesId,
    pub role: EcosystemRole,
    /// Live biomass pool — *aggregate* across all cells. Same units
    /// as `producer_capacity`. Equal to `sum(cell_biomass)` once
    /// `cell_biomass` has been initialised (F2); a cached derived
    /// value the step loop maintains in sync.
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
    /// Per-cell biomass distribution (F2). Length equals the
    /// planet's `n_cells` once initialised, empty otherwise.
    pub cell_biomass: Vec<Real>,
    /// Environmental tolerance envelope (F3). Derived from the planet's
    /// metabolic substrate at worldgen with ±20% per-axis jitter from
    /// the species seed. Catastrophe path multiplies biomass loss by
    /// `(1 - tolerance.match_score(local conditions))`.
    pub tolerance: ToleranceEnvelope,
}
