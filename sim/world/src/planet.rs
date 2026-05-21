//! `Planet` — bulk planet properties sampled at run start. Subset of
//! the full ~50-property seed; expands as later milestones need
//! more.

use crate::composition::{AtmosphericComposition, CrustalComposition, Moon};
use crate::types::{
    Atmosphere, BiosphereClass, Composition, Crust, LockingState, Magnetosphere,
    MetabolicSubstrate,
};
use sim_arith::transcendental::sqrt;
use sim_arith::Real;

/// Earth-equivalent surface gravity in m/s². Used as the
/// reference constant so `gravity(M, R) = EARTH_G * M / R²`
/// where M and R are in Earth units.
const EARTH_GRAVITY_MS2_X100: i64 = 981;

/// Earth's mean radius in metres. Used to lift the
/// Earth-relative escape velocity into km/s for human-friendly
/// output and downstream physics consumers.
const EARTH_RADIUS_M: i64 = 6_371_000;

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
    /// Planet mass in Earth masses (Earth = 1.0). Range depends
    /// on composition: Earth-like rocky 0.5-2.0, gas-giant-style
    /// gaseous shells up to ~50, low-mass sub-surface oceans
    /// down to ~0.1. Together with `radius` this derives surface
    /// gravity via the closed-form `g = EARTH_G × M / R²` and
    /// escape velocity via `v_escape = sqrt(2 × g × R)`. Sprint
    /// 5 Item 21 separated the mass/radius pair from the prior
    /// `gravity` scalar so atmospheric retention (Item 17) and
    /// tidal Love numbers (Item 16) can be derived rather than
    /// re-sampled.
    pub mass: Real,
    /// Planet radius in Earth radii (Earth = 1.0). Range
    /// depends on composition: Earth-like rocky 0.7-1.4,
    /// gas-giant gaseous shells up to ~11 (Jupiter ≈ 11
    /// Earth-radii), sub-surface oceans 0.3-0.7. Couples with
    /// `mass` for derived gravity / escape velocity.
    pub radius: Real,
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
    /// Tidal-locking regime of the planet's rotation relative to
    /// its orbit. Sampled at worldgen (Item 24) and consulted per-
    /// tick by tidal-locking dynamics (Item 19): controls the
    /// eccentricity-damping rate of each moon and whether the sub-
    /// stellar point is fixed (Synchronous) or rotates.
    ///
    /// Defaults to `FreeRotator` in legacy worldgen paths so back-
    /// compat fixtures don't have to populate this field.
    pub locking_state: LockingState,
}

impl Planet {
    /// Surface gravitational acceleration in m/s² derived from
    /// the planet's mass / radius pair. Earth-relative closed
    /// form: `g = EARTH_G × M / R²` where M and R are in
    /// Earth units and `EARTH_G ≈ 9.81 m/s²`. The Sprint 5 Item
    /// 21 refactor moved `gravity` from a stored scalar to this
    /// derived accessor so atmospheric retention (Item 17) and
    /// tidal coupling (Item 16) can read consistent
    /// mass/radius-driven values.
    #[must_use]
    pub fn gravity(&self) -> Real {
        // g = (EARTH_G / 100) * mass / (radius * radius), with
        // the divide-by-100 baked into the Real constant so the
        // intermediate stays inside Q32.32 range.
        let earth_g = Real::from_ratio(EARTH_GRAVITY_MS2_X100, 100);
        if self.radius == Real::ZERO {
            return Real::ZERO;
        }
        earth_g * self.mass / (self.radius * self.radius)
    }

    /// Escape velocity in km/s at the planet's surface. Formula:
    /// `v_escape = sqrt(2 × g × R)` lifted into km/s. In
    /// Earth-relative units this reduces to
    /// `v_escape_kms = sqrt(2 × g_earth_ms2 × R_earth_m × M / R) / 1000`.
    /// Earth analog (M=1, R=1) yields ≈11.186 km/s. Used by
    /// Item 17's atmospheric retention calculation.
    #[must_use]
    pub fn escape_velocity(&self) -> Real {
        // Derivation:
        //   v_kms² = 2 * g_ms2 * R_meters / 1_000_000
        //          = 2 * (g_earth_ms2 * M/R²) * (R * R_earth_m) / 1e6
        //          = 2 * g_earth_ms2 * R_earth_m / 1e6 * M/R
        //
        // 2 × 9.81 × 6_371_000 / 1_000_000 ≈ 124.99 km²/s². With
        // M=R=1 (Earth), v ≈ sqrt(124.99) ≈ 11.18 km/s. We pack
        // the constant as a `Real::from_ratio` to stay integer-
        // only on the deterministic path. The `100 × 1000`
        // denominator absorbs both the EARTH_GRAVITY_MS2_X100
        // hundredths anchor and the metres→kilometres divide.
        if self.radius == Real::ZERO || self.mass == Real::ZERO {
            return Real::ZERO;
        }
        let two_g_r_earth_km2_s2 = Real::from_ratio(
            2 * EARTH_GRAVITY_MS2_X100 * (EARTH_RADIUS_M / 1_000),
            100 * 1_000,
        );
        let v_squared = two_g_r_earth_km2_s2 * self.mass / self.radius;
        sqrt(v_squared)
    }

    /// Bulk density in g/cm³ derived from the planet's
    /// metabolic substrate. Earth ≈ 5.5 g/cm³ (silicate rock);
    /// water-substrate ocean worlds drop to ~1; ammonia ~0.7;
    /// hydrocarbon ices ~0.5. Used by Item 16 tidal Love-number
    /// coupling and Item 21's mass/radius consistency.
    ///
    /// The mapping is substrate-driven by design (Item 21:
    /// "density derived per substrate"); mass/radius vary
    /// independently within each substrate's class.
    #[must_use]
    pub fn density(&self, substrate: &MetabolicSubstrate) -> Real {
        match substrate {
            // Water-substrate ocean worlds: liquid-water-dominated
            // bulk, density ~1 g/cm³.
            MetabolicSubstrate::Aqueous => Real::ONE,
            // Ammonia-substrate cold worlds: ammonia is 0.68
            // g/cm³ liquid; ammonia/ice mix lands near 0.7.
            MetabolicSubstrate::Ammoniacal => Real::from_ratio(7, 10),
            // Methane/ethane Titan-style: bulk density ~0.5
            // g/cm³ (methane-water-ice hybrid worlds).
            MetabolicSubstrate::Hydrocarbon => Real::from_ratio(5, 10),
            // Silicate rock worlds: Earth ≈ 5.5 g/cm³; round
            // to 5 g/cm³ as the substrate-typical anchor.
            MetabolicSubstrate::Silicate => Real::from_int(5),
        }
    }

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
