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

/// Absolute-density lift applied to the per-unit carrying capacity so
/// a real-sized planet supports a realistic *several-billion* total
/// population, not the ~tens-of-millions the bare 50,000/unit baseline
/// produced. The point of "planet-scale realism" is that the hex grid
/// is only a sampling resolution of a much larger physical world: an
/// Earth-radius, fully-habitable, baseline-tech planet should host a
/// few billion carrying capacity so a typical civ (tens to hundreds of
/// millions) is a small fraction of the planet and never saturates the
/// whole map.
///
/// Calibration: at the reference grid the producer pool of a fully-
/// habitable world is `~n_cells × biosphere_density ≈ 1080 × 0.9 ≈ 970`
/// units; with `50,000 × 50 = 2,500,000` individuals per unit the
/// planet-wide total lands at `~2.4 billion` (median cognition) to
/// `~2.8 billion` (max cognition) — squarely in the low-single-digit
/// billions target. Every relative multiplier (habitability, seasonal,
/// tech, tool, producer share, area) still rides on top, so the
/// demographic-transition arc and ecosystem coupling are unchanged in
/// *shape* — only the absolute scale moved.
pub const PLANET_CAPACITY_DENSITY_LIFT: i64 = 50;

/// Planet surface-area scaling factor relative to Earth. Surface area
/// of a sphere is `4πr²`, so area ∝ radius²; Earth (radius 1.0) maps to
/// factor 1.0 and a 1.4-Earth-radius world to `1.4² = 1.96×`. The hex
/// grid samples the planet at a fixed resolution regardless of physical
/// size, so this factor restores the real planet's scale into every
/// carrying-capacity computation: a bigger planet has proportionally
/// more habitable surface and therefore proportionally more capacity.
/// Guarded against a non-positive radius (degenerate test planets) by
/// flooring at a tiny positive value so the factor never zeroes a civ's
/// capacity outright.
#[must_use]
pub fn planet_area_factor(radius: Real) -> Real {
    let r = radius.max(Real::percent(1));
    r * r
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
/// unit fuel-density). High-cognition species push the scale up;
/// low-cognition push it down. `cell_count` rescales per-cell so
/// total planet capacity is grid-resolution-invariant: per_unit ∝
/// REFERENCE / cell_count.
///
/// Earth-equivalent baseline (max-cog) at the reference grid
/// resolution is `~50,000 × PLANET_CAPACITY_DENSITY_LIFT` per unit —
/// the bare 50,000 anchor lifted into the billion-scale absolute total
/// a real-sized planet should host (see `PLANET_CAPACITY_DENSITY_LIFT`).
/// Biosphere is *not* multiplied here
/// — it's already captured by the per-cell `Substance::Fuel`
/// ceiling (`bio_fuel` in `world/init.rs`: Sparse 0.20, Lush 1.0,
/// HyperBio 3.0). Gravity is *not* multiplied either — native
/// species are adapted to their home gravity, so penalising them
/// against a 1g Earth anchor is Earth-centrism. The downstream
/// per-cell `cell_capacity` multiplication by `fuel` still gives
/// biosphere its effect (and `tech_multiplier × tool_multiplier`
/// gives tech its effect) without the prior double-count.
///
/// The absolute scale is set so a fully-habitable Earth-radius planet
/// at baseline tech hosts a few-billion total carrying capacity: a
/// typical civ (tens to hundreds of millions) is then a small fraction
/// of the planet and the map no longer shows one civ saturating
/// everything. The tech-tier multiplier stack (see
/// `tools::tool_capacity_multiplier` / `tech::effects`) still rides on
/// top so the paleolithic → modern density arc is preserved in shape;
/// only the absolute floor moved up. The ratio thresholds (food
/// security, migration pressure) are scale-invariant — both still
/// trigger at the same fractions of cap regardless of the absolute
/// number.
///
/// Total demographic span baseline → fully-teched is ~8,000× (see
/// `tech::effects::capacity_multiplier`), reproducing the real
/// paleolithic → modern density growth of ~1000–5000× plus headroom
/// for speculative future-age tools.
#[must_use]
pub fn carrying_capacity_per_unit(cognition: Real, cell_count: u32) -> Real {
    let cog = cognition.clamp01();
    // Widened from the prior 0.95-1.0 no-op band to 0.85-1.15 so
    // cognition is a real signal — low-cognition species pay a
    // small capacity tax, high-cognition species get a small bonus.
    // Still narrow enough that low-cog species aren't doomed; the
    // hypothesizer-cadence and stress-factor penalties carry most
    // of the cognition signal elsewhere.
    let cognition_factor = Real::percent(85) + Real::percent(30) * cog;
    let effective_cells = cell_count.max(1);
    let resolution_factor = Real::from_int(i64::from(REFERENCE_PLANET_CELL_COUNT))
        / Real::from_int(i64::from(effective_cells));
    // `PLANET_CAPACITY_DENSITY_LIFT` raises the absolute magnitude into
    // the billion-scale total a real-sized planet should host; the
    // relative cognition / resolution factors are unchanged so the
    // per-cell density ladder and grid-resolution invariance still hold.
    Real::from_int(50_000 * PLANET_CAPACITY_DENSITY_LIFT) * cognition_factor * resolution_factor
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
/// expansion is a *resource-poor* response.
///
/// Calibration: multiplier is `min(1.10, sqrt(tool_capacity_multiplier))`
/// (was 1.50). The prior aggressive multiplier pinned the threshold
/// to the 0.97 ceiling for any civ with ~1.3× tools — and once at
/// 0.97, surplus headroom collapsed to 3% × cap, drain rate fell
/// ~16× off the natural ~0.05 × overflow target, and migration
/// went glacial. The 1.10 cap keeps the design intent (denser
/// cores for high-tech) without starving the drain channel.
///
/// Output is clamped to ≤ 0.92 (was 0.97) so even tech-saturated
/// civs preserve 8 % cap of working overflow each tick.
/// Net: a high-tech civ tolerates 55-92 % fill before spillover
/// instead of the base 55-75 %.
#[must_use]
pub fn tech_augmented_migration_threshold(base: Real, tool_capacity_multiplier: Real) -> Real {
    let safe_mult = tool_capacity_multiplier.max(Real::percent(1));
    let raw_factor = sim_arith::transcendental::sqrt(safe_mult);
    let max_factor = Real::from_ratio(110, 100);
    let factor = if raw_factor > max_factor {
        max_factor
    } else {
        raw_factor
    };
    let augmented = base * factor;
    let ceiling = Real::percent(92);
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

/// `attempt_period_for_cognition` with the species'
/// `CognitionTopology` multiplier applied. DistributedRedundant
/// shrinks the period by 0.7 (parallel sensing); Acentric
/// stretches by 5.0 (very slow individual cognition);
/// Centralized and Collective stay at the cognition-only
/// baseline.
#[must_use]
pub fn attempt_period_for_cognition_and_topology(
    cognition: Real,
    topology: sim_species::CognitionTopology,
) -> u64 {
    let base = attempt_period_for_cognition(cognition);
    let scaled = Real::from_int(i64::try_from(base).unwrap_or(i64::MAX))
        * topology.attempt_period_multiplier();
    let raw: i64 = scaled.raw().to_num();
    let clamped = raw.max(5);
    u64::try_from(clamped).unwrap_or(base)
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

    /// Calibration regression test. Pins the per-unit carrying
    /// capacity to ±10% of the `50,000 × PLANET_CAPACITY_DENSITY_LIFT`
    /// base for the typical habitable seed (Sparse–Lush–Hyper
    /// biospheres at Earth gravity and median cognition). The density
    /// lift puts a fully-habitable Earth-radius planet at a low-single-
    /// digit-billion *total* carrying capacity so a typical civ is a
    /// small fraction of the planet; the cognition band (0.85–1.15) and
    /// grid-resolution invariance are unchanged. Sparse-world floor
    /// still stays within the same relative band so marginal seeds
    /// don't tip into `food_crisis` from substrate alone.
    #[test]
    fn carrying_capacity_envelope_is_calibrated() {
        let ref_cells = REFERENCE_PLANET_CELL_COUNT;
        let max_cog = carrying_capacity_per_unit(Real::ONE, ref_cells);
        let median_cog =
            carrying_capacity_per_unit(Real::from_ratio(5, 10), ref_cells);
        let no_cog = carrying_capacity_per_unit(Real::ZERO, ref_cells);
        // Anchor = 50,000 × 50 = 2,500,000 (median cognition). Max-cog
        // (cognition_factor = 1.15) → 2,875,000.
        assert!(
            max_cog >= Real::from_int(2_800_000) && max_cog <= Real::from_int(2_950_000)
        );
        // Median-cog sits exactly on the 2,500,000 anchor
        // (cognition_factor = 1.0).
        assert!(
            median_cog >= Real::from_int(2_450_000) && median_cog <= Real::from_int(2_550_000)
        );
        // Zero-cog is the floor (cognition_factor = 0.85 → 2,125,000).
        // Still viable — a low-cog species can found cities, just denser
        // tools/tech are needed to match a high-cog peer.
        assert!(
            no_cog >= Real::from_int(2_080_000) && no_cog <= Real::from_int(2_170_000)
        );
    }

    #[test]
    fn carrying_capacity_scales_inverse_with_cell_count() {
        // Halving the grid resolution should double per-cell cap so
        // total planet-wide capacity stays invariant: a coarse grid
        // models the same planet with each cell standing for more
        // physical area.
        let ref_cells = REFERENCE_PLANET_CELL_COUNT;
        let half_cells = ref_cells / 2;
        let base = carrying_capacity_per_unit(Real::ONE, ref_cells);
        let coarse = carrying_capacity_per_unit(Real::ONE, half_cells);
        let ratio = coarse / base;
        assert!(ratio > Real::from_ratio(195, 100));
        assert!(ratio < Real::from_ratio(205, 100));
    }

    /// tech_augmented_migration_threshold scales the base threshold
    /// by `min(1.10, sqrt(capacity_mult))` and clamps the output to
    /// `0.92`. Vanilla civs (capacity_mult = 1.0) pass through.
    #[test]
    fn tech_augmented_migration_threshold_envelopes() {
        let base = Real::percent(70);
        // No tools → unchanged.
        let t0 = tech_augmented_migration_threshold(base, Real::ONE);
        let drift0 = if t0 > base { t0 - base } else { base - t0 };
        assert!(drift0 < Real::from_ratio(1, 1000));

        // 4× capacity → sqrt = 2.0, capped at 1.10 → 0.70 * 1.10
        // = 0.77 (well under the 0.92 ceiling).
        let t4 = tech_augmented_migration_threshold(base, Real::from_int(4));
        let expected4 = Real::from_ratio(77, 100);
        let drift4 = if t4 > expected4 {
            t4 - expected4
        } else {
            expected4 - t4
        };
        assert!(
            drift4 < Real::percent(1),
            "4× capacity → 0.70 × 1.10 = 0.77; got {t4:?}"
        );

        // High base with high tech → clamp to 0.92 ceiling. Base
        // 0.85 × 1.10 = 0.935 → clamped to 0.92.
        let high_base = Real::percent(85);
        let t_high = tech_augmented_migration_threshold(high_base, Real::from_int(4));
        let ceiling = Real::percent(92);
        let drift_ceil = if t_high > ceiling {
            t_high - ceiling
        } else {
            ceiling - t_high
        };
        assert!(
            drift_ceil < Real::percent(1),
            "0.85 × 1.10 = 0.935 clamps to 0.92; got {t_high:?}"
        );

        // Sub-1× tools → factor < 1, threshold drops below base.
        let low = tech_augmented_migration_threshold(base, Real::percent(50));
        assert!(low < base, "tools < 1× should pull threshold below base");
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
