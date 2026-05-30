//! M5 breakaway step. Splits a child civ off an existing parent
//! when cohesion or dogmatism triggers fire and a charismatic
//! figure is present.

use crate::contact;
use crate::events::{claimed_cells_for_event, figure_born_event};
use crate::run_tick::RunState;
use crate::setup::emit_species_drift_if_meaningful;
use crate::territory::{compute_territory, target_cell_count};
use crate::RunConfig;
use protocol::{CivContact, CivFounded, CivTerritoryChanged, Event, KnowledgeTransmitted};
use sim_arith::Real;
use sim_civ::{transmission, Civ};
use sim_events::Emitter;
use std::collections::BTreeSet;

#[allow(clippy::too_many_lines)]
pub(crate) fn breakaway_step<E: Emitter>(
    rs: &mut RunState,
    cfg: &RunConfig,
    emitter: &mut E,
    tick: u64,
) -> Result<(), E::Error> {
    let breakaway_cooldown_ok = rs
        .last_breakaway_tick
        .is_none_or(|t| tick.saturating_sub(t) >= 500);
    let cohesion_breakaway_streak_metabolised = sim_civ::streak_ticks_for_metabolism(
        sim_civ::COHESION_BREAKAWAY_STREAK_TICKS,
        rs.planet.metabolic_substrate.metabolism(),
    );
    let cohesion_parent_id: Option<u32> = if breakaway_cooldown_ok {
        rs.civs
            .iter()
            .find(|c| {
                c.is_active()
                    && c.cohesion_breakaway_streak >= cohesion_breakaway_streak_metabolised
            })
            .map(|c| c.id)
    } else {
        None
    };
    let dogmatic_parent_id: Option<u32> =
        if breakaway_cooldown_ok && cohesion_parent_id.is_none() {
            rs.civs
                .iter()
                .find(|c| c.is_active() && c.cosmology.dogmatism() > Real::from_ratio(7, 10))
                .map(|c| c.id)
        } else {
            None
        };
    let breakaway_pick: Option<(u32, (i64, i64), bool)> = cohesion_parent_id
        .map(|id| (id, sim_civ::COHESION_BREAKAWAY_SHARE, true))
        .or_else(|| dogmatic_parent_id.map(|id| (id, (1, 2), false)));
    let Some((parent_id, (share_num, share_den), is_cohesion_breakaway)) = breakaway_pick else {
        return Ok(());
    };
    let probe = Civ::with_species(
        rs.next_civ_id,
        tick,
        sim_arith::Pop::from_int(50),
        rs.species.cognition,
        rs.species.seed,
        &rs.species_modality_kinds,
        rs.species.initial_cosmology,
    );
    let charismatic = probe
        .figures
        .iter()
        .any(|f| f.charisma >= Real::from_ratio(7, 10));
    if !charismatic {
        return Ok(());
    }
    let parent_idx = rs
        .civs
        .iter()
        .position(|c| c.id == parent_id)
        .expect("dogmatic parent in civs");
    let parent_aggregate = rs.civs[parent_idx].cohort.total();
    let share_ratio = Real::from_ratio(share_num, share_den);
    let breakaway_share = parent_aggregate * share_ratio;
    let breakaway_floor =
        sim_civ::founding_min_population(rs.planet.biosphere, rs.species.cognition)
            / Real::from_int(2);
    if breakaway_share < breakaway_floor {
        return Ok(());
    }
    let parent_centroid = rs.civs[parent_idx].territory_centroid;
    let mut border_candidates: Vec<u32> = Vec::new();
    for &cell in &rs.civs[parent_idx].claimed_cells {
        let axial = rs.state.grid().axial_of(sim_physics::CellId(cell));
        let has_non_parent_nbr = rs
            .state
            .grid()
            .neighbours(axial)
            .iter()
            .any(|nbr| !rs.civs[parent_idx].claimed_cells.contains(&nbr.0));
        if has_non_parent_nbr {
            border_candidates.push(cell);
        }
    }
    border_candidates.sort_unstable();
    let live_centroids: BTreeSet<u32> = rs
        .civs
        .iter()
        .filter(|c| c.collapsed_tick.is_none())
        .map(|c| c.territory_centroid)
        .collect();
    let pick = border_candidates
        .iter()
        .find(|c| !live_centroids.contains(c))
        .or_else(|| border_candidates.first())
        .copied();
    let Some(centroid) = pick else {
        return Ok(());
    };
    let mut seized_cohort = sim_civ::Cohort::empty_with_civ(rs.next_civ_id);
    rs.civs[parent_idx].claimed_cells.remove(&centroid);
    if let Some(c) = rs.civs[parent_idx].region_cohorts.remove(&centroid) {
        seized_cohort.merge_in(&c);
    }
    rs.civs[parent_idx].resync_aggregate_from_regions();
    let seized_cells: Vec<u32> = vec![centroid];
    let parent_aggregate_post = rs.civs[parent_idx].cohort.total();
    let breakaway_share = parent_aggregate_post * share_ratio;
    let keep_share = Real::ONE - share_ratio;
    rs.civs[parent_idx].cohort.scale_in_place(keep_share);
    for c in rs.civs[parent_idx].region_cohorts.values_mut() {
        c.scale_in_place(keep_share);
    }
    let _ = parent_centroid;
    if is_cohesion_breakaway {
        let recovery = Real::from_ratio(
            sim_civ::COHESION_PARENT_RECOVERY.0,
            sim_civ::COHESION_PARENT_RECOVERY.1,
        );
        rs.civs[parent_idx].cohesion = (rs.civs[parent_idx].cohesion + recovery).clamp01();
        rs.civs[parent_idx].cohesion_breakaway_streak = 0;
    }
    if !seized_cells.is_empty() {
        let parent_civ_ref = &rs.civs[parent_idx];
        let parent_claimed_sorted = claimed_cells_for_event(parent_civ_ref);
        let parent_cell_pops: Vec<i128> = parent_claimed_sorted
            .iter()
            .map(|c| {
                parent_civ_ref
                    .region_cohorts
                    .get(c)
                    .map_or(0i128, |cohort| cohort.total().raw().to_bits())
            })
            .collect();
        let parent_cell_caps: Vec<i128> = parent_claimed_sorted
            .iter()
            .map(|&c| {
                parent_civ_ref
                    .cell_capacity(&rs.state, c, tick, &rs.planet)
                    .raw()
                    .to_bits()
            })
            .collect();
        emitter.emit(&Event::CivTerritoryChanged(CivTerritoryChanged {
            tick,
            civ_id: parent_id,
            claimed_cells: parent_claimed_sorted,
            population_q32: parent_civ_ref.cohort.total().raw().to_bits(),
            cell_populations_q32: parent_cell_pops,
            cell_capacities_q32: parent_cell_caps,
        }))?;
        rs.civs[parent_idx].last_territory_emit_tick = tick;
    }
    let mut breakaway_cohort = sim_civ::Cohort::with_civ(breakaway_share, rs.next_civ_id);
    breakaway_cohort.merge_in(&seized_cohort);
    breakaway_cohort.civ_membership = Some(rs.next_civ_id);
    let mut new_civ = Civ::refound_from_stateless(
        rs.next_civ_id,
        tick,
        breakaway_cohort,
        rs.species.cognition,
        rs.species.seed,
        &rs.species_modality_kinds,
        parent_id,
    );
    new_civ.name = sim_civ::civ_name_from_seed(cfg.seed, rs.next_civ_id);
    {
        let parent_civ = &rs.civs[parent_idx];
        new_civ.inherit_species_drift_with_environment(
            parent_civ,
            rs.planet.seed,
            rs.planet.metabolic_substrate.metabolism(),
            rs.planet.biosphere,
        );
        new_civ.inherit_lineage_from(parent_civ);
    }
    if is_cohesion_breakaway {
        new_civ.cohesion = Real::from_ratio(
            sim_civ::COHESION_BREAKAWAY_INITIAL.0,
            sim_civ::COHESION_BREAKAWAY_INITIAL.1,
        );
        new_civ.last_emitted_cohesion = new_civ.cohesion;
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
    // Same species↔planet survivability that gates the parent civ's
    // capacity — a breakaway inherits the same biochemistry on the
    // same world.
    new_civ.apply_planet_survivability(sim_species::planet_survivability(
        &rs.species.tolerance,
        &rs.planet,
    ));
    new_civ.configure_lifecycle_state(&rs.species.lifecycle);
    new_civ.territory_centroid = centroid;
    let target = target_cell_count(&new_civ, rs.state.grid().n_cells());
    let occupied_post: BTreeSet<u32> = rs
        .civs
        .iter()
        .flat_map(|c| c.claimed_cells.iter().copied())
        .collect();
    let cells = compute_territory(
        new_civ.territory_centroid,
        target,
        rs.state.grid(),
        &rs.state,
        &rs.planet,
        &new_civ,
        rs.species.habitat,
        &occupied_post,
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

    let (transmissions, mythologizations) = {
        let parent_civ = &rs.civs[parent_idx];
        let decay_ticks = rs.species.transmission_decay_ticks();
        let comm_speed = rs.species.communication_speed_multiplier();
        transmission::transmit_from_parent(&mut new_civ, parent_civ, tick, decay_ticks, comm_speed)
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

    let new_id = new_civ.id;
    rs.civs.push(new_civ);
    rs.next_civ_id += 1;
    rs.last_breakaway_tick = Some(tick);
    let pair = if parent_id < new_id {
        (parent_id, new_id)
    } else {
        (new_id, parent_id)
    };
    if !rs.emitted_contacts.contains(&pair) {
        let parent_civ = rs
            .civs
            .iter()
            .find(|c| c.id == parent_id)
            .expect("parent civ");
        let child_civ = rs
            .civs
            .iter()
            .find(|c| c.id == new_id)
            .expect("just-pushed breakaway civ");
        if contact::civs_in_contact(parent_civ, child_civ, rs.species.habitat, &rs.state) {
            rs.emitted_contacts.insert(pair);
            if let Some(parent_mut) = rs.civs.iter_mut().find(|c| c.id == parent_id) {
                parent_mut.contact_history.insert(new_id);
            }
            if let Some(child_mut) = rs.civs.iter_mut().find(|c| c.id == new_id) {
                child_mut.contact_history.insert(parent_id);
            }
            emitter.emit(&Event::CivContact(CivContact {
                tick,
                civ_a: pair.0,
                civ_b: pair.1,
            }))?;
        }
    }
    Ok(())
}
