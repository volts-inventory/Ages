//! `Star` — the host star a planet orbits. Captures spectral
//! type, full SED breakdown (bolometric / EUV / UV / visible / IR
//! flux at the planet's distance), and main-sequence age so
//! downstream code can model:
//!
//! 1. **Per-type flare rates** — M dwarfs flare ~100× as often
//!    as G dwarfs; high-mass A dwarfs are nearly inert. Drives
//!    catastrophe trigger frequency and atmospheric escape.
//! 2. **Habitable-zone edge migration** — as a main-sequence
//!    star ages, its bolometric luminosity drifts up (the "faint
//!    young sun" effect, run in reverse). The HZ inner edge
//!    migrates outward over Gyr. A planet that was habitable
//!    early can be pushed inside the inner edge before MS end.
//! 3. **Red-giant brightening** — at `age >= 0.95 × lifetime`,
//!    the star inflates and brightens toward ~1000× MS
//!    luminosity over the final ~5% of its lifetime. Inner
//!    planets become uninhabitable; the HZ migrates far beyond
//!    1 AU equivalents.
//!
//! All quantities are in SI units except flux ratios, which are
//! dimensionless multipliers of the Solar baseline. The `Star`
//! is sampled per planet by worldgen; same seed → same star.

use sim_arith::Real;

/// Stellar spectral class on the main sequence. Five archetypes
/// keyed by surface temperature, mass, and luminosity:
///
/// - `M`: red dwarf. ~0.08-0.45 solar mass, T ≈ 2400-3700 K,
///   L ≈ 0.001-0.08 Lsun. Very long lifetime (10-1000 Gyr).
///   Frequent flares (100× G). Most common stellar class in
///   the galaxy.
/// - `K`: orange dwarf. ~0.45-0.8 solar mass, T ≈ 3700-5200 K,
///   L ≈ 0.08-0.6 Lsun. Lifetime 15-30 Gyr. Moderately active.
/// - `G`: yellow dwarf (Sun-like). ~0.8-1.04 solar mass,
///   T ≈ 5200-6000 K, L ≈ 0.6-1.5 Lsun. Lifetime ~10 Gyr.
///   Baseline activity.
/// - `F`: yellow-white. ~1.04-1.4 solar mass, T ≈ 6000-7500 K,
///   L ≈ 1.5-5 Lsun. Lifetime ~3-7 Gyr. Less active than G.
/// - `A`: white. ~1.4-2.1 solar mass, T ≈ 7500-10000 K,
///   L ≈ 5-25 Lsun. Lifetime ~0.5-3 Gyr. Nearly inert
///   (radiative envelope, no convective dynamo).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpectralType {
    M,
    K,
    G,
    F,
    A,
}

impl SpectralType {
    /// Snake-case tag for protocol/event payloads.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            SpectralType::M => "m",
            SpectralType::K => "k",
            SpectralType::G => "g",
            SpectralType::F => "f",
            SpectralType::A => "a",
        }
    }

    /// Per-type relative flare rate. G-dwarf is baseline `1.0`.
    /// Calibration:
    ///
    /// - M dwarf: 100× G — convective envelope + strong magnetic
    ///   dynamo + small surface area means a single spot
    ///   covers a large fraction of the disk; flares are
    ///   constant. Drives atmospheric stripping on close-in
    ///   M-dwarf habitable-zone planets.
    /// - K dwarf: 10× G — convective envelope still active but
    ///   spots cover less surface fraction.
    /// - G dwarf: 1.0 baseline — Sun-like activity. ~1 X-class
    ///   flare per month at solar maximum.
    /// - F dwarf: 0.3× G — thinner convective envelope, weaker
    ///   dynamo.
    /// - A dwarf: 0.1× G — radiative envelope, no convective
    ///   dynamo, residual activity from rotation only.
    ///
    /// Returns a `Real` multiplier in `[0, ∞)`.
    #[must_use]
    pub fn flare_rate_per_tick(self) -> Real {
        match self {
            SpectralType::M => Real::from_int(100),
            SpectralType::K => Real::from_int(10),
            SpectralType::G => Real::ONE,
            SpectralType::F => Real::from_ratio(3, 10),
            SpectralType::A => Real::from_ratio(1, 10),
        }
    }

    /// Nominal main-sequence lifetime in Gyr per spectral type.
    /// Real stars span a range within each class; these are the
    /// class-mean values used by worldgen. M dwarfs are pinned
    /// at 1000 Gyr (effectively immortal for the simulation;
    /// real M dwarfs live 100-1000 Gyr).
    #[must_use]
    pub fn nominal_lifetime_gyr(self) -> Real {
        match self {
            SpectralType::M => Real::from_int(1_000),
            SpectralType::K => Real::from_int(25),
            SpectralType::G => Real::from_int(10),
            SpectralType::F => Real::from_int(5),
            SpectralType::A => Real::from_int(2),
        }
    }

    /// Nominal main-sequence bolometric luminosity in units of
    /// Solar luminosity (Lsun = 3.828e26 W). M dwarfs span
    /// 0.001-0.08; we pin a representative 0.04 here. The
    /// effective stellar irradiance at the planet depends on
    /// orbital distance, which is sampled per planet.
    #[must_use]
    pub fn nominal_luminosity_solar(self) -> Real {
        match self {
            // 0.04 Lsun
            SpectralType::M => Real::from_ratio(4, 100),
            // 0.4 Lsun
            SpectralType::K => Real::from_ratio(4, 10),
            // 1.0 Lsun (Solar baseline)
            SpectralType::G => Real::ONE,
            // 2.5 Lsun
            SpectralType::F => Real::from_ratio(25, 10),
            // 12 Lsun
            SpectralType::A => Real::from_int(12),
        }
    }

    /// SED breakdown — fraction of bolometric luminosity emitted
    /// in each of (EUV, UV, visible, IR) bands. Sums to ~1.0.
    /// Calibration follows the blackbody curve for each class's
    /// surface temperature, with the EUV / UV channels boosted
    /// for cool stars to capture chromospheric / coronal emission
    /// that the photospheric blackbody alone underestimates.
    ///
    /// Channels (each as a `Real` fraction of bolometric):
    /// - `euv`: ionising 10-100 nm.
    /// - `uv`:  near-/far-UV 100-400 nm.
    /// - `visible`: 400-700 nm.
    /// - `ir`:  > 700 nm.
    ///
    /// M dwarfs emit most flux as IR but a disproportionate
    /// fraction in EUV due to extreme chromospheric activity;
    /// A dwarfs peak in UV/visible and emit little IR.
    #[must_use]
    pub fn sed_fractions(self) -> SedFractions {
        match self {
            // M dwarf: ~85% IR, 10% visible, 2% UV, 3% EUV
            // (EUV fraction inflated vs blackbody by chromosphere).
            SpectralType::M => SedFractions {
                euv: Real::from_ratio(3, 100),
                uv: Real::from_ratio(2, 100),
                visible: Real::from_ratio(10, 100),
                ir: Real::from_ratio(85, 100),
            },
            // K dwarf: ~60% IR, 33% visible, 5% UV, 2% EUV.
            SpectralType::K => SedFractions {
                euv: Real::from_ratio(2, 100),
                uv: Real::from_ratio(5, 100),
                visible: Real::from_ratio(33, 100),
                ir: Real::from_ratio(60, 100),
            },
            // G dwarf (Sun): ~50% IR, 41% visible, 8% UV, 1% EUV.
            SpectralType::G => SedFractions {
                euv: Real::from_ratio(1, 100),
                uv: Real::from_ratio(8, 100),
                visible: Real::from_ratio(41, 100),
                ir: Real::from_ratio(50, 100),
            },
            // F dwarf: ~35% IR, 47% visible, 16% UV, 2% EUV.
            SpectralType::F => SedFractions {
                euv: Real::from_ratio(2, 100),
                uv: Real::from_ratio(16, 100),
                visible: Real::from_ratio(47, 100),
                ir: Real::from_ratio(35, 100),
            },
            // A dwarf: ~15% IR, 50% visible, 30% UV, 5% EUV.
            SpectralType::A => SedFractions {
                euv: Real::from_ratio(5, 100),
                uv: Real::from_ratio(30, 100),
                visible: Real::from_ratio(50, 100),
                ir: Real::from_ratio(15, 100),
            },
        }
    }
}

/// SED energy fractions across the four physically-relevant
/// bands. Each channel is a `Real` in `[0, 1]`; the four sum
/// to ~1.0 modulo small rounding.
#[derive(Debug, Clone, Copy)]
pub struct SedFractions {
    pub euv: Real,
    pub uv: Real,
    pub visible: Real,
    pub ir: Real,
}

/// The host star a planet orbits. Sampled per seed; same seed
/// → same star. Worldgen builds this alongside `Planet` and
/// embeds it in `Planet::star`.
///
/// SI units throughout. Flux channels are in **W/m² at the
/// planet's orbit** (so they integrate against the planet's
/// cross-section directly). `bolometric_luminosity` is the
/// total irradiance at the planet (Sun-on-Earth ≈ 1361 W/m²);
/// the four SED channels (`euv_flux`, `uv_flux`, `visible_flux`,
/// `ir_flux`) sum to it (modulo rounding).
#[derive(Debug, Clone, Copy)]
pub struct Star {
    /// Spectral class on the main sequence.
    pub spectral_type: SpectralType,
    /// Total irradiance at the planet's orbit in W/m².
    /// At MS start this equals the orbital-distance-adjusted
    /// luminosity; it drifts up over MS age and ramps to
    /// 1000× during the red-giant phase.
    pub bolometric_luminosity: Real,
    /// EUV flux at planet (10-100 nm), W/m². Drives
    /// hydrodynamic atmospheric escape (Item 17 / 18a).
    pub euv_flux: Real,
    /// UV flux at planet (100-400 nm), W/m².
    pub uv_flux: Real,
    /// Visible flux at planet (400-700 nm), W/m². Drives
    /// photosynthesis-equivalent biosphere energy intake.
    pub visible_flux: Real,
    /// IR flux at planet (> 700 nm), W/m². Drives thermal
    /// equilibrium temperature.
    pub ir_flux: Real,
    /// Star's current main-sequence age in Gyr. 0.0 = ZAMS;
    /// increases with sim time on geological scales. Used by
    /// `hz_inner_edge_au` and `bolometric_at_age`.
    pub main_sequence_age_gyr: Real,
    /// Total main-sequence lifetime in Gyr. After
    /// `main_sequence_age_gyr >= main_sequence_lifetime_gyr`
    /// the star is post-MS; the red-giant ramp applies once
    /// `age >= 0.95 × lifetime`.
    pub main_sequence_lifetime_gyr: Real,
}

impl Star {
    /// Construct a fresh main-sequence star of the given
    /// spectral type at zero age. Flux channels are derived
    /// from `bolometric_at_planet` and the per-class SED
    /// fractions; `main_sequence_lifetime_gyr` comes from the
    /// per-class nominal value.
    ///
    /// `bolometric_at_planet` is the planet-orbit irradiance in
    /// W/m² (Sun-on-Earth ≈ 1361). The four SED channels are
    /// derived from this baseline by per-class fractions.
    #[must_use]
    pub fn new(spectral_type: SpectralType, bolometric_at_planet: Real) -> Self {
        let sed = spectral_type.sed_fractions();
        Self {
            spectral_type,
            bolometric_luminosity: bolometric_at_planet,
            euv_flux: bolometric_at_planet.saturating_mul(sed.euv),
            uv_flux: bolometric_at_planet.saturating_mul(sed.uv),
            visible_flux: bolometric_at_planet.saturating_mul(sed.visible),
            ir_flux: bolometric_at_planet.saturating_mul(sed.ir),
            main_sequence_age_gyr: Real::ZERO,
            main_sequence_lifetime_gyr: spectral_type.nominal_lifetime_gyr(),
        }
    }

    /// Construct a star with explicit age and lifetime — used
    /// by tests that need to probe specific points in stellar
    /// evolution (e.g. `red_giant_phase_renders_inner_planets_uninhabitable`).
    #[must_use]
    pub fn with_age(
        spectral_type: SpectralType,
        bolometric_at_planet_zams: Real,
        age_gyr: Real,
        lifetime_gyr: Real,
    ) -> Self {
        let mut star = Self::new(spectral_type, bolometric_at_planet_zams);
        star.main_sequence_age_gyr = age_gyr;
        star.main_sequence_lifetime_gyr = lifetime_gyr;
        // Apply the age-dependent luminosity drift + red-giant
        // ramp by recomputing the bolometric channels.
        let scale = bolometric_scale_at_age(age_gyr, lifetime_gyr);
        let bol = bolometric_at_planet_zams.saturating_mul(scale);
        let sed = spectral_type.sed_fractions();
        star.bolometric_luminosity = bol;
        star.euv_flux = bol.saturating_mul(sed.euv);
        star.uv_flux = bol.saturating_mul(sed.uv);
        star.visible_flux = bol.saturating_mul(sed.visible);
        star.ir_flux = bol.saturating_mul(sed.ir);
        star
    }

    /// Per-tick flare rate multiplier. Pulled from
    /// `SpectralType::flare_rate_per_tick`. Lifted to the
    /// `Star` level so downstream code (catastrophe triggers,
    /// EM events) can read the star directly without re-routing
    /// through the spectral class.
    #[must_use]
    pub fn flare_rate_per_tick(&self) -> Real {
        self.spectral_type.flare_rate_per_tick()
    }

    /// Whether the star is past the red-giant ignition
    /// threshold (`age >= 0.95 × lifetime`). After this point
    /// `bolometric_luminosity` ramps toward 1000× ZAMS over the
    /// final 5% of lifetime, and the habitable zone migrates
    /// far out beyond any MS-era orbital distance.
    #[must_use]
    pub fn is_red_giant(&self) -> bool {
        // 0.95 × lifetime, integer-only to keep determinism.
        let threshold = self
            .main_sequence_lifetime_gyr
            .saturating_mul(Real::from_ratio(95, 100));
        self.main_sequence_age_gyr >= threshold
    }

    /// Inner edge of the habitable zone in AU, given the star's
    /// current `bolometric_luminosity` (already age-adjusted)
    /// referenced to a Sun-on-Earth baseline of 1361 W/m² at
    /// 1 AU.
    ///
    /// HZ inner edge scales as `sqrt(L / Lsun_at_1AU)` (the
    /// classical Kasting moist-greenhouse boundary at 0.95 AU
    /// for the present-day Sun scales with the square root of
    /// luminosity). Approximated here as `0.95 AU × sqrt(L / 1361)`.
    ///
    /// The boundary migrates **outward** as the star ages
    /// (luminosity drifts up). At red-giant onset (×1000
    /// luminosity ramp), the inner edge passes well beyond any
    /// MS-era orbital distance.
    #[must_use]
    pub fn hz_inner_edge_au(&self) -> Real {
        // 0.95 AU × sqrt(L / 1361). Use sim_arith::math::sqrt
        // on the ratio L / 1361.
        let ratio = self.bolometric_luminosity / Real::from_int(1_361);
        let sqrt_ratio = sim_arith::transcendental::sqrt(ratio);
        Real::from_ratio(95, 100).saturating_mul(sqrt_ratio)
    }

    /// Outer edge of the habitable zone in AU, sized for the
    /// classical Kasting maximum-greenhouse boundary at 1.37 AU
    /// for the present-day Sun. Scales with `sqrt(L / Lsun)`.
    #[must_use]
    pub fn hz_outer_edge_au(&self) -> Real {
        let ratio = self.bolometric_luminosity / Real::from_int(1_361);
        let sqrt_ratio = sim_arith::transcendental::sqrt(ratio);
        Real::from_ratio(137, 100).saturating_mul(sqrt_ratio)
    }
}

/// Bolometric luminosity scale at a given main-sequence age,
/// expressed as a multiplier of the ZAMS (zero-age) value.
///
/// - During MS (`age < 0.95 × lifetime`): linear drift from
///   1.0× to ~1.4× over the full MS lifetime. Captures the
///   faint-young-sun → bright-old-sun trend.
/// - During red-giant ramp (`0.95 × lifetime ≤ age < lifetime`):
///   ramps from ~1.4× toward 1000× over the final 5% of lifetime.
/// - Beyond MS: capped at 1000× (the star has left the MS).
#[must_use]
pub fn bolometric_scale_at_age(age_gyr: Real, lifetime_gyr: Real) -> Real {
    if lifetime_gyr <= Real::ZERO {
        return Real::ONE;
    }
    let frac = age_gyr / lifetime_gyr;
    let ms_end = Real::from_ratio(95, 100);
    if frac < ms_end {
        // MS drift: 1.0 at frac=0 → 1.4 at frac=0.95.
        // scale = 1 + (0.4 / 0.95) × frac.
        // Computed as 1 + (40 × frac) / 95 in integer-fraction.
        let drift = frac
            .saturating_mul(Real::from_int(40))
            .saturating_div(Real::from_int(95));
        Real::ONE.saturating_add(drift)
    } else if frac < Real::ONE {
        // Red-giant ramp: at frac=0.95 → 1.4×;
        // at frac=1.0 → 1000×.
        // Linear in (frac - 0.95) / 0.05.
        let into_ramp = frac.saturating_sub(ms_end);
        // 0.05 in Real.
        let ramp_width = Real::from_ratio(5, 100);
        let t = into_ramp.saturating_div(ramp_width);
        // scale = 1.4 + (1000 - 1.4) × t.
        let span = Real::from_ratio(9_986, 10);
        let base = Real::from_ratio(14, 10);
        base.saturating_add(t.saturating_mul(span))
    } else {
        // Past MS end — cap at 1000×.
        Real::from_int(1_000)
    }
}

// Helper trait impl on Real: saturating_div / saturating_add.
// Real already provides saturating_mul, saturating_add,
// saturating_sub via inherent impls; `Div` is also implemented
// via the std `Div` trait. Wrap to give consistent naming used
// above.
trait RealOps {
    fn saturating_div(self, rhs: Self) -> Self;
}

impl RealOps for Real {
    fn saturating_div(self, rhs: Real) -> Real {
        // Real already supports `/` via std::ops::Div. Wrap so
        // the call sites in this module read uniformly with the
        // other saturating_* operations on Real. Behaviour:
        // delegate to the std Div impl.
        self / rhs
    }
}
