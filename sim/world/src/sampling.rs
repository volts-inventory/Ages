//! Deterministic planet name pool + substrate-first
//! `sample_planet`. Pulled out of `lib.rs` so the bulky sampling
//! pipeline (~290 lines of weighted-distribution table) sits next
//! to its rationale rather than burying the type definitions
//! that lib.rs exists to declare.

use crate::{
    Atmosphere, AtmosphericComposition, BiosphereClass, Composition, Crust, CrustalComposition,
    LockingState, Magnetosphere, MetabolicSubstrate, Moon, Planet, SpectralType, Star,
};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use sim_arith::Real;

/// SplitMix64 salt for the locking-state jitter stream (Sprint 5
/// Item 24). Distinct from the main `ChaCha20Rng` draw sequence so
/// the planet-wide locking decision doesn't disturb byte-replay of
/// the other worldgen draws — the locking state is a *post-process*
/// classifier over already-sampled moon + rotation fields rather
/// than an entry in the linear draw order.
const LOCKING_SALT: u64 = 0x4C6F_636B_696E_6721; // "Locking!" ASCII

/// Sample a deterministic Planet from a seed. Same seed → identical
/// Planet bit-for-bit. All quantities are in SI units.
///
/// Substrate-first. The sampler picks a `MetabolicSubstrate`
/// (Aqueous / Ammoniacal / Hydrocarbon / Silicate) up front, then
/// constrains temperature / atmosphere / composition / crust to
/// that substrate's tolerance window. Every seed produces a
/// habitable world of *some* chemistry — `BiosphereClass::None` is
/// no longer a normal-sampling outcome.
///
/// Deterministic planet name from the seed. 64 evocative
/// stem names × suffix letter (16) gives ~1024 distinct names
/// before collisions; same seed always picks the same name.
#[must_use]
pub fn planet_name_from_seed(seed: u64) -> String {
    const STEMS: [&str; 64] = [
        "Vela", "Lyra", "Cygnus", "Phoebe", "Aleph", "Sevra", "Tolak", "Mira", "Orix", "Kepler",
        "Rigel", "Astra", "Caelum", "Draco", "Ember", "Fornax", "Gleam", "Hesper", "Indus",
        "Jovis", "Kraken", "Lethe", "Morn", "Nyx", "Ophir", "Pyre", "Quasar", "Rune", "Sable",
        "Talos", "Umbra", "Vesper", "Wraith", "Xen", "Yarn", "Zephyr", "Aurora", "Boreal",
        "Cinder", "Dune", "Echo", "Frost", "Glade", "Halo", "Iris", "Jasper", "Kelvin", "Lumen",
        "Mist", "Nova", "Onyx", "Pulse", "Quartz", "Reverb", "Solace", "Tide", "Undine", "Verge",
        "Whisp", "Xylem", "Yonder", "Zenith", "Aether", "Brume",
    ];
    const SUFFIX: [char; 16] = [
        'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p',
    ];
    // Q32.32 deterministic indexing — modulo on the raw u64
    // bits keeps the same value across architectures. The
    // post-mod values are bounded by STEMS.len() / SUFFIX.len()
    // (≤ 64) so the `as usize` truncation is provably safe.
    let stem_idx = usize::try_from(seed % (STEMS.len() as u64)).unwrap_or(0);
    let suffix_idx = usize::try_from((seed >> 6) % (SUFFIX.len() as u64)).unwrap_or(0);
    format!("{}-{}", STEMS[stem_idx], SUFFIX[suffix_idx])
}

/// User-supplied overrides applied on top of the seed-sampled
/// `Planet`. Each field is `Option`; `None` keeps the seed-driven
/// value. Map geography (terrain elevation, water depth, sea level,
/// terrain peak) is **not** overridable — it always comes from the
/// seed so a `--config` run still produces a varied landscape.
///
/// Coherence: when `substrate` is overridden, the downstream
/// substrate-conditional fields (atmospheric_composition,
/// crustal_composition) re-sample from a deterministic salt-stream
/// keyed on the seed + override pair, so the planet card and physics
/// still agree internally.
#[derive(Debug, Clone, Default)]
pub struct PlanetOverrides {
    pub substrate: Option<MetabolicSubstrate>,
    pub atmosphere: Option<Atmosphere>,
    pub mean_temperature_k: Option<i64>,
    /// Gravity in Earth-g × 100 (so 100 = 1.0 g, 250 = 2.5 g).
    /// Implemented by setting `mass = g_x100/100` and `radius = 1.0`,
    /// which gives the requested derived gravity (`g = M/R²` in
    /// Earth-relative units).
    pub gravity_g_x100: Option<i64>,
    pub spectral_type: Option<crate::SpectralType>,
    pub axial_tilt_deg: Option<i64>,
    pub day_length_hours: Option<i64>,
    pub orbital_period_months: Option<u32>,
    pub moon_count: Option<u8>,
    pub magnetosphere: Option<crate::Magnetosphere>,
    pub crust: Option<Crust>,
    pub biosphere: Option<BiosphereClass>,
}

/// Salt for the override re-sample RNG stream. Distinct from
/// `LOCKING_SALT` so the locking decision stays bit-identical when
/// `--config` only changes substrate.
const OVERRIDE_SALT: u64 = 0x436F_6E66_6967_2121; // "Config!!" ASCII

/// Sample a planet from `seed`, then apply user overrides on top.
/// Same semantics as [`sample_planet`] but with the per-field
/// overrides from a `--config` interactive run.
#[must_use]
pub fn sample_planet_with_overrides(seed: u64, overrides: &PlanetOverrides) -> Planet {
    let mut planet = sample_planet(seed);
    apply_overrides(&mut planet, seed, overrides);
    planet
}

fn apply_overrides(planet: &mut Planet, seed: u64, o: &PlanetOverrides) {
    let substrate_changed = o.substrate.is_some_and(|s| s != planet.metabolic_substrate);
    let atmosphere_changed = o.atmosphere.is_some_and(|a| a != planet.atmosphere);

    if let Some(s) = o.substrate {
        planet.metabolic_substrate = s;
    }
    if let Some(a) = o.atmosphere {
        planet.atmosphere = a;
    }
    if let Some(t_k) = o.mean_temperature_k {
        planet.mean_temperature = Real::from_int(t_k);
    }
    if let Some(g_x100) = o.gravity_g_x100 {
        // g = M / R²; set R=1 so g = M directly. Earth-relative.
        planet.mass = Real::from_ratio(g_x100, 100);
        planet.radius = Real::ONE;
    }
    if let Some(tilt) = o.axial_tilt_deg {
        planet.axial_tilt_deg = Real::from_int(tilt);
    }
    if let Some(d) = o.day_length_hours {
        planet.day_length_hours = Real::from_int(d);
    }
    if let Some(m) = o.orbital_period_months {
        planet.orbital_period_months = m;
    }
    if let Some(mc) = o.moon_count {
        planet.moon_count = mc;
        planet.moons.truncate(mc as usize);
    }
    if let Some(mag) = o.magnetosphere {
        planet.magnetosphere = mag;
    }
    if let Some(c) = o.crust {
        planet.crust = c;
    }
    if let Some(b) = o.biosphere {
        planet.biosphere = b;
    }
    if let Some(spec) = o.spectral_type {
        // Rebuild the star at the new spectral type, keeping the
        // sampled bolometric irradiance + age intact so the rest of
        // the physics still has consistent inputs.
        let lifetime_gyr = spec.nominal_lifetime_gyr();
        let age_gyr = planet.star.main_sequence_age_gyr;
        planet.star = crate::Star::with_age(spec, planet.stellar_luminosity, age_gyr, lifetime_gyr);
    }

    // Substrate-conditional re-sampling. Atmospheric + crustal
    // composition baselines are keyed on the (atmosphere, substrate)
    // pair; if either changed, regenerate them from a salted RNG
    // substream so the planet stays internally consistent without
    // disturbing the main worldgen draw sequence.
    if substrate_changed || atmosphere_changed {
        let salt = splitmix64(seed.wrapping_add(OVERRIDE_SALT));
        let mut rng = ChaCha20Rng::seed_from_u64(salt);
        planet.atmospheric_composition = sample_atmospheric_composition(
            planet.atmosphere,
            planet.metabolic_substrate,
            &mut rng,
        );
        planet.crustal_composition =
            sample_crustal_composition(planet.crust, planet.metabolic_substrate, &mut rng);
    }
}

/// Reconcile a sampled `composition` with the sampled mean surface
/// temperature so the planet *type* and its *climate* can't contradict
/// each other — the source of nonsensical worlds like a 378 K
/// "sub-surface ocean". Keyed on the substrate's reference freeze/boil
/// points (pressure-independent here — composition only sets the
/// terrain-morphology ranges; the pressure-accurate liquid-vs-dry state
/// of individual cells is decided later by `surface_solvent_boiled` /
/// `terrain_glyph_at`):
///
///   * **Above boil** — the surface solvent has boiled off, so a
///     standing-liquid surface is impossible. `OceanWorld` /
///     `SubSurfaceOcean` collapse to a dry `Rocky` surface (which still
///     samples a normal sea_level, so any sub-boil cells stay wet).
///     Gas shells have no surface liquid to lose and are left alone.
///   * **Sub-surface ocean needs a frozen lid** — a sub-surface ocean
///     is liquid *beneath an ice shell*, which only forms when the
///     surface is frozen (mean temp below freeze). A warm
///     `SubSurfaceOcean` sample is incoherent (the ocean would sit at
///     the surface), so it is promoted to a surface `OceanWorld`.
///
/// Consumes no RNG — applied between temperature sampling and the
/// composition-keyed terrain draws — so only genuinely-inconsistent
/// seeds shift; every other seed's planet is byte-identical.
fn reconcile_composition_with_temperature(
    composition: Composition,
    substrate: MetabolicSubstrate,
    mean_temperature: Real,
) -> Composition {
    let (freeze_k, boil_k) = sim_physics::chemistry::substrate_phase_thresholds(substrate.tag());
    if mean_temperature > boil_k {
        return match composition {
            Composition::OceanWorld | Composition::SubSurfaceOcean => Composition::Rocky,
            other => other,
        };
    }
    if matches!(composition, Composition::SubSurfaceOcean) && mean_temperature >= freeze_k {
        return Composition::OceanWorld;
    }
    composition
}

#[allow(clippy::too_many_lines)]
pub fn sample_planet(seed: u64) -> Planet {
    let name = planet_name_from_seed(seed);
    let mut rng = ChaCha20Rng::seed_from_u64(seed);

    // Pick the metabolic substrate first. Aqueous biased high
    // (water is the most common solvent in the universe); the
    // remaining 40% rotates rarer chemistries.
    let metabolic_substrate = match rng.gen_range(0..20) {
        0..=11 => MetabolicSubstrate::Aqueous,      // 60%
        12..=14 => MetabolicSubstrate::Ammoniacal,  // 15%
        15..=17 => MetabolicSubstrate::Hydrocarbon, // 15%
        _ => MetabolicSubstrate::Silicate,          // 10%
    };

    // Sample mass + radius in Earth units, biased by substrate.
    // Sprint 5 Item 21: gravity is now derived from (mass, radius)
    // instead of sampled directly, so atmospheric retention
    // (Item 17) and tidal Love numbers (Item 16) can be consistent
    // with the planet's bulk pair.
    //
    // Ranges (Earth-relative):
    //   Aqueous     : mass 0.5-2.0, radius 0.8-1.4  (Earth-like)
    //   Silicate    : mass 0.5-2.5, radius 0.7-1.3  (rocky variant
    //                 with hot dense lattice; can be heavier)
    //   Ammoniacal  : mass 0.5-2.0, radius 0.9-1.6  (cold ammonia
    //                 worlds tend larger / lower-density)
    //   Hydrocarbon : mass 0.3-1.5, radius 0.6-1.3  (Titan-style
    //                 low-mass ices)
    //
    // Derived gravity (Earth-relative `g = M/R²` ×9.81 m/s²)
    // ends up inside 2.5-50 m/s² across the four substrates —
    // a wider plausible band than the prior 1.0-30.0 m/s² scalar
    // sampling, picking up super-Earth and high-density silicate
    // edge cases. Integers ×10 → Real ratio so the deterministic
    // path stays integer-only.
    let (mass_lo_x10, mass_hi_x10, radius_lo_x10, radius_hi_x10) = match metabolic_substrate {
        MetabolicSubstrate::Aqueous => (5, 20, 8, 14),
        MetabolicSubstrate::Silicate => (5, 25, 7, 13),
        MetabolicSubstrate::Ammoniacal => (5, 20, 9, 16),
        MetabolicSubstrate::Hydrocarbon => (3, 15, 6, 13),
    };
    let mass_x10: i64 = rng.gen_range(mass_lo_x10..=mass_hi_x10);
    let radius_x10: i64 = rng.gen_range(radius_lo_x10..=radius_hi_x10);
    let mass = Real::from_ratio(mass_x10, 10);
    let radius = Real::from_ratio(radius_x10, 10);

    // Composition biased per substrate. Aqueous / Ammoniacal /
    // Silicate live on rocky-or-ocean worlds; Hydrocarbon also
    // accommodates GaseousShell (Titan / Jupiter atmospheric
    // chemistry). SubSurfaceOcean is Aqueous-only — its sub-surface
    // liquid is water by definition.
    let composition = match metabolic_substrate {
        MetabolicSubstrate::Aqueous => match rng.gen_range(0..20) {
            0..=11 => Composition::Rocky,
            12..=15 => Composition::OceanWorld,
            _ => Composition::SubSurfaceOcean,
        },
        MetabolicSubstrate::Ammoniacal => match rng.gen_range(0..10) {
            0..=6 => Composition::Rocky,
            _ => Composition::OceanWorld,
        },
        MetabolicSubstrate::Hydrocarbon => match rng.gen_range(0..10) {
            0..=4 => Composition::Rocky,
            5..=6 => Composition::OceanWorld,
            _ => Composition::GaseousShell,
        },
        MetabolicSubstrate::Silicate => Composition::Rocky,
    };

    // Mean surface temperature in K, constrained by the substrate's
    // liquid-phase window. The substrate's `temperature_range()` is
    // the source of truth; sampling within it guarantees the
    // chemistry is biochemically viable on this planet.
    let (t_lo_k, t_hi_k) = metabolic_substrate.temperature_range();
    let mean_temperature = Real::from_int(rng.gen_range(t_lo_k..=t_hi_k));

    // Reconcile the planet type with the just-sampled temperature so
    // terrain and climate agree (e.g. a hothouse "sub-surface ocean").
    // Done before the composition-keyed sea_level / terrain_peak draws
    // so the corrected type drives the terrain ranges; consumes no RNG
    // so unaffected seeds stay byte-identical.
    let composition =
        reconcile_composition_with_temperature(composition, metabolic_substrate, mean_temperature);

    // Equator-to-pole temperature spread in K, weakly
    // correlated with axial tilt. High-tilt worlds have wider
    // gradients (solar irradiance concentrates at the sub-solar
    // latitude band); low-tilt worlds get a narrower gradient.
    // Pre-sample tilt in i64 so the gradient bracket can derive
    // from it without f64 round-trip (the deterministic path
    // stays integer / Q32.32-only).
    let axial_tilt_pre_int: i64 = rng.gen_range(0..=45);
    // Linear interpolation: at tilt=0 → [5,25]; at tilt=45 → [20,50].
    // Computed integer-only: bracket_lo = 5 + (15 * tilt / 45),
    // bracket_hi = 25 + (25 * tilt / 45).
    let gradient_lo: i64 = 5 + (15 * axial_tilt_pre_int) / 45;
    let gradient_hi: i64 = 25 + (25 * axial_tilt_pre_int) / 45;
    let temperature_gradient = Real::from_int(rng.gen_range(gradient_lo..=gradient_hi));

    // Sea level in metres above the abyssal plain. Sampled before
    // terrain peak so peaks can be guaranteed to rise above the
    // waterline — without that guarantee, a sampled (peak, sea)
    // pair often had peak < sea, leaving the planet ocean-only and
    // its biosphere with no land cells to deposit fuel on. That was
    // turning otherwise-habitable seeds (lush rocky planets) into
    // immediate species_extinction runs.
    let sea_level = match composition {
        Composition::Rocky => Real::from_int(rng.gen_range(1_000..=4_000)),
        Composition::OceanWorld => Real::from_int(rng.gen_range(3_000..=7_000)),
        Composition::SubSurfaceOcean => Real::from_int(rng.gen_range(8_000..=15_000)),
        Composition::GaseousShell => Real::ZERO,
    };

    // Terrain peak above the abyssal plain, in metres. Earth's
    // Everest ≈ 8849 m; cap at 15 000 m for high-relief worlds.
    // Lower bound is `sea_level + 1500 m` so a non-trivial land
    // mass exists; without it, low-peak high-sea-level samples
    // would erase all land cells.
    // Convert sea_level to integer metres so the terrain_peak range
    // can use it as a lower bound. Sea levels are sampled from
    // integer ranges (1000..=15000) so the f64 round-trip is lossless
    // within the sampled domain; the suppression here is local to
    // this safe site rather than a crate-wide allow.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let sea_lvl_int = sea_level.to_f64_for_display() as i64;
    let terrain_peak = match composition {
        Composition::Rocky => Real::from_int(rng.gen_range((sea_lvl_int + 1_500)..=15_000)),
        Composition::OceanWorld => {
            // Ocean worlds have shallow peaks; allow the peak to be
            // close to but above sea level so a small archipelago
            // exists. Without land, the biosphere has nowhere to
            // deposit fuel.
            Real::from_int(rng.gen_range((sea_lvl_int + 500)..=(sea_lvl_int + 2_500)))
        }
        Composition::SubSurfaceOcean => Real::from_int(rng.gen_range(0..=2_000)),
        Composition::GaseousShell => Real::ZERO,
    };

    // Planet-scale realism: terrain relief is a real physical feature
    // of a real-sized planet, so its magnitude scales with the planet's
    // radius — a bigger world has proportionally grander relief (taller
    // peaks, deeper basins). `sea_level` and `terrain_peak` scale by the
    // same `radius` factor so their *ratio* is unchanged: the per-cell
    // glyph classifier in `habitability.rs` keys land bands off
    // fractions of `terrain_peak` (`peak_70` / `peak_40`) and water
    // bands off `sea_level`, so a uniform scale leaves the glyph
    // distribution invariant while the absolute relief grows. The
    // `init_planet` elevation builder scales its slopes / buffer by the
    // same radius so landmass *extent* (in cells) is also preserved.
    // Earth radius (1.0) is a no-op, keeping every Earth-radius seed
    // byte-identical. Applied after the RNG draws so the draw sequence
    // — and therefore byte-replay — is unchanged.
    let sea_level = sea_level * radius;
    let terrain_peak = terrain_peak * radius;

    // Peak position; init_planet wraps to grid bounds.
    let terrain_centre_q = rng.gen_range(0..32);
    let terrain_centre_r = rng.gen_range(0..32);

    // Atmosphere is sampled within the substrate's compatibility
    // window so the substrate-first contract holds. Aqueous picks
    // any-non-None; Ammoniacal picks Reducing/Thin; Hydrocarbon
    // picks Reducing/Hazy/Thin; Silicate is the only substrate
    // that admits Atmosphere::None (silicon-substrate life doesn't
    // need a fluid solvent atmosphere).
    let atmosphere = match metabolic_substrate {
        MetabolicSubstrate::Aqueous => match rng.gen_range(0..20) {
            0..=2 => Atmosphere::Thin,
            3..=9 => Atmosphere::Oxidising,
            10..=14 => Atmosphere::Reducing,
            _ => Atmosphere::Hazy,
        },
        MetabolicSubstrate::Ammoniacal => match rng.gen_range(0..10) {
            0..=6 => Atmosphere::Reducing,
            _ => Atmosphere::Thin,
        },
        MetabolicSubstrate::Hydrocarbon => match rng.gen_range(0..10) {
            0..=4 => Atmosphere::Reducing,
            5..=7 => Atmosphere::Hazy,
            _ => Atmosphere::Thin,
        },
        MetabolicSubstrate::Silicate => match rng.gen_range(0..10) {
            0..=2 => Atmosphere::None,
            3..=5 => Atmosphere::Thin,
            6..=7 => Atmosphere::Oxidising,
            _ => Atmosphere::Hazy,
        },
    };

    // Biosphere — never None now. The substrate guarantees the
    // chemistry is viable in the sampled (atmosphere, temperature)
    // window, so the biosphere always carries some life. Density
    // bands (Sparse / Lush / HyperBiodiverse) sampled freely.
    let biosphere = match rng.gen_range(0..20) {
        0..=5 => BiosphereClass::Sparse,      // 30%
        6..=13 => BiosphereClass::Lush,       // 40%
        _ => BiosphereClass::HyperBiodiverse, // 30%
    };

    let moon_count = rng.gen_range(0..=4);
    // Generate per-moon orbital configs. The first moon
    // (if present) is Earth-like (mass 100, period 28). Each
    // additional moon gets a different period (chosen from a
    // small Earth-system-inspired set) and a randomly varied
    // mass. Multi-moon planets get genuinely interfering tides;
    // the seed-driven `gen_range` keeps this deterministic.
    // Orbital eccentricity. Most worldgen samples produce
    // near-circular orbits (Earth-like); ~10% of planets get a
    // moderately-eccentric orbit (e ≤ 0.30). Ranges:
    //   - 70%: e ∈ [0, 0.05] (Earth-like + low-eccentricity)
    //   - 25%: e ∈ [0.05, 0.30] (Mars-Mercury-like)
    //   - 5%:  e ∈ [0.30, 0.60] (highly eccentric exoplanet)
    let orbital_eccentricity_x100 = match rng.gen_range(0..100) {
        0..=69 => rng.gen_range(0..=5),
        70..=94 => rng.gen_range(5..=30),
        _ => rng.gen_range(30..=60),
    };
    let moons: Vec<Moon> = (0..moon_count)
        .map(|i| {
            let period = match i {
                0 => 28, // Earth-Moon-like
                1 => 13, // Mars-Phobos-like (fast inner moon)
                2 => 79, // Jupiter-IO-like (slow outer moon)
                3 => 7,  // ultra-fast inner moon
                _ => 100,
            };
            let mass = rng.gen_range(20..=120);
            // Inclination derived deterministically from mass +
            // period so we don't disturb the RNG sequence other
            // worldgen paths depend on. Most planetary moons
            // cluster in [0, 100] (0°-10°); some captured / chaotic
            // moons go up to 300 (30°). Mass-period hash maps into
            // [0, 120].
            let inclination_deg_x10 = {
                let mass_u = u64::try_from(mass).unwrap_or(0);
                let p_u = u64::from(period);
                i32::try_from((mass_u ^ p_u) % 121).unwrap_or(0)
            };
            Moon {
                mass_relative_x100: mass,
                orbital_period_macros: period,
                inclination_deg_x10,
                // Per-moon eccentricity. Earth's moon ≈ 0.055, Io
                // ≈ 0.004. The sampler picks a small initial value
                // in `[0.00, 0.10]`; Item 19's per-tick damping then
                // shrinks it (Synchronous) or holds it
                // (Resonance-pumped). Sprint 5 Item 24 will sample
                // the locking_state per moon-planet pair — until
                // then the planet default `FreeRotator` gates the
                // slow damping path.
                eccentricity: Real::from_ratio(
                    rng.gen_range(0_i64..=10_i64),
                    100,
                ),
            }
        })
        .collect();

    // Surface pressure in Pa (Earth ≈ 101 325 Pa). Bands chosen so
    // Earth-like sits inside Oxidising at ~100 kPa.
    let surface_pressure = match atmosphere {
        Atmosphere::None => Real::ZERO,
        Atmosphere::Thin => Real::from_int(rng.gen_range(10_000..=30_000)),
        Atmosphere::Oxidising | Atmosphere::Reducing => {
            Real::from_int(rng.gen_range(60_000..=180_000))
        }
        Atmosphere::Hazy => Real::from_int(rng.gen_range(80_000..=300_000)),
    };

    let magnetosphere = match rng.gen_range(0..3) {
        0 => Magnetosphere::None,
        1 => Magnetosphere::Weak,
        _ => Magnetosphere::Strong,
    };

    // Stellar irradiance in W/m² at the planet (Earth ≈ 1361).
    let stellar_luminosity = Real::from_int(rng.gen_range(200..=3_000));

    // Planet orbital distance in AU (Earth = 1.0). Sampled per
    // substrate as a small AU band: aqueous worlds cluster near
    // 1 AU (the canonical liquid-water habitable distance);
    // ammoniacal / hydrocarbon worlds sit further out (cold
    // chemistry favours wider orbits); silicate worlds run hot
    // and close-in. The integer × 100 → ratio path keeps the
    // deterministic draw integer-only. Item 18 / P1.4: this is
    // consulted against `star.hz_inner_edge_au()` /
    // `hz_outer_edge_au()` in `habitability::cell_habitability`
    // so stellar-evolution-driven HZ migration can drift biome
    // classes outside the HZ as the star ages.
    let (orbit_au_lo_x100, orbit_au_hi_x100): (i64, i64) = match metabolic_substrate {
        // Aqueous: 0.85-1.15 AU (Earth-band).
        MetabolicSubstrate::Aqueous => (85, 115),
        // Ammoniacal: 1.5-3.5 AU (cold outer worlds).
        MetabolicSubstrate::Ammoniacal => (150, 350),
        // Hydrocarbon: 5.0-15.0 AU (Titan-band).
        MetabolicSubstrate::Hydrocarbon => (500, 1_500),
        // Silicate: 0.30-0.80 AU (hot inner worlds).
        MetabolicSubstrate::Silicate => (30, 80),
    };
    let orbit_au_x100 = rng.gen_range(orbit_au_lo_x100..=orbit_au_hi_x100);
    let orbital_distance_au = Real::from_ratio(orbit_au_x100, 100);

    // Spectral class. Realistic galactic frequencies skew
    // heavily toward M dwarfs, but the simulation biases the
    // distribution slightly toward middleweight stars (K/G/F)
    // so the typical seed lands a star with a habitable-zone
    // wide enough to host the sampled planet without driving
    // every run into M-dwarf-locked-rotator territory.
    //
    // Distribution (out of 1000):
    //   M: 600 (60%)
    //   K: 200 (20%)
    //   G: 120 (12%)
    //   F:  50 (5%)
    //   A:  30 (3%)
    let spectral_type = match rng.gen_range(0..1_000) {
        0..=599 => SpectralType::M,
        600..=799 => SpectralType::K,
        800..=919 => SpectralType::G,
        920..=969 => SpectralType::F,
        _ => SpectralType::A,
    };

    // Main-sequence age in Gyr. Sample uniformly within
    // `[0, 0.9 × lifetime)` so most planets sit comfortably in
    // the mid-MS band (no red-giant runs by default; tests
    // that need post-MS stars construct via `Star::with_age`).
    let lifetime_gyr = spectral_type.nominal_lifetime_gyr();
    // Express lifetime as an integer Gyr × 10 to give the age
    // sampler enough resolution without leaving Q32.32.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let lifetime_int_x10 = (lifetime_gyr.to_f64_for_display() * 10.0) as i64;
    let max_age_x10 = (lifetime_int_x10 * 9) / 10;
    let age_x10 = if max_age_x10 > 0 {
        rng.gen_range(0..max_age_x10)
    } else {
        0
    };
    let age_gyr = Real::from_ratio(age_x10, 10);

    // Build the star with age-adjusted luminosity. The
    // ZAMS bolometric irradiance at the planet is the
    // sampled `stellar_luminosity`; `Star::with_age` applies
    // the per-age scale factor + SED fractions.
    let star = Star::with_age(spectral_type, stellar_luminosity, age_gyr, lifetime_gyr);

    // Crust mineral profile. Biased toward Basaltic (the Earth-
    // adjacent default) so most seeds remain familiar; the four
    // exotic variants give the rare seed a genuinely different
    // tech-tree affordance per the "different worlds, different
    // sciences" project goal. Sub-surface oceans and gaseous
    // shells get Basaltic by convention since their
    // not-really-rocky composition makes the distinction moot.
    // Axial tilt in degrees. Sampled across [0, 90]; typical
    // habitable worlds cluster in 0-45.
    // Axial tilt is correlated with temperature_gradient
    // above. Pre-sampled `axial_tilt_pre_int` lifted into the Real.
    let axial_tilt_deg = Real::from_int(axial_tilt_pre_int);

    // Day length in hours. Earth ≈ 24; range 4-200 covers tidally-
    // locked-adjacent (4h) through slow-rotators (months-long days).
    // The range admits tidally-locked rotators
    // (day_length >= 1000 hours, ~42 Earth-days+ of "day"). The
    // is_tidally_locked predicate fires once a sampled value crosses
    // the threshold; ~5% of seeds end up tidally locked under the
    // weighted distribution.
    let day_length_hours = match rng.gen_range(0..20) {
        0 => Real::from_int(rng.gen_range(1_000..=4_000)), // 5% tidally-locked
        _ => Real::from_int(rng.gen_range(4..=200)),       // 95% normal rotation
    };

    // Months per orbital period. 8-16 brackets habitable
    // worlds' typical orbital fractions (Earth = 12; Mars-like
    // worlds skew higher; tighter-orbit hot rocks skew lower).
    // The 1-tick = 1-month cadence holds; the *number* of months
    // per year is now per-planet.
    let orbital_period_months = rng.gen_range(8..=16);

    // Crust biased per substrate. Hydrocarbon-substrate worlds
    // tilt heavily Hydrocarbon-crust (the buried fossil fuels feed
    // the biosphere); Silicate-substrate worlds tilt to Piezoelectric
    // / RareEarth (silicon-rich crystalline crusts). Aqueous and
    // Ammoniacal pick from the standard rocky distribution.
    let crust = match (composition, metabolic_substrate) {
        (Composition::SubSurfaceOcean | Composition::GaseousShell, _) => Crust::Basaltic,
        (_, MetabolicSubstrate::Hydrocarbon) => match rng.gen_range(0..10) {
            0..=6 => Crust::Hydrocarbon,
            _ => Crust::Basaltic,
        },
        (_, MetabolicSubstrate::Silicate) => match rng.gen_range(0..10) {
            0..=4 => Crust::Piezoelectric,
            5..=7 => Crust::RareEarth,
            _ => Crust::Basaltic,
        },
        (Composition::Rocky | Composition::OceanWorld, _) => match rng.gen_range(0..20) {
            0..=11 => Crust::Basaltic,
            12..=14 => Crust::Hydrocarbon,
            15..=16 => Crust::Piezoelectric,
            17..=18 => Crust::Ferrous,
            _ => Crust::RareEarth,
        },
    };

    // Continuous atmospheric composition. Sampled per
    // category-and-substrate: each (atmosphere, substrate) pair
    // has a baseline mass-fraction profile that the sampler then
    // perturbs by ±10% per channel before normalising. Aqueous
    // worlds get Earth-or-Mars-style mixtures; reducing worlds get
    // ammonia + methane proxies; hazy worlds get N₂ + CH₄ Titan-
    // style. The categorical label still summarises the result,
    // but the concrete fractions vary per seed.
    let atmospheric_composition =
        sample_atmospheric_composition(atmosphere, metabolic_substrate, &mut rng);

    // Continuous biosphere richness scalar. Categorical →
    // [low, high] ranges, clamped to [0, 1] after jitter.
    let biosphere_density = sample_biosphere_density(biosphere, radius, &mut rng);

    // Continuous crustal composition. Sampled per
    // (categorical-crust, substrate); ±10% jitter then normalise.
    let crustal_composition = sample_crustal_composition(crust, metabolic_substrate, &mut rng);

    // Sprint 5 Item 24: tidal-locking regime sampled from the
    // already-drawn moon + rotation fields. The classifier inspects
    // `moons[0]` (if present) for a close-massive-moon synchronous
    // capture, and the day_length / moon-orbital-period ratio for
    // Mercury-style spin-orbit resonances; ~5% of remaining planets
    // get a Resonance assignment by SplitMix64 jitter for variety.
    let locking_state = sample_locking_state(seed, &moons, day_length_hours);

    // -----------------------------------------------------------
    // Multi-continent + islands + lakes (planet-scale variety).
    //
    // The original sampler placed a single `(terrain_centre_q,
    // terrain_centre_r)` and `init_planet` built one multi-peak
    // landmass around it — every seed therefore yielded "one
    // continent, the same shape". This block extends that by drawing
    // additional continent centres, isolated island peaks, and lake
    // basins, with counts that scale with the planet's surface area
    // (∝ radius²).
    //
    // Determinism contract: at Earth-radius (1.0) the area_factor is
    // exactly 1.0 and every count threshold below evaluates to 0, so
    // no new RNG draws happen and the byte sequence for the existing
    // worldgen fields is preserved. For radius > 1.0 the bounds widen
    // and additional draws unfold; that's the intended divergence on
    // bigger worlds. These draws are appended at the end of the
    // worldgen sequence so they can never disturb the legacy fields
    // above (which is the whole point of byte-identical at radius=1).
    let area_factor = radius * radius;
    // Capacity formulae. All return 0 at radius=1.0 so no further
    // RNG draws fire on Earth-radius. Bounds grow with area_factor:
    //   - continents : up to 3 extras at area_factor ≈ 4 (radius 2).
    //   - islands    : up to 6 at area_factor ≈ 4.
    //   - lakes/cont : up to 3 at area_factor ≈ 4.
    let extra_continent_capacity = {
        // (area_factor - 1) × 2 → 0 at radius 1.0, 2 at radius 1.4,
        // 6 at radius 2.0 (then capped at 3 below). Integer-only via
        // raw Q32.32 conversion to stay inside the deterministic path.
        let f = (area_factor - Real::ONE) * Real::from_int(2);
        let n: i64 = f.raw().to_num();
        n.clamp(0, 3)
    };
    let island_capacity = {
        // (area_factor - 1) × 4 → 0 at radius 1.0, 4 at radius 1.4,
        // capped at 6 below.
        let f = (area_factor - Real::ONE) * Real::from_int(4);
        let n: i64 = f.raw().to_num();
        n.clamp(0, 6)
    };
    let lake_capacity_per_continent = {
        // (area_factor - 1) × 2 → 0 at radius 1.0, 2 at radius 1.4,
        // capped at 3 below.
        let f = (area_factor - Real::ONE) * Real::from_int(2);
        let n: i64 = f.raw().to_num();
        n.clamp(0, 3)
    };

    let grid_w: i32 = 32;
    let grid_h: i32 = 32;
    // Continent-shape envelope for the *primary* continent. Defaults
    // mirror the pre-multi-continent behaviour (5..=8 peaks, 100 %
    // spread) so a single-continent planet's primary stays
    // byte-equivalent under the new builder path. The `peak_count`
    // here counts the *secondaries*; the elevation builder pushes the
    // primary peak in addition.
    let primary_peak_count = 5;
    let primary_spread = 100;
    let mut continent_centres: Vec<crate::ContinentSeed> = vec![crate::ContinentSeed {
        centre_q: terrain_centre_q,
        centre_r: terrain_centre_r,
        peak_count: primary_peak_count,
        spread_pct_x100: primary_spread,
    }];

    // Additional continents. We require a minimum Manhattan
    // separation from every already-accepted centre so two
    // continents don't merge into one super-mass under
    // `init_planet`'s max-of-cones elevation field.
    let min_continent_separation = grid_w.min(grid_h) / 2; // ≈ 16 on a 32×32 sampling grid
    let max_attempts: u32 = 64;
    if extra_continent_capacity > 0 {
        // Draw how many extras land on this seed; range is [0, cap].
        let n_extras: i64 = rng.gen_range(0..=extra_continent_capacity);
        for _ in 0..n_extras {
            let mut last: (i32, i32) = (0, 0);
            let mut chosen: Option<(i32, i32)> = None;
            for _ in 0..max_attempts {
                let q = rng.gen_range(0..grid_w);
                let r = rng.gen_range(0..grid_h);
                last = (q, r);
                let far_enough = continent_centres.iter().all(|c| {
                    (q - c.centre_q).abs() + (r - c.centre_r).abs() >= min_continent_separation
                });
                if far_enough {
                    chosen = Some((q, r));
                    break;
                }
            }
            // Sample shape envelope for this extra continent: 3..=8
            // secondaries and an 80..=120 % spread multiplier on the
            // base buffer. Drawn after position so the primary's
            // sampling is independent of how many extras land.
            let peak_count = rng.gen_range(3_u32..=8);
            let spread = rng.gen_range(80_i32..=120);
            let (q, r) = chosen.unwrap_or(last);
            continent_centres.push(crate::ContinentSeed {
                centre_q: q,
                centre_r: r,
                peak_count,
                spread_pct_x100: spread,
            });
        }
    }

    // Islands — small isolated peaks away from any continent centre.
    // Same rejection-sampling pattern; the separation threshold here
    // is smaller (continents can't share islands' tightly-packed
    // arrangement) but still keeps every island a few cells away from
    // any continent so the island's tiny cone doesn't merge.
    let mut islands: Vec<(i32, i32)> = Vec::new();
    if island_capacity > 0 {
        let n_islands: i64 = rng.gen_range(0..=island_capacity);
        let min_island_separation = 4; // a few cells; islands must be off-continent
        for _ in 0..n_islands {
            let mut last: (i32, i32) = (0, 0);
            let mut chosen: Option<(i32, i32)> = None;
            for _ in 0..max_attempts {
                let q = rng.gen_range(0..grid_w);
                let r = rng.gen_range(0..grid_h);
                last = (q, r);
                let far_from_continents = continent_centres.iter().all(|c| {
                    (q - c.centre_q).abs() + (r - c.centre_r).abs() >= min_island_separation
                });
                let far_from_islands = islands
                    .iter()
                    .all(|&(iq, ir)| (q - iq).abs() + (r - ir).abs() >= min_island_separation);
                if far_from_continents && far_from_islands {
                    chosen = Some((q, r));
                    break;
                }
            }
            let (q, r) = chosen.unwrap_or(last);
            islands.push((q, r));
        }
    }

    // Lake basins — sampled per continent. The structural check
    // ("all hex neighbours are land") is enforced at carve time in
    // `init_planet`; the sampler just nominates candidate cells near
    // each continent centre and the carve drops any whose neighbours
    // aren't entirely on land. Per-continent draws (rather than
    // planet-wide) so each continent gets its own lake budget — this
    // is the geographic intent.
    let mut lakes: Vec<(i32, i32)> = Vec::new();
    if lake_capacity_per_continent > 0 {
        for c in &continent_centres {
            let n_lakes: i64 = rng.gen_range(0..=lake_capacity_per_continent);
            for _ in 0..n_lakes {
                // Lakes sit close to the continent centre — within a
                // small radius — so they have a fighting chance of
                // landing on land cells. Distance bounded by 3 cells
                // (Manhattan) so the carve check has good odds of
                // passing on a typical continent's interior.
                let dq: i32 = rng.gen_range(-3..=3);
                let dr: i32 = rng.gen_range(-3..=3);
                let q = (c.centre_q + dq).rem_euclid(grid_w);
                let r = (c.centre_r + dr).rem_euclid(grid_h);
                lakes.push((q, r));
            }
        }
    }

    // P1.4: one-shot biosphere-class downgrade if the sampled
    // orbital distance lands outside the star's habitable zone.
    // `cell_habitability` reads the same HZ edges per tick, but
    // a planet that started life well outside the HZ (e.g. a hot
    // silicate world inside the inner edge, or a cold hydrocarbon
    // world far past the outer edge) starts at a degraded biosphere
    // tier. The per-tick `cell_habitability` scalar then continues
    // to drift as the star ages and the HZ edges migrate.
    let biosphere = apply_hz_biosphere_drift(biosphere, &star, orbital_distance_au);

    Planet {
        seed,
        name,
        mass,
        radius,
        composition,
        mean_temperature,
        temperature_gradient,
        terrain_peak,
        terrain_centre_q,
        terrain_centre_r,
        continent_centres,
        islands,
        lakes,
        sea_level,
        atmosphere,
        atmospheric_composition,
        surface_pressure,
        biosphere,
        biosphere_density,
        magnetosphere,
        crust,
        crustal_composition,
        stellar_luminosity,
        orbital_distance_au,
        moon_count,
        moons,
        orbital_eccentricity_x100,
        axial_tilt_deg,
        day_length_hours,
        orbital_period_months,
        metabolic_substrate,
        // Per-seed substrate-chemistry perturbation in
        // [-0.05, +0.05]. RNG draw of an i64 in [-50, 50] divided
        // by 1000 — gives 5% relative drift on freeze/boil points.
        substrate_perturbation: Real::from_ratio(rng.gen_range(-50_i64..=50_i64), 1000),
        locking_state,
        star,
    }
}

/// One-shot biosphere-class downgrade for planets sampled outside
/// the habitable zone. Reads the star's HZ edges via
/// `habitability::hz_factor` against the planet's orbital distance;
/// if the factor falls below `0.5` the categorical `BiosphereClass`
/// drops by one tier:
///
/// - `HyperBiodiverse → Lush`
/// - `Lush → Sparse`
/// - `Sparse → None`
/// - `None → None`
///
/// P1.4: this is the worldgen-time application of the HZ migration
/// gate. The per-tick `cell_habitability` scalar continues to drift
/// with the star's age (since `Star::hz_inner_edge_au` /
/// `hz_outer_edge_au` read the age-adjusted bolometric luminosity);
/// the categorical biosphere class only drops once at sampling
/// because it's a `Copy` enum on the `Planet` and the per-tick
/// physics doesn't yet rewrite planet metadata. Per-tick categorical
/// drift is documented as a follow-up.
pub(crate) fn apply_hz_biosphere_drift(
    biosphere: crate::BiosphereClass,
    star: &crate::Star,
    orbital_distance_au: Real,
) -> crate::BiosphereClass {
    use crate::BiosphereClass;
    let factor = crate::habitability::hz_factor(star, orbital_distance_au);
    if factor >= Real::from_ratio(5, 10) {
        return biosphere;
    }
    match biosphere {
        BiosphereClass::HyperBiodiverse => BiosphereClass::Lush,
        BiosphereClass::Lush => BiosphereClass::Sparse,
        BiosphereClass::Sparse | BiosphereClass::None => BiosphereClass::None,
    }
}

/// Classify a planet's tidal-locking regime from its sampled moon
/// list + rotation rate. Worldgen runs this once at planet creation
/// (Sprint 5 Item 24); the per-tick dynamics in `tidal_locking.rs`
/// (Item 19) then reads `Planet::locking_state` to drive
/// eccentricity damping + sub-stellar-point behaviour.
///
/// Heuristic, in priority order:
///
/// 1. **Synchronous** if the planet has a close, massive first moon
///    (mass > 0.1 Earth-moon ratios and orbital period under 100
///    macro-steps / days). Such a moon's tidal torque locks the
///    planet's rotation to the moon's orbit, giving one face
///    perpetually moon-ward (the same dynamics that locked Earth's
///    moon to Earth, run in reverse for a sufficiently dominant
///    satellite).
///
/// 2. **Resonance { 3, 2 }** or **Resonance { 2, 3 }** if
///    `day_length / orbital_period_hours` lands close to 1.5 or
///    ~0.667 — Mercury-style spin-orbit resonances where the
///    rotation period sits at a small-integer ratio of the orbital
///    period. The "orbital period" here is the *first moon's*
///    period (Earth-Moon-like coupling); on moonless worlds the
///    resonance check is skipped.
///
/// 3. **Resonance { 3, 2 }** for ~5% of remaining planets via a
///    SplitMix64 jitter on the seed (variety: keeps a small slice
///    of the population in spin-orbit resonance regardless of the
///    deterministic heuristic).
///
/// 4. **FreeRotator** otherwise. Earth's regime — slow tidal
///    dissipation, no locked rotation.
///
/// The jitter uses a SplitMix64 finaliser salted with
/// `LOCKING_SALT` so the per-planet decision is deterministic in
/// the seed but doesn't touch the main `ChaCha20Rng` draw sequence
/// (byte-replay of the other worldgen fields stays stable).
pub(crate) fn sample_locking_state(
    seed: u64,
    moons: &[Moon],
    day_length_hours: Real,
) -> LockingState {
    // Rule 1 — close massive first moon → Synchronous.
    // Mass threshold: mass_relative_x100 > 10 corresponds to mass
    // ratio > 0.10 (Earth's moon = 100). Period threshold:
    // orbital_period_macros < 100 (≈ 100 days) is "close orbit".
    if let Some(first) = moons.first() {
        if first.mass_relative_x100 > 10 && first.orbital_period_macros < 100 {
            return LockingState::Synchronous;
        }
    }

    // Rule 2 — Mercury-style 3:2 or 2:3 spin-orbit resonance.
    // The "orbital period" is the first moon's period lifted into
    // hours via the 1 macro-step ≈ 1 day convention. On moonless
    // worlds we skip this check (no orbital coupling to compare
    // against the rotation rate).
    if let Some(first) = moons.first() {
        let orbital_period_hours =
            Real::from_int(i64::from(first.orbital_period_macros) * 24);
        if orbital_period_hours > Real::ZERO {
            let ratio = day_length_hours / orbital_period_hours;
            // Tolerance band ±5% around the target ratio. Wider
            // than strict equality (the sampled fields are integer
            // multiples of 1 hour / 1 day, so a 3:2 ratio rarely
            // falls exactly on 1.500); narrower than 10% so we
            // don't over-claim resonance.
            let three_halves = Real::from_ratio(3, 2);
            let two_thirds = Real::from_ratio(2, 3);
            let tol = Real::from_ratio(5, 100);
            if (ratio - three_halves).abs() < tol {
                return LockingState::Resonance { p: 3, q: 2 };
            }
            if (ratio - two_thirds).abs() < tol {
                return LockingState::Resonance { p: 2, q: 3 };
            }
        }
    }

    // Rule 3 — ~5% variety jitter via SplitMix64. Salted seed
    // → uniform u64; compare against `u64::MAX / 20` to fire
    // exactly 5% of the time on average without touching the
    // main ChaCha draw sequence.
    let jitter = splitmix64(seed.wrapping_add(LOCKING_SALT));
    if jitter < (u64::MAX / 20) {
        return LockingState::Resonance { p: 3, q: 2 };
    }

    // Rule 4 — default. Earth's regime.
    LockingState::FreeRotator
}

/// SplitMix64 finaliser. Standard four-step pattern (the same
/// shape used in `ecosystem::hgt::splitmix64_for` and
/// `physics::tectonics`). Deterministic, no RNG state — folds a
/// 64-bit input to a uniformly-distributed 64-bit output.
#[inline]
fn splitmix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Per-seed atmospheric composition. Each `(Atmosphere,
/// MetabolicSubstrate)` pair has a baseline mixture; the sampler
/// perturbs each channel by ±10% (additive) and renormalises so
/// the total stays at 1.0 (or 0.0 for `None`). Categorical-label
/// derivation via `Atmosphere::from_composition` recovers the
/// summary class from any sampled mixture.
///
/// Determinism: takes the same `ChaCha20Rng` already in
/// `sample_planet`'s draw sequence, so byte-replay holds.
pub(crate) fn sample_atmospheric_composition(
    atmosphere: Atmosphere,
    substrate: MetabolicSubstrate,
    rng: &mut ChaCha20Rng,
) -> AtmosphericComposition {
    if atmosphere == Atmosphere::None {
        return AtmosphericComposition::vacuum();
    }
    // Baseline mixtures keyed on (atmosphere, substrate). Numbers
    // are mass-fraction × 1000, normalised below.
    let baseline = match (atmosphere, substrate) {
        // Earth-style oxidising on aqueous: 78% N2, 21% O2, ~1% Ar/H2O traces.
        (Atmosphere::Oxidising, MetabolicSubstrate::Aqueous) => [780, 210, 4, 0, 0, 4, 0, 9, 3],
        // Mars-style thin: 95% CO2, 3% N2, 2% Ar.
        (Atmosphere::Thin, MetabolicSubstrate::Aqueous) => [30, 0, 950, 0, 0, 0, 0, 20, 0],
        // Venus-style reducing on aqueous: 96% CO2, 3.5% N2.
        (Atmosphere::Reducing, MetabolicSubstrate::Aqueous) => [35, 0, 960, 0, 0, 0, 0, 5, 0],
        // Hazy on aqueous: nitrogen-methane Titan-style.
        (Atmosphere::Hazy, MetabolicSubstrate::Aqueous) => [950, 0, 5, 40, 0, 0, 0, 0, 5],
        // Ammoniacal substrate: ammonia-and-hydrogen reducing chemistry.
        (Atmosphere::Reducing, MetabolicSubstrate::Ammoniacal) => {
            [200, 0, 50, 100, 500, 0, 100, 0, 50]
        }
        (Atmosphere::Hazy, MetabolicSubstrate::Ammoniacal) => [400, 0, 20, 200, 300, 0, 50, 0, 30],
        (Atmosphere::Thin, MetabolicSubstrate::Ammoniacal) => [400, 0, 100, 100, 300, 0, 50, 0, 50],
        (Atmosphere::Oxidising, MetabolicSubstrate::Ammoniacal) => {
            [400, 100, 100, 50, 250, 0, 50, 0, 50]
        }
        // Hydrocarbon substrate: methane-rich Titan / sub-surface ocean.
        (Atmosphere::Hazy, MetabolicSubstrate::Hydrocarbon) => [800, 0, 5, 150, 5, 0, 30, 0, 10],
        (Atmosphere::Thin, MetabolicSubstrate::Hydrocarbon) => [600, 0, 50, 250, 50, 0, 30, 0, 20],
        (Atmosphere::Reducing, MetabolicSubstrate::Hydrocarbon) => {
            [400, 0, 100, 300, 100, 0, 80, 0, 20]
        }
        (Atmosphere::Oxidising, MetabolicSubstrate::Hydrocarbon) => {
            [600, 50, 50, 200, 50, 0, 30, 0, 20]
        }
        // Silicate substrate: high-temperature exotic chemistry; default
        // is hot CO₂ + sulphates (lumped in `other`).
        (Atmosphere::Reducing, MetabolicSubstrate::Silicate) => [50, 0, 800, 0, 0, 0, 0, 0, 150],
        (Atmosphere::Hazy, MetabolicSubstrate::Silicate) => [200, 0, 600, 0, 0, 0, 0, 0, 200],
        (Atmosphere::Thin, MetabolicSubstrate::Silicate) => [50, 0, 800, 0, 0, 0, 0, 50, 100],
        (Atmosphere::Oxidising, MetabolicSubstrate::Silicate) => {
            [400, 200, 200, 0, 0, 0, 0, 50, 150]
        }
        (Atmosphere::None, _) => [0, 0, 0, 0, 0, 0, 0, 0, 0],
    };
    // Per-channel ±10% perturbation, then normalise.
    let mut raw: [i64; 9] = [0; 9];
    for (i, &b) in baseline.iter().enumerate() {
        if b == 0 {
            raw[i] = 0;
            continue;
        }
        let jitter_pct = rng.gen_range(-10_i64..=10_i64);
        let jittered = b + (b * jitter_pct) / 100;
        raw[i] = jittered.max(0);
    }
    let sum: i64 = raw.iter().sum();
    if sum <= 0 {
        return AtmosphericComposition::vacuum();
    }
    // Normalise so fractions sum to 1.0 (within Q32.32 rounding).
    AtmosphericComposition {
        n2: Real::from_ratio(raw[0], sum),
        o2: Real::from_ratio(raw[1], sum),
        co2: Real::from_ratio(raw[2], sum),
        ch4: Real::from_ratio(raw[3], sum),
        nh3: Real::from_ratio(raw[4], sum),
        h2o: Real::from_ratio(raw[5], sum),
        h2: Real::from_ratio(raw[6], sum),
        ar: Real::from_ratio(raw[7], sum),
        other: Real::from_ratio(raw[8], sum),
    }
}

/// Per-seed biosphere richness scalar. Categorical →
/// continuous mapping with ±0.10 jitter, clamped to `[0, 1]`.
/// Determinism: takes the same `ChaCha20Rng` already in
/// `sample_planet`'s draw sequence.
///
/// Planet-scale realism (habitat generation): a larger planet offers
/// proportionally more — and more varied — habitable surface, so its
/// biosphere fills out a larger fraction of that surface. The richness
/// scalar is therefore nudged up by an area-derived factor before the
/// `[0, 1]` clamp: a bigger world's habitat *extent / diversity* rises
/// toward saturation. The clamp at 1.0 keeps the lift sub-quadratic —
/// it can't run the richness away — so planet-wide carrying capacity
/// still scales ≈ `radius²` (owned by `planet_area_factor` on the civ),
/// not `radius⁴`. Earth radius (1.0) is a no-op, leaving every
/// Earth-radius seed's density byte-identical; only larger worlds gain
/// extra habitat coverage. Applied after the RNG draw so byte-replay of
/// the draw sequence is unchanged.
pub(crate) fn sample_biosphere_density(
    class: BiosphereClass,
    radius: Real,
    rng: &mut ChaCha20Rng,
) -> Real {
    let baseline = match class {
        BiosphereClass::None => 0,
        BiosphereClass::Sparse => 30,
        BiosphereClass::Lush => 60,
        BiosphereClass::HyperBiodiverse => 90,
    };
    let jitter = rng.gen_range(-10_i64..=10_i64);
    let raw = (baseline + jitter).clamp(0, 100);
    let base_density = Real::from_ratio(raw, 100);
    // Area-derived habitat-extent lift. Surface area ∝ radius², but we
    // apply the milder linear `radius` factor (floored at 1.0 so smaller
    // worlds aren't penalised below their sampled richness) so the
    // post-clamp density rises gently with planet size rather than
    // saturating instantly on any super-Earth.
    let area_lift = radius.max(Real::ONE);
    (base_density * area_lift).min(Real::ONE)
}

/// Per-seed crustal composition. Sampled per (categorical-crust,
/// substrate) with ±10% per-channel jitter then normalised. Each
/// `Crust` archetype has a baseline mineral profile that the sampler
/// perturbs to give per-seed variability.
pub(crate) fn sample_crustal_composition(
    crust: Crust,
    substrate: MetabolicSubstrate,
    rng: &mut ChaCha20Rng,
) -> CrustalComposition {
    // Baseline mineral mixtures per categorical crust × substrate.
    // Numbers are mass-fraction × 1000, normalised below.
    // Channels: silicate, hydrocarbon, piezoelectric, ferrous,
    // rare_earth, ice, other.
    let baseline: [i64; 7] = match (crust, substrate) {
        // Earth-like balanced crust.
        (Crust::Basaltic, MetabolicSubstrate::Aqueous) => [600, 5, 50, 80, 5, 10, 250],
        (Crust::Basaltic, _) => [550, 20, 50, 80, 10, 40, 250],
        // Hydrocarbon-rich (coal / oil / methane clathrate).
        (Crust::Hydrocarbon, MetabolicSubstrate::Hydrocarbon) => [350, 250, 30, 50, 10, 100, 210],
        (Crust::Hydrocarbon, _) => [450, 100, 30, 60, 10, 50, 300],
        // Piezoelectric (quartz / tourmaline).
        (Crust::Piezoelectric, MetabolicSubstrate::Silicate) => [300, 5, 400, 50, 50, 5, 190],
        (Crust::Piezoelectric, _) => [400, 5, 300, 60, 30, 10, 195],
        // Ferrous (iron / nickel / transition metal).
        (Crust::Ferrous, _) => [350, 5, 30, 350, 30, 10, 225],
        // Rare-earth + lanthanide-rich.
        (Crust::RareEarth, MetabolicSubstrate::Silicate) => [350, 5, 100, 80, 200, 5, 260],
        (Crust::RareEarth, _) => [400, 10, 80, 80, 150, 10, 270],
    };
    // Per-channel ±10% perturbation, then normalise.
    let mut raw: [i64; 7] = [0; 7];
    for (i, &b) in baseline.iter().enumerate() {
        if b == 0 {
            raw[i] = 0;
            continue;
        }
        let jitter_pct = rng.gen_range(-10_i64..=10_i64);
        let jittered = b + (b * jitter_pct) / 100;
        raw[i] = jittered.max(0);
    }
    let sum: i64 = raw.iter().sum();
    if sum <= 0 {
        return CrustalComposition::empty();
    }
    CrustalComposition {
        silicate: Real::from_ratio(raw[0], sum),
        hydrocarbon: Real::from_ratio(raw[1], sum),
        piezoelectric: Real::from_ratio(raw[2], sum),
        ferrous: Real::from_ratio(raw[3], sum),
        rare_earth: Real::from_ratio(raw[4], sum),
        ice: Real::from_ratio(raw[5], sum),
        other: Real::from_ratio(raw[6], sum),
    }
}
