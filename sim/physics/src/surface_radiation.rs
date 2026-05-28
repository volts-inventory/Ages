//! Per-cell ionizing surface radiation — the nuclear civilizational
//! lever's substrate signal.
//!
//! A diagnostic field (like `SolarInsolation`): each tick it is
//! rewritten from a radiogenic crust baseline (rare-earth / heavy-
//! element decay, uniform) plus a cosmic / stellar-particle flux that
//! a strong magnetosphere shields and that rises toward the poles
//! (where field lines funnel charged particles in). It is purely
//! additive — no legacy law reads `state.surface_radiation()`, so
//! installing it leaves every existing channel bit-identical; it only
//! feeds the new recognition (`Field::Radiation`) and discovery
//! (`Channel::Radiogenic`) consumers.
//!
//! A rare-earth-crust, weakly-shielded world (the nuclear archetype)
//! carries a strong radiation field that thermal-sensing species and
//! radiation-hardened biologies fit laws over; a basaltic, strongly-
//! shielded world carries a faint one.

use crate::laws::Law;
use crate::state::PhysicsState;
use sim_arith::Real;

/// Diagnostic surface-radiation field law.
#[derive(Debug, Clone)]
pub struct SurfaceRadiation {
    /// Uniform radiogenic-decay floor from the crust's heavy-element
    /// fraction (Earth-surface background ≈ 1 working unit).
    radiogenic_floor: Real,
    /// Cosmic / stellar-particle flux reaching the equator after
    /// magnetospheric shielding (0 on a strongly-shielded world).
    cosmic_flux: Real,
}

impl SurfaceRadiation {
    pub fn earth_like() -> Self {
        Self {
            radiogenic_floor: Real::ONE,
            cosmic_flux: Real::from_ratio(5, 10),
        }
    }

    /// Per-planet field. `rare_earth_fraction` is the crust's heavy-
    /// element mass fraction (`0..1`); `shielding` is the fraction of
    /// cosmic flux the magnetosphere blocks (None 0 / Weak 0.5 /
    /// Strong 0.9).
    pub fn for_planet(rare_earth_fraction: Real, shielding: Real) -> Self {
        // Radiogenic floor: Earth background scaled up with heavy-
        // element abundance (rare-earth crust ~5x background).
        let radiogenic_floor =
            Real::from_ratio(5, 10) + rare_earth_fraction * Real::from_int(20);
        // Unshielded cosmic flux at the equator.
        let cosmic_flux = Real::from_int(3) * (Real::ONE - shielding).max(Real::ZERO);
        Self {
            radiogenic_floor,
            cosmic_flux,
        }
    }

    /// Latitude factor for the cosmic component: weak at the equator
    /// (0.3), strong at the poles (1.0) where field lines funnel
    /// particles in. Linear proxy. The radiogenic floor is uniform and
    /// added separately.
    fn cosmic_latitude_factor(r: usize, height: u32) -> Real {
        if height == 0 {
            return Real::ONE;
        }
        let mid = Real::from_int(i64::from(height)) / Real::from_int(2);
        let row = Real::from_int(r as i64);
        let dist = (row - mid).abs() / mid; // 0 equator, ~1 poles
        (Real::from_ratio(30, 100) + dist * Real::from_ratio(70, 100)).min(Real::ONE)
    }
}

impl Law for SurfaceRadiation {
    fn integrate(&self, state: &mut PhysicsState, _dt: Real) {
        let grid = state.grid().clone();
        let width = grid.width() as usize;
        let height = grid.height();
        let n = state.surface_radiation().len();
        let out = state.surface_radiation_mut();
        for cell in 0..n {
            let r = cell / width;
            let cosmic = self.cosmic_flux * Self::cosmic_latitude_factor(r, height);
            out[cell] = self.radiogenic_floor + cosmic;
        }
    }
}
