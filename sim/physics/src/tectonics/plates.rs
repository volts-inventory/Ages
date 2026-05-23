//! Plate data model + deterministic worldgen sampling.
//!
//! This module owns the `Plate`, `CrustType`, and the seed-driven
//! Voronoi tiler that turns a `(seed, grid)` pair into a plate roster
//! + per-cell `plate_id` / `crust_thickness` arrays. The runtime law
//! (`Tectonics::integrate`) lives next door in `mod.rs` and consumes
//! the data this module emits at worldgen.

use crate::grid::HexGrid;
use sim_arith::Real;

/// Hex-direction axial offsets matching `HexGrid::neighbours` canonical
/// order (E, NE, NW, W, SW, SE). Duplicated from `hydrology.rs` /
/// `wind.rs` — two-line trivia, and re-exporting would commit the
/// internal representation as a stable API.
pub(super) const NEIGHBOUR_DIRECTIONS: [(i64, i64); 6] = [
    (1, 0),  // E
    (1, -1), // NE
    (0, -1), // NW
    (-1, 0), // W
    (-1, 1), // SW
    (0, 1),  // SE
];

/// Default oceanic crust thickness in km-equivalent. Real Earth ocean
/// crust averages ~7 km — much thinner and denser than continental
/// crust, which is why subduction (Item 12a) preferentially consumes
/// oceanic plates.
pub const OCEANIC_THICKNESS_KM: i64 = 7;

/// Default continental crust thickness in km-equivalent. Real Earth
/// continental crust averages ~35 km (≈40 under mountain belts).
pub const CONTINENTAL_THICKNESS_KM: i64 = 35;

/// Minimum number of plates sampled at worldgen for an earth-like
/// seed. Real Earth has ~7 major + ~8 minor → 15 total; lower bound
/// keeps small-grid worlds from collapsing to one super-plate.
pub const MIN_PLATES: u32 = 8;

/// Maximum number of plates sampled. Beyond ~15 the plate-boundary
/// density gets so high every cell sits at a boundary, which is
/// physically unrealistic (real planets have continent-sized plates
/// interspersed with smaller microplates, not uniformly tiny ones).
pub const MAX_PLATES: u32 = 15;

/// Probability (out of 100) a sampled plate is oceanic. The ~60/40
/// oceanic/continental split matches Earth's surface fraction:
/// ~71 % of the surface is ocean, but ocean crust is consumed
/// continuously by subduction, so the steady-state plate count
/// skews less than the surface area would imply.
pub const OCEANIC_PERCENT: u32 = 60;

/// SplitMix64 salt for the plate-sampling stream. Distinct from
/// terrain (`0xA17E_BEEF_C0DE_0147`) and species naming
/// (`0xFEED_FACE_BAAD_F00D`) so the streams stay independent.
const PLATE_SALT: u64 = 0x71EC_701C_5A1A_DEED;

/// Crust archetype carried by each `Plate`.
///
/// - `Oceanic` — thin (~7 km), basaltic, dense. Sinks at convergent
///   boundaries (Item 12a subduction).
/// - `Continental` — thick (~35 km), granitic, buoyant. Floats over
///   oceanic crust at convergent boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CrustType {
    Oceanic,
    Continental,
}

impl CrustType {
    /// Default thickness in km-equivalent for this crust class.
    #[must_use]
    pub fn default_thickness_km(self) -> Real {
        match self {
            CrustType::Oceanic => Real::from_int(OCEANIC_THICKNESS_KM),
            CrustType::Continental => Real::from_int(CONTINENTAL_THICKNESS_KM),
        }
    }
}

/// One tectonic plate. Holds its id, crust archetype, drift velocity
/// in cell-units per macro-tick (axial coords: `(vq, vr)`), and the
/// per-plate default crust thickness (per-cell thickness lives in
/// `PhysicsState::crust_thickness` so subduction / isostasy can mutate
/// individual cells without rewriting the plate).
#[derive(Debug, Clone, Copy)]
pub struct Plate {
    pub id: u32,
    pub crust_type: CrustType,
    /// Per-tick drift `(vq, vr)` in axial cell-units. Spec window is
    /// `[-2, +2]` per axis; the worldgen sampler enforces this.
    pub velocity: (Real, Real),
    /// Default thickness for cells owned by this plate at init, in
    /// km-equivalent. Per-cell thickness diverges from this once
    /// isostasy / subduction land.
    pub thickness: Real,
}

/// Sample a deterministic plate roster for a planet seed and grid.
/// 8-15 plates, each with a random `(crust_type, velocity, thickness)`
/// triple drawn from the same SplitMix64 stream so the same seed
/// always produces the same layout.
///
/// Returns `(plates, plate_id_per_cell, crust_thickness_per_cell)`.
/// Callers (typically `Tectonics::sample_plates_for_seed`) wrap the
/// plate vector in a `Tectonics` and write the latter two into
/// `PhysicsState` via `state.set_tectonics_fields(...)`.
pub(super) fn sample(
    seed: u64,
    grid: &HexGrid,
) -> (Vec<Plate>, Vec<u32>, Vec<Real>) {
    // SplitMix64 stream salted with `PLATE_SALT` so plate sampling
    // is independent of terrain / species name streams.
    let mut rng_state = seed ^ PLATE_SALT;
    // Step 1: choose plate count in [MIN_PLATES, MAX_PLATES].
    let plate_count = MIN_PLATES + (next_u64(&mut rng_state) % u64::from(MAX_PLATES - MIN_PLATES + 1)) as u32;
    // Step 2: sample plate cores. One axial coord per plate,
    // uniformly across the grid. Duplicates allowed but unlikely
    // on any reasonably-sized grid (≥ 8 cores out of ≥ 30 cells).
    let w = i64::from(grid.width());
    let h = i64::from(grid.height());
    let mut cores: Vec<(i64, i64)> = Vec::with_capacity(plate_count as usize);
    for _ in 0..plate_count {
        let q = (next_u64(&mut rng_state) % w as u64) as i64;
        let r = (next_u64(&mut rng_state) % h as u64) as i64;
        cores.push((q, r));
    }
    // Step 3: sample per-plate attributes. Lock the iteration
    // order so plate ids are stable across runs of the same seed.
    let mut plates: Vec<Plate> = Vec::with_capacity(plate_count as usize);
    for id in 0..plate_count {
        let crust_roll = next_u64(&mut rng_state) % 100;
        let crust_type = if crust_roll < u64::from(OCEANIC_PERCENT) {
            CrustType::Oceanic
        } else {
            CrustType::Continental
        };
        // Velocity in [-2, +2] per axis. Sample a uniform integer
        // in [0, 401), centre it on 200 to give [-200, +200], then
        // scale by 1/100 → [-2.00, +2.00].
        let vq_raw = (next_u64(&mut rng_state) % 401) as i64 - 200;
        let vr_raw = (next_u64(&mut rng_state) % 401) as i64 - 200;
        let velocity = (
            Real::from_ratio(vq_raw, 100),
            Real::from_ratio(vr_raw, 100),
        );
        let thickness = crust_type.default_thickness_km();
        plates.push(Plate {
            id,
            crust_type,
            velocity,
            thickness,
        });
    }
    // Step 4: assign every cell to its nearest plate core under
    // axial-distance metric (`|dq| + |dr|`, same metric the
    // terrain peak sampler uses for determinism). Ties broken by
    // lowest plate id so the assignment is deterministic.
    let n = grid.n_cells();
    let mut plate_id = vec![0u32; n];
    let mut crust_thickness = vec![Real::ZERO; n];
    for (cid, axial) in grid.cells() {
        let mut best_dist = i64::MAX;
        let mut best_id: u32 = 0;
        for (id, &(cq, cr)) in cores.iter().enumerate() {
            let dq = (i64::from(axial.q) - cq).abs();
            let dr = (i64::from(axial.r) - cr).abs();
            let dist = dq + dr;
            if dist < best_dist {
                best_dist = dist;
                best_id = id as u32;
            }
        }
        let i = cid.0 as usize;
        plate_id[i] = best_id;
        crust_thickness[i] = plates[best_id as usize].thickness;
    }
    (plates, plate_id, crust_thickness)
}

/// SplitMix64 step. Standard finaliser; mutates the state in-place
/// and returns the next 64-bit draw. Same shape as the SplitMix
/// helpers in `ecosystem::hgt` and `species::sampling` — deterministic,
/// no RNG state outside the caller's `u64`, uniform output.
pub(super) fn next_u64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}
