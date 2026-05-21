//! Categorical bulk-planet enums: `Composition`, `Atmosphere`,
//! `BiosphereClass`, `Magnetosphere`, `Crust`, `MetabolicSubstrate`.
//! The continuous mixture structs (`AtmosphericComposition`,
//! `CrustalComposition`) live in `composition`; `Planet` itself
//! lives in `planet`.

/// Tidal-locking regime of a planet's rotation relative to its
/// orbit. Sampled at worldgen (Item 24) and evolved per-tick (Item
/// 19): eccentricity damping rate depends on which regime the world
/// is in, and locked planets get a fixed sub-stellar point rather
/// than a rotating one.
///
/// - `Synchronous`: rotation period == orbital period. One face
///   perpetually toward the star. Eccentricity damps quickly toward
///   zero (locked orbits become circular).
/// - `Resonance { p, q }`: rotation:orbit period ratio is `p:q`
///   (Mercury is 3:2 — three spins per two orbits). Gravitational
///   forcing from other bodies (Laplace-type resonances, Io-Europa-
///   Ganymede) sustains a non-zero eccentricity rather than letting
///   it damp out.
/// - `FreeRotator`: not locked. Eccentricity damps slowly through
///   ordinary tidal dissipation. Earth is here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockingState {
    Synchronous,
    Resonance { p: u8, q: u8 },
    FreeRotator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Composition {
    Rocky,
    OceanWorld,
    SubSurfaceOcean,
    GaseousShell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Atmosphere {
    None,
    Thin,
    Oxidising,
    Reducing,
    Hazy,
}

impl Atmosphere {
    /// Barometric-formula scale height in metres. Earth-
    /// like atmospheres get ~8400 m; thin (Mars-like) ~11000;
    /// thick (Venus-like) ~15000; hazy (Titan-cold-methane-rich)
    /// ~21000; `None` returns 1 (so the barometric `exp(-h/H)`
    /// factor returns ≈ 0 for any non-zero altitude — i.e.
    /// vacuum at every height).
    #[must_use]
    pub fn scale_height_m(self) -> i64 {
        match self {
            Self::None => 1,
            Self::Thin => 11_000,
            Self::Oxidising => 8_400,
            Self::Reducing => 15_000,
            Self::Hazy => 21_000,
        }
    }

    /// Surface atmospheric mass density × 100, in
    /// kg/m³ × 100 (so we keep it as `i64` without losing the
    /// integer-fraction Earth value of 1.22). `None` is 0
    /// (vacuum). Used by per-atmosphere callers that scale
    /// momentum / advection coefficients with density.
    #[must_use]
    pub fn density_x100(self) -> i64 {
        match self {
            Self::None => 0,
            // Mars-like surface ~0.02 kg/m³.
            Self::Thin => 2,
            // Earth-like surface ~1.22 kg/m³.
            Self::Oxidising => 122,
            // Venus-like surface ~67 kg/m³.
            Self::Reducing => 6_700,
            // Titan-like surface ~5.4 kg/m³.
            Self::Hazy => 540,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BiosphereClass {
    None,
    Sparse,
    Lush,
    HyperBiodiverse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Magnetosphere {
    None,
    Weak,
    Strong,
}

/// Crust mineral composition. Drives fuel availability and which
/// sensorium-extending tools / engineering disciplines a civ can
/// reach. Default is `Basaltic` (Earth-like balance); other
/// variants bias toward specific developmental tracks per the
/// project's "different worlds, different sciences" goal.
///
/// - `Basaltic`: balanced, nothing favoured.
/// - `Hydrocarbon`: buried fossil fuels accessible. Civs on these
///   worlds can develop combustion-driven tech easily.
/// - `Piezoelectric`: shallow piezoelectric crystal beds. Favours
///   resonance- and field-engineering tracks (the "field-and-
///   resonance civilisation" archetype). Combustion is harder.
/// - `Ferrous`: iron- and rare-earth-rich. Favours magnetism
///   and metallurgy without needing combustion.
/// - `RareEarth`: superconductor and exotic-element bias. Late-
///   game advanced electronics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Crust {
    Basaltic,
    Hydrocarbon,
    Piezoelectric,
    Ferrous,
    RareEarth,
}

/// The chemistry the planet's life is built on. Sampled at
/// planet creation; constrains the temperature / atmosphere / crust
/// windows so every seed produces a habitable world of *some* kind
/// rather than `BiosphereClass::None` Earth-water-chauvinism.
///
/// - `Aqueous` (water-based) — the Earth norm. Liquid water
///   250-400 K, requires an atmosphere.
/// - `Ammoniacal` (ammonia-based) — cold methane-skies. Liquid
///   ammonia 195-240 K, reducing or thin atmosphere.
/// - `Hydrocarbon` (methane / ethane-based) — Titan-style. Very
///   cold 90-180 K, reducing or hazy atmosphere; biased toward
///   Hydrocarbon crust.
/// - `Silicate` (silicon-substrate) — hypothesised high-T
///   crystalline life. Hot 800-1500 K; the only substrate that
///   tolerates `Atmosphere::None` (the crystal lattice doesn't
///   need a solvent atmosphere).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetabolicSubstrate {
    Aqueous,
    Ammoniacal,
    Hydrocarbon,
    Silicate,
}

impl MetabolicSubstrate {
    /// Substrate's liquid-phase temperature range in Kelvin. Used
    /// by `sample_planet` to constrain `mean_temperature` to a
    /// window where this substrate's biochemistry actually works.
    #[must_use]
    pub fn temperature_range(self) -> (i64, i64) {
        match self {
            MetabolicSubstrate::Aqueous => (250, 400),
            MetabolicSubstrate::Ammoniacal => (195, 240),
            MetabolicSubstrate::Hydrocarbon => (90, 180),
            MetabolicSubstrate::Silicate => (800, 1500),
        }
    }

    /// Whether this substrate's biochemistry tolerates the given
    /// atmosphere class. Aqueous needs *some* atmosphere
    /// (liquid water boils away in vacuum); Ammoniacal can't survive
    /// oxidising chemistries (NH3 burns); Hydrocarbon similarly;
    /// Silicate is the only one tolerant of `None` because crystal-
    /// substrate life doesn't need a fluid solvent.
    #[must_use]
    pub fn atmosphere_compatible(self, atm: Atmosphere) -> bool {
        match self {
            MetabolicSubstrate::Aqueous => !matches!(atm, Atmosphere::None),
            MetabolicSubstrate::Ammoniacal => {
                matches!(atm, Atmosphere::Reducing | Atmosphere::Thin)
            }
            MetabolicSubstrate::Hydrocarbon => {
                matches!(
                    atm,
                    Atmosphere::Reducing | Atmosphere::Hazy | Atmosphere::Thin
                )
            }
            MetabolicSubstrate::Silicate => true,
        }
    }

    /// Snake-case tag for protocol/event payloads.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            MetabolicSubstrate::Aqueous => "aqueous",
            MetabolicSubstrate::Ammoniacal => "ammoniacal",
            MetabolicSubstrate::Hydrocarbon => "hydrocarbon",
            MetabolicSubstrate::Silicate => "silicate",
        }
    }

    /// Substrate-derived **biological time-scale factor**. Slow
    /// chemistries unfold over geological time: a silicate crystal
    /// lattice's "generation" is glacial compared to an aqueous one's.
    /// This factor multiplies every per-tick biological / societal
    /// rate (birth, claim, tech, cohesion drift, religion drift) and
    /// inversely scales every streak / cooldown measured in ticks, so
    /// the integer tick count of the sim covers proportionally more
    /// substrate-internal time on slow substrates. Physics catastrophe
    /// cadences (asteroid, ice age, solar flare, volcanism) are *not*
    /// scaled — they're external to biology.
    ///
    /// Returns a multiplier in (0, 1]. Aqueous is the calibration
    /// baseline (1.0). The conservative initial spread (0.5 / 0.4 /
    /// 0.2) gives slow-chemistry worlds two- to five-times-longer
    /// civilizational arcs without compressing into a single tick what
    /// used to span a century.
    #[must_use]
    pub fn metabolism(self) -> sim_arith::Real {
        use sim_arith::Real;
        match self {
            MetabolicSubstrate::Aqueous => Real::ONE,
            MetabolicSubstrate::Ammoniacal => Real::from_ratio(5, 10),
            MetabolicSubstrate::Hydrocarbon => Real::from_ratio(4, 10),
            MetabolicSubstrate::Silicate => Real::from_ratio(2, 10),
        }
    }
}
