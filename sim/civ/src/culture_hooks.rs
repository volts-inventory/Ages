//! Cultural-influence hooks. M3 shipped these as no-op
//! pass-throughs; M4 wires them to the cosmology vector.
//!
//! - **`allow_observation`**: returns a suppression
//!   probability `[0, 1]`. M4-min returns `0.0` (never
//!   suppresses) — the per-cell-roll consumer waits on
//!   settlement-pop.
//! - **`focus_weight`**: returns `[0, 1]+` weight scaling
//!   per-relation attention. M4-min reads cosmology
//!   `empirical` + `reformist` to produce a global weight; the
//!   per-relation consumer (cadence variation) wires in M4 v2.
//! - **`suppress_confidence`**: returns a multiplier
//!   `[0, 1]` applied to fit confidence before the
//!   `≥ exp(-1)` confirmation gate. **Wired in M4-min** —
//!   the discovery pipeline reads the multiplier directly.

use crate::cosmology::{form_distance, Cosmology};
use crate::fit::Sample;
use crate::forms::Form;
use sim_arith::Real;

/// Dogmatism-derived per-template suppression
/// probability. M4-min returns `0.0` (never suppresses) until
/// per-cell taboo state is wired. Hook signature reads
/// dogmatism so callers consume the formula even before the
/// behavioural consumer lands.
pub fn allow_observation_suppression(_template_id: u32, _cell: u32, _dogmatism: Real) -> Real {
    Real::ZERO
}

/// Backwards-compatible bool wrapper used by the M3 stub.
pub fn allow_observation(template_id: u32, cell: u32) -> bool {
    let _ = (template_id, cell);
    true
}

/// Focus weight in `[0, 1]+`. M4-min formula reads the
/// civ's cosmology and produces a global per-relation weight;
/// per-relation novelty taxonomy lands in M4 v2.
pub fn focus_weight_for(_relation_id: u32, cosmology: &Cosmology) -> Real {
    let bonus = Real::from_ratio(25, 100);
    Real::ONE + bonus * cosmology.empirical + bonus * cosmology.reformist
}

/// Backwards-compatible signature for tests and pre-cosmology
/// callers (returns the constant 1.0).
pub fn focus_weight(_relation_id: u32) -> Real {
    Real::ONE
}

/// Confidence-suppression multiplier — **wired in M4-min**.
/// `factor = clamp(1 − dogmatism × form_distance(form, cosmology),
/// 0, 1)`. Dogmatic civs accumulate confidence at a fraction of
/// the base rate on heretical forms; neutral civs and forms
/// matching the cosmology pass through at 1.0.
pub fn suppress_confidence_for(form: Form, cosmology: &Cosmology) -> Real {
    let dist = form_distance(form, cosmology);
    let dog = cosmology.dogmatism();
    let raw = Real::ONE - dog * dist;
    raw.clamp01()
}

/// Backwards-compatible signature (no cosmology in scope).
pub fn suppress_confidence(_relation_id: u32, raw_confidence: Real) -> Real {
    raw_confidence
}

/// Convenience: M3-era passthrough kept so tests don't break.
pub fn gate_samples(samples: Vec<Sample>) -> Vec<Sample> {
    samples
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_observation_pass_through_in_m4_min() {
        // Until per-cell consumer is wired, suppression
        // probability returns 0 regardless of dogmatism.
        for tid in 1..=5 {
            for cell in 0..16 {
                assert_eq!(
                    allow_observation_suppression(tid, cell, Real::ONE),
                    Real::ZERO
                );
                assert!(allow_observation(tid, cell));
            }
        }
    }

    #[test]
    fn focus_weight_neutral_returns_one() {
        let f = focus_weight_for(0, &Cosmology::NEUTRAL);
        assert_eq!(f, Real::ONE);
    }

    #[test]
    fn focus_weight_empirical_civ_amplifies() {
        let cos = Cosmology {
            empirical: Real::from_ratio(8, 10),
            ..Cosmology::NEUTRAL
        };
        let f = focus_weight_for(0, &cos);
        assert!(f > Real::ONE);
    }

    #[test]
    fn suppress_confidence_neutral_passes_through() {
        let m = suppress_confidence_for(Form::Linear, &Cosmology::NEUTRAL);
        assert_eq!(m, Real::ONE);
    }

    #[test]
    fn suppress_confidence_mystical_civ_suppresses_threshold_step() {
        let mystical = Cosmology {
            mystical: Real::from_ratio(9, 10),
            ..Cosmology::NEUTRAL
        };
        let pass = suppress_confidence_for(Form::PeriodicSine, &mystical);
        let suppressed = suppress_confidence_for(Form::ThresholdStep, &mystical);
        assert!(suppressed < pass);
        assert!(suppressed >= Real::ZERO);
    }
}
