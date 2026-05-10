//! successor centroid placement.
//!
//! When a civ collapses, sim/core's founding pipeline can spawn a
//! successor from the stateless cohort. Pre- the successor's
//! centroid was deterministically `figures[0].cell_assignment` —
//! often the same cell as the parent's centroid because the band-id
//! sequence repeated. [`pick_successor_centroid`] forces the new
//! capital onto a parent-adjacent (or any other) claimed cell so
//! the two civs read as visually distinct on the viewport map and
//! narratively distinct in the digest.

use std::collections::BTreeSet;

/// pick a centroid for a successor civ that's distinct from
/// its predecessor's centroid. Pre- a successor's centroid was
/// `figures[0].cell_assignment`, which is deterministic from the
/// founding band — for civ 2 spawning out of civ 1's collapse the
/// band-id sequence often produced cell 0 for both, leaving both
/// civs with the same numeric centroid even though fixed the
/// display tie.
///
/// Selection rules, in order:
/// 1. **Adjacent-to-parent**: prefer a `claimed` cell that is one
///    of the parent centroid's six hex neighbours (cardinal hex
///    adjacency on the torus). The new capital then sits "next
///    door" to the predecessor's, which reads narratively as a
///    successor settling on the parent's frontier rather than
///    spawning an arbitrary distance away.
/// 2. **Any other claimed cell**: if no adjacent neighbour is in
///    `claimed`, return any claimed cell != parent's centroid
///    (lowest cell-id wins for determinism via `BTreeSet` order).
/// 3. **Fallback**: if `claimed` is empty or every claimed cell
///    coincides with `parent_centroid`, return `fallback` (the
///    figure-derived default). Preserves prior behaviour rather
///    than panicking on degenerate inputs.
#[must_use]
pub fn pick_successor_centroid(
    parent_centroid: u32,
    claimed: &BTreeSet<u32>,
    fallback: u32,
    grid: &sim_physics::HexGrid,
) -> u32 {
    if claimed.is_empty() {
        return fallback;
    }
    let parent_axial = grid.axial_of(sim_physics::CellId(parent_centroid));
    // Rule 1: adjacent-to-parent. Iterate the canonical neighbour
    // order so the pick is byte-deterministic across runs (the hex
    // neighbour list is fixed E,NE,NW,W,SW,SE per ).
    for nb in grid.neighbours(parent_axial) {
        if nb.0 != parent_centroid && claimed.contains(&nb.0) {
            return nb.0;
        }
    }
    // Rule 2: any other claimed cell. BTreeSet iterates in sorted
    // order so the lowest non-parent cell wins.
    for &cell in claimed {
        if cell != parent_centroid {
            return cell;
        }
    }
    // Rule 3: fallback (every claimed cell is the parent centroid).
    fallback
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_physics::{CellId, HexGrid};

    /// successor centroid prefers a hex neighbour of the
    /// parent's centroid when one is in the successor's claimed set.
    /// On a 4×4 torus with parent at cell 5 (axial (1,1)) and the
    /// successor claiming a contiguous block, the new capital lands
    /// on one of cell 5's six adjacent cells, not on cell 5 itself.
    #[test]
    fn successor_centroid_prefers_parent_neighbour() {
        let grid = HexGrid::new(4, 4);
        let parent_centroid = 5u32; // axial (1, 1)
                                    // Successor claims a block that includes parent_centroid
                                    // plus all six of its neighbours. Helper must NOT pick
                                    // parent_centroid; must pick a neighbour.
        let mut claimed: BTreeSet<u32> = BTreeSet::new();
        claimed.insert(parent_centroid);
        let parent_axial = grid.axial_of(CellId(parent_centroid));
        for nb in grid.neighbours(parent_axial) {
            claimed.insert(nb.0);
        }
        let chosen = pick_successor_centroid(parent_centroid, &claimed, parent_centroid, &grid);
        assert_ne!(chosen, parent_centroid);
        let neighbour_ids: BTreeSet<u32> =
            grid.neighbours(parent_axial).iter().map(|c| c.0).collect();
        assert!(
            neighbour_ids.contains(&chosen),
            "expected an adjacent cell of the parent centroid; got {chosen}"
        );
    }

    /// when no neighbour of the parent's centroid is claimed,
    /// the helper falls back to any other claimed cell. Sanity for
    /// the rule-2 branch.
    #[test]
    fn successor_centroid_falls_back_to_any_other_claimed_cell() {
        let grid = HexGrid::new(8, 8);
        let parent_centroid = 0u32;
        // Successor claims two cells far from cell 0's neighbours.
        let mut claimed: BTreeSet<u32> = BTreeSet::new();
        claimed.insert(30u32);
        claimed.insert(35u32);
        let parent_axial = grid.axial_of(CellId(parent_centroid));
        let neighbour_ids: BTreeSet<u32> =
            grid.neighbours(parent_axial).iter().map(|c| c.0).collect();
        for nb in &neighbour_ids {
            assert!(!claimed.contains(nb));
        }
        let chosen = pick_successor_centroid(parent_centroid, &claimed, parent_centroid, &grid);
        assert!(claimed.contains(&chosen));
        assert_ne!(chosen, parent_centroid);
    }

    /// empty claimed set falls back to the supplied default
    /// rather than panicking. Degenerate input shouldn't blow up
    /// the founding pipeline.
    #[test]
    fn successor_centroid_falls_back_when_claimed_empty() {
        let grid = HexGrid::new(4, 4);
        let claimed: BTreeSet<u32> = BTreeSet::new();
        let chosen = pick_successor_centroid(0, &claimed, 7, &grid);
        assert_eq!(chosen, 7);
    }
}
