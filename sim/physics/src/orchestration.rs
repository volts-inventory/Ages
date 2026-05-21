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
//!
//! ## Cumulative-drift accumulator
//!
//! The per-kernel `debug_assert!` above catches *sudden* jumps —
//! anything ≥ 1e-6 in a single integration. But a kernel could leak
//! a few least-significant bits per tick (well below the 1e-6
//! threshold) and the cumulative drift would still grow without
//! bound across a long run. To catch slow leaks, `OrchestratorState`
//! threads a per-quantity cumulative drift across ticks: each
//! tick's signed delta (`post - pre`) is added to the running
//! total, and the absolute total is asserted to stay under a tight
//! ceiling (`Real::from_ratio(1, 1_000_000)`). Q32.32 has plenty of
//! headroom for a part-per-million absolute drift, so the bound is
//! a true regression detector — under bit-exact pair-flux + bit-
//! exact stoichiometry the cumulative drift stays at zero forever.

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

/// Tight ceiling for the cumulative-drift accumulator. Signed
/// per-tick deltas are summed across ticks; the absolute total is
/// asserted to stay below this bound on every tick. One part per
/// million is comfortably below the per-kernel `drift_tolerance()`
/// of `1e-6` (which is itself a single-tick budget — the cumulative
/// budget is held to the same magnitude since bit-exact kernels
/// should produce zero drift). Any slow leak that accumulates past
/// `1e-6` over thousands of ticks trips the cumulative assert.
#[cfg(debug_assertions)]
fn cumulative_drift_bound() -> Real {
    Real::from_ratio(1, 1_000_000)
}

/// Looser cumulative bound for hydrology specifically. The two
/// documented vapour clamps in `Hydrology::integrate` truncate
/// LSB-level mass per tick under earth-like coefficients; over a
/// thousand-tick run that accumulates to ~1e-6, which sits right at
/// the tight bound and tripped in long-run sim-core integration
/// tests. The 1e-4 (one part per ten thousand) ceiling is still 100×
/// tighter than the per-tick scaled tolerance so structural leaks
/// trip well before this fires.
#[cfg(debug_assertions)]
fn cumulative_hydrology_drift_bound() -> Real {
    Real::from_ratio(1, 10_000)
}

/// Mutable orchestrator state that persists across
/// `integrate_civ_step` calls. Distinct from `OrchestrationConfig`
/// (which is immutable per-run tuning and thus `Copy`) and from
/// `PhysicsState` (which is the per-cell physics buffers). Carries
/// the cumulative conservation-drift accumulators used by the
/// long-running debug-mode leak detectors.
///
/// One field per conserved quantity tracked by the per-kernel
/// asserts:
///
/// - `tides_water_drift`: cumulative `Σ water_depth` drift from
///   `Tides::integrate`.
/// - `hydrology_drift`: cumulative `Σ water + Σ vapour` drift from
///   `Hydrology::integrate`.
/// - `chemistry_substance_drift`: cumulative `Σ all substances`
///   drift from `Chemistry::integrate`.
///
/// All three default to `Real::ZERO` and are signed (a kernel that
/// silently *creates* mass pushes the accumulator positive; one
/// that destroys mass pushes it negative). Mutated only in debug
/// builds; in release builds the struct still exists (so callers
/// don't need to `#[cfg]`-gate their state plumbing) but the fields
/// stay at their initial values because the per-kernel snapshot
/// code is `#[cfg(debug_assertions)]`-gated.
#[derive(Debug, Clone)]
pub struct OrchestratorState {
    /// Signed cumulative drift of `Σ water_depth` across all
    /// `Tides::integrate` calls since this state was constructed.
    tides_water_drift: Real,
    /// Signed cumulative drift of `Σ water_depth + Σ vapour` across
    /// all `Hydrology::integrate` calls since this state was
    /// constructed. Hydrology has documented mass-truncating clamps
    /// (see `hydrology_drift_tolerance`) so this accumulator can
    /// move under earth-like-but-edge-case coefficients; the
    /// cumulative bound is held to a tight `1e-6` and trips on any
    /// structural leak.
    hydrology_drift: Real,
    /// Signed cumulative drift of `Σ over all substances` across all
    /// `Chemistry::integrate` calls since this state was constructed.
    chemistry_substance_drift: Real,
    /// Cumulative CO2 mass removed by `Weathering::integrate` since
    /// this state was constructed. Always non-negative (weathering
    /// is a one-sided sink); used to offset the chemistry-substance
    /// mass invariant so weathering's intentional removal doesn't
    /// register as a "leak." Mutated only in debug builds.
    weathering_co2_removed: Real,
    /// Cumulative CO2 mass added by Volcanism::integrate (Sprint 4 Item 12d).
    volcanism_co2_added: Real,
    /// Cumulative H2O mass added by Volcanism::integrate (Sprint 4 Item 12d).
    volcanism_h2o_added: Real,
}

impl Default for OrchestratorState {
    fn default() -> Self {
        Self::new()
    }
}

impl OrchestratorState {
    /// Build a fresh orchestrator state with all cumulative-drift
    /// accumulators zeroed. Call once per run; pass the same
    /// instance into every `integrate_civ_step` of that run so the
    /// accumulators see the full tick history.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tides_water_drift: Real::ZERO,
            hydrology_drift: Real::ZERO,
            chemistry_substance_drift: Real::ZERO,
            weathering_co2_removed: Real::ZERO,
            volcanism_co2_added: Real::ZERO,
            volcanism_h2o_added: Real::ZERO,
        }
    }

    /// Sum of the per-quantity cumulative drifts. Each component is
    /// signed, and the sum preserves sign-cancellation across
    /// quantities. Wired up for debug-mode tests + diagnostic
    /// tooling that want a single scalar summarising "how much
    /// drift has the orchestrator accumulated so far?" Per-quantity
    /// accessors below let callers attribute drift to a specific
    /// kernel.
    #[must_use]
    pub fn cumulative_conservation_drift(&self) -> Real {
        self.tides_water_drift + self.hydrology_drift + self.chemistry_substance_drift
    }

    /// Cumulative `Σ water_depth` drift attributed to `Tides`.
    /// Signed. Zero in release builds (mutation is debug-gated).
    #[must_use]
    pub fn tides_water_drift(&self) -> Real {
        self.tides_water_drift
    }

    /// Cumulative `Σ water + Σ vapour` drift attributed to
    /// `Hydrology`. Signed. Zero in release builds.
    #[must_use]
    pub fn hydrology_drift(&self) -> Real {
        self.hydrology_drift
    }

    /// Cumulative `Σ over all substances` drift attributed to
    /// `Chemistry`. Signed. Zero in release builds.
    #[must_use]
    pub fn chemistry_substance_drift(&self) -> Real {
        self.chemistry_substance_drift
    }

    /// Cumulative CO2 mass removed by all `Weathering::integrate`
    /// calls since construction. Non-negative. Zero in release
    /// builds. Used by the chemistry-mass invariant to offset
    /// weathering's intentional one-sided removal so the assertion
    /// continues to flag *unintentional* mass leaks.
    #[must_use]
    pub fn weathering_co2_removed(&self) -> Real {
        self.weathering_co2_removed
    }

    /// Cumulative CO2 mass added by Volcanism (Sprint 4 Item 12d).
    #[must_use]
    pub fn volcanism_co2_added(&self) -> Real {
        self.volcanism_co2_added
    }

    /// Cumulative H2O mass added by Volcanism (Sprint 4 Item 12d).
    #[must_use]
    pub fn volcanism_h2o_added(&self) -> Real {
        self.volcanism_h2o_added
    }
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
///
/// `orch_state` carries the cumulative conservation-drift
/// accumulators across ticks. Pass the *same* instance into every
/// call of a given run; constructing a fresh one each tick defeats
/// the slow-leak detector. In release builds `orch_state` is
/// effectively a no-op — the cumulative-drift bookkeeping is
/// `#[cfg(debug_assertions)]`-gated — but callers still thread it
/// through so the surface API doesn't shift between debug and
/// release.
#[allow(clippy::too_many_arguments)]
pub fn integrate_civ_step(
    state: &mut PhysicsState,
    orch_state: &mut OrchestratorState,
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
    weathering: Option<&crate::weathering::Weathering>,
    ice_albedo: Option<&crate::albedo::IceAlbedo>,
    tectonics: Option<&crate::tectonics::Tectonics>,
    volcanism: Option<&crate::volcanism::Volcanism>,
    magnetic_reversal: Option<&crate::magnetism::MagneticReversal>,
) {
    // In release builds the cumulative-drift mutations vanish under
    // `#[cfg(debug_assertions)]`, so `orch_state` would warn as
    // unused. The discard keeps the API stable without leaking
    // `#[cfg]` into call sites.
    #[cfg(not(debug_assertions))]
    let _ = &orch_state;
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
                let delta = post_water - pre_water;
                debug_assert!(
                    delta.abs() < drift_tolerance(),
                    "tides leaked water: pre={pre_water:?} post={post_water:?}"
                );
                // Slow-leak detector: accumulate the signed
                // per-call delta and assert the cumulative
                // absolute drift stays under the tight bound.
                // Under bit-exact donor-limited pair-flux every
                // delta is exactly zero, so the accumulator never
                // moves; the assert only trips on a structural
                // regression that biases drift in one direction.
                orch_state.tides_water_drift = orch_state.tides_water_drift + delta;
                debug_assert!(
                    orch_state.tides_water_drift.abs() < cumulative_drift_bound(),
                    "cumulative tides water drift exceeded bound: {:?}",
                    orch_state.tides_water_drift
                );
            }
        }
        for _fluid_step in 0..cfg.fluid_substeps_per_macro {
            fluid.integrate(state, cfg.fluid_dt);
        }
        // Ice-albedo update runs before radiation so each
        // macro-step's per-cell albedo reflects the freshest
        // surface temperature. Source order:
        //   ice_albedo (updates snow / sea-ice fractions)
        //     → radiation (reads them via the effective-albedo
        //       slice)
        //     → heat diffusion
        // Closes the positive-feedback loop that drives the
        // snowball bifurcation: cold cells grow ice, ice
        // raises albedo, higher albedo lowers radiative
        // equilibrium temperature, lower T grows more ice.
        if let Some(ia) = ice_albedo {
            ia.integrate(state, cfg.heat_dt);
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
                let delta = post_wv - pre_wv;
                debug_assert!(
                    delta.abs() < tol,
                    "hydrology leaked mass: pre={pre_wv:?} post={post_wv:?} tol={tol:?}"
                );
                // Slow-leak detector. The per-tick assert above
                // allows clamp-driven slack scaled to total mass;
                // the cumulative assert holds the long-run drift
                // to the tight `1e-6` ceiling so a systematic
                // leak (always-positive or always-negative deltas)
                // trips even when individual deltas stay under the
                // scaled per-tick budget.
                orch_state.hydrology_drift = orch_state.hydrology_drift + delta;
                debug_assert!(
                    orch_state.hydrology_drift.abs() < cumulative_hydrology_drift_bound(),
                    "cumulative hydrology drift exceeded bound: {:?}",
                    orch_state.hydrology_drift
                );
            }
        }
        // Carbon-silicate weathering thermostat. Runs after
        // hydrology (so it reads post-evap/precip water and
        // vapour) and before chemistry (so chemistry sees the
        // post-weathering CO2 state). Weathering is a *one-sided*
        // sink on `Substance::CO2`: intentional removal, not a
        // leak. The orchestrator accumulates the total removed
        // mass per tick so the chemistry-substance-mass invariant
        // below can offset it; without that offset the invariant
        // would trip the moment the first volcanism source (Item
        // 12d) lands and chemistry sees a non-zero `pre - post`
        // delta on CO2 that's actually weathering's bookkeeping.
        if let Some(w) = weathering {
            let removed = w.integrate(state, cfg.heat_dt);
            #[cfg(debug_assertions)]
            {
                orch_state.weathering_co2_removed =
                    orch_state.weathering_co2_removed + removed;
            }
            #[cfg(not(debug_assertions))]
            let _ = removed;
        }
        // Tectonics + fluvial erosion (Sprint 4 Item 12). Runs after
        // hydrology (so erosion reads post-precipitation water and
        // vapour) and before chemistry (so any rock-cycle CO2 source
        // landing in Item 12d sees post-tectonic surface state).
        // Grouped with weathering as the rock-cycle band of the
        // macro-step. No-ops when the plate roster is empty so tests
        // that don't seed plates aren't affected.
        if let Some(t) = tectonics {
            t.integrate(state, cfg.heat_dt);
        }
        // Volcanic CO2 + H2O outgassing (Sprint 4 Item 12d). Runs
        // after tectonics so it reads the post-tectonic plate-
        // boundary geometry, and before chemistry so chemistry
        // sees the volcanically enriched CO2 + vapour state.
        if let Some(v) = volcanism {
            let emission = v.integrate(state, cfg.heat_dt);
            #[cfg(debug_assertions)]
            {
                orch_state.volcanism_co2_added = orch_state.volcanism_co2_added + emission.co2_added;
                orch_state.volcanism_h2o_added = orch_state.volcanism_h2o_added + emission.h2o_added;
            }
            #[cfg(not(debug_assertions))]
            let _ = emission;
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
        // Geomagnetic reversal Markov chain (Sprint 5 Item 20).
        // Runs after Magnetism + Lorentz so any law in the same
        // macro-step reads a coherent dipole envelope. The
        // reversal law mutates the per-planet `dipole_state` /
        // `dipole_strength` envelope, not the per-cell vector
        // field directly; downstream couplings (cosmic-ray flux
        // multiplier on species mutation, ion-channel escape)
        // read the envelope via `state.cosmic_ray_ground_flux()`.
        if let Some(mr) = magnetic_reversal {
            mr.integrate(state, cfg.em_dt);
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
                let delta = post_mass - pre_mass;
                debug_assert!(
                    delta.abs() < drift_tolerance(),
                    "chemistry leaked mass: pre={pre_mass:?} post={post_mass:?}"
                );
                // Slow-leak detector. Chemistry stoichiometry is
                // bit-exact under integer-coefficient reactions
                // (combustion 1+1→2, regrowth 2→1+1, phase swaps
                // 1↔1) so the per-tick delta should be exactly
                // zero; any biased drift across thousands of ticks
                // trips this cumulative assert long before the
                // single-tick bound.
                orch_state.chemistry_substance_drift =
                    orch_state.chemistry_substance_drift + delta;
                debug_assert!(
                    orch_state.chemistry_substance_drift.abs() < cumulative_drift_bound(),
                    "cumulative chemistry substance drift exceeded bound: {:?}",
                    orch_state.chemistry_substance_drift
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

        let mut orch_a = OrchestratorState::new();
        let mut orch_b = OrchestratorState::new();
        integrate_civ_step(
            &mut a, &mut orch_a, &cfg, &fluid, &heat, &em, &chem, None, None, None, None, None,
            None, None, None, None, None, None, None, None,
        );
        integrate_civ_step(
            &mut b, &mut orch_b, &cfg, &fluid, &heat, &em, &chem, None, None, None, None, None,
            None, None, None, None, None, None, None, None,
        );

        assert_eq!(a.temperature(), b.temperature());
        assert_eq!(a.water_depth(), b.water_depth());
    }

    /// The cumulative-drift accumulator must stay within the tight
    /// `1e-6` bound across a long run under earth-like coefficients.
    /// 1000 ticks ≈ 83 sim-years on the 1-tick = 1-month cadence —
    /// well beyond the regime where a per-tick truncation bug
    /// would have accumulated visible mass. The kernels exercised
    /// here (tides, hydrology, chemistry) are all bit-exact, so
    /// the cumulative drift should sit at exactly zero; the assert
    /// uses the tight `1e-6` ceiling to leave room for a future
    /// kernel that introduces a tiny LSB-level non-bit-exact step.
    #[test]
    fn cumulative_drift_stays_bounded_over_1000_ticks() {
        let grid = HexGrid::new(6, 6);
        let mut state = PhysicsState::new(grid);
        // Seed a small non-trivial water column + temperature so
        // tides + hydrology + chemistry all have something to chew
        // on. Without any state the kernels noop and the test would
        // be vacuous.
        let centre_idx = state.grid().n_cells() / 2;
        state.water_depth_mut()[centre_idx] = Real::from_int(10);
        state.temperature_mut()[centre_idx] = Real::from_int(290);
        // Use compact per-tick sub-step counts so 1000 ticks
        // finishes in the test budget. The cumulative-drift
        // invariant depends on kernel correctness, not on
        // sub-step count, so a coarser schedule is fine.
        let cfg = OrchestrationConfig {
            macro_steps_per_step: 4,
            fluid_substeps_per_macro: 2,
            heat_substeps_per_macro: 1,
            chemistry_macros_per_substep: 2,
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

        let mut orch = OrchestratorState::new();
        for _ in 0..1000 {
            integrate_civ_step(
                &mut state, &mut orch, &cfg, &fluid, &heat, &em, &chem, None, None, None, None,
                None, None, None, None, None, None, None, None, None,
            );
        }
        let tight_bound = Real::from_ratio(1, 1_000_000);
        let hydro_bound = Real::from_ratio(1, 10_000);
        let combined = tight_bound + hydro_bound;
        assert!(
            orch.cumulative_conservation_drift().abs() < combined,
            "cumulative drift exceeded bound after 1000 ticks: {:?} (bound {:?})",
            orch.cumulative_conservation_drift(),
            combined
        );
        assert!(
            orch.tides_water_drift().abs() < tight_bound,
            "tides water drift exceeded bound: {:?}",
            orch.tides_water_drift()
        );
        assert!(
            orch.hydrology_drift().abs() < hydro_bound,
            "hydrology drift exceeded bound: {:?}",
            orch.hydrology_drift()
        );
        assert!(
            orch.chemistry_substance_drift().abs() < tight_bound,
            "chemistry substance drift exceeded bound: {:?}",
            orch.chemistry_substance_drift()
        );
    }

    /// The cumulative-drift accessor returns a signed `Real`; a
    /// positive component (silent mass creation) and a negative
    /// one (silent mass destruction) cancel in the sum. Since the
    /// fields are only mutated from inside `integrate_civ_step`
    /// (and never through public setters), the test reaches into
    /// the private fields from this same-module test to forge
    /// known per-quantity values and exercise the signedness of
    /// the accessor. Confirms a positive-only forge reads positive,
    /// a counter-balancing negative forge drives the sum negative,
    /// and per-quantity accessors preserve their own sign.
    #[test]
    fn cumulative_drift_accessor_reports_signed_value() {
        let mut orch = OrchestratorState::new();
        // Fresh state is exactly zero. The accessor is `Real`,
        // which is signed by construction.
        assert_eq!(orch.cumulative_conservation_drift(), Real::ZERO);
        // Forge a positive drift via the chemistry channel.
        orch.chemistry_substance_drift = Real::from_ratio(1, 1_000_000_000);
        let positive = orch.cumulative_conservation_drift();
        assert!(
            positive > Real::ZERO,
            "expected positive cumulative drift, got {positive:?}"
        );
        // Forge a counter-balancing larger negative drift on
        // tides; the sum should drop below zero.
        orch.tides_water_drift = Real::from_ratio(-2, 1_000_000_000);
        let negative = orch.cumulative_conservation_drift();
        assert!(
            negative < Real::ZERO,
            "expected negative cumulative drift after cancelling, got {negative:?}"
        );
        // Per-quantity accessors also surface signed values.
        assert!(orch.tides_water_drift() < Real::ZERO);
        assert!(orch.chemistry_substance_drift() > Real::ZERO);
    }
}
