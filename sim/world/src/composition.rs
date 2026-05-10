//! Continuous mass-fraction mixtures and per-moon orbital data.
//! `AtmosphericComposition` and `CrustalComposition` are the
//! fine-grained successors to the categorical labels in
//! `types`; `Moon` is the per-moon orbital descriptor.

use sim_arith::Real;

/// Per-moon orbital data. Each moon contributes a tidal
/// bulge at its own period; the tides law sums their
/// per-cell potentials, producing real interference patterns
/// (Earth's spring / neap cycle from moon × sun is the classic
/// example; multi-moon planets get richer beats).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Moon {
    /// Mass relative to Earth's moon × 100. Larger mass → bigger
    /// tidal bulge. Sampled at worldgen.
    pub mass_relative_x100: i64,
    /// Orbital period in macro-steps (≈ days). Earth's moon =
    /// 28; smaller / closer moons orbit faster.
    pub orbital_period_macros: u32,
}

/// Continuous atmospheric composition. Mass fractions for
/// the eight molecular species the sim tracks; sums to ≤ 1.0 (the
/// `other` slot covers everything else). All fractions are Q32.32.
///
/// This sits alongside the categorical `Atmosphere` enum rather
/// than replacing it — existing code paths (recognition templates,
/// physics scale heights, density approximations) read the enum
/// label, while new consumers can inspect the actual ratios. A
/// future PR migrates the categorical paths to read composition
/// directly. The composition data ships now; categorical paths
/// migrate later.
///
/// Sampled in `sample_atmospheric_composition` per planet seed,
/// constrained by the categorical `Atmosphere` so a planet
/// labelled `Oxidising` always has substantial O₂ (≥ 0.15) and
/// substantial N₂ (≥ 0.5), whereas a `Reducing` planet can only
/// have trace O₂ (< 0.01). The categorical label is preserved as
/// the *summary* of the composition — `Atmosphere::from_composition`
/// recovers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtmosphericComposition {
    /// Nitrogen N₂ mass fraction. Earth-equivalent ≈ 0.78.
    pub n2: Real,
    /// Oxygen O₂ mass fraction. Earth-equivalent ≈ 0.21.
    pub o2: Real,
    /// Carbon dioxide CO₂ mass fraction. Earth-equivalent ≈ 0.0004.
    /// Venus-equivalent ≈ 0.96.
    pub co2: Real,
    /// Methane CH₄ mass fraction. Titan-equivalent ≈ 0.014;
    /// Earth-equivalent ≈ 1.8e-6.
    pub ch4: Real,
    /// Ammonia NH₃ mass fraction. Reducing-atmosphere proxy on
    /// non-Earth substrates; Earth-equivalent ≈ 0.
    pub nh3: Real,
    /// Water vapour H₂O mass fraction. Earth-tropics ≈ 0.04.
    pub h2o: Real,
    /// Hydrogen H₂ mass fraction. Reducing / gas-giant atmospheres.
    pub h2: Real,
    /// Argon Ar mass fraction. Earth-equivalent ≈ 0.009.
    pub ar: Real,
    /// Catch-all for everything else (sulphates, halogens, dust,
    /// noble-gas trace, unmodelled species). Closes the sum at 1.0.
    pub other: Real,
}

impl AtmosphericComposition {
    /// Vacuum-equivalent composition: all zeros. Sums to 0,
    /// so the categorical-label derivation returns
    /// `Atmosphere::None`.
    #[must_use]
    pub fn vacuum() -> Self {
        Self {
            n2: Real::ZERO,
            o2: Real::ZERO,
            co2: Real::ZERO,
            ch4: Real::ZERO,
            nh3: Real::ZERO,
            h2o: Real::ZERO,
            h2: Real::ZERO,
            ar: Real::ZERO,
            other: Real::ZERO,
        }
    }

    /// Sum of all fractions. Should be ≤ 1.0; the
    /// `from_composition` derivation tolerates small rounding
    /// drift but a sum below 0.5 is treated as no atmosphere.
    #[must_use]
    pub fn total(&self) -> Real {
        self.n2
            + self.o2
            + self.co2
            + self.ch4
            + self.nh3
            + self.h2o
            + self.h2
            + self.ar
            + self.other
    }
}

/// Continuous crustal composition (mass fractions). Alongside
/// the categorical `Crust` enum, this is the actual mineral mix — a
/// `Basaltic` crust always has substantial silicate (≥ 0.5) but the
/// remainder varies per seed across the other channels. Future
/// physics / tech / habitability paths can read fractions directly
/// instead of categorical buckets; the enum remains as the
/// summary label.
///
/// All fractions Q32.32; total approaches 1.0. Sampler in
/// `sample_crustal_composition` keyed on the categorical `Crust`
/// label and `MetabolicSubstrate`, with ±10% per-channel jitter
/// before normalising.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrustalComposition {
    /// Silicate / basalt mineral fraction. Earth crust ≈ 0.59
    /// (silica + alumina + iron oxide combined).
    pub silicate: Real,
    /// Buried-hydrocarbon (coal / oil / methane clathrate) fraction.
    /// Earth crust ≈ 0.0001; Hydrocarbon-archetype worlds 0.05-0.15.
    pub hydrocarbon: Real,
    /// Piezoelectric crystal (quartz, tourmaline, topaz) fraction.
    /// Earth crust ≈ 0.05 (quartz alone); Piezoelectric-archetype
    /// worlds 0.20-0.40.
    pub piezoelectric: Real,
    /// Iron + transition-metal fraction. Earth crust ≈ 0.05;
    /// Ferrous-archetype worlds 0.20-0.40.
    pub ferrous: Real,
    /// Rare-earth + lanthanide + exotic-element fraction. Earth
    /// crust ≈ 0.0001; RareEarth-archetype worlds 0.05-0.15.
    pub rare_earth: Real,
    /// Subsurface ice / volatiles fraction. Earth crust ≈ 0.001
    /// (cryosphere); icy-moon-style worlds 0.30+.
    pub ice: Real,
    /// Catch-all for sulphides, carbonates, sulphates, silicate-
    /// dust, and unmodelled minerals. Closes the sum at 1.0.
    pub other: Real,
}

impl CrustalComposition {
    /// Vacuum-equivalent (no crust). All zeros; categorical
    /// derivation falls back to `Basaltic`.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            silicate: Real::ZERO,
            hydrocarbon: Real::ZERO,
            piezoelectric: Real::ZERO,
            ferrous: Real::ZERO,
            rare_earth: Real::ZERO,
            ice: Real::ZERO,
            other: Real::ZERO,
        }
    }

    /// Sum of all fractions. ≈ 1.0 for any sampled crust;
    /// 0 for `empty()`.
    #[must_use]
    pub fn total(&self) -> Real {
        self.silicate
            + self.hydrocarbon
            + self.piezoelectric
            + self.ferrous
            + self.rare_earth
            + self.ice
            + self.other
    }
}
