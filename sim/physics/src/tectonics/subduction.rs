//! Subduction logic (Sprint 4 Item 12a, substrate-aware F6).
//!
//! Owns the side-picking tie-break, the per-tick crust-thickness
//! decay at convergent oceanic boundaries, and the cell-id reassign
//! step that flips a consumed oceanic cell onto the overriding plate.
//! The aggregate `subducted_mass` accessor that wires into the Item
//! 12d volcanism pool also lives here.

use super::plates::{CrustType, Plate, NEIGHBOUR_DIRECTIONS, OCEANIC_THICKNESS_KM};
use super::Tectonics;
use crate::state::PhysicsState;
use sim_arith::Real;

/// Number of ticks an oceanic cell at a convergent boundary takes to
/// decay from its initial thickness to zero (Sprint 4 Item 12a). The
/// per-tick decrement is `OCEANIC_THICKNESS_KM / SUBDUCTION_DT_TICKS`
/// — fixed regardless of convergence speed, so the geological
/// timescale of consumption is the same across worlds (tunable here
/// in one place rather than entangled with per-plate velocities).
pub const SUBDUCTION_DT_TICKS: i64 = 100;

/// Minimum crust thickness in km-equivalent below which a subducting
/// oceanic cell flips ownership to the overriding (continental, or
/// lower-id oceanic) plate. Picked at 1 km so the cell still has a
/// non-trivial thickness when reassigned — the alternative of
/// "exactly zero" would leave a dangling cell with no crust which
/// would confuse Item 12c isostasy when it lands.
pub const MIN_CRUST_THICKNESS_KM: i64 = 1;

/// Decide which side of a convergent oceanic-bearing boundary
/// subducts. Returns `(subducting_idx, overriding_idx)` chosen
/// from the input pair `(i, j)`, or `None` for boundaries that
/// don't subduct (continental-continental: existing uplift logic
/// keeps running unchanged).
///
/// Tie-break rules:
/// - Oceanic vs continental → oceanic subducts (denser).
/// - Oceanic vs oceanic → higher-`id` plate's cell subducts.
///   Real geology picks "older / colder / denser"; Item 12b adds
///   per-cell crust age, after which we'll switch this rule.
///   Until then, plate id is a stable per-run proxy.
/// - Continental vs continental → `None`. No subduction; the
///   uplift path in `integrate` does Himalayan-style thickening.
pub(super) fn pick_subducting_side(
    pi: &Plate,
    pj: &Plate,
    i: usize,
    j: usize,
) -> Option<(usize, usize)> {
    match (pi.crust_type, pj.crust_type) {
        (CrustType::Continental, CrustType::Continental) => None,
        (CrustType::Oceanic, CrustType::Continental) => Some((i, j)),
        (CrustType::Continental, CrustType::Oceanic) => Some((j, i)),
        (CrustType::Oceanic, CrustType::Oceanic) => {
            // Higher-id plate is the proxy-older plate; its cell
            // subducts under the lower-id plate. Equal ids
            // shouldn't reach here (same plate filtered upstream)
            // but if they somehow do, fall through to "no
            // subduction" rather than panicking.
            if pi.id > pj.id {
                Some((i, j))
            } else if pj.id > pi.id {
                Some((j, i))
            } else {
                None
            }
        }
    }
}

/// Run the subduction pass. Two-phase:
///
/// Pass 1 (collect): for every convergent boundary pair where at
/// least one side is oceanic, decide which side subducts and record
/// the overriding plate id for the subducting cell. Collected into
/// `BTreeMap<cell_idx, overriding_plate_id>` so a cell that borders
/// multiple convergent boundaries resolves to a single deterministic
/// overriding plate (first writer wins, walking pairs in canonical
/// grid order).
///
/// Pass 2 (apply): for each marked cell, decrement its crust thickness
/// by a fixed `OCEANIC_THICKNESS_KM / SUBDUCTION_DT_TICKS` step per dt
/// unit, depositing the lost mass into the aggregate `subducted_mass`
/// pool. Once the thickness drops below `MIN_CRUST_THICKNESS_KM`,
/// reassign the cell's `plate_id` to the recorded overriding plate.
///
/// Continental-continental boundaries are untouched (the uplift step
/// above already drove the Himalayan thickening path). Oceanic-oceanic
/// is handled with a plate-id tie-break for Item 12a; Item 12b will
/// swap that for proper crust age.
pub(super) fn run(
    tect: &Tectonics,
    state: &mut PhysicsState,
    dt: Real,
    velocities: &[(Real, Real)],
) {
    if state.crust_thickness().len() != state.grid().n_cells() {
        return;
    }

    let grid = state.grid().clone();
    let elevation_snapshot = state.elevation().to_vec();
    let crust_thickness_in = state.crust_thickness().to_vec();
    let plate_ids_now = state.plate_id().to_vec();

    // `BTreeMap` keeps iteration deterministic in pass 2 even though
    // per-cell decay is independent. The map also gives us a clean
    // "first writer wins" semantic via `or_insert` for cells touching
    // multiple boundaries.
    let mut subducting: std::collections::BTreeMap<usize, u32> =
        std::collections::BTreeMap::new();

    for (cid, axial) in grid.cells() {
        let i = cid.0 as usize;
        let plate_i = plate_ids_now[i];
        for (k, nb) in grid.neighbours(axial).iter().enumerate() {
            let j = nb.0 as usize;
            if j <= i {
                continue;
            }
            let plate_j = plate_ids_now[j];
            if plate_i == plate_j {
                continue;
            }
            let (pi, pj) = match (
                tect.plate_for(plate_i),
                tect.plate_for(plate_j),
            ) {
                (Some(a), Some(b)) => (a, b),
                _ => continue,
            };
            // Convergence test reuses the uplift step's projection:
            // negative projection = i and j approach along the i→j
            // direction. Reads from `velocities` (Item 12e evolved)
            // so slab-pull can drive an initially-parallel pair into
            // convergence on the same tick subduction kicks in — the
            // feedback loop.
            let (dir_q, dir_r) = NEIGHBOUR_DIRECTIONS[k];
            let vel_i = velocities[plate_i as usize];
            let vel_j = velocities[plate_j as usize];
            let rel_q = vel_j.0 - vel_i.0;
            let rel_r = vel_j.1 - vel_i.1;
            let projection =
                rel_q * Real::from_int(dir_q) + rel_r * Real::from_int(dir_r);
            if projection >= Real::ZERO {
                continue;
            }
            // Subducting side and the plate id that wins the cell
            // once it's consumed.
            let (sub_idx, over_idx) =
                match pick_subducting_side(pi, pj, i, j) {
                    Some(pair) => pair,
                    None => continue,
                };
            let overriding_plate = plate_ids_now[over_idx];
            // First writer wins so cells at multi-boundary corners
            // get a stable assignment.
            subducting.entry(sub_idx).or_insert(overriding_plate);
        }
    }

    if subducting.is_empty() {
        return;
    }

    // Per-tick decrement: a fresh oceanic cell takes
    // `SUBDUCTION_DT_TICKS` ticks at `dt = 1` to drop below the
    // reassignment threshold. The decrement scales linearly with `dt`
    // so accelerated tests (large dt) consume in proportionally fewer
    // ticks.
    let decrement_per_tick =
        Real::from_ratio(OCEANIC_THICKNESS_KM, SUBDUCTION_DT_TICKS);
    let decrement = decrement_per_tick * dt;
    let min_thickness = Real::from_int(MIN_CRUST_THICKNESS_KM);

    let mut crust_thickness = crust_thickness_in.clone();
    let mut plate_ids = plate_ids_now.clone();
    let mut elevation_out = elevation_snapshot.clone();
    let mut total_subducted = Real::ZERO;

    for (&cell, &overriding_plate) in subducting.iter() {
        let before = crust_thickness[cell];
        let after = (before - decrement).max(Real::ZERO);
        let lost = before - after;
        if lost > Real::ZERO {
            total_subducted = total_subducted + lost;
        }
        crust_thickness[cell] = after;
        // Reassign once thickness has fallen below the floor. Drop
        // the cell's elevation toward zero at the same time so the
        // next tectonic step doesn't see a residual "ghost mountain"
        // left behind by the consumed oceanic crust. Clamp at zero —
        // Item 12c isostasy will lift this to a proper bathymetry
        // depth once it lands.
        if after < min_thickness {
            plate_ids[cell] = overriding_plate;
            elevation_out[cell] = Real::ZERO;
        }
    }

    if total_subducted > Real::ZERO {
        let pool = state.subducted_mass_mut();
        *pool = *pool + total_subducted;
    }
    state.crust_thickness_mut().copy_from_slice(&crust_thickness);
    state.plate_id_mut().copy_from_slice(&plate_ids);
    state.elevation_mut().copy_from_slice(&elevation_out);
}
