//! `Law` trait — the contract every law family implements.
//!
//! Each law family (mechanics, fluids, heat, chemistry, EM) provides
//! an `integrate(state, dt)` method. The orchestration loop
//! calls each law at its own sub-step cadence, with fixed
//! coupling order, so the per-tick behaviour is bit-deterministic.

use crate::state::PhysicsState;
use sim_arith::Real;

pub trait Law {
    /// Advance this law's contribution by the given `dt`, mutating
    /// `state` in place. Reads from any field; writes only its own.
    /// The orchestrator guarantees a fixed coupling order.
    fn integrate(&self, state: &mut PhysicsState, dt: Real);
}
