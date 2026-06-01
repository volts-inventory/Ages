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
//! (`ch4_decay_per_tick`).
//!
//! ## Module layout
//!
//! - [`equilibrium`] — bare T_eq, Stefan-Boltzmann, seasonal
//!   modulation; builds the `[row][season]` table.
//! - [`greenhouse`] — per-substance coefficients, cirrus / stratus
//!   cloud forcing, the pressure-scaled saturation cap, CH4 decay.
//! - [`locking`] — `LockingMode` enum + synchronous day-night
//!   gradient (sub-stellar point, great-circle distance).

mod equilibrium;
mod greenhouse;
mod locking;

#[cfg(test)]
mod tests;

use crate::albedo::{albedo_radiation_factor, effective_albedo_slice};
use crate::chemistry::Substance;
use crate::clouds::{dry_adiabatic_lapse_rate, REFERENCE_CIRRUS_ALTITUDE_M};
use crate::laws::Law;
use crate::state::PhysicsState;
use sim_arith::transcendental::cos;
use sim_arith::Real;

use equilibrium::{
    compute_t_eq_table, diurnal_amplitude_from_day_length, relaxation_rate, SEASONS_PER_YEAR,
};
use greenhouse::{
    ch4_decay_per_tick, earth_surface_pressure_pa, greenhouse_cell, GreenhouseConstants,
};
use locking::{synchronous_day_factor, SubStellarTrig};

pub use equilibrium::equilibrium_mean_k;
pub use greenhouse::greenhouse_cap_scaled;
pub use locking::LockingMode;

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
        let t_eq_per_row_per_season = compute_t_eq_table(
            grid_height,
            stellar_w_per_m2,
            albedo_x100,
            greenhouse_k,
            axial_tilt_deg,
            eccentricity_x100,
        );

        let (day_length_macros, diurnal_amplitude) =
            diurnal_amplitude_from_day_length(day_length_hours);

        Self {
            t_eq_per_row_per_season,
            baseline_albedo_factor: albedo_radiation_factor(Real::from_ratio(
                albedo_x100.clamp(0, 100),
                100,
            )),
            greenhouse_k,
            year_macros,
            relaxation: relaxation_rate(),
            day_length_macros,
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
        let greenhouse_consts =
            GreenhouseConstants::new(self.surface_pressure_pa, lapse_rate, self.cirrus_altitude_m);

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
        let sub_solar_q_real = Real::from_int(i64::from(width_i)) * day_phase;
        // Round to nearest cell index for the longitude sweep.
        let sub_solar_q = sub_solar_q_real.raw().round_ties_even().to_num::<i64>();
        let sub_solar_q_i = i32::try_from(sub_solar_q.rem_euclid(i64::from(width_i))).unwrap_or(0);

        // P1.5: synchronous-lock path setup. For a `Synchronous`
        // planet the sub-stellar point is fixed; pre-compute its
        // (sin, cos)-latitude and longitude in radians so the
        // per-cell great-circle-distance formula stays O(1) per
        // cell. Without this, locked worlds are climatically
        // indistinguishable from spinning ones (the per-row
        // zonal-mean equilibrium already averages the day-night
        // gradient away).
        let half_h = height_i / 2;
        let is_synchronous = matches!(self.locking_mode, LockingMode::Synchronous);
        let sub_trig = SubStellarTrig::new(self.substellar_lat_turns, self.substellar_lon_turns);
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
                synchronous_day_factor(
                    axial.q, axial.r, width_i, height_i, half_h, pi_v, two_pi_v, sub_trig,
                )
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
            // baseline.
            let greenhouse_cell_k = greenhouse_cell(
                &greenhouse_consts,
                vapour[i],
                co2[i],
                ch4[i],
                cloud_fraction[i],
                cloud_type[i],
                temps_prev[i],
            );
            let t_eq = t_eq_base
                .saturating_mul(day_factor)
                .saturating_add(greenhouse_cell_k);
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
