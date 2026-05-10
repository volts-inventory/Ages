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

pub mod chemistry;
pub mod coriolis;
pub mod em;
pub mod fluid;
pub mod grid;
pub mod heat;
pub mod hydrology;
pub mod laws;
pub mod lorentz;
pub mod magnetism;
pub mod mechanics;
pub mod orchestration;
pub mod radiation;
pub mod state;
pub mod tides;
pub mod vertical;
pub mod wind;

pub use chemistry::{Chemistry, Substance};
pub use coriolis::Coriolis;
pub use em::Electromagnetism;
pub use fluid::GravityFlow;
pub use grid::{Axial, CellId, HexGrid};
pub use hydrology::Hydrology;
pub use laws::Law;
pub use lorentz::Lorentz;
pub use magnetism::Magnetism;
pub use mechanics::Mechanics;
pub use orchestration::{integrate_civ_step, OrchestrationConfig};
pub use radiation::Radiation;
pub use state::{Cell, PhysicsState, N_SUBSTANCES};
pub use tides::{MoonTide, Tides};
pub use vertical::VerticalConvection;
pub use wind::Wind;
