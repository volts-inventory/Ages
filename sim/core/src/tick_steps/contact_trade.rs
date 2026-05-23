//! Contact and trade step: emits `CivContact` for newly co-existing
//! reachable pairs, opens / closes trade routes, and runs the
//! per-tick trade flow over open routes.

use crate::contact;
use crate::run_tick::RunState;
use protocol::{CivContact, Event};
use sim_civ::conflict;
use sim_events::Emitter;
use std::collections::BTreeSet;

pub(crate) fn contact_and_trade_step<E: Emitter>(
    rs: &mut RunState,
    emitter: &mut E,
    tick: u64,
    active_ids: &[u32],
) -> Result<(), E::Error> {
    if tick.is_multiple_of(contact::CONTACT_CHECK_TICKS) {
        for i in 0..active_ids.len() {
            for j in (i + 1)..active_ids.len() {
                let pair = if active_ids[i] < active_ids[j] {
                    (active_ids[i], active_ids[j])
                } else {
                    (active_ids[j], active_ids[i])
                };
                if rs.emitted_contacts.contains(&pair) {
                    continue;
                }
                let civ_a = rs.civs.iter().find(|c| c.id == pair.0).expect("active civ");
                let civ_b = rs.civs.iter().find(|c| c.id == pair.1).expect("active civ");
                if !contact::civs_in_contact(civ_a, civ_b, rs.species.habitat, &rs.state) {
                    continue;
                }
                rs.emitted_contacts.insert(pair);
                if let Some(a_mut) = rs.civs.iter_mut().find(|c| c.id == pair.0) {
                    a_mut.contact_history.insert(pair.1);
                }
                if let Some(b_mut) = rs.civs.iter_mut().find(|c| c.id == pair.1) {
                    b_mut.contact_history.insert(pair.0);
                }
                let civ_a = rs.civs.iter().find(|c| c.id == pair.0).expect("active civ");
                let civ_b = rs.civs.iter().find(|c| c.id == pair.1).expect("active civ");
                emitter.emit(&Event::CivContact(CivContact {
                    tick,
                    civ_a: pair.0,
                    civ_b: pair.1,
                }))?;
                if conflict::is_peaceful_pair(civ_a, civ_b)
                    && !rs.trade_routes.contains_key(&pair)
                {
                    rs.trade_routes.insert(pair, tick);
                    emitter.emit(&Event::TradeRouteEstablished(
                        protocol::TradeRouteEstablished {
                            tick,
                            civ_a: pair.0,
                            civ_b: pair.1,
                        },
                    ))?;
                }
            }
        }
    }

    // M8: per-tick trade flow over open routes.
    let active_set_now: BTreeSet<u32> = rs
        .civs
        .iter()
        .filter(|c| c.is_active())
        .map(|c| c.id)
        .collect();
    let stale_routes: Vec<(u32, u32)> = rs
        .trade_routes
        .keys()
        .copied()
        .filter(|(a, b)| !active_set_now.contains(a) || !active_set_now.contains(b))
        .collect();
    for pair in stale_routes {
        rs.trade_routes.remove(&pair);
        emitter.emit(&Event::TradeRouteClosed(protocol::TradeRouteClosed {
            tick,
            civ_a: pair.0,
            civ_b: pair.1,
            reason: "civ_collapsed".to_string(),
        }))?;
    }
    let route_pairs: Vec<(u32, u32)> = rs.trade_routes.keys().copied().collect();
    for (a_id, b_id) in route_pairs {
        let pos_a = rs.civs.iter().position(|c| c.id == a_id);
        let pos_b = rs.civs.iter().position(|c| c.id == b_id);
        if let (Some(pa), Some(pb)) = (pos_a, pos_b) {
            let (lo, hi) = if pa < pb { (pa, pb) } else { (pb, pa) };
            let (left, right) = rs.civs.split_at_mut(hi);
            let civ_lo = &mut left[lo];
            let civ_hi = &mut right[0];
            sim_civ::trade_flow_between(civ_lo, civ_hi);
        }
    }
    Ok(())
}
