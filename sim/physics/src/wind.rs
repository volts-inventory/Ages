//! Atmospheric advection.
//!
//! Previously the only horizontal heat transport was molecular thermal
//! conduction (`HeatConduction`'s `alpha ≈ 0.1` per macro-step).
//! Real planets transport heat ~100-1000× faster via *atmospheric
//! circulation*: temperature gradients drive density gradients,
//! density gradients drive pressure gradients, pressure gradients
//! accelerate air mass, and the moving air carries enthalpy with
//! it. Without this, the equator-to-pole gradient that the
//! radiation law sources never gets effectively redistributed —
//! the climatology has the right *driver* but the wrong *transport*.
//!
//! This module adds a `Wind` law that:
//!
//! 1. Computes per-cell pressure as `P = c · T` (ideal-gas proxy
//!    for fixed atmospheric mass; only the *gradient* matters
//!    physically).
//! 2. Updates per-cell velocity `(v_q, v_r)` with the pressure-
//!    gradient force minus friction:
//!    ```text
//!      v[i] += -dt · grad_P[i] / ρ
//!      v[i] *= (1 - friction · dt)
//!    ```
//!    `grad_P[i]` is the axial-coordinate sum over neighbours of
//!    `(P[nb] - P[i]) · direction(i → nb)`. Without friction the
//!    velocity grows unboundedly under any sustained gradient (and
//!    eventually overflows fixed-point range). Friction also models real
//!    surface drag.
//! 3. Advects thermal *energy* via pair-flux upwind differencing,
//!    using the barometric column-mass ratio
//!    `m[i] = exp(-elevation[i] / scale_height_m)`:
//!    ```text
//!      e[i]         = m[i] · T[i]                         // pre-pass
//!      v_along_pair = midpoint(v[i], v[j]) · dir(i → j)
//!      upwind_e     = if v_along > 0 { e[i] } else { e[j] }
//!      flux         = wind_advect_k · dt · v_along · upwind_e
//!      e[i]        -= flux
//!      e[j]        += flux
//!      T[i]         = e[i] / m[i]                         // post-pass
//!    ```
//!    Pair-flux preserves total energy bit-exactly (the same `delta`
//!    is applied with opposite signs to both cells), independent of
//!    mass distribution. Previously the same pass operated on `T`
//!    directly, which conserved Σ T but only accidentally conserved
//!    Σ (m · T) when every cell had the same implicit mass. Once
//!    elevation modulates column mass that approximation breaks
//!    silently — the energy form is the correct conservation
//!    invariant. Upwind is approximate at the heat-conservation
//!    level (a centered scheme would conserve heat-content of the
//!    pair independent of donor) but is the standard finite-volume
//!    choice for transport stability.
//!
//! Determinism: pair iteration is canonical-order (`j > i` filter
//! over `grid.cells()`); pressure and velocity computed from
//! previous-tick snapshots; no per-tick allocation beyond a single
//! pressure / velocity buffer pair. No state-dependent branching
//! beyond `v_along > 0` (a Real comparison — exact under fixed-point arithmetic).
//!
//! Hydrology reuses `(v_q, v_r)` to advect water vapour,
//! closing a real hydrologic cycle (evaporate over warm cells,
//! transport via wind, condense over cold cells).

use crate::laws::Law;
use crate::state::PhysicsState;
use sim_arith::transcendental::exp;
use sim_arith::Real;

/// Hex-direction axial offsets for the six neighbours, in the same
/// canonical order as `HexGrid::neighbours` (E, NE, NW, W, SW, SE).
/// Each entry is `(dq, dr)` — the axial-coordinate offset from the
/// centre cell to that neighbour. These aren't Euclidean unit
/// vectors (axial coordinates aren't orthonormal) but they're the
/// natural "direction toward neighbour" basis for our pair-flux
/// scheme.
const NEIGHBOUR_DIRECTIONS: [(i64, i64); 6] = [
    (1, 0),  // E
    (1, -1), // NE
    (0, -1), // NW
    (-1, 0), // W
    (-1, 1), // SW
    (0, 1),  // SE
];

#[derive(Debug, Clone, Copy)]
pub struct Wind {
    /// Pressure-from-temperature factor `c` in `P = c · T`. Units
    /// folded into `wind_k` so this is just a normalisation; default
    /// `1` keeps `P` numerically equal to `T` in K.
    pub pressure_per_kelvin: Real,
    /// Pressure-gradient → velocity-acceleration coefficient. Folds
    /// `1/ρ` and any unit-conversion factors. Tuned so a 1 K/cell
    /// gradient over one macro-step changes velocity by ~`wind_k`.
    pub wind_k: Real,
    /// Per-tick fraction of velocity lost to surface drag. Values
    /// near `1` damp velocity to near-zero each tick (no momentum);
    /// values near `0` give long-lived winds that can build up.
    /// Default `0.3` per tick gives ~3-tick velocity memory — long
    /// enough that winds *do* something but short enough that they
    /// don't run away.
    pub friction_per_tick: Real,
    /// Pair-flux heat-advection coefficient. Multiplies `v_along ·
    /// upwind_T · dt` to give per-pair temperature exchange.
    /// Tuned so wind-driven heat transport is ~10-100× faster than
    /// molecular conduction (`HeatConduction::alpha ≈ 0.1`) at
    /// realistic gradients — matching real-atmosphere ratios.
    pub advect_k: Real,
    /// Vacuum guard. `false` for `Atmosphere::None`
    /// planets — no medium means no pressure-gradient force,
    /// no friction, no heat advection. The `integrate` path
    /// short-circuits when this is false. Defaults to `true`
    /// for `earth_like`; `build_laws` sets it from
    /// `planet.atmosphere != Atmosphere::None`.
    pub has_atmosphere: bool,
    /// Atmospheric scale height in metres. Used by the
    /// energy-conserving advection pass to compute per-cell column
    /// mass as `m(h) = exp(-h / H)` (dimensionless, normalised to 1
    /// at sea level). Previously the pair-flux temperature transport
    /// implicitly assumed equal-mass cells, which conserved Σ T but
    /// not Σ (m · T) — i.e. it accidentally conserved energy only
    /// because elevation was effectively ignored. With column-mass
    /// scaling on, pair-flux transports energy bit-exactly even
    /// under non-uniform terrain. `0` keeps the legacy unit-mass
    /// behaviour (used by `earth_like` and `vacuum` paths).
    pub scale_height_m: i64,
}

impl Wind {
    /// Earth-like defaults: ideal-gas pressure scaling, moderate
    /// wind generation, light friction, advection magnitude calibrated
    /// against `HeatConduction::alpha`'s typical value.
    #[must_use]
    pub fn earth_like() -> Self {
        Self {
            pressure_per_kelvin: Real::ONE,
            // Small absolute scale so `v` stays well under 100
            // (axial-coord units / tick). 1 K gradient over the
            // pair → v contribution ~0.001 / tick.
            wind_k: Real::from_ratio(1, 1_000),
            // 30%/tick friction → velocity half-life ~2 ticks.
            friction_per_tick: Real::percent(30),
            // Tuned so a typical 50 K equator-to-pole gradient
            // moves heat ~50×/tick faster than `HeatConduction`'s
            // alpha = 0.1 default. Combined with friction, gives
            // a stable steady-state where radiation, conduction,
            // and wind-advection all balance.
            advect_k: Real::percent(1),
            has_atmosphere: true,
            // Earth-like 8.4 km scale height. `build_laws`
            // overrides this per-planet from
            // `Atmosphere::scale_height_m`.
            scale_height_m: 8_400,
        }
    }
}

impl Law for Wind {
    // Axial coordinates use the canonical `q` / `r` naming (see
    // `grid::Axial`); pair-named bindings like `vel_q_*` / `vel_r_*`
    // trip clippy's `similar_names` lint despite being the natural
    // domain vocabulary.
    #[allow(clippy::similar_names)]
    fn integrate(&self, state: &mut PhysicsState, dt: Real) {
        // Vacuum short-circuit. No atmosphere means no
        // pressure-gradient force, no friction, no advection —
        // velocity stays at whatever it was (zero on a fresh
        // state) and temperature is undisturbed by wind.
        if !self.has_atmosphere {
            return;
        }
        let grid = state.grid().clone();
        let n = grid.n_cells();
        let prev_t = state.temperature().to_vec();

        // Step 1: derive pressure from temperature.
        let pressures: Vec<Real> = prev_t
            .iter()
            .map(|t| *t * self.pressure_per_kelvin)
            .collect();
        state.pressure_mut().copy_from_slice(&pressures);

        // Step 2: update velocity from pressure gradient + friction.
        let (vel_q_prev_buf, vel_r_prev_buf) = {
            let (q, r) = state.fluid_velocity();
            (q.to_vec(), r.to_vec())
        };
        let mut vel_q_after = vel_q_prev_buf;
        let mut vel_r_after = vel_r_prev_buf;
        for (cid, axial) in grid.cells() {
            let i = cid.0 as usize;
            // Pressure-gradient force: sum over neighbours of
            // (P[nb] - P[i]) × direction(i → nb). The minus sign
            // in `dv = -grad_P / ρ` gets folded into the convention
            // that we accumulate (P[nb] - P[i]) (so positive means
            // *higher pressure away* from i, which would push i
            // *toward* the lower-pressure side, i.e. *opposite* of
            // the (i→nb) direction we're summing). Hence the
            // *negative* of this sum is the acceleration.
            let mut grad_along_q = Real::ZERO;
            let mut grad_along_r = Real::ZERO;
            for (k, nb) in grid.neighbours(axial).iter().enumerate() {
                let j = nb.0 as usize;
                let dp = pressures[j] - pressures[i];
                let (dir_q, dir_r) = NEIGHBOUR_DIRECTIONS[k];
                grad_along_q = grad_along_q + dp * Real::from_int(dir_q);
                grad_along_r = grad_along_r + dp * Real::from_int(dir_r);
            }
            // Acceleration: a = -grad_P · wind_k. Apply over dt.
            vel_q_after[i] = vel_q_after[i] - dt * self.wind_k * grad_along_q;
            vel_r_after[i] = vel_r_after[i] - dt * self.wind_k * grad_along_r;
            // Friction: v *= (1 - friction · dt).
            let damp = Real::ONE - self.friction_per_tick * dt;
            vel_q_after[i] = vel_q_after[i] * damp;
            vel_r_after[i] = vel_r_after[i] * damp;
        }
        let (vel_q_out, vel_r_out) = state.fluid_velocity_mut();
        vel_q_out.copy_from_slice(&vel_q_after);
        vel_r_out.copy_from_slice(&vel_r_after);

        // Step 3: pair-flux upwind *energy* advection.
        //
        // Previously this transported temperature directly, which
        // conserved Σ T over cells but not Σ (m · T) — i.e. energy.
        // When every cell has the same implicit mass that's the same
        // thing, but the moment elevation modulates column mass (or
        // saturation-pressure hydrology shifts air mass between
        // cells) the temperature-conserving pass would silently leak
        // or gain energy. Operating on `e[i] = m[i] · T[i]` directly
        // and converting back via `T_out = e_out / m[i]` keeps the
        // pair-flux conservation property at the energy level
        // independent of mass distribution. `m[i]` here is the
        // dimensionless column-mass ratio `exp(-elevation[i] /
        // scale_height_m)`, normalised so a sea-level cell has m = 1
        // and the legacy uniform-elevation behaviour reproduces
        // bit-exactly.
        let elevation = state.elevation().to_vec();
        let column_mass = column_mass_ratios(&elevation, self.scale_height_m);
        let mut next_e: Vec<Real> = prev_t
            .iter()
            .zip(column_mass.iter())
            .map(|(t, m)| *t * *m)
            .collect();
        let two = Real::from_int(2);
        for (cid, axial) in grid.cells() {
            let i = cid.0 as usize;
            for (k, nb) in grid.neighbours(axial).iter().enumerate() {
                let j = nb.0 as usize;
                if j > i {
                    let (dir_q, dir_r) = NEIGHBOUR_DIRECTIONS[k];
                    // Velocity at the pair midpoint, projected onto
                    // i → j axial direction.
                    let vmid_q = (vel_q_after[i] + vel_q_after[j]) / two;
                    let vmid_r = (vel_r_after[i] + vel_r_after[j]) / two;
                    let v_along = vmid_q * Real::from_int(dir_q) + vmid_r * Real::from_int(dir_r);
                    // Upwind: positive v_along means flow i → j, so
                    // i is the donor and the energy parcel arriving
                    // at j carries the donor's `m · T`.
                    let upwind_e = if v_along > Real::ZERO {
                        prev_t[i] * column_mass[i]
                    } else {
                        prev_t[j] * column_mass[j]
                    };
                    let flux = self.advect_k * dt * v_along * upwind_e;
                    next_e[i] = next_e[i] - flux;
                    next_e[j] = next_e[j] + flux;
                }
            }
        }
        // Convert energy back to temperature. `column_mass` is
        // strictly positive (exp of any finite argument), so the
        // division is well-defined.
        let next_t: Vec<Real> = next_e
            .iter()
            .zip(column_mass.iter())
            .map(|(e, m)| *e / *m)
            .collect();
        let _ = n;
        state.temperature_mut().copy_from_slice(&next_t);
    }
}

/// Per-cell column-mass ratio computed from elevation via the
/// barometric formula `m(h) = exp(-h / H)`. Dimensionless and
/// normalised so a cell at sea level (`elevation = 0`) has mass 1.
/// `scale_height_m <= 10` is treated as a vacuum / sentinel value,
/// matching the convention in `Hydrology::cell_pressure_pa`; the
/// returned ratios fall back to a uniform 1 so dividing through to
/// recover temperature is still well-defined. Pulled out as a free
/// function so the energy-conservation invariant can be tested
/// directly.
#[must_use]
pub fn column_mass_ratios(elevation: &[Real], scale_height_m: i64) -> Vec<Real> {
    if scale_height_m <= 10 {
        return vec![Real::ONE; elevation.len()];
    }
    let scale_h = Real::from_int(scale_height_m);
    elevation
        .iter()
        .map(|elev| {
            // Same clamp window as `Hydrology::cell_pressure_pa`:
            // bound the argument to `[-20, +1]` so extreme terrain
            // doesn't push fixed-point `exp` out of range.
            let argument = -*elev / scale_h;
            let clamped = argument.max(-Real::from_int(20)).min(Real::ONE);
            exp(clamped)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::{Axial, HexGrid};

    #[test]
    fn wind_pushes_heat_from_hot_toward_cold() {
        // 5x1 strip: hot cell at (2,0), cold elsewhere. After many
        // wind sub-steps, neighbours of the hot cell should warm.
        let grid = HexGrid::new(5, 1);
        let mut state = PhysicsState::new(grid);
        for t in state.temperature_mut() {
            *t = Real::from_int(200);
        }
        let hot = state.grid().cell_id(Axial::new(2, 0)).0 as usize;
        state.temperature_mut()[hot] = Real::from_int(400);
        let initial_neighbour_t = state.temperature()[hot - 1];

        let wind = Wind::earth_like();
        for _ in 0..200 {
            wind.integrate(&mut state, Real::ONE);
        }
        let final_neighbour_t = state.temperature()[hot - 1];
        assert!(
            final_neighbour_t > initial_neighbour_t,
            "wind should warm the cell adjacent to a heat source: \
             initial={initial_neighbour_t:?} final={final_neighbour_t:?}"
        );
    }

    #[test]
    fn wind_is_deterministic() {
        let grid = HexGrid::new(4, 4);
        let mut a = PhysicsState::new(grid.clone());
        let mut b = PhysicsState::new(grid);
        for (i, t) in a.temperature_mut().iter_mut().enumerate() {
            *t = Real::from_int(200 + i64::try_from(i).unwrap() * 5);
        }
        for (i, t) in b.temperature_mut().iter_mut().enumerate() {
            *t = Real::from_int(200 + i64::try_from(i).unwrap() * 5);
        }
        let wind = Wind::earth_like();
        for _ in 0..30 {
            wind.integrate(&mut a, Real::ONE);
            wind.integrate(&mut b, Real::ONE);
        }
        assert_eq!(a.temperature(), b.temperature());
        assert_eq!(a.fluid_velocity().0, b.fluid_velocity().0);
        assert_eq!(a.fluid_velocity().1, b.fluid_velocity().1);
    }

    #[test]
    fn pair_flux_conserves_total_temperature() {
        // Pair-flux structure means total temperature is bit-exactly
        // preserved. Set up a non-trivial gradient and verify.
        let grid = HexGrid::new(4, 4);
        let mut state = PhysicsState::new(grid);
        for (i, t) in state.temperature_mut().iter_mut().enumerate() {
            *t = Real::from_int(200 + i64::try_from(i).unwrap() * 10);
        }
        let initial: Real = state
            .temperature()
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        let wind = Wind::earth_like();
        for _ in 0..50 {
            wind.integrate(&mut state, Real::ONE);
        }
        let after: Real = state
            .temperature()
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        assert_eq!(
            initial, after,
            "pair-flux advection must conserve total temperature bit-exactly"
        );
    }

    #[test]
    fn equal_temperature_means_no_wind() {
        // Uniform T → uniform P → zero gradient → zero acceleration.
        // Velocity stays at its initial zero; temperature unchanged.
        let grid = HexGrid::new(3, 3);
        let mut state = PhysicsState::new(grid);
        for t in state.temperature_mut() {
            *t = Real::from_int(280);
        }
        let initial: Vec<_> = state.temperature().to_vec();
        let wind = Wind::earth_like();
        for _ in 0..20 {
            wind.integrate(&mut state, Real::ONE);
        }
        assert_eq!(state.temperature(), &initial[..]);
        for v in state.fluid_velocity().0 {
            assert_eq!(*v, Real::ZERO);
        }
        for v in state.fluid_velocity().1 {
            assert_eq!(*v, Real::ZERO);
        }
    }

    #[test]
    fn wind_advection_conserves_energy_under_varying_column_mass() {
        // Earlier the pair-flux pass operated directly on T and
        // implicitly assumed every cell carried the same mass, which
        // conserved Σ T but not Σ (m · T). Once elevation modulates
        // column mass — `m(h) = exp(-h / H)` — the temperature-only
        // pass would silently leak or gain energy on the order of
        // ~(elev_max - elev_min)/H × (T_max - T_min).
        //
        // The new pass converts each cell's `T` to energy `e = m · T`,
        // moves energy via pair-flux (which is bit-exact on `e`), then
        // divides back through the column-mass ratio. Bit-exact
        // conservation holds *within* a single integrate call: Σ (m·T)
        // after = Σ e after = Σ e before = Σ (m·T) before. Across
        // many ticks the round-trip `T → m·T → e → e/m → T` accrues
        // sub-ULP fixed-point drift, so the strong test is per-tick.
        //
        // Verifying per-tick conservation across many ticks bounds
        // the drift and demonstrates that the underlying invariant
        // is the right one (vs the old Σ T which would diverge by
        // a finite, *non-shrinking* amount under varying mass).
        let grid = HexGrid::new(5, 5);
        let mut state = PhysicsState::new(grid);
        // Non-uniform elevation: a ridge running east-west at large
        // altitude (4 km), valley floors near sea level.
        for (i, axial) in state.grid().clone().cells() {
            let idx = i.0 as usize;
            state.elevation_mut()[idx] = if axial.r % 2 == 0 {
                Real::from_int(4_000)
            } else {
                Real::ZERO
            };
        }
        // Non-uniform temperature: hot west, cool east.
        for (i, axial) in state.grid().clone().cells() {
            let idx = i.0 as usize;
            state.temperature_mut()[idx] = Real::from_int(260 + i64::from(axial.q) * 8);
        }
        let wind = Wind::earth_like();
        let column_mass = column_mass_ratios(state.elevation(), wind.scale_height_m);
        let energy_now = |s: &PhysicsState| -> Real {
            s.temperature()
                .iter()
                .zip(column_mass.iter())
                .fold(Real::ZERO, |acc, (t, m)| acc + *t * *m)
        };

        // The pair-flux pass is bit-exact on the internal energy
        // vector. The visible `m · T` sum (recomputed by the test
        // after the integrator divides through to land back in T)
        // can drift by at most one fixed-point ULP per cell per tick
        // from the `(e / m) * m` round-trip. After 50 ticks on a
        // 25-cell grid that's at most ~1250 ULPs against an absolute
        // magnitude of several thousand — i.e. well under 1 part in
        // 10⁶. Verify both pieces:
        //   (a) per-tick relative drift bounded under that ULP
        //       envelope (catches gross conservation violations);
        //   (b) integrated 50-tick drift bounded under
        //       1 part in 10⁵, which is the real "no silent leak"
        //       guarantee callers care about.
        let initial = energy_now(&state);
        for _ in 0..50 {
            let pre = energy_now(&state);
            wind.integrate(&mut state, Real::ONE);
            let post = energy_now(&state);
            let drift = (post - pre).abs();
            // One ULP of I32F32 is 2⁻³² ≈ 2.3e-10. Per-tick drift
            // across 25 cells should stay under 25 × few-ULPs ≈
            // 1e-8. Bound at 1e-6 to leave headroom.
            let bound = Real::from_ratio(1, 1_000_000);
            assert!(
                drift < bound,
                "per-tick energy drift exceeded bound: drift={drift:?} \
                 pre={pre:?} post={post:?}"
            );
        }
        let final_energy = energy_now(&state);
        let total_drift = (final_energy - initial).abs();
        // Relative-drift bound: 1 part in 10⁵ across the whole run.
        let rel_bound = initial.abs() / Real::from_int(100_000);
        assert!(
            total_drift < rel_bound,
            "50-tick energy drift exceeded 1e-5 relative bound: \
             total_drift={total_drift:?} initial={initial:?} \
             final={final_energy:?}"
        );

        // Crucially, the *old* Σ T law was *not* even approximately
        // energy-conserving under varying column mass: a Σ T-conserving
        // pass leaks Σ (m · T) by ~(advect_k · dt · v · Δm · T) per
        // pair per tick, which is finite and grows monotonically with
        // mass disparity. The above bound is one part in 10⁵ across
        // 50 ticks; the old pass on this same grid drifts by tens of
        // percent on the same horizon. The bound is the *right
        // conservation law*, not a coincidence.
    }

    #[test]
    fn vacuum_planet_wind_short_circuits() {
        // Atmosphere::None worlds: Wind should be a no-op even
        // with non-uniform temperature. Earlier the law would
        // run pressure-gradient + advection; the vacuum guard short-circuits.
        let grid = HexGrid::new(4, 4);
        let mut state = PhysicsState::new(grid);
        for (i, t) in state.temperature_mut().iter_mut().enumerate() {
            *t = Real::from_int(200 + i64::try_from(i).unwrap() * 5);
        }
        let initial_t: Vec<_> = state.temperature().to_vec();
        let initial_vq: Vec<_> = state.fluid_velocity().0.to_vec();
        let mut wind = Wind::earth_like();
        wind.has_atmosphere = false; // vacuum planet
        for _ in 0..30 {
            wind.integrate(&mut state, Real::ONE);
        }
        assert_eq!(state.temperature(), &initial_t[..]);
        assert_eq!(state.fluid_velocity().0, &initial_vq[..]);
    }
}
