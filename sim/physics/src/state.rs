//! Per-cell physics state.
//!
//! Storage is array-of-arrays (one Vec per field) rather than
//! array-of-structs, for cache friendliness and so each integrator
//! can iterate just the fields it needs without walking unrelated
//! ones.
//!
//! M1a fields: temperature, pressure, fluid velocity (q, r
//! components in axial coordinates). Mechanics doesn't add a field
//! — it acts as gravitational forcing on fluid velocity each
//! sub-step. M1b adds charge and per-substance density vectors.

use crate::grid::{CellId, HexGrid};
use sim_arith::Real;

/// A view of a single cell's M1a state. Returned by lookups; not the
/// canonical storage.
#[derive(Debug, Clone, Copy)]
pub struct Cell {
    pub elevation: Real,
    pub water_depth: Real,
    pub temperature: Real,
    pub pressure: Real,
    pub fluid_v_q: Real,
    pub fluid_v_r: Real,
}

/// Number of named-substance density tracks held in
/// `PhysicsState::substances`. The integer indices align with the
/// `Substance` enum in `crate::chemistry`. Grows as new substances
/// are authored — keep this in sync with the enum.
pub const N_SUBSTANCES: usize = 8;

#[derive(Debug, Clone)]
pub struct PhysicsState {
    grid: HexGrid,
    /// Terrain height. Set at run start from planet seed; not
    /// updated by M1a physics (tectonics is permanently deferred).
    elevation: Vec<Real>,
    /// Water column depth above the terrain. Mutated by fluid law.
    water_depth: Vec<Real>,
    temperature: Vec<Real>,
    pressure: Vec<Real>,
    fluid_v_q: Vec<Real>,
    fluid_v_r: Vec<Real>,
    /// Per-substance density at each cell. Outer index is the
    /// substance id (a `crate::chemistry::Substance` cast to usize);
    /// inner index is the cell index. Mutated by chemistry law.
    substances: Vec<Vec<Real>>,
    /// Electric charge per cell. Signed; mutated by EM law (M1b).
    charge: Vec<Real>,
    /// Planetary magnetic field as a vector — `(B_q, B_r)`
    /// per cell in axial coordinates. Previously this was a scalar
    /// magnitude with no direction; promoting to a vector lets the
    /// dipole pattern (axis-aligned, equatorially horizontal) carry
    /// through to any future Lorentz-force coupling on charged
    /// particle motion. Initialised at planet init by
    /// `Magnetism::init_field`; modulated each macro-step by
    /// `Magnetism::integrate` for diurnal variation.
    magnetic_b_q: Vec<Real>,
    magnetic_b_r: Vec<Real>,
    /// Vertical-axis (out-of-plane) component of the
    /// magnetic vector field. Real planetary dipoles have a
    /// vertical component that peaks at the magnetic *poles*
    /// (where the horizontal components vanish) and is zero at
    /// the equator. Previously the Lorentz law approximated
    /// this with `|B|` (the horizontal magnitude), which was
    /// wrong at the poles where `|B_horizontal|` ≈ 0 but `B_z` is
    /// largest. With `B_z` as a real field, the Lorentz law reads the true
    /// vertical component for the cross-product physics.
    magnetic_b_z: Vec<Real>,
    /// Cached per-cell magnetic-field magnitude
    /// (`sqrt(B_q² + B_r²)`). Updated by `Magnetism::init_field`
    /// and `Magnetism::integrate` whenever they touch the vector
    /// components. Cached because the recognition scan reads
    /// per-cell magnitude on every tick × every template; the
    /// uncached version added ~50 % to the long-test runtime.
    /// Maintenance contract: any law that writes
    /// `magnetic_b_q` / `magnetic_b_r` must also refresh
    /// `magnetic_magnitude`.
    magnetic_magnitude: Vec<Real>,
    /// Macro-step counter used by laws that need a planet-wide
    /// clock (tides, seasonal insolation, diurnal cycles). Advanced
    /// once per macro-step by `orchestration::integrate_civ_step`.
    /// Stored as u64 so a million-year run (~1.2×10⁸ macro-steps)
    /// fits with margin to spare.
    macro_step: u64,
    /// Per-cell upper-atmosphere temperature in K. Sits one
    /// layer above the surface `temperature` field; coupled to it
    /// by vertical convection (`VerticalConvection` law). Previously
    /// the simulation had only one temperature field per cell —
    /// no vertical structure, no lapse rate, no high-altitude
    /// cloud / vapour layering. This field introduces a single upper
    /// layer as the minimum-viable vertical resolution; future
    /// refinements can grow this into a multi-level stack.
    upper_temperature: Vec<Real>,
    /// Per-cell biofuel carrying-capacity ceiling. Set at planet
    /// init from the cell's biosphere contribution (land + non-zero
    /// biosphere class → positive ceiling; sea + lifeless worlds →
    /// zero). Read by the `BiofuelRegrowth` reaction as the upper
    /// bound a regrowing cell relaxes toward; never mutated by
    /// physics laws. Stored per-cell rather than recomputed because
    /// future tuning may want spatial variation (terrain, microclimate
    /// modifiers) that's awkward to derive from scratch each tick.
    biofuel_ceiling: Vec<Real>,
}

impl PhysicsState {
    /// Build a fresh state with the given grid; all fields zero. Real
    /// runs initialise from planet seed before the first integration;
    /// tests construct directly.
    pub fn new(grid: HexGrid) -> Self {
        let n = grid.n_cells();
        Self {
            elevation: vec![Real::ZERO; n],
            water_depth: vec![Real::ZERO; n],
            temperature: vec![Real::ZERO; n],
            pressure: vec![Real::ZERO; n],
            fluid_v_q: vec![Real::ZERO; n],
            fluid_v_r: vec![Real::ZERO; n],
            substances: vec![vec![Real::ZERO; n]; N_SUBSTANCES],
            charge: vec![Real::ZERO; n],
            magnetic_b_q: vec![Real::ZERO; n],
            magnetic_b_r: vec![Real::ZERO; n],
            magnetic_b_z: vec![Real::ZERO; n],
            magnetic_magnitude: vec![Real::ZERO; n],
            macro_step: 0,
            upper_temperature: vec![Real::ZERO; n],
            biofuel_ceiling: vec![Real::ZERO; n],
            grid,
        }
    }

    /// Current macro-step counter. Laws that need a planet-wide
    /// clock (tides, seasonal forcing) read this. Advanced exclusively
    /// by `orchestration::integrate_civ_step`; never written by laws.
    #[must_use]
    pub fn macro_step(&self) -> u64 {
        self.macro_step
    }

    /// Bump the macro-step counter. Only the orchestrator should
    /// call this — once per macro-step, after the law sequence runs.
    pub fn advance_macro_step(&mut self) {
        self.macro_step = self.macro_step.saturating_add(1);
    }

    pub fn grid(&self) -> &HexGrid {
        &self.grid
    }

    pub fn cell(&self, id: CellId) -> Cell {
        let i = id.0 as usize;
        Cell {
            elevation: self.elevation[i],
            water_depth: self.water_depth[i],
            temperature: self.temperature[i],
            pressure: self.pressure[i],
            fluid_v_q: self.fluid_v_q[i],
            fluid_v_r: self.fluid_v_r[i],
        }
    }

    pub fn elevation(&self) -> &[Real] {
        &self.elevation
    }

    pub fn elevation_mut(&mut self) -> &mut [Real] {
        &mut self.elevation
    }

    pub fn water_depth(&self) -> &[Real] {
        &self.water_depth
    }

    pub fn water_depth_mut(&mut self) -> &mut [Real] {
        &mut self.water_depth
    }

    pub fn temperature(&self) -> &[Real] {
        &self.temperature
    }

    pub fn temperature_mut(&mut self) -> &mut [Real] {
        &mut self.temperature
    }

    pub fn pressure(&self) -> &[Real] {
        &self.pressure
    }

    pub fn pressure_mut(&mut self) -> &mut [Real] {
        &mut self.pressure
    }

    pub fn fluid_velocity(&self) -> (&[Real], &[Real]) {
        (&self.fluid_v_q, &self.fluid_v_r)
    }

    pub fn fluid_velocity_mut(&mut self) -> (&mut [Real], &mut [Real]) {
        (&mut self.fluid_v_q, &mut self.fluid_v_r)
    }

    /// Density of the substance at index `id` for every cell.
    pub fn substance(&self, id: usize) -> &[Real] {
        &self.substances[id]
    }

    pub fn substance_mut(&mut self, id: usize) -> &mut [Real] {
        &mut self.substances[id]
    }

    pub fn charge(&self) -> &[Real] {
        &self.charge
    }

    pub fn charge_mut(&mut self) -> &mut [Real] {
        &mut self.charge
    }

    /// Vector magnetic field — `(B_q, B_r)` slices in axial
    /// coordinates. Previously this returned a scalar magnitude; the
    /// vector form lets directional couplings (Lorentz force,
    /// magnetic compass templates) read the orientation as well.
    /// `magnetic_field_magnitude(cell)` derives the scalar magnitude
    /// for callers that only want it.
    #[must_use]
    pub fn magnetic_field(&self) -> (&[Real], &[Real]) {
        (&self.magnetic_b_q, &self.magnetic_b_r)
    }

    pub fn magnetic_field_mut(&mut self) -> (&mut [Real], &mut [Real]) {
        (&mut self.magnetic_b_q, &mut self.magnetic_b_r)
    }

    /// Vertical (out-of-plane) magnetic-field component.
    /// Read by the Lorentz coupling for the true 3D
    /// `F = q · v × B` cross-product instead of the previous
    /// `|B|` proxy.
    #[must_use]
    pub fn magnetic_field_z(&self) -> &[Real] {
        &self.magnetic_b_z
    }

    pub fn magnetic_field_z_mut(&mut self) -> &mut [Real] {
        &mut self.magnetic_b_z
    }

    /// Scalar magnitude of the magnetic field at a cell —
    /// `sqrt(B_q² + B_r²)`. Returns the cached
    /// `magnetic_magnitude[cell]` rather than recomputing the
    /// sqrt each call. The cache is refreshed by
    /// `Magnetism::init_field` / `Magnetism::integrate`; tests
    /// that mutate the vector components directly should call
    /// `refresh_magnetic_magnitude` (test-only helper) before
    /// reading.
    #[must_use]
    pub fn magnetic_field_magnitude(&self, cell: usize) -> Real {
        self.magnetic_magnitude[cell]
    }

    /// Full magnitude slice (caller-side bulk reads). Same
    /// caching contract as `magnetic_field_magnitude`.
    #[must_use]
    pub fn magnetic_magnitude(&self) -> &[Real] {
        &self.magnetic_magnitude
    }

    /// Write-side accessor for the cached magnitude. Only
    /// `Magnetism` (or test code mirroring its updates) should
    /// touch this; recognition reads via `magnetic_magnitude()`.
    pub fn magnetic_magnitude_mut(&mut self) -> &mut [Real] {
        &mut self.magnetic_magnitude
    }

    /// Per-cell upper-atmosphere temperature in K. Previously
    /// the simulation was vertically homogeneous; this slice gives
    /// vertical-convection laws somewhere to put the lapse-rate
    /// signal. Initialised to zero and seeded at planet init or
    /// by the first `VerticalConvection::integrate` pass.
    #[must_use]
    pub fn upper_temperature(&self) -> &[Real] {
        &self.upper_temperature
    }

    pub fn upper_temperature_mut(&mut self) -> &mut [Real] {
        &mut self.upper_temperature
    }

    /// Per-cell biofuel ceiling (regrowth target). Set once at
    /// planet init from biosphere class + land mask; constant
    /// across a run.
    #[must_use]
    pub fn biofuel_ceiling(&self) -> &[Real] {
        &self.biofuel_ceiling
    }

    pub fn biofuel_ceiling_mut(&mut self) -> &mut [Real] {
        &mut self.biofuel_ceiling
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_state_zeroed() {
        let g = HexGrid::new(3, 3);
        let s = PhysicsState::new(g);
        for t in s.temperature() {
            assert_eq!(*t, Real::ZERO);
        }
    }
}
