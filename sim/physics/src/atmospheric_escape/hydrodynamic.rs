//! Hydrodynamic blow-off channel.
//!
//! When XUV (extreme UV) flux is high enough, the exosphere expands
//! outward as a bulk fluid rather than as individual escaping
//! molecules. Young stars deliver ~100× modern Sun's XUV; the early
//! Solar System lost most of Mars's primordial atmosphere this way.
//! Modelled as `base × euv_flux × thermal_factor` — only fires
//! meaningfully when both are high.

use sim_arith::Real;

/// Temperature reference for the hydrodynamic thermal factor.
/// Hydrodynamic blow-off only fires once the upper atmosphere is
/// warm enough that the bulk flow speed exceeds the escape
/// velocity locally. `300 K` puts a typical Earth surface at
/// factor 1.0; a hot young Venus (700 K) hits ~2.3×.
pub const HYDRODYNAMIC_T_REF_K: i64 = 300;

/// Per-cell hydrodynamic thermal factor. `T / T_ref` — linear
/// scaling with temperature so a warmer atmosphere blows off more
/// dramatically. Clamped at zero for completeness; no upper cap so
/// truly hot atmospheres can lose mass without bound (the per-cell
/// density floor still applies).
#[must_use]
pub fn hydrodynamic_thermal_factor(temperature_k: Real) -> Real {
    let ratio = temperature_k / Real::from_int(HYDRODYNAMIC_T_REF_K);
    ratio.max(Real::ZERO)
}
