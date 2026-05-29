//! Stateless re-founding step. Runs when no civs are active to
//! seed a fresh civ from the stateless cohort, optionally
//! inheriting from a recently-collapsed parent.

use crate::events::{claimed_cells_for_event, figure_born_event};
use crate::run_tick::RunState;
use crate::setup::emit_species_drift_if_meaningful;
use crate::territory::{
    compute_territory, pick_distant_habitable_cell, pick_habitable_cell, target_cell_count,
};
use crate::RunConfig;
use protocol::{CivFounded, Event, KnowledgeTransmitted};
use sim_arith::Real;
use sim_civ::{transmission, Civ};
use sim_events::Emitter;
use std::collections::BTreeSet;

#[allow(clippy::too_many_lines)]
pub(crate) fn stateless_refound_step<E: Emitter>(
    rs: &mut RunState,
    cfg: &RunConfig,
    emitter: &mut E,
    tick: u64,
) -> Result<(), E::Error> {
    let parent_collapse = rs.last_collapse_tick;
    let elapsed_collapse = parent_collapse.map(|t| tick.saturating_sub(t));
    let metabolism = rs.planet.metabolic_substrate.metabolism();
    let dark_age_min =
        sim_civ::streak_ticks_for_metabolism(sim_civ::FOUNDING_MIN_DARK_AGE_TICKS, metabolism);
    let remnant_window =
        sim_civ::streak_ticks_for_metabolism(sim_civ::RECENT_REMNANT_WINDOW_TICKS, metabolism);
    let v1_eligible =
        elapsed_collapse.is_some_and(|e| (dark_age_min..=remnant_window).contains(&e));
    let probe_civ = Civ::with_species(
        rs.next_civ_id,
        tick,
        sim_arith::Pop::from_int(50),
        rs.species.cognition,
        rs.species.seed,
        &rs.species_modality_kinds,
        rs.species.initial_cosmology,
    );
    let charismatic_present = probe_civ
        .figures
        .iter()
        .any(|f| f.charisma >= Real::from_ratio(8, 10));
    let recent_catastrophe = rs
        .civs
        .iter()
        .filter_map(|c| c.last_catastrophe_tick)
        .max()
        .is_some_and(|t| tick.saturating_sub(t) <= 100);
    let v2_eligible = recent_catastrophe && charismatic_present;
    if !(v1_eligible || v2_eligible) {
        return Ok(());
    }
    let stateless_total = rs
        .civs
        .iter()
        .filter(|c| c.cohort.civ_membership.is_none())
        .map(|c| c.cohort.total())
        .fold(sim_arith::Pop::ZERO, |a, b| a + b);
    let founding_floor =
        sim_civ::founding_min_population(rs.planet.biosphere, rs.species.cognition);
    if stateless_total < founding_floor {
        return Ok(());
    }
    let parent_id = rs
        .civs
        .iter()
        .rev()
        .find(|c| c.collapsed_tick.is_some())
        .map_or(0, |c| c.id);
    let parent_centroid = rs
        .civs
        .iter()
        .find(|c| c.id == parent_id)
        .map(|c| c.territory_centroid);
    for c in &mut rs.civs {
        if c.cohort.civ_membership.is_none() {
            c.cohort.scale_in_place(Real::ZERO);
        }
    }
    let stateless_cohort = sim_civ::Cohort::with_civ(stateless_total, rs.next_civ_id);
    let mut new_civ = Civ::refound_from_stateless(
        rs.next_civ_id,
        tick,
        stateless_cohort,
        rs.species.cognition,
        rs.species.seed,
        &rs.species_modality_kinds,
        parent_id,
    );
    new_civ.name = sim_civ::civ_name_from_seed(cfg.seed, rs.next_civ_id);
    if let Some(parent_civ) = rs.civs.iter().find(|c| c.id == parent_id) {
        new_civ.inherit_species_drift_with_environment(
            parent_civ,
            rs.planet.seed,
            rs.planet.metabolic_substrate.metabolism(),
            rs.planet.biosphere,
        );
        new_civ.inherit_lineage_from(parent_civ);
    }
    new_civ.dynamics = sim_civ::dynamics_for_civ(&new_civ, &rs.species, &rs.planet);
    new_civ.configure_substrate_with_topology(
        rs.species.habitat,
        new_civ.effective_cognition(&rs.species),
        new_civ.effective_sociality(&rs.species),
        rs.planet.metabolic_substrate.metabolism(),
        rs.state.grid().width().saturating_mul(rs.state.grid().height()),
        rs.planet.radius,
        rs.species.cognition_topology,
    );
    new_civ.configure_lifecycle_state(&rs.species.lifecycle);
    let occupied: BTreeSet<u32> = rs
        .civs
        .iter()
        .flat_map(|c| c.claimed_cells.iter().copied())
        .collect();
    new_civ.territory_centroid = if occupied.is_empty() {
        pick_habitable_cell(
            new_civ.territory_centroid,
            rs.state.grid(),
            &rs.state,
            &rs.planet,
            &new_civ,
            rs.species.habitat,
        )
    } else {
        pick_distant_habitable_cell(
            new_civ.territory_centroid,
            rs.state.grid(),
            &rs.state,
            &rs.planet,
            &new_civ,
            rs.species.habitat,
            &occupied,
        )
    };
    let target = target_cell_count(&new_civ, rs.state.grid().n_cells());
    let cells = compute_territory(
        new_civ.territory_centroid,
        target,
        rs.state.grid(),
        &rs.state,
        &rs.planet,
        &new_civ,
        rs.species.habitat,
        &BTreeSet::new(),
    );
    let _ = parent_centroid;
    new_civ.claim_cells(&cells);
    new_civ.refresh_available_forms_with_modalities(
        &rs.species_baseline,
        &rs.recognition,
        &rs.species_modality_kinds,
    );
    let initial_pop_q32 = new_civ.cohort.total().raw().to_bits();
    let band = u32::try_from(new_civ.figures.len()).unwrap_or(0);
    for f in &new_civ.figures {
        emitter.emit(&Event::FigureBorn(figure_born_event(new_civ.id, f)))?;
    }
    let new_cells = claimed_cells_for_event(&new_civ);
    let new_caps: Vec<i128> = new_cells
        .iter()
        .map(|&c| {
            new_civ
                .cell_capacity(&rs.state, c, tick, &rs.planet)
                .raw()
                .to_bits()
        })
        .collect();
    emitter.emit(&Event::CivFounded(CivFounded {
        tick,
        civ_id: new_civ.id,
        parent_civ_id: Some(parent_id),
        name: new_civ.name.clone(),
        initial_population_q32: initial_pop_q32,
        founding_figure_count: band,
        claimed_cells: new_cells,
        cell_capacities_q32: new_caps,
    }))?;
    new_civ.last_territory_emit_tick = tick;
    emit_species_drift_if_meaningful(emitter, &new_civ)?;

    let decay_ticks = rs.species.transmission_decay_ticks();
    let comm_speed = rs.species.communication_speed_multiplier();
    let (transmissions, mythologizations) =
        if let Some(parent_civ) = rs.civs.iter().find(|c| c.id == parent_id) {
            transmission::transmit_from_parent(
                &mut new_civ,
                parent_civ,
                tick,
                decay_ticks,
                comm_speed,
            )
        } else {
            (Vec::new(), Vec::new())
        };
    let dest_civ_id = new_civ.id;
    let dest_figure_id = new_civ.figures.first().map_or(0, |f| f.id);
    for record in &transmissions {
        emitter.emit(&Event::KnowledgeTransmitted(KnowledgeTransmitted {
            tick,
            source_civ_id: parent_id,
            dest_civ_id,
            dest_figure_id,
            relation_id: record.relation.relation_id,
            source_form: record.relation.form.tag().to_string(),
            comprehension_q32: record.comprehension.raw().to_bits(),
        }))?;
        rs.total_knowledge_transmissions = rs.total_knowledge_transmissions.saturating_add(1);
    }
    for myth in &mythologizations {
        emitter.emit(&Event::RelationMythologized(
            protocol::RelationMythologized {
                tick,
                source_civ_id: parent_id,
                dest_civ_id,
                relation_id: myth.relation_id,
                axis: myth.axis,
                magnitude_q32: myth.magnitude.raw().to_bits(),
                comprehension_q32: myth.comprehension.raw().to_bits(),
            },
        ))?;
    }

    rs.civs.push(new_civ);
    rs.next_civ_id += 1;
    Ok(())
}
