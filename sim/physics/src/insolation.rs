//! Per-cell stellar insolation — the photonic civilizational lever's
//! substrate signal.
//!
//! Unlike the evolving resonance / attention field, insolation is a
//! *diagnostic* field: each tick it is rewritten from the planet's
//! stellar irradiance, the cell's latitude (incidence falls off toward
//! the poles), and the cell's cloud cover (clouds dim the surface).
//! It is purely additive — no legacy law reads `state.insolation()`,
//! so installing this law leaves every existing channel bit-identical;
//! it only feeds the new recognition (`Field::Insolation`) and
//! discovery (`Channel::Optics`) consumers.
//!
//! A bright-star, thin-cloud, low-axial-spread world (the photonic
//! archetype) carries a strong, sharply latitude-banded insolation
//! field that light-sensing species fit laws over; a dim or perpetually
//! clouded world carries a faint, flat one.

use crate::laws::Law;
use crate::state::PhysicsState;
use sim_arith::Real;

/// Diagnostic stellar-insolation field law.
#[derive(Debug, Clone)]
pub struct SolarInsolation {
    /// Stellar irradiance scaled into the channel's working range
    /// (raw W/m² ÷ 300, so Earth's ~1361 → ~4.5). Keeps fits well
    /// inside Q32.32 and on the same unit-scale as the other channels.
    surface_irradiance: Real,
    /// Fraction of irradiance a fully-overcast cell still receives at
    /// the surface (the rest is reflected by cloud tops).
    cloud_floor: Real,
}

impl SolarInsolation {
    /// Earth-baseline: ~1361 W/m² irradiance, half-dimming under full
    /// cloud.
    pub fn earth_like() -> Self {
        Self {
            surface_irradiance: Real::from_ratio(1361, 300),
            cloud_floor: Real::from_ratio(50, 100),
        }
    }

    /// Per-planet field from the sampled stellar irradiance (W/m²).
    pub fn for_planet(stellar_luminosity: Real) -> Self {
        Self {
            surface_irradiance: stellar_luminosity / Real::from_int(300),
            cloud_floor: Real::from_ratio(50, 100),
        }
    }

    /// Latitude incidence factor for grid row `r` of `height` rows.
    /// Linear cosine-proxy: 1.0 at the equator falling to 0.3 at the
    /// poles (avoids fixed-point trig while keeping the equator-bright
    /// / pole-dim banding the channel exists to expose).
    fn latitude_factor(r: usize, height: u32) -> Real {
        if height == 0 {
            return Real::ONE;
        }
        let mid = Real::from_int(i64::from(height)) / Real::from_int(2);
        let row = Real::from_int(i64::from(r as u32));
        let dist = (row - mid).abs() / mid; // 0 at equator, ~1 at poles
        (Real::ONE - dist * Real::from_ratio(70, 100)).max(Real::from_ratio(30, 100))
    }
}

impl Law for SolarInsolation {
    fn integrate(&self, state: &mut PhysicsState, _dt: Real) {
        let grid = state.grid().clone();
        let width = grid.width() as usize;
        let height = grid.height();
        let n = state.insolation().len();
        let cloud = state.cloud_fraction().to_vec();
        let out = state.insolation_mut();
        for cell in 0..n {
            let r = cell / width;
            let lat = Self::latitude_factor(r, height);
            // Cloud dimming: clear sky = full irradiance, overcast =
            // `cloud_floor` of it.
            let clarity = Real::ONE - (Real::ONE - self.cloud_floor) * cloud[cell];
            out[cell] = self.surface_irradiance * lat * clarity;
        }
    }
}
