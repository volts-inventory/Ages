//! Coriolis deflection on horizontal velocity.
//!
//! Real winds curve right in the northern hemisphere and left
//! in the southern due to the planet's rotation. The Coriolis
//! force `F = -2m·Ω × v` for horizontal motion picks up the
//! vertical component of the rotation vector:
//! `Ω_z = Ω · sin(latitude)`. Previously the wind law was
//! axisymmetric — N and S hemispheres behaved identically.
//! Without Coriolis, real Hadley cells, geostrophic flows, and
//! mirror-symmetric storm rotation are all impossible.
//!
//! `Coriolis` runs after `Wind` (so it sees post-pressure-
//! gradient velocity) and applies the cross-product:
//!
//! ```text
//!   Ω_z[i] = coriolis_k · sin(latitude[i])      // signed by hemisphere
//!   Δv_q   = +Ω_z · v_r · dt
//!   Δv_r   = -Ω_z · v_q · dt
//! ```
//!
//! Sign of `sin(latitude)` flips between hemispheres
//! (positive in north, negative in south), giving real
//! mirror-symmetric deflection: a parcel of N-hemisphere wind
//! moving east curves toward the south; the same parcel in the
//! S-hemisphere curves toward the north. Same shape as the
//! Lorentz law on `B_z`, but unconditional on charge — it acts on
//! every cell, not just charged ones.
//!
//! Determinism: per-cell read of `(v_q, v_r)` and grid axial
//! row; per-cell write of `(v_q, v_r)`. No state-dependent
//! branching beyond hemisphere sign.

use crate::laws::Law;
use crate::state::PhysicsState;
use sim_arith::transcendental::{cos, half_pi, sin};
use sim_arith::Real;

#[derive(Debug, Clone, Copy)]
pub struct Coriolis {
    /// Per-tick deflection coefficient. Real planet's
    /// `2 · Ω` at the macro-step cadence (~1 day) is
    /// too large for stable explicit Euler — naive use would
    /// produce per-tick rotation of ~12 rad, far past the
    /// Lorentz law's 0.5-rad-per-tick stability ceiling. We absorb that into
    /// `coriolis_k`, calibrated so the per-tick rotation at
    /// mid-latitudes is small (a few degrees per macro-step).
    /// Default `0.001` matches the magnitude of `Wind::wind_k`'s
    /// pressure-gradient acceleration so Coriolis is a real
    /// influence but doesn't dominate.
    pub coriolis_k: Real,
    /// Vacuum guard. `false` for `Atmosphere::None` —
    /// no medium means no fluid for Coriolis to deflect. The
    /// integrate path short-circuits.
    pub has_atmosphere: bool,
}

impl Coriolis {
    /// Earth-like default.
    #[must_use]
    pub fn earth_like() -> Self {
        Self {
            coriolis_k: Real::from_ratio(1, 1_000),
            has_atmosphere: true,
        }
    }

    /// Build from a planet's rotation rate. For now we
    /// derive a single multiplicative scale from the day-length
    /// (faster spinners → stronger Coriolis); the actual
    /// `coriolis_k` is the product of a base coefficient and
    /// `24 / day_length_hours` so an Earth-day planet gets the
    /// default `0.001`, a 12-hour planet gets 2× that, and a
    /// 48-hour planet gets half.
    #[must_use]
    pub fn for_planet(day_length_hours: Real, has_atmosphere: bool) -> Self {
        let base = Real::from_ratio(1, 1_000);
        // Reference: Earth = 24 h. Avoid divide-by-zero on a
        // pathological zero-length day by clamping to ≥ 1 h.
        let ref_hours = Real::from_int(24);
        let dl = if day_length_hours <= Real::ZERO {
            Real::ONE
        } else {
            day_length_hours
        };
        Self {
            coriolis_k: base * ref_hours / dl,
            has_atmosphere,
        }
    }
}

impl Law for Coriolis {
    #[allow(clippy::similar_names)]
    fn integrate(&self, state: &mut PhysicsState, dt: Real) {
        // Vacuum short-circuit. No medium = no fluid for
        // Coriolis to deflect.
        if !self.has_atmosphere {
            return;
        }
        let grid = state.grid().clone();
        let height_i = i32::try_from(grid.height()).unwrap_or(i32::MAX).max(1);
        let half_h = height_i / 2;
        let n = grid.n_cells();
        let cells: Vec<_> = grid
            .cells()
            .map(|(cid, axial)| (cid.0 as usize, axial.r))
            .collect();
        let coeff = self.coriolis_k * dt;
        let (vq_state, vr_state) = state.fluid_velocity_mut();
        let v_q_prev: Vec<Real> = vq_state.to_vec();
        let v_r_prev: Vec<Real> = vr_state.to_vec();
        for (i, r) in cells {
            if i >= n {
                continue;
            }
            // Latitude angle ∈ [-π/2, π/2], with sign carrying the
            // hemisphere. Above the equator (signed_offset < 0)
            // gets positive sin; below (signed_offset > 0) gets
            // negative — matching the convention the magnetism law uses for
            // `B_z`. Coriolis and Lorentz then deflect in the
            // same rotational sense in each hemisphere.
            let signed_offset = r - half_h;
            let lat_angle = if half_h > 0 {
                let mag =
                    half_pi() * Real::from_ratio(i64::from(signed_offset.abs()), i64::from(half_h));
                match signed_offset.cmp(&0) {
                    std::cmp::Ordering::Less => mag,
                    std::cmp::Ordering::Greater => -mag,
                    std::cmp::Ordering::Equal => Real::ZERO,
                }
            } else {
                Real::ZERO
            };
            // Boris-pusher rotation. Earlier code used explicit
            // Euler which grows |v|² by (1+ω_z²) per tick. The Boris
            // pusher uses the exact 2D rotation, conserving |v| at
            // every magnitude of θ.
            let theta = coeff * sin(lat_angle);
            if theta == Real::ZERO {
                continue;
            }
            let cos_t = cos(theta);
            let sin_t = sin(theta);
            vq_state[i] = cos_t * v_q_prev[i] + sin_t * v_r_prev[i];
            vr_state[i] = -sin_t * v_q_prev[i] + cos_t * v_r_prev[i];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::HexGrid;

    #[test]
    fn equator_no_deflection() {
        // sin(0) = 0 → no Coriolis at the equator.
        let mut state = PhysicsState::new(HexGrid::new(3, 5));
        let centre = state.grid().cell_id(crate::grid::Axial::new(1, 2)).0 as usize;
        state.fluid_velocity_mut().0[centre] = Real::ONE;
        let initial_v_r = state.fluid_velocity().1[centre];
        let c = Coriolis::earth_like();
        for _ in 0..50 {
            c.integrate(&mut state, Real::ONE);
        }
        assert_eq!(state.fluid_velocity().1[centre], initial_v_r);
    }

    #[test]
    fn northern_hemisphere_deflects_one_way_southern_the_other() {
        // Eastward wind in N hemisphere → curves toward south
        // (negative v_r); same wind in S hemisphere → curves
        // toward north (positive v_r). Mirror symmetry.
        let mut state = PhysicsState::new(HexGrid::new(3, 9));
        let north_cell = state.grid().cell_id(crate::grid::Axial::new(1, 1)).0 as usize;
        let south_cell = state.grid().cell_id(crate::grid::Axial::new(1, 7)).0 as usize;
        state.fluid_velocity_mut().0[north_cell] = Real::ONE;
        state.fluid_velocity_mut().0[south_cell] = Real::ONE;

        let c = Coriolis {
            coriolis_k: Real::percent(1),
            has_atmosphere: true,
        };
        for _ in 0..30 {
            c.integrate(&mut state, Real::ONE);
        }
        let north_v_r = state.fluid_velocity().1[north_cell];
        let south_v_r = state.fluid_velocity().1[south_cell];
        assert!(
            north_v_r < Real::ZERO,
            "N-hemisphere eastward wind should curve toward -r: \
             north_v_r={north_v_r:?}"
        );
        assert!(
            south_v_r > Real::ZERO,
            "S-hemisphere eastward wind should curve toward +r: \
             south_v_r={south_v_r:?}"
        );
    }

    #[test]
    fn coriolis_is_deterministic() {
        let mut a = PhysicsState::new(HexGrid::new(4, 4));
        let mut b = PhysicsState::new(HexGrid::new(4, 4));
        for (i, vq) in a.fluid_velocity_mut().0.iter_mut().enumerate() {
            *vq = Real::from_ratio(i64::try_from(i).unwrap() % 5, 10);
        }
        for (i, vq) in b.fluid_velocity_mut().0.iter_mut().enumerate() {
            *vq = Real::from_ratio(i64::try_from(i).unwrap() % 5, 10);
        }
        let c = Coriolis::earth_like();
        for _ in 0..20 {
            c.integrate(&mut a, Real::ONE);
            c.integrate(&mut b, Real::ONE);
        }
        assert_eq!(a.fluid_velocity().0, b.fluid_velocity().0);
        assert_eq!(a.fluid_velocity().1, b.fluid_velocity().1);
    }
}
