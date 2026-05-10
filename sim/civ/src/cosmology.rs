//! Cosmology drift dynamics. Five orthogonal axes per civ
//! that drift with events; magnitudes scaled by figure charisma;
//! `civ_dogmatism` derived as the L2-norm of the vector.
//! Culture-influence hooks read from this state.

use crate::forms::Form;
use sim_arith::transcendental::sqrt;
use sim_arith::Real;

/// Five axes, each in `[-1, 1]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cosmology {
    pub empirical: Real,
    pub communitarian: Real,
    pub reformist: Real,
    pub mystical: Real,
    pub hierarchical: Real,
}

impl Cosmology {
    pub const NEUTRAL: Cosmology = Cosmology {
        empirical: Real::ZERO,
        communitarian: Real::ZERO,
        reformist: Real::ZERO,
        mystical: Real::ZERO,
        hierarchical: Real::ZERO,
    };

    /// L2-norm magnitude. Used for `dogmatism` and for the
    /// `CosmologyShifted` event-emission gate.
    pub fn magnitude(&self) -> Real {
        let s = self.empirical * self.empirical
            + self.communitarian * self.communitarian
            + self.reformist * self.reformist
            + self.mystical * self.mystical
            + self.hierarchical * self.hierarchical;
        sqrt(s)
    }

    /// dogmatism = magnitude / sqrt(5), clamped `[0, 1]`.
    /// Read by / suppression formulas.
    pub fn dogmatism(&self) -> Real {
        let denom = sqrt(Real::from_int(5));
        if denom == Real::ZERO {
            return Real::ZERO;
        }
        let raw = self.magnitude() / denom;
        raw.max(Real::ZERO).min(Real::ONE)
    }

    /// L2 distance to another cosmology vector.
    pub fn distance_to(&self, other: &Cosmology) -> Real {
        let de = self.empirical - other.empirical;
        let dc = self.communitarian - other.communitarian;
        let dr = self.reformist - other.reformist;
        let dm = self.mystical - other.mystical;
        let dh = self.hierarchical - other.hierarchical;
        sqrt(de * de + dc * dc + dr * dr + dm * dm + dh * dh)
    }

    /// Apply a push-vector then clamp every component to `[-1, 1]`.
    pub fn push(&mut self, push: &Cosmology, magnitude: Real) {
        self.empirical = (self.empirical + push.empirical * magnitude)
            .max(-Real::ONE)
            .min(Real::ONE);
        self.communitarian = (self.communitarian + push.communitarian * magnitude)
            .max(-Real::ONE)
            .min(Real::ONE);
        self.reformist = (self.reformist + push.reformist * magnitude)
            .max(-Real::ONE)
            .min(Real::ONE);
        self.mystical = (self.mystical + push.mystical * magnitude)
            .max(-Real::ONE)
            .min(Real::ONE);
        self.hierarchical = (self.hierarchical + push.hierarchical * magnitude)
            .max(-Real::ONE)
            .min(Real::ONE);
    }

    /// Pack into Q32.32 raw bits for protocol events.
    pub fn axes_q32(&self) -> [i64; 5] {
        [
            self.empirical.raw().to_bits(),
            self.communitarian.raw().to_bits(),
            self.reformist.raw().to_bits(),
            self.mystical.raw().to_bits(),
            self.hierarchical.raw().to_bits(),
        ]
    }
}

/// per-event push tables. See `q24.md`. halved every
/// magnitude — cosmology is now the slow-drift deep-worldview
/// layer; the fast cultural drift work lives in
/// `crate::religion::push_for_*` with 3× the impulse.
pub fn push_for_relation_confirmed() -> Cosmology {
    Cosmology {
        empirical: Real::from_ratio(25, 1000),
        communitarian: Real::ZERO,
        reformist: Real::from_ratio(10, 1000),
        mystical: -Real::from_ratio(20, 1000),
        hierarchical: Real::ZERO,
    }
}

pub fn push_for_refinement_proposed() -> Cosmology {
    Cosmology {
        empirical: Real::from_ratio(10, 1000),
        communitarian: Real::ZERO,
        reformist: Real::from_ratio(20, 1000),
        mystical: Real::ZERO,
        hierarchical: Real::ZERO,
    }
}

pub fn push_for_refinement_confirmed() -> Cosmology {
    Cosmology {
        empirical: Real::from_ratio(20, 1000),
        communitarian: Real::ZERO,
        reformist: Real::from_ratio(30, 1000),
        mystical: -Real::from_ratio(10, 1000),
        hierarchical: Real::ZERO,
    }
}

pub fn push_for_refinement_rejected() -> Cosmology {
    Cosmology {
        empirical: -Real::from_ratio(10, 1000),
        communitarian: Real::ZERO,
        reformist: -Real::from_ratio(20, 1000),
        mystical: Real::from_ratio(20, 1000),
        hierarchical: Real::from_ratio(10, 1000),
    }
}

pub fn push_for_civ_collapsed() -> Cosmology {
    Cosmology {
        empirical: Real::ZERO,
        communitarian: Real::from_ratio(50, 1000),
        reformist: -Real::from_ratio(50, 1000),
        mystical: Real::from_ratio(75, 1000),
        hierarchical: Real::from_ratio(25, 1000),
    }
}

/// emission-gate: re-emit `CosmologyShifted` only when the
/// cosmology has drifted at least this far in L2 distance from the
/// last emitted snapshot. raised this from 0.20 to 0.50:
/// cosmology is now the slow-drift deep-worldview layer (the
/// fast-divergent religion / customs work moved to
/// `crate::religion`), and 0.50 keeps cosmology events near-
/// millennium-rare rather than firing every few centuries.
pub const COSMOLOGY_EMIT_THRESHOLD: (i64, i64) = (50, 100);

/// form-distance tag used by `suppress_confidence`. Returns the
/// distance `[0, 1]` between the form and the civ's cosmology
/// preference. Higher = more heretical = stronger suppression.
/// Match arms enumerated per form for readability.
#[allow(clippy::match_same_arms)]
pub fn form_distance(form: Form, cosmology: &Cosmology) -> Real {
    let half = Real::from_ratio(1, 2);
    let quarter = Real::from_ratio(1, 4);
    match form {
        Form::Constant => (Real::ONE + cosmology.reformist) * half,
        Form::Linear => Real::ZERO,
        Form::ThresholdStep => (Real::ONE - cosmology.empirical) * half,
        Form::Polynomial2 | Form::Polynomial3 => {
            (Real::ONE - (cosmology.reformist + cosmology.empirical) * half) * half
        }
        Form::PeriodicSine => (Real::ONE - cosmology.mystical) * half,
        Form::ExpDecay | Form::ExpGrowth => {
            ((Real::ONE - cosmology.empirical) + cosmology.reformist) * quarter
        }
        Form::Logistic => (Real::ONE - cosmology.reformist) * half,
        Form::PowerLaw => {
            ((Real::ONE - cosmology.empirical) + (Real::ONE - cosmology.reformist)) * quarter
        }
        Form::Logarithmic | Form::InverseSquare => (Real::ONE - cosmology.empirical) * half,
    }
    .max(Real::ZERO)
    .min(Real::ONE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_cosmology_has_zero_dogmatism() {
        assert_eq!(Cosmology::NEUTRAL.dogmatism(), Real::ZERO);
    }

    #[test]
    fn fully_aligned_cosmology_has_dogmatism_one() {
        // All axes at +1 → magnitude = sqrt(5) → dogmatism = 1.
        let c = Cosmology {
            empirical: Real::ONE,
            communitarian: Real::ONE,
            reformist: Real::ONE,
            mystical: Real::ONE,
            hierarchical: Real::ONE,
        };
        let dog = c.dogmatism();
        // Within ~0.01 of 1.0 (sqrt under Q32.32).
        assert!(dog > Real::from_ratio(99, 100));
        assert!(dog <= Real::ONE);
    }

    #[test]
    fn push_clamps_to_unit_range() {
        let mut c = Cosmology::NEUTRAL;
        let big = Cosmology {
            empirical: Real::from_int(2),
            ..Cosmology::NEUTRAL
        };
        c.push(&big, Real::from_int(10));
        assert_eq!(c.empirical, Real::ONE);
    }

    #[test]
    fn push_negative_clamps_to_minus_one() {
        let mut c = Cosmology::NEUTRAL;
        let neg = Cosmology {
            mystical: -Real::from_int(2),
            ..Cosmology::NEUTRAL
        };
        c.push(&neg, Real::from_int(10));
        assert_eq!(c.mystical, -Real::ONE);
    }

    #[test]
    fn relation_confirmed_push_increments_empirical() {
        let mut c = Cosmology::NEUTRAL;
        c.push(&push_for_relation_confirmed(), Real::ONE);
        assert!(c.empirical > Real::ZERO);
        assert!(c.mystical < Real::ZERO);
    }

    #[test]
    fn collapse_push_drives_toward_mysticism() {
        let mut c = Cosmology::NEUTRAL;
        c.push(&push_for_civ_collapsed(), Real::ONE);
        assert!(c.mystical > Real::ZERO);
        assert!(c.hierarchical > Real::ZERO);
        assert!(c.reformist < Real::ZERO);
    }

    #[test]
    fn neutral_form_distance_zero_for_linear() {
        assert_eq!(form_distance(Form::Linear, &Cosmology::NEUTRAL), Real::ZERO);
    }

    #[test]
    fn mystical_civ_finds_periodic_sine_close() {
        let mystical = Cosmology {
            mystical: Real::from_ratio(8, 10),
            ..Cosmology::NEUTRAL
        };
        let neutral_dist = form_distance(Form::PeriodicSine, &Cosmology::NEUTRAL);
        let mystical_dist = form_distance(Form::PeriodicSine, &mystical);
        assert!(mystical_dist < neutral_dist);
    }

    #[test]
    fn empirical_civ_finds_threshold_step_close() {
        let empirical = Cosmology {
            empirical: Real::from_ratio(8, 10),
            ..Cosmology::NEUTRAL
        };
        let neutral_dist = form_distance(Form::ThresholdStep, &Cosmology::NEUTRAL);
        let empirical_dist = form_distance(Form::ThresholdStep, &empirical);
        assert!(empirical_dist < neutral_dist);
    }
}
