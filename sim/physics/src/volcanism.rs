//! Volcanic CO2 + H2O outgassing (Sprint 4 Item 12d).
//!
//! Closes the carbon-silicate cycle loop: where `Weathering` (Item
//! 14a) removes atmospheric CO2 each tick, `Volcanism` returns it.
//! Without this source, a habitable seed would slowly drift toward
//! a frozen-out CO2-depleted state over geological time as the
//! one-sided weathering sink drains the atmosphere. The pairing of
//! the two sets up the negative-feedback thermostat that keeps real
//! Earth's climate (and any earth-like seed here) bounded.
//!
//! ## Two emission pathways
//!
//! 1. Boundary volcanism - every cell sitting at a plate
//!    boundary (a neighbour belongs to a different plate) emits a
//!    deterministic per-tick rate of CO2 and H2O. Convergent
//!    boundaries (subduction zones) and divergent boundaries
//!    (mid-ocean ridges, rifts) both qualify.
//!
//! 2. Hot-spot volcanism - non-boundary cells get a rare
//!    deterministic SplitMix64 trial at 1e-5 probability per tick.
//!    On success the cell emits a 10x larger dose.
//!
//! ## Determinism
//!
//! Boundary emission is purely deterministic. Hot-spot trial is
//! deterministic per (cell_id, macro_step) via a SplitMix64
//! finaliser salted with `VOLCANISM_SALT`.

use crate::chemistry::Substance;
use crate::state::PhysicsState;
use sim_arith::Real;

/// Per-tick per-boundary-cell CO2 emission rate numerator.
pub const VOLCANIC_CO2_NUM: i64 = 1;
/// Per-tick per-boundary-cell CO2 emission rate denominator.
pub const VOLCANIC_CO2_DEN: i64 = 100_000;
/// Per-tick per-boundary-cell H2O emission rate numerator.
pub const VOLCANIC_H2O_NUM: i64 = 5;
/// Per-tick per-boundary-cell H2O emission rate denominator.
pub const VOLCANIC_H2O_DEN: i64 = 100_000;
/// Hot-spot trial numerator (1e-5 probability per tick).
pub const HOT_SPOT_PROBABILITY_NUM: u64 = 10;
/// Hot-spot trial denominator (1e-5 probability per tick).
pub const HOT_SPOT_PROBABILITY_DEN: u64 = 1_000_000;
/// Per-eruption CO2 dose numerator.
pub const HOT_SPOT_CO2_NUM: i64 = 1;
/// Per-eruption CO2 dose denominator.
pub const HOT_SPOT_CO2_DEN: i64 = 10_000;
/// Per-eruption H2O dose numerator.
pub const HOT_SPOT_H2O_NUM: i64 = 5;
/// Per-eruption H2O dose denominator.
pub const HOT_SPOT_H2O_DEN: i64 = 10_000;

/// SplitMix64 salt for the hot-spot trial stream.
const VOLCANISM_SALT: u64 = 0xC0DE_F00D_70CC_A101;

/// Volcanic-emission law.
#[derive(Debug, Clone)]
pub struct Volcanism {
    pub boundary_co2_rate: Real,
    pub boundary_h2o_rate: Real,
    pub hot_spot_probability_num: u64,
    pub hot_spot_probability_den: u64,
    pub hot_spot_co2: Real,
    pub hot_spot_h2o: Real,
    pub seed_salt: u64,
}

/// Aggregate mass added by one Volcanism::integrate call.
#[derive(Debug, Clone, Copy)]
pub struct VolcanismEmission {
    pub co2_added: Real,
    pub h2o_added: Real,
}

impl Volcanism {
    /// Earth-like calibration.
    #[must_use]
    pub fn earth_like() -> Self {
        Self::for_planet(Real::ONE)
    }

    /// Planet-scale calibration. `area_factor = radius²` lifts the
    /// per-tick eruption rates (both the deterministic plate-boundary
    /// CO₂/H₂O emission and the hot-spot trial probability) in
    /// proportion to surface area: a bigger world has proportionally
    /// more plate-boundary length and more hot-spot targets, so per-
    /// century volcanic activity scales with `area`. Earth (factor
    /// 1.0) leaves every coefficient at the legacy `earth_like()`
    /// baseline byte-for-byte. The hot-spot probability is scaled by
    /// lifting the numerator (denominator fixed) so the integer
    /// modulo trial stays well-defined.
    #[must_use]
    pub fn for_planet(area_factor: Real) -> Self {
        let base_co2 = Real::from_ratio(VOLCANIC_CO2_NUM, VOLCANIC_CO2_DEN);
        let base_h2o = Real::from_ratio(VOLCANIC_H2O_NUM, VOLCANIC_H2O_DEN);
        let base_hs_co2 = Real::from_ratio(HOT_SPOT_CO2_NUM, HOT_SPOT_CO2_DEN);
        let base_hs_h2o = Real::from_ratio(HOT_SPOT_H2O_NUM, HOT_SPOT_H2O_DEN);
        // Hot-spot probability scale-up: multiply the numerator (raw
        // u64) so the modulo trial stays in the existing integer
        // domain. Earth area_factor = 1.0 → numerator unchanged.
        // Floored at 1 so even a near-zero area_factor still has a
        // (tiny) firing probability rather than overflowing to zero.
        let factor_x100: u64 = {
            // Convert area_factor (Real) into u64-hundredths via the
            // deterministic integer-only path; degenerate negatives
            // (a test planet with radius below 0) collapse to zero
            // so the saturating scale below keeps the law well-defined.
            let scaled: i64 = (area_factor * Real::from_int(100))
                .raw()
                .to_num();
            u64::try_from(scaled.max(0)).unwrap_or(0)
        };
        let hs_num_scaled = (HOT_SPOT_PROBABILITY_NUM
            .saturating_mul(factor_x100)
            / 100)
            .max(1);
        Self {
            boundary_co2_rate: base_co2 * area_factor,
            boundary_h2o_rate: base_h2o * area_factor,
            hot_spot_probability_num: hs_num_scaled,
            hot_spot_probability_den: HOT_SPOT_PROBABILITY_DEN,
            hot_spot_co2: base_hs_co2 * area_factor,
            hot_spot_h2o: base_hs_h2o * area_factor,
            seed_salt: VOLCANISM_SALT,
        }
    }

    /// Apply one volcanic-emission step.
    pub fn integrate(&self, state: &mut PhysicsState, dt: Real) -> VolcanismEmission {
        let n = state.grid().n_cells();
        if state.plate_id().len() != n {
            return VolcanismEmission {
                co2_added: Real::ZERO,
                h2o_added: Real::ZERO,
            };
        }
        let grid = state.grid().clone();
        let plate_ids = state.plate_id().to_vec();
        let macro_step = state.macro_step();
        let mut co2_added = Real::ZERO;
        let mut h2o_added = Real::ZERO;
        let mut at_boundary = vec![false; n];
        let mut hot_spot_fired = vec![false; n];
        for (cid, axial) in grid.cells() {
            let i = cid.0 as usize;
            let plate_i = plate_ids[i];
            let mut boundary = false;
            for nb in grid.neighbours(axial).iter() {
                let j = nb.0 as usize;
                if plate_ids[j] != plate_i {
                    boundary = true;
                    break;
                }
            }
            at_boundary[i] = boundary;
            if !boundary {
                let mut s = self
                    .seed_salt
                    .wrapping_add(macro_step.wrapping_mul(0x9E37_79B9_7F4A_7C15))
                    .wrapping_add((i as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9));
                let roll = next_u64(&mut s) % self.hot_spot_probability_den;
                if roll < self.hot_spot_probability_num {
                    hot_spot_fired[i] = true;
                }
            }
        }
        let co2_boundary = self.boundary_co2_rate * dt;
        let co2_hotspot = self.hot_spot_co2 * dt;
        {
            let co2 = state.substance_mut(Substance::CO2.idx());
            for i in 0..n {
                if at_boundary[i] {
                    co2[i] = co2[i] + co2_boundary;
                    co2_added = co2_added + co2_boundary;
                } else if hot_spot_fired[i] {
                    co2[i] = co2[i] + co2_hotspot;
                    co2_added = co2_added + co2_hotspot;
                }
            }
        }
        let h2o_boundary = self.boundary_h2o_rate * dt;
        let h2o_hotspot = self.hot_spot_h2o * dt;
        {
            let vapour = state.substance_mut(Substance::Vapour.idx());
            for i in 0..n {
                if at_boundary[i] {
                    vapour[i] = vapour[i] + h2o_boundary;
                    h2o_added = h2o_added + h2o_boundary;
                } else if hot_spot_fired[i] {
                    vapour[i] = vapour[i] + h2o_hotspot;
                    h2o_added = h2o_added + h2o_hotspot;
                }
            }
        }
        VolcanismEmission { co2_added, h2o_added }
    }
}

fn next_u64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::{Axial, HexGrid};
    use crate::weathering::Weathering;

    #[test]
    fn volcanism_emits_co2_at_subduction_zones() {
        let grid = HexGrid::new(6, 3);
        let n = grid.n_cells();
        let mut state = PhysicsState::new(grid.clone());
        let mut plate_id = vec![0u32; n];
        for (cid, axial) in grid.cells() {
            plate_id[cid.0 as usize] = if axial.q < 3 { 0 } else { 1 };
        }
        let crust_thickness = vec![Real::from_int(35); n];
        state.set_tectonics_fields(plate_id, crust_thickness);
        let boundary_cell = state.grid().cell_id(Axial::new(2, 1)).0 as usize;
        let co2_before_boundary = state.substance(Substance::CO2.idx())[boundary_cell];
        let vapour_before_boundary = state.substance(Substance::Vapour.idx())[boundary_cell];
        let volcanism = Volcanism::earth_like();
        let emission = volcanism.integrate(&mut state, Real::ONE);
        let co2_after_boundary = state.substance(Substance::CO2.idx())[boundary_cell];
        let vapour_after_boundary = state.substance(Substance::Vapour.idx())[boundary_cell];
        assert!(co2_after_boundary > co2_before_boundary);
        assert!(vapour_after_boundary > vapour_before_boundary);
        assert!(emission.co2_added > Real::ZERO);
        assert!(emission.h2o_added > Real::ZERO);
        // H2O:CO2 ~5:1 by construction. Fixed-point Q32.32 rounding
        // on the small per-tick rates introduces single-LSB drift so
        // we use approximate ratio bounds rather than bit-equality.
        let ratio = emission.h2o_added / emission.co2_added;
        assert!(ratio > Real::from_ratio(49, 10) && ratio < Real::from_ratio(51, 10),
            "H2O:CO2 ratio should be ~5: got {ratio:?}");
    }

    #[test]
    fn weathering_volcanism_balance_holds_earth_like_co2() {
        let grid = HexGrid::new(6, 4);
        let n = grid.n_cells();
        let mut state = PhysicsState::new(grid.clone());
        let mut plate_id = vec![0u32; n];
        for (cid, axial) in grid.cells() {
            plate_id[cid.0 as usize] = if axial.q < 3 { 0 } else { 1 };
        }
        let crust_thickness = vec![Real::from_int(35); n];
        state.set_tectonics_fields(plate_id, crust_thickness);
        // Earth-like moderate seed: T at reference (290 K) so
        // T_factor = 1.0; humidity near ref_humidity so precip_factor
        // = 0.5. Neither weathering nor volcanism is artificially
        // saturated; the balance test exercises the natural earth-
        // like equilibrium where the steady-state CO2 stays bounded
        // by the negative feedback alone.
        for t in state.temperature_mut() { *t = Real::from_int(290); }
        for w in state.water_depth_mut() { *w = Real::from_int(250); }
        for v in state.substance_mut(Substance::Vapour.idx()) { *v = Real::from_int(250); }
        for c in state.substance_mut(Substance::CO2.idx()) { *c = Real::from_ratio(1, 10); }
        let initial_co2: Real = state.substance(Substance::CO2.idx()).iter().copied().fold(Real::ZERO, |a, b| a + b);
        let weathering = Weathering::earth_like();
        let volcanism = Volcanism::earth_like();
        for _ in 0..1000 {
            let _ = volcanism.integrate(&mut state, Real::ONE);
            // Replenish hydrology so the precip factor stays steady
            // tick over tick. Without this, weathering would drain
            // its own driver and the comparison would flip.
            for w in state.water_depth_mut() { *w = Real::from_int(250); }
            for v in state.substance_mut(Substance::Vapour.idx()) { *v = Real::from_int(250); }
            let _ = weathering.integrate(&mut state, Real::ONE);
        }
        let final_co2: Real = state.substance(Substance::CO2.idx()).iter().copied().fold(Real::ZERO, |a, b| a + b);
        let upper = initial_co2 * Real::from_int(10);
        let lower = initial_co2 / Real::from_int(10);
        assert!(final_co2 < upper && final_co2 > lower, "final={final_co2:?} bounds=[{lower:?}, {upper:?}]");
        assert!(state.substance(Substance::CO2.idx()).iter().all(|c| *c >= Real::ZERO));
        let _ = n;
    }

    #[test]
    fn volcanism_is_deterministic() {
        let grid = HexGrid::new(8, 6);
        let n = grid.n_cells();
        let mut state_a = PhysicsState::new(grid.clone());
        let mut state_b = PhysicsState::new(grid);
        let mut plate_id = vec![0u32; n];
        for (cid, axial) in state_a.grid().cells() {
            plate_id[cid.0 as usize] = if axial.q < 4 { 0 } else { 1 };
        }
        let crust_thickness = vec![Real::from_int(35); n];
        state_a.set_tectonics_fields(plate_id.clone(), crust_thickness.clone());
        state_b.set_tectonics_fields(plate_id, crust_thickness);
        let volcanism = Volcanism::earth_like();
        for _ in 0..50 {
            let _ = volcanism.integrate(&mut state_a, Real::ONE);
            let _ = volcanism.integrate(&mut state_b, Real::ONE);
            state_a.advance_macro_step();
            state_b.advance_macro_step();
        }
        assert_eq!(state_a.substance(Substance::CO2.idx()), state_b.substance(Substance::CO2.idx()));
        assert_eq!(state_a.substance(Substance::Vapour.idx()), state_b.substance(Substance::Vapour.idx()));
    }

    #[test]
    fn volcanism_noops_without_plates() {
        let grid = HexGrid::new(4, 3);
        let mut state = PhysicsState::new(grid);
        let co2_before: Vec<Real> = state.substance(Substance::CO2.idx()).to_vec();
        let volcanism = Volcanism::earth_like();
        let emission = volcanism.integrate(&mut state, Real::ONE);
        assert_eq!(state.substance(Substance::CO2.idx()), co2_before.as_slice());
        assert_eq!(emission.co2_added, Real::ZERO);
        assert_eq!(emission.h2o_added, Real::ZERO);
    }
}
