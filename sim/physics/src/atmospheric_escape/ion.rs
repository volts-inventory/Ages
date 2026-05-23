//! Ion escape channel.
//!
//! Charged species escape along open magnetic field lines. A strong
//! planetary magnetic field traps ions on closed field lines
//! (magnetosphere); a weak / absent one lets the solar wind strip
//! charged particles directly. Modelled as `base / (1 + B_local)`.
//! Earth's strong dipole keeps ion loss negligible; Mars (no
//! dipole) loses ~2 kg/s of O via this channel and ~few × 10^25 ions
//! per second according to MAVEN.

use sim_arith::Real;

/// Per-cell magnetic shielding factor used by the ion-escape and
/// photochemical channels (P3.5). Reads the local shielding
/// strength — which combines the global dipole with crustal
/// remanence — rather than the planet-wide
/// `PlanetEscapeParams::magnetic_strength` scalar. The canonical
/// magnetosphere shielding form `1 / (1 + B_local)` applied to
/// the per-cell field: at `B_local = 0` factor = 1.0 (no
/// shielding); at `B_local = 1.0` (Earth baseline) = 0.5; at
/// `B_local = 1.5` (strong crustal-remanence umbrella ceiling) ≈
/// 0.4. The function is exposed so callers and tests can verify
/// the per-cell coupling without going through the full
/// orchestrator path.
#[must_use]
pub fn ion_escape_factor(local_magnetic_strength: Real) -> Real {
    Real::ONE / (Real::ONE + local_magnetic_strength)
}
