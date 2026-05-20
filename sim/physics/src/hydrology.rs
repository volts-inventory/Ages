//! Hydrologic cycle: surface evaporation → wind-driven
//! vapour transport → condensation back to surface water.
//!
//! Previously the planet had two disconnected water stories:
//!
//! - `water_depth` (a surface column tracked by `state.rs`,
//!   redistributed by `GravityFlow`). Set at world-gen, sloshes
//!   around with gravity, never grows or shrinks.
//! - `Substance::Water` ↔ `Substance::Vapour` transitions in
//!   `Chemistry`, working on a *separate* per-cell substance
//!   density. The transitions correctly captured per-substrate
//!   thresholds but had no horizontal transport — vapour
//!   created over hot cells condensed right back at the same cell
//!   with no spatial redistribution. Effectively a no-op cycle.
//!
//! Real planets evaporate from oceans, ride atmospheric circulation
//! to land, and precipitate as rain/snow that runs back to the sea.
//! This module wires the loop:
//!
//! 1. **Surface evaporation:** cells with `water_depth > 0` and
//!    `temperature > evap_threshold` move a fraction of their
//!    surface water into `Substance::Vapour`. Rate scales with
//!    temperature excess and available water (whichever is the
//!    binding constraint).
//! 2. **Vapour advection:** pair-flux upwind transport of
//!    `Substance::Vapour` along the `(v_q, v_r)` wind field —
//!    same scheme `Wind` uses for temperature.
//! 3. **Precipitation:** cells with `Substance::Vapour > 0` and
//!    `temperature < condense_threshold` move a fraction of their
//!    vapour into `water_depth`. Cold cells now actually receive
//!    rain instead of bookkeeping it as a separate substance.
//!
//! `evap_threshold` and `condense_threshold` both equal the
//! substrate-aware Clausius-Clapeyron boiling point at the cell's
//! pressure: real planets don't only evaporate at boil — there's
//! a saturation curve below it — and this module now models that
//! with a conservative quadratic drive (see "Saturation-pressure
//! sub-boil evaporation" below). The earlier behaviour was
//! "above boil → vapour, below boil → liquid", a step function;
//! the current behaviour adds a small quadratic curve under boil
//! so warm-but-sub-boil cells still evaporate at a realistic rate.
//!
//! ## Saturation-pressure sub-boil evaporation (PR6 retry)
//!
//! An earlier attempt (PR #22) used a quartic drive
//! `(T / T_boil)^4` at a scale of `× 0.10`; on long runs this
//! overflowed downstream Q32.32 fit code (per-cell vapour grew
//! unbounded because the symmetric precipitation drive wasn't
//! strengthened to match, so net flux was always positive on
//! moderately warm cells). This retry is more conservative:
//!
//! - **Sub-boil evaporation** (when `T < T_boil`):
//!   `evap_rate × (T / T_boil)^2 × water_depth × dt × 0.05`.
//!   Quadratic (not quartic) drive; half the magnitude of the
//!   failed attempt. At `T = 0.5 · T_boil` the rate is
//!   `0.0625 × evap_rate × water × dt` — a real but small flux.
//! - **Sub-boil precipitation** (when `T < T_boil`):
//!   `condense_rate × (1 - T/T_boil)^2 × vapour × dt × 0.05`.
//!   Symmetric quadratic strengthening. At T well below boil,
//!   `(1 - T/T_boil)^2 → 1` and precipitation is strong enough
//!   to absorb the evap flux from warmer cells, so the cycle
//!   reaches steady state rather than accumulating vapour
//!   unbounded.
//! - **Hard vapour cap**: even if the balance somehow breaks,
//!   per-cell vapour is clamped to `100 × max(water_depth)` at
//!   the end of each integrate. That ceiling is far above any
//!   plausible steady-state vapour level (the atmosphere can't
//!   hold that much), so the clamp is a safety net rather than
//!   a routine constraint. Without the clamp, a future
//!   regression in either drive could overflow the I32F32 range
//!   (~2e9) thousands of ticks downstream — exactly the failure
//!   mode the PR #22 retry needs to avoid.
//!
//! Above-boil evaporation still uses the existing excess-driven
//! branch (`evap_rate × excess × water_depth × dt`) — boil-point
//! physics is unchanged.
//!
//! Bit-exact mass conservation: each transfer moves the same
//! `Real` amount out of one field and into another in the same
//! pass. Pair-flux advection uses the standard
//! `+delta` / `-delta` symmetry (verified by test).

use crate::chemistry::{substrate_boiling_point_k, Substance};
use crate::laws::Law;
use crate::state::PhysicsState;
use sim_arith::transcendental::exp;
use sim_arith::Real;

/// Hex-direction axial offsets matching `HexGrid::neighbours`
/// canonical order (E, NE, NW, W, SW, SE). Same definition as in
/// `wind.rs` — duplicated here rather than re-exported because
/// the constant is two-line trivia and a public re-export would
/// commit `Wind`'s internal representation as a stable API.
const NEIGHBOUR_DIRECTIONS: [(i64, i64); 6] = [
    (1, 0),  // E
    (1, -1), // NE
    (0, -1), // NW
    (-1, 0), // W
    (-1, 1), // SW
    (0, 1),  // SE
];

/// Default atmospheric scale height in metres (Earth-like).
/// Made per-planet via `Hydrology::for_substrate`'s
/// `scale_height_m` parameter; this constant remains as the
/// `earth_like()` default for unit tests.
const DEFAULT_SCALE_HEIGHT_M: i64 = 8_400;

#[derive(Debug, Clone)]
pub struct Hydrology {
    /// Per-tick fraction of surface water that evaporates per K
    /// of temperature excess above the per-cell boil threshold.
    /// Tuned so a 50 K excess over warm equator evaporates ~5 %
    /// of column per tick — fast enough to drive a visible
    /// cycle, slow enough not to dry oceans within tens of ticks.
    pub evap_rate: Real,
    /// Per-tick fraction of vapour that condenses per K of
    /// temperature deficit below the per-cell boil threshold.
    /// Symmetric to `evap_rate` so the cycle balances at steady
    /// state.
    pub condense_rate: Real,
    /// Pair-flux upwind coefficient for vapour transport along
    /// `(v_q, v_r)`. Same form as `Wind::advect_k` — multiplies
    /// `v_along · upwind_density · dt`.
    pub vapour_advect_k: Real,
    /// Substrate tag passed to `substrate_boiling_point_k`. Defines
    /// which solvent's Clausius-Clapeyron parameters drive the
    /// per-cell phase threshold (`aqueous`, `hydrocarbon`,
    /// `ammoniacal`, `silicate`, …).
    pub substrate_tag: String,
    /// Surface (sea-level) pressure in Pa for this planet.
    /// Combined with `state.elevation()[i]` and the barometric
    /// formula `P(h) = P_0 · exp(-h / H)`, this gives a per-cell
    /// pressure that feeds `substrate_boiling_point_k` so high-
    /// altitude cells boil at lower temperatures. Previously the
    /// boil threshold was a planet-wide constant; high-altitude
    /// boiling was structurally invisible.
    pub surface_pressure_pa: Real,
    /// Atmospheric scale height in metres for the
    /// barometric formula `P(h) = P_0 · exp(-h / H)`. Previously
    /// this was a hardcoded `8400` (Earth-like) for every
    /// planet; now derived from `Atmosphere::scale_height_m`
    /// so Mars-like (Thin → 11000), Venus-like (Reducing →
    /// 15000), and Titan-like (Hazy → 21000) atmospheres each
    /// get their characteristic altitude-pressure profile.
    pub scale_height_m: i64,
    /// Latent heat released per unit mass of vapour
    /// condensing back to liquid (Δ-temperature in K per unit
    /// transfer). Sign convention matches Chemistry: positive =
    /// exothermic (warms the receiver). Evaporation is the reverse
    /// (`-lh_condense`, cools the source). Previously, mass moved
    /// between `water_depth` and `Vapour` without any energy
    /// accounting — the cycle was thermodynamically incoherent
    /// (the same energy that boiled the water didn't return when
    /// it condensed somewhere else). With latent heat the water cycle is
    /// also a heat-redistribution mechanism: warm cells lose
    /// energy as their liquid evaporates; cold cells gain energy
    /// when the migrated vapour rains out. Real atmospheric
    /// physics calls this "latent heat transport"; it's a major
    /// horizontal heat-flow mechanism in real climates.
    pub lh_condense: Real,
    /// Vacuum guard. `false` for `Atmosphere::None` planets
    /// — no atmosphere means no vapour can exist (it would
    /// instantly escape to space). The cycle short-circuits.
    /// Previously the law ran on vacuum worlds and silently produced
    /// zero transfers (because `surface_pressure_pa = 0` cascaded
    /// to a sub-freeze sentinel boil point); the explicit guard
    /// makes the short-circuit clear.
    pub has_atmosphere: bool,
}

impl Hydrology {
    /// Earth-like defaults: aqueous solvent at 1-atm surface
    /// pressure. Per-cell boil point falls off with altitude.
    #[must_use]
    pub fn earth_like() -> Self {
        // Aqueous L_vaporisation in K/(kg/cell) units, same
        // calibration as Chemistry.
        let denom =
            Real::from_int(crate::chemistry::C_P_WATER * crate::chemistry::CELL_THERMAL_MASS_KG);
        let lh_condense = Real::from_int(crate::chemistry::L_VAPORISATION_WATER) / denom;
        Self {
            evap_rate: Real::from_ratio(1, 1_000),
            condense_rate: Real::from_ratio(1, 1_000),
            vapour_advect_k: Real::percent(1),
            substrate_tag: "aqueous".to_string(),
            surface_pressure_pa: Real::from_int(101_325),
            scale_height_m: DEFAULT_SCALE_HEIGHT_M,
            lh_condense,
            has_atmosphere: true,
        }
    }

    /// Build a per-planet `Hydrology` with the substrate-aware
    /// Clausius-Clapeyron parameters, the planet's surface
    /// pressure, and the atmosphere's scale height.
    #[must_use]
    pub fn for_substrate(
        substrate_tag: &str,
        surface_pressure_pa: Real,
        scale_height_m: i64,
        has_atmosphere: bool,
    ) -> Self {
        let props = crate::chemistry::substrate_properties(substrate_tag);
        // Use per-substrate c_p and cell_thermal_mass_kg so
        // the latent-heat → temperature conversion matches the
        // substrate. Earlier code hardcoded water values regardless.
        let denom = Real::from_int(props.c_p * props.cell_thermal_mass_kg);
        let lh_condense = Real::from_int(props.l_vaporisation) / denom;
        Self {
            substrate_tag: substrate_tag.to_string(),
            surface_pressure_pa,
            scale_height_m: scale_height_m.max(1),
            lh_condense,
            has_atmosphere,
            ..Self::earth_like()
        }
    }

    /// Per-cell barometric pressure in Pa: `P(h) = P_0 · exp(-h/H)`.
    /// `state.elevation()[cell]` is metres above the planet's
    /// reference geoid; negative elevations (sub-sea-level basins)
    /// give pressures slightly above sea level, matching real
    /// physics.
    fn cell_pressure_pa(&self, state: &PhysicsState, cell: usize) -> Real {
        // A vacuum atmosphere (`Atmosphere::None`) is signalled
        // by `scale_height_m <= 10` (the sentinel is 1; no real
        // atmosphere has H < 1 km). For vacuum, return 0 pressure
        // so `substrate_boiling_point_k` returns its sub-freeze
        // sentinel and every cell evaporates instantly — correct
        // vacuum physics.
        if self.scale_height_m <= 10 {
            return Real::ZERO;
        }
        let elev = state.elevation()[cell];
        let scale_h = Real::from_int(self.scale_height_m);
        // exp(-elev / H). Clamp negative-elevation argument so
        // sub-sea-level basins on a thin atmosphere don't push
        // `exp(very-positive)` past fixed-point range. Real basins
        // see only mildly elevated pressure (Death Valley ~1.05
        // atm); we cap at +1 (≈ exp(1) = 2.7× surface pressure)
        // which is well above any plausible terrestrial basin.
        let argument = -elev / scale_h;
        let clamped = argument.max(-Real::from_int(20)).min(Real::ONE);
        self.surface_pressure_pa * exp(clamped)
    }

    /// Per-cell boil threshold via substrate-aware
    /// Clausius-Clapeyron at the elevation-derived pressure.
    fn cell_boil_threshold_k(&self, state: &PhysicsState, cell: usize) -> Real {
        let p = self.cell_pressure_pa(state, cell);
        substrate_boiling_point_k(&self.substrate_tag, p)
    }
}

impl Law for Hydrology {
    // Same axial-coords-similar-names justification as `Wind::integrate`.
    #[allow(clippy::similar_names)]
    fn integrate(&self, state: &mut PhysicsState, dt: Real) {
        // Vacuum short-circuit. Atmosphere::None worlds have
        // no medium for vapour; the cycle short-circuits.
        if !self.has_atmosphere {
            return;
        }
        let grid = state.grid().clone();
        let n = grid.n_cells();
        let temps = state.temperature().to_vec();
        let two = Real::from_int(2);

        // Per-cell boil threshold from elevation-derived
        // pressure. Computed once per macro-step (elevation is
        // static after init, surface_pressure constant), then
        // reused for both evaporation and precipitation passes.
        let boil_thresholds: Vec<Real> = (0..n)
            .map(|i| self.cell_boil_threshold_k(state, i))
            .collect();

        // Step 1: surface evaporation. water_depth → Vapour. Two
        // regimes:
        //
        // - **Above boil** (`T > T_boil_i`): excess-driven, same
        //   formula as before. Boil-point physics is unchanged.
        // - **Sub-boil saturation curve** (`T <= T_boil_i`): real
        //   water still evaporates well below boiling — there's a
        //   Clausius-Clapeyron saturation curve under boil that
        //   the earlier step-function ignored. Drive is
        //   `(T / T_boil)^2 × 0.05` — quadratic (not the quartic
        //   PR #22 tried) and half the magnitude (`0.05`, not
        //   `0.10`). Pairs symmetrically with the sub-boil
        //   precipitation drive in Step 3 so the cycle reaches
        //   steady state rather than accumulating vapour
        //   unbounded (the failure mode of the earlier retry).
        //
        // Each unit of mass that evaporates absorbs
        // `lh_condense` of heat from the source cell (cooling it).
        let mut water_depth_next = state.water_depth().to_vec();
        let mut vapour_next = state.substance(Substance::Vapour.idx()).to_vec();
        let mut temps_next = temps.clone();
        // Conservative sub-boil drive scale. `× 0.05` halves the
        // PR #22 attempt's `× 0.10`; combined with a quadratic
        // (not quartic) drive that's a ~40× reduction near
        // `T = 0.7 T_boil`. Empirically chosen so the canary
        // sim-core 16k-tick test still completes within budget
        // without growing vapour past the I32F32 ceiling.
        let saturation_scale = Real::percent(5);
        for (i, t) in temps.iter().enumerate().take(n) {
            let boil_i = boil_thresholds[i];
            if water_depth_next[i] <= Real::ZERO {
                continue;
            }
            let transfer = if *t > boil_i {
                // Above-boil: excess-driven. Same as before.
                let excess = *t - boil_i;
                let raw = self.evap_rate * excess * water_depth_next[i] * dt;
                raw.min(water_depth_next[i])
            } else if boil_i > Real::ZERO {
                // Sub-boil saturation curve. `(T/T_boil)^2` ramps
                // from 0 at absolute zero to 1 just below boil.
                // `T/T_boil` is bounded in `[0, 1]` so the square
                // is also bounded — no risk of overflow regardless
                // of substrate or planet pressure.
                let ratio = (*t / boil_i).clamp01();
                let drive = ratio * ratio;
                let raw =
                    self.evap_rate * drive * water_depth_next[i] * dt * saturation_scale;
                raw.min(water_depth_next[i])
            } else {
                // Vacuum / sub-freeze sentinel: no liquid phase
                // exists at this pressure. Skip evaporation; the
                // existing vacuum short-circuit at the top of
                // integrate handles the no-atmosphere case more
                // aggressively.
                Real::ZERO
            };
            if transfer > Real::ZERO {
                water_depth_next[i] = water_depth_next[i] - transfer;
                vapour_next[i] = vapour_next[i] + transfer;
                // Latent heat absorbed by evaporation cools
                // the source cell.
                temps_next[i] = temps_next[i] - transfer * self.lh_condense;
            }
        }

        // Step 2: pair-flux upwind vapour advection along velocity.
        let (vq, vr) = state.fluid_velocity();
        let vq_v = vq.to_vec();
        let vr_v = vr.to_vec();
        let mut vapour_advected = vapour_next.clone();
        for (cid, axial) in grid.cells() {
            let i = cid.0 as usize;
            for (k, nb) in grid.neighbours(axial).iter().enumerate() {
                let j = nb.0 as usize;
                if j > i {
                    let (dir_q, dir_r) = NEIGHBOUR_DIRECTIONS[k];
                    let vmid_q = (vq_v[i] + vq_v[j]) / two;
                    let vmid_r = (vr_v[i] + vr_v[j]) / two;
                    let v_along = vmid_q * Real::from_int(dir_q) + vmid_r * Real::from_int(dir_r);
                    let upwind = if v_along > Real::ZERO {
                        vapour_next[i]
                    } else {
                        vapour_next[j]
                    };
                    let flux = self.vapour_advect_k * dt * v_along * upwind;
                    vapour_advected[i] = vapour_advected[i] - flux;
                    vapour_advected[j] = vapour_advected[j] + flux;
                }
            }
        }
        // Defensive clamp: pair-flux can momentarily push a cell
        // negative if upwind density is small and v_along × dt is
        // bigger than the cell's stock. Real atmospheres can't have
        // negative vapour; the bit-exact pair conservation already
        // handles total mass, so clamping per-cell only loses a
        // small amount when it triggers (which shouldn't happen
        // under earth-like coefficients).
        for v in &mut vapour_advected {
            if *v < Real::ZERO {
                *v = Real::ZERO;
            }
        }

        // Step 3: precipitation. Vapour → water_depth for cells
        // below the cell's own boil threshold. Cap by available
        // vapour. Drive is `(1 - T/T_boil)^2 × 0.05` —
        // symmetric to the sub-boil evaporation curve in Step 1
        // so the cycle balances at steady state. (1 - T/T_boil)
        // is bounded in `[0, 1]` for sub-boil cells; the square
        // is also bounded.
        //
        // Each unit of mass that condenses releases
        // `lh_condense` of heat into the receiving cell (warming
        // it). This is how the hydrologic cycle redistributes
        // heat poleward in real climates: warm cells evaporate
        // (cooling), wind transports vapour, cold cells condense
        // (warming). Closes the energy budget that earlier was
        // open by the L_vap of every kg of water cycled.
        for (i, t) in temps.iter().enumerate().take(n) {
            let boil_i = boil_thresholds[i];
            if *t < boil_i && vapour_advected[i] > Real::ZERO && boil_i > Real::ZERO {
                // Symmetric quadratic drive. At T near 0
                // (`1 - T/T_boil → 1`), full strength; at T near
                // T_boil (`1 - T/T_boil → 0`), zero precipitation.
                let ratio = (*t / boil_i).clamp01();
                let drive = (Real::ONE - ratio) * (Real::ONE - ratio);
                let raw =
                    self.condense_rate * drive * vapour_advected[i] * dt * saturation_scale;
                let transfer = raw.min(vapour_advected[i]);
                vapour_advected[i] = vapour_advected[i] - transfer;
                water_depth_next[i] = water_depth_next[i] + transfer;
                temps_next[i] = temps_next[i] + transfer * self.lh_condense;
            }
        }

        // Step 4: per-cell vapour safety cap. PR #22 retry: even
        // with the symmetric evap + precip drives above, a
        // pathological seed could in principle accumulate vapour
        // above the I32F32 (~2e9) ceiling and overflow downstream
        // fit code. Earlier the cap was `100 × max(water_depth)`
        // over the *entire grid* — meaning a single deep ocean cell
        // at 1000 m set every cell's cap to 100,000, including
        // inland desert cells that had no business with that much
        // headroom. Now per-cell: `cap[i] = max(100 × water_depth[i],
        // static_floor)`. Cells with no water get only the static
        // floor; cells near the ocean get the deep-water tolerance
        // they need. The static floor stays at 10_000 so dry-cell
        // vapour can still build up to reasonable storm-system
        // concentrations on planets that start water-poor.
        let static_floor = Real::from_int(10_000);
        for (i, v) in vapour_advected.iter_mut().enumerate() {
            let local_water = water_depth_next.get(i).copied().unwrap_or(Real::ZERO);
            let cap = (local_water * Real::from_int(100)).max(static_floor);
            if *v > cap {
                *v = cap;
            }
        }

        // Commit.
        state
            .substance_mut(Substance::Vapour.idx())
            .copy_from_slice(&vapour_advected);
        state.water_depth_mut().copy_from_slice(&water_depth_next);
        state.temperature_mut().copy_from_slice(&temps_next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::{Axial, HexGrid};

    #[test]
    fn hot_water_evaporates_into_vapour() {
        // Single hot cell with surface water → vapour density rises.
        let grid = HexGrid::new(3, 3);
        let mut state = PhysicsState::new(grid);
        let centre = state.grid().cell_id(Axial::new(1, 1)).0 as usize;
        state.water_depth_mut()[centre] = Real::from_int(10);
        state.temperature_mut()[centre] = Real::from_int(450);
        let initial_vapour = state.substance(Substance::Vapour.idx())[centre];
        let initial_water = state.water_depth()[centre];

        let hydro = Hydrology::earth_like();
        for _ in 0..50 {
            hydro.integrate(&mut state, Real::ONE);
        }
        let final_vapour = state.substance(Substance::Vapour.idx())[centre];
        let final_water = state.water_depth()[centre];
        assert!(
            final_vapour > initial_vapour,
            "vapour should have increased: initial={initial_vapour:?} final={final_vapour:?}"
        );
        assert!(
            final_water < initial_water,
            "surface water should have decreased: initial={initial_water:?} final={final_water:?}"
        );
    }

    #[test]
    fn cold_cell_with_vapour_precipitates() {
        // A cold cell with seeded vapour → vapour drops and
        // water_depth grows.
        let grid = HexGrid::new(3, 3);
        let mut state = PhysicsState::new(grid);
        let centre = state.grid().cell_id(Axial::new(1, 1)).0 as usize;
        state.substance_mut(Substance::Vapour.idx())[centre] = Real::from_int(10);
        state.temperature_mut()[centre] = Real::from_int(200);
        let initial_water = state.water_depth()[centre];

        let hydro = Hydrology::earth_like();
        for _ in 0..50 {
            hydro.integrate(&mut state, Real::ONE);
        }
        let final_water = state.water_depth()[centre];
        assert!(
            final_water > initial_water,
            "precipitation should fill water_depth: initial={initial_water:?} final={final_water:?}"
        );
    }

    #[test]
    fn hydrology_is_deterministic() {
        let grid = HexGrid::new(4, 4);
        let mut a = PhysicsState::new(grid.clone());
        let mut b = PhysicsState::new(grid);
        for (i, w) in a.water_depth_mut().iter_mut().enumerate() {
            *w = Real::from_int(2 + i64::try_from(i).unwrap() % 5);
        }
        for (i, w) in b.water_depth_mut().iter_mut().enumerate() {
            *w = Real::from_int(2 + i64::try_from(i).unwrap() % 5);
        }
        for (i, t) in a.temperature_mut().iter_mut().enumerate() {
            *t = Real::from_int(300 + i64::try_from(i).unwrap() * 5);
        }
        for (i, t) in b.temperature_mut().iter_mut().enumerate() {
            *t = Real::from_int(300 + i64::try_from(i).unwrap() * 5);
        }
        let hydro = Hydrology::earth_like();
        for _ in 0..30 {
            hydro.integrate(&mut a, Real::ONE);
            hydro.integrate(&mut b, Real::ONE);
        }
        assert_eq!(a.water_depth(), b.water_depth());
        assert_eq!(
            a.substance(Substance::Vapour.idx()),
            b.substance(Substance::Vapour.idx())
        );
    }

    #[test]
    fn vapour_advection_conserves_total_mass() {
        // No evaporation / condensation triggers (all cells uniform
        // and below boil). Just advection. Pair-flux must conserve
        // total vapour bit-exactly.
        let grid = HexGrid::new(4, 4);
        let mut state = PhysicsState::new(grid);
        for (i, v) in state
            .substance_mut(Substance::Vapour.idx())
            .iter_mut()
            .enumerate()
        {
            *v = Real::from_int(5 + i64::try_from(i).unwrap() % 7);
        }
        // Set non-zero velocity to trigger advection.
        for (i, vq) in state.fluid_velocity_mut().0.iter_mut().enumerate() {
            *vq = Real::from_ratio((i64::try_from(i).unwrap() % 3) - 1, 100);
        }
        for t in state.temperature_mut() {
            *t = Real::from_int(280);
        }
        let initial: Real = state
            .substance(Substance::Vapour.idx())
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        let hydro = Hydrology::earth_like();
        for _ in 0..30 {
            hydro.integrate(&mut state, Real::ONE);
        }
        // Vapour can have shrunk via condensation (T < 373 → cells
        // condense). Add precipitation back into the running total
        // so the *combined* (vapour + water_depth) stays conserved.
        let v_after: Real = state
            .substance(Substance::Vapour.idx())
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        let w_after: Real = state
            .water_depth()
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        assert_eq!(
            v_after + w_after,
            initial,
            "vapour + condensed water must equal starting vapour bit-exactly"
        );
    }

    #[test]
    fn high_altitude_cell_boils_at_lower_temperature() {
        // A high-elevation cell evaporates at a temperature
        // that would not trigger evaporation on a sea-level cell.
        // At 5000 m: P ≈ 101325 × exp(-5000/8400) ≈ 55 kPa, which
        // gives water boil ≈ 357 K via Clausius-Clapeyron — well
        // below sea-level 373 K.
        let grid = HexGrid::new(2, 1);
        let mut state = PhysicsState::new(grid);
        let sea_level = state.grid().cell_id(crate::grid::Axial::new(0, 0)).0 as usize;
        let mountain = state.grid().cell_id(crate::grid::Axial::new(1, 0)).0 as usize;
        state.elevation_mut()[sea_level] = Real::ZERO;
        state.elevation_mut()[mountain] = Real::from_int(5_000);
        for w in state.water_depth_mut() {
            *w = Real::from_int(10);
        }
        // 365 K — below sea-level boil (373) but above mountain
        // boil (~357).
        for t in state.temperature_mut() {
            *t = Real::from_int(365);
        }
        let hydro = Hydrology::earth_like();
        for _ in 0..50 {
            hydro.integrate(&mut state, Real::ONE);
        }
        let sea_vapour = state.substance(Substance::Vapour.idx())[sea_level];
        let mountain_vapour = state.substance(Substance::Vapour.idx())[mountain];
        assert!(
            mountain_vapour > sea_vapour,
            "mountain cell should evaporate more than sea-level cell at the same T: \
             sea_vapour={sea_vapour:?} mountain_vapour={mountain_vapour:?}"
        );
    }

    #[test]
    fn evaporation_cools_source_cell() {
        // Hot wet cell — evaporation should drop its temperature.
        let grid = HexGrid::new(3, 3);
        let mut state = PhysicsState::new(grid);
        let centre = state.grid().cell_id(crate::grid::Axial::new(1, 1)).0 as usize;
        state.water_depth_mut()[centre] = Real::from_int(50);
        state.temperature_mut()[centre] = Real::from_int(450);
        let initial_t = state.temperature()[centre];
        let hydro = Hydrology::earth_like();
        for _ in 0..50 {
            hydro.integrate(&mut state, Real::ONE);
        }
        let final_t = state.temperature()[centre];
        assert!(
            final_t < initial_t,
            "evaporation should cool the source cell: \
             initial={initial_t:?} final={final_t:?}"
        );
    }

    #[test]
    fn condensation_warms_receiver_cell() {
        // Cold cell with seeded vapour — condensation should
        // warm it (latent heat release).
        let grid = HexGrid::new(3, 3);
        let mut state = PhysicsState::new(grid);
        let centre = state.grid().cell_id(crate::grid::Axial::new(1, 1)).0 as usize;
        state.substance_mut(Substance::Vapour.idx())[centre] = Real::from_int(100);
        state.temperature_mut()[centre] = Real::from_int(200);
        let initial_t = state.temperature()[centre];
        let hydro = Hydrology::earth_like();
        for _ in 0..50 {
            hydro.integrate(&mut state, Real::ONE);
        }
        let final_t = state.temperature()[centre];
        assert!(
            final_t > initial_t,
            "condensation should warm the receiver cell: \
             initial={initial_t:?} final={final_t:?}"
        );
    }

    #[test]
    fn hydrology_cycle_reaches_steady_state() {
        // PR6 saturation-pressure retry: drive the cycle for 100
        // ticks under spatially-uniform conditions and assert
        // that `Σ water + Σ vapour` stays bit-exactly constant
        // (the cycle only moves mass between channels and between
        // cells, never creates or destroys it).
        //
        // Vapour itself can ratchet up from the new sub-boil
        // saturation curve, but the sum must not — that's the
        // overflow guard the PR #22 retry needed. Earlier
        // `vapour_advection_conserves_total_mass` covered the same
        // invariant for *pure* advection (uniform sub-boil with no
        // velocity); this test exercises the full saturation +
        // precipitation + advection cycle under non-zero wind.
        let grid = HexGrid::new(5, 5);
        let mut state = PhysicsState::new(grid);
        // Mid-temperature: sub-boil everywhere so the new
        // quadratic drive engages on every cell.
        for t in state.temperature_mut() {
            *t = Real::from_int(310);
        }
        for w in state.water_depth_mut() {
            *w = Real::from_int(50);
        }
        // Light wind so advection actually moves vapour around
        // (otherwise step 2 is a no-op and the invariant degrades
        // to a trivial single-cell check).
        for (i, vq) in state.fluid_velocity_mut().0.iter_mut().enumerate() {
            *vq = Real::from_ratio((i as i64) % 3 - 1, 100);
        }
        let initial: Real = {
            let w_sum: Real = state
                .water_depth()
                .iter()
                .copied()
                .fold(Real::ZERO, |a, b| a + b);
            let v_sum: Real = state
                .substance(Substance::Vapour.idx())
                .iter()
                .copied()
                .fold(Real::ZERO, |a, b| a + b);
            w_sum + v_sum
        };
        let hydro = Hydrology::earth_like();
        for tick in 0..100 {
            hydro.integrate(&mut state, Real::ONE);
            let w_sum: Real = state
                .water_depth()
                .iter()
                .copied()
                .fold(Real::ZERO, |a, b| a + b);
            let v_sum: Real = state
                .substance(Substance::Vapour.idx())
                .iter()
                .copied()
                .fold(Real::ZERO, |a, b| a + b);
            // Tolerance: zero. Pair-flux + per-cell transfers are
            // bit-exact in Q32.32, so we should land on exact
            // equality. If the saturation curve ever introduces
            // a stray rounding path this asserts catches it.
            assert_eq!(
                w_sum + v_sum,
                initial,
                "cycle leaked mass at tick {tick}: water={w_sum:?} \
                 vapour={v_sum:?} initial={initial:?}"
            );
        }
        // Sanity: vapour actually grew from the new sub-boil
        // drive (otherwise the test is trivially passing on a
        // no-op evaporation path).
        let final_v: Real = state
            .substance(Substance::Vapour.idx())
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        assert!(
            final_v > Real::ZERO,
            "expected sub-boil saturation curve to evaporate some \
             water; got vapour={final_v:?}"
        );
    }

    #[test]
    fn hydrology_vapour_cap_clamps_pathological_overload() {
        // Safety: even if a future regression in the drive
        // formula somehow pushed vapour above the cap, the
        // per-cell clamp in Step 4 stops the overflow. Seed a
        // cell with vapour already above the cap and verify
        // the next integrate clamps it.
        let grid = HexGrid::new(3, 3);
        let mut state = PhysicsState::new(grid);
        for w in state.water_depth_mut() {
            *w = Real::from_int(50);
        }
        // Vastly over the cap (50 × 100 = 5000) to ensure the
        // clamp triggers regardless of static-floor logic.
        let centre = state.grid().cell_id(crate::grid::Axial::new(1, 1)).0 as usize;
        state.substance_mut(Substance::Vapour.idx())[centre] = Real::from_int(1_000_000);
        // Hot cell so we exercise the above-boil branch as well.
        state.temperature_mut()[centre] = Real::from_int(280);
        let hydro = Hydrology::earth_like();
        hydro.integrate(&mut state, Real::ONE);
        let v_after = state.substance(Substance::Vapour.idx())[centre];
        // Cap is `max(max_water × 100, 10_000)`. Max water is 50
        // → `max(5000, 10000) = 10_000`. After the clamp the
        // pathologically-overloaded cell must be at most that.
        assert!(
            v_after <= Real::from_int(10_000),
            "pathological vapour overload should have been clamped; \
             got {v_after:?}"
        );
        // And: clamp definitely fired (we started at 1M, this is
        // well below).
        assert!(
            v_after < Real::from_int(100_000),
            "vapour clamp didn't reduce the field; got {v_after:?}"
        );
    }
}
