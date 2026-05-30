//! Emergent founding step: when nomadic clusters saturate and a
//! suitable cell is available, found a new civ around that cell.

use crate::events::{claimed_cells_for_event, figure_born_event};
use crate::nomads;
use crate::run_tick::RunState;
use crate::setup::emit_species_drift_if_meaningful;
use crate::territory::{compute_territory, target_cell_count};
use crate::RunConfig;
use protocol::{CivFounded, Event};
use sim_civ::Civ;
use sim_events::Emitter;
use std::collections::BTreeSet;

#[allow(clippy::too_many_lines)]
pub(crate) fn emergent_founding_step<E: Emitter>(
    rs: &mut RunState,
    cfg: &RunConfig,
    emitter: &mut E,
    tick: u64,
    claim_union: &BTreeSet<u32>,
) -> Result<(), E::Error> {
    let centroids: Vec<u32> = rs
        .civs
        .iter()
        .filter(|c| c.is_active())
        .map(|c| c.territory_centroid)
        .collect();
    let producer_biomass = rs.ecosystem.tier_biomass(0);
    let Some(emerge_cell) = nomads::scan_for_emergence(
        &rs.nomad_pops,
        &rs.nomad_observations,
        &rs.nomad_pressure_streak,
        rs.species.cognition,
        rs.species.sociality,
        &rs.state,
        &rs.planet,
        rs.species.habitat,
        tick,
        producer_biomass,
        &centroids,
        claim_union,
    ) else {
        return Ok(());
    };
    rs.last_emergent_tick = tick;
    let mut new_civ = Civ::with_species(
        rs.next_civ_id,
        tick,
        sim_arith::Pop::from_int(200),
        rs.species.cognition,
        rs.species.seed,
        &rs.species_modality_kinds,
        rs.species.initial_cosmology,
    );
    new_civ.name = sim_civ::civ_name_from_seed(cfg.seed, rs.next_civ_id);
    if let Some(parent_civ) = rs
        .civs
        .iter()
        .filter(|c| c.collapsed_tick.is_some())
        .max_by_key(|c| c.collapsed_tick.unwrap_or(0))
    {
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
    new_civ.territory_centroid = emerge_cell;
    let target = target_cell_count(&new_civ, rs.state.grid().n_cells());
    let cells = compute_territory(
        new_civ.territory_centroid,
        target,
        rs.state.grid(),
        &rs.state,
        &rs.planet,
        &new_civ,
        rs.species.habitat,
        claim_union,
    );
    let gained: Vec<u32> = cells.iter().copied().collect();
    new_civ.claim_cells(&cells);
    let founder_loss =
        sim_arith::Real::from_ratio(nomads::FOUNDING_ABSORB_LOSS.0, nomads::FOUNDING_ABSORB_LOSS.1);
    nomads::absorb_into_civ(
        &mut rs.nomad_pops,
        &mut new_civ,
        gained.clone(),
        &rs.species.biology,
        founder_loss,
    );
    let _drained = nomads::drain_observations_for_cells(&mut rs.nomad_observations, gained);
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
    let claimed = claimed_cells_for_event(&new_civ);
    let claimed_caps: Vec<i128> = claimed
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
        parent_civ_id: None,
        name: new_civ.name.clone(),
        initial_population_q32: initial_pop_q32,
        founding_figure_count: band,
        claimed_cells: claimed,
        cell_capacities_q32: claimed_caps,
    }))?;
    new_civ.last_territory_emit_tick = tick;
    emit_species_drift_if_meaningful(emitter, &new_civ)?;
    rs.civs.push(new_civ);
    rs.next_civ_id += 1;
    Ok(())
}
