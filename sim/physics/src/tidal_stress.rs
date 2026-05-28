//! Per-cell tidal stress — the gravitational civilizational lever's
//! substrate signal.
//!
//! A diagnostic field (like `SolarInsolation`): each tick it is
//! rewritten from the planet's aggregate tidal strength (set by its
//! moons and surface gravity) and a latitude pattern (the tidal bulge
//! peaks in the equatorial / sub-lunar band and falls toward the
//! poles). It is purely additive — no legacy law reads
//! `state.tidal_stress()`, so installing it leaves every existing
//! channel bit-identical; it only feeds the new recognition
//! (`Field::TidalStress`) and discovery (`Channel::Tidal`) consumers.
//!
//! A many-mooned, high-gravity world (the gravitational archetype)
//! carries a strong, sharply banded tidal field that ground- and
//! motion-sensing species fit laws over; a moonless low-gravity world
//! carries a faint, flat one.

use crate::laws::Law;
use crate::state::PhysicsState;
use sim_arith::Real;

/// Diagnostic tidal-stress field law.
#[derive(Debug, Clone)]
pub struct TidalStress {
    /// Aggregate tidal amplitude at the sub-lunar band, in working
    /// units (Earth's single-moon tide ≈ 1). Scales with total moon
    /// pull × surface gravity.
    amplitude: Real,
}

impl TidalStress {
    pub fn earth_like() -> Self {
        Self {
            amplitude: Real::ONE,
        }
    }

    /// Per-planet field from an aggregate tidal coefficient (summed
    /// moon pull, Earth-relative) and surface gravity in g.
    pub fn for_planet(tidal_coefficient: Real, gravity_g: Real) -> Self {
        Self {
            amplitude: tidal_coefficient * gravity_g,
        }
    }

    /// Latitude band factor for grid row `r` of `height`: tidal bulge
    /// peaks at the equator (1.0), falls to 0.2 at the poles. Linear
    /// proxy for the cos² sub-lunar pattern (avoids fixed-point trig).
    fn latitude_factor(r: usize, height: u32) -> Real {
        if height == 0 {
            return Real::ONE;
        }
        let mid = Real::from_int(i64::from(height)) / Real::from_int(2);
        let row = Real::from_int(r as i64);
        let dist = (row - mid).abs() / mid;
        (Real::ONE - dist * Real::from_ratio(80, 100)).max(Real::from_ratio(20, 100))
    }
}

impl Law for TidalStress {
    fn integrate(&self, state: &mut PhysicsState, _dt: Real) {
        let grid = state.grid().clone();
        let width = grid.width() as usize;
        let height = grid.height();
        let n = state.tidal_stress().len();
        let out = state.tidal_stress_mut();
        for cell in 0..n {
            let r = cell / width;
            out[cell] = self.amplitude * Self::latitude_factor(r, height);
        }
    }
}
