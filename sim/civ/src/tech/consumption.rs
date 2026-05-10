//! Per-tick resource consumption: each unlocked tool with a
//! `Fuel` or `Fossil` `resource_prereqs` entry draws down the
//! substance across the civ's claimed territory, mirroring the
//! combustion stoichiometry (1 Fuel + 1 Oxidiser → 2 Ash) so
//! the cycle stays mass-conservative and feeds the regrowth
//! loop.
//!
//! Non-burnable resource prereqs (`Water`, `Ice`, `Vapour`) are
//! read-only — civs use them as cofactors but don't deplete them
//! via tool ownership. `Oxidiser` and `Ash` are produced /
//! consumed only as part of the burnable mirror.

use crate::Civ;
use sim_arith::Real;
use sim_physics::{PhysicsState, Substance};
use std::collections::BTreeSet;

use super::ToolKind;

/// Per-cell per-tool per-tick draw on a consumable substance.
/// Calibrated so an 8000-tick run with 5 unlocked Fuel-tools on
/// a 5-cell claim drains ≪ 1 unit/cell — comfortably inside the
/// `BiofuelRegrowth` recovery envelope. Tunable: bumping toward
/// `1 / 10_000` makes tool-driven depletion comparable to
/// natural wildfires within the same decade-scale window.
fn per_cell_consumption_rate() -> Real {
    Real::from_ratio(1, 100_000)
}

/// True if the substance is depleted by tool ownership
/// (combusted as fuel or used as feedstock). The five non-
/// burnable substances (`Water`, `Ice`, `Vapour`, `Oxidiser`,
/// `Ash`) are read-only at this layer — a tool may *require*
/// the substance be present in territory (`resource_prereqs`),
/// but tool ownership doesn't itself draw the cell down.
/// `Oxidiser` is depleted only via the combustion mirror below.
fn is_consumable(s: Substance) -> bool {
    matches!(s, Substance::Fuel | Substance::Fossil)
}

/// Map a `Substance.idx()` back to the enum. Mirrors the order
/// in `sim_physics::Substance` (Water=0 … Fossil=6) and is used
/// to decode `DynamicTool::resource_prereqs` (which stores the
/// substance as a `u32` so the `sim_species` crate can avoid a
/// `sim_physics` dep). Returns `None` for ids outside the
/// authored range.
fn substance_from_idx(idx: u32) -> Option<Substance> {
    match idx {
        0 => Some(Substance::Water),
        1 => Some(Substance::Ice),
        2 => Some(Substance::Vapour),
        3 => Some(Substance::Fuel),
        4 => Some(Substance::Oxidiser),
        5 => Some(Substance::Ash),
        6 => Some(Substance::Fossil),
        _ => None,
    }
}

/// Apply per-tick consumption from every active civ's unlocked
/// tools across their claimed territory. Mass-conservative
/// against the chemistry layer: each unit of Fuel/Fossil drawn
/// is matched by a unit of Oxidiser drawn and 2 units of Ash
/// produced — the exact combustion mirror — so the regrowth
/// reaction (which converts ash + cofactor water back into fuel
/// + oxidiser) closes the loop on long timescales.
///
/// Determinism: civs iterated in slice order; `claimed_cells`
/// is a `BTreeSet` (ascending iteration); `ToolKind::ALL` is
/// a fixed compile-time constant; `unlocked_dynamic_tools` is
/// a `Vec` preserving deterministic insertion order from the
/// emergence rule. Cap by available substance prevents negative
/// densities.
pub fn apply_tool_consumption(state: &mut PhysicsState, civs: &[Civ]) {
    let rate = per_cell_consumption_rate();
    for civ in civs {
        if !civ.is_active() {
            continue;
        }
        if civ.claimed_cells.is_empty() {
            continue;
        }
        // Static-tool consumption.
        for tool in ToolKind::ALL {
            if !civ.unlocked_tools.contains(&tool) {
                continue;
            }
            for (substance, _threshold) in tool.resource_prereqs() {
                if !is_consumable(*substance) {
                    continue;
                }
                consume_across_cells(state, &civ.claimed_cells, *substance, rate);
            }
        }
        // Dynamic-tool consumption.
        for dyn_tool in &civ.unlocked_dynamic_tools {
            for (substance_idx, _threshold) in &dyn_tool.resource_prereqs {
                let Some(substance) = substance_from_idx(*substance_idx) else {
                    continue;
                };
                if !is_consumable(substance) {
                    continue;
                }
                consume_across_cells(state, &civ.claimed_cells, substance, rate);
            }
        }
    }
}

/// Decrement `fuel_substance` density across `cells` by
/// `per_cell` each, drawing matching `Oxidiser` and producing
/// 2 × per-unit `Ash`. Each cell's draw is jointly capped by
/// available fuel + oxidiser (mirrors
/// `CombustionReaction::apply`'s cap).
fn consume_across_cells(
    state: &mut PhysicsState,
    cells: &BTreeSet<u32>,
    fuel_substance: Substance,
    per_cell: Real,
) {
    let fuel_idx = fuel_substance.idx();
    let ox_idx = Substance::Oxidiser.idx();
    let ash_idx = Substance::Ash.idx();
    for cell_id in cells {
        let cell = *cell_id as usize;
        // Stale `claimed_cells` (from a grid-resize) shouldn't
        // panic — silently skip.
        if state.substance(fuel_idx).get(cell).is_none() {
            continue;
        }
        let fuel_cur = state.substance(fuel_idx)[cell];
        let ox_cur = state.substance(ox_idx)[cell];
        let consumed = per_cell.min(fuel_cur).min(ox_cur);
        if consumed <= Real::ZERO {
            continue;
        }
        state.substance_mut(fuel_idx)[cell] = fuel_cur - consumed;
        state.substance_mut(ox_idx)[cell] = ox_cur - consumed;
        let ash_cur = state.substance(ash_idx)[cell];
        state.substance_mut(ash_idx)[cell] = ash_cur + (consumed + consumed);
    }
}
