//! Functional-form vocabulary. 12 hardcoded fixed-arity
//! single-variable templates that named figures may propose, fit,
//! and refine.
//!
//! Hypothesis-proposal sites query `available_forms()` rather than
//! enumerating `Form::ALL` directly. The current implementation is a
//! placeholder that returns all 12 at T0; the unlock table
//! graduates this to a sensorium / tech-tier driven gate
//! once that design is concrete enough to attach unlocks to.

use sim_arith::transcendental::{exp, ln, pow};
use sim_arith::Real;

/// The 12 forms in M3's vocabulary. Each is single-variable with
/// fixed arity; multi-variable regression is deferred past M3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Form {
    /// `y = a`.
    Constant,
    /// `y = a*x + b`.
    Linear,
    /// `y = a*sin(b*x + c) + d`.
    PeriodicSine,
    /// `y = a / x^2`.
    InverseSquare,
    /// `y = a * exp(-b*x)`.
    ExpDecay,
    /// `y = a * exp(b*x)`.
    ExpGrowth,
    /// `y = a / (1 + exp(-b*(x - c)))`.
    Logistic,
    /// `y = a*x^2 + b*x + c`.
    Polynomial2,
    /// `y = a*x^3 + b*x^2 + c*x + d`.
    Polynomial3,
    /// `y = a` if `x < t` else `b`. Params ordered `[a, b, t]`.
    ThresholdStep,
    /// `y = a * x^b`.
    PowerLaw,
    /// `y = a * ln(x) + b`.
    Logarithmic,
}

impl Form {
    pub const ALL: [Form; 12] = [
        Form::Constant,
        Form::Linear,
        Form::PeriodicSine,
        Form::InverseSquare,
        Form::ExpDecay,
        Form::ExpGrowth,
        Form::Logistic,
        Form::Polynomial2,
        Form::Polynomial3,
        Form::ThresholdStep,
        Form::PowerLaw,
        Form::Logarithmic,
    ];

    /// Number of free parameters in this form. Drives Occam-adjusted
    /// candidate scoring in the refinement lifecycle.
    pub fn param_count(self) -> usize {
        match self {
            Form::Constant | Form::InverseSquare => 1,
            Form::Linear
            | Form::ExpDecay
            | Form::ExpGrowth
            | Form::PowerLaw
            | Form::Logarithmic => 2,
            Form::Polynomial2 | Form::Logistic | Form::ThresholdStep => 3,
            Form::Polynomial3 | Form::PeriodicSine => 4,
        }
    }

    /// Per-form minimum-sample floor `k_form` from — how many
    /// observations the form needs to be identifiable. Higher-arity
    /// forms demand more data; the formula then divides this
    /// by `intelligence_factor` so smarter species need fewer points.
    #[allow(clippy::match_same_arms)]
    pub fn min_samples_floor(self) -> usize {
        match self {
            Form::Constant => 2,
            Form::Linear | Form::ExpDecay | Form::ExpGrowth | Form::Logarithmic => 4,
            Form::PowerLaw | Form::InverseSquare => 4,
            Form::Polynomial2 | Form::ThresholdStep => 6,
            Form::Polynomial3 => 8,
            Form::Logistic => 10,
            Form::PeriodicSine => 12,
        }
    }

    /// Per-form `base_per_form` tolerance multiplier. Tight forms
    /// (`constant`, `linear`) demand precise fits; loose forms
    /// (`periodic_sine`) are graded leniently. Numbers pinned at M3
    /// placeholders under tuning. Match arms enumerated
    /// per form for readability.
    #[allow(clippy::match_same_arms)]
    pub fn base_tolerance(self) -> Real {
        match self {
            Form::Constant => Real::from_ratio(5, 100),
            Form::Linear => Real::from_ratio(10, 100),
            Form::Polynomial2 => Real::from_ratio(15, 100),
            Form::Polynomial3 => Real::from_ratio(20, 100),
            Form::ExpDecay | Form::ExpGrowth => Real::from_ratio(15, 100),
            Form::Logarithmic => Real::from_ratio(15, 100),
            Form::PowerLaw => Real::from_ratio(20, 100),
            Form::InverseSquare => Real::from_ratio(20, 100),
            Form::ThresholdStep => Real::from_ratio(15, 100),
            Form::Logistic => Real::from_ratio(25, 100),
            Form::PeriodicSine => Real::from_ratio(30, 100),
        }
    }

    /// Stable lowercase tag for protocol events. Mirrors the
    /// snake-case convention used elsewhere in the schema.
    pub fn tag(self) -> &'static str {
        match self {
            Form::Constant => "constant",
            Form::Linear => "linear",
            Form::PeriodicSine => "periodic_sine",
            Form::InverseSquare => "inverse_square",
            Form::ExpDecay => "exp_decay",
            Form::ExpGrowth => "exp_growth",
            Form::Logistic => "logistic",
            Form::Polynomial2 => "polynomial_2",
            Form::Polynomial3 => "polynomial_3",
            Form::ThresholdStep => "threshold_step",
            Form::PowerLaw => "power_law",
            Form::Logarithmic => "logarithmic",
        }
    }

    /// SI rescaling: convert parameters fitted in normalised
    /// channel space (`x_norm = x_real / channel.scale()`) back to
    /// real-unit space so events emitted on the wire carry SI-
    /// consistent coefficients. `y` is dimensionless (firing-
    /// indicator 0/1) so only the `x` substitution matters.
    ///
    /// Per form, given `fit_params` for `f(x_norm)` and `s = scale`:
    /// - `Constant`: `[a]` unchanged
    /// - `Linear` `a*x + b`: `[a/s, b]`
    /// - `Polynomial2` `a*x² + b*x + c`: `[a/s², b/s, c]`
    /// - `Polynomial3` `a*x³ + b*x² + c*x + d`: `[a/s³, b/s², c/s, d]`
    /// - `ExpDecay` / `ExpGrowth` `a·exp(±b*x)`: `[a, b/s]`
    /// - `PowerLaw` `a·x^b`: `[a/s^b, b]`
    /// - `Logarithmic` `a·ln(x) + b`: `[a, b - a·ln(s)]`
    /// - `InverseSquare` `a/x²`: `[a*s²]`
    /// - `ThresholdStep` `[a, b, t]`: `[a, b, t*s]`
    /// - `PeriodicSine` `a·sin(b*x + c) + d`: `[a, b/s, c, d]`
    /// - `Logistic` `a/(1+exp(-b·(x−c)))`: `[a, b/s, c*s]`
    pub fn rescale_params(self, params: &[Real], x_scale: Real) -> Vec<Real> {
        debug_assert_eq!(params.len(), self.param_count(), "param arity");
        if x_scale == Real::ONE || x_scale == Real::ZERO {
            return params.to_vec();
        }
        let s = x_scale;
        let s2 = s * s;
        let s3 = s2 * s;
        match self {
            Form::Constant => params.to_vec(),
            Form::Linear => vec![params[0] / s, params[1]],
            Form::Polynomial2 => vec![params[0] / s2, params[1] / s, params[2]],
            Form::Polynomial3 => vec![params[0] / s3, params[1] / s2, params[2] / s, params[3]],
            Form::ExpDecay | Form::ExpGrowth => vec![params[0], params[1] / s],
            Form::PowerLaw => {
                // a / s^b
                let denom = pow(s, params[1]);
                vec![params[0] / denom, params[1]]
            }
            Form::Logarithmic => {
                // a · ln(x_real) + (b − a·ln(s))
                vec![params[0], params[1] - params[0] * ln(s)]
            }
            Form::InverseSquare => vec![params[0] * s2],
            Form::ThresholdStep => vec![params[0], params[1], params[2] * s],
            Form::PeriodicSine => vec![params[0], params[1] / s, params[2], params[3]],
            Form::Logistic => vec![params[0], params[1] / s, params[2] * s],
        }
    }

    /// Evaluate the form at `x` using the supplied parameters.
    /// `params` length must equal `self.param_count()`.
    pub fn evaluate(self, params: &[Real], x: Real) -> Real {
        debug_assert_eq!(params.len(), self.param_count(), "param arity");
        match self {
            Form::Constant => params[0],
            Form::Linear => params[0] * x + params[1],
            Form::Polynomial2 => params[0] * x * x + params[1] * x + params[2],
            Form::Polynomial3 => {
                let x2 = x * x;
                params[0] * x2 * x + params[1] * x2 + params[2] * x + params[3]
            }
            Form::ExpDecay => {
                // clamp the exp argument so a fit whose
                // params drift into a regime where `params[1] · x`
                // exceeds Q32.32's exp range doesn't panic during
                // evaluation. `exp` asserts the integer-shift
                // exponent ≤ 30; argument ≤ 30·ln(2) ≈ 20.8 keeps
                // it safe with margin.
                let arg = -(params[1] * x);
                let safe = arg.max(-Real::from_int(20)).min(Real::from_int(20));
                params[0] * exp(safe)
            }
            Form::ExpGrowth => {
                let arg = params[1] * x;
                let safe = arg.max(-Real::from_int(20)).min(Real::from_int(20));
                params[0] * exp(safe)
            }
            Form::PowerLaw => {
                if x <= Real::ZERO {
                    Real::ZERO
                } else {
                    params[0] * pow(x, params[1])
                }
            }
            Form::Logarithmic => {
                if x <= Real::ZERO {
                    Real::ZERO
                } else {
                    params[0] * ln(x) + params[1]
                }
            }
            Form::InverseSquare => {
                // Q32.32 underflow guard: smallest representable positive
                // value is 2^-32 ≈ 2.3e-10, so for |x| ≲ 1.5e-5 the
                // product x*x rounds to zero and the divide panics. Use
                // the same `min_safe_x` threshold as `fit_inverse_square`
                // — a meaningful inverse-square fit can't operate inside
                // the singularity anyway.
                let denom = x * x;
                if denom == Real::ZERO {
                    Real::ZERO
                } else {
                    params[0] / denom
                }
            }
            Form::ThresholdStep => {
                if x < params[2] {
                    params[0]
                } else {
                    params[1]
                }
            }
            // Stubbed for M3: stable signature so callers can compose,
            // but the fit routine returns None for these forms (see
            // `fit::fit`). follow-up replaces with iterative fits.
            Form::PeriodicSine | Form::Logistic => Real::ZERO,
        }
    }
}

/// hypothesis-proposal seam, **legacy placeholder** that returns
/// all 12 forms unconditionally. Retained for unit tests that don't
/// have a perceivable-template set in scope. Production callers use
/// `derive_available_forms` .
pub fn available_forms() -> &'static [Form] {
    &Form::ALL
}

/// form availability: derived from the structural tags on the
/// recognition templates the civ currently perceives, *not* from an
/// authored unlock table. Always includes `Constant` and `Linear`
/// (the baseline forms a civ can always propose); per-tag implications
/// add the rest.
pub fn derive_available_forms<I>(perceived_tags: I) -> Vec<Form>
where
    I: IntoIterator<Item = sim_recognition::FormTag>,
{
    let mut set: std::collections::BTreeSet<Form> = std::collections::BTreeSet::new();
    set.insert(Form::Constant);
    set.insert(Form::Linear);
    for tag in perceived_tags {
        for f in forms_for_tag(tag) {
            set.insert(*f);
        }
    }
    set.into_iter().collect()
}

/// Map a `FormTag` to the `Form` variants it implies. The
/// derivation rule: a perceivable template's tags pull these forms
/// into `available_forms`. Iterative-fit forms (`PeriodicSine`,
/// `Logistic`, `Polynomial2`/`Polynomial3`) are listed even though `fit::fit`
/// currently returns `None` for them — when iterative fits land
/// (a tuning follow-up), the form vocabulary is already correct.
pub const fn forms_for_tag(tag: sim_recognition::FormTag) -> &'static [Form] {
    use sim_recognition::FormTag;
    match tag {
        FormTag::Threshold => &[Form::ThresholdStep],
        FormTag::Periodic => &[Form::PeriodicSine],
        FormTag::DistanceDecay => &[Form::InverseSquare, Form::PowerLaw],
        FormTag::ExponentialChange => &[Form::ExpDecay, Form::ExpGrowth],
        FormTag::Logistic => &[Form::Logistic],
        FormTag::Polynomial => &[Form::Polynomial2, Form::Polynomial3],
        FormTag::PowerOrLog => &[Form::PowerLaw, Form::Logarithmic],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_twelve_present() {
        assert_eq!(Form::ALL.len(), 12);
        assert_eq!(available_forms().len(), 12);
    }

    #[test]
    fn param_counts_match_q33() {
        assert_eq!(Form::Constant.param_count(), 1);
        assert_eq!(Form::Linear.param_count(), 2);
        assert_eq!(Form::Polynomial2.param_count(), 3);
        assert_eq!(Form::Polynomial3.param_count(), 4);
        assert_eq!(Form::PeriodicSine.param_count(), 4);
    }

    #[test]
    fn evaluate_constant() {
        let p = [Real::from_int(7)];
        assert_eq!(Form::Constant.evaluate(&p, Real::from_int(123)), p[0]);
    }

    #[test]
    fn evaluate_linear() {
        // y = 2x + 3
        let p = [Real::from_int(2), Real::from_int(3)];
        assert_eq!(
            Form::Linear.evaluate(&p, Real::from_int(5)),
            Real::from_int(13)
        );
    }

    #[test]
    fn evaluate_polynomial2() {
        // y = x^2 - 2x + 1 = (x - 1)^2
        let p = [Real::from_int(1), Real::from_int(-2), Real::from_int(1)];
        assert_eq!(
            Form::Polynomial2.evaluate(&p, Real::from_int(4)),
            Real::from_int(9)
        );
    }

    #[test]
    fn evaluate_polynomial3() {
        // y = x^3
        let p = [Real::from_int(1), Real::ZERO, Real::ZERO, Real::ZERO];
        assert_eq!(
            Form::Polynomial3.evaluate(&p, Real::from_int(3)),
            Real::from_int(27)
        );
    }

    #[test]
    fn evaluate_threshold_step() {
        // a=1 below 5, b=10 at-or-above 5
        let p = [Real::from_int(1), Real::from_int(10), Real::from_int(5)];
        assert_eq!(
            Form::ThresholdStep.evaluate(&p, Real::from_int(3)),
            Real::from_int(1)
        );
        assert_eq!(
            Form::ThresholdStep.evaluate(&p, Real::from_int(7)),
            Real::from_int(10)
        );
        assert_eq!(
            Form::ThresholdStep.evaluate(&p, Real::from_int(5)),
            Real::from_int(10)
        );
    }

    #[test]
    fn evaluate_inverse_square() {
        // y = 100 / x^2
        let p = [Real::from_int(100)];
        assert_eq!(
            Form::InverseSquare.evaluate(&p, Real::from_int(10)),
            Real::ONE
        );
    }

    #[test]
    fn tags_are_stable_snake_case() {
        assert_eq!(Form::Linear.tag(), "linear");
        assert_eq!(Form::PeriodicSine.tag(), "periodic_sine");
        assert_eq!(Form::ThresholdStep.tag(), "threshold_step");
    }
}
