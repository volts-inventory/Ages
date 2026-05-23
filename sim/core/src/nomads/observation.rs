//! Per-cell, per-template observation accumulation for the
//! nomadic species pool.
//!
//! Each firing of a recognition template at a nomadic cell
//! increments `observations[cell][template_id]` by 1. Cells where
//! the species has no nomadic presence (or that are already inside
//! a civ's claim) accumulate nothing — civs handle their own
//! observation tally separately.
//!
//! On civ emergence the new civ inherits the per-template counts
//! from the cells it claims (via
//! [`super::emergence::drain_observations_for_cells`]), so a
//! coastal civ knows water / flood / fertile-land templates while
//! an inland-volcanic civ knows fire / thermal / magnetic-field
//! templates. Tool-unlock thresholds read these counts, so the
//! founding region literally shapes which technologies the civ can
//! build first.

use sim_arith::Real;
use std::collections::BTreeMap;

/// Per-cell, per-template observation count for nomads.
/// Replaces the earlier opaque scalar tech accumulator. Each firing
/// of `template_id` at a nomadic cell increments
/// `observations[cell][template_id]` by 1. Cells where the
/// species has no nomadic presence accumulate nothing.
///
/// On emergence, the new civ inherits the per-template counts
/// from the cells it claims, so a coastal civ knows water,
/// flood, fertile-land templates, while an inland-volcanic civ
/// knows fire, thermal, magnetic-field templates. Tool-
/// unlock thresholds read these counts, so the founding region
/// literally shapes which technologies the civ can build first.
pub(crate) fn accumulate_observation(
    observations: &mut BTreeMap<u32, BTreeMap<u32, u64>>,
    pops: &BTreeMap<u32, Real>,
    civ_claims: &std::collections::BTreeSet<u32>,
    cell: u32,
    template_id: u32,
) {
    if civ_claims.contains(&cell) {
        return;
    }
    if !pops.contains_key(&cell) {
        return;
    }
    let cell_obs = observations.entry(cell).or_default();
    *cell_obs.entry(template_id).or_insert(0) += 1;
}
