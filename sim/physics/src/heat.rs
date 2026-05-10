//! Heat conduction — first M1a law family. Pair-flux diffusion for
//! bit-exact conservation. Far-future enhancements include latent
//! heat at phase boundaries (water → ice, etc.) and proper insolation
//! modelling tied to planet axial tilt and orbital position.
//!
//! Pair-flux formulation. For each unique cell pair `(i, j)` with
//! `i < j`:
//!
//! ```text
//!   delta = alpha * dt * (T[j] - T[i])
//!   T[i] += delta
//!   T[j] -= delta
//! ```
//!
//! The `delta` is computed once per pair, then applied to both cells
//! with opposite signs. In real arithmetic this is the standard
//! explicit-Euler diffusion stencil; in fixed-point arithmetic
//! it preserves total heat **bit-exactly** because the
//! truncation in `delta` cancels between `+delta` and `-delta`.
//!
//! `alpha` is the thermal-diffusivity coefficient. Stable when
//! `alpha * dt * 6 < 1` per cell — checked implicitly by the
//! per-family CFL-bound dt selection.

use crate::laws::Law;
use crate::state::PhysicsState;
use sim_arith::Real;

#[derive(Debug, Clone, Copy)]
pub struct HeatConduction {
    pub alpha: Real,
}

impl Law for HeatConduction {
    fn integrate(&self, state: &mut PhysicsState, dt: Real) {
        let grid = state.grid().clone();
        let prev = state.temperature().to_vec();
        let next = state.temperature_mut();
        next.copy_from_slice(&prev);

        for (cid, axial) in grid.cells() {
            let i = cid.0 as usize;
            for nb in grid.neighbours(axial) {
                let j = nb.0 as usize;
                if j > i {
                    let delta = self.alpha * dt * (prev[j] - prev[i]);
                    next[i] = next[i] + delta;
                    next[j] = next[j] - delta;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::{Axial, HexGrid};

    #[test]
    fn diffusion_smooths_a_step() {
        // Initialise a 3x3 torus with one hot cell; diffusion should
        // spread heat to neighbours after one step.
        let grid = HexGrid::new(3, 3);
        let mut state = PhysicsState::new(grid);
        let centre = state.grid().cell_id(Axial::new(1, 1));
        state.temperature_mut()[centre.0 as usize] = Real::from_int(60);

        let law = HeatConduction {
            alpha: Real::from_ratio(1, 10),
        };
        law.integrate(&mut state, Real::ONE);

        // Centre lost heat; at least one neighbour gained.
        assert!(state.temperature()[centre.0 as usize] < Real::from_int(60));
        let n0 = state.grid().neighbours(Axial::new(1, 1))[0].0 as usize;
        assert!(state.temperature()[n0] > Real::ZERO);
    }

    #[test]
    fn diffusion_is_deterministic() {
        let grid = HexGrid::new(4, 4);
        let mut state_a = PhysicsState::new(grid.clone());
        let mut state_b = PhysicsState::new(grid);
        // Same initial condition.
        let centre_a = state_a.grid().cell_id(Axial::new(2, 2));
        let centre_b = state_b.grid().cell_id(Axial::new(2, 2));
        state_a.temperature_mut()[centre_a.0 as usize] = Real::from_int(100);
        state_b.temperature_mut()[centre_b.0 as usize] = Real::from_int(100);

        let law = HeatConduction {
            alpha: Real::from_ratio(1, 10),
        };
        for _ in 0..10 {
            law.integrate(&mut state_a, Real::ONE);
            law.integrate(&mut state_b, Real::ONE);
        }
        assert_eq!(state_a.temperature(), state_b.temperature());
    }

    #[test]
    fn diffusion_conserves_total_heat() {
        let grid = HexGrid::new(3, 3);
        let mut state = PhysicsState::new(grid);
        let centre = state.grid().cell_id(Axial::new(1, 1));
        state.temperature_mut()[centre.0 as usize] = Real::from_int(60);

        let initial: Real = state
            .temperature()
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);

        let law = HeatConduction {
            alpha: Real::from_ratio(1, 10),
        };
        for _ in 0..10 {
            law.integrate(&mut state, Real::ONE);
        }

        let after: Real = state
            .temperature()
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);

        // Discrete diffusion stencil with `T_new = T + alpha * dt *
        // sum(neighbour - here)` conserves total heat exactly: every
        // pairwise contribution `(T_n - T_i)` and `(T_i - T_n)`
        // cancel.
        assert_eq!(initial, after);
    }
}
