//! Coriolis deflection on 3D velocity (Sprint 5 Item 22).
//!
//! ## Hemisphere convention
//!
//! Canonical: `signed_offset = axial.r - half_h`; rows with
//! `signed_offset < 0` are **northern**, matching `magnetism.rs`
//! and `radiation.rs`. The helper
//! `crate::hemisphere::hemisphere_for_row` exposes this. Climate
//! (`world/src/climate.rs`) uses the opposite mapping; the
//! disagreement is named and tested in
//! `sim/world/src/hemisphere.rs`.
//!
//! Real winds curve right in the northern hemisphere and left
//! in the southern due to the planet's rotation. The Coriolis
//! force is the full 3D cross-product `F = -2 Ω × v`. Previously
//! this crate only carried the vertical component
//! `Ω_z = Ω · sin(latitude)` and acted on the horizontal velocity
//! field — so vertically moving air (warm-air updrafts, polar
//! downdrafts) never felt rotation, and the horizontal wind never
//! felt the `Ω_y × w` term that drives real Hadley-cell tilting.
//! Item 22 promotes Ω to a 3-vector and writes / reads the full
//! cross-product on `(v_q, v_r, v_w)`.
//!
//! ## Per-cell Ω components
//!
//! The planetary rotation axis is fixed in inertial space (along
//! the planet's spin axis). Projected into each cell's local
//! east-north-up frame, the components are latitude-dependent:
//!
//! ```text
//!   Ω_east  (= Ω_x) = 0              // no zonal component at any latitude
//!   Ω_north (= Ω_y) = Ω · cos(φ)     // max at equator, zero at poles
//!   Ω_up    (= Ω_z) = Ω · sin(φ)     // zero at equator, max at poles (signed)
//! ```
//!
//! At the equator `Ω_y` dominates: vertical motion (warm-air
//! updrafts) gets deflected zonally. Near the poles `Ω_z`
//! dominates: horizontal motion gets deflected horizontally
//! (the classic mid-latitude Coriolis). Item 15 (Hadley cells)
//! needs both — equatorial updrafts must feel a sideways kick
//! before they can re-curve into the descending Ferrel branch.
//!
//! ## Force on velocity
//!
//! ```text
//!   F = -2 Ω × v
//!   F_u = -2 (Ω_y · w  -  Ω_z · v) =  2 Ω_z · v  -  2 Ω_y · w
//!   F_v = -2 (Ω_z · u  -  Ω_x · w) = -2 Ω_z · u  +  2 Ω_x · w
//!   F_w = -2 (Ω_x · v  -  Ω_y · u) = -2 Ω_x · v  +  2 Ω_y · u
//! ```
//!
//! `omega.0 / .1 / .2` on the law are the planetary rotation
//! magnitudes about the (x, y, z) world axes. For a planet whose
//! spin axis is exactly the world-z axis (zero axial tilt) only
//! `omega.2` is nonzero — but per-cell components `Ω_y` and
//! `Ω_z` are still derived from it by `cos(φ)` / `sin(φ)`. P1.6
//! decomposes `|Ω|` across the world (x, z) axes by the planet's
//! axial tilt: `omega.0 = |Ω| · sin(tilt)` carries the
//! equatorial-plane component for tilted-axis worlds and
//! `omega.2 = |Ω| · cos(tilt)` is the reduced pole-aligned
//! component. `omega.1` stays zero in the canonical reference
//! frame (the tilt is fully described by the x and z
//! components).
//!
//! ## Coupling to vertical convection
//!
//! `VerticalConvection` writes per-cell `v_w` (warm-surface → +w,
//! cold-surface → -w). `Coriolis` reads `v_w` and writes the
//! horizontal kick (`-2 Ω_y · w`) into `v_q`, and reads
//! horizontal `(u, v)` to write a vertical Coriolis component
//! into `v_w`. This is the link that lets Item 15 grow real
//! Hadley cells without hand-coded zonal jets.
//!
//! Determinism: per-cell read of `(v_q, v_r, v_w)` and grid axial
//! row; per-cell write of `(v_q, v_r, v_w)`. No state-dependent
//! branching beyond hemisphere sign.

use crate::laws::Law;
use crate::state::PhysicsState;
use sim_arith::transcendental::{cos, half_pi, sin};
use sim_arith::Real;

/// Planetary rotation 3-vector. Components are about the world
/// (x, y, z) axes; per-cell local-frame components are derived
/// from `omega.2` by latitude (`Ω_y_local = omega.2 · cos(φ)`,
/// `Ω_z_local = omega.2 · sin(φ)`). `omega.0` carries the
/// equatorial-plane component for tilted-axis worlds
/// (`|Ω| · sin(tilt)`); `omega.1` is zero in the canonical
/// reference frame (a non-zero tilt is fully captured by the
/// (x, z) pair).
#[derive(Debug, Clone, Copy)]
pub struct Coriolis {
    /// Planetary rotation rate vector `(Ω_x, Ω_y, Ω_z)` at the
    /// macro-step cadence. The naive interpretation `2 Ω` over a
    /// ~1-day macro-step is too large for stable explicit Euler
    /// (per-tick rotation ~12 rad), so the magnitude is absorbed
    /// here in the same way the previous scalar `coriolis_k`
    /// was. Earth-like default has `omega = (0, 0, 0.001)`:
    /// only `omega.2` is nonzero because the spin axis points
    /// out of the cylinder (zero-tilt default). For tilted-axis
    /// worlds (`Coriolis::for_planet` with non-zero
    /// `axial_tilt_deg`) `|Ω|` distributes across the (x, z)
    /// pair: `omega.0 = |Ω| · sin(tilt)` and
    /// `omega.2 = |Ω| · cos(tilt)`. Per-cell `Ω_y` / `Ω_z` are
    /// then derived from `omega.2` by `cos(φ)` / `sin(φ)`;
    /// `omega.0` enters the cross-product directly as a
    /// latitude-independent zonal bias.
    pub omega: (Real, Real, Real),
    /// Vacuum guard. `false` for `Atmosphere::None` —
    /// no medium means no fluid for Coriolis to deflect. The
    /// integrate path short-circuits.
    pub has_atmosphere: bool,
}

impl Coriolis {
    /// Earth-like default. Spin axis is the local-z axis;
    /// `omega.2 = 0.001` matches the pre-Item-22 scalar
    /// `coriolis_k` so existing 2D Coriolis behaviour is
    /// bit-identical at the equator-symmetry test points.
    #[must_use]
    pub fn earth_like() -> Self {
        Self {
            omega: (Real::ZERO, Real::ZERO, Real::from_ratio(1, 1_000)),
            has_atmosphere: true,
        }
    }

    /// Build from a planet's rotation rate and axial tilt.
    ///
    /// The omega *magnitude* is derived from day-length as before
    /// (faster spinners → larger |Ω|); an Earth-day planet gets
    /// `|Ω| = 0.001`, a 12-hour planet gets 2× that, a 48-hour
    /// planet gets half. P1.6 then decomposes that magnitude
    /// across the world (x, z) axes by the planet's axial tilt:
    ///
    /// ```text
    ///   omega.0 = |Ω| · sin(tilt_rad)   // equatorial-plane component
    ///   omega.1 = 0                     // reserved (canonical frame)
    ///   omega.2 = |Ω| · cos(tilt_rad)   // pole-aligned component
    /// ```
    ///
    /// At zero tilt the spin axis is the world-z axis and the
    /// previous Earth-default `omega = (0, 0, 0.001)` is exactly
    /// reproduced. Earth's 23.4° tilt gives `omega.0 ≈ 0.397·|Ω|`
    /// and `omega.2 ≈ 0.918·|Ω|` — a non-trivial 3D Ω that lets
    /// the existing `F = -2 Ω × v` cross-product produce
    /// seasonal-style Hadley shifts on tilted worlds.
    #[must_use]
    pub fn for_planet(
        day_length_hours: Real,
        axial_tilt_deg: Real,
        has_atmosphere: bool,
    ) -> Self {
        let base = Real::from_ratio(1, 1_000);
        // Reference: Earth = 24 h. Avoid divide-by-zero on a
        // pathological zero-length day by clamping to >= 1 h.
        let ref_hours = Real::from_int(24);
        let dl = if day_length_hours <= Real::ZERO {
            Real::ONE
        } else {
            day_length_hours
        };
        let omega_magnitude = base * ref_hours / dl;
        // Convert tilt from degrees to radians: tilt_rad = deg · π / 180.
        // Clamp to [0, 90] so worldgen edge cases stay in the
        // physically meaningful range (a planet "tilted past 90°"
        // is just a retrograde-spin planet — outside this v1 scope).
        let tilt_deg = axial_tilt_deg
            .max(Real::ZERO)
            .min(Real::from_int(90));
        let tilt_rad = tilt_deg * sim_arith::transcendental::pi() / Real::from_int(180);
        let sin_tilt = sin(tilt_rad);
        let cos_tilt = cos(tilt_rad);
        Self {
            omega: (
                omega_magnitude * sin_tilt,
                Real::ZERO,
                omega_magnitude * cos_tilt,
            ),
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
        // Two-factor cross product. `2 * Ω * dt` is what each
        // velocity component is multiplied by. Compute once.
        let two_dt = Real::from_int(2) * dt;
        let two_dt_ox = two_dt * self.omega.0;
        // omega.1 carried for completeness; per-cell projection
        // uses cos(φ) / sin(φ) of omega.2 plus the bare omega.0
        // zonal bias. omega.1 is reserved and unused on
        // Earth-like inputs.
        let two_dt_oz_base = two_dt * self.omega.2;
        // Snapshot velocity into prev buffers before mutating —
        // the rotation must use the pre-step values so all three
        // components see a consistent input. Collect the
        // vertical prev buffer first: once we take the
        // `fluid_velocity_mut` borrow on the horizontal pair,
        // `state` is mutably borrowed and we can't reach
        // `fluid_velocity_w()` until that borrow drops.
        let v_w_prev: Vec<Real> = state.fluid_velocity_w().to_vec();
        let (vq_state, vr_state) = state.fluid_velocity_mut();
        let v_q_prev: Vec<Real> = vq_state.to_vec();
        let v_r_prev: Vec<Real> = vr_state.to_vec();
        // First pass: write horizontal updates into the existing
        // mut borrow. Compute per-cell Ω components on the fly.
        // Note: we mutate the horizontal slice now and the
        // vertical slice after the borrow ends.
        let mut v_w_next: Vec<Real> = v_w_prev.clone();
        for (i, r) in cells {
            if i >= n {
                continue;
            }
            // Latitude angle ∈ [-π/2, π/2], with sign carrying the
            // hemisphere. Above the equator (signed_offset < 0)
            // gets positive sin; below (signed_offset > 0) gets
            // negative — matching the convention the magnetism
            // law uses for `B_z`. Coriolis and Lorentz then
            // deflect in the same rotational sense in each
            // hemisphere.
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
            // Per-cell local-frame Ω components.
            //   Ω_z_local = omega.2 · sin(φ)   (signed by hemisphere)
            //   Ω_y_local = omega.2 · |cos(φ)| (always >= 0; equator-peaked)
            //   Ω_x_local = omega.0            (latitude-independent zonal bias)
            // cos is always non-negative on [-π/2, π/2] so the
            // sign of Ω_y matches the planetary spin sense (toward
            // celestial north on a prograde planet) at every
            // latitude — which is what real-Earth physics has.
            let sin_phi = sin(lat_angle);
            let cos_phi = cos(lat_angle);
            let two_dt_oz = two_dt_oz_base * sin_phi;
            let two_dt_oy = two_dt_oz_base * cos_phi;
            // F = -2 Ω × v applied as Δv = F · dt (already folded
            // into `two_dt_*`). Use the pre-step buffer for every
            // component so cells see a consistent input.
            let u = v_q_prev[i];
            let v = v_r_prev[i];
            let w = v_w_prev[i];
            // Δu = +2Ω_z · v − 2Ω_y · w
            let du = two_dt_oz * v - two_dt_oy * w;
            // Δv = −2Ω_z · u + 2Ω_x · w
            let dv = -two_dt_oz * u + two_dt_ox * w;
            // Δw = −2Ω_x · v + 2Ω_y · u
            let dw = -two_dt_ox * v + two_dt_oy * u;
            vq_state[i] = u + du;
            vr_state[i] = v + dv;
            v_w_next[i] = w + dw;
        }
        // Borrow-release: copy the staged vertical updates back
        // into state once the horizontal borrow has been dropped.
        state.fluid_velocity_w_mut().copy_from_slice(&v_w_next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::HexGrid;

    #[test]
    fn equator_no_horizontal_deflection_from_omega_z() {
        // At the equator sin(φ)=0 so Ω_z_local = 0. With no
        // vertical velocity the horizontal wind feels nothing
        // from Ω_z. (It can still feel Ω_y if v_w != 0; this
        // test isolates the Ω_z-only horizontal deflection.)
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
            omega: (Real::ZERO, Real::ZERO, Real::percent(1)),
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

    #[test]
    fn vertical_coriolis_component_active() {
        // Sprint 5 Item 22 spec test: an upward-moving parcel
        // near the equator gets deflected sideways by Ω_y.
        // At the equator sin(φ)=0 (so Ω_z = 0) but cos(φ)=1
        // (so Ω_y is at its maximum). The cross-product
        // Δu = -2Ω_y · w should pull the q-component of the
        // horizontal velocity away from zero even with no
        // initial horizontal motion. We use a strong omega so
        // the integer-tick effect is visible without thousands
        // of iterations.
        let mut state = PhysicsState::new(HexGrid::new(3, 5));
        let equator = state.grid().cell_id(crate::grid::Axial::new(1, 2)).0 as usize;
        // Seed pure vertical motion: warm-air updraft.
        state.fluid_velocity_w_mut()[equator] = Real::ONE;
        // Confirm there's no horizontal motion to start.
        assert_eq!(state.fluid_velocity().0[equator], Real::ZERO);
        assert_eq!(state.fluid_velocity().1[equator], Real::ZERO);
        let c = Coriolis {
            omega: (Real::ZERO, Real::ZERO, Real::percent(10)),
            has_atmosphere: true,
        };
        c.integrate(&mut state, Real::ONE);
        let u_after = state.fluid_velocity().0[equator];
        assert!(
            u_after != Real::ZERO,
            "upward parcel at the equator must be deflected sideways \
             by Ω_y (cos(0)=1): u_after={u_after:?}"
        );
        // Sanity: the deflection sign matches `Δu = -2 Ω_y · w`.
        // With omega.2 > 0, cos(0)=1, w=1, Ω_y > 0 and so Δu < 0.
        assert!(
            u_after < Real::ZERO,
            "upward parcel at the equator deflects in the -q direction \
             under prograde rotation: u_after={u_after:?}"
        );
    }

    #[test]
    fn coriolis_3d_couples_to_vertical_convection() {
        // Sprint 5 Item 22 spec test: the vertical convection
        // step now reads 3D Ω rather than just Ω_z. Run
        // VerticalConvection to seed a real vertical velocity
        // from a warm-surface / cold-upper-layer cell, then run
        // Coriolis at the equator and confirm the vertical
        // velocity itself feeds horizontal deflection — i.e.
        // Coriolis is reading `v_w` from `VerticalConvection`'s
        // output, not just `v_q` / `v_r`.
        use crate::vertical::VerticalConvection;
        let mut state = PhysicsState::new(HexGrid::new(3, 5));
        let equator = state.grid().cell_id(crate::grid::Axial::new(1, 2)).0 as usize;
        // Set up a warm surface / cold upper layer.
        state.temperature_mut()[equator] = Real::from_int(300);
        state.upper_temperature_mut()[equator] = Real::from_int(250);
        // No horizontal motion initially.
        assert_eq!(state.fluid_velocity().0[equator], Real::ZERO);
        // VerticalConvection writes a +v_w on a warm-surface cell.
        let vc = VerticalConvection::earth_like();
        vc.integrate(&mut state, Real::ONE);
        let v_w_after_vc = state.fluid_velocity_w()[equator];
        assert!(
            v_w_after_vc > Real::ZERO,
            "VerticalConvection must seed positive v_w on a warm-surface \
             cell: v_w={v_w_after_vc:?}"
        );
        // Now Coriolis reads that v_w and produces a horizontal
        // kick. If Coriolis were still 1D-Ω_z-only, the
        // horizontal velocity would stay zero.
        let c = Coriolis {
            omega: (Real::ZERO, Real::ZERO, Real::percent(10)),
            has_atmosphere: true,
        };
        c.integrate(&mut state, Real::ONE);
        let u_after = state.fluid_velocity().0[equator];
        assert!(
            u_after != Real::ZERO,
            "Coriolis must couple VerticalConvection's v_w into the \
             horizontal field (proving it reads 3D Ω, not just Ω_z): \
             u_after={u_after:?}"
        );
    }

    /// P1.6: a zero-tilt world reproduces the pre-P1.6 default
    /// exactly — `omega.0` ≈ 0 and `omega.2` is the full
    /// day-length-derived magnitude.
    #[test]
    fn zero_tilt_yields_pure_z_omega() {
        let day_length = Real::from_int(24);
        let tilt = Real::ZERO;
        let c = Coriolis::for_planet(day_length, tilt, true);
        let expected_magnitude = Real::from_ratio(1, 1_000);
        // sin(0) = 0 exactly in our transcendental; omega.0 must
        // be (numerically) zero.
        assert!(
            c.omega.0.abs() <= Real::from_ratio(1, 1_000_000),
            "zero-tilt world must have omega.0 ≈ 0: omega.0={:?}",
            c.omega.0,
        );
        // cos(0) = 1, so omega.2 must equal the full magnitude
        // (the bare day-length scale).
        let diff = (c.omega.2 - expected_magnitude).abs();
        assert!(
            diff <= Real::from_ratio(1, 1_000_000),
            "zero-tilt world omega.2 must equal |Ω|: omega.2={:?}, expected={:?}",
            c.omega.2,
            expected_magnitude,
        );
        // omega.1 is always zero in the canonical frame.
        assert_eq!(c.omega.1, Real::ZERO);
    }

    /// P1.6: an Earth-like 23.4° tilt distributes |Ω| across
    /// (x, z). sin(23.4°) ≈ 0.397, cos(23.4°) ≈ 0.918.
    #[test]
    fn earth_like_tilt_distributes_omega_into_x_and_z() {
        let day_length = Real::from_int(24);
        // Use 23.4° = 234/10. `from_ratio` keeps the fractional
        // part exact in Q32.32.
        let tilt = Real::from_ratio(234, 10);
        let c = Coriolis::for_planet(day_length, tilt, true);
        let magnitude = Real::from_ratio(1, 1_000);
        // Expected ratios from the polynomial sin/cos:
        //   sin(23.4°) ≈ 0.3971
        //   cos(23.4°) ≈ 0.9178
        // The Q32.32 polynomial approximation contributes ~1e-4
        // relative error; multiplying by `magnitude = 0.001`
        // reduces the absolute error to the ~1e-7 level. Use a
        // generous absolute tolerance of 2e-5 (2 × 10^-5) — far
        // larger than the expected approximation error but
        // small enough that a wrong decomposition fails loudly.
        let expected_x = magnitude * Real::from_ratio(397, 1_000);
        let expected_z = magnitude * Real::from_ratio(918, 1_000);
        let tol = Real::from_ratio(2, 100_000);
        assert!(
            (c.omega.0 - expected_x).abs() <= tol,
            "23.4° tilt: omega.0 ≈ 0.397·|Ω| expected; got omega.0={:?}, expected={:?}",
            c.omega.0,
            expected_x,
        );
        assert!(
            (c.omega.2 - expected_z).abs() <= tol,
            "23.4° tilt: omega.2 ≈ 0.918·|Ω| expected; got omega.2={:?}, expected={:?}",
            c.omega.2,
            expected_z,
        );
        // Strict positivity: a non-zero tilt must produce a
        // non-zero zonal axis component (this is the bug P1.6
        // fixes — `omega.0 = 0` regardless of tilt).
        assert!(
            c.omega.0 > Real::ZERO,
            "tilted-axis world must have omega.0 > 0: omega.0={:?}",
            c.omega.0,
        );
    }

    /// P1.6: vertical-velocity parcel on a tilted-axis world
    /// feels deflection from the new `omega.0` (Ω_x) component
    /// via `Δv = +2 Ω_x · w` — i.e. the v-component of horizontal
    /// velocity gets kicked. Item 22's vertical-Coriolis test
    /// only exercised `omega.2` (via `Ω_y_local`); this test
    /// confirms the omega.0 path is also live.
    #[test]
    fn tilted_axis_world_has_x_axis_coriolis_term() {
        // Pick a cell on the equator (latitude row r = half_h) so
        // sin(φ) = 0 and cos(φ) = 1 in local frame. That kills
        // the Ω_z_local contribution to Δv (the `-two_dt_oz * u`
        // term is zero because u = 0). Δv then comes entirely
        // from `+two_dt_ox * w` — the omega.0 path — so any
        // non-zero v_r after a tick must come from the tilt.
        let mut state = PhysicsState::new(HexGrid::new(3, 5));
        let equator = state.grid().cell_id(crate::grid::Axial::new(1, 2)).0 as usize;
        // Seed pure vertical motion.
        state.fluid_velocity_w_mut()[equator] = Real::ONE;
        assert_eq!(state.fluid_velocity().0[equator], Real::ZERO);
        assert_eq!(state.fluid_velocity().1[equator], Real::ZERO);
        // Build a tilted-axis world. We pick equal omega.0 and
        // omega.2 (corresponds to a 45° tilt) and use a strong
        // magnitude so the integer-tick effect is visible
        // without thousands of iterations. Construct the struct
        // directly (rather than via `for_planet`) because we
        // want a magnitude well above the Q32.32 LSB for a
        // single tick at `dt = 1`.
        let strong = Real::percent(10);
        let c = Coriolis {
            omega: (strong, Real::ZERO, strong),
            has_atmosphere: true,
        };
        c.integrate(&mut state, Real::ONE);
        let v_after = state.fluid_velocity().1[equator];
        // Δv = -2Ω_z_local · u + 2Ω_x · w
        //    = -2·(omega.2·sin(0))·0  +  2·omega.0·1
        //    =  2 · omega.0
        // With omega.0 > 0 and w = 1, v_r should turn positive.
        assert!(
            v_after > Real::ZERO,
            "tilted-axis world: vertical parcel must be deflected by \
             omega.0 into +v_r at the equator: v_after={v_after:?}"
        );
        // Sanity-check the magnitude: Δv ≈ 2·omega.0·dt = 0.2.
        let expected = Real::from_int(2) * strong;
        let diff = (v_after - expected).abs();
        assert!(
            diff <= Real::from_ratio(1, 1_000),
            "tilted-axis ΔV ≈ 2·omega.0·dt = 0.2 expected; got v_after={v_after:?}",
        );
    }
}
