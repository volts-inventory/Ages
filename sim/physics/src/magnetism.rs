//! Planetary magnetic field as a vector quantity.
//!
//! ## Hemisphere convention
//!
//! Canonical (shared with `coriolis.rs`, `radiation.rs`, and
//! `sim/recognition/src/lib.rs::Signature::Hemisphere`):
//! `signed_offset = axial.r - half_h`; `signed_offset < 0` is the
//! **northern** hemisphere ("Negative r direction = toward north
//! pole = compass-needle convention" — see the in-loop comment
//! below). The centralised helper is
//! `crate::hemisphere::hemisphere_for_row`. This file pre-dates
//! the helper and inlines the `signed_offset.cmp(&0)` branch
//! verbatim; a behaviour-preserving migration to the helper is a
//! separate PR. The convention itself is canonical; only the
//! per-call-site duplication is a refactor target.
//!
//! Note: `world/src/climate.rs::seasonal_temperature_offset` uses
//! the **opposite** mapping. See `sim/world/src/hemisphere.rs` for
//! the audit + the test that names the disagreement.
//!
//! Previously the magnetic field was a scalar magnitude (`Vec<Real>`)
//! with no direction. The recognition library's
//! `magnetic_field_strong` template even keyed on `Field::Charge`
//! as a proxy because nothing actually wrote the magnetic state.
//!
//! Real planetary magnetic fields have direction — they're
//! approximately dipole patterns aligned with the rotational axis,
//! with the horizontal component strongest at the equator and
//! tapering to zero at the poles (where the field becomes purely
//! vertical, which our 2D hex grid doesn't represent). This module
//! promotes the magnetic state to a vector `(B_q, B_r)` per cell
//! and ships a `Magnetism` law that:
//!
//! 1. Initialises the per-cell field at planet build time from
//!    a latitude-dependent dipole model. Magnitude scales with
//!    `Magnetosphere` strength (None / Weak / Strong); direction
//!    is `(0, -lat_factor)` in axial coords (axis-aligned dipole,
//!    no E-W component, pointing from south pole toward north
//!    pole at every cell — like a compass needle).
//! 2. Per macro-step, applies a small diurnal modulation: the
//!    field magnitude oscillates by ±`diurnal_amplitude` over a
//!    `diurnal_period` macro-step cycle. Real planets see this
//!    from ionospheric current systems driven by solar heating;
//!    we collapse the daily variation into a triangular wave
//!    indexed by `state.macro_step()` (the planetary clock). Magnitude
//!    only — direction stays fixed.
//!
//! No Lorentz coupling onto charge/wind motion yet. That's the
//! next refinement; the vector field is now in place to be read
//! by such couplings (and by recognition templates that key on
//! "compass alignment" or "magnetic deflection").
//!
//! Determinism: state inputs are `state.grid().height()` and
//! `state.macro_step()`; both are deterministic. No interior
//! mutability, no per-tick allocation beyond a single magnitude
//! buffer reused per cell.

use crate::laws::Law;
use crate::state::PhysicsState;
use sim_arith::transcendental::{cos, half_pi, sin, sqrt};
use sim_arith::Real;

#[derive(Debug, Clone, Copy)]
pub struct Magnetism {
    /// Equatorial dipole magnitude. Set by `Magnetosphere` class
    /// (None=0, Weak=10, Strong=50) at planet build time. Polar
    /// cells trend toward 0 magnitude (latitude factor); the
    /// equator gets the full `dipole_strength`.
    pub dipole_strength: Real,
    /// Per-tick fractional swing in field magnitude across the
    /// diurnal cycle. 0.05 = ±5 % per cycle; 0 disables modulation.
    pub diurnal_amplitude: Real,
    /// Macro-steps for one full diurnal cycle. With the 1-day-
    /// per-macro-step cadence this is ~1; on a fast-rotator world
    /// it could be smaller, but our macro-step is the finest
    /// resolution so anything < 1 collapses to per-step. Default 1.
    pub diurnal_period: u32,
}

impl Magnetism {
    /// Earth-like default: strong dipole, small daily swing.
    #[must_use]
    pub fn earth_like() -> Self {
        Self {
            dipole_strength: Real::from_int(50),
            diurnal_amplitude: Real::percent(5),
            diurnal_period: 1,
        }
    }

    /// Magnetism for a given magnetosphere class. None → no
    /// dipole at all; the law reduces to a no-op.
    #[must_use]
    pub fn for_strength(dipole_strength: Real) -> Self {
        Self {
            dipole_strength,
            ..Self::earth_like()
        }
    }

    /// Initialise the per-cell `(B_q, B_r)` vector field on a
    /// fresh state. Call once at planet init *before* the first
    /// `integrate` pass. For each cell at axial `(q, r)`:
    /// - `B_q = 0` (axis-aligned dipole has no E-W component in
    ///   the surface plane)
    /// - `B_r = -dipole_strength · cos_lat` (points from south
    ///   pole toward north pole; magnitude maxes at the equator
    ///   and tapers linearly toward the poles)
    #[allow(clippy::similar_names)]
    pub fn init_field(&self, state: &mut PhysicsState) {
        if self.dipole_strength == Real::ZERO {
            return;
        }
        let height_i = i32::try_from(state.grid().height())
            .unwrap_or(i32::MAX)
            .max(1);
        let half_h = height_i / 2;
        let n = state.grid().n_cells();
        let b_q = vec![Real::ZERO; n];
        let mut b_r = vec![Real::ZERO; n];
        let mut b_z = vec![Real::ZERO; n];
        let two = Real::from_int(2);
        for (cid, axial) in state.grid().cells() {
            let i = cid.0 as usize;
            // Signed pole offset: positive r is south of equator
            // in our convention; negative r is north.
            let signed_offset = axial.r - half_h;
            let pole_dist = signed_offset.abs();
            // Real cos latitude factor — 1 at equator,
            // 0 at poles. We also want the corresponding
            // sin (= 1 at poles, 0 at equator) for B_z.
            let (lat_cos, lat_sin) = if half_h > 0 {
                let angle = half_pi() * Real::from_ratio(i64::from(pole_dist), i64::from(half_h));
                (cos(angle), sin(angle))
            } else {
                (Real::ONE, Real::ZERO)
            };
            // Negative r direction = toward north pole = compass-needle
            // convention.
            b_r[i] = -self.dipole_strength * lat_cos;
            // Vertical-axis component. Real planetary
            // dipole has |B_z| = 2 · dipole_strength · sin(lat),
            // peaked at the magnetic poles where the horizontal
            // component vanishes. Sign flips between hemispheres
            // (field exits at one magnetic pole, enters at the
            // other). Convention: B_z > 0 in the northern
            // hemisphere (signed_offset < 0), B_z < 0 in the
            // southern (signed_offset > 0), zero at the equator.
            let sign = match signed_offset.cmp(&0) {
                std::cmp::Ordering::Less => Real::ONE,
                std::cmp::Ordering::Greater => -Real::ONE,
                std::cmp::Ordering::Equal => Real::ZERO,
            };
            b_z[i] = self.dipole_strength * lat_sin * two * sign;
            // b_q stays zero (axis-aligned dipole).
        }
        let (state_bq, state_br) = state.magnetic_field_mut();
        state_bq.copy_from_slice(&b_q);
        state_br.copy_from_slice(&b_r);
        state.magnetic_field_z_mut().copy_from_slice(&b_z);
        // Refresh the magnitude cache so recognition
        // scans never recompute sqrt per cell per template per
        // tick. With B_z now a real component, magnitude is
        // sqrt(B_q² + B_r² + B_z²).
        let mag_buf: Vec<Real> = b_q
            .iter()
            .zip(b_r.iter())
            .zip(b_z.iter())
            .map(|((q, r), z)| sqrt(*q * *q + *r * *r + *z * *z))
            .collect();
        state.magnetic_magnitude_mut().copy_from_slice(&mag_buf);
    }

    /// Diurnal modulation factor at the current macro-step. Pure
    /// triangular wave: returns 1.0 at the start of each cycle,
    /// rises linearly to (1 + amplitude) at half-cycle, falls
    /// back to (1 - amplitude) at end-cycle, and so on.
    /// Returns 1.0 when `diurnal_period == 0` (modulation
    /// disabled).
    fn diurnal_factor(&self, macro_step: u64) -> Real {
        if self.diurnal_period == 0 || self.diurnal_amplitude == Real::ZERO {
            return Real::ONE;
        }
        let period = u64::from(self.diurnal_period.max(1));
        let phase = macro_step % period;
        let half = period / 2;
        if half == 0 {
            // Period 1: one swing per macro-step; alternate sign.
            return if phase == 0 {
                Real::ONE + self.diurnal_amplitude
            } else {
                Real::ONE - self.diurnal_amplitude
            };
        }
        // Triangle wave on [-1, +1]:
        //   phase ∈ [0, half) → rises 0 → +1 (linear)
        //   phase ∈ [half, period) → falls +1 → -1 (linear)
        let phase_i = i64::try_from(phase).unwrap_or(i64::MAX);
        let half_i = i64::try_from(half).unwrap_or(i64::MAX);
        let triangle = if phase < half {
            Real::from_ratio(phase_i, half_i)
        } else {
            let down = phase_i - half_i;
            Real::ONE - Real::from_ratio(down, half_i) - Real::ONE
        };
        Real::ONE + self.diurnal_amplitude * triangle
    }
}

impl Law for Magnetism {
    #[allow(clippy::similar_names)]
    fn integrate(&self, state: &mut PhysicsState, _dt: Real) {
        if self.dipole_strength == Real::ZERO || self.diurnal_amplitude == Real::ZERO {
            return;
        }
        let factor = self.diurnal_factor(state.macro_step());
        // Apply the *delta* relative to the unscaled field by
        // re-deriving from latitude rather than scaling in place
        // (in-place scaling would compound across ticks). Same
        // pattern radiation uses (read previous-tick value,
        // write new-tick value derived from a model).
        let height_i = i32::try_from(state.grid().height())
            .unwrap_or(i32::MAX)
            .max(1);
        let half_h = height_i / 2;
        let n = state.grid().n_cells();
        let cells: Vec<_> = state
            .grid()
            .cells()
            .map(|(cid, axial)| (cid.0 as usize, axial.r))
            .collect();
        // Track new B_z and |B| separately so we can update both
        // caches once the vector pass is done (avoids holding
        // multiple mutable borrows on different state fields).
        let mut bz_after: Vec<Real> = vec![Real::ZERO; n];
        let mut mag_after: Vec<Real> = vec![Real::ZERO; n];
        let two = Real::from_int(2);
        let cells_owned = cells;
        {
            let (state_bq, state_br) = state.magnetic_field_mut();
            for (i, r) in &cells_owned {
                let i = *i;
                let r = *r;
                if i >= n {
                    continue;
                }
                let signed_offset = r - half_h;
                let pole_dist = signed_offset.abs();
                let (lat_cos, lat_sin) = if half_h > 0 {
                    let angle =
                        half_pi() * Real::from_ratio(i64::from(pole_dist), i64::from(half_h));
                    (cos(angle), sin(angle))
                } else {
                    (Real::ONE, Real::ZERO)
                };
                let sign = match signed_offset.cmp(&0) {
                    std::cmp::Ordering::Less => Real::ONE,
                    std::cmp::Ordering::Greater => -Real::ONE,
                    std::cmp::Ordering::Equal => Real::ZERO,
                };
                state_bq[i] = Real::ZERO;
                let new_br = -self.dipole_strength * lat_cos * factor;
                state_br[i] = new_br;
                let new_bz = self.dipole_strength * lat_sin * two * sign * factor;
                bz_after[i] = new_bz;
                // |B| with B_q = 0: sqrt(B_r² + B_z²).
                mag_after[i] = sqrt(new_br * new_br + new_bz * new_bz);
            }
        }
        state.magnetic_field_z_mut().copy_from_slice(&bz_after);
        state.magnetic_magnitude_mut().copy_from_slice(&mag_after);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::HexGrid;

    #[test]
    fn horizontal_strongest_at_equator() {
        // With B_z added, the *total* magnitude
        // peaks at the poles, not the equator. The horizontal
        // (|B_r|) component still peaks at the equator —
        // verify that explicitly.
        let mut state = PhysicsState::new(HexGrid::new(3, 9));
        let mag = Magnetism::earth_like();
        mag.init_field(&mut state);
        let mid_cell = state.grid().cell_id(crate::grid::Axial::new(0, 4));
        let pole_cell = state.grid().cell_id(crate::grid::Axial::new(0, 0));
        let (_bq, br) = state.magnetic_field();
        let mid_horizontal = br[mid_cell.0 as usize].abs();
        let pole_horizontal = br[pole_cell.0 as usize].abs();
        assert!(
            mid_horizontal > pole_horizontal,
            "equatorial |B_horizontal| should exceed polar: \
             mid={mid_horizontal:?} pole={pole_horizontal:?}"
        );
    }

    #[test]
    fn total_magnitude_strongest_at_poles() {
        // Real dipole: |B| ∝ sqrt(1 + 3 sin²(lat)). Poles get
        // |B| = 2 · dipole; equator gets |B| = dipole.
        let mut state = PhysicsState::new(HexGrid::new(3, 9));
        let mag = Magnetism::earth_like();
        mag.init_field(&mut state);
        let mid_cell = state.grid().cell_id(crate::grid::Axial::new(0, 4));
        let pole_cell = state.grid().cell_id(crate::grid::Axial::new(0, 0));
        let mid_mag = state.magnetic_field_magnitude(mid_cell.0 as usize);
        let pole_mag = state.magnetic_field_magnitude(pole_cell.0 as usize);
        assert!(
            pole_mag > mid_mag,
            "polar |B| should exceed equatorial when B_z is added: \
             mid={mid_mag:?} pole={pole_mag:?}"
        );
    }

    #[test]
    fn no_field_for_zero_dipole() {
        let mut state = PhysicsState::new(HexGrid::new(3, 3));
        let mag = Magnetism::for_strength(Real::ZERO);
        mag.init_field(&mut state);
        for i in 0..state.grid().n_cells() {
            assert_eq!(state.magnetic_field_magnitude(i), Real::ZERO);
        }
    }

    #[test]
    fn dipole_points_toward_north_pole() {
        // B_r should be negative (pointing from south r=high toward
        // north r=0) at every non-pole cell.
        let mut state = PhysicsState::new(HexGrid::new(3, 5));
        let mag = Magnetism::earth_like();
        mag.init_field(&mut state);
        let (_bq, br) = state.magnetic_field();
        // Mid row (r=2) gets the strongest negative B_r.
        let mid_idx = state.grid().cell_id(crate::grid::Axial::new(0, 2)).0 as usize;
        assert!(
            br[mid_idx] < Real::ZERO,
            "equatorial B_r should point toward north pole (negative r)"
        );
    }

    #[test]
    fn integrate_is_deterministic() {
        let mut a = PhysicsState::new(HexGrid::new(4, 4));
        let mut b = PhysicsState::new(HexGrid::new(4, 4));
        let mag = Magnetism::earth_like();
        mag.init_field(&mut a);
        mag.init_field(&mut b);
        for _ in 0..20 {
            mag.integrate(&mut a, Real::ONE);
            a.advance_macro_step();
            mag.integrate(&mut b, Real::ONE);
            b.advance_macro_step();
        }
        assert_eq!(a.magnetic_field().0, b.magnetic_field().0);
        assert_eq!(a.magnetic_field().1, b.magnetic_field().1);
    }
}
