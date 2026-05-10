//! `Planet` — bulk planet properties sampled at run start. Subset of
//! the full ~50-property seed; expands as later milestones need
//! more.

use crate::composition::{AtmosphericComposition, CrustalComposition, Moon};
use crate::types::{
    Atmosphere, BiosphereClass, Composition, Crust, Magnetosphere, MetabolicSubstrate,
};
use sim_arith::Real;

/// Bulk planet properties sampled at run start. Subset of the full
/// ~50-property seed; expands as later milestones need more.
///
/// All quantities are in SI units. Field documentation gives
/// the unit + a baseline reference where applicable. The reference
/// values are calibration anchors for sampling ranges only; per-
/// planet variation puts every seed somewhere different in the
/// space — gravity 0.4-2.5×, temperature 90-1500 K, pressure
/// 0-200% baseline, atmosphere 0-5 substrate-compatible classes,
/// etc. None of the documented "≈" values is hardcoded into
/// physics; they're the planetological neighbourhood the sampler
/// covers, written down for human readers.
#[derive(Debug, Clone)]
pub struct Planet {
    pub seed: u64,
    /// Deterministic planet name derived from the seed.
    /// Pure flavour; no physics depends on it. Used by the
    /// viewport / report layers for human-readable identification.
    pub name: String,
    /// Surface gravitational acceleration in m/s² (Earth ≈ 9.81).
    /// Range ~1.0 to ~30.0.
    pub gravity: Real,
    pub composition: Composition,
    /// Mean surface temperature in K (Earth ≈ 288). Range varies
    /// by composition.
    pub mean_temperature: Real,
    /// Equator-to-pole temperature spread in K. Smaller for ocean
    /// worlds (heat capacity), larger for desiccated rocky worlds.
    pub temperature_gradient: Real,
    /// Highest terrain point above the abyssal plain, in metres
    /// (Earth's Everest ≈ 8849). Range ~0 to ~15 000.
    pub terrain_peak: Real,
    /// Where the terrain peak sits, in axial coords on the run's
    /// grid. Wraps around grid bounds at init time.
    pub terrain_centre_q: i32,
    pub terrain_centre_r: i32,
    /// Sea level in metres above the abyssal plain. Below this
    /// elevation, water fills cells.
    pub sea_level: Real,
    pub atmosphere: Atmosphere,
    /// Continuous atmospheric composition (mass fractions).
    /// Sampled per planet alongside the categorical `atmosphere`
    /// label; the label summarises the composition. Read by
    /// physics / recognition / habitability paths that benefit
    /// from finer-grained mixture sensitivity (e.g. an
    /// `Oxidising` atmosphere with 0.18 O₂ tolerates fire-with-
    /// trace-CO₂ templates differently than one with 0.25 O₂).
    /// Existing categorical-only consumers ignore this field.
    pub atmospheric_composition: AtmosphericComposition,
    /// Surface atmospheric pressure in Pa (Earth ≈ 101 325 Pa).
    /// Drives the boiling point of water (Clausius-Clapeyron),
    /// charge mobility, and several other law coefficients.
    pub surface_pressure: Real,
    pub biosphere: BiosphereClass,
    /// Continuous biosphere richness in `[0, 1]`. 0 = lifeless
    /// rock, 0.25 = sparse pioneer microbiome, 0.5 = stable
    /// multi-trophic web, 0.75 = lush diversified biosphere, 1.0 =
    /// hyper-biodiverse rainforest-equivalent. Sampled per planet
    /// from a categorical-aware baseline (`BiosphereClass::Sparse`
    /// → 0.20-0.40 etc.) with ±0.10 jitter. Future
    /// habitability / tech / civ-capacity paths can read this scalar
    /// directly instead of bucketing on the four-class enum, e.g.
    /// continuous `food_security_floor = 0.3 + 0.4 × biosphere_density`
    /// rather than the current per-class step function.
    pub biosphere_density: Real,
    pub magnetosphere: Magnetosphere,
    /// Crust mineral profile. Decouples fossil-fuel availability
    /// from biosphere alone (allows "no combustion path" worlds)
    /// and gates resonance/EM/electronics tool tracks.
    pub crust: Crust,
    /// Continuous crustal composition (mass fractions across
    /// silicate / hydrocarbon / piezoelectric / ferrous / `rare_earth`
    /// / ice / other). Sampled per planet alongside the categorical
    /// `crust` label; the label summarises the dominant fraction.
    /// Future fuel / tech / industrial-capacity paths can read this
    /// directly (a `Hydrocarbon` crust with 0.06 hydrocarbon supports
    /// less combustion-driven tech than one with 0.14). Existing
    /// categorical-only consumers ignore this field.
    pub crustal_composition: CrustalComposition,
    /// Stellar irradiance at the planet's orbit, in W/m²
    /// (Earth ≈ 1361 W/m²). Range ~200 to ~3000.
    pub stellar_luminosity: Real,
    /// Moon count. An earlier single-moon tides law treated any
    /// non-zero count as one Earth-like cycle; the current
    /// per-moon list lets multi-moon planets get spring/neap
    /// interference patterns.
    pub moon_count: u8,
    /// Per-moon orbital configuration. Length matches
    /// `moon_count`. For moonless worlds this is empty.
    pub moons: Vec<Moon>,
    /// Orbital eccentricity × 100. Earth ≈ 1.67 (so 2);
    /// Mars ≈ 9.34 (so 9); typical exoplanet e ≤ 50. Drives
    /// the insolation swing between perihelion and aphelion:
    /// `S(season) ∝ 1 / (1 - e · cos(2π·season/year))²`.
    /// Earlier worldgen used circular orbits; with eccentricity
    /// sampled, eccentric worlds get asymmetric seasons (a long
    /// cool half, a short hot half on highly-eccentric worlds).
    pub orbital_eccentricity_x100: i64,
    /// Axial tilt in degrees (Earth ≈ 23.4°). Range 0–90. Drives
    /// seasonal swing strength (high tilt = pronounced seasons,
    /// 0 tilt = perpetual day-night equality at any latitude).
    /// Currently flavour-only; reserved for a future seasonal
    /// physics pass.
    pub axial_tilt_deg: Real,
    /// Sidereal day length in hours (Earth ≈ 24). Range 4–200.
    /// Short days produce rapid thermal cycling; long days produce
    /// stark day/night temperature swings. Flavour-only currently.
    pub day_length_hours: Real,
    /// Per-planet calendar — months per orbital period.
    /// Sampled in [8, 16]. This is the **planet's actual year
    /// length** for calendar purposes: seasonal physics
    /// (`climate::seasonal_temperature_offset`), the post-run report
    /// Y/M display, viewport caption, civ-founded year, log-line
    /// year prefix, and recognition `MonthIn` templates all use
    /// this period. The sim tick cadence is still 1 tick = 1
    /// species-month; rate-calibration constants like
    /// `STAGNATION_THRESHOLD_TICKS` and `earth_like_default`
    /// birth/death rates use the 12-tick `BASELINE_MONTHS_PER_YEAR`
    /// reference so a planet with a 16-month year still runs the
    /// same per-tick physics calibration — what changes is how
    /// many of *those calibration ticks* the user sees per planet-year.
    pub orbital_period_months: u32,
    /// The biochemistry life on this planet runs on. Sampled
    /// first in `sample_planet`; constrains the temperature /
    /// atmosphere / crust windows. Guarantees every seed produces a
    /// habitable world of *some* substrate kind.
    pub metabolic_substrate: MetabolicSubstrate,
    /// Per-seed substrate-chemistry perturbation in `[-0.05,
    /// +0.05]`. The substrate's nominal freeze and boil points
    /// (e.g. aqueous water at 273.15 K / 373.15 K) get shifted by
    /// `nominal × perturbation` so each seed's chemistry is
    /// slightly different — water on seed 42 might freeze at
    /// 273.5 K, on seed 100 at 270.7 K. The substrate enum
    /// (Aqueous / Ammoniacal / Hydrocarbon / Silicate) stays the
    /// same; what varies is the *exact* phase-transition
    /// temperature within the substrate's tolerance window.
    /// Sampled deterministically from the seed so byte-replay
    /// holds.
    pub substrate_perturbation: Real,
}

impl Planet {
    /// Every sampled planet is habitable by construction (the
    /// substrate-first sampler picks a metabolic chemistry, then
    /// constrains atmosphere/temperature/crust to that chemistry's
    /// tolerance window). Kept as a public predicate for downstream
    /// consumers that want to assert the contract.
    #[must_use]
    pub fn is_habitable(&self) -> bool {
        self.biosphere != BiosphereClass::None
            && self
                .metabolic_substrate
                .atmosphere_compatible(self.atmosphere)
    }

    /// Tidally-locked predicate. A planet whose `day_length_hours`
    /// equals or exceeds its orbital period is tidally locked — one
    /// face perpetually toward its star, the other perpetually away.
    /// Civilisations only thrive at the *terminator* (the boundary
    /// between perpetual day and perpetual night) where the
    /// temperature is mild. We pin tidal locking at
    /// `day_length_hours >= 1000` (~42 Earth days) as the simulation's
    /// proxy: any rotation that slow approximates locked-rotation
    /// dynamics for biography purposes.
    #[must_use]
    pub fn is_tidally_locked(&self) -> bool {
        self.day_length_hours >= Real::from_int(1_000)
    }
}
