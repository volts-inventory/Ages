//! Religion / customs vector. Three orthogonal axes per civ
//! that capture the *fast-divergent* cultural-religious layer
//! sitting on top of the slow-drift `Cosmology`. Cosmology is the
//! deep worldview shared across civs of one species (via the
//! species bias); religion is the fast-divergent layer that
//! actually drives intra-species war (Reformation, sectarian
//! conflict, etc.).
//!
//! Each axis sits in `[-1, 1]` like cosmology. Axes:
//! - `theology` — monist (-1) ↔ pluralist (+1). One-god/one-truth
//!   vs. many-spirits/animism.
//! - `ritual` — pragmatic (-1) ↔ liturgical (+1). Ad-hoc/expedient
//!   observance vs. formal/orthodox.
//! - `sacred_time` — cyclical (-1) ↔ eschatological (+1). Eternal-
//!   return vs. arc-toward-end.
//!
//! Drift dynamics mirror cosmology's `push_for_*` tables but with
//! 3–5× larger magnitudes (this layer is *meant* to be volatile).
//! Every cosmology drift hook has a religion counterpart in
//! `sim/core/src/phases.rs`. Founding-time variation comes from
//! figure traits + civ_id-derived jitter so two civs of the same
//! species don't start with identical religion vectors.

use sim_arith::transcendental::sqrt;
use sim_arith::Real;

/// Three orthogonal axes each in `[-1, 1]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Religion {
    /// Monist (-1) ↔ pluralist (+1).
    pub theology: Real,
    /// Pragmatic (-1) ↔ liturgical (+1).
    pub ritual: Real,
    /// Cyclical (-1) ↔ eschatological (+1).
    pub sacred_time: Real,
}

impl Religion {
    pub const NEUTRAL: Religion = Religion {
        theology: Real::ZERO,
        ritual: Real::ZERO,
        sacred_time: Real::ZERO,
    };

    /// L2-norm magnitude. Used for the `ReligionShifted`
    /// event-emission gate.
    pub fn magnitude(&self) -> Real {
        let s = self.theology * self.theology
            + self.ritual * self.ritual
            + self.sacred_time * self.sacred_time;
        sqrt(s)
    }

    /// dogmatism analogue: `magnitude / sqrt(3)`, clamped
    /// `[0, 1]`. A civ deep in any one corner of the religion
    /// space reads as "dogmatic in its religion" the same way
    /// cosmology's high-magnitude civs read as cosmologically
    /// dogmatic.
    pub fn dogmatism(&self) -> Real {
        let denom = sqrt(Real::from_int(3));
        if denom == Real::ZERO {
            return Real::ZERO;
        }
        let raw = self.magnitude() / denom;
        raw.max(Real::ZERO).min(Real::ONE)
    }

    /// L2 distance to another religion vector.
    pub fn distance_to(&self, other: &Religion) -> Real {
        let dt = self.theology - other.theology;
        let dr = self.ritual - other.ritual;
        let ds = self.sacred_time - other.sacred_time;
        sqrt(dt * dt + dr * dr + ds * ds)
    }

    /// Apply a push-vector then clamp every component to `[-1, 1]`.
    pub fn push(&mut self, push: &Religion, magnitude: Real) {
        self.theology = (self.theology + push.theology * magnitude)
            .max(-Real::ONE)
            .min(Real::ONE);
        self.ritual = (self.ritual + push.ritual * magnitude)
            .max(-Real::ONE)
            .min(Real::ONE);
        self.sacred_time = (self.sacred_time + push.sacred_time * magnitude)
            .max(-Real::ONE)
            .min(Real::ONE);
    }

    /// Pack into Q32.32 raw bits for protocol events.
    pub fn axes_q32(&self) -> [i64; 3] {
        [
            self.theology.raw().to_bits(),
            self.ritual.raw().to_bits(),
            self.sacred_time.raw().to_bits(),
        ]
    }
}

/// Founding-time religion. Built from species baseline (always
/// neutral — religion has no -style species bias because it
/// *should* differentiate per civ) plus a deterministic per-civ
/// jitter and a founding-figure-driven offset. The jitter +
/// figure offset together push two civs of the same species
/// onto distinct religion vectors at birth — mimicking how real
/// religious traditions diverge from founding-figure personality
/// + early-event chance even within one species.
///
/// `figure_charisma`, `figure_doubt`, `figure_curiosity` are the
/// founding figure's trait scalars in `[0, 1]`. `civ_id` seeds
/// the jitter so the same `civ_id` always lands at the same
/// religion vector (modulo figure traits, which are themselves
/// deterministic from `(species_seed, civ_id)`).
pub fn founding_religion(
    civ_id: u32,
    figure_charisma: Real,
    figure_doubt: Real,
    figure_curiosity: Real,
) -> Religion {
    // Deterministic ±0.20 jitter per axis from civ_id. Splitmix-
    // style hash without external RNG (kept deterministic and
    // arithmetic-only; same civ_id → same offset).
    let jitter = |salt: u32| -> Real {
        let mixed = civ_id
            .wrapping_mul(2_654_435_761)
            .wrapping_add(salt.wrapping_mul(40_503));
        // Map u32 → [-0.20, 0.20] in 41-step buckets so the
        // result is always representable as a clean Q32.32 ratio.
        let bucket = i64::from(mixed % 41) - 20;
        Real::from_ratio(bucket, 100)
    };
    // Figure-trait offsets:
    //  theology = -0.5·doubt (high doubt → toward monism — there must be one truth)
    //  ritual = +0.5·charisma (charismatic founder → formal liturgy)
    //  sacred_time = -0.3·curiosity (curious founder → look to past for patterns, cyclical)
    let half = Real::from_ratio(1, 2);
    let three_tenths = Real::from_ratio(3, 10);
    let theology = (-figure_doubt * half + jitter(1))
        .max(-Real::ONE)
        .min(Real::ONE);
    let ritual = (figure_charisma * half + jitter(2))
        .max(-Real::ONE)
        .min(Real::ONE);
    let sacred_time = (-figure_curiosity * three_tenths + jitter(3))
        .max(-Real::ONE)
        .min(Real::ONE);
    Religion {
        theology,
        ritual,
        sacred_time,
    }
}

// === push tables ============================================
//
// Mirror the cosmology `push_for_*` hooks but with 3× the magnitude
// — this layer is *meant* to be volatile.

/// Science accumulates → moves theology toward monism (one truth)
/// and sacred time toward eschatological (history-as-progress).
pub fn push_for_relation_confirmed() -> Religion {
    Religion {
        theology: -Real::from_ratio(15, 100),
        ritual: Real::ZERO,
        sacred_time: Real::from_ratio(10, 100),
    }
}

/// Openness to revising laws → ritual toward pragmatic, sacred
/// time toward eschatological.
pub fn push_for_refinement_proposed() -> Religion {
    Religion {
        theology: Real::ZERO,
        ritual: -Real::from_ratio(8, 100),
        sacred_time: Real::from_ratio(6, 100),
    }
}

/// Confirmed refinement: same direction as proposed but stronger.
pub fn push_for_refinement_confirmed() -> Religion {
    Religion {
        theology: -Real::from_ratio(8, 100),
        ritual: -Real::from_ratio(12, 100),
        sacred_time: Real::from_ratio(10, 100),
    }
}

/// Refinement rejected: defending orthodoxy → ritual toward
/// liturgical, theology toward pluralism (older spirits-and-omens
/// view), sacred time toward cyclical (return to tradition).
pub fn push_for_refinement_rejected() -> Religion {
    Religion {
        theology: Real::from_ratio(10, 100),
        ritual: Real::from_ratio(15, 100),
        sacred_time: -Real::from_ratio(8, 100),
    }
}

/// Civ collapse: surviving founders attribute it to gods/punishment
/// → strong push toward pluralism + liturgy + eschatological
/// "end-times" framing.
pub fn push_for_civ_collapsed() -> Religion {
    Religion {
        theology: Real::from_ratio(25, 100),
        ritual: Real::from_ratio(30, 100),
        sacred_time: Real::from_ratio(40, 100),
    }
}

/// emission gate: re-emit `ReligionShifted` only when the
/// religion vector has drifted at least this far in L2 distance
/// from the last emitted snapshot. Lower than the new
/// cosmology threshold (0.50) because religion is supposed to be
/// faster-moving and we want every meaningful schism on the wire.
pub const RELIGION_EMIT_THRESHOLD: (i64, i64) = (20, 100);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_religion_has_zero_dogmatism() {
        assert_eq!(Religion::NEUTRAL.dogmatism(), Real::ZERO);
    }

    #[test]
    fn fully_aligned_religion_has_dogmatism_one() {
        let r = Religion {
            theology: Real::ONE,
            ritual: Real::ONE,
            sacred_time: Real::ONE,
        };
        let dog = r.dogmatism();
        assert!(dog > Real::from_ratio(99, 100));
        assert!(dog <= Real::ONE);
    }

    #[test]
    fn push_clamps_to_unit_range() {
        let mut r = Religion::NEUTRAL;
        let big = Religion {
            theology: Real::from_int(2),
            ritual: Real::ZERO,
            sacred_time: Real::ZERO,
        };
        r.push(&big, Real::from_int(10));
        assert_eq!(r.theology, Real::ONE);
    }

    #[test]
    fn refinement_rejected_pushes_toward_orthodoxy() {
        let mut r = Religion::NEUTRAL;
        r.push(&push_for_refinement_rejected(), Real::ONE);
        assert!(r.ritual > Real::ZERO, "rejection should harden ritual");
        assert!(r.theology > Real::ZERO, "rejection should pluralise");
        assert!(r.sacred_time < Real::ZERO, "rejection should cyclise");
    }

    #[test]
    fn collapse_pushes_eschatological() {
        let mut r = Religion::NEUTRAL;
        r.push(&push_for_civ_collapsed(), Real::ONE);
        assert!(r.sacred_time > Real::ZERO);
        assert!(r.ritual > Real::ZERO);
        assert!(r.theology > Real::ZERO);
    }

    #[test]
    fn founding_religion_diverges_for_different_civs() {
        // Same trait inputs, different civ_ids should land on
        // different religion vectors (jitter does its job).
        let mid = Real::from_ratio(50, 100);
        let r1 = founding_religion(1, mid, mid, mid);
        let r2 = founding_religion(2, mid, mid, mid);
        assert_ne!(r1, r2);
    }

    #[test]
    fn founding_religion_deterministic_per_civ_id() {
        let mid = Real::from_ratio(50, 100);
        let a = founding_religion(7, mid, mid, mid);
        let b = founding_religion(7, mid, mid, mid);
        assert_eq!(a, b);
    }

    #[test]
    fn high_charisma_founder_pushes_ritual_liturgical() {
        // Holding civ_id constant, a high-charisma founder should
        // produce more liturgical religion than a low-charisma
        // founder. Tested on a few civ_ids to exclude jitter
        // dominating the trait offset.
        for civ_id in [1u32, 5, 13, 27] {
            let high = founding_religion(civ_id, Real::ONE, Real::ZERO, Real::ZERO);
            let low = founding_religion(civ_id, Real::ZERO, Real::ZERO, Real::ZERO);
            assert!(
                high.ritual > low.ritual,
                "civ {civ_id}: high-charisma founder should land more liturgical",
            );
        }
    }

    #[test]
    fn high_doubt_founder_pushes_theology_monist() {
        for civ_id in [2u32, 6, 14, 28] {
            let high = founding_religion(civ_id, Real::ZERO, Real::ONE, Real::ZERO);
            let low = founding_religion(civ_id, Real::ZERO, Real::ZERO, Real::ZERO);
            assert!(
                high.theology < low.theology,
                "civ {civ_id}: high-doubt founder should land more monist",
            );
        }
    }
}
