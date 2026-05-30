//! `Tectonics` struct definition and constructors.
//!
//! Carries the planet's plate roster, per-tick coefficients, and the
//! slab-pull / evolved-velocity buffers consumed by the orchestrator.
//! Split out of `mod.rs` so the data model lives separately from the
//! `Law::integrate` phase orchestration (see `orchestrator.rs`).

use super::plates::{self, Plate};
use crate::grid::HexGrid;
use crate::state::PhysicsState;
use sim_arith::Real;
use std::cell::RefCell;

/// Tectonics + erosion law. One instance carries the planet's plate
/// roster + coefficients. Wired into `integrate_civ_step` after
/// hydrology (so erosion sees the post-precipitation water field) and
/// before chemistry (so any rock-cycle CO2 follow-up in Item 12d sees
/// the post-tectonic surface state).
#[derive(Debug, Clone)]
pub struct Tectonics {
    /// Per-tick elevation gain per unit of inward boundary velocity.
    /// `1e-3 / (cell-unit · tick)` is small enough that the Himalaya-
    /// scale uplift accumulates over thousands of ticks (geological
    /// timescale on a per-month cadence) rather than spiking in one
    /// pass.
    pub convergence_rate: Real,
    /// Per-tick elevation loss per unit of outward boundary velocity.
    /// Same magnitude as `convergence_rate` so a symmetric collision
    /// + rift pair zeroes out (matches the "Earth's surface area is
    /// constant in the long-run mean" invariant; gross spatial
    /// rearrangement, not net creation / destruction).
    pub divergence_rate: Real,
    /// Per-tick fluvial-erosion coefficient. Multiplies
    /// `slope × precipitation × dt`. Tuned so a 100 m slope under
    /// earth-like wet precipitation (precip ≈ 1000) loses ~1 m per
    /// tick — visible on geological timescales without dominating
    /// the per-tick budget.
    pub erosion_k: Real,
    /// Plate roster, indexed by `plate_id`. Cell `i` belongs to
    /// `plates[state.plate_id()[i] as usize]`. Sorted by id so the
    /// vector index *is* the plate id; the worldgen sampler enforces
    /// this contract. `Plate::velocity` is the *initial* (worldgen)
    /// drift velocity — the evolved per-tick velocity lives in
    /// `current_velocity` so the initial roster stays inspectable
    /// (e.g. for the `tectonics_is_deterministic` test that checks
    /// the immediate post-sampling layout).
    pub plates: Vec<Plate>,
    /// Per-plate slab-pull force `(fq, fr)` accumulated this tick
    /// (Sprint 4 Item 12e). Re-computed on every `integrate` call
    /// before being applied to `current_velocity`. Exposed (and
    /// `RefCell`-wrapped) so callers — and the test suite — can
    /// inspect what the slab-pull pass produced this tick without
    /// having to re-derive it. Length matches `plates.len()` once
    /// `integrate` has run at least once.
    pub slab_pull_force: RefCell<Vec<(Real, Real)>>,
    /// Per-plate evolved velocity `(vq, vr)` (Sprint 4 Item 12e).
    /// Lazily initialised from `plates[i].velocity` on first
    /// `integrate` call. Subsequent calls add `slab_pull × dt` and
    /// clamp each axis to `[-MAX_PLATE_VELOCITY, +MAX_PLATE_VELOCITY]`.
    /// Reads through this field (not `plates[i].velocity`) so the
    /// uplift / divergence + subduction passes below see the
    /// up-to-date kinematic state — closes the "subduction drives
    /// velocity, velocity drives subduction" feedback loop.
    pub current_velocity: RefCell<Vec<(Real, Real)>>,
}

impl Tectonics {
    /// Earth-like default coefficients with an empty plate roster.
    /// Real runs build the plate roster via
    /// `Tectonics::sample_plates_for_seed`; this constructor exists
    /// for tests that build a deterministic plate layout by hand and
    /// for the orchestrator's parameter discovery path.
    #[must_use]
    pub fn earth_like() -> Self {
        Self::for_planet(Real::ONE)
    }

    /// Planet-scale tectonics calibration. `area_factor = radius²`
    /// lifts the per-tick uplift / divergence kicks in proportion
    /// to surface area: a bigger planet has proportionally more
    /// plate-boundary length and more crust-deformation activity,
    /// so the per-tick magnitude that drives boundary uplift /
    /// divergence (and the fluvial-erosion coefficient that
    /// redistributes the resulting relief) scales with `area`.
    /// Earth (factor 1.0) leaves every coefficient at the legacy
    /// `earth_like()` baseline byte-for-byte; the existing
    /// tectonics tests construct via `earth_like()` and so see no
    /// change.
    #[must_use]
    pub fn for_planet(area_factor: Real) -> Self {
        let base_conv = Real::from_ratio(1, 1_000);
        let base_div = Real::from_ratio(1, 1_000);
        let base_erosion = Real::from_ratio(1, 100_000);
        Self {
            convergence_rate: base_conv * area_factor,
            divergence_rate: base_div * area_factor,
            erosion_k: base_erosion * area_factor,
            plates: Vec::new(),
            // Empty until plates land; `integrate` resizes lazily so
            // tests that wire a plate roster after `earth_like()`
            // don't need a separate init step.
            slab_pull_force: RefCell::new(Vec::new()),
            current_velocity: RefCell::new(Vec::new()),
        }
    }

    /// Sample a deterministic plate roster for a planet seed and
    /// grid. 8-15 plates, each with a random `(crust_type, velocity,
    /// thickness)` triple drawn from the same SplitMix64 stream so
    /// the same seed always produces the same layout.
    ///
    /// Returns `(tectonics, plate_id_per_cell, crust_thickness_per_cell)`.
    /// Callers should write the latter two into `PhysicsState` via
    /// `state.set_tectonics_fields(...)` to keep the contract of
    /// "plate_id and crust_thickness are sized to grid.n_cells()"
    /// in one place.
    #[must_use]
    pub fn sample_plates_for_seed(
        seed: u64,
        grid: &HexGrid,
    ) -> (Self, Vec<u32>, Vec<Real>) {
        Self::sample_plates_for_planet(seed, grid, Real::ONE)
    }

    /// Same as `sample_plates_for_seed` but threads a planet
    /// `area_factor` (= `radius²`) through to the per-tick rate
    /// coefficients. Earth (factor 1.0) reduces to the legacy
    /// `sample_plates_for_seed` path. Real runs reach this
    /// constructor via `sim_core::setup`; tests stick with the
    /// `sample_plates_for_seed` shorthand.
    #[must_use]
    pub fn sample_plates_for_planet(
        seed: u64,
        grid: &HexGrid,
        area_factor: Real,
    ) -> (Self, Vec<u32>, Vec<Real>) {
        let (plates, plate_id, crust_thickness) = plates::sample(seed, grid);
        (
            Self {
                plates,
                ..Self::for_planet(area_factor)
            },
            plate_id,
            crust_thickness,
        )
    }

    /// Return the plate owning the given cell, if the plate roster
    /// is non-empty. Returns `None` when no plate is assigned
    /// (`plate_id[cell] >= plates.len()`), which happens on the
    /// default `earth_like()` construction before a worldgen sampler
    /// has run. The caller (e.g. `integrate`) treats this as "no
    /// tectonics this tick" rather than panicking.
    pub(crate) fn plate_for(&self, plate_id: u32) -> Option<&Plate> {
        self.plates.get(plate_id as usize)
    }

    /// Aggregate mass of crust subducted into the mantle (Sprint 4
    /// Item 12a). Reads through to `PhysicsState::subducted_mass`
    /// since the storage is per-state (the same plate roster can
    /// drive different planets); the accessor lives here so
    /// downstream callers (Item 12d volcanism) can wire to a single
    /// `Tectonics` handle rather than needing both the law and the
    /// state object.
    #[must_use]
    pub fn subducted_mass(state: &PhysicsState) -> Real {
        state.subducted_mass()
    }
}
