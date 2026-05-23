//! Subsurface ↔ surface conduction kernel (P1.1).
//!
//! Two-reservoir relaxation that drains the subsurface heat
//! reservoir back into the surface temperature field at a tuned
//! `CONDUCTION_K` rate. Strictly energy-conserving per cell
//! (modulo Q32.32 LSB drift): the same delta added to surface is
//! subtracted from subsurface.

use crate::state::PhysicsState;
use sim_arith::Real;

/// Surface ↔ subsurface conduction coefficient (P1.1). Per-tick
/// `delta_surface = (T_sub - T_surf) × CONDUCTION_K × dt`. Tuned
/// slow enough to give a multi-tick warm-up so the subsurface
/// reservoir accumulates heat over many macro-steps before the
/// surface follows. With `dt = 1` macro-step and a 20 K gradient
/// the per-tick surface bump is 0.02 K — visible on a 1000-tick
/// integration but invisible on a single macro-step, matching the
/// real icy-moon timescale where surface response lags subsurface
/// by orbital periods (Europa) or geologic eras (Enceladus).
#[inline]
fn conduction_k() -> Real {
    // 0.001 per tick. Use a Real::from_ratio so the constant stays
    // bit-exact in Q32.32.
    Real::from_ratio(1, 1_000)
}

/// Subsurface-to-surface conduction step (P1.1). For every cell:
///
/// ```text
///   delta = (T_subsurface - T_surface) × CONDUCTION_K × dt
///   T_surface     += delta
///   T_subsurface  -= delta
/// ```
///
/// This is the simplest energy-conserving "two-reservoir" relaxation
/// kernel: warm subsurface bleeds heat upward, cold surface gains it,
/// and the pair drifts toward equilibrium exponentially over many
/// ticks. The per-tick gain on a 20 K gradient is 0.02 K (with
/// `CONDUCTION_K = 0.001` and `dt = 1`), so a sealed Europa-class
/// planet with no other forcing relaxes its subsurface ocean by
/// ~10 % in ~100 ticks — slow enough to be a multi-tick warm-up,
/// fast enough to register on the 1000-tick canary.
///
/// Strictly energy-conserving by construction: the same delta is
/// added to surface and subtracted from subsurface, so the per-cell
/// `T_surf + T_sub` is invariant under this kernel (modulo Q32.32
/// LSB drift from the multiply, which sits at ~1e-10 per call).
///
/// `dt` is the conduction sub-step length in macro-step units. The
/// orchestrator passes `cfg.heat_dt` so the conduction cadence
/// matches the other heat-band kernels.
pub fn subsurface_conduction_step(state: &mut PhysicsState, dt: Real) {
    let n = state
        .temperature()
        .len()
        .min(state.subsurface_temperature().len());
    if n == 0 {
        return;
    }
    let k_dt = conduction_k().saturating_mul(dt);
    // Snapshot the surface temperatures so we can read both fields
    // while writing one. Single-cell read-then-write is cheaper than
    // a full clone in the common case (n ~ 100-1000).
    for i in 0..n {
        let t_surf = state.temperature()[i];
        let t_sub = state.subsurface_temperature()[i];
        let gradient = t_sub - t_surf;
        let delta = gradient.saturating_mul(k_dt);
        state.temperature_mut()[i] = t_surf.saturating_add(delta);
        // Subtract from subsurface so total energy is conserved per
        // cell.
        state.subsurface_temperature_mut()[i] = t_sub - delta;
    }
}
