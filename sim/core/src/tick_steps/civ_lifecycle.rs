//! Per-tick civ-lifecycle step: collapse checks, cohesion /
//! life-expectancy / surplus deltas, and the emitter calls for
//! each.

use crate::run_tick::RunState;
use protocol::{CivCollapsed, Event};
use sim_arith::Real;
use sim_civ::cosmology;
use sim_events::Emitter;
use std::collections::BTreeMap;

pub(crate) fn civ_lifecycle_step<E: Emitter>(
    rs: &mut RunState,
    emitter: &mut E,
    tick: u64,
) -> Result<(), E::Error> {
    let mut collapse_events: Vec<CivCollapsed> = Vec::new();
    let mut cohesion_events: Vec<protocol::CohesionShifted> = Vec::new();
    let mut life_expectancy_events: Vec<protocol::CivLifeExpectancyChanged> = Vec::new();
    let mut surplus_events: Vec<protocol::CivSurplusChanged> = Vec::new();
    let mut at_war_counts: BTreeMap<u32, u64> = BTreeMap::new();
    for (a, b) in rs.war_state.keys() {
        *at_war_counts.entry(*a).or_insert(0) += 1;
        *at_war_counts.entry(*b).or_insert(0) += 1;
    }
    for civ in rs.civs.iter_mut().filter(|c| c.is_active()) {
        if let Some(reason) = civ.check_collapse_with_terrain(tick, &rs.state, &rs.planet) {
            let final_pop_q32 = civ.cohort.total().raw().to_bits();
            let final_figs = u32::try_from(
                civ.figures
                    .iter()
                    .filter(|f| f.retired_tick.is_none())
                    .count(),
            )
            .unwrap_or(u32::MAX);
            let civ_id = civ.id;
            civ.collapse(tick);
            civ.apply_cosmology_push(&cosmology::push_for_civ_collapsed(), Real::ONE);
            civ.apply_religion_push(&sim_civ::push_for_civ_collapsed(), Real::ONE);
            collapse_events.push(CivCollapsed {
                tick,
                civ_id,
                reason: reason.tag().to_string(),
                final_population_q32: final_pop_q32,
                final_figure_count: final_figs,
            });
        }
        let threshold = sim_arith::Real::from_ratio(
            sim_civ::COHESION_EMIT_THRESHOLD.0,
            sim_civ::COHESION_EMIT_THRESHOLD.1,
        );
        if (civ.cohesion - civ.last_emitted_cohesion).abs() >= threshold {
            cohesion_events.push(protocol::CohesionShifted {
                tick,
                civ_id: civ.id,
                cohesion_q32: civ.cohesion.raw().to_bits(),
                previous_q32: civ.last_emitted_cohesion.raw().to_bits(),
            });
            civ.last_emitted_cohesion = civ.cohesion;
        }
        let life_threshold_months = sim_arith::Real::from_int(24);
        let current_life = civ.life_expectancy_months();
        let last_life = civ.last_emitted_life_expectancy_months;
        let never_emitted = last_life <= sim_arith::Real::ZERO;
        let drifted = (current_life - last_life).abs() >= life_threshold_months;
        if never_emitted || drifted {
            life_expectancy_events.push(protocol::CivLifeExpectancyChanged {
                tick,
                civ_id: civ.id,
                life_expectancy_months_q32: current_life.raw().to_bits(),
            });
            civ.last_emitted_life_expectancy_months = current_life;
        }
        let pop = civ.aggregate_population();
        let cap = civ.carrying_capacity_with_terrain(&rs.state, &rs.planet);
        let utilisation = if cap.raw().to_num::<i64>() > 0 {
            let pop_r = pop.to_real_nonneg();
            let cap_r = cap.to_real_nonneg().max(Real::ONE);
            (pop_r / cap_r).clamp01()
        } else {
            sim_arith::Real::ZERO
        };
        let at_war_count = at_war_counts.get(&civ.id).copied().unwrap_or(0);
        sim_civ::step_surplus(civ, utilisation, at_war_count);
        let emit_floor = sim_arith::Real::from_int(sim_civ::SURPLUS_EMIT_DELTA_FLOOR);
        let delta = civ.surplus - civ.last_emitted_surplus;
        if delta.abs() >= emit_floor {
            surplus_events.push(protocol::CivSurplusChanged {
                tick,
                civ_id: civ.id,
                surplus_q32: civ.surplus.raw().to_bits(),
                previous_q32: civ.last_emitted_surplus.raw().to_bits(),
            });
            civ.last_emitted_surplus = civ.surplus;
        }
    }
    if !collapse_events.is_empty() {
        rs.last_collapse_tick = Some(tick);
        for ev in collapse_events {
            emitter.emit(&Event::CivCollapsed(ev))?;
        }
    }
    for ev in cohesion_events {
        emitter.emit(&Event::CohesionShifted(ev))?;
    }
    for ev in life_expectancy_events {
        emitter.emit(&Event::CivLifeExpectancyChanged(ev))?;
    }
    for ev in surplus_events {
        emitter.emit(&Event::CivSurplusChanged(ev))?;
    }
    Ok(())
}
