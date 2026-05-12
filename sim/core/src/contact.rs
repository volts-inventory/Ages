//! Distance + tech + terrain gating for `CivContact` emission.
//!
//! Pre-existing behaviour: `CivContact` fires the moment two civs
//! coexist anywhere on the planet. Civs across the world from each
//! other "met" the same tick the second one founded.
//!
//! New behaviour (this module): contact fires when one civ can
//! plausibly reach the other given (a) per-civ contact range
//! (foot → boats → navigation → radio), (b) terrain barriers (water
//! cells block land-only civs and vice versa), and (c) a bounded
//! BFS on the hex torus from civ A's centroid through cells either
//! civ can cross.
//!
//! The breakaway path in `lib.rs` keeps the unconditional emit — a
//! child civ knows its parent.

use sim_civ::{tech::ToolKind, Civ};
use sim_physics::PhysicsState;
use sim_species::Habitat;
use std::collections::VecDeque;

/// Tick cadence for the M5 coexistence-pair contact pass. Bounded
/// BFS per un-met pair adds up if we run it every tick; once a year
/// is plenty for what is, narratively, a diplomatic-scale event.
/// Matches the cadence-style precedent set by
/// `conflict::CONFLICT_CHECK_TICKS` (every-N-ticks gate).
pub const CONTACT_CHECK_TICKS: u64 = 12;

/// Contact range radius in hex cells when the civ has *no* nav,
/// transport, or comms tools. Foot / runner range — barely enough
/// to span a single civ's claimed territory.
const RANGE_FOOT: u32 = 4;
/// Contact range radius when the civ has tier-2 boats or tier-2
/// trade networks. Coastal / short-haul mobility.
const RANGE_TIER2: u32 = 10;
/// Contact range radius when the civ has tier-3+ navigation or
/// tier-4 ground/air transport. Open-ocean and continent-spanning.
const RANGE_TIER3: u32 = 20;

/// Hex contact range for a civ in cells. `u32::MAX` means unlimited
/// (radio-equivalent or better — terrain-agnostic too).
pub fn contact_range(civ: &Civ) -> u32 {
    let has = |k: ToolKind| civ.unlocked_tools.contains(&k);
    if has(ToolKind::LongRangeCommunication)
        || has(ToolKind::InformationNetworking)
        || has(ToolKind::OrbitalReach)
    {
        u32::MAX
    } else if has(ToolKind::LongRangeNavigation)
        || has(ToolKind::HeavyTransport)
        || has(ToolKind::AerialTransport)
    {
        RANGE_TIER3
    } else if has(ToolKind::WatercraftConstruction)
        || has(ToolKind::MotivePropulsion)
        || has(ToolKind::TradeNetworks)
    {
        RANGE_TIER2
    } else {
        RANGE_FOOT
    }
}

/// Can the civ traverse water cells? Aquatic / amphibious species
/// always can; airborne species fly across water natively;
/// terrestrial civs unlock it via watercraft, navigation, or
/// amphibious construction. Aerial transport flies over water.
pub fn can_traverse_water(civ: &Civ, habitat: Habitat) -> bool {
    if matches!(
        habitat,
        Habitat::Aquatic | Habitat::Amphibious | Habitat::Airborne
    ) {
        return true;
    }
    let has = |k: ToolKind| civ.unlocked_tools.contains(&k);
    has(ToolKind::WatercraftConstruction)
        || has(ToolKind::MotivePropulsion)
        || has(ToolKind::LongRangeNavigation)
        || has(ToolKind::AmphibiousConstruction)
        || has(ToolKind::AerialTransport)
}

/// Can the civ traverse land cells? Terrestrial / amphibious /
/// airborne species always can; aquatic civs unlock it via
/// amphibious construction or aerial transport.
pub fn can_traverse_land(civ: &Civ, habitat: Habitat) -> bool {
    if matches!(
        habitat,
        Habitat::Terrestrial | Habitat::Amphibious | Habitat::Airborne
    ) {
        return true;
    }
    let has = |k: ToolKind| civ.unlocked_tools.contains(&k);
    has(ToolKind::AmphibiousConstruction) || has(ToolKind::AerialTransport)
}

/// Are these two civs in contact range, accounting for tech +
/// terrain? Returns true when:
///  - either civ has unlimited range (radio / orbital / internet), OR
///  - bounded BFS from `civ_a`'s `territory_centroid` reaches any
///    cell in `civ_b.claimed_cells` within `range(a) + range(b)`
///    steps, traversing only cells passable by *either* civ (the
///    union: A's runners cover land, B's boats cover water — the
///    message hops via whichever can carry it on each cell).
pub fn civs_in_contact(
    civ_a: &Civ,
    civ_b: &Civ,
    habitat: Habitat,
    state: &PhysicsState,
) -> bool {
    /// Hex-axial neighbour offsets in `(dq, dr)` form, paired with
    /// the wrap convention used by the rest of the sim. Encoded as
    /// `i8` so the BFS can stay in unsigned arithmetic and still
    /// handle the −1 step via modular `+ (width-1) % width`.
    const OFFSETS: [(i8, i8); 6] =
        [(1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)];

    let range_a = contact_range(civ_a);
    let range_b = contact_range(civ_b);
    if range_a == u32::MAX || range_b == u32::MAX {
        return true;
    }
    let budget = range_a.saturating_add(range_b);
    if budget == 0 {
        return false;
    }
    let any_water = can_traverse_water(civ_a, habitat) || can_traverse_water(civ_b, habitat);
    let any_land = can_traverse_land(civ_a, habitat) || can_traverse_land(civ_b, habitat);

    let grid = state.grid();
    let width = grid.width();
    let height = grid.height();
    if width == 0 || height == 0 {
        return false;
    }
    let total = (width as usize).saturating_mul(height as usize);
    let centroid = civ_a.territory_centroid;
    if (centroid as usize) >= total {
        return false;
    }
    if civ_b.claimed_cells.contains(&centroid) {
        return true;
    }

    let depth = state.water_depth();
    let cell_passable = |id: u32| -> bool {
        let idx = id as usize;
        if idx >= depth.len() {
            return false;
        }
        let is_water = depth[idx] > sim_arith::Real::from_int(0);
        if is_water {
            any_water
        } else {
            any_land
        }
    };

    if !cell_passable(centroid) {
        return false;
    }

    let mut visited = vec![false; total];
    visited[centroid as usize] = true;
    let mut queue: VecDeque<(u32, u32)> = VecDeque::new();
    queue.push_back((centroid, 0));
    while let Some((cell, d)) = queue.pop_front() {
        if d >= budget {
            continue;
        }
        let q = cell % width;
        let r = cell / width;
        for (dq, dr) in OFFSETS {
            let nq = match dq {
                1 => (q + 1) % width,
                -1 => (q + width - 1) % width,
                _ => q,
            };
            let nr = match dr {
                1 => (r + 1) % height,
                -1 => (r + height - 1) % height,
                _ => r,
            };
            let nid = nr * width + nq;
            let nidx = nid as usize;
            if visited[nidx] {
                continue;
            }
            if !cell_passable(nid) {
                continue;
            }
            visited[nidx] = true;
            if civ_b.claimed_cells.contains(&nid) {
                return true;
            }
            queue.push_back((nid, d + 1));
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_civ::Civ;
    use sim_physics::{HexGrid, PhysicsState};

    fn empty_state(width: u32, height: u32, water_cells: &[u32]) -> PhysicsState {
        let grid = HexGrid::new(width, height);
        let mut state = PhysicsState::new(grid);
        for &c in water_cells {
            state.water_depth_mut()[c as usize] = sim_arith::Real::from_int(1);
        }
        state
    }

    fn civ_at(id: u32, centroid: u32, claimed: &[u32]) -> Civ {
        let mut c = Civ::new(id, 0, sim_arith::Pop::from_int(1));
        c.territory_centroid = centroid;
        c.claimed_cells = claimed.iter().copied().collect();
        c
    }

    #[test]
    fn far_apart_terrestrial_civs_dont_meet() {
        // 30×30 hex torus; civs placed half-axis apart so the
        // wrap-around path (15 steps the other way) is also outside
        // the default 4+4 = 8 step budget.
        let state = empty_state(30, 30, &[]);
        let a = civ_at(1, 0, &[0]);
        let b = civ_at(2, 15, &[15]);
        assert!(!civs_in_contact(&a, &b, Habitat::Terrestrial, &state));
    }

    #[test]
    fn adjacent_terrestrial_civs_meet() {
        let state = empty_state(20, 20, &[]);
        let a = civ_at(1, 0, &[0]);
        let b = civ_at(2, 1, &[1]);
        assert!(civs_in_contact(&a, &b, Habitat::Terrestrial, &state));
    }

    #[test]
    fn water_blocks_terrestrial_civs() {
        // Column x=5 is a sea wall.
        let water: Vec<u32> = (0..20).map(|r| r * 20 + 5).collect();
        let state = empty_state(20, 20, &water);
        let a = civ_at(1, 0, &[0]); // left of wall
        let b = civ_at(2, 6, &[6]); // right of wall, ~6 cells away
        assert!(!civs_in_contact(&a, &b, Habitat::Terrestrial, &state));
    }

    #[test]
    fn watercraft_crosses_water() {
        let water: Vec<u32> = (0..20).map(|r| r * 20 + 5).collect();
        let state = empty_state(20, 20, &water);
        let mut a = civ_at(1, 0, &[0]);
        a.unlocked_tools.insert(ToolKind::WatercraftConstruction);
        let b = civ_at(2, 6, &[6]);
        assert!(civs_in_contact(&a, &b, Habitat::Terrestrial, &state));
    }

    #[test]
    fn long_range_communication_meets_anywhere() {
        // Same far placement as `far_apart_terrestrial_civs_dont_meet`
        // — radio short-circuits the BFS and connects regardless of
        // distance or terrain.
        let state = empty_state(30, 30, &[]);
        let mut a = civ_at(1, 0, &[0]);
        a.unlocked_tools.insert(ToolKind::LongRangeCommunication);
        let b = civ_at(2, 15, &[15]);
        assert!(civs_in_contact(&a, &b, Habitat::Terrestrial, &state));
    }

    #[test]
    fn aquatic_species_meet_across_water() {
        let water: Vec<u32> = (0..400).collect();
        let state = empty_state(20, 20, &water);
        let a = civ_at(1, 0, &[0]);
        let b = civ_at(2, 4, &[4]);
        assert!(civs_in_contact(&a, &b, Habitat::Aquatic, &state));
    }

    #[test]
    fn aquatic_species_blocked_by_land() {
        let state = empty_state(20, 20, &[]); // all land
        let a = civ_at(1, 0, &[0]);
        let b = civ_at(2, 1, &[1]);
        assert!(!civs_in_contact(&a, &b, Habitat::Aquatic, &state));
    }
}
