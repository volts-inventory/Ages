//! Photochemical (UV-driven dissociation) channel.
//!
//! UV photolysis breaks H2O / CH4 into lighter species (H, O, etc.);
//! those products escape faster than the parent. This is the
//! dominant loss channel for Mars *today* — no ozone layer means
//! raw UV reaches the lower atmosphere and cracks water vapour into
//! H + OH, and the H escapes. Modelled as
//! `base × uv_flux × dissociation_factor × ozone_shield`, where the
//! ozone shield reuses [`crate::atmospheric_escape::ion::ion_escape_factor`]
//! as a proxy: strong-magnetosphere planets co-evolve O2 → ozone
//! that absorbs UV before it reaches lower-atmosphere volatiles, so
//! the same `1 / (1 + B)` form that suppresses ion escape also
//! suppresses photochem.

use sim_arith::Real;

/// UV reference flux for the photochemical channel. `100 W/m²` puts
/// modern Earth (~90 W/m² near-UV) at factor ~0.9; the photochemical
/// channel scales linearly with UV.
pub const PHOTOCHEMICAL_UV_REF_W_M2: i64 = 100;

/// Per-cell UV factor: `uv_flux / PHOTOCHEMICAL_UV_REF_W_M2`. The
/// photochemical channel scales linearly with UV — twice the UV,
/// twice the photolysis rate.
#[must_use]
pub fn photochemical_uv_factor(uv_flux_w_m2: Real) -> Real {
    uv_flux_w_m2 / Real::from_int(PHOTOCHEMICAL_UV_REF_W_M2)
}
