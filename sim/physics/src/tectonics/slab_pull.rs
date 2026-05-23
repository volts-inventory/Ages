//! Slab-pull velocity dynamics (Sprint 4 Item 12e).
//!
//! Plate velocities are not static. Each tick, oceanic plates that
//! abut continental plates (or older oceanic plates once Item 12b
//! crust age is wired) accelerate *toward* the consuming boundary
//! under the integrated slab-pull force:
//!
//! ```text
//! slab_pull = Σ (slab_length × density_contrast × PULL_FACTOR)
//!             × (unit vector toward the subduction edge)
//! plate.velocity += slab_pull × dt
//! ```
//!
//! Velocity is capped per axis at `max_plate_velocity()` to keep the
//! simulation from running away under pathological boundary topology.
//! This module owns the per-tick force accumulation + the velocity
//! integration; `Tectonics::integrate` drives the order and feeds the
//! evolved velocities into the uplift / subduction passes downstream.

use super::plates::{CrustType, NEIGHBOUR_DIRECTIONS};
use super::Tectonics;
use crate::state::PhysicsState;
use sim_arith::Real;

/// Slab-pull coupling coefficient (Sprint 4 Item 12e). Multiplies
/// `slab_length × density_contrast` to give the per-tick velocity
/// kick. `1 / 10_000` keeps the response geological: an Earth-scale
/// 30-cell subduction zone at 0.22 contrast accumulates
/// ~6.6e-4 per axis per tick — many thousands of ticks to reach the
/// `MAX_PLATE_VELOCITY` cap, matching real plates' multi-million-year
/// acceleration timescales. Returned by a function (not a `const`)
/// because `Real::from_ratio` is not const-evaluable; the value
/// still resolves at link time and the call has no runtime cost.
#[must_use]
pub fn slab_pull_factor() -> Real {
    Real::from_ratio(1, 10_000)
}

/// Density contrast for an oceanic slab subducting beneath continental
/// crust (Sprint 4 Item 12e). Real-Earth oceanic basalt is ~3.0 g/cm³;
/// continental granite is ~2.7 g/cm³, so the fractional contrast is
/// ~0.10 — but the *effective* pull (oceanic minus mantle minus the
/// overriding plate's resistance) is higher because the overriding
/// plate transfers no buoyancy back through the trench. 0.22 sits in
/// the geophysical literature's range and is large enough to give a
/// visible signal in a few-tick test.
#[must_use]
pub fn slab_pull_density_contrast_oc_cont() -> Real {
    Real::from_ratio(22, 100)
}

/// Density contrast for an oceanic slab subducting beneath older
/// oceanic crust (Sprint 4 Item 12e). Both sides are basaltic so the
/// contrast is small — the cooler, denser older slab still wins, but
/// only marginally. Set to `0.05` per spec. Active once
/// `PhysicsState::crust_age` is populated (Sprint 4 Item 12b): the
/// per-cell side with the *greater* age is the diving slab, mirror
/// of the `Tectonics::pick_subducting_side` tie-break for oceanic-
/// oceanic boundaries.
#[must_use]
pub fn slab_pull_density_contrast_oc_oc() -> Real {
    Real::from_ratio(5, 100)
}

/// Per-axis velocity cap (Sprint 4 Item 12e). Without a cap, an
/// oceanic plate whose every edge is consuming would accelerate
/// without bound; the cap keeps the system stable at any boundary
/// topology. `5` (cells per macro-tick per axis) is 2.5× the
/// worldgen sampler's initial `[-2, +2]` window, leaving headroom
/// for slab-pull to meaningfully change the initial velocity while
/// preventing teleportation-scale drift.
#[must_use]
pub fn max_plate_velocity() -> Real {
    Real::from_int(5)
}

/// Lazily size `slab_pull_force` and `current_velocity` to match the
/// current plate roster. On first call `current_velocity[i]` is seeded
/// from `plates[i].velocity`; on subsequent calls the existing evolved
/// values are preserved and only the tail (if the roster grew) is
/// appended. Idempotent so the integrator can call it every tick
/// without paying for a full re-init.
pub(super) fn ensure_velocity_buffers(tect: &Tectonics) {
    let n = tect.plates.len();
    {
        let mut pull = tect.slab_pull_force.borrow_mut();
        if pull.len() != n {
            pull.resize(n, (Real::ZERO, Real::ZERO));
        }
    }
    {
        let mut vel = tect.current_velocity.borrow_mut();
        if vel.len() < n {
            for p in &tect.plates[vel.len()..] {
                vel.push(p.velocity);
            }
        } else if vel.len() > n {
            vel.truncate(n);
        }
    }
}

/// Phase 2 of the slab-pull pass: detect subducting edges and
/// accumulate per-plate force vectors. A subducting edge is a hex pair
/// `(i, j)` where:
///   - `plate_ids[i] != plate_ids[j]` (cross-plate)
///   - plate(i) is Oceanic AND plate(j) is Continental, OR
///   - both are Oceanic AND `crust_age[i] > crust_age[j]` so `i` is
///     the colder / denser / diving side.
///
/// The oceanic plate gets a force vector pointing *into* the
/// neighbour cell (toward the trench it's diving beneath). Density
/// contrast is `slab_pull_density_contrast_oc_cont()` (0.22) for
/// oceanic-continental and `slab_pull_density_contrast_oc_oc()`
/// (0.05) for the smaller age-driven contrast between two oceanic
/// plates. The `slab_length` is implicit in the per-edge
/// accumulation — each edge contributes once, so a longer subducting
/// boundary contributes proportionally more total force.
///
/// Iterates *all six* directions per cell so each oceanic-side cell
/// sees every neighbour edge it owns — slab-pull is per-edge with
/// direction, not per-unordered-pair. The opposite cell's pass picks
/// up the mirrored edge; that's intentional and preserves the spec's
/// "force vector toward the subduction edge" semantics on both sides
/// without an `if j > i` filter (the filter would halve the force on
/// the lower-index side).
///
/// Continental-continental boundaries do not accumulate slab-pull
/// (no slab dives at those edges; the uplift pass already drives
/// Himalayan thickening).
pub(super) fn accumulate_forces(tect: &Tectonics, state: &PhysicsState, plate_ids: &[u32]) {
    let grid = state.grid();
    let pull_factor = slab_pull_factor();
    let oc_cont = slab_pull_density_contrast_oc_cont();
    let oc_oc = slab_pull_density_contrast_oc_oc();
    // The crust-age field is length-matched once Item 12b's worldgen
    // sampler has populated it, otherwise an empty slice. Snapshot
    // once outside the loop so we don't hold a borrow on `state`
    // while also pushing into the per-plate pull accumulator.
    let crust_age: Vec<u64> = state.crust_age().to_vec();
    let crust_age_present = crust_age.len() == grid.n_cells();
    // Reset accumulator. Each tick's slab pull is fresh — the
    // *velocity* is the integrator (it carries history), the
    // force itself does not.
    {
        let mut pull = tect.slab_pull_force.borrow_mut();
        for f in pull.iter_mut() {
            *f = (Real::ZERO, Real::ZERO);
        }
    }
    for (cid, axial) in grid.cells() {
        let i = cid.0 as usize;
        let plate_i_id = plate_ids[i];
        let Some(pi) = tect.plate_for(plate_i_id) else {
            continue;
        };
        // Only the oceanic plate accumulates pull; the overriding
        // continental plate gets pushed *upward* (handled by the
        // existing convergent-uplift pass), not horizontally
        // toward the trench. Bailing here saves the inner
        // direction loop on the non-oceanic majority.
        if pi.crust_type != CrustType::Oceanic {
            continue;
        }
        for (k, nb) in grid.neighbours(axial).iter().enumerate() {
            let j = nb.0 as usize;
            let plate_j_id = plate_ids[j];
            if plate_i_id == plate_j_id {
                continue;
            }
            let Some(pj) = tect.plate_for(plate_j_id) else {
                continue;
            };
            let density_contrast = match pj.crust_type {
                CrustType::Continental => oc_cont,
                // Oceanic-vs-oceanic: only the older side dives.
                // Skip when no crust-age field exists yet (no Item
                // 12b worldgen sampler run), and skip when this
                // cell is the *younger* side (the other cell's
                // pass will accumulate the mirror force).
                CrustType::Oceanic => {
                    if !crust_age_present {
                        continue;
                    }
                    if crust_age[i] <= crust_age[j] {
                        continue;
                    }
                    oc_oc
                }
            };
            // Direction unit vector from `i` (oceanic) to `j`.
            // The pull on plate `i` points toward `j` — the slab
            // is being yanked into the subduction zone at that
            // edge.
            let (dir_q, dir_r) = NEIGHBOUR_DIRECTIONS[k];
            let dir_q_r = Real::from_int(dir_q);
            let dir_r_r = Real::from_int(dir_r);
            let force_step = pull_factor * density_contrast;
            let mut pull = tect.slab_pull_force.borrow_mut();
            let cur = pull[plate_i_id as usize];
            pull[plate_i_id as usize] = (
                cur.0 + force_step * dir_q_r,
                cur.1 + force_step * dir_r_r,
            );
        }
    }
}

/// Phase 3 of the slab-pull pass: apply the accumulated forces to
/// the evolved velocity, clamped per axis at `max_plate_velocity()`.
pub(super) fn apply_forces(tect: &Tectonics, dt: Real) {
    let cap = max_plate_velocity();
    let neg_cap = Real::ZERO - cap;
    let pull = tect.slab_pull_force.borrow();
    let mut vel = tect.current_velocity.borrow_mut();
    for (idx, v) in vel.iter_mut().enumerate() {
        let f = pull[idx];
        let new_q = v.0 + f.0 * dt;
        let new_r = v.1 + f.1 * dt;
        *v = (
            new_q.max(neg_cap).min(cap),
            new_r.max(neg_cap).min(cap),
        );
    }
}
