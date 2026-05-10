//! Lorentz coupling between charge, wind, and the magnetic
//! vector field.
//!
//! Previously the three M1b state fields lived in three sealed
//! compartments: `Wind` updated `(v_q, v_r)` from temperature
//! gradients; `Electromagnetism` diffused `charge`; `Magnetism`
//! shaped `(B_q, B_r)` from a planetary dipole. None of them
//! talked. Real physics has them all coupled through the Lorentz
//! force `F = q · v × B`, which deflects the motion of charged
//! particles in a magnetic field. Geostrophic-style wind
//! deflection, ionospheric currents, and aurora geometry all
//! depend on this coupling.
//!
//! This law applies the per-cell Lorentz
//! kick to the wind velocity each macro-step. In our 2D hex
//! plane:
//!
//! ```text
//!   v_q_next[i] += dt · charge[i] · v_r[i] · |B|[i] · lorentz_k
//!   v_r_next[i] -= dt · charge[i] · v_q[i] · |B|[i] · lorentz_k
//! ```
//!
//! where `|B|[i]` is the cached `magnetic_magnitude`. This
//! treats the magnetic field as having a vertical component
//! proxied by the cell's total field magnitude — a simplification
//! over true 3D cross-product physics, but the *direction* of
//! deflection (q-perpendicular for r-velocity, r-perpendicular
//! for q-velocity) matches real Lorentz qualitatively. Real
//! dipole physics has the vertical component peak at the
//! magnetic poles where horizontal magnitude is weakest, so the
//! magnitude proxy is wrong at the poles but right at mid-
//! latitudes (which is where most wind flows anyway). Refining
//! to a separate vertical-B field is the deferred follow-up.
//!
//! No sign convention games here: positive charge in a positive
//! `|B|` cell with rightward velocity gets a "downward" kick (in
//! axial coords, that's −r direction). For seed-42 worlds this
//! is a tiny effect — `lorentz_k = 1e-5` keeps the per-tick
//! velocity nudge well under existing wind dynamics — but it
//! couples the three previously-isolated fields into a single
//! consistent system.
//!
//! Determinism: pure read of `state.charge()`, `state.fluid_velocity()`,
//! and `state.magnetic_magnitude()`; writes only the velocity
//! field. No pair iteration (per-cell only). No interior
//! mutability.

use crate::laws::Law;
use crate::state::PhysicsState;
use sim_arith::transcendental::{cos, sin};
use sim_arith::Real;

#[derive(Debug, Clone, Copy)]
pub struct Lorentz {
    /// Per-tick coefficient for the `q · v × B` velocity kick.
    /// Default `1e-5` keeps the per-tick velocity nudge ≪
    /// `Wind`'s acceleration scale (~`1e-3`). Larger values are
    /// physically valid for high-charge environments (sun-grazing
    /// orbits, plasma worlds) but our charge dynamics rarely
    /// exceed `~50` per cell.
    pub lorentz_k: Real,
}

impl Lorentz {
    /// Earth-like default.
    #[must_use]
    pub fn earth_like() -> Self {
        Self {
            lorentz_k: Real::from_ratio(1, 100_000),
        }
    }
}

impl Law for Lorentz {
    // q / r naming matches axial coordinates; clippy's
    // `similar_names` lint isn't useful here.
    #[allow(clippy::similar_names)]
    fn integrate(&self, state: &mut PhysicsState, dt: Real) {
        let n = state.grid().n_cells();
        let charges = state.charge().to_vec();
        // Read B_z directly instead of the |B| proxy. The
        // 2D cross product `v × B` for purely-horizontal v and a
        // mixed (B_q, B_r, B_z) field has magnitude `v_along ·
        // B_z` projected horizontally — i.e., the rotational
        // (vertical-axis) deflection on horizontal v depends *only*
        // on B_z. Earlier code used `|B|` as a proxy that was
        // wrong at the magnetic poles where B_z is largest but
        // |B_horizontal| ≈ 0 — reading B_z directly fixes that.
        let bz = state.magnetic_field_z().to_vec();
        let (v_q_prev, v_r_prev) = {
            let (q, r) = state.fluid_velocity();
            (q.to_vec(), r.to_vec())
        };
        let coeff = self.lorentz_k * dt;
        let mut v_q_after = v_q_prev.clone();
        let mut v_r_after = v_r_prev.clone();
        for i in 0..n {
            let q = charges[i];
            if q == Real::ZERO {
                continue;
            }
            let b = bz[i];
            if b == Real::ZERO {
                continue;
            }
            // Boris-pusher rotation. Earlier code used explicit
            // Euler (Δv_q = +s·v_r; Δv_r = -s·v_q) which grows
            // |v|² by (1+s²) per tick — unconditionally
            // unstable. The Boris pusher swaps in the *exact rotation*:
            //   v_q_new = +cos(θ)·v_q + sin(θ)·v_r
            //   v_r_new = -sin(θ)·v_q + cos(θ)·v_r
            // where θ = coeff·q·B_z is the signed rotation
            // angle for this tick. cos² + sin² = 1, so |v| is
            // conserved exactly regardless of θ magnitude.
            // Sign of θ carries through B_z (positive in N
            // hemisphere, negative in S), so deflection rotates
            // in opposite senses across the equator.
            let theta = coeff * q * b;
            let cos_t = cos(theta);
            let sin_t = sin(theta);
            v_q_after[i] = cos_t * v_q_prev[i] + sin_t * v_r_prev[i];
            v_r_after[i] = -sin_t * v_q_prev[i] + cos_t * v_r_prev[i];
        }
        let (vq_out, vr_out) = state.fluid_velocity_mut();
        vq_out.copy_from_slice(&v_q_after);
        vr_out.copy_from_slice(&v_r_after);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::HexGrid;
    use crate::magnetism::Magnetism;

    #[test]
    fn no_charge_no_deflection() {
        let mut state = PhysicsState::new(HexGrid::new(3, 3));
        for vq in state.fluid_velocity_mut().0 {
            *vq = Real::ONE;
        }
        // Magnetic field is zero (no Magnetism::init_field call).
        let initial: Vec<_> = state.fluid_velocity().0.to_vec();
        let l = Lorentz::earth_like();
        for _ in 0..10 {
            l.integrate(&mut state, Real::ONE);
        }
        assert_eq!(state.fluid_velocity().0, &initial[..]);
    }

    #[test]
    fn no_field_no_deflection() {
        let mut state = PhysicsState::new(HexGrid::new(3, 3));
        for q in state.charge_mut() {
            *q = Real::from_int(10);
        }
        for vq in state.fluid_velocity_mut().0 {
            *vq = Real::ONE;
        }
        let initial: Vec<_> = state.fluid_velocity().0.to_vec();
        let l = Lorentz::earth_like();
        for _ in 0..10 {
            l.integrate(&mut state, Real::ONE);
        }
        assert_eq!(state.fluid_velocity().0, &initial[..]);
    }

    #[test]
    fn charge_in_field_deflects_velocity() {
        // Positive charge with rightward velocity in
        // positive B_z gains negative v_r. With B_z read
        // directly, place the charge at a non-equatorial cell
        // (where B_z is non-zero); equator now gives no deflection.
        let mut state = PhysicsState::new(HexGrid::new(3, 9));
        let mag = Magnetism::earth_like();
        mag.init_field(&mut state);
        // Row 1 sits firmly in the northern hemisphere (row 4 =
        // equator). B_z > 0 there.
        let cell = state.grid().cell_id(crate::grid::Axial::new(1, 1)).0 as usize;
        state.fluid_velocity_mut().0[cell] = Real::ONE;
        state.charge_mut()[cell] = Real::from_int(50);
        let initial_v_r = state.fluid_velocity().1[cell];

        let l = Lorentz::earth_like();
        // 30 ticks at scale ≈ 0.025 ≈ 0.75 rad of rotation —
        // first-quadrant deflection.
        for _ in 0..30 {
            l.integrate(&mut state, Real::ONE);
        }
        let final_v_r = state.fluid_velocity().1[cell];
        assert!(
            final_v_r < initial_v_r,
            "positive-charge cell with rightward v in positive B should gain negative v_r: \
             initial={initial_v_r:?} final={final_v_r:?}"
        );
    }

    #[test]
    fn lorentz_is_deterministic() {
        let mut a = PhysicsState::new(HexGrid::new(4, 4));
        let mut b = PhysicsState::new(HexGrid::new(4, 4));
        let mag = Magnetism::earth_like();
        mag.init_field(&mut a);
        mag.init_field(&mut b);
        for (i, q) in a.charge_mut().iter_mut().enumerate() {
            *q = Real::from_int(i64::try_from(i).unwrap() % 7);
        }
        for (i, q) in b.charge_mut().iter_mut().enumerate() {
            *q = Real::from_int(i64::try_from(i).unwrap() % 7);
        }
        for (i, vq) in a.fluid_velocity_mut().0.iter_mut().enumerate() {
            *vq = Real::from_ratio(i64::try_from(i).unwrap() % 5, 10);
        }
        for (i, vq) in b.fluid_velocity_mut().0.iter_mut().enumerate() {
            *vq = Real::from_ratio(i64::try_from(i).unwrap() % 5, 10);
        }
        let l = Lorentz::earth_like();
        for _ in 0..30 {
            l.integrate(&mut a, Real::ONE);
            l.integrate(&mut b, Real::ONE);
        }
        assert_eq!(a.fluid_velocity().0, b.fluid_velocity().0);
        assert_eq!(a.fluid_velocity().1, b.fluid_velocity().1);
    }

    #[test]
    fn boris_pusher_preserves_velocity_magnitude() {
        // Earlier explicit Euler grew |v|² by (1+s²) per
        // tick — over 1000 ticks |v| would diverge dramatically.
        // Boris pusher preserves it at every magnitude of the
        // rotation angle. Run 1000 ticks at large coefficient
        // and verify |v| ≈ initial within transcendental tolerance.
        let mut state = PhysicsState::new(HexGrid::new(3, 9));
        let mag = Magnetism::earth_like();
        mag.init_field(&mut state);
        let cell = state.grid().cell_id(crate::grid::Axial::new(1, 1)).0 as usize;
        state.fluid_velocity_mut().0[cell] = Real::from_int(2);
        state.fluid_velocity_mut().1[cell] = Real::from_int(3);
        state.charge_mut()[cell] = Real::from_int(50);
        let initial_q = state.fluid_velocity().0[cell];
        let initial_r = state.fluid_velocity().1[cell];
        let initial_mag_sq = initial_q * initial_q + initial_r * initial_r;
        let l = Lorentz {
            // Aggressive coefficient — explicit Euler would have
            // exploded. Boris stays bounded.
            lorentz_k: Real::from_ratio(1, 100),
        };
        for _ in 0..1000 {
            l.integrate(&mut state, Real::ONE);
        }
        let final_q = state.fluid_velocity().0[cell];
        let final_r = state.fluid_velocity().1[cell];
        let final_mag_sq = final_q * final_q + final_r * final_r;
        // sin/cos truncation gives a few-percent drift over 1000
        // iterations. Way better than (1+s²)^1000 explosion.
        let drift_frac = ((final_mag_sq - initial_mag_sq) / initial_mag_sq)
            .to_f64_for_display()
            .abs();
        assert!(
            drift_frac < 0.1,
            "Boris pusher should preserve |v|² to within ~10 % over 1000 ticks: \
             drift_frac={drift_frac}"
        );
    }
}
