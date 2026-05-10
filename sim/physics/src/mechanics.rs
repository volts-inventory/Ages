//! Mechanics — gravity and constants. Per the operator-splitting plan,
//! mechanics doesn't own a separate integration step; it provides the gravitational
//! constant and forcing that the fluid law uses every fluid
//! sub-step.
//!
//! M1a follow-ups will add planet-seed-driven elevation
//! initialisation here. M2/M3 may extend with planet-rotation
//! Coriolis terms once orbital mechanics matter for the recognition
//! layer.

use sim_arith::Real;

/// Mechanics configuration. The gravity exponent is structural and
/// stays fixed at 2 — only the magnitude / multiplier varies
/// per planet via seeded sampling.
#[derive(Debug, Clone, Copy)]
pub struct Mechanics {
    /// Gravitational acceleration magnitude in m/s². Earth-
    /// like default 9.81; configurable via planet seed
    /// (parametric variation is allowed; structural exponent
    /// stays at 2).
    pub gravity: Real,
}

impl Mechanics {
    /// Earth-equivalent default in m/s² — used when a Planet isn't
    /// available (tests, fallback). Real runs construct
    /// `Mechanics { gravity: planet.gravity }` directly since the
    /// `Planet` field is already in m/s².
    pub fn earth_like() -> Self {
        Self {
            gravity: Real::from_ratio(981, 100),
        }
    }
}
