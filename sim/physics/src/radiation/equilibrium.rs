//! Bare radiative equilibrium: stellar input × (1 - albedo) /
//! Stefan-Boltzmann → per-row equilibrium temperature, with seasonal
//! modulation from axial tilt + eccentricity. The pre-computed
//! `[row][season]` table in [`super::Radiation`] is filled once at
//! planet build by [`compute_t_eq_table`]; per-tick integration just
//! indexes it.

use sim_arith::transcendental::{cos, exp, half_pi, ln};
use sim_arith::Real;

/// Number of seasonal slices per orbital year. 12 (one per
/// month) gives a smooth-enough seasonal swing for our resolution
/// without ballooning the pre-computed `t_eq_per_row_per_season`
/// table. Each cell sees a step every `year_macros / 12` macro-
/// steps.
pub(super) const SEASONS_PER_YEAR: u32 = 12;

/// `ln(σ)` for σ = 5.67×10⁻⁸ W/(m²·K⁴). Pre-computed so the
/// per-row equilibrium formula
/// `T_eq = exp((ln(S × (1-A) × insol_avg) - ln(σ)) / 4)`
/// stays in fixed-point range without intermediate `S/σ` overflow
/// (`S/σ ≈ 10⁹` blows past the fixed-point ±2.1×10⁹ ceiling).
/// `ln(5.67e-8) ≈ -16.685`. Pre-computed to keep the per-tick
/// hot path branch-free.
pub(super) fn ln_sigma() -> Real {
    Real::from_ratio(-16_685, 1_000)
}

/// Per-tick fraction of the temperature gap a cell closes toward
/// its row's radiative equilibrium. 2% per tick gives a ~50-tick
/// relaxation timescale (~4 years monthly cadence), matching the
/// order-of-magnitude of real planetary thermal-equilibration
/// timescales for surface layers.
pub(super) fn relaxation_rate() -> Real {
    Real::percent(2)
}

/// Build the `[row][season]` equilibrium-temperature table. Pure
/// function of planet parameters — pulled out so [`super::Radiation::for_planet`]
/// reads as a thin assembler over (1) this table, (2) the
/// baseline-albedo factor, and (3) the diurnal amplitude. The
/// table folds in the atmosphere-class baseline `greenhouse_k` so
/// per-cell integration only adds the composition-dependent
/// dynamic greenhouse term on top.
///
/// Inputs:
/// - `grid_height` — number of grid rows (latitude bins). Clamped
///   to at least 1.
/// - `stellar_w_per_m2` — planet's stellar irradiance.
/// - `albedo_x100` — surface+atmosphere albedo (0–100, e.g.
///   Earth ~30).
/// - `greenhouse_k` — additive greenhouse offset to `T_eq`
///   (atmosphere-derived baseline; per-cell composition forcing
///   added at integrate time).
/// - `axial_tilt_deg` — orbital obliquity in degrees (0–45 typical).
///   Drives the seasonal swing of sub-solar latitude.
/// - `eccentricity_x100` — orbital eccentricity × 100. Per-season
///   insolation gets multiplied by `1 / (1 - e · cos(2π·season/12))²`.
pub(super) fn compute_t_eq_table(
    grid_height: u32,
    stellar_w_per_m2: Real,
    albedo_x100: i64,
    greenhouse_k: Real,
    axial_tilt_deg: i64,
    eccentricity_x100: i64,
) -> Vec<Vec<Real>> {
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

    t_eq_per_row_per_season
}

/// Map day length in hours to diurnal modulation amplitude
/// `∈ [0, 1]`. Earth-like (24 h → 1 macro-step) → 0 (fast rotator,
/// diurnal cycle washes out at macro-step resolution); tidally-
/// locked (≥ 240 h → ≥ 10 macro-steps) → 1.
///
/// Returns `(day_length_macros, diurnal_amplitude)` so the caller
/// can stash both without recomputing the `/24` division.
pub(super) fn diurnal_amplitude_from_day_length(day_length_hours: Real) -> (Real, Real) {
    let day_length_macros = day_length_hours / Real::from_int(24);
    // For day_length_macros ∈ [1, 10], amplitude ramps 0 → 1.
    // Earth-like (1) → 0; tidally-locked (>= 41 from the
    // 1000h threshold) → 1.
    let amp_threshold_low = Real::ONE;
    let amp_threshold_high = Real::from_int(10);
    let diurnal_amplitude = if day_length_macros <= amp_threshold_low {
        Real::ZERO
    } else if day_length_macros >= amp_threshold_high {
        Real::ONE
    } else {
        (day_length_macros - amp_threshold_low) / (amp_threshold_high - amp_threshold_low)
    };
    (day_length_macros, diurnal_amplitude)
}
