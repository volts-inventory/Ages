//! `sim-physics` — deterministic physics engine for Ages.
//!
//! Implements the M1a foundation: hex grid, physics state, the `Law`
//! trait, operator-splitting orchestration, and the first
//! law family (heat diffusion). M1a follow-ups add mechanics + gravity
//! and fluid dynamics. M1b adds chemistry and simplified EM.
//!
//! All real arithmetic flows through `sim_arith::Real` — no direct
//! `f64` use anywhere in this crate (fixed-point determinism contract).

#![allow(clippy::module_name_repetitions)]

pub mod albedo;
pub mod atmospheric_escape;
pub mod chemistry;
pub mod clouds;
pub mod coriolis;
pub mod em;
pub mod fluid;
pub mod grid;
pub mod hadley;
pub mod heat;
pub mod hemisphere;
pub mod hydrology;
pub mod isostasy;
pub mod laws;
pub mod lorentz;
pub mod magnetism;
pub mod mechanics;
pub mod orchestration;
pub mod radiation;
pub mod state;
pub mod tectonics;
pub mod tidal_heating;
pub mod tides;
pub mod vertical;
pub mod volcanism;
pub mod weathering;
pub mod wind;

pub use albedo::{
    albedo_radiation_factor, base_albedo_for, effective_albedo_for, effective_albedo_slice,
    sigmoid_real, IceAlbedo,
};
pub use atmospheric_escape::{
    atmospheric_escape_step, escape_rate_for, jeans_factor, EscapeChannels, PlanetEscapeParams,
    ATMOSPHERIC_SUBSTANCES,
};
pub use chemistry::{Chemistry, Substance};
pub use clouds::{cirrus_greenhouse_k, stratus_greenhouse_k, CloudType, Clouds};
pub use coriolis::Coriolis;
pub use em::Electromagnetism;
pub use fluid::GravityFlow;
pub use grid::{Axial, CellId, HexGrid};
pub use hadley::{
    apply_hadley_circulation, compute_hadley_layout, CellDirection, HadleyCell, HadleyCellLayout,
    HadleyCirculation,
};
pub use hemisphere::{hemisphere_for_row, Hemisphere};
pub use hydrology::Hydrology;
pub use isostasy::{apply_isostasy, continental_factor, oceanic_factor};
pub use laws::Law;
pub use lorentz::Lorentz;
pub use magnetism::{DipoleState, MagneticReversal, Magnetism};
pub use mechanics::Mechanics;
pub use orchestration::{integrate_civ_step, OrchestrationConfig, OrchestratorState};
pub use radiation::{LockingMode, Radiation};
pub use state::{Cell, PhysicsState, N_SUBSTANCES};
pub use tectonics::{CrustType, Plate, Tectonics};
pub use tidal_heating::{
    apply_tidal_heating, default_subsurface_heat_fraction, distribute_heat_to_cells,
    k2_over_q_icy, k2_over_q_rocky, love_number_rocky, moon_tidal_heat_rate,
    moon_tidal_heat_rate_si, q_factor_icy, q_factor_rocky, subsurface_conduction_step,
    subsurface_heat_fraction, MoonHeating, EARTH_RADIUS_M, SECONDS_PER_MACRO,
};
pub use tides::{MoonTide, Tides};
pub use vertical::VerticalConvection;
pub use volcanism::{Volcanism, VolcanismEmission};
pub use weathering::Weathering;
pub use wind::Wind;
