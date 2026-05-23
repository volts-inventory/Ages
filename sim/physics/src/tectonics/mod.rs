//! Tectonics + fluvial-erosion foundation (Sprint 4 Item 12) plus
//! slab-pull velocity dynamics (Sprint 4 Item 12e).
//!
//! This is the base layer of Sprint 4's rock-cycle stack. It introduces
//! the per-cell plate identity, per-plate kinematics, boundary-driven
//! uplift / depression, and slope × precipitation fluvial erosion that
//! Sprint 4 sub-items (subduction, crust_age, isostasy, volcanism,
//! slab-pull) extend.
//!
//! ## Module layout
//!
//! - `state` — `Tectonics` struct + constructors (`earth_like`,
//!   `sample_plates_for_seed`) and the `subducted_mass` accessor.
//! - `orchestrator` — `Law::integrate` impl that sequences the
//!   per-tick phase order (slab-pull → uplift / divergence →
//!   erosion → age + cooling → subduction → isostasy).
//! - `plates` — `Plate`, `CrustType`, deterministic worldgen sampler.
//! - `subduction` — F6 substrate-aware subduction pass.
//! - `slab_pull` — Item 12e velocity dynamics.
//! - `erosion` — fluvial erosion + uplift/divergence + ridge-cooling
//!   depth (the latter feeds isostasy in the integrator's final
//!   phase).
//! - `tests` — the integration test suite covering convergent /
//!   divergent uplift, erosion, determinism, subduction, ridge age
//!   + cooling, worldgen crust split, and slab-pull dynamics.
//!
//! See `state.rs` and `orchestrator.rs` for the data model and
//! per-tick step documentation respectively; the conceptual notes
//! that used to live in this header (slab-pull, data model,
//! tectonic step, erosion step, worldgen, determinism) now sit
//! next to the code that implements them.

mod erosion;
mod orchestrator;
mod plates;
mod slab_pull;
mod state;
mod subduction;

#[cfg(test)]
mod tests;

pub use erosion::{AGE_TICK_SCALE, OCEAN_DEPTH_K_DEN, OCEAN_DEPTH_K_NUM, RIDGE_DEPTH_PREFACTOR};
pub use plates::{
    CrustType, Plate, CONTINENTAL_THICKNESS_KM, MAX_PLATES, MIN_PLATES, OCEANIC_PERCENT,
    OCEANIC_THICKNESS_KM,
};
pub use slab_pull::{
    max_plate_velocity, slab_pull_density_contrast_oc_cont, slab_pull_density_contrast_oc_oc,
    slab_pull_factor,
};
pub use state::Tectonics;
pub use subduction::{MIN_CRUST_THICKNESS_KM, SUBDUCTION_DT_TICKS};
