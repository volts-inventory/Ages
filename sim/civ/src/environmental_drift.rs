//! M7 — environmental feedback on species drift.
//!
//! Two complementary mechanisms shipped together so the species'
//! trait trajectory reflects what its planet did to it:
//!
//! **Selection-on-survival (option a):** when a catastrophe kills
//! a fraction of a civ's population, the surviving cohort's trait
//! distribution is biased toward survival-correlated traits
//! (high-cognition civs survive volcanic ash better; cooperative
//! civs survive plague; etc.). The trait deltas the parent passes
//! to successors at founding reflect this bias. Bias accumulates
//! across the parent's lifetime, transmits to the child at
//! `inherit_species_drift_with_environment`, then resets so the
//! child accumulates its own.
//!
//! **Substrate-locked drift bands (option c):** the random
//! per-generation perturbation magnitude scales with the planet's
//! metabolism and biosphere class. Silicate worlds (metabolism
//! ≈ 0.2) get ~half the drift step of aqueous; hyper-biodiverse
//! biospheres widen the band ~1.5×. Net effect: a slow-substrate
//! sparse world gets very narrow per-generation drift bands,
//! a lush hyper-bio aqueous world gets wide ones.
//!
//! Stress-induced amplification (option b) was deliberately
//! skipped — it creates a self-reinforcing "harsh seeds stay
//! harsh" feedback that may not converge.

use crate::catastrophe::CatastropheKind;
use sim_arith::Real;
use sim_world::BiosphereClass;

/// Per-channel selection-on-survival bias. Each entry is the
/// pending bias on one of the four drift channels — same channel
/// order as the four `*_delta` fields on `Civ` (cognition,
/// sociality, lifespan_years, communication_fidelity).
///
/// Accumulates as catastrophes fire on the parent civ;
/// `inherit_species_drift_with_environment` folds it into the
/// successor's inherited delta before applying the random
/// perturbation. The child starts fresh with zero bias.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectionBias {
    pub cognition: Real,
    pub sociality: Real,
    pub lifespan_years: Real,
    pub communication_fidelity: Real,
}

impl SelectionBias {
    #[must_use]
    pub fn zero() -> Self {
        Self {
            cognition: Real::ZERO,
            sociality: Real::ZERO,
            lifespan_years: Real::ZERO,
            communication_fidelity: Real::ZERO,
        }
    }

    /// Element-wise add. Used to accumulate bias from multiple
    /// catastrophes on the same civ.
    #[must_use]
    pub fn add(self, other: Self) -> Self {
        Self {
            cognition: self.cognition + other.cognition,
            sociality: self.sociality + other.sociality,
            lifespan_years: self.lifespan_years + other.lifespan_years,
            communication_fidelity: self.communication_fidelity + other.communication_fidelity,
        }
    }

    /// Element-wise scale.
    #[must_use]
    pub fn scale(self, factor: Real) -> Self {
        Self {
            cognition: self.cognition * factor,
            sociality: self.sociality * factor,
            lifespan_years: self.lifespan_years * factor,
            communication_fidelity: self.communication_fidelity * factor,
        }
    }
}

/// Per-kind selection-on-survival weights. Each entry is the
/// per-channel pressure on the surviving cohort when the given
/// catastrophe fires — scaled by `fraction_lost` at the call site
/// so heavier catastrophes produce stronger selection. Values are
/// in the same units as the existing `*_delta` fields, i.e. small
/// fractions of the species baseline.
///
/// Channel order: (cognition, sociality, lifespan_years, communication_fidelity).
/// Lifespan is in years; the other three are unit-1 [0,1] traits.
///
/// **Rationale per kind:**
/// - Volcanic ash: heavy on cognition (planning + evacuation),
///   moderate on lifespan (experienced elders direct the
///   response).
/// - Disease: heavy on cognition (hygiene + medicine), heavy on
///   sociality (cooperative care, isolation discipline).
/// - Asteroid: heavy on cognition + sociality (coordination).
/// - Solar flare: moderate on cognition (shelter, EM literacy).
/// - Ice age: heavy on all three growth traits — cold pressure
///   selects for planning, cooperation, and longer-lived
///   knowledge-bearers.
#[must_use]
pub fn catastrophe_selection_weights(kind: CatastropheKind) -> SelectionBias {
    let r = |num, den| Real::from_ratio(num, den);
    match kind {
        // Volcanic: high cog (0.06), low soc (0.02), moderate
        // lifespan (1.0 year), low communication (0.02).
        CatastropheKind::Volcanic => SelectionBias {
            cognition: r(6, 100),
            sociality: r(2, 100),
            lifespan_years: r(1, 1),
            communication_fidelity: r(2, 100),
        },
        // Disease: cooperative + cognitive pressure dominates.
        CatastropheKind::Disease => SelectionBias {
            cognition: r(8, 100),
            sociality: r(6, 100),
            lifespan_years: r(2, 1),
            communication_fidelity: r(3, 100),
        },
        // Asteroid: rare, strong cog + soc when it hits.
        CatastropheKind::Asteroid => SelectionBias {
            cognition: r(10, 100),
            sociality: r(8, 100),
            lifespan_years: r(2, 1),
            communication_fidelity: r(3, 100),
        },
        // Solar flare: moderate cog only — pure-EM event, doesn't
        // pressure social structure much beyond shelter.
        CatastropheKind::SolarFlare => SelectionBias {
            cognition: r(5, 100),
            sociality: r(1, 100),
            lifespan_years: r(0, 1),
            communication_fidelity: r(2, 100),
        },
        // Ice age: sustained selection on every growth axis.
        CatastropheKind::IceAge => SelectionBias {
            cognition: r(8, 100),
            sociality: r(6, 100),
            lifespan_years: r(3, 1),
            communication_fidelity: r(3, 100),
        },
    }
}

/// Compute the per-civ selection-bias *contribution* of a single
/// catastrophe. Scales the per-kind weights by the fraction of
/// population lost — heavier catastrophes are stronger selection
/// pressures.
#[must_use]
pub fn selection_bias_for_catastrophe(
    kind: CatastropheKind,
    fraction_lost: Real,
) -> SelectionBias {
    let frac = fraction_lost.max(Real::ZERO).min(Real::ONE);
    catastrophe_selection_weights(kind).scale(frac)
}

/// Substrate-locked drift width. Returns the multiplier applied
/// to the random per-generation perturbation magnitude. Slow-
/// substrate worlds (silicate metabolism ≈ 0.2) shrink the drift
/// band so trait deltas accumulate over many more generations
/// than on a fast-substrate world.
///
/// Uses the same `metabolism` scalar already used by
/// `scale_attempt_period_for_metabolism` and
/// `streak_ticks_for_metabolism`, so substrate sensitivity
/// reads consistent across the codebase. Floor at 0.25 keeps
/// silicate worlds from going completely static.
#[must_use]
pub fn substrate_drift_factor(metabolism: Real) -> Real {
    let m = metabolism.max(Real::ZERO);
    // Linear: factor = max(0.25, metabolism). Aqueous (1.0) →
    // 1.0× (unchanged); silicate (~0.2) → 0.25× (4× slower
    // drift); ammoniacal (~0.5) → 0.5×.
    m.max(Real::from_ratio(25, 100))
}

/// Biosphere-driven drift width. Returns the multiplier applied
/// alongside `substrate_drift_factor` so hyper-biodiverse worlds
/// produce a wider drift band (faster trait turnover; more
/// variation between successors) and sparse worlds narrow it.
///
/// `None` biosphere returns 0.5 since worlds without a working
/// biosphere have no real selection pressure beyond random walk.
#[must_use]
pub fn biosphere_drift_factor(biosphere: BiosphereClass) -> Real {
    let r = |num, den| Real::from_ratio(num, den);
    match biosphere {
        BiosphereClass::None => r(50, 100),
        BiosphereClass::Sparse => r(70, 100),
        BiosphereClass::Lush => r(100, 100),
        BiosphereClass::HyperBiodiverse => r(150, 100),
    }
}

/// Combined environmental factor — `substrate × biosphere`. Used
/// by `inherit_species_drift_with_environment` to scale the
/// random perturbation step. Floored at 0.1 to keep determinism
/// (a perfectly-zero step would defeat replay-time hashing in
/// downstream consumers that read drift deltas).
#[must_use]
pub fn drift_band_factor(metabolism: Real, biosphere: BiosphereClass) -> Real {
    let combined = substrate_drift_factor(metabolism) * biosphere_drift_factor(biosphere);
    combined.max(Real::from_ratio(10, 100))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_bias_weights_are_positive_and_kind_distinct() {
        let v = catastrophe_selection_weights(CatastropheKind::Volcanic);
        let d = catastrophe_selection_weights(CatastropheKind::Disease);
        let i = catastrophe_selection_weights(CatastropheKind::IceAge);
        assert!(v.cognition > Real::ZERO);
        assert!(d.sociality > Real::ZERO);
        assert!(i.lifespan_years > Real::ZERO);
        // Disease > volcanic on sociality (cooperative care vs
        // ash evacuation).
        assert!(d.sociality > v.sociality);
        // Ice age > volcanic on lifespan (sustained pressure).
        assert!(i.lifespan_years > v.lifespan_years);
    }

    #[test]
    fn selection_bias_scales_linearly_with_fraction_lost() {
        let half =
            selection_bias_for_catastrophe(CatastropheKind::Disease, Real::from_ratio(5, 10));
        let full = selection_bias_for_catastrophe(CatastropheKind::Disease, Real::ONE);
        // 0.5 fraction → half the bias.
        let drift = full.cognition - (half.cognition * Real::from_int(2));
        assert!(drift.abs() < Real::from_ratio(1, 1000));
        // Zero fraction → zero bias (extreme).
        let zero =
            selection_bias_for_catastrophe(CatastropheKind::Disease, Real::ZERO);
        assert_eq!(zero, SelectionBias::zero());
    }

    #[test]
    fn substrate_drift_factor_narrows_for_silicate() {
        // Aqueous metabolism = 1.0 → factor 1.0.
        let aq = substrate_drift_factor(Real::ONE);
        let drift_aq = Real::ONE - aq;
        assert!(drift_aq.abs() < Real::from_ratio(1, 1000));
        // Silicate metabolism ≈ 0.2 → factor 0.25 (the floor).
        let si = substrate_drift_factor(Real::from_ratio(2, 10));
        let drift_si = si - Real::from_ratio(25, 100);
        assert!(drift_si.abs() < Real::from_ratio(1, 1000));
        // Ammoniacal metabolism = 0.5 → factor 0.5.
        let am = substrate_drift_factor(Real::from_ratio(5, 10));
        let drift_am = am - Real::from_ratio(5, 10);
        assert!(drift_am.abs() < Real::from_ratio(1, 1000));
    }

    #[test]
    fn biosphere_drift_factor_widens_with_biodiversity() {
        let none = biosphere_drift_factor(BiosphereClass::None);
        let sparse = biosphere_drift_factor(BiosphereClass::Sparse);
        let lush = biosphere_drift_factor(BiosphereClass::Lush);
        let hyper = biosphere_drift_factor(BiosphereClass::HyperBiodiverse);
        assert!(none < sparse);
        assert!(sparse < lush);
        assert!(lush < hyper);
    }

    #[test]
    fn drift_band_factor_combines_substrate_and_biosphere() {
        // Silicate + sparse: 0.25 × 0.7 = 0.175, above floor of 0.1.
        let f = drift_band_factor(Real::from_ratio(2, 10), BiosphereClass::Sparse);
        let expected = Real::from_ratio(175, 1000);
        let drift = if f > expected { f - expected } else { expected - f };
        assert!(drift < Real::from_ratio(1, 100));
        // Aqueous + hyper-biodiverse: 1.0 × 1.5 = 1.5.
        let f2 = drift_band_factor(Real::ONE, BiosphereClass::HyperBiodiverse);
        let exp2 = Real::from_ratio(15, 10);
        let drift2 = if f2 > exp2 { f2 - exp2 } else { exp2 - f2 };
        assert!(drift2 < Real::from_ratio(1, 100));
        // None biosphere on a very-low metabolism world: substrate
        // floor (0.25) × biosphere None (0.5) = 0.125, above the
        // 0.1 floor — so the formula doesn't hit the floor here.
        let f3 = drift_band_factor(Real::from_ratio(1, 100), BiosphereClass::None);
        let expected3 = Real::from_ratio(125, 1000);
        let drift3 = if f3 > expected3 {
            f3 - expected3
        } else {
            expected3 - f3
        };
        assert!(drift3 < Real::from_ratio(1, 100));
    }

    #[test]
    fn selection_bias_add_and_scale_are_element_wise() {
        // Q32.32 fixed-point arithmetic doesn't round perfectly
        // on rational additions of unit fractions (1/10 + 1/10
        // lands at 0.1999999997, not 0.2 exactly). Use a small
        // tolerance instead of `assert_eq` so the assertion
        // captures intent rather than bit-exactness.
        let near = |a: Real, b: Real| -> bool {
            let drift = if a > b { a - b } else { b - a };
            drift < Real::from_ratio(1, 1_000_000)
        };
        let a = SelectionBias {
            cognition: Real::from_ratio(1, 10),
            sociality: Real::from_ratio(2, 10),
            lifespan_years: Real::from_int(1),
            communication_fidelity: Real::from_ratio(3, 10),
        };
        let b = SelectionBias {
            cognition: Real::from_ratio(1, 10),
            sociality: Real::from_ratio(1, 10),
            lifespan_years: Real::from_int(2),
            communication_fidelity: Real::ZERO,
        };
        let sum = a.add(b);
        assert!(near(sum.cognition, Real::from_ratio(2, 10)));
        assert!(near(sum.sociality, Real::from_ratio(3, 10)));
        assert!(near(sum.lifespan_years, Real::from_int(3)));
        let scaled = a.scale(Real::from_ratio(5, 10));
        assert!(near(scaled.cognition, Real::from_ratio(5, 100)));
        assert!(near(scaled.lifespan_years, Real::from_ratio(5, 10)));
    }
}
