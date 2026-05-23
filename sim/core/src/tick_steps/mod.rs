//! Per-tick phase helpers extracted out of `run_tick.rs`. Each
//! submodule owns one mutually-cohesive step the top-level
//! `run_tick()` orchestrates. The split is mechanical: every
//! helper retains its original signature against `RunState`, no
//! behavioural changes.

mod breakaway;
mod catastrophe;
mod civ_lifecycle;
mod contact_trade;
mod emergent_founding;
mod knowledge_diffusion;
mod stateless_refound;
mod war_alliance;

pub(crate) use breakaway::breakaway_step;
pub(crate) use catastrophe::catastrophe_step;
pub(crate) use civ_lifecycle::civ_lifecycle_step;
pub(crate) use contact_trade::contact_and_trade_step;
pub(crate) use emergent_founding::emergent_founding_step;
pub(crate) use knowledge_diffusion::knowledge_diffusion_step;
pub(crate) use stateless_refound::stateless_refound_step;
pub(crate) use war_alliance::war_and_alliance_step;
