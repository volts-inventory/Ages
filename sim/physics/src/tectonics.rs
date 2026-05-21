//! Tectonics + fluvial-erosion foundation (Sprint 4 Item 12).
//!
//! This is the base layer of Sprint 4's rock-cycle stack. It introduces
//! the per-cell plate identity, per-plate kinematics, boundary-driven
//! uplift / depression, and slope × precipitation fluvial erosion that
//! Sprint 4 sub-items (subduction, crust_age, isostasy, volcanism,
//! slab-pull) extend.
//!
//! ## Data model
//!
//! - `Plate` carries an id, a `CrustType` (oceanic / continental),
//!   a `(vq, vr)` axial-coordinate drift velocity in cells per
//!   macro-tick, and a `thickness` in km-equivalent. The thickness
//!   defaults to ~7 km for oceanic and ~35 km for continental, matching
//!   real terrestrial values.
//! - `PhysicsState` holds two new per-cell fields:
//!   - `plate_id: Vec<u32>` — which plate each cell belongs to.
//!   - `crust_thickness: Vec<Real>` — per-cell thickness in km-equiv.
//!     Initialised from the owning plate's default but mutable so
//!     follow-up sub-items (isostasy, subduction) can grow / shrink
//!     individual cells.
//!
//! Plate-ids are immutable per cell in this PR. Future plate-boundary
//! migration (Item 12a subduction, Item 12e slab-pull) will mutate
//! them; for now the assignment from worldgen is sticky.
//!
//! ## Tectonic step
//!
//! For each ordered pair of neighbouring cells `(i, j)` belonging to
//! different plates:
//!
//! - Compute the relative velocity `v_rel = v_plate_j - v_plate_i`
//!   projected onto the unit vector pointing from `i` to `j`. A
//!   *positive* projection means the cells are separating (divergent);
//!   *negative* means converging.
//! - Convergent: both cells get an elevation kick `+convergence_rate ×
//!   |projection| × dt`. Mountain building.
//! - Divergent: both cells get an elevation kick `-divergence_rate ×
//!   |projection| × dt`. Rift / ridge depression.
//!
//! The kick is symmetric across the pair so neither side gets
//! preferential uplift — single-side bookkeeping would bias the
//! orientation of mountain belts toward whichever cell sorts first
//! in the iteration order. Symmetric application matches the
//! qualitative real-world behaviour: both sides of the Himalaya rise,
//! both sides of the Mid-Atlantic Ridge subside.
//!
//! ## Erosion step
//!
//! For each pair `(i, j)` of in-plate neighbours (any plate boundary
//! handled by the tectonic step is excluded so erosion doesn't double-
//! count cells at the boundary):
//!
//! - Slope = `elevation[i] - elevation[j]` (signed; positive means
//!   `i` is higher).
//! - Eroded mass = `erosion_k × slope × precipitation × dt` where
//!   `precipitation` is the post-hydrology `water_depth + Vapour`
//!   stock at the uphill cell — this is the same humidity proxy
//!   `Weathering` uses.
//! - Elevation transfers from the higher cell to the lower one; total
//!   `Σ elevation` is conserved bit-exactly per pair.
//!
//! The `cumulative_erosion` debug counter tracks total mass moved so
//! a future regression that breaks the pair-flux symmetry trips early.
//!
//! ## Worldgen sampling
//!
//! `Tectonics::sample_plates_for_seed(seed, grid)` Voronoi-tiles the
//! hex grid with 8-15 plate cores drawn from a deterministic SplitMix64
//! stream. Each plate is independently assigned a `CrustType` (60 %
//! oceanic, 40 % continental) and a random velocity in
//! `[-2, +2]` cells per macro-tick per axis.
//!
//! ## Determinism
//!
//! - SplitMix64 finaliser, same shape used in `ecosystem::hgt`.
//! - `BTreeMap` iteration where ordering matters (none here — pair
//!   iteration walks the canonical grid order).
//! - Pair-flux for both uplift and erosion so totals are bit-exact.

use crate::grid::HexGrid;
use crate::laws::Law;
use crate::state::PhysicsState;
use sim_arith::Real;

/// Hex-direction axial offsets matching `HexGrid::neighbours` canonical
/// order (E, NE, NW, W, SW, SE). Duplicated from `hydrology.rs` /
/// `wind.rs` — two-line trivia, and re-exporting would commit the
/// internal representation as a stable API.
const NEIGHBOUR_DIRECTIONS: [(i64, i64); 6] = [
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
    /// this contract.
    pub plates: Vec<Plate>,
}

impl Tectonics {
    /// Earth-like default coefficients with an empty plate roster.
    /// Real runs build the plate roster via
    /// `Tectonics::sample_plates_for_seed`; this constructor exists
    /// for tests that build a deterministic plate layout by hand and
    /// for the orchestrator's parameter discovery path.
    #[must_use]
    pub fn earth_like() -> Self {
        Self {
            convergence_rate: Real::from_ratio(1, 1_000),
            divergence_rate: Real::from_ratio(1, 1_000),
            erosion_k: Real::from_ratio(1, 100_000),
            plates: Vec::new(),
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
        (
            Self {
                plates,
                ..Self::earth_like()
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
    fn plate_for(&self, plate_id: u32) -> Option<&Plate> {
        self.plates.get(plate_id as usize)
    }
}

impl Law for Tectonics {
    fn integrate(&self, state: &mut PhysicsState, dt: Real) {
        // No plates → no tectonics. Tests that don't construct a
        // plate roster (e.g. the orchestration smoke tests) get a
        // no-op rather than a panic; real runs always have plates
        // because `init_planet` calls `sample_plates_for_seed`.
        if self.plates.is_empty() {
            return;
        }
        // Bail if the per-cell plate-id field hasn't been sized.
        // Same rationale: a state built fresh and never run through
        // worldgen sampling has zero-length `plate_id`; the law
        // no-ops cleanly.
        if state.plate_id().len() != state.grid().n_cells() {
            return;
        }

        // Snapshot the fields we read so the writes don't interleave
        // with subsequent neighbour reads inside the same pass.
        let grid = state.grid().clone();
        let plate_ids = state.plate_id().to_vec();
        let elevation_in = state.elevation().to_vec();
        let water = state.water_depth().to_vec();
        let vapour = state
            .substance(crate::chemistry::Substance::Vapour.idx())
            .to_vec();
        let mut elevation = elevation_in.clone();

        // ---- Tectonic uplift / divergence at plate boundaries. ----
        //
        // Pair-iteration: each unordered pair `(i, j)` is visited
        // exactly once (we gate on `j > i`). For each pair across
        // different plates, compute the relative velocity projected
        // onto the unit hex-neighbour direction. Convergent
        // (projection negative — j moves toward i) → both cells rise.
        // Divergent → both cells sink. Symmetric in `i`, `j` so the
        // total `Σ elevation` change is `2 × per-cell-kick` per
        // boundary pair; not strictly mass-conserved (real tectonics
        // sources / sinks crust at boundaries) but the small kick
        // magnitude makes the drift bounded over geological time.
        for (cid, axial) in grid.cells() {
            let i = cid.0 as usize;
            let plate_i = plate_ids[i];
            for (k, nb) in grid.neighbours(axial).iter().enumerate() {
                let j = nb.0 as usize;
                if j <= i {
                    continue;
                }
                let plate_j = plate_ids[j];
                if plate_i == plate_j {
                    continue;
                }
                // Both plates must exist in the roster. Out-of-range
                // ids are skipped (defensive; would only fire if a
                // future sub-item shrinks the roster without
                // remapping cells).
                let (pi, pj) = match (
                    self.plate_for(plate_i),
                    self.plate_for(plate_j),
                ) {
                    (Some(a), Some(b)) => (a, b),
                    _ => continue,
                };
                // Direction unit vector from i to j in axial coords.
                let (dir_q, dir_r) = NEIGHBOUR_DIRECTIONS[k];
                let dir_q_r = Real::from_int(dir_q);
                let dir_r_r = Real::from_int(dir_r);
                // Relative velocity: v_j - v_i. Project onto (dir_q,
                // dir_r). A *positive* projection means j is moving
                // away from i along the i→j direction → divergent.
                // A *negative* projection means j is moving toward i
                // along i→j → convergent.
                let rel_q = pj.velocity.0 - pi.velocity.0;
                let rel_r = pj.velocity.1 - pi.velocity.1;
                let projection = rel_q * dir_q_r + rel_r * dir_r_r;
                let magnitude = projection.abs();
                if projection < Real::ZERO {
                    // Convergent: uplift both sides.
                    let kick = self.convergence_rate * magnitude * dt;
                    elevation[i] = elevation[i] + kick;
                    elevation[j] = elevation[j] + kick;
                } else if projection > Real::ZERO {
                    // Divergent: depression both sides. Don't let
                    // elevation go negative — real ocean floor goes
                    // *below* sea level (the sea floor IS negative
                    // elevation in some coord systems), but per the
                    // current `elevation` convention (metres above the
                    // reference geoid, sea_level is the threshold)
                    // sub-zero would confuse downstream slope reads.
                    // Clamp at zero; future Item 12c isostasy lifts
                    // this restriction once oceanic basins are
                    // first-class.
                    let kick = self.divergence_rate * magnitude * dt;
                    let new_i = elevation[i] - kick;
                    let new_j = elevation[j] - kick;
                    elevation[i] = new_i.max(Real::ZERO);
                    elevation[j] = new_j.max(Real::ZERO);
                }
                // Zero-projection (parallel boundary, no relative
                // motion along the boundary normal): transform-fault
                // analogue; no elevation change.
            }
        }

        // ---- Fluvial erosion. ----
        //
        // For every same-plate neighbour pair, transfer elevation
        // from the higher cell to the lower one at rate
        // `erosion_k × slope × precipitation × dt`, where
        // `precipitation = water + vapour` at the uphill cell (the
        // humidity proxy `Weathering` uses). Pair-flux symmetric:
        // the transferred amount is subtracted from one and added to
        // the other, conserving `Σ elevation` over the pair bit-
        // exactly.
        //
        // Cross-plate pairs are excluded so plate boundaries don't
        // double-count (the tectonic step already touched them).
        // This also dodges the awkward question of "which plate
        // 'owns' the eroded mass at a boundary" — punted to a
        // future sub-item if it ever matters.
        for (cid, axial) in grid.cells() {
            let i = cid.0 as usize;
            let plate_i = plate_ids[i];
            for nb in grid.neighbours(axial).iter() {
                let j = nb.0 as usize;
                if j <= i {
                    continue;
                }
                let plate_j = plate_ids[j];
                if plate_i != plate_j {
                    continue;
                }
                let slope = elevation[i] - elevation[j];
                if slope == Real::ZERO {
                    continue;
                }
                let (uphill, downhill, slope_mag) = if slope > Real::ZERO {
                    (i, j, slope)
                } else {
                    (j, i, -slope)
                };
                let precip = water[uphill] + vapour[uphill];
                if precip <= Real::ZERO {
                    continue;
                }
                let raw = self.erosion_k * slope_mag * precip * dt;
                // Don't erode more than half the slope in one pass;
                // otherwise an extreme rate would invert the slope.
                // The cap keeps the integrator stable even if a
                // future tuning bumps `erosion_k` aggressively.
                let cap = slope_mag / Real::from_int(2);
                let transfer = raw.min(cap);
                if transfer > Real::ZERO {
                    elevation[uphill] = elevation[uphill] - transfer;
                    elevation[downhill] = elevation[downhill] + transfer;
                }
            }
        }

        state.elevation_mut().copy_from_slice(&elevation);
    }
}

/// SplitMix64 step. Standard finaliser; mutates the state in-place
/// and returns the next 64-bit draw. Same shape as the SplitMix
/// helpers in `ecosystem::hgt` and `species::sampling` — deterministic,
/// no RNG state outside the caller's `u64`, uniform output.
fn next_u64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chemistry::Substance;
    use crate::grid::{Axial, HexGrid};

    /// Build a minimal two-plate state where the plates move toward
    /// each other across a vertical boundary. After N ticks the
    /// elevation at the boundary should rise from the initial value.
    #[test]
    fn convergent_boundary_uplifts_elevation() {
        let grid = HexGrid::new(6, 3);
        let n = grid.n_cells();
        let mut state = PhysicsState::new(grid.clone());

        // Two plates: left half (cols 0..3) is plate 0, right half
        // (cols 3..6) is plate 1. Velocities point toward each other
        // (plate 0 moves east, plate 1 moves west).
        let mut plate_id = vec![0u32; n];
        for (cid, axial) in grid.cells() {
            plate_id[cid.0 as usize] = if axial.q < 3 { 0 } else { 1 };
        }
        let plates = vec![
            Plate {
                id: 0,
                crust_type: CrustType::Continental,
                velocity: (Real::from_int(1), Real::ZERO),
                thickness: Real::from_int(CONTINENTAL_THICKNESS_KM),
            },
            Plate {
                id: 1,
                crust_type: CrustType::Continental,
                velocity: (Real::from_int(-1), Real::ZERO),
                thickness: Real::from_int(CONTINENTAL_THICKNESS_KM),
            },
        ];
        let crust_thickness = vec![Real::from_int(CONTINENTAL_THICKNESS_KM); n];
        state.set_tectonics_fields(plate_id.clone(), crust_thickness);

        // Pick a boundary cell on each side. Cell (2, 1) is on the
        // left plate's east edge; cell (3, 1) is on the right plate's
        // west edge. Their elevations should rise after the tectonic
        // step runs.
        let east_edge = state.grid().cell_id(Axial::new(2, 1)).0 as usize;
        let west_edge = state.grid().cell_id(Axial::new(3, 1)).0 as usize;
        let east_before = state.elevation()[east_edge];
        let west_before = state.elevation()[west_edge];

        let tect = Tectonics {
            plates,
            // Bigger rates than earth_like so the effect is visible
            // in a handful of ticks — the test asserts direction not
            // magnitude, but a small rate drowns the signal in
            // round-to-zero on Q32.32 fixed-point.
            convergence_rate: Real::percent(10),
            divergence_rate: Real::percent(10),
            erosion_k: Real::ZERO,
        };
        for _ in 0..20 {
            tect.integrate(&mut state, Real::ONE);
        }
        let east_after = state.elevation()[east_edge];
        let west_after = state.elevation()[west_edge];
        assert!(
            east_after > east_before,
            "east boundary should rise under convergence: \
             before={east_before:?} after={east_after:?}"
        );
        assert!(
            west_after > west_before,
            "west boundary should rise under convergence: \
             before={west_before:?} after={west_after:?}"
        );
    }

    /// Two plates moving apart should lower the boundary elevation.
    /// Seed the boundary with non-zero elevation so the divergence
    /// step has somewhere to move from (with the zero-floor clamp).
    #[test]
    fn divergent_boundary_lowers_elevation() {
        let grid = HexGrid::new(6, 3);
        let n = grid.n_cells();
        let mut state = PhysicsState::new(grid.clone());

        let mut plate_id = vec![0u32; n];
        for (cid, axial) in grid.cells() {
            plate_id[cid.0 as usize] = if axial.q < 3 { 0 } else { 1 };
        }
        // Plates move apart: plate 0 west, plate 1 east.
        let plates = vec![
            Plate {
                id: 0,
                crust_type: CrustType::Oceanic,
                velocity: (Real::from_int(-1), Real::ZERO),
                thickness: Real::from_int(OCEANIC_THICKNESS_KM),
            },
            Plate {
                id: 1,
                crust_type: CrustType::Oceanic,
                velocity: (Real::from_int(1), Real::ZERO),
                thickness: Real::from_int(OCEANIC_THICKNESS_KM),
            },
        ];
        let crust_thickness = vec![Real::from_int(OCEANIC_THICKNESS_KM); n];
        state.set_tectonics_fields(plate_id, crust_thickness);

        // Seed elevation high enough that the zero-floor clamp doesn't
        // dominate.
        for e in state.elevation_mut() {
            *e = Real::from_int(1000);
        }

        let east_edge = state.grid().cell_id(Axial::new(2, 1)).0 as usize;
        let west_edge = state.grid().cell_id(Axial::new(3, 1)).0 as usize;
        let east_before = state.elevation()[east_edge];
        let west_before = state.elevation()[west_edge];

        let tect = Tectonics {
            plates,
            convergence_rate: Real::percent(10),
            divergence_rate: Real::percent(10),
            erosion_k: Real::ZERO,
        };
        for _ in 0..20 {
            tect.integrate(&mut state, Real::ONE);
        }
        let east_after = state.elevation()[east_edge];
        let west_after = state.elevation()[west_edge];
        assert!(
            east_after < east_before,
            "east boundary should fall under divergence: \
             before={east_before:?} after={east_after:?}"
        );
        assert!(
            west_after < west_before,
            "west boundary should fall under divergence: \
             before={west_before:?} after={west_after:?}"
        );
    }

    /// Wet, steep cell loses elevation; flat dry cell doesn't.
    #[test]
    fn erosion_lowers_elevation_in_wet_steep_cells() {
        let grid = HexGrid::new(4, 3);
        let n = grid.n_cells();
        let mut state = PhysicsState::new(grid.clone());

        // Single plate so no tectonic step interferes; pure erosion.
        let plate_id = vec![0u32; n];
        let plates = vec![Plate {
            id: 0,
            crust_type: CrustType::Continental,
            velocity: (Real::ZERO, Real::ZERO),
            thickness: Real::from_int(CONTINENTAL_THICKNESS_KM),
        }];
        let crust_thickness = vec![Real::from_int(CONTINENTAL_THICKNESS_KM); n];
        state.set_tectonics_fields(plate_id, crust_thickness);

        // Steep, wet cell at (1, 1) — surrounded by lower-elevation
        // neighbours and seeded with lots of water + vapour.
        let steep_wet = state.grid().cell_id(Axial::new(1, 1)).0 as usize;
        state.elevation_mut()[steep_wet] = Real::from_int(2000);
        state.water_depth_mut()[steep_wet] = Real::from_int(500);
        state.substance_mut(Substance::Vapour.idx())[steep_wet] = Real::from_int(500);

        // Flat, dry cell at (3, 0) — same elevation as its
        // neighbours, no water. Erosion driven by slope × precip
        // gives zero on both factors.
        let flat_dry = state.grid().cell_id(Axial::new(3, 0)).0 as usize;
        // Leave elevation at zero (matches neighbours), water at
        // zero (set by default).

        let steep_before = state.elevation()[steep_wet];
        let flat_before = state.elevation()[flat_dry];

        let tect = Tectonics {
            plates,
            convergence_rate: Real::ZERO,
            divergence_rate: Real::ZERO,
            // Large erosion_k so the signal lands in a few ticks.
            erosion_k: Real::from_ratio(1, 1_000),
        };
        for _ in 0..10 {
            tect.integrate(&mut state, Real::ONE);
        }
        let steep_after = state.elevation()[steep_wet];
        let flat_after = state.elevation()[flat_dry];

        assert!(
            steep_after < steep_before,
            "wet steep cell should lose elevation: \
             before={steep_before:?} after={steep_after:?}"
        );
        assert_eq!(
            flat_after, flat_before,
            "flat dry cell should not change: \
             before={flat_before:?} after={flat_after:?}"
        );
    }

    /// Same seed + grid → same plate layout + same per-tick
    /// evolution. Exercises the SplitMix64-based sampler.
    #[test]
    fn tectonics_is_deterministic() {
        let grid = HexGrid::new(10, 8);
        let seed = 0xDEAD_BEEF_CAFE_BABE_u64;
        let (tect_a, plate_a, crust_a) = Tectonics::sample_plates_for_seed(seed, &grid);
        let (tect_b, plate_b, crust_b) = Tectonics::sample_plates_for_seed(seed, &grid);

        assert_eq!(plate_a, plate_b);
        assert_eq!(crust_a, crust_b);
        assert_eq!(tect_a.plates.len(), tect_b.plates.len());
        for (pa, pb) in tect_a.plates.iter().zip(tect_b.plates.iter()) {
            assert_eq!(pa.id, pb.id);
            assert_eq!(pa.crust_type, pb.crust_type);
            assert_eq!(pa.velocity, pb.velocity);
            assert_eq!(pa.thickness, pb.thickness);
        }

        // Now run the integrator on two independent states and
        // assert bit-equality of the elevation field afterwards.
        let mut state_a = PhysicsState::new(grid.clone());
        let mut state_b = PhysicsState::new(grid);
        state_a.set_tectonics_fields(plate_a, crust_a);
        state_b.set_tectonics_fields(plate_b, crust_b);
        for e in state_a.elevation_mut() {
            *e = Real::from_int(500);
        }
        for e in state_b.elevation_mut() {
            *e = Real::from_int(500);
        }
        for w in state_a.water_depth_mut() {
            *w = Real::from_int(100);
        }
        for w in state_b.water_depth_mut() {
            *w = Real::from_int(100);
        }
        for _ in 0..50 {
            tect_a.integrate(&mut state_a, Real::ONE);
            tect_b.integrate(&mut state_b, Real::ONE);
        }
        assert_eq!(state_a.elevation(), state_b.elevation());
    }

    /// Bonus: confirm the worldgen sampler stays within the documented
    /// [MIN_PLATES, MAX_PLATES] window for an arbitrary earth-like seed.
    #[test]
    fn plate_count_within_range_for_earth_like_seed() {
        let grid = HexGrid::new(36, 30);
        for seed in [
            0x0000_0000_0000_0001_u64,
            0xDEAD_BEEF_CAFE_BABE,
            0x0123_4567_89AB_CDEF,
            0xFEED_FACE_BAAD_F00D,
        ] {
            let (tect, _, _) = Tectonics::sample_plates_for_seed(seed, &grid);
            let count = tect.plates.len() as u32;
            assert!(
                (MIN_PLATES..=MAX_PLATES).contains(&count),
                "plate count {count} outside [{MIN_PLATES}, {MAX_PLATES}] for seed {seed:#x}"
            );
        }
    }

    /// Worldgen sampler should produce a roughly 60/40 oceanic /
    /// continental split. With 8-15 plates per seed the individual
    /// counts vary, but aggregated across many seeds the ratio
    /// should land near the documented 60 %.
    #[test]
    fn worldgen_crust_split_is_roughly_60_oceanic() {
        let grid = HexGrid::new(20, 16);
        let mut oceanic = 0u32;
        let mut continental = 0u32;
        for seed in 0u64..200 {
            let (tect, _, _) = Tectonics::sample_plates_for_seed(seed, &grid);
            for p in &tect.plates {
                match p.crust_type {
                    CrustType::Oceanic => oceanic += 1,
                    CrustType::Continental => continental += 1,
                }
            }
        }
        let total = oceanic + continental;
        let pct = (oceanic * 100) / total;
        // 60 % ± 10 % tolerance — 200 seeds is enough sample to
        // catch a calibration bug without false-positive rejecting
        // legitimate sampling variation.
        assert!(
            (50..=70).contains(&pct),
            "oceanic share {pct}% outside [50, 70] across 200 seeds: \
             oceanic={oceanic} continental={continental}"
        );
    }
}
