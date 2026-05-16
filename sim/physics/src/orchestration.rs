//! Operator-splitting orchestration.
//!
//! Each civ-sim tick is one sim-month. Within it, the
//! orchestrator walks `macro_steps_per_step` macro-steps (one per
//! sim-day by default, i.e. ~30 per tick on a 30-day month).
//! Within each macro-step, each law family integrates at its own
//! sub-step ratio: fluid is finest (CFL-bound), heat coarser,
//! chemistry/EM coarsest.
//!
//! Coupling order is fixed (Lie splitting, sequential): fluid first,
//! then heat reads post-fluid velocity for advection, then (M1b)
//! chemistry sees post-heat state, then EM sees post-fluid charge.
//!
//! Determinism: fixed sub-step counts, fixed coupling order, fixed
//! dt per family. No state-dependent branching.

use crate::chemistry::Chemistry;
use crate::em::Electromagnetism;
use crate::fluid::GravityFlow;
use crate::heat::HeatConduction;
use crate::laws::Law;
use crate::state::PhysicsState;
use sim_arith::Real;

/// Per-family sub-step counts within a macro-step plus per-family
/// dt values. Defaults match the operator-splitting spec.
#[derive(Debug, Clone, Copy)]
pub struct OrchestrationConfig {
    /// Number of macro-steps per civ-sim tick (= 1 month).
    /// Defaults to 30 (one macro-step per sim-day on a 30-day month).
    /// Earlier this field was named `macro_steps_per_year`; the
    /// default was 365 for the same per-day cadence at year-grained
    /// ticks. The semantic is per-tick; only the unit shifted.
    pub macro_steps_per_step: u32,
    /// Fluid sub-steps per macro-step. Defaults to 50 (~30-min dt).
    pub fluid_substeps_per_macro: u32,
    /// Heat sub-steps per macro-step. Defaults to 1 (one heat update
    /// per sim-day).
    pub heat_substeps_per_macro: u32,
    /// Macro-steps between successive chemistry sub-steps. Defaults
    /// to 7 (one chemistry update per ~7 sim-days). Most reactions
    /// don't change meaningfully on minute timescales.
    pub chemistry_macros_per_substep: u32,
    /// EM sub-steps per macro-step. Defaults to 1.
    pub em_substeps_per_macro: u32,
    /// Per-family dt values. Real-world values are derived from CFL
    /// bounds on the chosen grid; for M1a tests we use units of
    /// "macro-step" so `dt = 1/macro_substep_count` is explicit.
    pub fluid_dt: Real,
    pub heat_dt: Real,
    pub chemistry_dt: Real,
    pub em_dt: Real,
}

impl Default for OrchestrationConfig {
    fn default() -> Self {
        Self {
            // Was 365 (one per day across a year); now 30 (one
            // per day across a month). Same per-day cadence.
            macro_steps_per_step: 30,
            fluid_substeps_per_macro: 50,
            heat_substeps_per_macro: 1,
            chemistry_macros_per_substep: 7,
            em_substeps_per_macro: 1,
            fluid_dt: Real::from_ratio(1, 50),
            heat_dt: Real::ONE,
            chemistry_dt: Real::from_int(7),
            em_dt: Real::ONE,
        }
    }
}

/// Advance the physics state by one civ-sim tick (one sim-month
/// in the current cadence; was one sim-year in earlier cadences).
///
/// Walks the coupling order within each macro-step:
///
///   1. Fluid sub-steps (gravity-driven flow); mechanics applied
///      every fluid step (gravity is part of `GravityFlow`).
///   2. Heat sub-steps (diffusion). M1a heat doesn't yet read
///      post-fluid velocity for advection — that lands when fluid
///      grows real momentum tracking.
///   3. EM sub-steps (charge diffusion). M1b foundation. Charge-
///      fluid advection coupling lands in an M1b follow-up.
///   4. Chemistry sub-step every `chemistry_macros_per_substep`
///      macro-steps. Reads post-heat temperatures so phase
///      transitions reflect the latest thermal state.
#[allow(clippy::too_many_arguments)]
pub fn integrate_civ_step(
    state: &mut PhysicsState,
    cfg: &OrchestrationConfig,
    fluid: &GravityFlow,
    heat: &HeatConduction,
    em: &Electromagnetism,
    chemistry: &Chemistry,
    radiation: Option<&crate::radiation::Radiation>,
    wind: Option<&crate::wind::Wind>,
    hydrology: Option<&crate::hydrology::Hydrology>,
    tides: Option<&crate::tides::Tides>,
    magnetism: Option<&crate::magnetism::Magnetism>,
    lorentz: Option<&crate::lorentz::Lorentz>,
    coriolis: Option<&crate::coriolis::Coriolis>,
    vertical: Option<&crate::vertical::VerticalConvection>,
) {
    let chem_period = cfg.chemistry_macros_per_substep.max(1);
    for macro_step in 0..cfg.macro_steps_per_step {
        // Tides run before fluid so each macro-step starts
        // by repositioning the lunar bulge, then GravityFlow lets
        // the redistributed water settle along the terrain
        // gradient. Tides reads `state.macro_step()` (advanced at
        // the bottom of this loop body).
        if let Some(t) = tides {
            t.integrate(state, cfg.heat_dt);
        }
        for _fluid_step in 0..cfg.fluid_substeps_per_macro {
            fluid.integrate(state, cfg.fluid_dt);
        }
        // Radiative balance runs once per macro-step before
        // diffusion. Source (radiation) → diffusion (heat) → reaction
        // (chemistry) is the standard operator-splitting order so
        // each pass reads the most-current temperature.
        if let Some(rad) = radiation {
            rad.integrate(state, cfg.heat_dt);
        }
        // Wind-driven heat advection runs after radiation but
        // before molecular conduction. Wind transports the gradient
        // radiation just sourced; conduction smooths what's left.
        // Pressure and velocity fields get refreshed on every wind
        // call so downstream laws (vapour, magnetism) see
        // current state.
        if let Some(w) = wind {
            w.integrate(state, cfg.heat_dt);
        }
        // Coriolis runs right after Wind so the
        // pressure-gradient-driven velocity gets the
        // hemisphere-mirror deflection before Hydrology and
        // EM read it.
        if let Some(c) = coriolis {
            c.integrate(state, cfg.heat_dt);
        }
        // Hydrologic cycle. Surface evaporation → wind-driven
        // vapour transport (reuses the wind law's `(v_q, v_r)`) → cold-cell
        // condensation back to `water_depth` so precipitation
        // actually feeds the surface-water column GravityFlow
        // moves around. Runs after wind so it reads the freshest
        // velocity, before heat so any latent-heat refinements
        // (follow-up) see pre-conduction temperatures.
        if let Some(h) = hydrology {
            h.integrate(state, cfg.heat_dt);
        }
        for _heat_step in 0..cfg.heat_substeps_per_macro {
            heat.integrate(state, cfg.heat_dt);
        }
        // Vertical convection runs after horizontal heat
        // diffusion. Couples surface and upper-atmosphere
        // temperatures so each cell maintains a real lapse rate.
        if let Some(v) = vertical {
            v.integrate(state, cfg.heat_dt);
        }
        for _em_step in 0..cfg.em_substeps_per_macro {
            em.integrate(state, cfg.em_dt);
        }
        // Refresh the planetary magnetic vector field with
        // diurnal modulation. Runs after EM (which only touches
        // charge) so the magnetic update sees current charge state
        // when future Lorentz couplings land. Direction stays
        // fixed (axis-aligned dipole); magnitude swings ±5 % per
        // diurnal cycle.
        if let Some(m) = magnetism {
            m.integrate(state, cfg.em_dt);
        }
        // Lorentz coupling. Runs after Magnetism (so it
        // reads the freshest |B|) and after Wind (so it reads
        // and modifies the post-pressure-gradient velocity).
        // Per-cell `q · v × B` kick, no pair iteration.
        if let Some(l) = lorentz {
            l.integrate(state, cfg.em_dt);
        }
        if macro_step % chem_period == 0 {
            chemistry.integrate(state, cfg.chemistry_dt);
        }
        // Advance the planetary clock at the end of every
        // macro-step so next iteration's tides see the new phase.
        state.advance_macro_step();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::{Axial, HexGrid};

    #[test]
    fn integrate_civ_step_is_deterministic() {
        let grid = HexGrid::new(5, 5);
        let mut a = PhysicsState::new(grid.clone());
        let mut b = PhysicsState::new(grid);

        let centre = a.grid().cell_id(Axial::new(2, 2));
        a.temperature_mut()[centre.0 as usize] = Real::from_int(100);
        b.temperature_mut()[centre.0 as usize] = Real::from_int(100);
        a.elevation_mut()[centre.0 as usize] = Real::from_int(10);
        b.elevation_mut()[centre.0 as usize] = Real::from_int(10);
        a.water_depth_mut()[centre.0 as usize] = Real::from_int(5);
        b.water_depth_mut()[centre.0 as usize] = Real::from_int(5);

        let cfg = OrchestrationConfig {
            macro_steps_per_step: 12,
            fluid_substeps_per_macro: 5,
            heat_substeps_per_macro: 1,
            chemistry_macros_per_substep: 7,
            em_substeps_per_macro: 1,
            fluid_dt: Real::from_ratio(1, 50),
            heat_dt: Real::percent(1),
            chemistry_dt: Real::from_int(7),
            em_dt: Real::percent(1),
        };
        let fluid = GravityFlow::earth_like();
        let heat = HeatConduction {
            alpha: Real::from_ratio(1, 10),
        };
        let em = Electromagnetism::earth_like();
        let chem = Chemistry::earth_like_water();

        integrate_civ_step(
            &mut a, &cfg, &fluid, &heat, &em, &chem, None, None, None, None, None, None, None, None,
        );
        integrate_civ_step(
            &mut b, &cfg, &fluid, &heat, &em, &chem, None, None, None, None, None, None, None, None,
        );

        assert_eq!(a.temperature(), b.temperature());
        assert_eq!(a.water_depth(), b.water_depth());
    }
}
