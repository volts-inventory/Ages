//! Per-tick catastrophe check on every active civ.

use crate::run_tick::RunState;
use protocol::{CatastropheFired, Event};
use sim_civ::catastrophe;
use sim_events::Emitter;

pub(crate) fn catastrophe_step<E: Emitter>(
    rs: &mut RunState,
    emitter: &mut E,
    tick: u64,
) -> Result<(), E::Error> {
    let mut cat_events: Vec<(u32, catastrophe::CatastropheRecord)> = Vec::new();
    for civ in rs.civs.iter_mut().filter(|c| c.is_active()) {
        let civ_id = civ.id;
        if let Some(rec) = catastrophe::check_and_apply(
            civ,
            &mut rs.state,
            &rs.planet,
            &rs.species,
            tick,
            Some(&mut rs.ecosystem),
        ) {
            civ.record_catastrophe_selection_bias(rec.kind, rec.fraction_lost);
            sim_civ::drain_surplus_on_catastrophe(civ);
            cat_events.push((civ_id, rec));
        }
    }
    for (civ_id, rec) in cat_events {
        emitter.emit(&Event::CatastropheFired(CatastropheFired {
            tick,
            civ_id,
            catastrophe_kind: rec.kind.tag().to_string(),
            fraction_lost_q32: rec.fraction_lost.raw().to_bits(),
        }))?;
        rs.total_catastrophes = rs.total_catastrophes.saturating_add(1);
    }
    Ok(())
}
