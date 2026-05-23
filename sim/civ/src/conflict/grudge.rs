//! Per-pair grudge accumulator with lazy decay.
//!
//! The grudge score subtracts from kinship in `kinship_pair` so a
//! pair that's been at war for decades reads as fundamentally
//! hostile in kinship space even if they share religion.
//!
//! Asymmetric: loser holds longer (winner forgets at ~0.001/tick,
//! loser at half that — defeated civs remember decades, victors
//! within years).

use sim_arith::Real;
use std::collections::BTreeMap;

/// Grudge increment per skirmish (the round-level loss applied by
/// `resolve`). Asymmetric: loser holds longer. Loser's grudge
/// against winner gets `GRUDGE_BUMP_LOSER`, winner's grudge against
/// loser gets `GRUDGE_BUMP_WINNER` — both grow during a multi-round
/// war but the loser's accumulator climbs faster *and* decays
/// slower (see `GRUDGE_DECAY_PER_TICK_LOSER` /
/// `GRUDGE_DECAY_PER_TICK_WINNER`).
pub(crate) const GRUDGE_BUMP_WINNER: (i64, i64) = (5, 100);
pub(crate) const GRUDGE_BUMP_LOSER: (i64, i64) = (10, 100);
/// Per-tick lazy decay rates. Applied at kinship-read time using
/// the stored `last_update_tick` so we don't need a per-tick
/// maintenance pass. Winner forgets at ~0.001/tick, loser at half
/// that — the defeated civ remembers significantly longer (decades
/// rather than years).
pub(crate) const GRUDGE_DECAY_PER_TICK_WINNER: (i64, i64) = (1, 1000);
pub(crate) const GRUDGE_DECAY_PER_TICK_LOSER: (i64, i64) = (5, 10_000);
/// Cap on a single grudge score. Prevents an infinite-war
/// edge case (the +0.05/0.10 bumps every 75 ticks would otherwise
/// drift unbounded under no decay window).
///
/// Stays `pub` because `Civ::bump_grudge` (in `state.rs`) reads it
/// to clamp accumulation.
pub const GRUDGE_CEILING: (i64, i64) = (60, 100);

/// Resolve a grudge score for the current tick, applying the
/// per-side decay rate retroactively. Returns `0` if the entry
/// is missing or the decay has driven the score to zero or below.
#[must_use]
pub(crate) fn decayed_grudge(
    grudges: &BTreeMap<u32, (Real, u64)>,
    other_id: u32,
    current_tick: u64,
    decay_per_tick: Real,
) -> Real {
    let (raw, last_tick) = match grudges.get(&other_id) {
        Some(v) => *v,
        None => return Real::ZERO,
    };
    let elapsed = current_tick.saturating_sub(last_tick);
    let elapsed_real = Real::from_int(i64::try_from(elapsed).unwrap_or(i64::MAX));
    let decayed = raw - decay_per_tick * elapsed_real;
    decayed.max(Real::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grudge_decays_over_time() {
        let g: BTreeMap<u32, (Real, u64)> = {
            let mut m = BTreeMap::new();
            m.insert(2u32, (Real::percent(20), 100));
            m
        };
        // Slowest decay (loser memory): 5/10_000 per tick.
        let decay = Real::from_ratio(GRUDGE_DECAY_PER_TICK_LOSER.0, GRUDGE_DECAY_PER_TICK_LOSER.1);
        // 200 ticks later: 0.20 - 0.0005*200 = 0.10.
        let d = decayed_grudge(&g, 2, 300, decay);
        let expected = Real::percent(10);
        let drift = if d > expected {
            d - expected
        } else {
            expected - d
        };
        assert!(
            drift < Real::percent(1),
            "grudge should decay to ~0.10; got {d:?}"
        );
        // 1000 ticks later: 0.20 - 0.5 → clamped to 0.
        let d2 = decayed_grudge(&g, 2, 1100, decay);
        assert_eq!(d2, Real::ZERO);
    }
}
