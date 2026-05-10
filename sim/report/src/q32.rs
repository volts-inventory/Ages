//! `Q32.32` raw-bits → `f64` for display in the report. The sim emits
//! every real-valued field as `i64` raw bits so the event log stays
//! bit-exact deterministic across platforms; the report only needs
//! values for human-readable rendering, so a lossy `f64` suffices.

const Q32_DIVISOR: f64 = (1_u64 << 32) as f64;

/// Convert a `Q32.32` raw-bits value (as emitted in protocol events)
/// to its `f64` magnitude. Display-only; never feed back into a sim.
#[inline]
#[must_use]
pub fn q32_to_f64(raw: i64) -> f64 {
    raw as f64 / Q32_DIVISOR
}
