//! Core run orchestration: seeded RNG, tick loop, phase walking.
//!
//! The whole sim threads a single `Rng` through every decision path.
//! The per-tick phase order is enforced here.
//!
//! `RunConfig` + `Rng` + `rng_from_seed` live in `mod config`.
//! The run-start setup helpers (`emit_nomads_changed`,
//! `emit_species_drift_if_meaningful`, `build_planet_context`,
//! `setup_run`, `lifecycle_for_role`) live in `mod setup`. The
//! per-tick body lives in `mod run_tick`. Per-run constants
//! (`PHASE_ORDER`, `STAGNATION_THRESHOLD_TICKS`,
//! `TRANSCENDENCE_SUSTAINED_TICKS`, `SNAPSHOT_INTERVAL_TICKS`) live
//! in `mod constants`.

use protocol::Event;
use sim_events::Emitter;

mod config;
mod constants;
mod contact;
mod events;
mod laws;
mod nomads;
mod phases;
mod run_tick;
mod setup;
mod territory;
mod tick_steps;

pub use config::{rng_from_seed, Rng, RunConfig};
pub use constants::{
    PHASE_ORDER, SNAPSHOT_INTERVAL_TICKS, STAGNATION_THRESHOLD_TICKS,
    TRANSCENDENCE_SUSTAINED_TICKS,
};
pub use setup::lifecycle_for_role;

/// The civ-sim tick loop. Walks the per-tick phase order each
/// tick. Physics laws are built from the sampled Planet so each seed
/// produces a different science.
///
/// The body is two stages:
///   1. `setup::setup_run` — sample planet, build laws/recognition/
///      species/ecosystem, emit run-start events, return `RunState`.
///   2. `run_tick::run_tick` per tick — advance one tick. Returns
///      `Ok(false)` to break the loop early (species extinction /
///      stagnation / transcendence run-end).
///
/// Followed by the `RunEnd` emit with the resolved end reason.
pub fn run<E: Emitter>(cfg: &RunConfig, emitter: &mut E) -> Result<(), E::Error> {
    let _rng = rng_from_seed(cfg.seed);

    let mut rs = setup::setup_run(cfg, emitter)?;

    for tick in 0..cfg.max_ticks {
        let cont = run_tick::run_tick(&mut rs, cfg, emitter, tick)?;
        if !cont {
            break;
        }
    }

    let (end_tick, end_reason) = rs.early_run_end.unwrap_or((cfg.max_ticks, "fixed_horizon"));
    emitter.emit(&Event::RunEnd {
        tick: end_tick,
        reason: end_reason.to_string(),
    })?;

    Ok(())
}

#[cfg(test)]
mod tests;
