//! Q-war conflict resolution + alliance formation / dissolution
//! pass over every active-civ pair.

use crate::run_tick::RunState;
use protocol::{
    AllianceDissolveReason, AllianceDissolved, AllianceFormed, ConflictResolved, Event,
    PeaceConcluded, PeaceReason, WarDeclared,
};
use sim_arith::Real;
use sim_civ::conflict;
use sim_events::Emitter;
use std::collections::BTreeSet;

#[allow(clippy::too_many_lines)]
pub(crate) fn war_and_alliance_step<E: Emitter>(
    rs: &mut RunState,
    emitter: &mut E,
    tick: u64,
    active_ids: &[u32],
) -> Result<(), E::Error> {
    let mut peace_events: Vec<PeaceConcluded> = Vec::new();
    let active_set: BTreeSet<u32> = active_ids.iter().copied().collect();
    let stale_pairs: Vec<(u32, u32)> = rs
        .war_state
        .keys()
        .copied()
        .filter(|(a, b)| !active_set.contains(a) || !active_set.contains(b))
        .collect();
    for pair in stale_pairs {
        let started = rs.war_state.remove(&pair).unwrap_or(tick);
        peace_events.push(PeaceConcluded {
            tick,
            civ_a: pair.0,
            civ_b: pair.1,
            reason: PeaceReason::TerritoryResolved,
            duration_ticks: tick.saturating_sub(started),
        });
    }

    let mut conflict_events: Vec<ConflictResolved> = Vec::new();
    let mut war_events: Vec<WarDeclared> = Vec::new();
    let mut trade_close_events: Vec<protocol::TradeRouteClosed> = Vec::new();
    for i in 0..active_ids.len() {
        for j in (i + 1)..active_ids.len() {
            let civ_id_first = active_ids[i];
            let civ_id_second = active_ids[j];
            let Some(slot_first) = rs.civs.iter().position(|c| c.id == civ_id_first) else {
                continue;
            };
            let Some(slot_second) = rs.civs.iter().position(|c| c.id == civ_id_second) else {
                continue;
            };
            let idx_a = slot_first;
            let idx_b = slot_second;
            let (lo, hi) = if idx_a < idx_b {
                (idx_a, idx_b)
            } else {
                (idx_b, idx_a)
            };
            let pair = if civ_id_first < civ_id_second {
                (civ_id_first, civ_id_second)
            } else {
                (civ_id_second, civ_id_first)
            };
            let already_at_war = rs.war_state.contains_key(&pair);

            let (left, right) = rs.civs.split_at_mut(hi);
            let civ_lo = &mut left[lo];
            let civ_hi = &mut right[0];

            if !rs.emitted_contacts.contains(&pair) {
                continue;
            }

            let overlap_empty = conflict::overlap(civ_lo, civ_hi).is_empty();
            if overlap_empty {
                if already_at_war {
                    let started = rs.war_state.remove(&pair).unwrap_or(tick);
                    peace_events.push(PeaceConcluded {
                        tick,
                        civ_a: pair.0,
                        civ_b: pair.1,
                        reason: PeaceReason::TerritoryResolved,
                        duration_ticks: tick.saturating_sub(started),
                    });
                }
                continue;
            }

            let Some(assessment) =
                conflict::assess_pair(civ_lo, civ_hi, &rs.state, &rs.planet, tick)
            else {
                continue;
            };
            let decision = conflict::decide_war(already_at_war, assessment.belligerence);

            match decision {
                conflict::WarDecision::StayPeaceful => continue,
                conflict::WarDecision::ConcludePeace => {
                    let started = rs.war_state.remove(&pair).unwrap_or(tick);
                    peace_events.push(PeaceConcluded {
                        tick,
                        civ_a: pair.0,
                        civ_b: pair.1,
                        reason: PeaceReason::BelligerenceDropped,
                        duration_ticks: tick.saturating_sub(started),
                    });
                    continue;
                }
                conflict::WarDecision::DeclareWar => {
                    rs.war_state.insert(pair, tick);
                    war_events.push(WarDeclared {
                        tick,
                        aggressor_civ_id: assessment.aggressor_id,
                        defender_civ_id: assessment.defender_id,
                        belligerence_q32: assessment.belligerence.raw().to_bits(),
                        drive_q32: assessment.drive.raw().to_bits(),
                        kinship_q32: assessment.kinship.raw().to_bits(),
                    });
                    if rs.trade_routes.remove(&pair).is_some() {
                        trade_close_events.push(protocol::TradeRouteClosed {
                            tick,
                            civ_a: pair.0,
                            civ_b: pair.1,
                            reason: "war_declared".to_string(),
                        });
                    }
                }
                conflict::WarDecision::ContinueWar => {}
            }

            let outcome = if idx_a < idx_b {
                conflict::resolve(civ_lo, civ_hi, tick)
            } else {
                conflict::resolve(civ_hi, civ_lo, tick)
            };
            if let Some(o) = outcome {
                let defeated = o.loser_defeated;
                conflict_events.push(ConflictResolved {
                    tick,
                    winner_civ_id: o.winner_id,
                    loser_civ_id: o.loser_id,
                    disputed_cell_count: u32::try_from(o.disputed_cells.len())
                        .unwrap_or(u32::MAX),
                    loss_fraction_q32: o.loss_fraction.raw().to_bits(),
                    loser_defeated: defeated,
                });
                if defeated {
                    let started = rs.war_state.remove(&pair).unwrap_or(tick);
                    peace_events.push(PeaceConcluded {
                        tick,
                        civ_a: pair.0,
                        civ_b: pair.1,
                        reason: PeaceReason::Defeated,
                        duration_ticks: tick.saturating_sub(started),
                    });
                }
            }
        }
    }
    for ev in war_events {
        emitter.emit(&Event::WarDeclared(ev))?;
    }
    for ev in trade_close_events {
        emitter.emit(&Event::TradeRouteClosed(ev))?;
    }
    for ev in conflict_events {
        emitter.emit(&Event::ConflictResolved(ev))?;
    }
    for ev in peace_events {
        emitter.emit(&Event::PeaceConcluded(ev))?;
    }

    // PR4: Alliance formation + dissolution pass.
    let mut alliance_formed_events: Vec<AllianceFormed> = Vec::new();
    let mut alliance_dissolved_events: Vec<AllianceDissolved> = Vec::new();
    let at_war_pairs: BTreeSet<(u32, u32)> = rs.war_state.keys().copied().collect();

    let mut dissolutions: Vec<((u32, u32), AllianceDissolveReason)> = Vec::new();
    let mut formations: Vec<(u32, u32)> = Vec::new();
    let mut trust_updates: Vec<((u32, u32), Real)> = Vec::new();

    for i in 0..active_ids.len() {
        for j in (i + 1)..active_ids.len() {
            let pair = if active_ids[i] < active_ids[j] {
                (active_ids[i], active_ids[j])
            } else {
                (active_ids[j], active_ids[i])
            };
            let Some(civ_a_idx) = rs.civs.iter().position(|c| c.id == pair.0) else {
                continue;
            };
            let Some(civ_b_idx) = rs.civs.iter().position(|c| c.id == pair.1) else {
                continue;
            };
            let civ_a = &rs.civs[civ_a_idx];
            let civ_b = &rs.civs[civ_b_idx];
            let mutually_allied = civ_a.allied_with.contains(&civ_b.id)
                && civ_b.allied_with.contains(&civ_a.id);

            if mutually_allied {
                if conflict::alliance_drifted_apart(civ_a, civ_b) {
                    dissolutions.push((pair, AllianceDissolveReason::CosmologyDrift));
                    continue;
                }
                let a_wars: BTreeSet<u32> = at_war_pairs
                    .iter()
                    .filter_map(|(lo, hi)| {
                        if *lo == civ_a.id {
                            Some(*hi)
                        } else if *hi == civ_a.id {
                            Some(*lo)
                        } else {
                            None
                        }
                    })
                    .filter(|other| *other != civ_b.id)
                    .collect();
                let b_wars: BTreeSet<u32> = at_war_pairs
                    .iter()
                    .filter_map(|(lo, hi)| {
                        if *lo == civ_b.id {
                            Some(*hi)
                        } else if *hi == civ_b.id {
                            Some(*lo)
                        } else {
                            None
                        }
                    })
                    .filter(|other| *other != civ_a.id)
                    .collect();
                let misaligned = a_wars.iter().any(|t| !b_wars.contains(t))
                    || b_wars.iter().any(|t| !a_wars.contains(t));
                if misaligned {
                    dissolutions.push((pair, AllianceDissolveReason::WarMisalignment));
                    continue;
                }
                let cosmo_gap = conflict::cosmology_distance(civ_a, civ_b);
                let religion_gap = conflict::religion_distance(civ_a, civ_b);
                let trust_a = civ_a
                    .alliance_trust
                    .get(&civ_b.id)
                    .copied()
                    .unwrap_or(Real::from(conflict::ALLIANCE_TRUST_INITIAL));
                let trust_b = civ_b
                    .alliance_trust
                    .get(&civ_a.id)
                    .copied()
                    .unwrap_or(Real::from(conflict::ALLIANCE_TRUST_INITIAL));
                let prior = trust_a.min(trust_b);
                let post = conflict::step_alliance_trust(prior, cosmo_gap, religion_gap);
                trust_updates.push((pair, post));
                if post < Real::from(conflict::ALLIANCE_TRUST_FLOOR) {
                    dissolutions.push((pair, AllianceDissolveReason::TrustEroded));
                }
            } else {
                let at_war_now = at_war_pairs.contains(&pair);
                if conflict::propose_alliance(civ_a, civ_b, at_war_now, tick) {
                    formations.push(pair);
                }
            }
        }
    }

    let trust_initial = Real::from(conflict::ALLIANCE_TRUST_INITIAL);
    for (pair, post) in trust_updates {
        if let Some(civ) = rs.civs.iter_mut().find(|c| c.id == pair.0) {
            civ.alliance_trust.insert(pair.1, post);
        }
        if let Some(civ) = rs.civs.iter_mut().find(|c| c.id == pair.1) {
            civ.alliance_trust.insert(pair.0, post);
        }
    }
    for pair in formations {
        if let Some(civ) = rs.civs.iter_mut().find(|c| c.id == pair.0) {
            civ.allied_with.insert(pair.1);
            civ.alliance_trust.insert(pair.1, trust_initial);
        }
        if let Some(civ) = rs.civs.iter_mut().find(|c| c.id == pair.1) {
            civ.allied_with.insert(pair.0);
            civ.alliance_trust.insert(pair.0, trust_initial);
        }
        alliance_formed_events.push(AllianceFormed {
            tick,
            civ_a: pair.0,
            civ_b: pair.1,
        });
    }
    for (pair, reason) in dissolutions {
        if let Some(civ) = rs.civs.iter_mut().find(|c| c.id == pair.0) {
            civ.allied_with.remove(&pair.1);
            civ.alliance_trust.remove(&pair.1);
            civ.alliance_cooldown.insert(pair.1, tick);
        }
        if let Some(civ) = rs.civs.iter_mut().find(|c| c.id == pair.1) {
            civ.allied_with.remove(&pair.0);
            civ.alliance_trust.remove(&pair.0);
            civ.alliance_cooldown.insert(pair.0, tick);
        }
        alliance_dissolved_events.push(AllianceDissolved {
            tick,
            civ_a: pair.0,
            civ_b: pair.1,
            reason,
        });
    }
    for ev in alliance_formed_events {
        emitter.emit(&Event::AllianceFormed(ev))?;
    }
    for ev in alliance_dissolved_events {
        emitter.emit(&Event::AllianceDissolved(ev))?;
    }
    Ok(())
}
