//! Run-setup helpers used by `run()` once per run start (or at
//! per-tick boundaries that don't fit cleanly inside the main tick
//! loop): emitting the run-start `SpeciesNomadsChanged`, the
//! per-civ `SpeciesDrift` snapshot, and computing the per-run
//! `PlanetContext` that calibrates recognition relative-to-this-
//! planet.

use crate::laws::ignition_threshold_for;
use protocol::{Event, SpeciesDrift, SpeciesNomadsChanged};
use sim_civ::Civ;
use sim_events::Emitter;
use std::collections::BTreeMap;

pub(crate) fn emit_nomads_changed<E: Emitter>(
    emitter: &mut E,
    tick: u64,
    pops: &BTreeMap<u32, sim_arith::Real>,
) -> Result<(), E::Error> {
    let mut cells: Vec<u32> = pops.keys().copied().collect();
    cells.sort_unstable();
    let pop_q32: Vec<i64> = cells
        .iter()
        .map(|c| {
            pops.get(c)
                .copied()
                .unwrap_or(sim_arith::Real::ZERO)
                .raw()
                .to_bits()
        })
        .collect();
    emitter.emit(&Event::SpeciesNomadsChanged(SpeciesNomadsChanged {
        tick,
        cells,
        population_q32: pop_q32,
    }))
}

/// Emit a `SpeciesDrift` event when a civ's inherited drift
/// crosses the half-step threshold on at least one channel. No-op
/// for inaugural civs (zero drift) and breakaway civs that happen
/// to roll a near-zero step. Called from the three founding sites
/// in sim/core right after the corresponding `CivFounded` emit so
/// the drift snapshot lands in tick order with its civ.
pub(crate) fn emit_species_drift_if_meaningful<E: Emitter>(
    emitter: &mut E,
    civ: &Civ,
) -> Result<(), E::Error> {
    if !civ.has_meaningful_drift() {
        return Ok(());
    }
    emitter.emit(&Event::SpeciesDrift(SpeciesDrift {
        tick: civ.founded_tick,
        civ_id: civ.id,
        parent_civ_id: civ.parent_civ_id,
        cognition_delta_q32: civ.cognition_delta.raw().to_bits(),
        sociality_delta_q32: civ.sociality_delta.raw().to_bits(),
        lifespan_delta_years_q32: civ.lifespan_delta_years.raw().to_bits(),
        communication_fidelity_delta_q32: civ.communication_fidelity_delta.raw().to_bits(),
    }))
}

/// Build the per-run `PlanetContext` consumed by recognition
/// scans. Carries the planet-derived calibration so climate-relative
/// signatures and `AboveIgnition` fire relative to *this planet*.
///
/// The *mean* and *gradient* come from the actual post-init cell-
/// temperature distribution rather than `planet.mean_temperature`
/// directly: per-cell imprinting (e.g. `GaseousShell` pinning
/// cells to a 700 K deep-atmosphere column independently of the
/// planet's stated 232 K cloud-top metadata) means the planet's
/// stated mean isn't always representative of the cells civs
/// actually observe. Reading the post-init state guarantees the
/// climate bands match what's on the grid.
pub(crate) fn build_planet_context(
    planet: &sim_world::Planet,
    state: &sim_physics::PhysicsState,
) -> sim_recognition::PlanetContext {
    let temps = state.temperature();
    let n = temps.len().max(1);
    let mut sum = sim_arith::Real::ZERO;
    let mut tmin = temps[0];
    let mut tmax = temps[0];
    for t in temps {
        sum = sum + *t;
        if *t < tmin {
            tmin = *t;
        }
        if *t > tmax {
            tmax = *t;
        }
    }
    let mean = sum / sim_arith::Real::from_int(i64::try_from(n).unwrap_or(1));
    // Gradient defined as max - min across the planet — the actual
    // span the cells span, regardless of how the planet metadata
    // labelled the equator-pole spread.
    let gradient = (tmax - tmin).max(sim_arith::Real::ZERO);
    sim_recognition::PlanetContext {
        mean_temperature: mean,
        temperature_gradient: gradient,
        ignition_threshold: ignition_threshold_for(planet.atmosphere),
        orbital_period_months: planet.orbital_period_months,
        is_tidally_locked: planet.is_tidally_locked(),
    }
}
