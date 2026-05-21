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

use crate::albedo::{albedo_radiation_factor, effective_albedo_slice};
use crate::laws::Law;
use crate::state::PhysicsState;
use sim_arith::transcendental::{cos, exp, half_pi, ln};
use sim_arith::Real;

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
        }
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

            // Diurnal modulation factor in `[1 - amp,
            // 1 + amp]`. amp=0 → 1 (no modulation, original
            // T_eq); amp=1 + cos at sub-solar → 2; amp=1 +
            // cos opposite → 0. The annual-mean T_eq factor is
            // 1.0 by convention; modulating multiplies by
            // (1 + amp · cos(longitude)) where positive
            // longitude offsets get cosine swing.
            let day_factor = if self.diurnal_amplitude > Real::ZERO {
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
            let t_eq = t_eq_base * day_factor;
            let gap = t_eq - temps_prev[i];
            temps_next[i] = temps_prev[i] + gap * dt_relax;
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
}
