//! Fluid dynamics — first M1a pass.
//!
//! Two transport terms now combine in `GravityFlow::integrate`:
//!
//! 1. **Gravity-driven equilibration.** For each cell pair `(i, j)`
//!    with `j > i`:
//!    ```text
//!      surface_i = elevation[i] + water_depth[i]
//!      surface_j = elevation[j] + water_depth[j]
//!      flux_g    = k · dt · gravity · (surface_i - surface_j)
//!      water_depth[i] -= flux_g  // donor when flux_g > 0
//!      water_depth[j] += flux_g
//!    ```
//! 2. **Wind-coupled advection.** A minimal momentum-carrying term
//!    couples the water-depth field to the wind velocity already
//!    present in `state.fluid_velocity()`:
//!    ```text
//!      v_along       = midpoint(v[i], v[j]) · dir(i → j)
//!      donor         = if v_along > 0 { i } else { j }
//!      flux_m        = momentum_k · dt · v_along · water_depth[donor]
//!      water_depth[i] -= flux_m
//!      water_depth[j] += flux_m
//!    ```
//!    This is the compromise short of a real shallow-water solver
//!    (a multi-day rewrite): it lets currents and rivers move
//!    *with* the wind, not just toward terrain depressions, while
//!    preserving pair-flux mass conservation. `momentum_k` is small
//!    enough to keep the additional term inside the same CFL bound
//!    as `k · gravity`; the upwind donor choice keeps it
//!    monotone-stable when the velocity field is well-behaved.
//!
//! `k` and `momentum_k` are transport coefficients. Pair-flux for
//! bit-exact mass conservation in fixed-point — same pattern as
//! heat conduction.

use crate::laws::Law;
use crate::mechanics::Mechanics;
use crate::state::PhysicsState;
use sim_arith::Real;

/// Hex-direction axial offsets for the six neighbours, in the same
/// canonical order as `HexGrid::neighbours` (E, NE, NW, W, SW, SE).
/// Mirrors the constant in `wind::NEIGHBOUR_DIRECTIONS`; replicated
/// to avoid a cross-module dependency on a private constant.
const NEIGHBOUR_DIRECTIONS: [(i64, i64); 6] = [
    (1, 0),  // E
    (1, -1), // NE
    (0, -1), // NW
    (-1, 0), // W
    (-1, 1), // SW
    (0, 1),  // SE
];

#[derive(Debug, Clone, Copy)]
pub struct GravityFlow {
    pub mechanics: Mechanics,
    /// Transport coefficient for the gravity-driven equilibration
    /// pass. Combined with `gravity * dt` it gives the per-pair flow
    /// rate from a unit surface-height difference.
    pub k: Real,
    /// Coupling strength of the wind-velocity advection term that
    /// moves water *with* the wind (not just toward terrain
    /// depressions). Multiplies the upwind donor depth × midpoint
    /// velocity to give per-pair flux. Small relative to `k` so the
    /// CFL bound on the combined pass stays the same as the legacy
    /// gravity-only pass; turn off (`Real::ZERO`) to recover the
    /// pre-coupling behaviour exactly.
    ///
    /// Pair-flux preserves mass conservation: the same `flux` is
    /// applied with opposite signs to donor and acceptor, so Σ
    /// water_depth is conserved bit-exactly.
    pub momentum_k: Real,
}

impl GravityFlow {
    /// A reasonable default for tests: gravity is Earth-like, `k` is
    /// tuned so a unit-height difference equilibrates over many
    /// sub-steps without overshoot at the test `dt = 1/50`.
    pub fn earth_like() -> Self {
        Self {
            mechanics: Mechanics::earth_like(),
            k: Real::from_ratio(1, 1000),
            // Order of magnitude below `k · gravity` (≈ 0.01 with
            // Earth gravity) so the wind-coupling term stays inside
            // the CFL bound on small grids while still letting
            // currents form when velocity is non-trivial.
            momentum_k: Real::from_ratio(1, 1000),
        }
    }

    /// Build from a planet's mechanics. Transport coefficient `k`
    /// stays fixed for now (a fluid-property thing rather than a
    /// gravity-dependent one); composition could later vary it
    /// (viscosity).
    pub fn from_mechanics(mechanics: Mechanics) -> Self {
        Self {
            mechanics,
            k: Real::from_ratio(1, 1000),
            momentum_k: Real::from_ratio(1, 1000),
        }
    }
}

impl Law for GravityFlow {
    // Axial coordinates use the canonical `q` / `r` naming; pair-
    // named bindings like `vel_q_*` / `vel_r_*` trip clippy's
    // `similar_names` lint despite being the natural domain
    // vocabulary.
    #[allow(clippy::similar_names)]
    fn integrate(&self, state: &mut PhysicsState, dt: Real) {
        let grid = state.grid().clone();
        let elev = state.elevation().to_vec();
        let prev = state.water_depth().to_vec();
        // Snapshot the wind velocity field so the upwind momentum
        // term reads previous-tick values (matches the snapshot
        // discipline used by `Wind::integrate` for temperature).
        let (vel_q_prev, vel_r_prev) = {
            let (q, r) = state.fluid_velocity();
            (q.to_vec(), r.to_vec())
        };
        let next = state.water_depth_mut();
        next.copy_from_slice(&prev);

        let gravity_coeff = self.k * self.mechanics.gravity * dt;
        let two = Real::from_int(2);

        for (cid, axial) in grid.cells() {
            let i = cid.0 as usize;
            let surface_i = elev[i] + prev[i];
            for (k, nb) in grid.neighbours(axial).iter().enumerate() {
                let j = nb.0 as usize;
                if j > i {
                    // Gravity-driven equilibration: high-surface →
                    // low-surface mass redistribution.
                    let surface_j = elev[j] + prev[j];
                    let flux_g = gravity_coeff * (surface_i - surface_j);
                    next[i] = next[i] - flux_g;
                    next[j] = next[j] + flux_g;

                    // Wind-coupled advection: water rides along with
                    // the wind. Skip when momentum_k = 0 so the
                    // hot path stays as cheap as the legacy version
                    // when wind coupling is disabled.
                    if self.momentum_k == Real::ZERO {
                        continue;
                    }
                    let (dir_q, dir_r) = NEIGHBOUR_DIRECTIONS[k];
                    let vmid_q = (vel_q_prev[i] + vel_q_prev[j]) / two;
                    let vmid_r = (vel_r_prev[i] + vel_r_prev[j]) / two;
                    let v_along =
                        vmid_q * Real::from_int(dir_q) + vmid_r * Real::from_int(dir_r);
                    // Upwind donor selection: positive v_along means
                    // flow i → j, so the donor is `i`. The flux moves
                    // donor's water; the receiver receives whatever
                    // the donor releases.
                    let donor_depth = if v_along > Real::ZERO {
                        prev[i]
                    } else {
                        prev[j]
                    };
                    let flux_m = self.momentum_k * dt * v_along * donor_depth;
                    next[i] = next[i] - flux_m;
                    next[j] = next[j] + flux_m;
                }
            }
        }

        // Ensure water_depth never goes negative. Discrete pair-flux
        // can briefly drive a cell negative when the per-step flux
        // exceeds available water; that's a numerical artefact, not
        // physics. Clamp in place; mass loss from this clamp is
        // tiny when dt is CFL-respecting and flagged as a known
        // limitation of this simple model. Real shallow-water with
        // momentum tracking (next commit) eliminates the issue.
        for w in next.iter_mut() {
            if *w < Real::ZERO {
                *w = Real::ZERO;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::{Axial, HexGrid};

    #[test]
    fn water_flows_downhill() {
        // Two-tier terrain: high cell at (1,1), low cell at (2,1).
        // Fill the high cell with water; check it drains toward the
        // low cell after some sub-steps.
        let grid = HexGrid::new(4, 3);
        let mut state = PhysicsState::new(grid);
        let high = state.grid().cell_id(Axial::new(1, 1));
        let low = state.grid().cell_id(Axial::new(2, 1));
        state.elevation_mut()[high.0 as usize] = Real::from_int(10);
        state.water_depth_mut()[high.0 as usize] = Real::from_int(5);

        let law = GravityFlow::earth_like();
        for _ in 0..50 {
            law.integrate(&mut state, Real::from_ratio(1, 50));
        }

        assert!(
            state.water_depth()[low.0 as usize] > Real::ZERO,
            "water should have flowed from high cell to low cell"
        );
    }

    #[test]
    fn gravity_flow_is_deterministic() {
        let grid = HexGrid::new(5, 5);
        let mut a = PhysicsState::new(grid.clone());
        let mut b = PhysicsState::new(grid);

        let high_a = a.grid().cell_id(Axial::new(2, 2));
        let high_b = b.grid().cell_id(Axial::new(2, 2));
        a.elevation_mut()[high_a.0 as usize] = Real::from_int(10);
        b.elevation_mut()[high_b.0 as usize] = Real::from_int(10);
        a.water_depth_mut()[high_a.0 as usize] = Real::from_int(5);
        b.water_depth_mut()[high_b.0 as usize] = Real::from_int(5);

        let law = GravityFlow::earth_like();
        for _ in 0..20 {
            law.integrate(&mut a, Real::from_ratio(1, 50));
            law.integrate(&mut b, Real::from_ratio(1, 50));
        }
        assert_eq!(a.water_depth(), b.water_depth());
    }

    #[test]
    fn equal_surface_means_no_flow() {
        // Flat terrain, equal water everywhere → nothing should move.
        // Velocity left at its initial zero so the wind-coupling term
        // also contributes nothing.
        let grid = HexGrid::new(4, 4);
        let mut state = PhysicsState::new(grid);
        for w in state.water_depth_mut().iter_mut() {
            *w = Real::from_int(3);
        }
        let initial: Vec<_> = state.water_depth().to_vec();

        let law = GravityFlow::earth_like();
        for _ in 0..10 {
            law.integrate(&mut state, Real::from_ratio(1, 50));
        }
        assert_eq!(state.water_depth(), &initial[..]);
    }

    #[test]
    fn gravity_flow_couples_to_wind_velocity() {
        // Flat terrain + a uniform water column + a uniform eastward
        // wind. Without wind coupling water would stay put forever.
        // With coupling the water rides with the wind; an upstream
        // cell (west of the seeded depth gradient) ends up wetter
        // after some ticks than its mirror-image counterpart would
        // be under the gravity-only solver. The cleanest assertion
        // is positive: a single seeded "wet patch" shifts mass in
        // the wind direction, so the eastern neighbour gains water
        // and the western neighbour loses water relative to the
        // gravity-only baseline.
        let width = 6_u32;
        let height = 1_u32;
        let grid = HexGrid::new(width, height);

        // Baseline state: a single wet patch in the middle. Wind off.
        let mut baseline = PhysicsState::new(grid.clone());
        let wet = baseline.grid().cell_id(Axial::new(2, 0)).0 as usize;
        baseline.water_depth_mut()[wet] = Real::from_int(10);
        let mut gravity_only = GravityFlow::earth_like();
        gravity_only.momentum_k = Real::ZERO;
        for _ in 0..30 {
            gravity_only.integrate(&mut baseline, Real::from_ratio(1, 50));
        }

        // Wind-coupled state: identical seed, plus a uniform eastward
        // wind. The east-neighbour direction in `NEIGHBOUR_DIRECTIONS`
        // is `(1, 0)` so vel_q > 0 advects water in the +q direction.
        let mut windy = PhysicsState::new(grid);
        windy.water_depth_mut()[wet] = Real::from_int(10);
        for v in windy.fluid_velocity_mut().0.iter_mut() {
            *v = Real::from_int(5);
        }
        let law = GravityFlow::earth_like();
        for _ in 0..30 {
            law.integrate(&mut windy, Real::from_ratio(1, 50));
        }

        // East-neighbour cell index in the 1-row grid: wet + 1.
        // West-neighbour index: wet - 1.
        let east = wet + 1;
        let west = wet - 1;
        assert!(
            windy.water_depth()[east] > baseline.water_depth()[east],
            "wind-coupling should push water eastward beyond the \
             gravity-only baseline: windy.east={:?} baseline.east={:?}",
            windy.water_depth()[east],
            baseline.water_depth()[east]
        );
        assert!(
            windy.water_depth()[west] < baseline.water_depth()[west],
            "wind-coupling should pull water away from the western \
             cell relative to gravity-only: windy.west={:?} \
             baseline.west={:?}",
            windy.water_depth()[west],
            baseline.water_depth()[west]
        );

        // Mass conservation still holds under the coupling.
        let total_windy: Real = windy
            .water_depth()
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        let total_baseline: Real = baseline
            .water_depth()
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        assert_eq!(
            total_windy, total_baseline,
            "wind-coupled pair-flux must preserve total water mass \
             bit-exactly against the gravity-only baseline"
        );
    }
}
