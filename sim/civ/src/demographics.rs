//! Substrate-derived demographic helpers.
//!
//! These helpers map the planet's biosphere class, gravity, and the
//! species' cognition / sociality traits into the demographic
//! constants the [`crate::Civ`] struct caches at founding (per-fuel
//! carrying capacity, founding-floor population, migration pressure
//! threshold, hypothesizer attempt period). Pulled out of `lib.rs`
//! so each helper sits with its calibration commentary rather than
//! getting lost in the `Civ` impl block.

use sim_arith::{Pop, Real};
use sim_population::PopulationDynamics;
use sim_world::{BiosphereClass, Planet};

/// Map a planet's biosphere class to a birth-rate multiplier.
/// Food abundance drives reproductive rate; `HyperBiodiverse` worlds
/// have ~1.7× the demographic momentum of `None` ones, with `Lush`
/// as the calibration baseline (1.0×). The factors stay close to
/// 1.0 so even `Sparse` / `None` worlds remain demographically
/// viable — otherwise civs on those planets crash through the
/// founding-pop floor before any chain forms.
///
/// The planet-aware factor also receives `axial_tilt_factor` (0–1) and
/// `luminosity_factor` (clamped 0.7–1.3) from the planet so high-
/// tilt worlds (harsher seasonal birth-rate pressure) and extreme-
/// luminosity worlds (chilly / scorched orbits) bias the birth
/// rate further. Pure biosphere lookup is preserved for callers
/// that don't have planet context (legacy tests).
#[must_use]
pub fn biosphere_birth_factor(biosphere: BiosphereClass) -> Real {
    match biosphere {
        BiosphereClass::None => Real::from_ratio(7, 10),
        BiosphereClass::Sparse => Real::from_ratio(9, 10),
        BiosphereClass::Lush => Real::ONE,
        BiosphereClass::HyperBiodiverse => Real::from_ratio(12, 10),
    }
}

/// Planet-derived birth-factor adjustment. Multiplies the
/// base biosphere factor by `(1 - 0.1 * tilt/90) *
/// luminosity_clamped`, where luminosity clamps to [0.85, 1.15]
/// of Earth's 1361 W/m². Mild adjustment — extremes shift the
/// birth rate by ~±15%, not the ~±35% a wider band would. Big
/// shifts destabilise marginal seeds; the substrate signal still
/// makes high-tilt worlds slightly harsher.
#[must_use]
pub fn biosphere_birth_factor_for_planet(planet: &Planet) -> Real {
    let base = biosphere_birth_factor(planet.biosphere);
    let tilt_norm = (planet.axial_tilt_deg / Real::from_int(90)).clamp01();
    let tilt_factor = Real::ONE - Real::from_ratio(1, 10) * tilt_norm;
    let earth_lum = Real::from_int(1361);
    let lum_norm = (planet.stellar_luminosity / earth_lum)
        .max(Real::percent(85))
        .min(Real::percent(115));
    base * tilt_factor * lum_norm
}

/// Substrate-derived founding-population floor. Lush worlds
/// need fewer founders to survive random shocks (~75); sparse worlds
/// need more (~200). Low-cognition species need more bodies to
/// maintain knowledge between collapses. Replaces the flat
/// `FOUNDING_MIN_POPULATION = 100` placeholder.
#[must_use]
pub fn founding_min_population(biosphere: BiosphereClass, cognition: Real) -> Pop {
    let biosphere_pressure = match biosphere {
        BiosphereClass::None => Real::from_ratio(15, 10),
        BiosphereClass::Sparse => Real::ONE,
        BiosphereClass::Lush => Real::from_ratio(5, 10),
        BiosphereClass::HyperBiodiverse => Real::percent(25),
    };
    let cog = cognition.clamp01();
    let cognition_penalty = Real::ONE - cog;
    Pop::from_int(50)
        + Pop::from_int(35) * biosphere_pressure
        + Pop::from_int(15) * cognition_penalty
}

/// Reference grid resolution (cells per planet) that
/// `carrying_capacity_per_unit` is calibrated against. The default
/// run grid is 36×30 = 1080 cells; at that resolution the scale
/// returns the historical ~50,000 / unit baseline. Smaller dev
/// grids (12×8 = 96) get a proportionally larger per-cell cap so a
/// single cell represents a larger slice of the planet; larger
/// custom grids get a proportionally smaller per-cell cap. Net
/// effect: total *planet-wide* carrying capacity stays invariant
/// to grid resolution — a 96-cell run hosts the same humanity as a
/// 1080-cell run of the same planet, just at coarser spatial
/// resolution.
pub const REFERENCE_PLANET_CELL_COUNT: u32 = 1080;

/// Substrate-derived carrying-capacity scale (individuals per
/// unit fuel-density). Lush biospheres + high-cognition species push
/// the scale up; sparse / low-cognition push it down. High-gravity
/// worlds cost more per individual; low-gravity worlds save energy.
/// `cell_count` rescales the per-cell base so total planet capacity
/// is grid-resolution-invariant: per_unit ∝ REFERENCE / cell_count.
///
/// Earth-equivalent baseline (Lush, g≈9.81, cog≈1) at the reference
/// grid resolution is ~50,000/unit. The 20× lift from the prior
/// 2,500 puts paleolithic civs at ~50k people per cell (city-state
/// density) so the tech-tier multiplier stack (see
/// `tools::tool_capacity_multiplier` / `tech::effects`) can carry
/// agricultural civs to ~M/cell, industrial civs to ~10M/cell, and
/// modern/future-age civs to ~hundreds of M/cell without needing
/// any further global retune. The ratio thresholds (food security
/// `demand / capacity`, migration pressure `pop / cell_capacity`)
/// are scale-invariant — both still trigger at the same fractions
/// of cap regardless of the absolute number.
///
/// Total demographic span baseline → fully-teched is ~8,000× (see
/// `tech::effects::capacity_multiplier`), reproducing the real
/// paleolithic → modern density growth of ~1000–5000× plus headroom
/// for speculative future-age tools.
#[must_use]
pub fn carrying_capacity_per_unit(
    biosphere: BiosphereClass,
    gravity: Real,
    cognition: Real,
    cell_count: u32,
) -> Real {
    let biosphere_factor = match biosphere {
        BiosphereClass::None => Real::from_ratio(7, 10),
        BiosphereClass::Sparse => Real::percent(95),
        BiosphereClass::Lush => Real::ONE,
        BiosphereClass::HyperBiodiverse => Real::percent(115),
    };
    let earth_g = Real::percent(981);
    let g_diff = if gravity > earth_g {
        gravity - earth_g
    } else {
        earth_g - gravity
    };
    let g_factor = (Real::ONE - Real::percent(5) * g_diff / earth_g).max(Real::from_ratio(5, 10));
    let cog = cognition.clamp01();
    // Cognition factor narrows further (0.95–1.0) so low-cognition
    // species don't compound a capacity hit on top of the existing
    // attempt-period and stress-factor cognition penalties.
    let cognition_factor = Real::percent(95) + Real::percent(5) * cog;
    // Grid-resolution scaling: at the reference grid the factor is
    // 1.0; halving the grid doubles per-cell cap (each cell stands
    // for twice the planetary area), doubling it halves.
    let effective_cells = cell_count.max(1);
    let resolution_factor =
        Real::from_int(i64::from(REFERENCE_PLANET_CELL_COUNT)) / Real::from_int(i64::from(effective_cells));
    Real::from_int(50_000) * biosphere_factor * g_factor * cognition_factor * resolution_factor
}

/// Substrate-derived migration pressure threshold. Solitary
/// species flee crowding earlier (0.55); cooperative species
/// tolerate it longer (0.75). Lowered from the prior 0.75–0.95 band
/// to keep claim activity going at the new 5× cell cap — at the old
/// threshold a civ would densify to ~tens of thousands per cell
/// before any spillover, never claiming neighbouring land within a
/// human-recognisable timeframe. The new band gives a dense core
/// (cells at 55–75% of cap) plus continued frontier expansion.
#[must_use]
pub fn migration_pressure_threshold(sociality: Real) -> Real {
    let s = sociality.clamp01();
    Real::percent(55) + Real::from_ratio(2, 10) * s
}

/// Tech-augmented migration threshold. Tech surplus (urban
/// planning, irrigation, sanitation) should make a civ tolerate
/// denser cores before pushing migrants outward — frontier
/// expansion is a *resource-poor* response. Multiplier is
/// `min(1.5, sqrt(tool_capacity_multiplier))` so:
///   - vanilla civ (tech = 1.0): 1.0× → threshold unchanged
///   - 4× capacity stack: sqrt = 2.0 → capped at 1.5×
///   - higher tech: still capped at 1.5×
/// Output is then clamped to ≤ 0.97 so the threshold can't push
/// above the saturation cliff (a 0.99 threshold would freeze
/// migration entirely). Net: a high-tech civ tolerates 55-97%
/// fill before spillover instead of the base 55-75%.
#[must_use]
pub fn tech_augmented_migration_threshold(base: Real, tool_capacity_multiplier: Real) -> Real {
    let safe_mult = tool_capacity_multiplier.max(Real::percent(1));
    let raw_factor = sim_arith::transcendental::sqrt(safe_mult);
    let max_factor = Real::from_ratio(15, 10);
    let factor = if raw_factor > max_factor {
        max_factor
    } else {
        raw_factor
    };
    let augmented = base * factor;
    let ceiling = Real::percent(97);
    if augmented > ceiling {
        ceiling
    } else {
        augmented
    }
}

/// Derive figure-hypothesizer `attempt_period` from
/// `species.cognition`. High-cognition species cycle through
/// hypothesis attempts faster (~3× more often than low-cognition);
/// low-cognition species are slower. Returns the Aqueous-baseline
/// period; slow-substrate worlds further stretch this via
/// [`scale_attempt_period_for_metabolism`], applied by
/// [`crate::Civ::configure_substrate`] once the planet is known.
#[must_use]
pub fn attempt_period_for_cognition(cognition: Real) -> u64 {
    let cog = cognition.clamp01();
    let factor = Real::from_ratio(15, 10) - cog;
    let period_real = Real::from_int(20) * factor;
    let raw: i64 = period_real.raw().to_num();
    let clamped = raw.max(5);
    u64::try_from(clamped).unwrap_or(20)
}

/// Scale a baseline `attempt_period` by the planet's metabolism so
/// slow-substrate worlds stretch the hypothesis-attempt cadence to
/// match their stretched biological time. Inverse relationship:
/// metabolism = 0.2 (Silicate) gives a 5× longer period.
#[must_use]
pub fn scale_attempt_period_for_metabolism(period: u64, metabolism: Real) -> u64 {
    let m = metabolism.max(Real::percent(1));
    let raw: i64 = (Real::from_int(i64::try_from(period).unwrap_or(i64::MAX)) / m)
        .raw()
        .to_num();
    let clamped = raw.max(5);
    u64::try_from(clamped).unwrap_or(period)
}

/// Stretch a baseline streak / cooldown / window measured in ticks by
/// the planet's substrate metabolism so that "75 baseline-years of
/// civil-war pressure" lasts the same number of substrate-internal
/// generations on every substrate. Slow metabolism (Silicate ≈ 0.2)
/// gives a 5× longer window in absolute ticks. Returns the baseline
/// when metabolism is Aqueous (1.0). Guarded against zero metabolism.
#[must_use]
pub fn streak_ticks_for_metabolism(base: u64, metabolism: Real) -> u64 {
    let m = metabolism.max(Real::percent(1));
    let raw: i64 = (Real::from_int(i64::try_from(base).unwrap_or(i64::MAX)) / m)
        .raw()
        .to_num();
    u64::try_from(raw.max(1)).unwrap_or(base)
}

/// Derive `PopulationDynamics` for a given species + planet.
/// The species's `PopulationBiology` (`clutch_size`, bracket
/// fractions, per-bracket survivals) drives the per-tick rates;
/// the planet's biosphere/tilt/luminosity then multiplies the
/// resulting birth rate so a sparse / high-tilt / dim-luminosity
/// world reproduces less successfully than a lush / low-tilt /
/// Earth-luminosity one. The planet's metabolic substrate further
/// multiplies the birth rate — slow chemistries unfold over
/// geological time, so a silicate population grows ~5× slower than
/// an aqueous one. Per-bracket survival rates are unaffected by the
/// planet — they're intrinsic to the species's biology.
#[must_use]
pub fn dynamics_for(species: &sim_species::Species, planet: &Planet) -> PopulationDynamics {
    let mut d = PopulationDynamics::for_species(
        &species.biology,
        species.lifespan_years,
        species.cognition,
        species.sociality,
    );
    let bio_factor = biosphere_birth_factor_for_planet(planet);
    let metabolism = planet.metabolic_substrate.metabolism();
    d.birth_rate = d.birth_rate * bio_factor * metabolism;
    d
}

/// Per-civ population dynamics. Same formula as
/// `dynamics_for` but reads `civ.effective_*` traits so each civ's
/// per-generation drift in cognition / sociality / lifespan is
/// reflected in its birth/death rates and stress factor. Used by
/// sim/core's three founding sites (inaugural emergent,
/// refound-from-stateless, breakaway) once the civ's drift has been
/// inherited from its parent (or zero-defaulted for inaugurals).
#[must_use]
pub fn dynamics_for_civ(
    civ: &crate::Civ,
    species: &sim_species::Species,
    planet: &Planet,
) -> PopulationDynamics {
    let mut d = PopulationDynamics::for_species(
        &species.biology,
        civ.effective_lifespan_years(species),
        civ.effective_cognition(species),
        civ.effective_sociality(species),
    );
    let bio_factor = biosphere_birth_factor_for_planet(planet);
    let metabolism = planet.metabolic_substrate.metabolism();
    d.birth_rate = d.birth_rate * bio_factor * metabolism;
    // Per-bracket tech mortality reduction from currently-unlocked
    // tools — folded in here (rather than at step time) so a single
    // re-derivation each tick captures the full tech state, including
    // any newly-unlocked sanitation / medicine.
    d.mortality_reduction = civ.tool_mortality_reduction_per_bracket();
    // Birth-rate multiplier from currently-unlocked nutrition +
    // medicine tools; mirrors how mortality_reduction is folded.
    d.birth_rate_multiplier = Real::ONE + civ.tool_fertility_bonus();
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Calibration regression test. Pins the per-fuel-unit
    /// carrying capacity to ±10% of the 50,000 base for the typical
    /// habitable seed (Sparse–Lush–Hyper biospheres at Earth gravity
    /// and median cognition). The 20× lift over the prior 2,500 base
    /// underwrites the billion-scale demographic-transition arc — a
    /// fully-teched cell can now hold ~hundreds of millions, and
    /// civs reach planetary-scale populations as the tech tree fills.
    /// Sparse-world floor still stays within 10% of Earth-equivalent
    /// so marginal seeds don't tip into `food_crisis` from substrate
    /// alone.
    #[test]
    fn carrying_capacity_envelope_is_calibrated() {
        let earth_g = Real::percent(981);
        let median_cog = Real::from_ratio(5, 10);
        // Use the reference grid resolution so the per-cell base
        // recovers the historical ~50,000 anchor; smaller / larger
        // grids legitimately rescale per-cell from that point.
        let ref_cells = REFERENCE_PLANET_CELL_COUNT;
        let lush = carrying_capacity_per_unit(BiosphereClass::Lush, earth_g, Real::ONE, ref_cells);
        let sparse =
            carrying_capacity_per_unit(BiosphereClass::Sparse, earth_g, median_cog, ref_cells);
        let hyper =
            carrying_capacity_per_unit(BiosphereClass::HyperBiodiverse, earth_g, Real::ONE, ref_cells);
        // Earth-equivalent (Lush + Earth-g + max-cog) recovers ~50,000.
        assert!(lush >= Real::from_int(45_000) && lush <= Real::from_int(55_000));
        // Sparse + median-cog stays within 10% of Earth-equivalent
        // so marginal habitability holds.
        assert!(sparse >= Real::from_int(45_000));
        // Hyper bonus is real but bounded — no surprise 2× scaling.
        assert!(hyper >= Real::from_int(50_000) && hyper <= Real::from_int(65_000));
    }

    #[test]
    fn carrying_capacity_scales_inverse_with_cell_count() {
        // Halving the grid resolution should double per-cell cap so
        // total planet-wide capacity stays invariant: a coarse grid
        // models the same planet with each cell standing for more
        // physical area.
        let earth_g = Real::percent(981);
        let ref_cells = REFERENCE_PLANET_CELL_COUNT;
        let half_cells = ref_cells / 2;
        let base = carrying_capacity_per_unit(BiosphereClass::Lush, earth_g, Real::ONE, ref_cells);
        let coarse =
            carrying_capacity_per_unit(BiosphereClass::Lush, earth_g, Real::ONE, half_cells);
        let ratio = coarse / base;
        // Within ~1% of 2× (rounding from Q32.32 division on small
        // ints is tight here).
        assert!(ratio > Real::from_ratio(195, 100));
        assert!(ratio < Real::from_ratio(205, 100));
    }

    /// tech_augmented_migration_threshold scales the base threshold
    /// by `min(1.5, sqrt(capacity_mult))` and clamps the output to
    /// `0.97`. Vanilla civs (capacity_mult = 1.0) pass through.
    #[test]
    fn tech_augmented_migration_threshold_envelopes() {
        let base = Real::percent(70);
        // No tools → unchanged.
        let t0 = tech_augmented_migration_threshold(base, Real::ONE);
        let drift0 = if t0 > base { t0 - base } else { base - t0 };
        assert!(drift0 < Real::from_ratio(1, 1000));

        // 4× capacity → sqrt = 2.0, capped at 1.5 → 0.70 * 1.5 = 1.05
        // → clamped to 0.97 ceiling.
        let t4 = tech_augmented_migration_threshold(base, Real::from_int(4));
        let ceiling = Real::percent(97);
        let drift_ceil = if t4 > ceiling {
            t4 - ceiling
        } else {
            ceiling - t4
        };
        assert!(
            drift_ceil < Real::percent(1),
            "4× capacity should clamp to 0.97; got {t4:?}"
        );

        // 2.25× capacity → sqrt = 1.5 (boundary) → 0.70 * 1.5 = 1.05
        // → still clamped.
        let t225 = tech_augmented_migration_threshold(base, Real::percent(225));
        let drift_ceil2 = if t225 > ceiling {
            t225 - ceiling
        } else {
            ceiling - t225
        };
        assert!(drift_ceil2 < Real::percent(1));

        // Lower base with same 4× tech → factor 1.5 → 0.55 * 1.5 =
        // 0.825 (under the 0.97 ceiling).
        let low_base = Real::percent(55);
        let t_low = tech_augmented_migration_threshold(low_base, Real::from_int(4));
        let expected = Real::from_ratio(825, 1000);
        let drift = if t_low > expected {
            t_low - expected
        } else {
            expected - t_low
        };
        assert!(
            drift < Real::percent(1),
            "0.55 × 1.5 = 0.825; got {t_low:?}"
        );
    }

    /// Calibration regression test for the founding floor.
    /// Pins the floor near 100 (the prior placeholder) for the
    /// typical habitable seed so substrate variation never doubles
    /// the floor and chokes refound chains.
    #[test]
    fn founding_floor_envelope_is_calibrated() {
        let lush = founding_min_population(BiosphereClass::Lush, Real::ONE);
        let sparse = founding_min_population(BiosphereClass::Sparse, Real::from_ratio(5, 10));
        let none = founding_min_population(BiosphereClass::None, Real::ZERO);
        // Lush + max-cog: ~67 (was 100). Lower is easier — fine.
        assert!(lush <= Pop::from_int(80));
        // Sparse + median-cog: ~92 (close to old 100).
        assert!(sparse >= Pop::from_int(80) && sparse <= Pop::from_int(110));
        // None + zero-cog: ~120 (notably higher — these worlds
        // shouldn't host civs anyway).
        assert!(none >= Pop::from_int(115));
    }
}
