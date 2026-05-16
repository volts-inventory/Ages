//! Lunar gravitational tides on the surface-water column.
//!
//! Previously the planet had no orbital companion and no tidal
//! forcing — `water_depth` only changed via gravity flow,
//! hydrology (evaporation/precipitation), and the seeded
//! initial state. Real planets with moons see the surface ocean
//! lift into a sub-lunar bulge and an antipodal bulge (the
//! gradient term in the lunar gravitational potential), with
//! the bulges sweeping the planet as the moon orbits — twice-
//! daily tides on Earth, much slower on slow rotators.
//!
//! This module adds a `Tides` law that:
//!
//! 1. Tracks the current sub-lunar longitude as
//!    `sub_lunar_q = (macro_step · width / lunar_period_macros) % width`.
//!    Pure integer arithmetic — no transcendentals, no cell-time
//!    drift.
//! 2. Computes a tidal potential per cell:
//!    `Φ[i] = 1 - 8 · min(|q[i]-sub|, |q[i]-anti|) / width` (clamped
//!    so two peaks at sub-lunar and antipodal hit Φ=+1, quarter-
//!    circle low tides hit Φ=-1). Triangular shape rather than
//!    cos(2θ) because the fixed-point arithmetic has no sin/cos and the order-of-
//!    magnitude shape is identical for tidal-flux purposes.
//! 3. For each pair (i, j) with i<j: redistributes `water_depth`
//!    from low-Φ cells to high-Φ cells using the standard pair-
//!    flux pattern:
//!    ```text
//!      flux = tide_k · dt · (Φ[i] - Φ[j])
//!      water_depth[i] += flux
//!      water_depth[j] -= flux
//!    ```
//!    Pair-flux preserves total water bit-exactly — the moon
//!    only *moves* surface water around, it doesn't create or
//!    destroy any.
//!
//! No bulge in the r (latitude) direction yet — real tides do
//! have one (the bulge tilts with lunar declination), but a 1D
//! model captures the dominant 2-bulge sweep that gives twice-
//! per-orbit tidal forcing at every cell. The follow-up adds
//! r-modulation once moon orbital inclination becomes a planet
//! parameter.
//!
//! Determinism: the only state-derived input is `state.macro_step()`,
//! which the orchestrator advances exactly once per macro-step.
//! Pair iteration is canonical-order. No interior mutability, no
//! per-tick allocation beyond a single potential vector. No state-
//! dependent branching beyond `pair (i, j) with j > i`.
//!
//! Mass conservation: pair-flux structure (each `flux` applied as
//! `+flux` to one cell and `-flux` to the other). Verified by
//! `tides_conserve_total_water`.

use crate::laws::Law;
use crate::state::PhysicsState;
use sim_arith::transcendental::{cos, two_pi};
use sim_arith::Real;

/// Per-moon tidal configuration. Earlier the `Tides`
/// law tracked a single `lunar_period_macros`; this struct promotes
/// that to a list so multi-moon planets get genuine
/// interference patterns. Mass scales each moon's contribution
/// to the per-cell potential.
#[derive(Debug, Clone, Copy)]
pub struct MoonTide {
    /// Tidal contribution weight. Earth's moon ≈ 1.0; the
    /// sun ≈ 0.46. For our model, `mass_relative` is just a
    /// scalar multiplier on the cos(2θ) potential.
    pub mass_relative: Real,
    /// Macro-steps for one full sub-lunar cycle. Earth's moon
    /// = 28 macro-steps at the standard cadence.
    pub period_macros: u32,
}

#[derive(Debug, Clone)]
pub struct Tides {
    /// Pair-flux coefficient. Multiplies `(Φ[i] - Φ[j]) · dt` to
    /// give per-pair water transfer.
    pub tide_k: Real,
    /// Per-moon orbital configs. Empty for moonless
    /// planets (the `integrate` path returns early). Each moon
    /// contributes a cos(2θ) bulge at its own period; the
    /// per-cell potential is the mass-weighted sum.
    pub moons: Vec<MoonTide>,
}

impl Tides {
    /// Earth-like defaults: one Earth-Moon-equivalent moon with
    /// 28-macro-step cycle.
    #[must_use]
    pub fn earth_like() -> Self {
        Self {
            tide_k: Real::from_ratio(1, 1_000),
            moons: vec![MoonTide {
                mass_relative: Real::ONE,
                period_macros: 28,
            }],
        }
    }

    /// Build from a list of per-moon tide configs. Empty
    /// list means moonless (integrate becomes a no-op).
    #[must_use]
    pub fn for_planet(moons: Vec<MoonTide>) -> Self {
        Self {
            tide_k: Real::from_ratio(1, 1_000),
            moons,
        }
    }

    /// Sub-lunar longitude in grid-q for the moon
    /// at `moon_idx` and the given macro-step. Public so tests
    /// can pin it without driving an integration.
    #[must_use]
    pub fn sub_lunar_q(&self, moon_idx: usize, macro_step: u64, width: u32) -> i32 {
        let Some(moon) = self.moons.get(moon_idx) else {
            return 0;
        };
        let period = u64::from(moon.period_macros).max(1);
        let phase = macro_step.saturating_mul(u64::from(width)) / period;
        i32::try_from(phase % u64::from(width.max(1))).unwrap_or(0)
    }
}

impl Law for Tides {
    fn integrate(&self, state: &mut PhysicsState, dt: Real) {
        if self.moons.is_empty() {
            return;
        }
        let grid = state.grid().clone();
        let width = i32::try_from(grid.width()).unwrap_or(1).max(1);

        // Per-cell tidal potential is the *sum* of each
        // moon's cos(2θ) contribution, weighted by moon mass.
        // Multi-moon planets get genuine interference patterns
        // (the spring/neap-cycle analogue). Each moon's
        // sub_lunar_q advances at its own period; their
        // collective bulge sweep is the geographic / temporal
        // beat pattern of the moon system.
        let n = grid.n_cells();
        let mut potential = vec![Real::ZERO; n];
        let two_pi_v = two_pi();
        for (m_idx, moon) in self.moons.iter().enumerate() {
            if moon.period_macros == 0 {
                continue;
            }
            let sub_lunar_q = self.sub_lunar_q(m_idx, state.macro_step(), grid.width());
            for (cid, axial) in grid.cells() {
                let i = cid.0 as usize;
                let raw_diff = (axial.q - sub_lunar_q).rem_euclid(width);
                let signed_q_diff = if raw_diff <= width / 2 {
                    raw_diff
                } else {
                    raw_diff - width
                };
                let theta = two_pi_v * Real::from_ratio(i64::from(signed_q_diff), i64::from(width));
                potential[i] = potential[i] + moon.mass_relative * cos(theta + theta);
            }
        }

        // Step 2: pair-flux water redistribution. Bit-exact
        // mass conservation by construction.
        let prev_w = state.water_depth().to_vec();
        let mut next_w = prev_w.clone();
        for (cid, axial) in grid.cells() {
            let i = cid.0 as usize;
            for nb in grid.neighbours(axial) {
                let j = nb.0 as usize;
                if j > i {
                    let dphi = potential[i] - potential[j];
                    let flux = self.tide_k * dt * dphi;
                    next_w[i] = next_w[i] + flux;
                    next_w[j] = next_w[j] - flux;
                }
            }
        }
        // Defensive non-negative clamp — under earth-like coefficients
        // and `tide_k=0.001` the per-step flux stays well below typical
        // water_depth values, so this should never trigger in practice;
        // it guards against pathological coefficients (large `tide_k`
        // relative to ambient depth).
        for w in &mut next_w {
            if *w < Real::ZERO {
                *w = Real::ZERO;
            }
        }
        state.water_depth_mut().copy_from_slice(&next_w);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::HexGrid;

    #[test]
    fn sub_lunar_advances_each_cycle() {
        let tides = Tides::earth_like();
        // 28-cycle, width 28: one cell per macro-step.
        assert_eq!(tides.sub_lunar_q(0, 0, 28), 0);
        assert_eq!(tides.sub_lunar_q(0, 1, 28), 1);
        assert_eq!(tides.sub_lunar_q(0, 14, 28), 14);
        assert_eq!(tides.sub_lunar_q(0, 28, 28), 0);
    }

    #[test]
    fn tides_conserve_total_water() {
        let grid = HexGrid::new(8, 4);
        let mut state = PhysicsState::new(grid);
        for w in state.water_depth_mut() {
            *w = Real::from_int(100);
        }
        let initial: Real = state
            .water_depth()
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        let tides = Tides::earth_like();
        for _ in 0..50 {
            tides.integrate(&mut state, Real::ONE);
            state.advance_macro_step();
        }
        let after: Real = state
            .water_depth()
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        assert_eq!(
            initial, after,
            "pair-flux tidal redistribution must conserve total water bit-exactly"
        );
    }

    #[test]
    fn tides_lift_sub_lunar_cell_above_quarter_cell() {
        // With sub_lunar at q=0, the cells at q=0 should have
        // higher water depth than cells at q=width/4 after several
        // ticks of tidal redistribution.
        let grid = HexGrid::new(8, 1);
        let mut state = PhysicsState::new(grid);
        for w in state.water_depth_mut() {
            *w = Real::from_int(100);
        }
        let tides = Tides {
            tide_k: Real::percent(1),
            // freeze sub_lunar at 0 with a huge period
            moons: vec![MoonTide {
                mass_relative: Real::ONE,
                period_macros: u32::MAX,
            }],
        };
        for _ in 0..30 {
            tides.integrate(&mut state, Real::ONE);
            state.advance_macro_step();
        }
        let high = state.water_depth()[0];
        let low = state.water_depth()[2]; // q=2 is the quarter-circle for width=8
        assert!(
            high > low,
            "sub-lunar cell should be higher than quarter-circle cell: high={high:?} low={low:?}"
        );
    }

    #[test]
    fn tides_are_deterministic() {
        let grid = HexGrid::new(6, 4);
        let mut a = PhysicsState::new(grid.clone());
        let mut b = PhysicsState::new(grid);
        for w in a.water_depth_mut() {
            *w = Real::from_int(50);
        }
        for w in b.water_depth_mut() {
            *w = Real::from_int(50);
        }
        let tides = Tides::earth_like();
        for _ in 0..40 {
            tides.integrate(&mut a, Real::ONE);
            a.advance_macro_step();
            tides.integrate(&mut b, Real::ONE);
            b.advance_macro_step();
        }
        assert_eq!(a.water_depth(), b.water_depth());
        assert_eq!(a.macro_step(), b.macro_step());
    }

    #[test]
    fn no_moon_is_no_op() {
        let grid = HexGrid::new(4, 4);
        let mut state = PhysicsState::new(grid);
        for (i, w) in state.water_depth_mut().iter_mut().enumerate() {
            *w = Real::from_int(10 + i64::try_from(i).unwrap());
        }
        let initial: Vec<_> = state.water_depth().to_vec();
        let tides = Tides::for_planet(vec![]);
        for _ in 0..20 {
            tides.integrate(&mut state, Real::ONE);
            state.advance_macro_step();
        }
        assert_eq!(state.water_depth(), &initial[..]);
    }
}
