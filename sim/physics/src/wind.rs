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
//! Stability is enforced via adaptive sub-stepping driven by the
//! acoustic-aware CFL condition (`(|u_max| + c_s) · advect_k · dt ≤
//! CFL`). Previously the integrator ran the kernel at the caller's
//! requested `dt` and then *clamped* the post-update velocity into
//! the CFL stability envelope; that clamp limited a real physical
//! signal (high winds) rather than the numerical artifact (too-large
//! `dt`). Sub-stepping addresses the root cause: when the requested
//! `dt` exceeds the CFL bound the integrator subdivides into
//! `ceil(dt / dt_max)` equal pieces and runs the unclamped kernel on
//! each. The `c_s` term in the bound is the ideal-gas sound speed
//! `sqrt(γ · P / ρ)` evaluated from the current temperature field,
//! per the plan v2 Item 1a directive that real atmospheric CFL
//! includes the acoustic mode.
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
use sim_arith::transcendental::{exp, sqrt};
use sim_arith::Real;

/// Hard cap on the number of CFL sub-steps a single `Wind::integrate`
/// call will spawn. A pathological seed (extreme `wind_k` ×
/// atmospheric pressure coupling combined with a huge requested
/// macro-`dt`) can in principle ask for thousands of sub-steps;
/// honouring that silently would lock the tick. Sixteen is the
/// soft ceiling agreed in the plan (Item 1b) — beyond it we fall
/// back to sixteen sub-steps and raise a `debug_assert!` so debug
/// builds surface the issue while release builds degrade gracefully.
const MAX_WIND_SUB_STEPS: u32 = 16;

/// CFL safety factor. Standard practice for upwind donor-cell
/// schemes is `≤ 1`; `0.5` leaves headroom for the operator-split
/// coupling with hydrology and absorbs the few-percent error in
/// the constant `γ` (heat-capacity ratio) used to derive `c_s`.
const CFL_SAFETY: (i64, i64) = (1, 2);

/// Heat-capacity ratio for the ideal-gas sound-speed formula
/// `c_s = sqrt(γ · P / ρ)`. Earth's diatomic-dominated atmosphere
/// sits at `γ ≈ 1.4` (`7/5`); the same value is good to a few
/// percent for the other modelled atmospheres (Mars CO₂ → 1.30,
/// Venus CO₂ → 1.30, Titan N₂/CH₄ mix → 1.32). The CFL safety
/// factor (`0.5`) absorbs the residual mis-estimate.
const GAMMA_GAS: (i64, i64) = (7, 5);

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

/// Earth-baseline pressure-gradient acceleration coefficient. The
/// `for_gravity` constructor divides this by `gravity_g` so a low-
/// gravity planet sees stronger per-K acceleration (same gradient,
/// less weight to overcome).
const WIND_K_EARTH: (i64, i64) = (1, 1_000);

/// Earth-like atmospheric scale height (m). Used as the default for
/// the back-compat `earth_like` constructor.
const EARTH_SCALE_HEIGHT_M: i64 = 8_400;

impl Wind {
    /// Earth-like defaults: ideal-gas pressure scaling, moderate
    /// wind generation, light friction, advection magnitude calibrated
    /// against `HeatConduction::alpha`'s typical value. Back-compat
    /// alias for `Wind::for_gravity(Real::ONE, EARTH_SCALE_HEIGHT_M)`.
    #[must_use]
    pub fn earth_like() -> Self {
        Self::for_gravity(Real::ONE, EARTH_SCALE_HEIGHT_M)
    }

    /// Build a `Wind` law tuned for a planet whose surface gravity is
    /// `gravity_g` Earth-gravities and whose atmospheric scale height
    /// is `atmosphere_scale_height_m` metres. The `wind_k` coefficient
    /// scales as `1 / gravity_g` — pressure-gradient force per unit
    /// mass (`∇P / ρ`) is gravity-invariant at the level of the
    /// pointwise equation of motion, but the per-cell mass column the
    /// gradient acts against scales linearly with `g` (heavier air at
    /// the same scale height), so a fixed gradient produces a smaller
    /// per-mass acceleration in a high-gravity atmosphere. The
    /// `1/g` coupling is the simple physically-defensible choice: low-
    /// gravity worlds see stronger winds for the same temperature
    /// gradient, high-gravity worlds see weaker winds.
    #[must_use]
    pub fn for_gravity(gravity_g: Real, atmosphere_scale_height_m: i64) -> Self {
        Self {
            pressure_per_kelvin: Real::ONE,
            // Small absolute scale so `v` stays well under 100
            // (axial-coord units / tick). 1 K gradient over the
            // pair → v contribution ~0.001 / tick at Earth gravity;
            // a 0.5g world sees ~0.002 / tick.
            wind_k: wind_k_for_gravity(gravity_g),
            // 30%/tick friction → velocity half-life ~2 ticks.
            friction_per_tick: Real::percent(30),
            // Tuned so a typical 50 K equator-to-pole gradient
            // moves heat ~50×/tick faster than `HeatConduction`'s
            // alpha = 0.1 default. Combined with friction, gives
            // a stable steady-state where radiation, conduction,
            // and wind-advection all balance.
            advect_k: Real::percent(1),
            has_atmosphere: true,
            scale_height_m: atmosphere_scale_height_m,
        }
    }
}

/// Scale the Earth-baseline `wind_k` by `1 / gravity_g`.
/// `gravity_g` is in Earth-gravities (1.0 for Earth, 0.38 for Mars,
/// 2.0 for a 2g super-Earth). A non-positive input falls back to the
/// Earth baseline rather than producing infinite or negative
/// coefficients.
fn wind_k_for_gravity(gravity_g: Real) -> Real {
    let base = Real::from_ratio(WIND_K_EARTH.0, WIND_K_EARTH.1);
    if gravity_g <= Real::ZERO {
        return base;
    }
    base / gravity_g
}

impl Law for Wind {
    fn integrate(&self, state: &mut PhysicsState, dt: Real) {
        // Vacuum short-circuit. No atmosphere means no
        // pressure-gradient force, no friction, no advection —
        // velocity stays at whatever it was (zero on a fresh
        // state) and temperature is undisturbed by wind.
        if !self.has_atmosphere {
            return;
        }
        if dt <= Real::ZERO {
            return;
        }
        // Adaptive sub-stepping. The previous implementation ran the
        // whole kernel at the caller-requested `dt` and then *clamped*
        // the resulting velocity into the CFL stability envelope.
        // Clamping limits a real physical signal (high winds) rather
        // than the numerical artifact (too-large `dt` for the upwind
        // advection scheme's stability). The replacement subdivides
        // `dt` into `n_sub` equal pieces sized to satisfy the
        // acoustic-aware CFL condition
        //
        //   (|u_max| + c_s) · advect_k · dt_sub  ≤  CFL
        //
        // with `CFL = 0.5` and `c_s = sqrt(γ · P / ρ)` evaluated from
        // the current temperature field via the `P = pressure_per_kelvin
        // · T` ideal-gas proxy. `advect_k` plays the role of the
        // inverse cell length `1/Δx` in the model's axial-coordinate
        // units (it is the coefficient that multiplies `v · dt` in
        // the per-pair upwind flux, so its inverse is the effective
        // characteristic length the donor-cell scheme sees per pair).
        // Each sub-step runs the *unclamped* kernel; the pair-flux
        // bit-exact energy conservation and the symmetric pressure-
        // gradient force are preserved per sub-step and therefore
        // over the loop.
        let n_sub = self.sub_step_count(state, dt);
        let dt_sub = dt / Real::from_int(i64::from(n_sub));
        for _ in 0..n_sub {
            self.advance_one_sub_step(state, dt_sub);
        }
    }
}

impl Wind {
    /// Number of CFL-stable sub-steps `Wind::integrate` will use to
    /// advance a macro `dt`. Returns at least 1 (so the kernel always
    /// runs once) and at most `MAX_WIND_SUB_STEPS`; the uncapped
    /// estimate raises a `debug_assert!` if it exceeds the ceiling,
    /// matching the plan v2 Item 1b directive that pathological seeds
    /// should be rejected at worldgen rather than silently swallowed
    /// at runtime.
    #[must_use]
    pub fn sub_step_count(&self, state: &PhysicsState, dt: Real) -> u32 {
        if !self.has_atmosphere || dt <= Real::ZERO {
            return 1;
        }
        let dt_max = self.dt_max_cfl(state);
        if dt_max <= Real::ZERO || dt <= dt_max {
            return 1;
        }
        // `n = ceil(dt / dt_max)`. Compute via integer arithmetic on
        // the raw Q32.32 bits since `Real` doesn't expose a `ceil`.
        let ratio = dt / dt_max;
        let ratio_i: i64 = ratio.raw().to_num::<i64>();
        let fractional = ratio - Real::from_int(ratio_i);
        let n_i64 = if fractional > Real::ZERO {
            ratio_i + 1
        } else {
            ratio_i.max(1)
        };
        debug_assert!(
            n_i64 <= i64::from(MAX_WIND_SUB_STEPS),
            "wind sub-step count {n_i64} exceeded ceiling {MAX_WIND_SUB_STEPS}; \
             worldgen should reject seeds that demand more than \
             {MAX_WIND_SUB_STEPS} sub-steps (plan v2 Item 1b)"
        );
        let clamped = n_i64.clamp(1, i64::from(MAX_WIND_SUB_STEPS));
        u32::try_from(clamped).unwrap_or(MAX_WIND_SUB_STEPS)
    }

    /// Maximum stable sub-step size under the acoustic-aware CFL
    /// condition. Returns `Real::ZERO` if the denominator collapses
    /// (no advection coefficient, no wind speed, no sound speed) —
    /// callers treat that as "single sub-step is fine".
    #[must_use]
    pub fn dt_max_cfl(&self, state: &PhysicsState) -> Real {
        if self.advect_k <= Real::ZERO {
            return Real::ZERO;
        }
        let (vq, vr) = state.fluid_velocity();
        let mut u_sq_max = Real::ZERO;
        for (q, r) in vq.iter().zip(vr.iter()) {
            // Compare |v|² rather than |v| so we can defer the sqrt
            // to a single call; cheaper and avoids per-cell sqrt
            // round-off accumulating.
            let sq = (*q) * (*q) + (*r) * (*r);
            if sq > u_sq_max {
                u_sq_max = sq;
            }
        }
        let u_max = sqrt(u_sq_max);
        let c_s = self.sound_speed(state);
        let denom = (u_max + c_s) * self.advect_k;
        if denom <= Real::ZERO {
            return Real::ZERO;
        }
        Real::from_ratio(CFL_SAFETY.0, CFL_SAFETY.1) / denom
    }

    /// Ideal-gas sound speed `c_s = sqrt(γ · P / ρ)` in the kernel's
    /// internal velocity units. The wind kernel models pressure as
    /// `P = pressure_per_kelvin · T` with `1/ρ` rolled into `wind_k`
    /// (the same coefficient that turns `grad P` into per-tick
    /// acceleration). The dimensionally-consistent internal sound
    /// speed is therefore `sqrt(γ · wind_k · pressure_per_kelvin · T)`.
    /// We use the *maximum* per-cell temperature as the reference —
    /// the worst case for stability — so a hot cell raises local
    /// `c_s` and pulls the CFL limit tighter, which is exactly what
    /// we want.
    #[must_use]
    pub fn sound_speed(&self, state: &PhysicsState) -> Real {
        let mut t_max = Real::ZERO;
        for t in state.temperature() {
            if *t > t_max {
                t_max = *t;
            }
        }
        if t_max <= Real::ZERO {
            return Real::ZERO;
        }
        let gamma = Real::from_ratio(GAMMA_GAS.0, GAMMA_GAS.1);
        let cs_sq = gamma * self.wind_k * self.pressure_per_kelvin * t_max;
        if cs_sq <= Real::ZERO {
            return Real::ZERO;
        }
        sqrt(cs_sq)
    }

    /// One CFL-stable sub-step of the wind kernel. The integration
    /// (pressure derivation → pressure-gradient + friction velocity
    /// update → pair-flux energy advection) matches the pre-adaptive
    /// implementation except the post-update velocity *clamp* is
    /// gone: callers reach this method only with `dt_sub ≤
    /// dt_max_cfl(state)`, so the upwind branch is guaranteed
    /// stable without artificially capping the physical signal.
    //
    // Axial coordinates use the canonical `q` / `r` naming (see
    // `grid::Axial`); pair-named bindings like `vel_q_*` / `vel_r_*`
    // trip clippy's `similar_names` lint despite being the natural
    // domain vocabulary.
    #[allow(clippy::similar_names)]
    fn advance_one_sub_step(&self, state: &mut PhysicsState, dt: Real) {
        let grid = state.grid().clone();
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
    // Floor `m` at 1% (0.01) to keep the divide-by-mass at the end
    // of Wind's energy-conserving advection from overflowing. In
    // pure math, `m = exp(-elev/H)` is strictly positive — but in
    // Q32.32 fixed-point dividing energy `e` by `m ≈ 2e-9` overflows
    // when `e > 4.3`. The `-20` lower clamp on the exp argument
    // alone left `m` near 2e-9 in pathological worlds (terrain peak
    // 30km on 8km scale-height). 0.01 is the practical floor: it
    // means an extreme high-altitude cell has 1% of sea-level column
    // mass at most for the purposes of the energy / temperature
    // round-trip, which keeps fixed-point safe without distorting
    // the conservation invariant materially (a 1% floor is the same
    // order as the existing `m_safe` floor in the integrator).
    let m_floor = Real::from_ratio(1, 100);
    elevation
        .iter()
        .map(|elev| {
            let argument = -*elev / scale_h;
            let clamped = argument.max(-Real::from_int(20)).min(Real::ONE);
            exp(clamped).max(m_floor)
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

    #[test]
    fn adaptive_dt_handles_supersonic_request() {
        // Build a wind field with a velocity that would have triggered
        // the old velocity clamp (the clamp's bound was
        // `|v_max| = 0.5 / (advect_k · dt) = 50` for the earth-like
        // defaults at `dt = 1`). Seed `|v| = 200` so the requested
        // step is firmly above the CFL stability envelope; the
        // adaptive sub-stepping must subdivide enough that the kernel
        // completes without breaking the pair-flux conservation
        // invariants (Σ (m·T) bit-exact per sub-step → bounded total
        // drift across the macro-step).
        let grid = HexGrid::new(6, 6);
        let mut state = PhysicsState::new(grid);
        for (i, t) in state.temperature_mut().iter_mut().enumerate() {
            *t = Real::from_int(260 + i64::try_from(i).unwrap() * 4);
        }
        // Seed the velocity field above the old clamp ceiling.
        {
            let (vq, vr) = state.fluid_velocity_mut();
            for v in vq.iter_mut() {
                *v = Real::from_int(200);
            }
            for v in vr.iter_mut() {
                *v = Real::from_int(-150);
            }
        }
        let wind = Wind::earth_like();
        // Sub-stepping should kick in: the CFL bound on this state is
        // `dt_max = 0.5 / ((|u| + c_s) · advect_k)` ≈ `0.5 / (250 ·
        // 0.01) = 0.2`, so a `dt = 1` request needs at least
        // `ceil(1 / 0.2) = 5` sub-steps. Assert that observation
        // before checking conservation — it's the load-bearing test
        // that the adaptive path activated.
        let n_sub = wind.sub_step_count(&state, Real::ONE);
        assert!(
            n_sub >= 2,
            "supersonic-equivalent velocity must trigger sub-stepping, got n_sub={n_sub}"
        );
        // Pair-flux conservation: total energy Σ (m · T) is bit-exact
        // per sub-step (column_mass ≡ 1 here since elevation is zero,
        // so this collapses to Σ T which is also bit-exact). After
        // the macro step the total temperature must be unchanged.
        let initial: Real = state
            .temperature()
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        wind.integrate(&mut state, Real::ONE);
        let after: Real = state
            .temperature()
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        assert_eq!(
            initial, after,
            "adaptive sub-stepping must preserve pair-flux total temperature: \
             initial={initial:?} after={after:?}"
        );
    }

    #[test]
    fn sub_step_count_grows_with_velocity() {
        // The sub-step count `n = ceil(dt / dt_max)` is monotone in
        // `|u_max|` because `dt_max ∝ 1 / (|u_max| + c_s)`. Seed
        // three states with progressively larger velocity magnitudes
        // and verify that the sub-step count is non-decreasing.
        let grid = HexGrid::new(5, 5);
        let make_state = |v_mag: i64| {
            let mut s = PhysicsState::new(grid.clone());
            for t in s.temperature_mut() {
                *t = Real::from_int(280);
            }
            let (vq, _vr) = s.fluid_velocity_mut();
            for v in vq.iter_mut() {
                *v = Real::from_int(v_mag);
            }
            s
        };
        let wind = Wind::earth_like();
        // Pick velocities so the highest one stays within the
        // `MAX_WIND_SUB_STEPS = 16` ceiling. With the earth-like
        // defaults `dt_max ≈ 0.5 / ((|u| + c_s) · 0.01)` for `dt = 1`,
        // so `|u| ≤ 800` keeps the implied sub-step demand ≤ 16
        // (`ceil(1 / (0.5 / (800 · 0.01))) = 16`). The debug-assert
        // in `sub_step_count` exists to surface pathological seeds
        // that *exceed* the ceiling — see
        // `sub_step_count_caps_pathological_demand_in_release`.
        let n_low = wind.sub_step_count(&make_state(10), Real::ONE);
        let n_mid = wind.sub_step_count(&make_state(100), Real::ONE);
        let n_high = wind.sub_step_count(&make_state(700), Real::ONE);
        assert!(
            n_low <= n_mid && n_mid <= n_high,
            "sub-step count must be monotone in |u_max|: \
             low={n_low} mid={n_mid} high={n_high}"
        );
        // At least one of the larger seeds should require >1 sub-step
        // — otherwise we're not exercising the adaptive path at all.
        assert!(
            n_high > 1,
            "high-velocity seed must require sub-stepping: n_high={n_high}"
        );
        // The cap is honoured for in-range requests: `MAX_WIND_SUB_STEPS`
        // is the absolute ceiling. We probe just *below* the cap here
        // rather than firing a debug-mode panic with a wildly
        // out-of-range velocity (the `debug_assert!` inside
        // `sub_step_count` exists precisely to surface that pathology
        // — see `sub_step_count_caps_pathological_demand_in_release`
        // for the cap-behaviour test that exercises the >cap branch).
        assert!(
            n_high <= MAX_WIND_SUB_STEPS,
            "sub-step count must respect MAX_WIND_SUB_STEPS ceiling: \
             n_high={n_high} cap={MAX_WIND_SUB_STEPS}"
        );
    }

    /// Release-mode-only cap test: a pathologically large requested
    /// velocity would demand more than `MAX_WIND_SUB_STEPS` sub-steps.
    /// The integrator must degrade gracefully (clamp to the ceiling)
    /// rather than spawn thousands of sub-steps and hang the tick.
    /// In debug builds the `debug_assert!` fires instead — that's the
    /// surface-the-pathology contract — so this test is release-only.
    #[cfg(not(debug_assertions))]
    #[test]
    fn sub_step_count_caps_pathological_demand_in_release() {
        let grid = HexGrid::new(3, 3);
        let mut state = PhysicsState::new(grid);
        for t in state.temperature_mut() {
            *t = Real::from_int(280);
        }
        let (vq, _vr) = state.fluid_velocity_mut();
        for v in vq.iter_mut() {
            *v = Real::from_int(1_000_000);
        }
        let wind = Wind::earth_like();
        let n = wind.sub_step_count(&state, Real::ONE);
        assert!(
            n <= MAX_WIND_SUB_STEPS,
            "release builds must clamp sub-step count at MAX_WIND_SUB_STEPS \
             rather than honour pathological demands: n={n} cap={MAX_WIND_SUB_STEPS}"
        );
    }

    #[test]
    fn dt_max_cfl_matches_acoustic_formula() {
        // Parametric check: with a quiescent atmosphere (u = 0), the
        // CFL bound collapses to `dt_max = CFL / (c_s · advect_k)`
        // with `c_s = sqrt(γ · wind_k · pressure_per_kelvin · T_max)`.
        // Verify the integrator's `dt_max_cfl` matches the closed form
        // within fixed-point tolerance.
        let grid = HexGrid::new(3, 3);
        let mut state = PhysicsState::new(grid);
        for t in state.temperature_mut() {
            *t = Real::from_int(300);
        }
        let wind = Wind::earth_like();
        let c_s_observed = wind.sound_speed(&state);
        // Closed form: c_s = sqrt(γ · wind_k · ppk · T_max).
        let gamma = Real::from_ratio(GAMMA_GAS.0, GAMMA_GAS.1);
        let cs_sq_expected =
            gamma * wind.wind_k * wind.pressure_per_kelvin * Real::from_int(300);
        let c_s_expected = sqrt(cs_sq_expected);
        assert_eq!(
            c_s_observed, c_s_expected,
            "sound_speed must match sqrt(γ · wind_k · ppk · T_max)"
        );
        // dt_max = CFL / (c_s · advect_k) when |u_max| = 0.
        let dt_max_observed = wind.dt_max_cfl(&state);
        let dt_max_expected =
            Real::from_ratio(CFL_SAFETY.0, CFL_SAFETY.1) / (c_s_expected * wind.advect_k);
        assert_eq!(
            dt_max_observed, dt_max_expected,
            "dt_max_cfl must match CFL / (c_s · advect_k) at zero wind"
        );
    }

    #[test]
    fn wind_scales_with_gravity() {
        // T3: a low-gravity planet's `wind_k` must be larger than a
        // high-gravity planet's (1/g coupling), so the same per-K
        // temperature gradient produces a larger per-tick velocity
        // change. Drive the kernel on identical states with two
        // gravities and verify that the low-g state ends up with a
        // measurably higher wind speed magnitude.
        let grid = HexGrid::new(5, 1);
        let make_state = || {
            let mut s = PhysicsState::new(grid.clone());
            for (i, t) in s.temperature_mut().iter_mut().enumerate() {
                // Strong west-to-east gradient: 200 K to 360 K.
                *t = Real::from_int(200 + i64::try_from(i).unwrap() * 40);
            }
            s
        };
        let mut low_g_state = make_state();
        let mut high_g_state = make_state();
        // 0.4g (Mars-like) vs 2g (super-Earth). Earth-baseline scale
        // height (8.4 km) on both so the only difference is gravity.
        let low_g = Wind::for_gravity(Real::from_ratio(4, 10), EARTH_SCALE_HEIGHT_M);
        let high_g = Wind::for_gravity(Real::from_int(2), EARTH_SCALE_HEIGHT_M);
        assert!(
            low_g.wind_k > high_g.wind_k,
            "low-gravity planet must have larger wind_k than high-gravity planet: \
             low_g.wind_k={:?} high_g.wind_k={:?}",
            low_g.wind_k,
            high_g.wind_k
        );
        // Drive a handful of ticks so the per-K pressure-gradient
        // acceleration accumulates into a real velocity. We compare
        // the L2 wind-speed sum across the grid.
        for _ in 0..5 {
            low_g.integrate(&mut low_g_state, Real::ONE);
            high_g.integrate(&mut high_g_state, Real::ONE);
        }
        let speed_l1 = |s: &PhysicsState| -> Real {
            let (vq, vr) = s.fluid_velocity();
            vq.iter()
                .zip(vr.iter())
                .fold(Real::ZERO, |acc, (q, r)| acc + q.abs() + r.abs())
        };
        let low_g_speed = speed_l1(&low_g_state);
        let high_g_speed = speed_l1(&high_g_state);
        assert!(
            low_g_speed > high_g_speed,
            "low-gravity planet must develop higher wind speeds for the same gradient: \
             low_g_speed={low_g_speed:?} high_g_speed={high_g_speed:?}"
        );
        // Earth-equivalent: `for_gravity(Real::ONE, EARTH_SCALE_HEIGHT_M)`
        // must reproduce the legacy `earth_like` coefficients
        // bit-exactly so existing sim-physics tests stay green.
        let earth = Wind::for_gravity(Real::ONE, EARTH_SCALE_HEIGHT_M);
        let legacy = Wind::earth_like();
        assert_eq!(
            earth.wind_k, legacy.wind_k,
            "Real::ONE gravity must reproduce earth_like wind_k exactly"
        );
        assert_eq!(earth.advect_k, legacy.advect_k);
        assert_eq!(earth.friction_per_tick, legacy.friction_per_tick);
        assert_eq!(earth.scale_height_m, legacy.scale_height_m);
        assert_eq!(earth.has_atmosphere, legacy.has_atmosphere);
        assert_eq!(earth.pressure_per_kelvin, legacy.pressure_per_kelvin);
    }
}
