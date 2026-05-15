//! Cell-targeting helpers shared across catastrophe kinds:
//! deterministic per-tick pick, hex neighbour lookup, and the
//! pop-loss applicator that propagates a fraction across a cell
//! and its claimed neighbours.

use crate::Civ;
use sim_arith::{Pop, Real};
use std::collections::BTreeSet;

/// pick the densest claimed cell for a civ — the cohort
/// with highest count. Returns `None` if the civ has no
/// region cohorts.
pub fn densest_claimed_cell(civ: &Civ) -> Option<u32> {
    civ.region_cohorts
        .iter()
        .max_by(|a, b| {
            a.1.total()
                .raw()
                .to_bits()
                .cmp(&b.1.total().raw().to_bits())
        })
        .map(|(cell, _)| *cell)
}

/// deterministic per-civ-tick cell pick from the claimed
/// set. Hashes `(tick, civ_id)` into an index; same seed = same
/// asteroid impact site. Returns `None` if the civ has no claim.
pub fn deterministic_cell_pick(civ: &Civ, tick: u64) -> Option<u32> {
    if civ.claimed_cells.is_empty() {
        return None;
    }
    let cells: Vec<u32> = civ.claimed_cells.iter().copied().collect();
    let mix = tick
        .wrapping_mul(2_654_435_761)
        .wrapping_add(u64::from(civ.id).wrapping_mul(40_503));
    let len = u64::try_from(cells.len()).unwrap_or(1);
    let idx = usize::try_from(mix % len).unwrap_or(0);
    Some(cells[idx])
}

const HEX_OFFSETS: [(i32, i32); 6] = [(1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)];

/// six axial neighbours of `cell` on a torus-wrapping hex
/// grid of width × height. Mirrors `sim_core::compute_territory`
/// neighbour offsets so the catastrophe geometry agrees with
/// territory expansion.
pub fn hex_neighbors(cell: u32, width: u32, height: u32) -> Vec<u32> {
    if width == 0 || height == 0 {
        return Vec::new();
    }
    let w = i32::try_from(width).unwrap_or(i32::MAX);
    let h = i32::try_from(height).unwrap_or(i32::MAX);
    let q = i32::try_from(cell % width).unwrap_or(0);
    let r = i32::try_from(cell / width).unwrap_or(0);
    let mut out = Vec::with_capacity(6);
    for (dq, dr) in HEX_OFFSETS {
        let nq = (q + dq).rem_euclid(w);
        let nr = (r + dr).rem_euclid(h);
        let nc = u32::try_from(nr).unwrap_or(0) * width + u32::try_from(nq).unwrap_or(0);
        out.push(nc);
    }
    out
}

/// drop a fraction of population at `center` and a (smaller)
/// fraction at each of its hex neighbours. If `claimed_only` is
/// true, neighbour drops only apply to cells in the civ's
/// `claimed_cells`. Returns total population lost across all
/// affected cells (for event payload).
pub fn apply_to_cell_and_neighbors(
    civ: &mut Civ,
    grid_width: u32,
    grid_height: u32,
    center: u32,
    center_frac: Real,
    neighbor_frac: Real,
    claimed_only: bool,
) -> Pop {
    let mut total_lost = Pop::ZERO;
    total_lost = total_lost + civ.drop_cell_pop(center, center_frac);
    let neighbours = hex_neighbors(center, grid_width, grid_height);
    let claimed: Option<BTreeSet<u32>> = if claimed_only {
        Some(civ.claimed_cells.iter().copied().collect())
    } else {
        None
    };
    for n in neighbours {
        if let Some(c) = &claimed {
            if !c.contains(&n) {
                continue;
            }
        }
        total_lost = total_lost + civ.drop_cell_pop(n, neighbor_frac);
    }
    total_lost
}
