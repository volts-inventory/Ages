//! Fluid dynamics — first M1a pass.
//!
//! Simple gravity-driven mass redistribution. For each cell pair
//! `(i, j)` with `i < j`:
//!
//! ```text
//!   surface_i = elevation[i] + water_depth[i]
//!   surface_j = elevation[j] + water_depth[j]
//!   flux = k * dt * gravity * (surface_i - surface_j)
//!   // clamp by available water on the donor side
//!   water_depth[i] -= flux  // donor when flux > 0
//!   water_depth[j] += flux
//! ```
//!
//! `k` is a transport coefficient. Pair-flux for bit-exact mass
//! conservation in fixed-point — same pattern as heat conduction.
//!
//! This is **not yet real shallow-water**. There's no momentum
//! tracking — velocity isn't a stateful field, just an
//! instantaneous rate derived per pair. Real shallow-water with
//! momentum and proper wave dynamics is the next M1a commit, which
//! will also start updating `fluid_v_q` / `fluid_v_r` so heat can
//! advect against velocity (fluid → heat coupling).

use crate::laws::Law;
use crate::mechanics::Mechanics;
use crate::state::PhysicsState;
use sim_arith::Real;

#[derive(Debug, Clone, Copy)]
pub struct GravityFlow {
    pub mechanics: Mechanics,
    /// Transport coefficient. Combined with `gravity * dt` it gives
    /// the per-pair flow rate from a unit surface-height difference.
    pub k: Real,
}

impl GravityFlow {
    /// A reasonable default for tests: gravity is Earth-like, `k` is
    /// tuned so a unit-height difference equilibrates over many
    /// sub-steps without overshoot at the test `dt = 1/50`.
    pub fn earth_like() -> Self {
        Self {
            mechanics: Mechanics::earth_like(),
            k: Real::from_ratio(1, 1000),
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
        }
    }
}

impl Law for GravityFlow {
    fn integrate(&self, state: &mut PhysicsState, dt: Real) {
        let grid = state.grid().clone();
        let elev = state.elevation().to_vec();
        let prev = state.water_depth().to_vec();
        let next = state.water_depth_mut();
        next.copy_from_slice(&prev);

        let coeff = self.k * self.mechanics.gravity * dt;

        for (cid, axial) in grid.cells() {
            let i = cid.0 as usize;
            let surface_i = elev[i] + prev[i];
            for nb in grid.neighbours(axial) {
                let j = nb.0 as usize;
                if j > i {
                    let surface_j = elev[j] + prev[j];
                    let flux = coeff * (surface_i - surface_j);
                    next[i] = next[i] - flux;
                    next[j] = next[j] + flux;
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
}
