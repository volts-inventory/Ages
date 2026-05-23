//! Cross-civ knowledge diffusion step: peaceful civ pairs exchange
//! relations through the transmission layer.

use crate::run_tick::RunState;
use protocol::{Event, KnowledgeDiffused};
use sim_civ::{conflict, transmission};
use sim_events::Emitter;

pub(crate) fn knowledge_diffusion_step<E: Emitter>(
    rs: &mut RunState,
    emitter: &mut E,
    tick: u64,
    active_ids: &[u32],
) -> Result<(), E::Error> {
    let mut diffusions: Vec<KnowledgeDiffused> = Vec::new();
    for i in 0..active_ids.len() {
        for j in 0..active_ids.len() {
            if i == j {
                continue;
            }
            let source_civ_id = active_ids[i];
            let receiver_civ_id = active_ids[j];
            let Some(source_slot) = rs.civs.iter().position(|c| c.id == source_civ_id) else {
                continue;
            };
            let Some(receiver_slot) = rs.civs.iter().position(|c| c.id == receiver_civ_id) else {
                continue;
            };
            let src_idx = source_slot;
            let dst_idx = receiver_slot;
            let peaceful = {
                let s = &rs.civs[src_idx];
                let d = &rs.civs[dst_idx];
                conflict::is_peaceful_pair(s, d)
            };
            if !peaceful {
                continue;
            }
            let (lo, hi) = if src_idx < dst_idx {
                (src_idx, dst_idx)
            } else {
                (dst_idx, src_idx)
            };
            let (left, right) = rs.civs.split_at_mut(hi);
            let civ_lo = &mut left[lo];
            let civ_hi = &mut right[0];
            let (source, dest) = if src_idx < dst_idx {
                (&*civ_lo, civ_hi)
            } else {
                (&*civ_hi, civ_lo)
            };
            let dest_id = dest.id;
            let dest_fig = dest.figures.first().map_or(0, |f| f.id);
            let records = transmission::diffuse_between(
                source,
                dest,
                tick,
                rs.species.communication_speed_multiplier(),
            );
            for r in records {
                diffusions.push(KnowledgeDiffused {
                    tick,
                    source_civ_id,
                    dest_civ_id: dest_id,
                    dest_figure_id: dest_fig,
                    relation_id: r.relation.relation_id,
                    source_form: r.relation.form.tag().to_string(),
                    comprehension_q32: r.comprehension.raw().to_bits(),
                });
            }
        }
    }
    for ev in diffusions {
        emitter.emit(&Event::KnowledgeDiffused(ev))?;
        rs.total_knowledge_diffusions = rs.total_knowledge_diffusions.saturating_add(1);
    }
    Ok(())
}
