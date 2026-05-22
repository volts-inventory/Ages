//! Radiative-balance law.
//!
//! Real-planet temperatures don't come from "diffuse this initial
//! gradient forever" — they come from stellar input minus blackbody
//! emission, modulated by latitude / atmosphere / surface absorption.
//! Earlier the sim initialised cell temperatures to a latitude-driven
//! gradient at `init_planet` and then only diffused; over thousands
//! of ticks heat conduction smooths the gradient toward planet-wide
//! mean (a slowly-cooling-toward-uniform world rather than a
//! climatologically realistic one).
//!
//! This law fixes that by relaxing each cell's temperature toward a
//! latitude-dependent equilibrium temperature each tick:
//!
//! ```text
//!   T_eq[row] = ( S_star * cos_lat * (1 - albedo) / sigma )^(1/4) + greenhouse
//!   dT/dt    += (T_eq[row] - T[cell]) * relaxation_rate
//! ```
//!
//! `cos_lat` derives from grid row distance from the equator.
//! `S_star` is the planet's stellar irradiance. `albedo` and
//! `greenhouse` come from atmosphere class. `sigma` is the Stefan-
//! Boltzmann constant.
//!
//! The relaxation rate is small (per-tick contribution ~1-2% of the
//! gap) so heat-conduction (smoothing) and radiation (sourcing) reach
//! a steady state where polar cells stay cold and equatorial cells
//! stay warm — matching real planetary climatology shape.
//!
//! Seasonal swing layered on top: the sub-solar latitude shifts
//! ±`axial_tilt_rows` over a year-long cycle, so summer hemispheres
//! warm and winter hemispheres cool. The per-row `T_eq` becomes a
//! per-(row, season) table indexed by the planet's `macro_step` clock.
//! Per-cell day/night insolation (diurnal cycling) is still a
//! deferred follow-up.
//!
//! ## Per-substance greenhouse (Sprint 3 Item 14)
//!
//! Earlier the greenhouse offset was a single planet-wide constant
//! (`greenhouse_k`, derived from atmosphere class) baked into the
//! per-(row, season) equilibrium table at planet build. That was
//! linear-in-nothing — atmospheric composition never changed the
//! greenhouse response, so a Venus-style runaway (water vapour
//! feeding back on temperature, raising the saturation cap, which
//! raises vapour, which raises greenhouse, …) was structurally
//! impossible.
//!
//! Sprint 3 Item 14 keeps that planet-wide baseline (still folded
//! into the table) and adds a per-cell dynamic contribution at
//! integrate time:
//!
//! ```text
//!   greenhouse[cell] = h2o_vapour[cell] * H2O_GREENHOUSE_K
//!                    + co2[cell]        * CO2_GREENHOUSE_K
//!                    + ch4[cell]        * CH4_GREENHOUSE_K
//!   T_eq[cell] = (t_eq_base[row][season] * day_factor) + greenhouse[cell]
//! ```
//!
//! H2O is the Clausius-Clapeyron-coupled channel — its cap (see
//! [`crate::hydrology::saturation_vapour_cap`]) grows quartically in
//! T/T_ref, so as a cell warms the vapour ceiling lifts, evaporation
//! pushes the cell toward that new ceiling, and the H2O greenhouse
//! term grows superlinearly with T → positive feedback. Above a
//! threshold (Komabayashi-Ingersoll-like) the loop diverges and the
//! cell slides into a Venus-style runaway.
//!
//! CO2 is linear (long-lived, no T-coupling). CH4 is short-lived
//! (photolysis); the law decays it by a small per-tick factor
//! ([`ch4_decay_per_tick`]).

use crate::albedo::{albedo_radiation_factor, effective_albedo_slice};
use crate::chemistry::Substance;
use crate::clouds::{
    cirrus_greenhouse_strength, dry_adiabatic_lapse_rate, stratus_greenhouse_k, CloudType,
    REFERENCE_CIRRUS_ALTITUDE_M,
};
use crate::laws::Law;
use crate::state::PhysicsState;
use sim_arith::transcendental::{cos, exp, half_pi, ln, pow, sin};
use sim_arith::Real;

/// Tidal-locking regime as seen by the radiation law. Mirrors the
/// `LockingState` enum in `sim-world` but lives here so `sim-physics`
/// doesn't depend on `sim-world` (the dependency points the other
/// way). `Synchronous` flips on the per-cell day-night gradient
/// (substellar point fixed); `Other` falls back to the per-row /
/// diurnal-amplitude path used for free rotators and resonances.
///
/// See `Radiation::with_locking` for the wire-in surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockingMode {
    /// Synchronous (1:1) tidal lock. Sub-stellar point fixed at
    /// the supplied (lat, lon) in fractional turns; per-cell day-
    /// night gradient computed each tick from the great-circle
    /// angle to that point.
    Synchronous,
    /// Anything not 1:1-locked — free rotator or `p:q` resonance.
    /// The sub-stellar longitude rotates over time, so on a
    /// macro-step timescale the day-night gradient washes out and
    /// the existing per-row table (annual + zonal mean) is the
    /// right physics.
    Other,
}

/// Night-side residual absorption fraction for a synchronous
/// world. Stellar input drops to ~5 % of full at the antistellar
/// point — atmospheric IR redistribution + some scattered light
/// from the day-night limb keeps the dark side from going to a
/// true 0 K. Matches the value in `post-implementation-fixes.md`
/// P1.5 spec.
fn night_factor() -> Real {
    Real::from_ratio(5, 100)
}

/// Per-unit-density greenhouse coefficient for water vapour.
/// Calibrated against [`crate::hydrology::saturation_vapour_cap`],
/// which lets vapour density rise into the tens of thousands as
/// T approaches the cap reference. The feedback slope
/// `d(T_eq)/dT = K × d(cap)/dT = K × 4 × cap / T` crosses unity
/// (the K-I runaway threshold) when `K > T / (4 × cap(T))`. With
/// `K = 0.002` the threshold sits around T ~ 350-400 K —
/// temperate Earth-like cells stay below it (loop converges to a
/// modest equilibrium) and a hot seed pushes past it (loop
/// diverges into a Venus-style runaway, plateauing only when
/// [`greenhouse_cap_scaled`] binds).
fn h2o_greenhouse_k() -> Real {
    Real::from_ratio(2, 1_000)
}

/// Per-unit-density greenhouse coefficient for CO2. Per-molecule
/// CO2 is a stronger IR absorber than H2O in its bands, but
/// planetary CO2 densities are much lower; the ~1500× scale vs.
/// H2O (in this unit system) lets a modest CO2 buildup deliver
/// the multi-K forcing real Earth sees from CO2 doubling without
/// requiring Venus-scale columns.
///
/// Calibrated for T13 snowball recovery (fix-C3): a snowball
/// planet with the stock `Volcanism::earth_like` source builds
/// ~9 units of CO2 per cell over ~10⁶ ticks (≈ 1000× the
/// depleted initial column). At `K = 5.0` the raw greenhouse
/// term lands near ~45 K — enough that, even with the
/// snowball-cell effective albedo still pinned high by sea-ice
/// (per-cell T_eq baseline ~233 K, ~36 K below the ice-free
/// baseline ~269 K), the total per-cell T_eq crosses the freeze
/// line and the positive ice-albedo feedback unlocks. The
/// previous coefficient (0.030 K per unit) topped out at ~0.3 K
/// of forcing from the same buildup, ~100× too small to lever
/// past the bifurcation gap.
///
/// Earth-baseline cells carry only fractions of a unit of CO2,
/// so the contribution stays a fraction of a degree — the modern
/// Earth equilibrium is dominated by the atmosphere-class
/// baseline `greenhouse_k`. A Venus-like dense-CO2 column
/// (~10⁶ units in the calibration test) overdrives the cap, so
/// [`greenhouse_cap_k`] binds and the plateau is set by the cap
/// rather than this coefficient.
fn co2_greenhouse_k() -> Real {
    Real::from_int(5)
}

/// Per-unit-density greenhouse coefficient for methane. Similar
/// order to CO2 (per-molecule strong, density-low). Combined with
/// the photolysis decay below, this gives a transient warming
/// pulse from any CH4 injection that fades over hundreds of ticks.
fn ch4_greenhouse_k() -> Real {
    Real::from_ratio(25, 1_000)
}

/// Per-tick exponential-decay factor on CH4 density, mimicking
/// UV photolysis (real-atmosphere lifetime ~10 years). Set to
/// `0.999` so a 1.0-density CH4 column halves in ~700 ticks —
/// short enough that a CH4 burst doesn't perpetually warm the
/// planet, long enough that CH4 still contributes meaningfully
/// to short-term warming events. Applied uniformly per cell;
/// future work can couple decay rate to local UV flux.
fn ch4_decay_per_tick() -> Real {
    Real::from_ratio(999, 1_000)
}

/// Earth's nominal surface pressure (Pa). Anchors the pressure-
/// scaled greenhouse cap so the Earth-pressure case reduces to
/// the original 250 K saturation ceiling.
fn earth_surface_pressure_pa() -> Real {
    Real::from_int(101_325)
}

/// `ln(10)` precomputed for converting natural log to log10.
/// `log10(x) = ln(x) / ln(10)`; pulled into a helper so the
/// pressure-cap formula avoids recomputing the constant per
/// integrate call.
fn ln_10() -> Real {
    // ln(10) ≈ 2.30258509…; rational approximation keeps the
    // Q32.32 representation deterministic across builds.
    Real::from_ratio(2_302_585, 1_000_000)
}

/// Per-cell greenhouse contribution ceiling, in K, scaled by
/// surface pressure. Physically motivated: real-atmosphere IR
/// absorption bands saturate at high optical depth, so doubling
/// the greenhouse gas column doesn't double the warming once the
/// bands are already opaque (Beer-Lambert with τ ≫ 1). However,
/// the *post-saturation* contribution from pressure-broadened
/// continuum absorption and overlapping-band wing forcing does
/// continue scaling with column density (i.e. with surface
/// pressure for a well-mixed gas). A constant cap therefore
/// underestimates the ceiling for thick atmospheres like Venus's
/// 90-bar CO2 column.
///
/// Formula: `cap = 250 + 100 × log10(P / P_earth)`, clamped to
/// `[50, 600]` K to keep the cap finite for arbitrarily thin /
/// thick atmospheres and to stay well clear of the Q32.32 range.
/// Anchor points:
/// - Earth (~101 325 Pa) → 250 K (preserves legacy calibration
///   and existing tests).
/// - Venus (~9.2×10⁶ Pa) → 250 + 100 × log10(90.8) ≈ 446 K,
///   which sits a Venus-equivalent runaway plateau in the
///   literature 700-770 K band (bare T_eq ~309 K + ~446 K cap
///   ≈ 755 K).
/// - Mars (~610 Pa) → clamped at the 50 K floor (raw value
///   would be 250 − 222 = 28 K).
///
/// Without this cap a Venus-style runaway in the simulation has
/// no upper bound — the H2O cycle keeps lifting
/// `saturation_vapour_cap(T)` quartically with T, which lifts
/// vapour, which lifts greenhouse forcing, with no physical stop
/// until the fixed-point arithmetic overflows
/// (`saturation_vapour_cap` hits `Real`'s ~2.1e9 ceiling around
/// T ≈ 5300 K).
fn greenhouse_cap_scaled(surface_pressure_pa: Real) -> Real {
    let earth_p = earth_surface_pressure_pa();
    let base = Real::from_int(250);
    // Guard against ln(0) / ln(negative): a zero / negative
    // surface pressure short-circuits to the 50 K floor (the
    // thin-atmosphere clamp). The constructor enforces a
    // positive default, but the explicit guard keeps the helper
    // robust if a caller threads through a degenerate value.
    if surface_pressure_pa <= Real::ZERO {
        return Real::from_int(50);
    }
    let ratio = surface_pressure_pa / earth_p;
    // `ln(ratio)` panics on a non-positive argument; the divide
    // above keeps `ratio > 0` because both numerator and
    // denominator are positive Reals.
    let log10_ratio = ln(ratio) / ln_10();
    let raw_cap = base + Real::from_int(100) * log10_ratio;
    // Clamp to `[50, 600]` K. The floor keeps thin-atmosphere
    // worlds (Mars) from collapsing the runaway-bounding cap to
    // zero; the ceiling keeps a hypothetical super-Earth with a
    // multi-hundred-bar atmosphere from overflowing the per-cell
    // feedback term.
    let floor = Real::from_int(50);
    let ceil = Real::from_int(600);
    raw_cap.max(floor).min(ceil)
}

/// `ln(σ)` for σ = 5.67×10⁻⁸ W/(m²·K⁴). Pre-computed so the
/// per-row equilibrium formula
/// `T_eq = exp((ln(S × (1-A) × insol_avg) - ln(σ)) / 4)`
/// stays in fixed-point range without intermediate `S/σ` overflow
/// (`S/σ ≈ 10⁹` blows past the fixed-point ±2.1×10⁹ ceiling).
/// `ln(5.67e-8) ≈ -16.685`. Pre-computed to keep the per-tick
/// hot path branch-free.
fn ln_sigma() -> Real {
    Real::from_ratio(-16_685, 1_000)
}

/// Per-tick fraction of the temperature gap a cell closes toward
/// its row's radiative equilibrium. 2% per tick gives a ~50-tick
/// relaxation timescale (~4 years monthly cadence), matching the
/// order-of-magnitude of real planetary thermal-equilibration
/// timescales for surface layers.
fn relaxation_rate() -> Real {
    Real::percent(2)
}

/// Number of seasonal slices per orbital year. 12 (one per
/// month) gives a smooth-enough seasonal swing for our resolution
/// without ballooning the pre-computed `t_eq_per_row_per_season`
/// table. Each cell sees a step every `year_macros / 12` macro-
/// steps.
const SEASONS_PER_YEAR: u32 = 12;

#[derive(Debug, Clone)]
pub struct Radiation {
    /// Pre-computed 2D table of per-row equilibrium
    /// temperatures, indexed `[row][season]`. `season` advances
    /// over the orbital year via the planet's `macro_step` clock;
    /// each season's column corresponds to the sub-solar latitude
    /// at that time of year.
    ///
    /// Earlier this was a 1D `Vec<Real>` (annual mean); the table
    /// promotes it to a per-season table so summer hemispheres
    /// genuinely warm and winter hemispheres cool. Without the
    /// table, the integrate path would need `exp` + `ln` per
    /// row per macro-step — significant cost over a 24,000-tick
    /// run. The table is `height × 12` entries (~108 for Earth-
    /// sized grids), computed once at planet build.
    ///
    /// Entries are *baseline* equilibrium temperatures with the
    /// planet-mean albedo baked in; per-cell deviation from that
    /// baseline is applied each tick by multiplying with
    /// `((1 - A_cell) / (1 - A_baseline))^(1/4)` so the per-row
    /// table can stay precomputed without recomputing the
    /// Stefan-Boltzmann root for every cell.
    pub t_eq_per_row_per_season: Vec<Vec<Real>>,
    /// Pre-computed `(1 - A_baseline)^(1/4)` factor used to
    /// invert the baseline albedo from
    /// `t_eq_per_row_per_season` when applying the per-cell
    /// `(1 - A_cell)^(1/4)` factor each tick. Caches a single
    /// `sqrt(sqrt)` so the per-cell ratio reduces to one
    /// multiply + one divide.
    pub baseline_albedo_factor: Real,
    /// Greenhouse offset (K) baked into the per-row table; held
    /// separately so the per-cell albedo rescaling can subtract
    /// it before applying the `(1 - A_cell)` ratio, then re-add
    /// it after. Without the separation, the per-cell rescaling
    /// would erroneously scale the greenhouse contribution by
    /// the albedo ratio.
    pub greenhouse_k: Real,
    /// Macro-steps per full orbital year. Indexes into
    /// `t_eq_per_row_per_season`'s season axis. Defaults to 360
    /// for Earth (12 months × 30 macro-steps/month at the
    /// 1-day-per-macro-step cadence). `0` collapses the seasonal
    /// table to its annual-mean column for moonless / non-orbit-
    /// resolved cases.
    pub year_macros: u64,
    pub relaxation: Real,
    /// Diurnal cycling. Day length in macro-steps
    /// (`day_length_hours / 24` rounded). For Earth (24 h), 1
    /// macro-step = 1 day → 1 here; tidally-locked planets get
    /// `>= 41` (1000 h / 24).
    pub day_length_macros: Real,
    /// Per-cell diurnal-swing amplitude in `[0, 1]`. Earlier
    /// this was always 0 (no diurnal effect — uniform day/night
    /// average). For fast-rotator planets where multiple
    /// rotations fit in one macro-step, the diurnal cycle washes
    /// out → 0 is correct. For slow / tidally-locked rotators
    /// the day/night asymmetry persists across macro-steps →
    /// amplitude trends toward 1.
    pub diurnal_amplitude: Real,
    /// Tidal-locking regime for this planet. `Synchronous` swaps
    /// the per-cell day-night modulation for the great-circle-
    /// distance gradient anchored on `substellar_lat_turns /
    /// substellar_lon_turns`; `Other` keeps the existing diurnal
    /// path (which trivially zeroes out for fast rotators).
    pub locking_mode: LockingMode,
    /// Sub-stellar latitude in fractional turns `[-1/4, +1/4]`
    /// (so `+1/4` is the north pole). For `LockingMode::Other`
    /// this field is unread.
    pub substellar_lat_turns: Real,
    /// Sub-stellar longitude in fractional turns `[0, 1)`. For
    /// `LockingMode::Other` this field is unread.
    pub substellar_lon_turns: Real,
    /// Surface gravity (m/s²) used to derive the dry adiabatic
    /// lapse rate for the cirrus-greenhouse calculation
    /// (any-planet backlog T5). Earth default ≈ 9.81; high-gravity
    /// super-Earths get steeper lapse → cooler cirrus tops → more
    /// `T_surface − T_cloud_top` contrast → stronger cirrus
    /// longwave trap. Set via [`Radiation::with_lapse_inputs`].
    pub gravity_ms2: Real,
    /// Cirrus deck altitude (m) at which the cloud-top temperature
    /// is evaluated. Defaults to
    /// [`REFERENCE_CIRRUS_ALTITUDE_M`] (10 km — real-Earth cirrus
    /// top); per-planet override possible via
    /// [`Radiation::with_lapse_inputs`] for atmospheres with a
    /// markedly different cloud-deck altitude.
    pub cirrus_altitude_m: Real,
    /// Surface atmospheric pressure (Pa) used to scale the
    /// greenhouse saturation cap via [`greenhouse_cap_scaled`].
    /// Defaults to Earth's pressure (101 325 Pa) so legacy
    /// callers / tests reproduce the historical 250 K cap;
    /// sim-core overrides per-planet via
    /// [`Radiation::with_surface_pressure`] so dense atmospheres
    /// (Venus-equivalent worlds, super-Earths) get a higher cap
    /// matching pressure-broadened continuum absorption and
    /// thin atmospheres (Mars-equivalent worlds) get the
    /// clamp-floored lower cap.
    pub surface_pressure_pa: Real,
}

impl Radiation {
    /// Build a radiation law from planet parameters.
    /// - `grid_height` — number of grid rows (latitude bins)
    /// - `stellar_w_per_m2` — planet's stellar irradiance
    /// - `albedo_x100` — surface+atmosphere albedo (0–100,
    ///   e.g. Earth ~30)
    /// - `greenhouse_k` — additive greenhouse offset to `T_eq`
    ///   (atmosphere-derived; thicker / hazy atmospheres trap
    ///   more heat)
    /// - `axial_tilt_deg` — orbital obliquity in degrees (0–45
    ///   typical). Drives the seasonal swing of sub-solar
    ///   latitude.
    /// - `year_macros` — macro-steps per orbital year. Defaults
    ///   to 360 (12 months × 30 days). `0` disables seasons.
    /// - `eccentricity_x100` — orbital eccentricity × 100.
    ///   Per-season insolation gets multiplied by
    ///   `1 / (1 - e · cos(2π·season/12))²`. 0 = circular orbit
    ///   (no eccentricity swing); 50 = highly eccentric.
    /// - `day_length_hours` — sidereal day length. Used to
    ///   derive diurnal modulation amplitude: fast rotators get 0
    ///   (washes out at our resolution); slow / tidally-locked
    ///   rotators get amplitude → 1.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn for_planet(
        grid_height: u32,
        stellar_w_per_m2: Real,
        albedo_x100: i64,
        greenhouse_k: Real,
        axial_tilt_deg: i64,
        year_macros: u64,
        eccentricity_x100: i64,
        day_length_hours: Real,
    ) -> Self {
        let height = grid_height.max(1);
        let height_i = i32::try_from(height).unwrap_or(i32::MAX);
        let half_h = height_i / 2;
        // Max sub-solar latitude offset in rows. Earth's
        // 23.5° tilt on a 9-row grid (half_h=4) gives ~1 row of
        // swing — modest but enough to flip "warmest band" from
        // equator toward a hemisphere when amplified.
        let axial_tilt_rows = if half_h > 0 {
            i32::try_from(axial_tilt_deg.clamp(0, 45) * i64::from(half_h) / 90).unwrap_or(0)
        } else {
            0
        };

        let albedo_factor = Real::from_ratio(100 - albedo_x100.clamp(0, 100), 100);
        let four = Real::from_int(4);
        let ln_sigma_v = ln_sigma();
        // Polar-bias floor — guarantee even pole cells get *some*
        // insolation, since real planets have non-zero pole insolation
        // from atmospheric scattering / longitude variance our 1D
        // model can't represent. Without this, pole cells trend toward
        // 0 K and the climate gradient becomes unphysically extreme.
        let lat_floor = Real::percent(15);

        let n_seasons = SEASONS_PER_YEAR as usize;
        let mut t_eq_per_row_per_season: Vec<Vec<Real>> =
            vec![vec![Real::ZERO; n_seasons]; height as usize];

        // Eccentricity factor `e` ∈ [0, 1) clamped from
        // the per-100 input. Insolation per season gets multiplied
        // by `1 / (1 - e · cos(2π·season/year))²` to model the
        // 1/r² dependence as the planet swings between perihelion
        // and aphelion.
        let eccentricity = Real::from_ratio(eccentricity_x100.clamp(0, 95), 100);

        for (season, _) in (0..n_seasons).enumerate() {
            // Sub-solar row offset for this season,
            // computed as `axial_tilt_rows · cos(2π · season / 12)`.
            // Conventions: season 0 = N summer peak / perihelion,
            // 3 = autumn equinox, 6 = S summer / aphelion,
            // 9 = spring equinox.
            let s_i64 = i64::try_from(season).unwrap_or(0);
            let n_i64 = i64::from(SEASONS_PER_YEAR);
            let two_pi_v = sim_arith::transcendental::two_pi();
            let phase = two_pi_v * Real::from_ratio(s_i64, n_i64);
            let cos_phase = cos(phase);
            let max_o = Real::from_int(i64::from(axial_tilt_rows));
            let raw_offset_real = max_o * cos_phase;
            let raw_offset_int = raw_offset_real.raw().round_ties_even().to_num::<i64>();
            // Per-season insolation multiplier from
            // eccentricity. At perihelion (cos_phase=+1, season=0):
            // factor = 1/(1-e)². At aphelion (cos_phase=-1,
            // season=6): factor = 1/(1+e)². Circular orbit (e=0)
            // gives 1.0 across all seasons.
            let one_minus_ecos = Real::ONE - eccentricity * cos_phase;
            let safe_denom = if one_minus_ecos <= Real::ZERO {
                Real::percent(5)
            } else {
                one_minus_ecos
            };
            let inv = Real::ONE / safe_denom;
            let ecc_factor = inv * inv;
            // Subtract (not add) so positive `raw_offset` shifts
            // sub-solar latitude toward row 0 (the N pole in our
            // convention).
            let sub_solar_row = half_h - i32::try_from(raw_offset_int).unwrap_or(0);

            for r in 0..height_i {
                let pole_dist = (r - sub_solar_row).abs();
                // Real cosine latitude attenuation. Map
                // pole_dist ∈ [0, max_dist] to angle ∈ [0, π/2]
                // and take cos. Earlier code used triangular `1 -
                // pole_dist/max_dist`; cos gives the correct
                // physics shape (steeper at the poles, flatter
                // near the sub-solar row).
                let max_dist = i32::max(sub_solar_row, height_i - 1 - sub_solar_row).max(1);
                let angle = half_pi() * Real::from_ratio(i64::from(pole_dist), i64::from(max_dist));
                let lat_factor = cos(angle).max(lat_floor);
                // Insolation absorbed at this row, W/m². Annual
                // average factor 0.25 distributes a sun's
                // irradiance over the sphere (the sub-solar point
                // sees S, the average sees S/4).
                let absorbed =
                    stellar_w_per_m2 * lat_factor * albedo_factor * Real::percent(25) * ecc_factor;
                // T_eq = (absorbed / σ)^(1/4) in K. Compute via
                // logs to dodge fixed-point overflow on `absorbed/σ ≈
                // 10^9`:  T_eq = exp((ln(absorbed) - ln(σ)) / 4)
                let t_eq_no_greenhouse = if absorbed > Real::ZERO {
                    exp((ln(absorbed) - ln_sigma_v) / four)
                } else {
                    Real::ZERO
                };
                let r_idx = usize::try_from(r).unwrap_or(0);
                t_eq_per_row_per_season[r_idx][season] = t_eq_no_greenhouse + greenhouse_k;
            }
        }

        // Derive diurnal amplitude from day length.
        // For day_length_macros ∈ [1, 10], amplitude ramps 0 → 1.
        // Earth-like (1) → 0; tidally-locked (>= 41 from the
        // 1000h threshold) → 1.
        let day_length_macros_real = day_length_hours / Real::from_int(24);
        let amp_threshold_low = Real::ONE;
        let amp_threshold_high = Real::from_int(10);
        let diurnal_amplitude = if day_length_macros_real <= amp_threshold_low {
            Real::ZERO
        } else if day_length_macros_real >= amp_threshold_high {
            Real::ONE
        } else {
            (day_length_macros_real - amp_threshold_low) / (amp_threshold_high - amp_threshold_low)
        };

        Self {
            t_eq_per_row_per_season,
            baseline_albedo_factor: albedo_radiation_factor(Real::from_ratio(
                albedo_x100.clamp(0, 100),
                100,
            )),
            greenhouse_k,
            year_macros,
            relaxation: relaxation_rate(),
            day_length_macros: day_length_macros_real,
            diurnal_amplitude,
            // Default: spinning planet. Callers (sim-core
            // `build_laws`) override via `with_locking` for
            // tidally-locked worlds. Without the override the
            // `LockingMode::Other` path runs and the existing
            // diurnal-amplitude code carries through unchanged.
            locking_mode: LockingMode::Other,
            substellar_lat_turns: Real::ZERO,
            substellar_lon_turns: Real::ZERO,
            // Default to Earth gravity + 10 km cirrus altitude so
            // existing callers (and tests) reproduce the historical
            // 15 K cirrus forcing without a `with_lapse_inputs`
            // chain. sim-core overrides per-planet at law build.
            gravity_ms2: Real::from_ratio(981, 100),
            cirrus_altitude_m: Real::from_int(REFERENCE_CIRRUS_ALTITUDE_M),
            // Default to Earth pressure so existing callers /
            // tests reproduce the historical 250 K cap exactly
            // (the pressure-scaled formula evaluates to 250 K
            // when `surface_pressure_pa == earth_surface_pressure_pa`).
            // sim-core overrides per-planet via
            // `with_surface_pressure` at law build.
            surface_pressure_pa: earth_surface_pressure_pa(),
        }
    }

    /// Override the locking regime + sub-stellar point. Callers
    /// pass the result of `sim_world::sub_stellar_point` here
    /// (latitude / longitude in fractional turns) when building
    /// the law for a tidally-locked planet. For
    /// `LockingMode::Other` the lat/lon arguments are unread —
    /// the diurnal-amplitude path stays in charge — but it costs
    /// nothing to thread them through uniformly.
    ///
    /// Returned by value (builder style) so the wire-in in
    /// `sim-core` chains cleanly:
    /// `Radiation::for_planet(...).with_locking(mode, lat, lon)`.
    #[must_use]
    pub fn with_locking(
        mut self,
        locking_mode: LockingMode,
        substellar_lat_turns: Real,
        substellar_lon_turns: Real,
    ) -> Self {
        self.locking_mode = locking_mode;
        self.substellar_lat_turns = substellar_lat_turns;
        self.substellar_lon_turns = substellar_lon_turns;
        self
    }

    /// Override the inputs to the per-cell cirrus-greenhouse
    /// calculation (any-planet backlog T5). Dry-adiabatic lapse
    /// rate is derived from `gravity_ms2` via `g / c_p_air`;
    /// `cirrus_altitude_m` sets the cloud-top height at which the
    /// surface-vs-cloud-top temperature contrast is evaluated.
    /// Together they drive `cirrus_greenhouse_strength` so a
    /// high-gravity world gets a steeper lapse → cooler cirrus
    /// tops → stronger longwave trap.
    ///
    /// Defaults (Earth gravity 9.81 m/s², altitude 10 km) reproduce
    /// the historical 15 K constant. sim-core overrides per-planet
    /// at law build; tests that don't care about the lapse-driven
    /// path can rely on the defaults.
    ///
    /// `cirrus_altitude_m ≤ 0` is clamped at the Earth reference;
    /// `gravity_ms2 ≤ 0` falls through to
    /// `dry_adiabatic_lapse_rate(0) = 0` which collapses cirrus
    /// greenhouse to zero (consistent with a gravity-less world
    /// where the lapse-rate concept doesn't apply).
    #[must_use]
    pub fn with_lapse_inputs(mut self, gravity_ms2: Real, cirrus_altitude_m: Real) -> Self {
        self.gravity_ms2 = gravity_ms2;
        self.cirrus_altitude_m = if cirrus_altitude_m > Real::ZERO {
            cirrus_altitude_m
        } else {
            Real::from_int(REFERENCE_CIRRUS_ALTITUDE_M)
        };
        self
    }

    /// Override the surface pressure used by the per-cell
    /// greenhouse-cap scaling (calibration fix C1). Thicker
    /// atmospheres lift the saturation ceiling so Venus-
    /// equivalent worlds plateau in the literature 700-770 K
    /// runaway band; thinner atmospheres lower it (clamped at
    /// the 50 K floor in [`greenhouse_cap_scaled`]) so Mars-
    /// equivalent worlds don't accidentally trap heat with the
    /// Earth-pressure ceiling. Defaults to
    /// `earth_surface_pressure_pa()` so callers that omit the
    /// override reproduce the legacy 250 K cap exactly.
    ///
    /// Non-positive inputs fall back to the Earth-pressure
    /// default so a misconfigured caller doesn't collapse the
    /// cap to the floor unintentionally.
    #[must_use]
    pub fn with_surface_pressure(mut self, surface_pressure_pa: Real) -> Self {
        self.surface_pressure_pa = if surface_pressure_pa > Real::ZERO {
            surface_pressure_pa
        } else {
            earth_surface_pressure_pa()
        };
        self
    }

    /// Current seasonal index for the given macro-step.
    /// `0` always when `year_macros == 0` (seasons disabled).
    #[must_use]
    pub fn season_index(&self, macro_step: u64) -> usize {
        if self.year_macros == 0 {
            return 0;
        }
        let n = u64::from(SEASONS_PER_YEAR);
        let phase = (macro_step.saturating_mul(n)) / self.year_macros;
        usize::try_from(phase % n).unwrap_or(0)
    }
}

impl Law for Radiation {
    fn integrate(&self, state: &mut PhysicsState, dt: Real) {
        let grid = state.grid().clone();
        let dt_relax = self.relaxation * dt;
        let temps_prev = state.temperature().to_vec();
        let season = self.season_index(state.macro_step());
        let width_i = i32::try_from(grid.width()).unwrap_or(1).max(1);
        let height_i = i32::try_from(grid.height()).unwrap_or(i32::MAX).max(1);

        // Per-cell effective albedo from the snow / sea-ice /
        // cloud channels (authored by `IceAlbedo` upstream) plus
        // the surface base type. Pre-computed once per
        // `integrate` so the per-cell hot loop just multiplies
        // by a slice index.
        let effective_albedo = effective_albedo_slice(state);
        // Guard against a divide-by-zero on a degenerate
        // baseline (`albedo_x100 = 100`). The albedo helper
        // already clamps at 0.9999, so the baseline factor
        // stays positive; the explicit floor keeps the math
        // robust against future tuning.
        let baseline_factor = if self.baseline_albedo_factor > Real::ZERO {
            self.baseline_albedo_factor
        } else {
            Real::from_ratio(1, 10_000)
        };

        // Per-substance greenhouse coefficients. Pulled here so the
        // hot loop avoids the function-call overhead per cell.
        let h2o_k = h2o_greenhouse_k();
        let co2_k = co2_greenhouse_k();
        let ch4_k = ch4_greenhouse_k();
        let greenhouse_cap = greenhouse_cap_scaled(self.surface_pressure_pa);

        // Snapshot the gas densities that feed the per-cell
        // greenhouse term. Water vapour is the C-C-coupled channel
        // (its cap rises quartically with T → positive feedback).
        // CO2 is linear (long-lived, no T-coupling). CH4 is similar
        // but additionally decays via photolysis (see Step 2).
        let vapour = state.substance(Substance::Vapour.idx()).to_vec();
        let co2 = state.substance(Substance::CO2.idx()).to_vec();
        let ch4 = state.substance(Substance::Methane.idx()).to_vec();
        // Per-cell cloud cover + type (Sprint 5 Item 23). Cirrus
        // contributes more greenhouse forcing than stratus
        // (high-altitude ice clouds trap more outgoing longwave
        // than low-altitude liquid-water clouds). Read once per
        // integrate so the per-cell hot loop avoids the byte
        // decode.
        let cloud_fraction = state.cloud_fraction().to_vec();
        let cloud_type = state.cloud_type().to_vec();
        // Lapse-rate-driven cirrus forcing (any-planet backlog
        // T5). Dry adiabatic lapse = g / c_p_air is invariant per
        // tick; pre-compute it (plus the cirrus deck altitude) so
        // the per-cell call to `cirrus_greenhouse_strength` only
        // varies on `temps_prev[i]`. On a high-gravity world the
        // steeper lapse drives cooler cirrus tops at the same
        // altitude → larger `T_surface − T_cloud_top` → stronger
        // longwave trap (the spec's `(ΔT)^4` Stefan-Boltzmann
        // scaling).
        let lapse_rate = dry_adiabatic_lapse_rate(self.gravity_ms2);
        let cirrus_altitude = self.cirrus_altitude_m;
        let stratus_gh = stratus_greenhouse_k();

        // Per-cell diurnal modulation. Sub-solar longitude
        // advances at rate 1 / day_length_macros per macro-step.
        // For tidally-locked planets it advances ~0; for Earth
        // it makes a full sweep per macro-step. Combined with the
        // amplitude ramp, the modulation is `0` for fast rotators
        // and `cos(longitude_diff)` for slow / locked rotators.
        let macro_step_real = Real::from_int(i64::try_from(state.macro_step()).unwrap_or(0));
        let two_pi_v = sim_arith::transcendental::two_pi();
        let day_phase = if self.day_length_macros > Real::ZERO {
            macro_step_real / self.day_length_macros
        } else {
            Real::ZERO
        };
        // Sub-solar longitude in [0, 2π) at the current macro-step.
        let two_pi_phase = two_pi_v * day_phase;
        let sub_solar_q_real = Real::from_int(i64::from(width_i)) * day_phase;
        // Round to nearest cell index for the longitude sweep.
        let sub_solar_q = sub_solar_q_real.raw().round_ties_even().to_num::<i64>();
        let sub_solar_q_i = i32::try_from(sub_solar_q.rem_euclid(i64::from(width_i))).unwrap_or(0);
        let _ = two_pi_phase; // unused — sub_solar_q_i is the integer phase

        // P1.5: synchronous-lock path setup. For a `Synchronous`
        // planet the sub-stellar point is fixed; pre-compute its
        // (sin, cos)-latitude and longitude in radians so the
        // per-cell great-circle-distance formula
        // `cos(angle) = sin(lat₁)·sin(lat₂)
        //              + cos(lat₁)·cos(lat₂)·cos(lon₁ - lon₂)`
        // stays O(1) per cell. The factor maps cos(angle) into a
        // day-side / night-side absorption multiplier
        // `night + (1 - night) · max(0, cos(angle))`; T_eq scales
        // as the fourth root of absorption (Stefan-Boltzmann).
        // Without this, locked worlds are climatically
        // indistinguishable from spinning ones (the per-row
        // zonal-mean equilibrium already averages the day-night
        // gradient away).
        let half_h = height_i / 2;
        let is_synchronous = matches!(self.locking_mode, LockingMode::Synchronous);
        let sub_lat_rad = self.substellar_lat_turns * two_pi_v;
        let sub_lon_rad = self.substellar_lon_turns * two_pi_v;
        let sub_sin_lat = sin(sub_lat_rad);
        let sub_cos_lat = cos(sub_lat_rad);
        let night = night_factor();
        let one_minus_night = Real::ONE - night;
        // Fourth-root exponent for T_eq ∝ absorption^(1/4).
        let quarter = Real::from_ratio(1, 4);
        let pi_v = sim_arith::transcendental::pi();

        let temps_next = state.temperature_mut();
        for (cid, axial) in grid.cells() {
            let i = cid.0 as usize;
            let row = usize::try_from(axial.r.rem_euclid(height_i)).unwrap_or(0);
            let t_eq_table = self
                .t_eq_per_row_per_season
                .get(row)
                .and_then(|row_seasons| row_seasons.get(season))
                .copied()
                .unwrap_or(temps_prev[i]);

            // Strip the planet-mean greenhouse offset, swap the
            // baseline-mean albedo for this cell's effective
            // albedo, then re-add the greenhouse. The
            // `(1 - A_cell)^(1/4) / (1 - A_baseline)^(1/4)`
            // ratio rescales the Stefan-Boltzmann root without
            // recomputing exp/ln per cell.
            let t_no_greenhouse = t_eq_table - self.greenhouse_k;
            let cell_factor = albedo_radiation_factor(effective_albedo[i]);
            let t_eq_base = t_no_greenhouse * cell_factor / baseline_factor + self.greenhouse_k;

            // Day-night modulation factor on T_eq. Three paths:
            //
            // 1. `LockingMode::Synchronous`: per-cell great-circle
            //    distance to the *fixed* sub-stellar point.
            //    Absorbed insolation scales as
            //    `night + (1 - night) · max(0, cos(angle))`;
            //    T_eq scales as the fourth root (Stefan-
            //    Boltzmann). This is the path that makes a
            //    tidally-locked world climatically distinct from
            //    a spinning one — without it, the sub-stellar
            //    point is sampled but never read.
            //
            // 2. `LockingMode::Other` with `diurnal_amplitude > 0`:
            //    slow-rotator longitude-sweep modulation (the
            //    existing `1 + amp · cos(q_diff)` path). Sub-
            //    solar longitude advances with `macro_step`.
            //
            // 3. `LockingMode::Other` with `diurnal_amplitude = 0`:
            //    fast rotator — diurnal cycle washes out at our
            //    resolution. Factor = 1.
            let day_factor = if is_synchronous {
                // Cell latitude in radians. Convention: row 0 =
                // north pole (lat = +π/2), row `half_h` = equator
                // (lat = 0), row `height-1` = south pole
                // (lat ≈ -π/2). Map row → lat directly via
                // `lat = π · (half_h - r) / height`, which gives
                // the half-turn range [-π/2, +π/2] without
                // round-tripping through fractional turns.
                let row_i = axial.r.rem_euclid(height_i);
                let lat_rad = pi_v.saturating_mul(Real::from_ratio(
                    i64::from(half_h - row_i),
                    i64::from(height_i.max(1)),
                ));
                // Longitude in radians, `axial.q ∈ [0, width)`.
                let lon_turns = Real::from_ratio(
                    i64::from(axial.q.rem_euclid(width_i)),
                    i64::from(width_i),
                );
                let lon_rad = lon_turns * two_pi_v;
                // Great-circle distance:
                //   cos(angle) = sin(lat₁)·sin(lat₂)
                //              + cos(lat₁)·cos(lat₂)·cos(Δlon).
                let sin_lat = sin(lat_rad);
                let cos_lat = cos(lat_rad);
                let d_lon = lon_rad - sub_lon_rad;
                let cos_angle = sin_lat
                    .saturating_mul(sub_sin_lat)
                    .saturating_add(cos_lat.saturating_mul(sub_cos_lat).saturating_mul(cos(d_lon)));
                // Day side fraction. The strict-Lambertian
                // `max(0, cos_angle)` clamps both terminator and
                // antistellar to 0, leaving them at the same
                // equilibrium T — physically reasonable for a
                // radiation-only model (neither sees direct
                // insolation) but it collapses the terminator
                // zone tests. We instead use the smoothed half-
                // cosine `(cos_angle + 1) / 2`, which maps
                // [-1, +1] → [0, 1]: 1 at sub-stellar, 0.5 at
                // the terminator (90°), 0 at antistellar. This
                // is the same shape a real atmosphere produces
                // by carrying day-side heat across the
                // terminator via winds + conduction; our
                // radiation law approximates that redistribution
                // with the smoothed profile directly so the per-
                // cell equilibrium picks up the right gradient
                // even on a 1-row grid (where the diffusion
                // laws can't move heat between longitudes).
                let cell_day = (cos_angle + Real::ONE) * Real::from_ratio(1, 2);
                let cell_day = cell_day.clamp01();
                // Absorption multiplier: night + (1 - night) · cell_day.
                // night = 0.05 floor at the antistellar point,
                // 1.0 ceiling at the sub-stellar point, ~0.525
                // at the terminator. T_eq scales as the fourth
                // root (Stefan-Boltzmann).
                let absorption_mult =
                    night.saturating_add(one_minus_night.saturating_mul(cell_day));
                pow(absorption_mult, quarter)
            } else if self.diurnal_amplitude > Real::ZERO {
                let q_diff = (axial.q - sub_solar_q_i).rem_euclid(width_i);
                let signed = if q_diff <= width_i / 2 {
                    q_diff
                } else {
                    q_diff - width_i
                };
                let theta = two_pi_v * Real::from_ratio(i64::from(signed), i64::from(width_i));
                Real::ONE + self.diurnal_amplitude * cos(theta)
            } else {
                Real::ONE
            };
            // Per-cell dynamic greenhouse contribution. The
            // atmosphere-class baseline (`greenhouse_k` passed to
            // `for_planet`) is already folded into `t_eq_base`;
            // this adds composition-dependent forcing on top so
            // a vapour-rich cell warms beyond the planet-wide
            // baseline. The H2O term is *implicitly* exponential
            // in T because vapour density itself is bounded by
            // [`crate::hydrology::saturation_vapour_cap`], which
            // rises quartically with T/T_ref — that's the
            // Clausius-Clapeyron-coupled positive feedback that
            // drives runaway. Bounded by [`greenhouse_cap_scaled`]
            // so a saturated runaway plateaus at a Venus-like
            // temperature rather than overflowing fixed-point
            // arithmetic; the cap rises with surface pressure so
            // dense atmospheres reach the literature-anchored
            // Venus plateau (~735 K) rather than the legacy
            // Earth-pressure ceiling (~559 K with a 250 K cap).
            // Per-cell cloud greenhouse contribution. Cirrus cells
            // add `cirrus_gh × cloud_fraction`; stratus cells add
            // the smaller `stratus_gh × cloud_fraction`. Without
            // this term the cloud_fraction field affected only
            // albedo (shortwave shielding) — clouds in the real
            // climate also trap outgoing longwave.
            //
            // Cirrus magnitude is lapse-driven (any-planet backlog
            // T5): `cirrus_greenhouse_strength` evaluates
            // `(T_surface − T_cloud_top)^4 / ΔT_earth^4 × 15 K` per
            // cell, so a hotter cell at the same lapse + altitude
            // contributes more forcing — the Stefan-Boltzmann
            // surface-vs-cloud-top emission difference. Stratus
            // remains a constant: low-altitude clouds emit at
            // near-surface T and the per-planet variation is
            // dominated by composition rather than lapse.
            let cloud_gh_peak = match CloudType::from_byte(cloud_type[i]) {
                CloudType::Cirrus => {
                    cirrus_greenhouse_strength(temps_prev[i], lapse_rate, cirrus_altitude)
                }
                CloudType::Stratus => stratus_gh,
            };
            let cloud_gh = cloud_gh_peak * cloud_fraction[i].clamp01();
            // P0.6: saturating arithmetic so a hot seed whose
            // `vapour[i]` is at the Clausius-Clapeyron-driven cap
            // (`saturation_vapour_cap` peaks near ~4e7 at silicate-
            // world temperatures) doesn't panic on the
            // `vapour[i] * h2o_k` multiply or the four-way sum. The
            // subsequent `min(greenhouse_cap)` clamps the meaningful
            // contribution at the pressure-scaled cap (250 K at
            // Earth pressure, ~446 K at Venus pressure; see
            // `greenhouse_cap_scaled`).
            let v_term = vapour[i].saturating_mul(h2o_k);
            let c_term = co2[i].saturating_mul(co2_k);
            let m_term = ch4[i].saturating_mul(ch4_k);
            let greenhouse_raw = v_term
                .saturating_add(c_term)
                .saturating_add(m_term)
                .saturating_add(cloud_gh);
            let greenhouse_cell = greenhouse_raw.min(greenhouse_cap);
            let t_eq = t_eq_base.saturating_mul(day_factor).saturating_add(greenhouse_cell);
            let gap = t_eq - temps_prev[i];
            temps_next[i] = temps_prev[i] + gap * dt_relax;
        }

        // Step 2: CH4 photolysis decay. CH4 has a real-atmosphere
        // lifetime of ~10 years (UV-radical destruction). Modelled
        // as a per-tick exponential decay on each cell — short-
        // lived gases trend toward zero unless something keeps
        // sourcing them. dt-aware: the decay factor per dt is
        // `1 - (1 - base) * dt`. For Earth-like dt=1 macro-step
        // this collapses to `base` (= 0.999). Larger dt would
        // accelerate the decay proportionally; the formulation
        // stays linear-in-dt for small per-step decay rates,
        // which matches the first-order Taylor expansion of true
        // exponential decay.
        let decay_base = ch4_decay_per_tick();
        let one_minus_decay = Real::ONE - decay_base;
        let factor = Real::ONE - one_minus_decay * dt;
        // Clamp the factor at zero so a pathologically large dt
        // can't drive CH4 negative. Real::ZERO is the physical
        // floor (no negative methane density).
        let factor = factor.max(Real::ZERO);
        for v in state.substance_mut(Substance::Methane.idx()).iter_mut() {
            *v = *v * factor;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::HexGrid;

    #[test]
    fn equator_warmer_than_pole_at_equinox() {
        let rad = Radiation::for_planet(
            8,
            Real::from_int(1_361),
            30,
            Real::ZERO,
            0, // no tilt → no seasonal shift
            360,
            0,                             // circular orbit for tests
            sim_arith::Real::from_int(24), // Earth-like 24h day
        );
        // With zero tilt, every season's column should put the
        // hottest band at the equator (mid-grid).
        let mid = rad.t_eq_per_row_per_season[4][0];
        let pole = rad.t_eq_per_row_per_season[0][0];
        assert!(mid > pole);
    }

    #[test]
    fn relaxation_pulls_cell_toward_equilibrium() {
        let mut state = PhysicsState::new(HexGrid::new(3, 3));
        for t in state.temperature_mut() {
            *t = Real::from_int(500);
        }
        let rad = Radiation::for_planet(
            3,
            Real::from_int(1_361),
            30,
            Real::ZERO,
            0,
            360,
            0,                             // circular orbit for tests
            sim_arith::Real::from_int(24), // Earth-like 24h day
        );
        let initial = state.temperature()[0];
        let row_eq = rad.t_eq_per_row_per_season[0][0];
        for _ in 0..100 {
            rad.integrate(&mut state, Real::ONE);
        }
        let final_t = state.temperature()[0];
        let initial_gap = (row_eq - initial).abs();
        let final_gap = (row_eq - final_t).abs();
        // The cell must have moved toward equilibrium.
        assert!(final_gap < initial_gap);
    }

    #[test]
    fn season_index_advances_with_macro_step() {
        let rad = Radiation::for_planet(
            3,
            Real::from_int(1_361),
            30,
            Real::ZERO,
            23,
            360,                           // 12 months × 30 macro-steps
            0,                             // circular orbit
            sim_arith::Real::from_int(24), // 24h day
        );
        assert_eq!(rad.season_index(0), 0);
        assert_eq!(rad.season_index(30), 1);
        assert_eq!(rad.season_index(180), 6);
        assert_eq!(rad.season_index(360), 0); // wraps
    }

    #[test]
    fn northern_hemisphere_warmer_in_n_summer_than_s_summer() {
        // With axial tilt, a northern-hemisphere row's T_eq
        // should peak in N-summer (season 0) and trough in
        // S-summer (season 6).
        let rad = Radiation::for_planet(
            9, // tall enough that tilt produces ≥1 row offset
            Real::from_int(1_361),
            30,
            Real::ZERO,
            45, // strong tilt — guaranteed nonzero axial_tilt_rows
            360,
            0,                             // circular orbit for tests
            sim_arith::Real::from_int(24), // Earth-like 24h day
        );
        // Row 1 sits firmly in the northern hemisphere (row 4 = mid).
        let n_summer = rad.t_eq_per_row_per_season[1][0];
        let s_summer = rad.t_eq_per_row_per_season[1][6];
        assert!(
            n_summer > s_summer,
            "N hemisphere row should warm in N summer relative to S summer: \
             n_summer={n_summer:?} s_summer={s_summer:?}"
        );
    }

    #[test]
    fn no_tilt_means_no_seasonal_swing() {
        let rad = Radiation::for_planet(
            9,
            Real::from_int(1_361),
            30,
            Real::ZERO,
            0, // no tilt
            360,
            0,                             // circular orbit for tests
            sim_arith::Real::from_int(24), // Earth-like 24h day
        );
        // Every season's table column should be identical when
        // tilt is zero.
        for r in 0..9 {
            let s0 = rad.t_eq_per_row_per_season[r][0];
            let s6 = rad.t_eq_per_row_per_season[r][6];
            assert_eq!(s0, s6, "row {r}: tilt=0 must give identical seasons");
        }
    }

    #[test]
    fn perihelion_warmer_than_aphelion_for_eccentric_orbit() {
        // With zero tilt and high eccentricity, perihelion (season 0)
        // should be hotter than aphelion (season 6) at every row.
        let rad = Radiation::for_planet(
            5,
            Real::from_int(1_361),
            30,
            Real::ZERO,
            0, // no tilt — isolate eccentricity effect
            360,
            30,                 // e = 0.30 (highly eccentric)
            Real::from_int(24), // 24h day
        );
        for r in 0..5 {
            let perihelion = rad.t_eq_per_row_per_season[r][0];
            let aphelion = rad.t_eq_per_row_per_season[r][6];
            assert!(
                perihelion > aphelion,
                "row {r}: perihelion T_eq should exceed aphelion T_eq with e=0.30: \
                 perihelion={perihelion:?} aphelion={aphelion:?}"
            );
        }
    }

    #[test]
    fn tidally_locked_planet_has_diurnal_amplitude_one() {
        // A tidally-locked planet (day_length >= 1000h)
        // gets full diurnal amplitude. Earth-like (24h) gets 0.
        let earth_like = Radiation::for_planet(
            5,
            Real::from_int(1_361),
            30,
            Real::ZERO,
            0,
            360,
            0,
            Real::from_int(24),
        );
        assert_eq!(earth_like.diurnal_amplitude, Real::ZERO);

        let tidally_locked = Radiation::for_planet(
            5,
            Real::from_int(1_361),
            30,
            Real::ZERO,
            0,
            360,
            0,
            Real::from_int(1_500),
        );
        assert_eq!(tidally_locked.diurnal_amplitude, Real::ONE);
    }

    #[test]
    fn diurnal_modulation_warms_day_side_cools_night_side() {
        // Tidally-locked planet → permanent day side (sub-solar
        // longitude fixed at q=0) gets warmer than the antipodal
        // night side. Run from a uniform initial T and verify.
        use crate::laws::Law;
        let rad = Radiation::for_planet(
            5,
            Real::from_int(1_361),
            30,
            Real::ZERO,
            0,
            0,                     // no seasonal swing
            0,                     // circular orbit
            Real::from_int(2_000), // tidally locked
        );
        let mut state = PhysicsState::new(HexGrid::new(8, 1));
        for t in state.temperature_mut() {
            *t = Real::from_int(280);
        }
        // Run many ticks to reach quasi-steady state.
        for _ in 0..200 {
            rad.integrate(&mut state, Real::ONE);
        }
        let day_cell = state.temperature()[0];
        let night_cell = state.temperature()[4]; // q=4 is antipodal for width=8
        assert!(
            day_cell > night_cell,
            "tidally-locked day side should be warmer than night side: \
             day={day_cell:?} night={night_cell:?}"
        );
    }

    // P1.5 — synchronous-lock day-night gradient. These tests
    // verify that a `LockingMode::Synchronous` planet develops a
    // permanent hot day side / cold night side anchored on the
    // sub-stellar point, that a `LockingMode::Other` planet stays
    // zonally symmetric (rotation washes any gradient out), and
    // that the terminator zone lands between the two extremes.

    #[test]
    fn synchronous_planet_has_hot_day_side_cold_night_side() {
        // A `Synchronous` planet's sub-stellar point is fixed at
        // (lat=0, lon=0); the cell closest to that point should
        // sit at a higher equilibrium T than the cell at the
        // antistellar point (lon=0.5). We use a 1-row grid to
        // remove latitudinal cooling — the only modulator left is
        // the day-night gradient, so any T separation must come
        // from the synchronous code path.
        use crate::laws::Law;
        let rad = Radiation::for_planet(
            1,
            Real::from_int(1_361),
            30,
            Real::ZERO,
            0,    // no axial tilt
            0,    // no seasonal swing
            0,    // circular orbit
            Real::from_int(24), // day length irrelevant under
                                // Synchronous mode (the fixed
                                // sub-stellar point is what
                                // drives the gradient)
        )
        .with_locking(LockingMode::Synchronous, Real::ZERO, Real::ZERO);
        let mut state = PhysicsState::new(HexGrid::new(8, 1));
        for t in state.temperature_mut() {
            *t = Real::from_int(280);
        }
        // Run many ticks to reach quasi-steady state. The
        // relaxation rate is ~2 % per tick (50-tick e-folding),
        // so 500 ticks brings every cell to within 5e-5 of its
        // (now per-cell) equilibrium.
        for _ in 0..500 {
            rad.integrate(&mut state, Real::ONE);
        }
        // Sub-stellar cell sits at q=0 (cos_angle=1, full
        // insolation). Antistellar at q=4 for width=8
        // (cos_angle=-1, clamped → night floor).
        let day_cell = state.temperature()[0];
        let night_cell = state.temperature()[4];
        assert!(
            day_cell > night_cell + Real::from_int(50),
            "synchronous-lock day side must be much hotter than night side: \
             day={day_cell:?} night={night_cell:?}"
        );
    }

    #[test]
    fn free_rotator_has_zonal_symmetric_radiation() {
        // Same planet parameters as the synchronous test above,
        // but in `LockingMode::Other` (free rotator). With Earth-
        // like day length the diurnal amplitude is zero (fast
        // rotators average out at our macro-step resolution), so
        // all cells at the same latitude should converge to the
        // same equilibrium T — no day/night gradient.
        use crate::laws::Law;
        let rad = Radiation::for_planet(
            1,
            Real::from_int(1_361),
            30,
            Real::ZERO,
            0,
            0,
            0,
            Real::from_int(24),
        )
        .with_locking(LockingMode::Other, Real::ZERO, Real::ZERO);
        let mut state = PhysicsState::new(HexGrid::new(8, 1));
        for t in state.temperature_mut() {
            *t = Real::from_int(280);
        }
        for _ in 0..500 {
            rad.integrate(&mut state, Real::ONE);
        }
        // Every cell in this 1-row grid sits at the equator;
        // under `LockingMode::Other` they should converge to the
        // same T. Tolerance ~1 K covers Q32.32 LSB rounding and
        // the residual gap from the relaxation pull.
        let t0 = state.temperature()[0];
        for q in 1..8 {
            let tq = state.temperature()[q];
            let diff = (tq - t0).abs();
            assert!(
                diff < Real::from_int(1),
                "free-rotator row should be zonally symmetric: q=0 T={t0:?}, q={q} T={tq:?}, diff={diff:?}"
            );
        }
    }

    #[test]
    fn terminator_zone_has_moderate_temperature() {
        // The terminator is the 90°-from-substellar great-circle
        // band — half the cell sees the star and half doesn't. A
        // cell sitting on the terminator should have an
        // equilibrium T between the hottest day-side cell (sub-
        // stellar) and the coldest night-side cell (antistellar).
        //
        // Use a 5-row grid so we can include latitude variation,
        // then check the terminator row's temperature is between
        // the day and night extremes.
        use crate::laws::Law;
        let rad = Radiation::for_planet(
            1,
            Real::from_int(1_361),
            30,
            Real::ZERO,
            0,
            0,
            0,
            Real::from_int(24),
        )
        .with_locking(LockingMode::Synchronous, Real::ZERO, Real::ZERO);
        let mut state = PhysicsState::new(HexGrid::new(8, 1));
        for t in state.temperature_mut() {
            *t = Real::from_int(280);
        }
        for _ in 0..500 {
            rad.integrate(&mut state, Real::ONE);
        }
        // For width=8 with sub-stellar at q=0: q=2 sits at
        // longitude 2/8 = 0.25 turns = 90° east — exactly on the
        // terminator. q=6 (270° = -90°) is the other terminator.
        // Both should land strictly between the day-side cell
        // (q=0) and the night-side cell (q=4).
        let day = state.temperature()[0];
        let term_east = state.temperature()[2];
        let term_west = state.temperature()[6];
        let night = state.temperature()[4];
        assert!(
            term_east < day && term_east > night,
            "east terminator should sit between day and night: \
             day={day:?} term={term_east:?} night={night:?}"
        );
        assert!(
            term_west < day && term_west > night,
            "west terminator should sit between day and night: \
             day={day:?} term={term_west:?} night={night:?}"
        );
        // The two terminators sit at symmetric great-circle
        // distances from the sub-stellar point, so their
        // equilibrium temperatures should match (within Q32.32
        // LSB rounding).
        let diff = (term_east - term_west).abs();
        assert!(
            diff < Real::from_int(1),
            "east and west terminators should be symmetric: \
             east={term_east:?} west={term_west:?} diff={diff:?}"
        );
    }

    #[test]
    fn circular_orbit_no_eccentricity_swing() {
        let rad = Radiation::for_planet(
            5,
            Real::from_int(1_361),
            30,
            Real::ZERO,
            0,
            360,
            0,                  // circular
            Real::from_int(24), // 24h day
        );
        for r in 0..5 {
            let s0 = rad.t_eq_per_row_per_season[r][0];
            let s6 = rad.t_eq_per_row_per_season[r][6];
            assert_eq!(
                s0, s6,
                "row {r}: e=0 + tilt=0 must give identical perihelion/aphelion"
            );
        }
    }

    // Sprint 3 Item 14 — Per-substance greenhouse coupling tests.

    #[test]
    fn h2o_vapour_contributes_to_per_cell_greenhouse() {
        // Direct check that per-cell greenhouse increases T_eq by
        // the vapour-weighted constant. Two states differing only
        // in vapour density; the vapour-rich one warms faster
        // toward equilibrium because its T_eq is higher.
        let rad = Radiation::for_planet(
            3,
            Real::from_int(1_361),
            30,
            Real::ZERO,
            0,
            0, // no seasons
            0,
            Real::from_int(24),
        );
        let mut dry = PhysicsState::new(HexGrid::new(3, 3));
        let mut wet = PhysicsState::new(HexGrid::new(3, 3));
        for t in dry.temperature_mut() {
            *t = Real::from_int(280);
        }
        for t in wet.temperature_mut() {
            *t = Real::from_int(280);
        }
        // Seed only the wet state with vapour. A vapour density
        // of 50,000 × H2O_GREENHOUSE_K (0.002) = +100 K of
        // equilibrium greenhouse forcing — large enough that a
        // single 2%-relaxation tick produces a measurable T
        // shift.
        for v in wet.substance_mut(Substance::Vapour.idx()) {
            *v = Real::from_int(50_000);
        }
        // One integrate. Wet state's per-cell T should advance
        // further toward (higher) T_eq than dry state's.
        rad.integrate(&mut dry, Real::ONE);
        rad.integrate(&mut wet, Real::ONE);
        let dry_t = dry.temperature()[0];
        let wet_t = wet.temperature()[0];
        assert!(
            wet_t > dry_t,
            "vapour-rich cell should warm faster: dry={dry_t:?} wet={wet_t:?}"
        );
    }

    #[test]
    fn co2_contributes_linearly_to_greenhouse() {
        // CO2's contribution is per-density × `co2_greenhouse_k`
        // (5.0 K per unit post-fix-C3). A modest CO2 load should
        // still produce a measurable T_eq lift; the greenhouse cap
        // clamps the contribution at 250 K for very dense columns.
        let rad = Radiation::for_planet(
            3,
            Real::from_int(1_361),
            30,
            Real::ZERO,
            0,
            0,
            0,
            Real::from_int(24),
        );
        let mut without_co2 = PhysicsState::new(HexGrid::new(3, 3));
        let mut with_co2 = PhysicsState::new(HexGrid::new(3, 3));
        for t in without_co2.temperature_mut() {
            *t = Real::from_int(280);
        }
        for t in with_co2.temperature_mut() {
            *t = Real::from_int(280);
        }
        for v in with_co2.substance_mut(Substance::CO2.idx()) {
            // 5000 × 5.0 = 25,000 K raw, clamped to the 250 K cap;
            // still well above the no-CO2 baseline so the assertion
            // below (with_co2 warmer than without_co2) holds.
            *v = Real::from_int(5_000);
        }
        rad.integrate(&mut without_co2, Real::ONE);
        rad.integrate(&mut with_co2, Real::ONE);
        assert!(
            with_co2.temperature()[0] > without_co2.temperature()[0],
            "CO2-rich cell should warm faster: without={:?} with={:?}",
            without_co2.temperature()[0],
            with_co2.temperature()[0]
        );
    }

    #[test]
    fn ch4_decays_via_photolysis() {
        // CH4 should decay each tick via the photolysis factor
        // (~0.999/tick). Verify a seeded methane column shrinks
        // after a single integrate, and shrinks further over many.
        let rad = Radiation::for_planet(
            3,
            Real::from_int(1_361),
            30,
            Real::ZERO,
            0,
            0,
            0,
            Real::from_int(24),
        );
        let mut state = PhysicsState::new(HexGrid::new(3, 3));
        for v in state.substance_mut(Substance::Methane.idx()) {
            *v = Real::from_int(100);
        }
        let initial = state.substance(Substance::Methane.idx())[0];
        rad.integrate(&mut state, Real::ONE);
        let after_one = state.substance(Substance::Methane.idx())[0];
        assert!(
            after_one < initial,
            "CH4 should decay after one tick: initial={initial:?} after_one={after_one:?}"
        );
        for _ in 0..99 {
            rad.integrate(&mut state, Real::ONE);
        }
        let after_hundred = state.substance(Substance::Methane.idx())[0];
        assert!(
            after_hundred < after_one,
            "CH4 should keep decaying: after_one={after_one:?} after_hundred={after_hundred:?}"
        );
    }

    #[test]
    fn hot_seed_slides_into_venus_state_via_h2o_runaway() {
        // Per Sprint 3 Item 14 acceptance criteria: a hot seed
        // (350 K) with vapour-rich atmosphere coupled to the
        // saturation cap should slide into a Venus-style
        // runaway. Final mean T must exceed 400 K — the
        // signature that the C-C-coupled feedback actually took
        // hold rather than cells relaxing to a hot equilibrium.
        //
        // The atmosphere-baseline `greenhouse_k` is zero — *all*
        // greenhouse forcing comes from the per-substance dynamic
        // term. That isolates the runaway signal from atmosphere-
        // class forcing.
        //
        // 1-row grid removes latitudinal cooling (the runaway
        // feedback is global, not latitude-dependent).
        //
        // Vapour pegged to `sat_cap(T)` each tick mimics "vapour
        // equilibrates instantly with the saturation cap" — the
        // limit a much-larger-than-hydrology-timescale tick
        // approximates. Without a peg the test would need
        // geological timescales (millions of ticks) for
        // hydrology's deliberately-slow sub-boil evap (Sprint 1
        // Item 4) to fill the cap as T climbs; the peg compresses
        // that into the test horizon.
        let rad = Radiation::for_planet(
            1,
            Real::from_int(2_500),    // strong stellar input
            10,                       // low albedo (heat-absorbing)
            Real::ZERO,               // baseline greenhouse=0
            0,
            0,                        // no seasons
            0,
            Real::from_int(24),
        );
        let mut state = PhysicsState::new(HexGrid::new(3, 1));
        for t in state.temperature_mut() {
            *t = Real::from_int(350);
        }
        // Initial vapour seeded at sat_cap(350); will be re-
        // pegged each tick.
        for v in state.substance_mut(Substance::Vapour.idx()) {
            *v = crate::hydrology::saturation_vapour_cap(Real::from_int(350));
        }
        for _ in 0..2_000 {
            rad.integrate(&mut state, Real::ONE);
            // Peg vapour to current sat_cap(T) per cell.
            let temps = state.temperature().to_vec();
            for (i, v) in state
                .substance_mut(Substance::Vapour.idx())
                .iter_mut()
                .enumerate()
            {
                *v = crate::hydrology::saturation_vapour_cap(temps[i]);
            }
        }
        let final_mean_t = {
            let temps = state.temperature();
            let mut sum = Real::ZERO;
            for t in temps {
                sum = sum + *t;
            }
            sum / Real::from_int(i64::try_from(temps.len()).unwrap_or(1))
        };
        assert!(
            final_mean_t > Real::from_int(400),
            "H2O runaway should drive mean T above 400 K; got {final_mean_t:?}"
        );
    }

    #[test]
    fn runaway_threshold_at_published_t_temp() {
        // Komabayashi-Ingersoll-style threshold: under Earth-like
        // stellar input + a near-saturation initial atmosphere
        // the climate stays sub-runaway (mean T plateau well
        // below 400 K). Boost stellar irradiance significantly
        // and the H2O runaway feedback tips the planet into a
        // Venus-style hot state.
        //
        // Run both planets from the same temperate seed (290 K)
        // with identical vapour loads pegged to `sat_cap(T)`
        // each tick (the C-C feedback path); only stellar
        // irradiance differs. The earth-like one settles into a
        // temperate equilibrium; the boosted one diverges past
        // 400 K.
        //
        // The boost magnitude in this test (~85 %) is larger
        // than the real-Earth K-I threshold (~10 % above modern
        // solar) because the per-cell greenhouse coefficients
        // and the [`greenhouse_cap_scaled`] ceiling are tuned for
        // the simulation's Q32.32 range rather than calibrated
        // against the real K-I value. The qualitative bistable
        // threshold — temperate equilibrium vs. runaway — is
        // what matters; the exact tipping irradiance is a tuning
        // choice tied to the rest of the constants.
        let earth_like_irradiance = Real::from_int(1_361);
        let boosted_irradiance = Real::from_int(2_500);
        // 1-row grid so no latitude variation contaminates the
        // mean-T signal (the runaway feedback is global, not
        // latitude-dependent).
        let build = |stellar: Real| {
            Radiation::for_planet(
                1,
                stellar,
                10,
                Real::ZERO,
                0,
                0,
                0,
                Real::from_int(24),
            )
        };
        let seed_state = || {
            let mut s = PhysicsState::new(HexGrid::new(3, 1));
            for t in s.temperature_mut() {
                *t = Real::from_int(290);
            }
            // Initial vapour at sat_cap(290); the peg loop below
            // re-evaluates each tick (the C-C feedback path).
            for v in s.substance_mut(Substance::Vapour.idx()) {
                *v = crate::hydrology::saturation_vapour_cap(Real::from_int(290));
            }
            s
        };
        // Run radiation only with a per-tick vapour peg to
        // `sat_cap(T)` — same approach as the runaway test, for
        // the same reason: hydrology's deliberately-slow sub-
        // boil evap (Sprint 1 Item 4) would need geological
        // timescales to fill the rising sat_cap as T climbs.
        // The peg compresses the C-C feedback into the test
        // horizon. Both states see the same dynamics; only
        // stellar irradiance differs.
        let run = |rad: &Radiation, state: &mut PhysicsState| -> Real {
            for _ in 0..2_000 {
                rad.integrate(state, Real::ONE);
                let temps = state.temperature().to_vec();
                for (i, v) in state
                    .substance_mut(Substance::Vapour.idx())
                    .iter_mut()
                    .enumerate()
                {
                    *v = crate::hydrology::saturation_vapour_cap(temps[i]);
                }
            }
            let temps = state.temperature();
            let mut sum = Real::ZERO;
            for t in temps {
                sum = sum + *t;
            }
            sum / Real::from_int(i64::try_from(temps.len()).unwrap_or(1))
        };
        let mut earth_state = seed_state();
        let mut boosted_state = seed_state();
        let rad_earth = build(earth_like_irradiance);
        let rad_boost = build(boosted_irradiance);
        let earth_mean = run(&rad_earth, &mut earth_state);
        let boost_mean = run(&rad_boost, &mut boosted_state);
        assert!(
            earth_mean < Real::from_int(360),
            "earth-like stellar input should not run away; got {earth_mean:?}"
        );
        assert!(
            boost_mean > Real::from_int(400),
            "boosted stellar input should trigger runaway; got {boost_mean:?}"
        );
    }

    // T11 (any-planet backlog) — Venus runaway plateau calibration.
    //
    // Literature anchor: Venus surface T ~ 735 K under ~2613 W/m²
    // stellar irradiance and a 90-bar CO2 atmosphere, with the H2O
    // runaway-greenhouse Komabayashi-Ingersoll plateau falling in
    // the 700-770 K band. The simulation's greenhouse coupling
    // (`co2_greenhouse_k` + `h2o_greenhouse_k` + C-C-coupled vapour
    // cap) and bounding cap (`greenhouse_cap_scaled`, pressure-
    // scaled per calibration fix C1) lets a Venus-equivalent
    // planet settle on (or near) that plateau when the surface
    // pressure is set to the Venus 90-bar value via
    // `Radiation::with_surface_pressure`.
    //
    // With the pressure-scaled cap (`250 + 100 × log10(P/P_earth)`,
    // clamped to `[50, 600]`), Venus's 9.2×10⁶ Pa column gives a
    // cap of ~446 K above the bare ~309 K T_eq → ~755 K plateau,
    // squarely in the literature band.
    #[test]
    fn venus_runaway_plateau_t_in_700_to_770_k() {
        // Venus-equivalent radiation environment:
        //   - stellar irradiance ~2600 W/m² at Venus's orbit
        //   - low albedo so we maximise net stellar absorption
        //     (Venus's *real* albedo is high, ~0.75, because of
        //     sulphuric-acid clouds — but the greenhouse + atmospheric
        //     dynamics keep the *surface* hot; for this test we drop
        //     albedo so the surface forcing matches the surface-
        //     temperature anchor rather than top-of-atmosphere
        //     balance, which the model doesn't separate)
        //   - zero atmosphere-class baseline greenhouse; the per-cell
        //     dynamic term carries all greenhouse forcing so the
        //     calibration target sits purely on
        //     `co2_greenhouse_k` × CO2 + `h2o_greenhouse_k` × vapour
        //     + the pressure-scaled saturation cap.
        //   - surface pressure pinned to Venus's ~9.2×10⁶ Pa
        //     (≈90.8 bar) so the pressure-scaled cap lifts to
        //     ~446 K (calibration fix C1).
        let rad = Radiation::for_planet(
            1,
            Real::from_int(2_600),
            10,
            Real::ZERO,
            0,
            0,
            0,
            Real::from_int(24),
        )
        .with_surface_pressure(Real::from_int(9_200_000));
        let mut state = PhysicsState::new(HexGrid::new(3, 1));
        // Seed at 500 K so the runaway path triggers immediately
        // (vapour cap is already well above the K-I threshold at
        // 500 K). The plateau is asymptotic — initial T just sets
        // how fast we reach it, not where it lands.
        for t in state.temperature_mut() {
            *t = Real::from_int(500);
        }
        // Dense CO2 column — Venus has ~90 bar CO2, ~2000× Earth's
        // total atmospheric column. Pegging CO2 to a high value so
        // the per-cell CO2 greenhouse term saturates the cap
        // independent of any biogeochemistry.
        for v in state.substance_mut(Substance::CO2.idx()) {
            *v = Real::from_int(1_000_000);
        }
        // Initial vapour seeded at sat_cap(500). The per-tick peg
        // below keeps vapour at sat_cap(T) so the C-C-coupled
        // feedback path stays armed (same approach as the
        // `hot_seed_slides_into_venus_state_via_h2o_runaway`
        // baseline test).
        for v in state.substance_mut(Substance::Vapour.idx()) {
            *v = crate::hydrology::saturation_vapour_cap(Real::from_int(500));
        }
        // 5000 ticks to reach steady state. The relaxation
        // timescale is ~50 ticks (2 %/tick), so 5000 ticks =
        // ~100 e-foldings — fully converged.
        for _ in 0..5_000 {
            rad.integrate(&mut state, Real::ONE);
            // Re-peg vapour to sat_cap(T) each tick (the C-C
            // feedback path). Re-peg CO2 too so the photolysis /
            // chemistry channels can't bleed it down inside this
            // radiation-only test.
            let temps = state.temperature().to_vec();
            for (i, v) in state
                .substance_mut(Substance::Vapour.idx())
                .iter_mut()
                .enumerate()
            {
                *v = crate::hydrology::saturation_vapour_cap(temps[i]);
            }
            for v in state.substance_mut(Substance::CO2.idx()) {
                *v = Real::from_int(1_000_000);
            }
        }
        let final_mean_t = {
            let temps = state.temperature();
            let mut sum = Real::ZERO;
            for t in temps {
                sum = sum + *t;
            }
            sum / Real::from_int(i64::try_from(temps.len()).unwrap_or(1))
        };
        // Literature plateau: T ∈ [700, 770] K. With the
        // pressure-scaled cap (calibration fix C1) Venus's 90-bar
        // surface pressure lifts the greenhouse ceiling from the
        // legacy 250 K to ~446 K, putting the saturated runaway
        // plateau in the literature band.
        //
        // Decomposition: T_eq_base for stellar=2600 W/m²,
        // albedo=10 %, equator 1-row grid:
        // `(2600 × 0.9 × 1.0 × 0.25 / σ)^(1/4) ≈ 309 K`, plus the
        // pressure-scaled cap `250 + 100 × log10(9.2×10⁶ /
        // 101325) ≈ 446 K`, gives ~755 K — squarely in the band.
        let plateau_lo = Real::from_int(700);
        let plateau_hi = Real::from_int(770);
        assert!(
            final_mean_t >= plateau_lo && final_mean_t <= plateau_hi,
            "Venus-equivalent plateau out of literature 700-770 K band: \
             got {final_mean_t:?}"
        );
    }

    // Calibration fix C1 — verify `greenhouse_cap_scaled` honours
    // the documented anchors: Earth pressure → ~250 K (legacy
    // baseline preserved), Venus pressure → ~446 K (lifts the
    // runaway plateau into the literature 700-770 K band).
    #[test]
    fn greenhouse_cap_scales_with_pressure() {
        // Earth-pressure caller (101 325 Pa) reproduces the
        // legacy 250 K cap exactly (the formula is anchored on
        // log10(P/P_earth) so the Earth case evaluates the
        // additive term to zero).
        let earth_cap = greenhouse_cap_scaled(Real::from_int(101_325));
        let earth_lo = Real::from_int(249);
        let earth_hi = Real::from_int(251);
        assert!(
            earth_cap >= earth_lo && earth_cap <= earth_hi,
            "Earth-pressure cap should be ~250 K; got {earth_cap:?}"
        );

        // Venus-pressure caller (9.2×10⁶ Pa) lifts the cap into
        // the ~440-450 K band. With `100 × log10(90.8) ≈ 195.8`
        // the formula evaluates to ~446 K; allow ±10 K so the
        // assertion survives Q32.32 rounding without becoming a
        // brittle exact-value check.
        let venus_cap = greenhouse_cap_scaled(Real::from_int(9_200_000));
        let venus_lo = Real::from_int(436);
        let venus_hi = Real::from_int(456);
        assert!(
            venus_cap >= venus_lo && venus_cap <= venus_hi,
            "Venus-pressure cap should be ~446 K; got {venus_cap:?}"
        );

        // Mars-pressure caller (~610 Pa) clamps at the 50 K
        // floor — the raw log10 evaluation would be negative
        // (28 K), but the floor keeps the cap usable for thin-
        // atmosphere worlds without collapsing it to zero.
        let mars_cap = greenhouse_cap_scaled(Real::from_int(610));
        assert!(
            mars_cap == Real::from_int(50),
            "Mars-pressure cap should clamp to the 50 K floor; got {mars_cap:?}"
        );

        // Defensive: a degenerate zero pressure should fall back
        // to the 50 K floor (the ln(0) panic guard inside
        // `greenhouse_cap_scaled`).
        let zero_cap = greenhouse_cap_scaled(Real::ZERO);
        assert!(
            zero_cap == Real::from_int(50),
            "Zero-pressure cap should fall back to the 50 K floor; got {zero_cap:?}"
        );
    }
}
