//! RMSE fit module. Fit a `Form` against accumulated
//! `(x, y)` samples; report residual, exponential-decay confidence,
//! and the per-form intelligence-scaled tolerance.
//!
//! Spec:
//!
//! - **Metric:** `residual = sqrt(Σ(y_i − f(x_i))² / n)` (RMSE).
//! - **Tolerance:** `tolerance = base_per_form × (1 / intelligence)
//!   / sqrt(n)`. Smarter species demand tighter fits; more data
//!   shrinks the tolerance.
//! - **Confidence:** `confidence = exp(−residual / tolerance)`.
//! - **Minimum samples:** `n_min = ceil(k_form / intelligence)` with
//!   per-form floor `k_form`.
//!
//! M3 implements closed-form fits where the parameter dependence is
//! linear (`Constant`, `Linear`, `Logarithmic`, `InverseSquare`;
//! `ExpDecay` / `ExpGrowth` / `PowerLaw` via log-linearisation), plus
//! a search-based fit for `ThresholdStep`. `Polynomial2`/`3`,
//! `PeriodicSine`, and `Logistic` return `None` for now; the
//! iterative-fit pass is a tunable, staged follow-up that
//! lands once empirical M3 data shows the pipeline benefits from the
//! extra forms.

// Matrix code uses indexed loops by nature; range loops are the
// natural form for Gauss-Jordan elimination.
#![allow(clippy::needless_range_loop)]

use crate::forms::Form;
use sim_arith::transcendental::{exp, ln, sqrt};
use sim_arith::Real;

/// One observation point fit against. `x` is the independent
/// variable (usually a recognition channel reading); `y` is the
/// dependent quantity (firing count, accumulated mass, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    pub x: Real,
    pub y: Real,
}

/// Result of a single `(form, samples, intelligence)` fit attempt.
/// All outputs in one shot so callers don't recompute.
#[derive(Debug, Clone)]
pub struct FitResult {
    pub form: Form,
    pub params: Vec<Real>,
    pub residual: Real,
    pub tolerance: Real,
    pub confidence: Real,
    pub n_samples: usize,
}

impl FitResult {
    /// confirmation threshold: `residual ≤ tolerance` ⇔
    /// `confidence ≥ exp(−1)`. anchors refinement-trigger and
    /// reject thresholds against the same scale.
    pub fn is_confirmed(&self) -> bool {
        self.confidence >= exp_neg_one()
    }
}

/// minimum-sample requirement scaled by intelligence:
/// `n_min = ceil(k_form / intelligence)`. Falls back to `k_form`
/// when `intelligence ≤ 0` so the formula is well-defined for
/// degenerate species; that floor is also the reference point.
pub fn min_samples(form: Form, intelligence: Real) -> usize {
    let k = form.min_samples_floor();
    if intelligence <= Real::ZERO {
        return k;
    }
    let scaled = Real::from_int(i64::try_from(k).unwrap_or(i64::MAX)) / intelligence;
    // ceil() via raw shift on the underlying Q-format. Use saturating
    // i64 conversion so a tiny intelligence doesn't blow the cast.
    let bits = scaled.raw().to_bits();
    let frac_mask = (1_i64 << 32) - 1;
    let int_part = bits >> 32;
    let frac = bits & frac_mask;
    let ceiled = if frac > 0 { int_part + 1 } else { int_part };
    usize::try_from(ceiled.max(1)).unwrap_or(1)
}

/// Top-level entry. Returns `None` when the form cannot be fit (too
/// few samples, log-fit precondition violated, or the form is one of
/// the M3-stubbed iterative templates).
pub fn fit(form: Form, samples: &[Sample], intelligence: Real) -> Option<FitResult> {
    let n = samples.len();
    if n < min_samples(form, intelligence) {
        return None;
    }
    let params = fit_params(form, samples)?;
    let residual = rmse(form, &params, samples);
    let tolerance = compute_tolerance(form, intelligence, n);
    if tolerance <= Real::ZERO {
        return None;
    }
    let confidence = exp(-(residual / tolerance));
    Some(FitResult {
        form,
        params,
        residual,
        tolerance,
        confidence,
        n_samples: n,
    })
}

fn fit_params(form: Form, samples: &[Sample]) -> Option<Vec<Real>> {
    match form {
        Form::Constant => Some(vec![mean_y(samples)]),
        Form::Linear => fit_linear_in_basis(samples, 2, |x| [x, Real::ONE].to_vec()),
        Form::Logarithmic => fit_logarithmic(samples),
        Form::InverseSquare => fit_inverse_square(samples),
        Form::ExpDecay => fit_exp(samples, false),
        Form::ExpGrowth => fit_exp(samples, true),
        Form::PowerLaw => fit_power_law(samples),
        Form::ThresholdStep => fit_threshold_step(samples),
        // Polynomial2 + Polynomial3 are linear-in-params, so
        // the existing Gauss-Jordan least-squares machinery handles
        // them via a basis expansion. Param order matches `evaluate`:
        //  Polynomial2: y = p[0]·x² + p[1]·x + p[2]
        //  Polynomial3: y = p[0]·x³ + p[1]·x² + p[2]·x + p[3]
        // so the basis lists the highest-power term first.
        Form::Polynomial2 => fit_linear_in_basis(samples, 3, |x| {
            let x2 = x * x;
            vec![x2, x, Real::ONE]
        }),
        Form::Polynomial3 => fit_linear_in_basis(samples, 4, |x| {
            let x2 = x * x;
            let x3 = x2 * x;
            vec![x3, x2, x, Real::ONE]
        }),
        // PeriodicSine + Logistic are nonlinear in their parameters
        // and would need iterative search (Levenberg-Marquardt or
        // similar) under Q32.32 fixed-point. Genuinely tractable
        // but a separate workstream — keep the existing stub so the
        // hypothesizer doesn't try them and produce spurious None
        // returns at higher cost.
        Form::PeriodicSine | Form::Logistic => None,
    }
}

fn mean_y(samples: &[Sample]) -> Real {
    let n = Real::from_int(i64::try_from(samples.len()).unwrap_or(i64::MAX));
    let sum: Real = samples.iter().fold(Real::ZERO, |a, s| a + s.y);
    sum / n
}

/// Generic least-squares solve for `y = β · ϕ(x)` with arbitrary
/// `param_count`-dim basis. Works in Q32.32 when the basis values
/// stay well within range (linear, logarithmic, power-law-via-log).
fn fit_linear_in_basis<F>(samples: &[Sample], param_count: usize, basis: F) -> Option<Vec<Real>>
where
    F: Fn(Real) -> Vec<Real>,
{
    let n = samples.len();
    if n < param_count {
        return None;
    }

    // Normal equations: A = Σ ϕ(x) ϕ(x)ᵀ, c = Σ ϕ(x) · y.
    //
    // P0.6: saturating arithmetic on every product and running
    // sum. For polynomial bases (`Polynomial3` has `[x³, x², x, 1]`)
    // the cross-term `x³ × x³ = x⁶` already overflows Q32.32 at
    // x ≈ 100. With wider sample distributions post-Items-12-24 the
    // hypothesizer can sample physics deltas spanning four orders
    // of magnitude in one observation window. Saturating clamps
    // produce a degenerate (near-singular) normal-equations matrix
    // that `solve_linear_system` rejects as `None`; the caller
    // then treats the fit as failed and moves on to the next form.
    // The pre-fix behaviour was a hard panic that aborted the run.
    let mut a = vec![vec![Real::ZERO; param_count]; param_count];
    let mut c = vec![Real::ZERO; param_count];
    for s in samples {
        let phi = basis(s.x);
        debug_assert_eq!(phi.len(), param_count);
        for i in 0..param_count {
            for j in 0..param_count {
                let prod = phi[i].saturating_mul(phi[j]);
                a[i][j] = a[i][j].saturating_add(prod);
            }
            let prod = phi[i].saturating_mul(s.y);
            c[i] = c[i].saturating_add(prod);
        }
    }
    solve_linear_system(a, c)
}

/// Gauss-Jordan elimination for an `n × n` system. Returns `None`
/// if the matrix is singular *or near-singular* (pivot magnitude
/// drops below `1e-6` × the largest matrix element seen at this
/// step). Deterministic under Q32.32 since all ops are exact
/// fixed-point.
///
/// The near-singular check matters under Q32.32 because saturating
/// division of a finite RHS by a tiny pivot produces an enormous
/// quotient that then overflows the next `factor * a[i][j]`
/// multiply. Returning `None` early makes the fit fail cleanly
/// (caller treats as "no fit found") instead of panicking. Pre-
/// the surrounding code rarely hit this regime — temperatures
/// homogenised to exactly equal x values, giving a strict-zero
/// pivot that the existing check already rejected. 's radiative
/// balance maintains row-banded variance, so near-singular (but
/// not exactly singular) matrices became reachable.
fn solve_linear_system(mut a: Vec<Vec<Real>>, mut c: Vec<Real>) -> Option<Vec<Real>> {
    let n = c.len();
    debug_assert_eq!(a.len(), n);
    // Relative pivot threshold: below this fraction of the matrix's
    // largest absolute element, the system is too ill-conditioned
    // to solve safely in Q32.32.
    let cond_threshold = Real::from_ratio(1, 1_000_000);
    for i in 0..n {
        // Partial pivoting — find the row with the largest |a[k][i]|
        // for k >= i and swap into row i.
        let mut pivot_row = i;
        let mut pivot_mag = a[i][i].abs();
        for k in (i + 1)..n {
            let mag = a[k][i].abs();
            if mag > pivot_mag {
                pivot_row = k;
                pivot_mag = mag;
            }
        }
        if pivot_mag == Real::ZERO {
            return None;
        }
        // Reject ill-conditioning: compare the chosen pivot against
        // the largest absolute element anywhere in the remaining
        // sub-matrix. A pivot many orders of magnitude smaller means
        // the elimination's `factor * row` step would saturate /
        // overflow.
        let mut max_abs = Real::ZERO;
        for row in i..n {
            for col in i..n {
                let m = a[row][col].abs();
                if m > max_abs {
                    max_abs = m;
                }
            }
        }
        if pivot_mag < max_abs * cond_threshold {
            return None;
        }
        if pivot_row != i {
            a.swap(i, pivot_row);
            c.swap(i, pivot_row);
        }
        let pivot = a[i][i];
        for j in 0..n {
            a[i][j] = a[i][j] / pivot;
        }
        c[i] = c[i] / pivot;
        for k in 0..n {
            if k == i {
                continue;
            }
            let factor = a[k][i];
            if factor == Real::ZERO {
                continue;
            }
            for j in 0..n {
                // P0.6: saturating arithmetic on the elimination
                // step. The pivot-magnitude conditioning check above
                // (`pivot_mag < max_abs * cond_threshold`) usually
                // rejects matrices where this product would
                // overflow, but with the saturating sums in
                // `fit_linear_in_basis` above feeding this routine,
                // a row whose entries already saturated at `Real::MAX`
                // can produce a `factor * a[i][j]` term that would
                // panic the subtract. Saturating the chain keeps the
                // solver returning a clamped solution rather than
                // aborting the whole run loop.
                let prod = factor.saturating_mul(a[i][j]);
                a[k][j] = a[k][j].saturating_sub(prod);
            }
            let prod = factor.saturating_mul(c[i]);
            c[k] = c[k].saturating_sub(prod);
        }
    }
    Some(c)
}

fn fit_logarithmic(samples: &[Sample]) -> Option<Vec<Real>> {
    if samples.iter().any(|s| s.x <= Real::ZERO) {
        return None;
    }
    fit_linear_in_basis(samples, 2, |x| vec![ln(x), Real::ONE])
}

fn fit_inverse_square(samples: &[Sample]) -> Option<Vec<Real>> {
    // Guard against `s.x` near zero: u = 1/x² blows up and the
    // subsequent `u * u` term overflows the Q32.32 range. The
    // recognition vocabulary expansion (flood_zone / cold_zone /
    // fertile_land) lets cells with very small normalised x land
    // in the sample set — without this guard those samples
    // immediately panic in `Real::Mul`. Skip the fit when any
    // sample is too close to the singularity at x=0; a real
    // inverse-square law has no meaningful fit there anyway.
    //
    // Tightened from 1/100 → 1/10: at |x| = 0.01 a single u² is
    // 1e8 and the running `den` cumulative sum overflows Q32.32
    // (~2.14e9) after ~25 samples. At |x| ≥ 0.1 the per-sample u²
    // ≤ 1e4 so even 1000+ samples stay well within range.
    let min_safe_x = Real::from_ratio(1, 10);
    if samples.iter().any(|s| s.x.abs() < min_safe_x) {
        return None;
    }
    // y = a · u with u = 1/x². Linear-through-origin: a = Σ(uy)/Σ(u²).
    let mut num = Real::ZERO;
    let mut den = Real::ZERO;
    for s in samples {
        let u = Real::ONE / (s.x * s.x);
        num = num + u * s.y;
        den = den + u * u;
    }
    if den == Real::ZERO {
        return None;
    }
    Some(vec![num / den])
}

fn fit_exp(samples: &[Sample], growth: bool) -> Option<Vec<Real>> {
    if samples.iter().any(|s| s.y <= Real::ZERO) {
        return None;
    }
    // ln(y) = ln(a) + sign·b·x, with sign = +1 (growth) or −1 (decay).
    // Fit by mapping samples to (x, ln y) and running linear LS;
    // recover a = exp(intercept) and b accordingly.
    let mapped: Vec<Sample> = samples
        .iter()
        .map(|s| Sample { x: s.x, y: ln(s.y) })
        .collect();
    let lin = fit_linear_in_basis(&mapped, 2, |x| vec![x, Real::ONE])?;
    let slope = lin[0];
    let intercept = lin[1];
    // P0.6: `exp` panics when its argument exceeds ln(Real::MAX) ≈ 21.5.
    // A wild least-squares regression on widely-spread y values can yield
    // an intercept hundreds of orders of magnitude away from origin
    // (e.g. heated cells whose `ln(y)` walks a steep ramp far from x=0).
    // Reject the fit cleanly rather than panic the run loop — the caller
    // already treats `None` as "no fit found" and falls back to other
    // forms. The guard matches the `k <= 30` ceiling in
    // `sim_arith::transcendental::exp`.
    if intercept.abs() >= Real::from_int(20) {
        return None;
    }
    let a = exp(intercept);
    let b = if growth { slope } else { -slope };
    Some(vec![a, b])
}

fn fit_power_law(samples: &[Sample]) -> Option<Vec<Real>> {
    if samples
        .iter()
        .any(|s| s.x <= Real::ZERO || s.y <= Real::ZERO)
    {
        return None;
    }
    // ln(y) = ln(a) + b · ln(x). Linear in (ln x, 1).
    let mapped: Vec<Sample> = samples
        .iter()
        .map(|s| Sample {
            x: ln(s.x),
            y: ln(s.y),
        })
        .collect();
    let lin = fit_linear_in_basis(&mapped, 2, |x| vec![x, Real::ONE])?;
    let b = lin[0];
    // P0.6: same guard as `fit_exp`. `exp(intercept)` panics when
    // the linear-regression intercept walks past ln(Real::MAX).
    if lin[1].abs() >= Real::from_int(20) {
        return None;
    }
    let a = exp(lin[1]);
    Some(vec![a, b])
}

fn fit_threshold_step(samples: &[Sample]) -> Option<Vec<Real>> {
    // Search the threshold over the unique sample x-values. For each
    // candidate t, the optimum a is mean(y where x<t), b is
    // mean(y where x≥t). Pick the candidate with lowest RMSE.
    if samples.len() < 2 {
        return None;
    }
    // Sort indices by x, deterministically.
    let mut idx: Vec<usize> = (0..samples.len()).collect();
    idx.sort_by(|&i, &j| samples[i].x.cmp(&samples[j].x));

    let mut best: Option<(Real, [Real; 3])> = None;
    let n = samples.len();
    for split in 1..n {
        let t = samples[idx[split]].x;
        if samples[idx[split - 1]].x == t {
            continue; // skip identical-x boundary
        }
        let mut sum_lo = Real::ZERO;
        let mut sum_hi = Real::ZERO;
        let mut count_lo = 0_i64;
        let mut count_hi = 0_i64;
        for &i in &idx {
            if samples[i].x < t {
                sum_lo = sum_lo + samples[i].y;
                count_lo += 1;
            } else {
                sum_hi = sum_hi + samples[i].y;
                count_hi += 1;
            }
        }
        if count_lo == 0 || count_hi == 0 {
            continue;
        }
        let a = sum_lo / Real::from_int(count_lo);
        let b = sum_hi / Real::from_int(count_hi);
        let params = [a, b, t];
        let res = rmse(Form::ThresholdStep, &params, samples);
        if best.as_ref().is_none_or(|(prev_res, _)| res < *prev_res) {
            best = Some((res, params));
        }
    }
    best.map(|(_, p)| p.to_vec())
}

pub fn rmse(form: Form, params: &[Real], samples: &[Sample]) -> Real {
    let n = samples.len();
    if n == 0 {
        return Real::ZERO;
    }
    // Q32.32 squaring overflow guard: `Real * Real` panics when
    // the product can't fit in I32F32 (max ≈ 2^31). The safe-
    // square ceiling is |x| < sqrt(2^31) ≈ 46340; anything above
    // that produces a panic. To leave headroom for `sum_sq`
    // accumulation across n samples, clamp each diff to ~10000 —
    // squared = 1e8, summed across hundreds of cells stays well
    // under the 2^31 ceiling. A wildly bad fit then returns a
    // finite (large) RMSE; `fit()` computes a near-zero
    // confidence and rejects the form, which is the desired
    // behaviour.
    let safe_max = Real::from_int(10_000);
    // I32F32 ceiling guard. Each clamped `diff²` is ≤ 1e8, but a
    // wildly bad fit over *many* samples can still drive `sum_sq` past
    // the ~2.1e9 I32F32 maximum (the per-diff clamp alone is not
    // enough once the sample count is large — e.g. a big habitable
    // ocean). Stop and return a saturating-large RMSE, which `fit`
    // turns into ~zero confidence so the form is rejected — the same
    // outcome a huge finite RMSE would give, minus the panic. Normal
    // fits never approach the ceiling, so their RMSE is bit-identical.
    let sum_sq_ceiling = Real::from_int(2_000_000_000);
    let mut sum_sq = Real::ZERO;
    for s in samples {
        let pred = form.evaluate(params, s.x);
        let raw_diff = s.y - pred;
        let diff = raw_diff.max(-safe_max).min(safe_max);
        let term = diff * diff;
        if sum_sq > sum_sq_ceiling - term {
            return safe_max;
        }
        sum_sq = sum_sq + term;
    }
    let mean_sq = sum_sq / Real::from_int(i64::try_from(n).unwrap_or(i64::MAX));
    // Q32.32 add/mul with the per-iter clamp keeps `sum_sq`
    // non-negative in normal flow, but a `Real::from_ratio` call
    // upstream can produce a tiny negative residual after
    // sub-cell rounding on certain seeds. Clamp the sqrt input
    // at zero so a value like `-1e-9` doesn't trip the
    // "sqrt of negative" assertion.
    if mean_sq < Real::ZERO {
        Real::ZERO
    } else {
        sqrt(mean_sq)
    }
}

/// tolerance:
/// `tolerance = base_per_form × (1 / intelligence) / sqrt(n)`.
/// Tiny `intelligence` is clamped to a small floor so the tolerance
/// stays positive and finite.
pub fn compute_tolerance(form: Form, intelligence: Real, n: usize) -> Real {
    let base = form.base_tolerance();
    let intel = intelligence.max(Real::percent(1));
    let denom = sqrt(Real::from_int(i64::try_from(n).unwrap_or(i64::MAX)));
    if denom == Real::ZERO {
        return base;
    }
    (base / intel) / denom
}

fn exp_neg_one() -> Real {
    // exp(-1) computed once via the deterministic transcendental.
    exp(-Real::ONE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(x: i64, y: i64) -> Sample {
        Sample {
            x: Real::from_int(x),
            y: Real::from_int(y),
        }
    }

    fn close(a: Real, b: Real, tol: Real) -> bool {
        (a - b).abs() <= tol
    }

    fn small_tol() -> Real {
        Real::percent(1)
    }

    #[test]
    fn rmse_zero_for_perfect_fit() {
        let samples = vec![sample(1, 5), sample(2, 5), sample(3, 5)];
        let r = rmse(Form::Constant, &[Real::from_int(5)], &samples);
        assert_eq!(r, Real::ZERO);
    }

    #[test]
    fn fit_constant_recovers_mean() {
        let samples = vec![sample(0, 4), sample(1, 6), sample(2, 5)];
        let res = fit(Form::Constant, &samples, Real::ONE).unwrap();
        assert!(close(res.params[0], Real::from_int(5), small_tol()));
        assert_eq!(res.residual.abs(), res.residual); // non-negative
    }

    #[test]
    fn fit_linear_recovers_exact_line() {
        // y = 2x + 3 over 6 points
        let samples: Vec<Sample> = (0..6).map(|i| sample(i, 2 * i + 3)).collect();
        let res = fit(Form::Linear, &samples, Real::ONE).unwrap();
        assert!(
            close(res.params[0], Real::from_int(2), small_tol()),
            "slope {:?}",
            res.params[0]
        );
        assert!(
            close(res.params[1], Real::from_int(3), small_tol()),
            "intercept {:?}",
            res.params[1]
        );
        // Perfect fit -> RMSE near zero.
        assert!(res.residual < small_tol());
        assert!(res.is_confirmed());
    }

    #[test]
    fn fit_linear_rejects_below_min_samples() {
        let samples = vec![sample(0, 0), sample(1, 1)];
        // intelligence=1 -> min_samples = ceil(4/1) = 4. Two points
        // is below the floor.
        assert!(fit(Form::Linear, &samples, Real::ONE).is_none());
    }

    #[test]
    fn smarter_species_needs_fewer_samples() {
        let samples: Vec<Sample> = (0..3).map(|i| sample(i, i)).collect();
        // intelligence=1 -> need 4; intelligence=2 -> ceil(4/2) = 2; intelligence=4 -> 1.
        assert!(fit(Form::Linear, &samples, Real::ONE).is_none());
        assert!(fit(Form::Linear, &samples, Real::from_int(2)).is_some());
        assert!(fit(Form::Linear, &samples, Real::from_int(4)).is_some());
    }

    #[test]
    fn fit_threshold_step_recovers_split() {
        // Two-piece constant: 1 below x=4, 10 at-or-above.
        let samples = vec![
            sample(0, 1),
            sample(1, 1),
            sample(2, 1),
            sample(3, 1),
            sample(4, 10),
            sample(5, 10),
            sample(6, 10),
            sample(7, 10),
        ];
        let res = fit(Form::ThresholdStep, &samples, Real::ONE).unwrap();
        // a (below threshold) ≈ 1, b (above) ≈ 10, t somewhere in (3, 4].
        assert!(close(res.params[0], Real::from_int(1), small_tol()));
        assert!(close(res.params[1], Real::from_int(10), small_tol()));
        assert!(res.params[2] > Real::from_int(3));
        assert!(res.params[2] <= Real::from_int(4));
        assert!(res.is_confirmed());
    }

    #[test]
    fn fit_logarithmic_recovers_coefficients() {
        // y = 2 * ln(x) + 1
        let samples: Vec<Sample> = (1..=8)
            .map(|i| Sample {
                x: Real::from_int(i),
                y: Real::from_int(2) * ln(Real::from_int(i)) + Real::ONE,
            })
            .collect();
        let res = fit(Form::Logarithmic, &samples, Real::ONE).unwrap();
        let tol = Real::percent(5);
        assert!(close(res.params[0], Real::from_int(2), tol));
        assert!(close(res.params[1], Real::ONE, tol));
    }

    #[test]
    fn fit_power_law_recovers_coefficients() {
        // y = 3 * x^2 over x=1..=6
        let samples: Vec<Sample> = (1..=6)
            .map(|i| Sample {
                x: Real::from_int(i),
                y: Real::from_int(3 * i * i),
            })
            .collect();
        let res = fit(Form::PowerLaw, &samples, Real::ONE).unwrap();
        let tol = Real::percent(15);
        assert!(
            close(res.params[1], Real::from_int(2), tol),
            "exponent {:?}",
            res.params[1]
        );
        assert!(
            close(res.params[0], Real::from_int(3), Real::percent(50)),
            "coefficient {:?}",
            res.params[0]
        );
    }

    #[test]
    fn fit_inverse_square_recovers_coefficient() {
        // y = 100 / x^2 over x=1..=8
        let samples: Vec<Sample> = (1..=8)
            .map(|i| Sample {
                x: Real::from_int(i),
                y: Real::from_int(100) / Real::from_int(i * i),
            })
            .collect();
        let res = fit(Form::InverseSquare, &samples, Real::ONE).unwrap();
        let tol = Real::ONE;
        assert!(
            close(res.params[0], Real::from_int(100), tol),
            "coeff {:?}",
            res.params[0]
        );
    }

    /// (was: stubbed → now implemented). Polynomial2 fit
    /// confirms on a `y = x²` dataset. Kept under its original
    /// name as a regression marker so the M3-era stub assumption
    /// can never silently come back.
    #[test]
    fn fit_polynomial2_now_succeeds() {
        let samples: Vec<Sample> = (0..8).map(|i| sample(i, i * i)).collect();
        let res = fit(Form::Polynomial2, &samples, Real::ONE).unwrap();
        // Coefficients: 1·x² + 0·x + 0
        assert!(close(res.params[0], Real::from_int(1), small_tol()));
        assert!(close(res.params[1], Real::ZERO, small_tol()));
        assert!(close(res.params[2], Real::ZERO, small_tol()));
        assert!(res.is_confirmed());
    }

    #[test]
    fn confidence_bounded_in_zero_one() {
        let samples: Vec<Sample> = (0..6).map(|i| sample(i, 2 * i + 3)).collect();
        let res = fit(Form::Linear, &samples, Real::ONE).unwrap();
        assert!(res.confidence > Real::ZERO);
        assert!(res.confidence <= Real::ONE);
    }

    #[test]
    fn confidence_threshold_matches_q35() {
        // anchors confirmation at residual ≤ tolerance, equivalently
        // confidence ≥ exp(-1) ≈ 0.368. Construct a fit with residual
        // exactly equal to tolerance.
        // confidence = exp(-1) -> is_confirmed() must be true.
        let res = FitResult {
            form: Form::Constant,
            params: vec![Real::ZERO],
            residual: Real::ONE,
            tolerance: Real::ONE,
            confidence: exp(-Real::ONE),
            n_samples: 4,
        };
        assert!(res.is_confirmed());
    }

    #[test]
    fn determinism_for_same_inputs() {
        let samples: Vec<Sample> = (0..6).map(|i| sample(i, 3 * i + 7)).collect();
        let a = fit(Form::Linear, &samples, Real::ONE).unwrap();
        let b = fit(Form::Linear, &samples, Real::ONE).unwrap();
        assert_eq!(a.params, b.params);
        assert_eq!(a.residual, b.residual);
        assert_eq!(a.confidence, b.confidence);
    }

    #[test]
    fn min_samples_intelligence_floor() {
        // intelligence=0 -> just k_form
        assert_eq!(min_samples(Form::Linear, Real::ZERO), 4);
        // intelligence=1 -> k_form / 1 = 4
        assert_eq!(min_samples(Form::Linear, Real::ONE), 4);
        // intelligence=2 -> 2
        assert_eq!(min_samples(Form::Linear, Real::from_int(2)), 2);
        // intelligence between 1 and 2 (1.5) -> ceil(4/1.5) = 3
        assert_eq!(min_samples(Form::Linear, Real::from_ratio(15, 10)), 3);
    }

    /// Polynomial2 (`y = a·x² + b·x + c`) recovers exact
    /// coefficients on a clean quadratic dataset. Eight samples,
    /// y = 2x² − 3x + 5.
    #[test]
    fn fit_polynomial2_recovers_exact_quadratic() {
        let samples: Vec<Sample> = (0..8).map(|i| sample(i, 2 * i * i - 3 * i + 5)).collect();
        let res = fit(Form::Polynomial2, &samples, Real::ONE).unwrap();
        assert!(
            close(res.params[0], Real::from_int(2), small_tol()),
            "a {:?}",
            res.params[0]
        );
        assert!(
            close(res.params[1], -Real::from_int(3), small_tol()),
            "b {:?}",
            res.params[1]
        );
        assert!(
            close(res.params[2], Real::from_int(5), small_tol()),
            "c {:?}",
            res.params[2]
        );
        assert!(res.residual < small_tol());
        assert!(res.is_confirmed());
    }

    /// Polynomial3 (`y = a·x³ + b·x² + c·x + d`) recovers
    /// exact coefficients on a clean cubic dataset. Twelve samples,
    /// y = x³ − 2x² + x + 4.
    #[test]
    fn fit_polynomial3_recovers_exact_cubic() {
        let samples: Vec<Sample> = (0..12)
            .map(|i| sample(i, i * i * i - 2 * i * i + i + 4))
            .collect();
        let res = fit(Form::Polynomial3, &samples, Real::ONE).unwrap();
        assert!(
            close(res.params[0], Real::from_int(1), small_tol()),
            "a {:?}",
            res.params[0]
        );
        assert!(
            close(res.params[1], -Real::from_int(2), small_tol()),
            "b {:?}",
            res.params[1]
        );
        assert!(
            close(res.params[2], Real::from_int(1), small_tol()),
            "c {:?}",
            res.params[2]
        );
        assert!(
            close(res.params[3], Real::from_int(4), small_tol()),
            "d {:?}",
            res.params[3]
        );
        assert!(res.residual < small_tol());
        assert!(res.is_confirmed());
    }

    /// `PeriodicSine` + `Logistic` remain stubbed (nonlinear in
    /// params; iterative fit is a separate workstream). The
    /// hypothesizer should treat them as `None` rather than
    /// constructing degenerate fits.
    #[test]
    fn fit_periodic_sine_remains_stubbed() {
        let samples: Vec<Sample> = (0..12).map(|i| sample(i, i % 3)).collect();
        assert!(fit(Form::PeriodicSine, &samples, Real::ONE).is_none());
        assert!(fit(Form::Logistic, &samples, Real::ONE).is_none());
    }
}
