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
//!
//! ## Conservation invariants (debug-mode runtime asserts)
//!
//! Substance transport, chemistry, hydrology, and tides are all
//! pair-flux or stoichiometric-redistribution kernels that should
//! preserve mass bit-exactly under Q32.32 fixed-point arithmetic.
//! A regression in any of them would silently leak mass and
//! manifest as downstream weirdness thousands of ticks later. To
//! catch leaks at the first tick of drift, the orchestrator snapshots
//! the relevant conservation total before each kernel and asserts
//! `< 1e-6` drift afterwards via `debug_assert!`. These are
//! zero-cost in release builds and budget ~30 s of overhead on the
//! 16k-tick canary in debug builds — a reasonable trade for fast
//! regression detection.
//!
//! The invariants checked:
//!
//! - `Tides`: `Σ water_depth` (donor-limited pair-flux is bit-exact).
//! - `Hydrology`: `Σ water_depth + Σ Substance::Vapour` (evap +
//!   pair-flux advect + condense only redistribute between channels).
//! - `Chemistry`: `Σ over all substances` (combustion 1+1→2,
//!   biofuel regrowth 2→1+1, and phase transitions are all
//!   stoichiometrically mass-conservative).

use crate::chemistry::Chemistry;
#[cfg(debug_assertions)]
use crate::chemistry::Substance;
use crate::em::Electromagnetism;
use crate::fluid::GravityFlow;
use crate::heat::HeatConduction;
use crate::laws::Law;
use crate::state::PhysicsState;
#[cfg(debug_assertions)]
use crate::state::N_SUBSTANCES;
use sim_arith::Real;

/// Sum of every per-cell entry across every substance channel.
/// Used by the debug-mode conservation asserts wrapping each
/// kernel's `integrate` call so a regression in pair-flux or
/// chemistry stoichiometry trips on the *first* tick of drift
/// rather than as downstream weirdness ~10k ticks later.
///
/// Zero-cost in release builds (the calls are inside
/// `debug_assert!`-gated closures the optimiser strips).
#[cfg(debug_assertions)]
fn total_substance_mass(state: &PhysicsState) -> Real {
    let mut total = Real::ZERO;
    for s in 0..N_SUBSTANCES {
        for v in state.substance(s) {
            total = total + *v;
        }
    }
    total
}

/// Total surface water column. Tides conserve this exactly under
/// donor-limited pair-flux (PR1); the orchestrator-level assert
/// catches future regressions in that loop.
#[cfg(debug_assertions)]
fn total_water_depth(state: &PhysicsState) -> Real {
    state
        .water_depth()
        .iter()
        .copied()
        .fold(Real::ZERO, |a, b| a + b)
}

/// `Σ water_depth + Σ Substance::Vapour`. Hydrology's three-step
/// cycle (evaporation → advection → condensation) moves mass
/// between these two channels and never creates or destroys any,
/// so the sum is invariant across every `Hydrology::integrate`
/// call. Surface water that evaporates becomes vapour; vapour
/// that advects out of one cell lands in a neighbour; vapour that
/// precipitates becomes water_depth. A regression in any of the
/// three steps trips this assert.
#[cfg(debug_assertions)]
fn total_water_plus_vapour(state: &PhysicsState) -> Real {
    let water = total_water_depth(state);
    let vapour = state
        .substance(Substance::Vapour.idx())
        .iter()
        .copied()
        .fold(Real::ZERO, |a, b| a + b);
    water + vapour
}

/// Conservation drift tolerance for the debug-mode asserts.
/// Q32.32 fixed-point pair-flux is bit-exact, so the expected
/// drift is *zero* — `1e-6` is a generous safety margin that
/// would still flag any structural leak well before it accumulates
/// into a visible deviation. Real::from_ratio(1, 1_000_000).
#[cfg(debug_assertions)]
fn drift_tolerance() -> Real {
    Real::from_ratio(1, 1_000_000)
}

/// Conservation drift tolerance for hydrology specifically.
/// Hydrology has TWO documented mass-truncating clamps that the
/// bit-exact pair-flux invariant doesn't cover:
///
/// 1. The negative-vapour clamp in `Hydrology::integrate` step 2
///    (post-advection): if pair-flux ever drives a cell's vapour
///    below zero (which shouldn't happen under any earth-like
///    coefficients but is guarded for pathological tide_k /
///    advect_k values), the value is reset to zero — silently
///    creating mass.
///
/// 2. The per-cell vapour cap in step 4: if a cell's vapour
///    exceeds `max(water_depth × 100, 10_000)` (a fixed-point
///    safety bound), the excess is discarded — silently
///    destroying mass.
///
/// Neither clamp fires under earth-like coefficients (verified
/// by `hydrology_cycle_reaches_steady_state`). But on extreme
/// substrates (methane-boil at 112 K, ammonia at 240 K) the
/// saturation curve can push toward those bounds. Use a more
/// generous tolerance for hydrology so the assert catches
/// structural leaks (10× tighter than the cap magnitudes) without
/// false-positive panicking on legitimate clamp firings.
///
/// Scales with total water+vapour so the tolerance is meaningful
/// for both small dev grids and large prod grids. Returns
/// `total × 0.001` — well below the clamp magnitudes (which would
/// be visible as 1%+ drift) but well above the bit-exact pair-flux
/// drift.
#[cfg(debug_assertions)]
fn hydrology_drift_tolerance(total_water_plus_vapour: Real) -> Real {
    let scaled = total_water_plus_vapour * Real::from_ratio(1, 1000);
    // Floor at the tighter pair-flux tolerance so we still catch
    // structural bugs on essentially-dry planets.
    scaled.max(drift_tolerance())
}

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
            // Conservation invariant: pair-flux tides only move
            // water between cells, never create or destroy it.
            // Donor-limited (PR1) ensures bit-exact preservation
            // even under pathological coefficients; the debug
            // assert catches future regressions on the first tick
            // of drift.
            #[cfg(debug_assertions)]
            let pre_water = total_water_depth(state);
            t.integrate(state, cfg.heat_dt);
            #[cfg(debug_assertions)]
            {
                let post_water = total_water_depth(state);
                debug_assert!(
                    (post_water - pre_water).abs() < drift_tolerance(),
                    "tides leaked water: pre={pre_water:?} post={post_water:?}"
                );
            }
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
            // Conservation invariant: the hydrologic cycle moves
            // mass between `water_depth` and `Substance::Vapour`
            // (evaporation, condensation) and shuffles vapour
            // between cells (pair-flux advection); it never
            // creates or destroys total `water + vapour`. A
            // regression in any of the three steps trips this
            // assert.
            #[cfg(debug_assertions)]
            let pre_wv = total_water_plus_vapour(state);
            h.integrate(state, cfg.heat_dt);
            #[cfg(debug_assertions)]
            {
                let post_wv = total_water_plus_vapour(state);
                // Use the more generous hydrology-specific
                // tolerance: the two documented vapour clamps in
                // step 2 + step 4 are allowed to truncate small
                // amounts on extreme substrates without panicking.
                // Structural leaks still trip the 0.1%-of-total
                // bound.
                let tol = hydrology_drift_tolerance(pre_wv);
                debug_assert!(
                    (post_wv - pre_wv).abs() < tol,
                    "hydrology leaked mass: pre={pre_wv:?} post={post_wv:?} tol={tol:?}"
                );
            }
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
            // Conservation invariant: combustion (1 fuel + 1
            // oxidiser → 2 ash), biofuel regrowth (2 ash → 1
            // fuel + 1 oxidiser), and phase transitions
            // (water ↔ ice ↔ vapour) all preserve total
            // substance mass bit-exactly. The summed
            // `Σ over all substances` is invariant across
            // every chemistry call.
            #[cfg(debug_assertions)]
            let pre_mass = total_substance_mass(state);
            chemistry.integrate(state, cfg.chemistry_dt);
            #[cfg(debug_assertions)]
            {
                let post_mass = total_substance_mass(state);
                debug_assert!(
                    (post_mass - pre_mass).abs() < drift_tolerance(),
                    "chemistry leaked mass: pre={pre_mass:?} post={post_mass:?}"
                );
            }
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
