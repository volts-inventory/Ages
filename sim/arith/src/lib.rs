//! Deterministic real arithmetic.
//!
//! Wraps a Q-format fixed-point integer behind a `Real` newtype.
//! All real-valued sim code (physics, fits, recognition, scoring)
//! routes through this crate. Direct `f64` use outside this crate
//! is forbidden.
//!
//! Default Q-format: Q32.32 — 64-bit underlying, ±~2.1e9 range,
//! ~2.3e-10 precision. If a module needs different precision or
//! range, expose a separate type backed by a different Q-format
//! here rather than reaching for `f64`.

#![allow(clippy::module_name_repetitions)]

use core::ops::{Add, Div, Mul, Neg, Sub};
use fixed::types::I32F32;

/// The default real number type used across the sim. Q32.32.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct Real(I32F32);

impl Real {
    pub const ZERO: Self = Self(I32F32::ZERO);
    pub const ONE: Self = Self(I32F32::ONE);

    /// Construct from a integer.
    pub const fn from_int(n: i64) -> Self {
        Self(I32F32::const_from_int(n))
    }

    /// Construct from a numerator and a denominator. `den` must be
    /// non-zero. Used in tests and config loading; not in the inner
    /// loop.
    pub fn from_ratio(num: i64, den: i64) -> Self {
        debug_assert!(den != 0, "denominator must be non-zero");
        Self(I32F32::from_num(num) / I32F32::from_num(den))
    }

    /// The underlying Q-format value. Internal — exposed for
    /// debugging and serialisation only.
    pub fn raw(self) -> I32F32 {
        self.0
    }

    pub fn from_raw(v: I32F32) -> Self {
        Self(v)
    }

    #[must_use]
    pub fn min(self, other: Self) -> Self {
        if self.0 < other.0 {
            self
        } else {
            other
        }
    }

    #[must_use]
    pub fn max(self, other: Self) -> Self {
        if self.0 > other.0 {
            self
        } else {
            other
        }
    }

    #[must_use]
    pub fn abs(self) -> Self {
        if self.0 < I32F32::ZERO {
            Self(-self.0)
        } else {
            self
        }
    }

    /// Return a `f64` *for display only*. Not for use in deterministic
    /// computation; if you call this in a sim loop you have a bug.
    pub fn to_f64_for_display(self) -> f64 {
        self.0.to_num()
    }
}

impl Add for Real {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self(self.0 + rhs.0)
    }
}

impl Sub for Real {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self(self.0 - rhs.0)
    }
}

impl Mul for Real {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self(self.0 * rhs.0)
    }
}

impl Div for Real {
    type Output = Self;
    fn div(self, rhs: Self) -> Self {
        Self(self.0 / rhs.0)
    }
}

impl Neg for Real {
    type Output = Self;
    fn neg(self) -> Self {
        Self(-self.0)
    }
}

impl core::fmt::Display for Real {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Deterministic transcendentals over `Real` (Q32.32). All
/// operations work on the underlying integer bit pattern; same
/// inputs produce the same output bits on every platform.
///
/// Strategies:
/// - `sqrt` — Newton-Raphson with initial guess derived from the
///   highest set bit of the underlying integer.
/// - `ln` — range-reduce to `[1, 2)` via bit shifts, then Taylor
///   series `2(v + v³/3 + v⁵/5 + …)` with `v = (m-1)/(m+1)`.
/// - `exp` — range-reduce by writing `x = k·ln2 + r` with
///   `|r| ≤ ln2/2`, then standard Taylor; multiply by `2^k` via
///   bit shift.
/// - `pow(a, b) = exp(b·ln(a))`.
/// - `sin` / `cos` — deferred until shallow-water momentum needs
///   them (M1.5).
// Bit-shift sizes derived from i64 leading-zero positions are
// bounded above by 63, well within u32; the cast lints would
// demand try_from() noise that obscures the math.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::many_single_char_names
)]
pub mod transcendental {
    use super::Real;
    use fixed::types::I32F32;

    /// `ln(2)` in Q32.32: `round(0.6931471805599453 * 2^32)`.
    const LN_2_BITS: i64 = 2_977_044_472;

    /// `1 / ln(2)` in Q32.32: `round(1.4426950408889634 * 2^32)`.
    const INV_LN_2_BITS: i64 = 6_196_328_019;

    /// Q101: `π` in Q32.32. `round(3.141592653589793 * 2^32)`.
    const PI_BITS: i64 = 13_493_037_705;

    /// Q101: `2π` in Q32.32.
    const TWO_PI_BITS: i64 = 26_986_075_409;

    /// Q101: `π/2` in Q32.32.
    const HALF_PI_BITS: i64 = 6_746_518_852;

    fn ln_2() -> Real {
        Real::from_raw(I32F32::from_bits(LN_2_BITS))
    }

    fn inv_ln_2() -> Real {
        Real::from_raw(I32F32::from_bits(INV_LN_2_BITS))
    }

    /// Q101: π in Q32.32. Public so callers can compute angle
    /// arithmetic without re-computing the constant each call.
    #[must_use]
    pub fn pi() -> Real {
        Real::from_raw(I32F32::from_bits(PI_BITS))
    }

    /// Q101: 2π in Q32.32. Used by `sin`/`cos` for argument
    /// reduction and by callers doing full-rotation phases.
    #[must_use]
    pub fn two_pi() -> Real {
        Real::from_raw(I32F32::from_bits(TWO_PI_BITS))
    }

    /// Q101: π/2 in Q32.32. Used by `cos = sin(x + π/2)` and
    /// quarter-circle callers.
    #[must_use]
    pub fn half_pi() -> Real {
        Real::from_raw(I32F32::from_bits(HALF_PI_BITS))
    }

    /// Position of the highest set bit of a positive `i64`. Caller
    /// must ensure `bits > 0`.
    fn highest_set_bit(bits: i64) -> u32 {
        debug_assert!(bits > 0);
        63 - bits.leading_zeros()
    }

    /// Deterministic square root. Panics on negative input.
    #[must_use]
    pub fn sqrt(x: Real) -> Real {
        let bits = x.raw().to_bits();
        assert!(bits >= 0, "sqrt of negative number");
        if bits == 0 {
            return Real::ZERO;
        }
        // Initial guess: 2^((h + 32) / 2) where h is the highest
        // set bit of the raw bits. Q32.32 means raw = x * 2^32,
        // so x ≈ 2^(h-32) and sqrt(x) raw ≈ 2^((h+32)/2).
        let h = highest_set_bit(bits);
        let guess_shift = u32::midpoint(h, 32);
        let y0_bits = 1i64 << guess_shift;
        let mut y = Real::from_raw(I32F32::from_bits(y0_bits));
        let two = Real::from_int(2);
        let mut prev = y;
        // Quadratic convergence; ~6 iterations suffice for 32-bit
        // fractions, but allow oscillation between two LSB-adjacent
        // values near the fixed point.
        for _ in 0..32 {
            let next = (y + x / y) / two;
            if next == y || next == prev {
                return next;
            }
            prev = y;
            y = next;
        }
        y
    }

    /// Natural logarithm. Panics on non-positive input.
    #[must_use]
    pub fn ln(x: Real) -> Real {
        let bits = x.raw().to_bits();
        assert!(bits > 0, "ln of non-positive number");
        // x = m * 2^k with m ∈ [1, 2). Underlying raw = bits;
        // align so the highest bit lands at position 32 (the
        // implicit binary point).
        let h = highest_set_bit(bits) as i32;
        let k = h - 32;
        let m_bits = if k >= 0 { bits >> k } else { bits << (-k) };
        let m = Real::from_raw(I32F32::from_bits(m_bits));
        // ln(m) = 2 * Σ v^(2n+1) / (2n+1) with v = (m-1)/(m+1).
        // For m ∈ [1, 2), v ∈ [0, 1/3); v^21/21 ≲ 5e-12 ≪ Q32.32 LSB.
        let one = Real::ONE;
        let v = (m - one) / (m + one);
        let v2 = v * v;
        let mut term = v;
        let mut sum = v;
        for n in 1..=15 {
            term = term * v2;
            let denom = Real::from_int(2 * n + 1);
            sum = sum + term / denom;
        }
        let ln_m = sum + sum;
        ln_m + Real::from_int(i64::from(k)) * ln_2()
    }

    /// Exponential. Underflows to `ZERO` for very negative inputs;
    /// panics if the result would overflow `Real`'s range.
    #[must_use]
    pub fn exp(x: Real) -> Real {
        // x = k * ln2 + r with |r| ≤ ln2/2 ≈ 0.347. exp(x) =
        // 2^k * exp(r); the post-Taylor shift handles 2^k.
        let k_fixed = (x * inv_ln_2()).raw().round_ties_even();
        let k: i64 = k_fixed.to_num();
        let r = x - Real::from_int(k) * ln_2();
        // Taylor: |r|^15 / 15! ≲ 5e-20 ≪ Q32.32 LSB.
        let mut term = Real::ONE;
        let mut sum = Real::ONE;
        for n in 1..=15 {
            term = term * r / Real::from_int(n);
            sum = sum + term;
        }
        let sum_bits = sum.raw().to_bits();
        // sum ∈ (≈0.707, ≈1.414) so sum_bits has its top bit
        // around position 32. Shifting by k > 30 would overflow
        // i64; shifting by k < -63 would zero out (saturate to 0).
        assert!(k <= 30, "exp overflow: argument too large");
        if k < -63 {
            return Real::ZERO;
        }
        let result_bits = if k >= 0 {
            sum_bits.checked_shl(k as u32).expect("exp overflow")
        } else {
            sum_bits >> ((-k) as u32)
        };
        Real::from_raw(I32F32::from_bits(result_bits))
    }

    /// `a` raised to `b`, computed as `exp(b · ln(a))`. Panics if
    /// `a < 0` (no support for fractional powers of negatives).
    /// `pow(0, 0) = 1` by convention; `pow(0, b) = 0` for `b > 0`
    /// and panics for `b < 0`.
    #[must_use]
    pub fn pow(a: Real, b: Real) -> Real {
        if b == Real::ZERO {
            return Real::ONE;
        }
        if a == Real::ZERO {
            assert!(b > Real::ZERO, "pow(0, b) with b < 0 is undefined");
            return Real::ZERO;
        }
        exp(b * ln(a))
    }

    /// Deterministic sine in Q32.32 via Taylor series
    /// after argument reduction to `[-π, π]` and identity
    /// reflection to `[-π/2, π/2]`. Earlier callers approximated
    /// `sin`/`cos` shapes with triangular profiles because no
    /// trig was available; the real implementation lets every
    /// place that wants a real cosine bulge use one.
    ///
    /// Accuracy: 17-term Taylor in `[-π/2, π/2]` is bounded by
    /// `(π/2)^17 / 17! ≈ 5e-13` — well below Q32.32's LSB
    /// (~2.3e-10) at the worst-case input.
    #[must_use]
    pub fn sin(x: Real) -> Real {
        // Step 1: reduce to [-π, π] via x - 2π · round(x / 2π).
        let two_pi_v = two_pi();
        let pi_v = pi();
        let k = (x / two_pi_v).raw().round_ties_even();
        let mut r = x - Real::from_raw(k) * two_pi_v;
        // After the rounding, |r| ≤ π. Reflect to [-π/2, π/2]
        // using sin(π - r) = sin(r) and sin(-π - r) = -sin(-r).
        let half_pi_v = half_pi();
        if r > half_pi_v {
            r = pi_v - r;
        } else if r < -half_pi_v {
            r = -pi_v - r;
        }
        // Taylor series: sin(r) = r - r^3/3! + r^5/5! - ...
        // 17 terms for sub-LSB accuracy on |r| ≤ π/2 ≈ 1.5708.
        let mut term = r;
        let mut sum = r;
        let r_sq = r * r;
        for n in 1..=8 {
            // Each iteration adds the next pair: divide term by
            // (2n)(2n+1), flip sign.
            let denom = Real::from_int(i64::from(2 * n) * i64::from(2 * n + 1));
            term = -term * r_sq / denom;
            sum = sum + term;
        }
        sum
    }

    /// Cosine via the identity `cos(x) = sin(x + π/2)`.
    #[must_use]
    pub fn cos(x: Real) -> Real {
        sin(x + half_pi())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_is_deterministic() {
        let a = Real::from_int(3);
        let b = Real::from_int(4);
        assert_eq!(a + b, Real::from_int(7));
    }

    #[test]
    fn ratio_construction() {
        let half = Real::from_ratio(1, 2);
        assert_eq!(half + half, Real::ONE);
    }

    #[test]
    fn associativity_holds_for_integers() {
        let a = Real::from_int(2);
        let b = Real::from_int(3);
        let c = Real::from_int(4);
        assert_eq!((a + b) + c, a + (b + c));
    }

    #[test]
    fn raw_roundtrip() {
        let x = Real::from_ratio(7, 3);
        assert_eq!(Real::from_raw(x.raw()), x);
    }
}

#[cfg(test)]
mod transcendental_tests {
    use super::transcendental::{cos, exp, ln, pi, pow, sin, sqrt, two_pi};
    use super::Real;
    use fixed::types::I32F32;

    /// Q32.32 LSB ≈ 2.33e-10. Loose tolerance for transcendentals
    /// gives Taylor-truncation error room without masking bugs.
    const EPSILON_F64: f64 = 1e-7;

    /// Convert an `f64` to `Real` without going through
    /// `from_ratio` (which overflows on large numerators). Tests
    /// only — production code uses integer-only constructors.
    fn rf(v: f64) -> Real {
        Real::from_raw(I32F32::from_num(v))
    }

    fn close(actual: Real, expected: f64) -> bool {
        let a = actual.to_f64_for_display();
        let tol = EPSILON_F64.max(expected.abs() * EPSILON_F64);
        (a - expected).abs() < tol
    }

    #[test]
    fn sqrt_of_zero_is_zero() {
        assert_eq!(sqrt(Real::ZERO), Real::ZERO);
    }

    #[test]
    fn sqrt_of_one_is_one() {
        assert_eq!(sqrt(Real::ONE), Real::ONE);
    }

    #[test]
    fn sqrt_of_four_is_two() {
        assert_eq!(sqrt(Real::from_int(4)), Real::from_int(2));
    }

    #[test]
    fn sqrt_matches_f64_across_range() {
        for &v in &[0.25_f64, 0.5, 1.5, 2.0, 9.0, 100.0, 9_810.0, 1e6, 1e8] {
            let r = sqrt(rf(v));
            assert!(
                close(r, v.sqrt()),
                "sqrt({}) = {} but f64 says {}",
                v,
                r.to_f64_for_display(),
                v.sqrt()
            );
        }
    }

    #[test]
    fn sqrt_is_deterministic() {
        let x = Real::from_ratio(73, 13);
        assert_eq!(sqrt(x).raw(), sqrt(x).raw());
    }

    #[test]
    fn ln_of_one_is_zero() {
        assert_eq!(ln(Real::ONE), Real::ZERO);
    }

    #[test]
    fn ln_of_e_is_one() {
        // e ≈ 2.718281828, fed through exp(1).
        let e = exp(Real::ONE);
        assert!(close(ln(e), 1.0));
    }

    #[test]
    fn ln_matches_f64_across_range() {
        for &v in &[0.5_f64, 1.0, 2.0, 10.0, 100.0, 1e6, 1e9] {
            let r = ln(rf(v));
            assert!(
                close(r, v.ln()),
                "ln({}) = {} but f64 says {}",
                v,
                r.to_f64_for_display(),
                v.ln()
            );
        }
    }

    #[test]
    fn ln_is_deterministic() {
        let x = Real::from_ratio(101, 7);
        assert_eq!(ln(x).raw(), ln(x).raw());
    }

    #[test]
    fn exp_of_zero_is_one() {
        assert_eq!(exp(Real::ZERO), Real::ONE);
    }

    #[test]
    fn exp_matches_f64_across_range() {
        for &v in &[-10.0_f64, -1.0, -0.5, 0.5, 1.0, 5.0, 10.0, 15.0] {
            let r = exp(rf(v));
            assert!(
                close(r, v.exp()),
                "exp({}) = {} but f64 says {}",
                v,
                r.to_f64_for_display(),
                v.exp()
            );
        }
    }

    #[test]
    fn exp_ln_roundtrip() {
        for &v in &[0.5_f64, 1.0, 2.5, 7.0, 100.0] {
            let x = rf(v);
            let round_tripped = exp(ln(x));
            assert!(
                close(round_tripped, v),
                "exp(ln({})) = {}",
                v,
                round_tripped.to_f64_for_display()
            );
        }
    }

    #[test]
    fn exp_is_deterministic() {
        let x = Real::from_ratio(13, 7);
        assert_eq!(exp(x).raw(), exp(x).raw());
    }

    #[test]
    fn pow_zero_zero_is_one() {
        assert_eq!(pow(Real::ZERO, Real::ZERO), Real::ONE);
    }

    #[test]
    fn pow_a_zero_is_one() {
        assert_eq!(pow(Real::from_int(7), Real::ZERO), Real::ONE);
    }

    #[test]
    fn pow_zero_b_is_zero() {
        assert_eq!(pow(Real::ZERO, Real::from_int(3)), Real::ZERO);
    }

    #[test]
    fn pow_matches_f64_across_range() {
        let cases: &[(f64, f64)] = &[
            (2.0, 10.0),
            (10.0, 3.0),
            (1.5, 4.0),
            (3.0, 0.5),
            (100.0, 0.25),
        ];
        for &(a, b) in cases {
            let r = pow(rf(a), rf(b));
            let expected = a.powf(b);
            assert!(
                close(r, expected),
                "pow({}, {}) = {} but f64 says {}",
                a,
                b,
                r.to_f64_for_display(),
                expected
            );
        }
    }

    #[test]
    fn pow_is_deterministic() {
        let a = Real::from_ratio(31, 7);
        let b = Real::from_ratio(11, 5);
        assert_eq!(pow(a, b).raw(), pow(a, b).raw());
    }

    #[test]
    fn clausius_clapeyron_smoke() {
        // ln(P2/P1) = -L/R * (1/T2 - 1/T1)
        // For water at sea level: P1 = 101325 Pa, T1 = 373.15 K,
        // L = 2.257e6 J/kg → ln(P2/101325) at T2 = 363.15 should be
        // negative; exp() of it should give a P2 < P1.
        let l_over_r = Real::from_ratio(2_257_000, 461); // L_vap / R_specific (water)
        let t1 = Real::from_ratio(373_150, 1000);
        let t2 = Real::from_ratio(363_150, 1000);
        let inv_t2 = Real::ONE / t2;
        let inv_t1 = Real::ONE / t1;
        let exponent = -l_over_r * (inv_t2 - inv_t1);
        let ratio = exp(exponent);
        let p1 = Real::from_int(101_325);
        let p2 = p1 * ratio;
        let p2_f = p2.to_f64_for_display();
        // Real-world P2 at 90°C ≈ 70.1 kPa.
        assert!(
            (p2_f - 70_100.0).abs() < 5_000.0,
            "Clausius-Clapeyron P2 = {p2_f} Pa, expected ≈ 70 100 Pa",
        );
    }

    /// Trig accuracy at the cardinal angles. Q32.32 LSB
    /// is ~2.3e-10; Taylor truncation at 17 terms is ~5e-13;
    /// argument-reduction roundoff dominates the 1e-7 budget.
    #[test]
    fn sin_at_cardinal_angles() {
        let pi_v = pi();
        let two_pi_v = two_pi();
        let half = Real::from_ratio(1, 2);
        // sin(0) = 0
        assert!(sin(Real::ZERO).to_f64_for_display().abs() < EPSILON_F64);
        // sin(π/2) = 1
        let s_half_pi = sin(pi_v * half);
        assert!(
            (s_half_pi.to_f64_for_display() - 1.0).abs() < EPSILON_F64,
            "sin(π/2) = {s_half_pi:?}"
        );
        // sin(π) = 0
        assert!(sin(pi_v).to_f64_for_display().abs() < EPSILON_F64);
        // sin(3π/2) = -1
        let three_half = Real::from_ratio(3, 2);
        assert!((sin(pi_v * three_half).to_f64_for_display() + 1.0).abs() < EPSILON_F64);
        // sin(2π) = 0
        assert!(sin(two_pi_v).to_f64_for_display().abs() < EPSILON_F64);
    }

    #[test]
    fn cos_at_cardinal_angles() {
        let pi_v = pi();
        let half = Real::from_ratio(1, 2);
        // cos(0) = 1
        assert!((cos(Real::ZERO).to_f64_for_display() - 1.0).abs() < EPSILON_F64);
        // cos(π/2) = 0
        assert!(cos(pi_v * half).to_f64_for_display().abs() < EPSILON_F64);
        // cos(π) = -1
        assert!((cos(pi_v).to_f64_for_display() + 1.0).abs() < EPSILON_F64);
    }

    #[test]
    fn sin_squared_plus_cos_squared_is_one() {
        // Pythagorean identity at a few intermediate angles.
        let test_angles = [0.0, 0.1, 0.5, 1.0, 1.5, 2.0, 3.0, 5.0, -1.5];
        for theta in test_angles {
            let t = rf(theta);
            let s = sin(t).to_f64_for_display();
            let c = cos(t).to_f64_for_display();
            let identity = s * s + c * c;
            assert!(
                (identity - 1.0).abs() < EPSILON_F64,
                "sin²+cos² at θ={theta}: {identity}"
            );
        }
    }

    #[test]
    fn sin_is_deterministic() {
        let t = Real::from_ratio(7, 5);
        let a = sin(t);
        let b = sin(t);
        assert_eq!(a.raw().to_bits(), b.raw().to_bits());
    }
}
