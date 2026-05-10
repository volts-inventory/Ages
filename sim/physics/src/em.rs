//! Simplified electromagnetism — second M1b law family.
//!
//! M1b foundation tracks per-cell charge and lets it diffuse to
//! neighbours via pair-flux (charge-conserving by construction, same
//! pattern as heat and fluid mass).
//!
//! What's deliberately not in this commit: lightning discharge
//! events (charge-threshold-driven discrete spikes), charge-fluid
//! advection coupling (operator-splitting fluid → EM coupling order), magnetic-
//! field dynamics. These land in M1b follow-ups.
//!
//! The magnetic field is treated as a static per-cell quantity in
//! M1b — set at planet init from planet config, read by other laws
//! (e.g. for future Lorentz force on charged particle motion), but
//! not evolved.

use crate::laws::Law;
use crate::state::PhysicsState;
use sim_arith::Real;

/// Hex-direction axial offsets (E, NE, NW, W, SW, SE) used by
/// the charge-advection pair-flux. Same canonical order as
/// `wind.rs` / `hydrology.rs` / `lorentz.rs`.
const NEIGHBOUR_DIRECTIONS: [(i64, i64); 6] = [(1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)];

/// EM law: charge propagation by neighbour diffusion + lightning
/// discharge events when local charge accumulates past a threshold.
/// Magnetic field stays static. M1b follow-ups add charge advection
/// and magnetostatic structure derived from planet rotation + core
/// composition.
/// EM law: charge propagation by neighbour diffusion + lightning
/// discharge events when local charge accumulates past a threshold.
/// Also advects charge along the wind velocity field
/// (`(v_q, v_r)`) — completes the M1b coupling triangle (charge ↔
/// velocity ↔ B). Magnetic field stays static. M1b follow-ups add
/// magnetostatic structure derived from planet rotation + core
/// composition.
#[derive(Debug, Clone, Copy)]
pub struct Electromagnetism {
    /// Conductivity coefficient. Per-step charge transfer between
    /// neighbours scales linearly with this.
    pub conductivity: Real,
    /// |charge| above which the cell discharges (lightning). The
    /// discharge dissipates the charge to ground (sets charge to
    /// zero) and converts the magnitude into heat, raising
    /// temperature by `|charge| * discharge_energy`. Setting the
    /// threshold very high effectively disables discharges.
    pub discharge_threshold: Real,
    /// Heat released per unit of discharged |charge|.
    pub discharge_energy: Real,
    /// Pair-flux upwind coefficient for charge advection
    /// along `(v_q, v_r)`. Multiplies `v_along · upwind_charge ·
    /// dt`. Default 0.01 keeps wind-driven charge transport
    /// comparable to the molecular conductivity diffusion.
    pub charge_advect_k: Real,
}

impl Electromagnetism {
    pub fn earth_like() -> Self {
        Self {
            conductivity: Real::from_ratio(1, 100),
            discharge_threshold: Real::from_int(50),
            discharge_energy: Real::from_int(5),
            charge_advect_k: Real::from_ratio(1, 100),
        }
    }
}

impl Law for Electromagnetism {
    fn integrate(&self, state: &mut PhysicsState, dt: Real) {
        self.diffuse_charge(state, dt);
        self.advect_charge(state, dt);
        self.discharge(state);
    }
}

impl Electromagnetism {
    fn diffuse_charge(&self, state: &mut PhysicsState, dt: Real) {
        let grid = state.grid().clone();
        let prev = state.charge().to_vec();
        let next = state.charge_mut();
        next.copy_from_slice(&prev);

        for (cid, axial) in grid.cells() {
            let i = cid.0 as usize;
            for nb in grid.neighbours(axial) {
                let j = nb.0 as usize;
                if j > i {
                    // Pair-flux: charge flows from higher-charge cell
                    // to lower-charge one. Single multiplication per
                    // pair so the +delta and -delta are exact
                    // negations; total charge conserved bit-exactly.
                    let delta = self.conductivity * dt * (prev[j] - prev[i]);
                    next[i] = next[i] + delta;
                    next[j] = next[j] - delta;
                }
            }
        }
    }

    /// Pair-flux upwind charge advection along the wind field.
    /// Same scheme `Wind` uses for temperature; pair-flux preserves
    /// total charge bit-exactly. Closes the M1b coupling triangle
    /// — the Lorentz law already deflects velocity by `q · v × B`, but previously
    /// charge itself didn't ride the wind, so charged regions
    /// stayed sealed in their cell of origin. Now they drift with
    /// the atmospheric flow.
    #[allow(clippy::similar_names)]
    fn advect_charge(&self, state: &mut PhysicsState, dt: Real) {
        let grid = state.grid().clone();
        let prev = state.charge().to_vec();
        let (vq, vr) = state.fluid_velocity();
        let vq_v = vq.to_vec();
        let vr_v = vr.to_vec();
        let two = Real::from_int(2);
        let mut next = prev.clone();
        for (cid, axial) in grid.cells() {
            let i = cid.0 as usize;
            for (k, nb) in grid.neighbours(axial).iter().enumerate() {
                let j = nb.0 as usize;
                if j > i {
                    let (dir_q, dir_r) = NEIGHBOUR_DIRECTIONS[k];
                    let vmid_q = (vq_v[i] + vq_v[j]) / two;
                    let vmid_r = (vr_v[i] + vr_v[j]) / two;
                    let v_along = vmid_q * Real::from_int(dir_q) + vmid_r * Real::from_int(dir_r);
                    let upwind = if v_along > Real::ZERO {
                        prev[i]
                    } else {
                        prev[j]
                    };
                    let flux = self.charge_advect_k * dt * v_along * upwind;
                    next[i] = next[i] - flux;
                    next[j] = next[j] + flux;
                }
            }
        }
        state.charge_mut().copy_from_slice(&next);
    }

    fn discharge(&self, state: &mut PhysicsState) {
        let n = state.grid().n_cells();
        for i in 0..n {
            let q = state.charge()[i];
            if q.abs() > self.discharge_threshold {
                // Discharge to ground: convert |charge| to heat and
                // zero the cell. Charge "lost" goes to a conceptual
                // ground sink — same approximation real lightning
                // makes (charge neutralised through the discharge
                // path). Documented limitation: this is the only
                // place in M1b where total charge is not bit-
                // exactly conserved.
                state.temperature_mut()[i] =
                    state.temperature()[i] + (q.abs() * self.discharge_energy);
                state.charge_mut()[i] = Real::ZERO;
            }
        }
    }
}

#[cfg(test)]
mod discharge_tests {
    use super::*;
    use crate::grid::HexGrid;

    #[test]
    fn high_charge_discharges_to_heat() {
        let grid = HexGrid::new(3, 3);
        let mut state = PhysicsState::new(grid);
        state.charge_mut()[0] = Real::from_int(100);
        let initial_temp = state.temperature()[0];

        let em = Electromagnetism::earth_like();
        em.integrate(&mut state, Real::from_ratio(1, 100));

        // After discharge, cell 0 charge is zero and its
        // temperature has risen.
        assert_eq!(state.charge()[0], Real::ZERO);
        assert!(state.temperature()[0] > initial_temp);
    }

    #[test]
    fn low_charge_does_not_discharge() {
        let grid = HexGrid::new(3, 3);
        let mut state = PhysicsState::new(grid);
        // Start with charge well below threshold (50). Use a small
        // conductivity so diffusion doesn't move much.
        state.charge_mut()[0] = Real::from_int(10);
        let initial_temp = state.temperature()[0];

        let em = Electromagnetism {
            conductivity: Real::from_ratio(1, 1000),
            discharge_threshold: Real::from_int(50),
            discharge_energy: Real::from_int(5),
            charge_advect_k: Real::ZERO,
        };
        em.integrate(&mut state, Real::from_ratio(1, 100));

        // No discharge — charge approximately preserved (some
        // diffusion to neighbours), temperature unchanged.
        assert_eq!(state.temperature()[0], initial_temp);
    }

    #[test]
    fn negative_charge_also_discharges() {
        let grid = HexGrid::new(3, 3);
        let mut state = PhysicsState::new(grid);
        state.charge_mut()[0] = Real::from_int(-100);
        let initial_temp = state.temperature()[0];

        let em = Electromagnetism::earth_like();
        em.integrate(&mut state, Real::from_ratio(1, 100));

        // Negative charge above |threshold| discharges too —
        // |charge| triggers the threshold, energy released is
        // positive.
        assert_eq!(state.charge()[0], Real::ZERO);
        assert!(state.temperature()[0] > initial_temp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::{Axial, HexGrid};

    #[test]
    fn charge_diffuses_from_a_spike() {
        let grid = HexGrid::new(3, 3);
        let mut state = PhysicsState::new(grid);
        let centre = state.grid().cell_id(Axial::new(1, 1));
        state.charge_mut()[centre.0 as usize] = Real::from_int(60);

        let em = Electromagnetism {
            conductivity: Real::from_ratio(1, 10),
            discharge_threshold: Real::from_int(1_000_000),
            discharge_energy: Real::ZERO,
            charge_advect_k: Real::ZERO,
        };
        em.integrate(&mut state, Real::ONE);

        // Centre lost charge; some neighbour gained.
        assert!(state.charge()[centre.0 as usize] < Real::from_int(60));
        let nb = state.grid().neighbours(Axial::new(1, 1))[0].0 as usize;
        assert!(state.charge()[nb] > Real::ZERO);
    }

    #[test]
    fn em_is_deterministic() {
        let grid = HexGrid::new(4, 4);
        let mut a = PhysicsState::new(grid.clone());
        let mut b = PhysicsState::new(grid);

        let centre_a = a.grid().cell_id(Axial::new(2, 2));
        let centre_b = b.grid().cell_id(Axial::new(2, 2));
        a.charge_mut()[centre_a.0 as usize] = Real::from_int(100);
        b.charge_mut()[centre_b.0 as usize] = Real::from_int(100);

        let em = Electromagnetism {
            conductivity: Real::from_ratio(1, 10),
            discharge_threshold: Real::from_int(1_000_000),
            discharge_energy: Real::ZERO,
            charge_advect_k: Real::ZERO,
        };
        for _ in 0..10 {
            em.integrate(&mut a, Real::ONE);
            em.integrate(&mut b, Real::ONE);
        }
        assert_eq!(a.charge(), b.charge());
    }

    #[test]
    fn charge_is_conserved() {
        let grid = HexGrid::new(3, 3);
        let mut state = PhysicsState::new(grid);
        // Mix of positive and negative charges to exercise sign
        // handling.
        state.charge_mut()[0] = Real::from_int(10);
        state.charge_mut()[4] = Real::from_int(-7);
        state.charge_mut()[8] = Real::from_int(3);

        let initial: Real = state
            .charge()
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);

        let em = Electromagnetism {
            conductivity: Real::from_ratio(1, 10),
            discharge_threshold: Real::from_int(1_000_000),
            discharge_energy: Real::ZERO,
            charge_advect_k: Real::ZERO,
        };
        for _ in 0..20 {
            em.integrate(&mut state, Real::ONE);
        }

        let after: Real = state
            .charge()
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);

        // Pair-flux conserves charge bit-exactly.
        assert_eq!(initial, after);
    }

    #[test]
    fn wind_advects_charge() {
        // With non-zero wind velocity, charge migrates along
        // the velocity field. A point charge with steady eastward
        // wind should drift its centre eastward.
        let grid = HexGrid::new(5, 1);
        let mut state = PhysicsState::new(grid);
        // Point charge at q=2.
        let centre = state.grid().cell_id(crate::grid::Axial::new(2, 0)).0 as usize;
        state.charge_mut()[centre] = Real::from_int(40);
        // Eastward wind everywhere.
        for vq in state.fluid_velocity_mut().0 {
            *vq = Real::ONE;
        }
        let east_initial = state.charge()[centre + 1];
        let em = Electromagnetism {
            conductivity: Real::ZERO, // isolate advection
            discharge_threshold: Real::from_int(1_000_000),
            discharge_energy: Real::ZERO,
            charge_advect_k: Real::from_ratio(1, 10),
        };
        for _ in 0..5 {
            em.integrate(&mut state, Real::ONE);
        }
        let east_final = state.charge()[centre + 1];
        assert!(
            east_final > east_initial,
            "eastward wind should push charge eastward: \
             initial={east_initial:?} final={east_final:?}"
        );
    }

    #[test]
    fn charge_advection_conserves_total() {
        // Pair-flux structure means total charge is bit-exactly
        // preserved. With conductivity=0 and discharge gated very
        // high, the only mover is advection.
        let grid = HexGrid::new(4, 4);
        let mut state = PhysicsState::new(grid);
        for (i, q) in state.charge_mut().iter_mut().enumerate() {
            *q = Real::from_int(i64::try_from(i).unwrap() % 7 - 3);
        }
        for (i, vq) in state.fluid_velocity_mut().0.iter_mut().enumerate() {
            *vq = Real::from_ratio(i64::try_from(i).unwrap() % 3 - 1, 100);
        }
        let initial: Real = state
            .charge()
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        let em = Electromagnetism {
            conductivity: Real::ZERO,
            discharge_threshold: Real::from_int(1_000_000),
            discharge_energy: Real::ZERO,
            charge_advect_k: Real::from_ratio(1, 100),
        };
        for _ in 0..40 {
            em.integrate(&mut state, Real::ONE);
        }
        let after: Real = state
            .charge()
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        assert_eq!(initial, after);
    }
}
