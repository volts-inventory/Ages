//! Shared per-(habitat, glyph) suitability multiplier. The single
//! source of truth for "how good is this terrain glyph for this
//! species habitat?" — consumed by both `sim_civ::capacity`
//! (per-cell carrying capacity) and `sim_core::territory` (cell
//! selection scoring).
//!
//! Earlier the two subsystems each kept their own table. They drifted:
//! Subterranean peaks scored 1.30 in territory (the *highest* of any
//! glyph, so peaks ranked top for founding) but only 0.10 in capacity
//! (lower than coast's 1.20, so a peak-founded Subterranean civ
//! immediately starved). The mismatch hid a real bug for any
//! species whose territory-preferred terrain disagreed with its
//! capacity-rewarded terrain. Consolidating here keeps the two
//! consumers honest: change one number, both subsystems move.
//!
//! All multipliers are dimensionless `Real` (Q32.32) values — no `f64`
//! enters the sim path.

use crate::Habitat;
use sim_arith::Real;

/// `(habitat, glyph) -> multiplier`. Range `[0, 1.3]`. Both
/// territory-score and per-cell capacity multiply this against
/// other factors (fuel, tech, seasonal) so the absolute scale
/// matters less than the *relative* ordering across glyphs for a
/// given habitat. Native land/water cells score near or above
/// 1.0; cross-habitat marginal cells score below 0.1.
///
/// Glyph alphabet matches `sim_world::habitability_multiplier`:
/// `≈` deep ocean, `~` shallow, `░` coast, `▒` inland, `·` plain,
/// `△` hill, `▲` peak, `≡` gas band.
///
/// Subterranean note: peaks/inland are *deepest dry stone* for
/// excavated habitats, so they score at or above 1.0 in both
/// subsystems — this is the bug the consolidation fixes.
#[must_use]
pub fn habitat_glyph_multiplier(habitat: Habitat, glyph: char) -> Real {
    match habitat {
        // Terrestrial / Airborne: standard land bias. Coast bonus,
        // peaks marginal, water cells nearly uninhabitable (but
        // shallow sea still scores a tiny edge for fishing).
        Habitat::Terrestrial | Habitat::Airborne => match glyph {
            '\u{2248}' => Real::ZERO,         // ≈ deep ocean
            '\u{2261}' => Real::ZERO,         // ≡ gas band
            '~' => Real::percent(5),          // ~ shallow sea — marginal
            '\u{2591}' => Real::percent(120), // ░ coast — richest
            '\u{2592}' => Real::percent(90),  // ▒ inland
            '\u{25B3}' => Real::percent(60),  // △ hill
            '\u{25B2}' => Real::percent(10),  // ▲ peak
            _ => Real::ONE,                   // · plain / unrecognised
        },
        Habitat::Aquatic => match glyph {
            '\u{2248}' => Real::ONE,          // ≈ deep ocean — native
            '~' => Real::percent(80),         // ~ shallow sea
            '\u{2591}' => Real::percent(120), // ░ coast — richest (tidal feeding)
            '\u{2261}' => Real::ZERO,         // ≡ gas band
            // Land glyphs: marginal — aquatic civs can't really
            // live here, but the multiplier stays non-zero so any
            // accidentally-claimed land cell doesn't divide by zero
            // in downstream formulas.
            _ => Real::percent(5),
        },
        Habitat::Amphibious => match glyph {
            '\u{2591}' => Real::percent(120),     // ░ coast — best of both
            '\u{2592}' | '\u{00B7}' => Real::ONE, // ▒ inland / · plain
            '~' => Real::percent(80),             // ~ shallow
            '\u{2248}' => Real::percent(80),      // ≈ deep
            '\u{25B3}' => Real::percent(60),      // △ hill
            '\u{25B2}' => Real::percent(10),      // ▲ peak
            '\u{2261}' => Real::ZERO,             // ≡ gas band
            _ => Real::ZERO,
        },
        // Subterranean: inverts the surface bias — peaks +
        // inland have the deepest excavable substrate; coast
        // is wet / less stable; water = uninhabitable. Peak
        // scores 1.30 (the highest score of any habitat-glyph
        // pair), inland 1.00, coast 0.60. Earlier the capacity
        // table contradicted this with peak = 0.10; consolidating
        // here means Subterranean civs founded on peaks actually
        // get the capacity bonus they were ranked into.
        Habitat::Subterranean => match glyph {
            '\u{25B2}' => Real::percent(130),     // ▲ peak — deepest dry stone
            '\u{25B3}' => Real::ONE,              // △ hill
            '\u{2592}' | '\u{00B7}' => Real::ONE, // ▒ inland / · plain
            '\u{2591}' => Real::percent(60),      // ░ coast — wet, less stable
            // Water and gas — non-native; tiny non-zero floor
            // so capacity arithmetic stays well-defined.
            '~' | '\u{2248}' | '\u{2261}' => Real::ZERO,
            _ => Real::ZERO,
        },
        // Endolithic: substrate-bound life. Same shape as
        // subterranean — rock pore-space dwelling — but peaks
        // top out at 1.20 (slightly less than subterranean since
        // endoliths don't excavate, they squat in existing pore
        // space).
        Habitat::Endolithic => match glyph {
            '\u{25B2}' => Real::percent(120),     // ▲ peak
            '\u{25B3}' => Real::ONE,              // △ hill
            '\u{2592}' | '\u{00B7}' => Real::ONE, // ▒ inland / · plain
            '\u{2591}' => Real::percent(40),      // ░ coast — marginal
            '~' | '\u{2248}' | '\u{2261}' => Real::ZERO,
            _ => Real::ZERO,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subterranean_peak_is_highest_native() {
        // The bug we're guarding against: Subterranean peak must
        // out-rank Subterranean inland in both directions.
        let peak = habitat_glyph_multiplier(Habitat::Subterranean, '\u{25B2}');
        let inland = habitat_glyph_multiplier(Habitat::Subterranean, '\u{2592}');
        assert!(peak >= Real::ONE);
        assert!(peak > inland);
        assert!(inland >= Real::percent(90));
    }

    #[test]
    fn terrestrial_coast_above_inland() {
        let coast = habitat_glyph_multiplier(Habitat::Terrestrial, '\u{2591}');
        let inland = habitat_glyph_multiplier(Habitat::Terrestrial, '\u{2592}');
        assert!(coast > inland);
    }

    #[test]
    fn aquatic_deep_above_land() {
        let deep = habitat_glyph_multiplier(Habitat::Aquatic, '\u{2248}');
        let inland = habitat_glyph_multiplier(Habitat::Aquatic, '\u{2592}');
        assert!(deep > inland);
    }

    #[test]
    fn subterranean_peak_consistent_across_subsystems() {
        // The exact assertion from the bug report: peak score must
        // pass both subsystems' thresholds. `score_for_habitat`
        // (territory) and `species_habitability` (capacity) both
        // delegate to `habitat_glyph_multiplier`, so a single check
        // here is the single source of truth for both.
        let peak_score = habitat_glyph_multiplier(Habitat::Subterranean, '\u{25B2}');
        assert!(
            peak_score >= Real::ONE,
            "Subterranean peak must score >=1.0 for territory ranking: got {peak_score:?}"
        );
        assert!(
            peak_score >= Real::percent(50),
            "Subterranean peak must score >=0.5 for capacity multiplier: got {peak_score:?}"
        );
        // Inland: the next-best Subterranean cell, must also clear
        // the 0.9 floor so inland founding civs don't immediately
        // starve.
        let inland = habitat_glyph_multiplier(Habitat::Subterranean, '\u{2592}');
        assert!(
            inland >= Real::percent(90),
            "Subterranean inland must score >=0.9: got {inland:?}"
        );
    }
}
