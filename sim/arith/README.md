# sim/arith

Deterministic real arithmetic. Wraps a Q-format fixed-point integer
behind a `Real` newtype. All real-valued sim code (physics, fits,
recognition, scoring) routes through this crate. **Direct `f64` use
outside this crate is forbidden.**

## Status

- **Default landed**: `Real` over Q32.32 fixed-point, ±~2.1×10⁹
  range, ~2.3×10⁻¹⁰ precision.
- **Transcendentals landed** (`sqrt`, `ln`, `exp`, `pow`) —
  deterministic Q-format implementations. `sin` / `cos` still
  stubbed; deferred until M1.5 shallow-water momentum needs them.

## Design

- `Real` is a transparent newtype over `fixed::types::I32F32`.
  `+`, `-`, `*`, `/`, comparison are integer ops — same input bits →
  same output bits on every platform.
- Per-module wider Q-formats are allowed when precision/range demands
  it (Q24.40 for ill-conditioned fits, Q16.48 for orbital). Adding a
  new format means adding a new typed wrapper here, not reaching for
  `f64`.
- `to_f64_for_display` exists for human-readable output only —
  calling it inside a sim loop is a bug.

## Transcendentals

Module: `arith::transcendental`. All implementations operate on the
underlying integer bits and are bit-deterministic across platforms.

- `sqrt(x)` — Newton-Raphson `y_{n+1} = (y_n + x/y_n)/2`. Initial
  guess `2^((h+32)/2)` from the highest set bit of the raw bits.
  Capped at 32 iterations with oscillation detection.
- `ln(x)` — range-reduces to `[1, 2)` via bit shifts on the raw
  bits, then `2(v + v³/3 + v⁵/5 + …)` with `v = (m-1)/(m+1)` for
  15 odd terms. `v ≤ 1/3` ⇒ truncation error ≪ Q32.32 LSB.
- `exp(x)` — range-reduces by `x = k·ln2 + r` with `|r| ≤ ln2/2`,
  then 15-term Taylor; multiplies by `2^k` via shift. Asserts on
  overflow (`k > 30`); underflows to `ZERO` for `k < -63`.
- `pow(a, b)` = `exp(b · ln(a))`. Conventions: `pow(0, 0) = 1`,
  `pow(0, b > 0) = 0`, `pow(0, b < 0)` panics.
- `sin` / `cos` — fixed-iteration CORDIC; deferred until M1.5
  shallow-water momentum needs them.

Tests live in `transcendental_tests`: parity against `f64::sqrt/ln/exp`
within `1e-7` relative tolerance, determinism (same input twice →
identical raw bits), and a Clausius-Clapeyron smoke test.

## Cited by

`sim/physics` (callers).
