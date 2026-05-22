//! Per-event record returned by `check_and_apply`. Surfaces
//! both the kind that fired and the realised population-loss
//! fraction so callers can emit `CatastropheFired` and update
//! `last_catastrophe_tick`.

use super::kind::CatastropheKind;
use sim_arith::Real;

/// One catastrophe applied this tick — what kind, and the
/// fraction of population lost.
#[derive(Debug, Clone, Copy)]
pub struct CatastropheRecord {
    pub kind: CatastropheKind,
    pub fraction_lost: Real,
}
