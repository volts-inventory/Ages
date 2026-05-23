//! `Law::integrate` implementation for `Tectonics` — the per-tick
//! phase orchestrator.
//!
//! Sequences the sub-passes in the order each one needs the previous
//! pass's outputs:
//!
//! 1. Slab-pull velocity update (lazy buffer init → force accumulate →
//!    apply with clamp).
//! 2. Tectonic uplift / divergence at plate boundaries (reads evolved
//!    velocities).
//! 3. Fluvial erosion (in-plate pairs only; boundary pairs already
//!    handled in step 2).
//! 4. Crust age + ridge-cooling depth (ridge cells from step 2 stay at
//!    age 0; everything else accumulates).
//! 5. Subduction (reads evolved velocities + post-uplift state).
//! 6. Airy isostasy (final lift / sink against the post-tectonic
//!    thickness column).
//!
//! Split out of `mod.rs` so the orchestration sequence reads as a
//! single function, with the `Tectonics` data model in `state.rs` and
//! sub-pass implementations in their respective sibling modules.

use super::state::Tectonics;
use super::{erosion, slab_pull, subduction};
use crate::laws::Law;
use crate::state::PhysicsState;
use sim_arith::Real;

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
        // Sprint 4 Item 12b. Track which cells participate in a
        // divergent boundary this tick. These cells are at a ridge —
        // their crust is freshly emplaced and the per-cell age is
        // reset to zero below. Every other cell ages by one tick.
        // Bool-mask rather than a `HashSet<usize>` so the inner loop
        // can flip bits with a flat index — same shape used by the
        // ice / snow fraction maps.
        let n = grid.n_cells();
        let mut at_ridge = vec![false; n];

        // ---- Slab-pull velocity update (Sprint 4 Item 12e). ----
        //
        // Phase 1: lazily size the evolved-velocity and slab-pull-force
        // vectors to match the current plate roster. The roster is
        // immutable per tick but a future sub-item (e.g. plate merge
        // after full subduction) may grow / shrink it across ticks;
        // we re-extend defensively on every call rather than locking
        // the vectors at `earth_like()` time.
        slab_pull::ensure_velocity_buffers(self);

        // Phase 2: detect subducting edges and accumulate per-plate
        // slab-pull force vectors.
        slab_pull::accumulate_forces(self, state, &plate_ids);

        // Phase 3: apply force to evolved velocity, clamped per axis.
        slab_pull::apply_forces(self, dt);

        // Snapshot evolved velocities once for the convergent /
        // subduction passes below. Holding the borrow across those
        // passes would force a `try_borrow` discipline I'd rather
        // not impose on this code path; the snapshot copy is small
        // (one `(Real, Real)` per plate).
        let velocities = self.current_velocity.borrow().clone();

        // ---- Tectonic uplift / divergence at plate boundaries. ----
        erosion::run_uplift_divergence(
            self,
            &grid,
            &plate_ids,
            &velocities,
            &mut elevation,
            &mut at_ridge,
            dt,
        );

        // ---- Fluvial erosion. ----
        erosion::run_fluvial(self, &grid, &plate_ids, &mut elevation, &water, &vapour, dt);

        // ---- Crust age update + ocean-floor ridge-cooling depth. ----
        erosion::run_age_and_cooling(self, state, &plate_ids, &mut elevation, &at_ridge, dt);

        state.elevation_mut().copy_from_slice(&elevation);

        // ---- Subduction (Sprint 4 Item 12a). ----
        subduction::run(self, state, dt, &velocities);

        // ---- Airy isostasy (Sprint 4 Item 12c). ----
        //
        // Re-balance surface elevation against the post-tectonic /
        // post-erosion / post-subduction crust thickness so a
        // thickened column (convergent uplift) lifts and a thinned
        // column (subduction consumption) sinks. The pass is a
        // no-op when nothing changed since the previous one — see
        // `isostasy::apply_isostasy` docs for the lazy-bake +
        // delta-form details.
        //
        // Runs *after* every other tectonic sub-step so external
        // elevation deltas (convergent kick, erosion redistribution,
        // subduction clamp-to-zero) flow into the isostatic baseline
        // this pass — they pass through unchanged while thickness
        // changes get scaled by the Airy lift coefficient.
        crate::isostasy::apply_isostasy(state);
    }
}
