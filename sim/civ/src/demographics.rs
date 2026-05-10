//! Substrate-derived demographic helpers.
//!
//! These helpers map the planet's biosphere class, gravity, and the
//! species' cognition / sociality traits into the demographic
//! constants the [`crate::Civ`] struct caches at founding (per-fuel
//! carrying capacity, founding-floor population, migration pressure
//! threshold, hypothesizer attempt period). Pulled out of `lib.rs`
//! so each helper sits with its calibration commentary rather than
//! getting lost in the `Civ` impl block.

use sim_arith::Real;
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
    let tilt_norm = (planet.axial_tilt_deg / Real::from_int(90))
        .max(Real::ZERO)
        .min(Real::ONE);
    let tilt_factor = Real::ONE - Real::from_ratio(1, 10) * tilt_norm;
    let earth_lum = Real::from_int(1361);
    let lum_norm = (planet.stellar_luminosity / earth_lum)
        .max(Real::from_ratio(85, 100))
        .min(Real::from_ratio(115, 100));
    base * tilt_factor * lum_norm
}

/// Substrate-derived founding-population floor. Lush worlds
/// need fewer founders to survive random shocks (~75); sparse worlds
/// need more (~200). Low-cognition species need more bodies to
/// maintain knowledge between collapses. Replaces the flat
/// `FOUNDING_MIN_POPULATION = 100` placeholder.
#[must_use]
pub fn founding_min_population(biosphere: BiosphereClass, cognition: Real) -> Real {
    let biosphere_pressure = match biosphere {
        BiosphereClass::None => Real::from_ratio(15, 10),
        BiosphereClass::Sparse => Real::ONE,
        BiosphereClass::Lush => Real::from_ratio(5, 10),
        BiosphereClass::HyperBiodiverse => Real::from_ratio(25, 100),
    };
    let cog = cognition.max(Real::ZERO).min(Real::ONE);
    let cognition_penalty = Real::ONE - cog;
    Real::from_int(50)
        + biosphere_pressure * Real::from_int(35)
        + cognition_penalty * Real::from_int(15)
}

/// Substrate-derived carrying-capacity scale (individuals per
/// unit fuel-density). Lush biospheres + high-cognition species push
/// the scale up; sparse / low-cognition push it down. High-gravity
/// worlds cost more per individual; low-gravity worlds save energy.
/// Earth-equivalent (Lush, g≈9.81, cog≈1) recovers ~500/unit. Each
/// 36×30 grid cell represents a continent-scale region (~470,000
/// km² on an Earth-sized world), so a per-cell ceiling on the order
/// of hundreds rather than tens reads as "regional population" not
/// "village headcount" while keeping the food-security ratio
/// (`demand / capacity`) and migration-pressure ratio
/// (`pop / cell_capacity`) scale-invariant — both still trigger at
/// the same fractional thresholds. Sparse worlds dip only ~5% below
/// Earth so marginal habitability holds; the substrate signal stays
/// meaningful at this width without destabilising habitability.
#[must_use]
pub fn carrying_capacity_per_unit(
    biosphere: BiosphereClass,
    gravity: Real,
    cognition: Real,
) -> Real {
    let biosphere_factor = match biosphere {
        BiosphereClass::None => Real::from_ratio(7, 10),
        BiosphereClass::Sparse => Real::from_ratio(95, 100),
        BiosphereClass::Lush => Real::ONE,
        BiosphereClass::HyperBiodiverse => Real::from_ratio(115, 100),
    };
    let earth_g = Real::from_ratio(981, 100);
    let g_diff = if gravity > earth_g {
        gravity - earth_g
    } else {
        earth_g - gravity
    };
    let g_factor =
        (Real::ONE - Real::from_ratio(5, 100) * g_diff / earth_g).max(Real::from_ratio(5, 10));
    let cog = cognition.max(Real::ZERO).min(Real::ONE);
    // Cognition factor narrows further (0.95–1.0) so low-cognition
    // species don't compound a capacity hit on top of the existing
    // attempt-period and stress-factor cognition penalties.
    let cognition_factor = Real::from_ratio(95, 100) + Real::from_ratio(5, 100) * cog;
    Real::from_int(500) * biosphere_factor * g_factor * cognition_factor
}

/// Substrate-derived migration pressure threshold. Solitary
/// species flee crowding earlier (0.75); cooperative species
/// tolerate it longer (0.95). Replaces flat 0.85. Range tightened
/// around the prior calibration so the substrate signal is real
/// without destabilising marginal civs.
#[must_use]
pub fn migration_pressure_threshold(sociality: Real) -> Real {
    let s = sociality.max(Real::ZERO).min(Real::ONE);
    Real::from_ratio(75, 100) + Real::from_ratio(2, 10) * s
}

/// Derive figure-hypothesizer `attempt_period` from
/// `species.cognition`. High-cognition species cycle through
/// hypothesis attempts faster (~3× more often than low-cognition);
/// low-cognition species are slower.
#[must_use]
pub fn attempt_period_for_cognition(cognition: Real) -> u64 {
    let cog = cognition.max(Real::ZERO).min(Real::ONE);
    let factor = Real::from_ratio(15, 10) - cog;
    let period_real = Real::from_int(20) * factor;
    let raw: i64 = period_real.raw().to_num();
    let clamped = raw.max(5);
    u64::try_from(clamped).unwrap_or(20)
}

/// Derive `PopulationDynamics` for a given species + planet.
/// The species's `PopulationBiology` (`clutch_size`, bracket
/// fractions, per-bracket survivals) drives the per-tick rates;
/// the planet's biosphere/tilt/luminosity then multiplies the
/// resulting birth rate so a sparse / high-tilt / dim-luminosity
/// world reproduces less successfully than a lush / low-tilt /
/// Earth-luminosity one. Per-bracket survival rates are unaffected
/// by the planet — they're intrinsic to the species's biology.
#[must_use]
pub fn dynamics_for(species: &sim_species::Species, planet: &Planet) -> PopulationDynamics {
    let mut d = PopulationDynamics::for_species(
        &species.biology,
        species.lifespan_years,
        species.cognition,
        species.sociality,
    );
    let bio_factor = biosphere_birth_factor_for_planet(planet);
    d.birth_rate = d.birth_rate * bio_factor;
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
    d.birth_rate = d.birth_rate * bio_factor;
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
    /// carrying capacity to ±10% of the 500 base for the *typical*
    /// habitable seed (Sparse–Lush–Hyper biospheres at Earth gravity
    /// and median cognition). The implementation pass landed
    /// substrate-derived demographics; the regional-scale rescale
    /// (50 → 500) preserves the same fractional habitability
    /// envelope, just with cells reading as continent-sized regional
    /// caps rather than village-sized ones. Sparse-world floor stays
    /// within 10% of Earth-equivalent so marginal seeds don't tip
    /// into `food_crisis` from the substrate factor alone.
    #[test]
    fn carrying_capacity_envelope_is_calibrated() {
        let earth_g = Real::from_ratio(981, 100);
        let median_cog = Real::from_ratio(5, 10);
        let lush = carrying_capacity_per_unit(BiosphereClass::Lush, earth_g, Real::ONE);
        let sparse = carrying_capacity_per_unit(BiosphereClass::Sparse, earth_g, median_cog);
        let hyper = carrying_capacity_per_unit(BiosphereClass::HyperBiodiverse, earth_g, Real::ONE);
        // Earth-equivalent (Lush + Earth-g + max-cog) recovers ~500.
        assert!(lush >= Real::from_int(450) && lush <= Real::from_int(550));
        // Sparse + median-cog stays within 10% of Earth-equivalent
        // so marginal habitability holds.
        assert!(sparse >= Real::from_int(450));
        // Hyper bonus is real but bounded — no surprise 2× scaling.
        assert!(hyper >= Real::from_int(500) && hyper <= Real::from_int(650));
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
        assert!(lush <= Real::from_int(80));
        // Sparse + median-cog: ~92 (close to old 100).
        assert!(sparse >= Real::from_int(80) && sparse <= Real::from_int(110));
        // None + zero-cog: ~120 (notably higher — these worlds
        // shouldn't host civs anyway).
        assert!(none >= Real::from_int(115));
    }
}
