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

/// Convert a `Q96.32` raw-bits value (as emitted by `Pop`-typed
/// fields like populations + capacities) to its `f64` magnitude.
/// Display-only; lossy past `2^53` (f64 mantissa) but adequate for
/// "how many billions of people" rendering.
#[inline]
#[must_use]
pub fn pop_q32_to_f64(raw: i128) -> f64 {
    raw as f64 / Q32_DIVISOR
}

/// Format a population for compact display. Switches between bare
/// integer (< 1,000), thousands ("12.3k"), millions ("47.8M"),
/// billions ("9.2B"), and trillions ("1.5T") so post-industrial
/// civs stay readable when their pop crosses what `{:.0}` would
/// otherwise render as a 10-digit unbroken number. Negative input
/// clamps to zero (sub-integer floating-point sum noise).
#[must_use]
pub fn fmt_pop(p: f64) -> String {
    let p = p.max(0.0);
    if p < 1_000.0 {
        format!("{p:.0}")
    } else if p < 1_000_000.0 {
        format!("{:.1}k", p / 1_000.0)
    } else if p < 1_000_000_000.0 {
        format!("{:.1}M", p / 1_000_000.0)
    } else if p < 1_000_000_000_000.0 {
        format!("{:.1}B", p / 1_000_000_000.0)
    } else {
        format!("{:.1}T", p / 1_000_000_000_000.0)
    }
}
