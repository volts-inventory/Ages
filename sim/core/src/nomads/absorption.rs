//! Nomad → civ cohort transfer. Runs at civ founding (with the
//! founder-effect loss fraction applied) and after each
//! `claim_cells` call during territory expansion (with zero loss
//! — those nomads join an existing institutional scaffold and
//! pay no reorganisation tax).

use sim_arith::Real;
use std::collections::BTreeMap;

/// Founder-effect loss fraction applied when nomadic populations
/// are folded into a fresh civ. Models the institutional-reorg
/// cost of band → polity: the social structure is being built
/// from scratch, food distribution + birth-cycle alignment + new
/// authority rules cost lives during the first decade. The
/// territory-expansion path (existing civ gaining cells) passes
/// zero loss — those nomads are joining an existing institutional
/// scaffold, no reorganisation tax.
pub(crate) const FOUNDING_ABSORB_LOSS: (i64, i64) = (15, 100);

/// territory expansion converts nomads into civ population. Returns
/// the absorbed total post-loss (tests use this; callers may ignore).
///
/// `loss_fraction` is applied bracket-uniformly before the absorbed
/// pop deposits: a `0.15` value drops 15% across every age bracket
/// equally. Pass `Real::ZERO` for paths that should preserve full
/// pop (existing civ gaining territory).
pub(crate) fn absorb_into_civ(
    pops: &mut BTreeMap<u32, Real>,
    civ: &mut sim_civ::Civ,
    cells: impl IntoIterator<Item = u32>,
    biology: &sim_species::PopulationBiology,
    loss_fraction: Real,
) -> Real {
    let retained = Real::ONE - loss_fraction.clamp01();
    let mut total = Real::ZERO;
    // Deposit the absorbed nomadic pop into the per-cell
    // `region_cohorts` for the gained cell, distributed across
    // the four age brackets per the species's biology fractions.
    // Nomadic groups are mixed-age — they don't all magically
    // become fertile adults on civ contact.
    for cell in cells {
        if let Some(p) = pops.remove(&cell) {
            let after_loss = p * retained;
            total = total + after_loss;
            let p_pop = sim_arith::Pop::from_real(after_loss);
            if let Some(cohort) = civ.region_cohorts.get_mut(&cell) {
                cohort.deposit_distributed(p_pop, biology);
            } else {
                // Cell isn't (yet) in `region_cohorts` — shouldn't
                // happen since `absorb_into_civ` is called on
                // gained cells right after `claim_cells`/
                // `expand_via_overflow` seed them, but be safe.
                let mut c = sim_civ::Cohort::empty_with_civ(civ.id);
                c.deposit_distributed(p_pop, biology);
                civ.region_cohorts.insert(cell, c);
            }
        }
    }
    if total > Real::ZERO {
        civ.cohort
            .deposit_distributed(sim_arith::Pop::from_real(total), biology);
    }
    total
}
