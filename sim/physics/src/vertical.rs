//! Vertical atmosphere stack: surface ↔ upper-layer
//! convective heat exchange.
//!
//! Real atmospheres have *vertical* structure. Surface heating
//! drives upward convection (warm air rises); upper-atmosphere
//! cooling drives downward convection (cold air sinks). The
//! horizontal advection the wind law ships and the seasonal/diurnal
//! insolation source are all 2D — they don't capture
//! the lapse-rate physics that gives real atmospheres their
//! vertical temperature gradient (~6.5 K/km on Earth) and that
//! drives cloud formation, Hadley circulation, and convective
//! instability.
//!
//! A full 3D atmosphere stack is a multi-PR architectural
//! overhaul. Minimum-viable shape: one upper-atmosphere
//! temperature field per cell, coupled to surface temperature
//! by a single convective heat-exchange step:
//!
//! ```text
//!   ΔT_surface = -k · (T_surface - T_upper) · dt + radiative_loss
//!   ΔT_upper   = +k · (T_surface - T_upper) · dt - radiative_to_space
//! ```
//!
//! Without the `radiative_loss` / `radiative_to_space` terms, the
//! two layers would equilibrate and erase the lapse rate. This law
//! models radiative loss to space as a small per-tick cooling
//! of the upper layer, biased toward the upper-layer
//! steady-state being colder than the surface — exactly the
//! lapse-rate signature.
//!
//! This is "1.5D" rather than full 3D — no horizontal advection
//! at the upper layer yet, no vertical velocity field. But it's
//! enough to give civs a queryable upper-atmosphere temperature
//! that varies with surface heating (tropical updrafts, polar
//! stratospheric vortex), and it sets up the structure for
//! later refinements to add real vertical advection.

use crate::laws::Law;
use crate::state::PhysicsState;
use sim_arith::Real;

#[derive(Debug, Clone, Copy)]
pub struct VerticalConvection {
    /// Per-tick exchange coefficient between surface and upper
    /// layer. Default 0.05 closes ~5 % of the gap per tick →
    /// ~20-tick equilibration, matching the order-of-magnitude
    /// of vertical convective mixing timescales.
    pub exchange_k: Real,
    /// Per-tick radiative cooling of the upper layer toward
    /// space. The "cosmic background" temperature; we use 100 K
    /// as a planet-friendly approximation (real CMB is ~2.7 K
    /// but radiation to space at the tropopause has effective
    /// emission temperature ~250 K; 100 K is a compromise that
    /// drives a meaningful lapse rate without freezing the
    /// upper layer to vacuum).
    pub space_temperature: Real,
    /// Per-tick fraction of the upper-to-space gap closed by
    /// radiative loss. Default 0.01 ≈ 100-tick radiative
    /// timescale, matching real-world stratospheric
    /// radiative-cooling rates (~0.5 K/day → ~5 K/year).
    pub radiative_loss_k: Real,
}

impl VerticalConvection {
    /// Earth-like default.
    #[must_use]
    pub fn earth_like() -> Self {
        Self {
            exchange_k: Real::from_ratio(5, 100),
            space_temperature: Real::from_int(100),
            radiative_loss_k: Real::from_ratio(1, 100),
        }
    }
}

impl Law for VerticalConvection {
    fn integrate(&self, state: &mut PhysicsState, dt: Real) {
        let n = state.grid().n_cells();
        let surface = state.temperature().to_vec();
        let upper = state.upper_temperature().to_vec();
        let exchange = self.exchange_k * dt;
        let radiative = self.radiative_loss_k * dt;
        let space_t = self.space_temperature;
        // First-pass init: if upper layer is exactly zero
        // (never seeded), bootstrap with a 30 K lapse below
        // surface. Without this, the first tick would see a
        // huge gap (T_surface ≈ 280 K, T_upper = 0) and the
        // exchange would yank surface temperature down hard.
        let surface_buf = state.temperature().to_vec();
        let mut upper_next = upper.clone();
        for i in 0..n {
            if upper_next[i] == Real::ZERO && surface_buf[i] > Real::ZERO {
                upper_next[i] = surface_buf[i] - Real::from_int(30);
            }
        }
        let mut surface_next = surface.clone();
        for i in 0..n {
            // Convective heat exchange: closes the gap each tick.
            let gap = surface[i] - upper_next[i];
            let xfer = exchange * gap;
            surface_next[i] = surface_next[i] - xfer;
            upper_next[i] = upper_next[i] + xfer;
            // Radiative loss of the upper layer toward space.
            // Drives the steady-state lapse rate by pulling
            // upper-T below surface-T even after convective
            // equilibration.
            let space_gap = upper_next[i] - space_t;
            upper_next[i] = upper_next[i] - radiative * space_gap;
        }
        state.temperature_mut().copy_from_slice(&surface_next);
        state.upper_temperature_mut().copy_from_slice(&upper_next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::HexGrid;

    #[test]
    fn upper_layer_settles_below_surface_at_steady_state() {
        // With no other physics, a uniform-surface planet
        // should reach a steady state where upper-layer T is
        // below surface T (the lapse rate). Run many ticks and
        // verify.
        let grid = HexGrid::new(3, 3);
        let mut state = PhysicsState::new(grid);
        for t in state.temperature_mut() {
            *t = Real::from_int(280);
        }
        // upper_temperature starts at 0; bootstrap kicks in on
        // first integrate.
        let v = VerticalConvection::earth_like();
        for _ in 0..500 {
            v.integrate(&mut state, Real::ONE);
        }
        let centre = state.grid().cell_id(crate::grid::Axial::new(1, 1)).0 as usize;
        let s = state.temperature()[centre];
        let u = state.upper_temperature()[centre];
        assert!(
            u < s,
            "upper layer should settle below surface at steady state: \
             surface={s:?} upper={u:?}"
        );
    }

    #[test]
    fn vertical_convection_is_deterministic() {
        let mut a = PhysicsState::new(HexGrid::new(4, 4));
        let mut b = PhysicsState::new(HexGrid::new(4, 4));
        for (i, t) in a.temperature_mut().iter_mut().enumerate() {
            *t = Real::from_int(280 + i64::try_from(i).unwrap() * 2);
        }
        for (i, t) in b.temperature_mut().iter_mut().enumerate() {
            *t = Real::from_int(280 + i64::try_from(i).unwrap() * 2);
        }
        let v = VerticalConvection::earth_like();
        for _ in 0..30 {
            v.integrate(&mut a, Real::ONE);
            v.integrate(&mut b, Real::ONE);
        }
        assert_eq!(a.temperature(), b.temperature());
        assert_eq!(a.upper_temperature(), b.upper_temperature());
    }

    #[test]
    fn warmer_surface_drives_upward_heat_flux() {
        // A cell with surface much warmer than upper should
        // transfer heat upward in one tick.
        let grid = HexGrid::new(3, 3);
        let mut state = PhysicsState::new(grid);
        let centre = state.grid().cell_id(crate::grid::Axial::new(1, 1)).0 as usize;
        state.temperature_mut()[centre] = Real::from_int(300);
        state.upper_temperature_mut()[centre] = Real::from_int(200);
        let initial_upper = state.upper_temperature()[centre];
        let v = VerticalConvection::earth_like();
        v.integrate(&mut state, Real::ONE);
        let final_upper = state.upper_temperature()[centre];
        assert!(
            final_upper > initial_upper,
            "warm surface should transfer heat upward: \
             initial={initial_upper:?} final={final_upper:?}"
        );
    }
}
