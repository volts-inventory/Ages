//! Per-terrain habitability multipliers, claim-eligibility
//! threshold, and the per-cell terrain-glyph classifier that maps
//! a `(state, planet, cell)` tuple to one of the eight render
//! glyphs. Pulled out of `lib.rs` so the calibration table
//! sits next to its rationale rather than getting lost in the
//! middle of the world-sampling pipeline.

use crate::Planet;
use sim_arith::Real;
use sim_physics::PhysicsState;

/// Per-terrain-glyph habitability multiplier on per-cell
/// carrying capacity. Same eight glyph alphabet the renderer
/// uses (`sim/report/src/render.rs::terrain_symbol` +
/// `sim/report/src/frame.rs::terrain_color_code`):
///
/// | glyph | terrain      | multiplier | rationale                |
/// |-------|--------------|-----------:|--------------------------|
/// | `≈`   | deep ocean   |  0.00      | uninhabitable            |
/// | `~`   | shallow sea  |  0.05      | fishing margin only      |
/// | `░`   | coast        |  1.20      | rich (fish + farm)       |
/// | `·`   | plain / featureless | 1.00 | baseline                 |
/// | `▒`   | inland land  |  0.90      | generic                  |
/// | `△`   | hill / low mountain | 0.60 | rough but workable      |
/// | `▲`   | peak         |  0.10      | nearly uninhabitable     |
/// | `≡`   | gas          |  0.00      | uninhabitable            |
///
/// Anything else (a glyph the renderer doesn't produce, e.g. a
/// civ marker leaking in) defaults to `1.00` so unfamiliar inputs
/// never inflate the cell. All multipliers are dimensionless
/// `Real` (Q32.32) values — no `f64` enters the sim path.
#[must_use]
pub fn habitability_multiplier(glyph: char) -> Real {
    match glyph {
        // ≈ deep ocean / ≡ gas band — uninhabitable.
        '\u{2248}' | '\u{2261}' => Real::ZERO,
        '~' => Real::percent(5),          // shallow sea
        '\u{2591}' => Real::percent(120), // ░ coast
        '\u{2592}' => Real::percent(90),  // ▒ inland
        '\u{25B3}' => Real::percent(60),  // △ hill / low mountain
        '\u{25B2}' => Real::percent(10),  // ▲ peak
        // · plain / featureless and any unrecognised glyph default
        // to baseline 1.00 — the explicit `·` arm is folded into
        // the wildcard for clippy's match-same-arms lint.
        _ => Real::ONE,
    }
}

/// Claim-eligibility threshold. A cell whose habitability
/// multiplier is `< CLAIM_HABITABILITY_THRESHOLD` is treated as a wall by
/// the territory BFS and is rejected as a candidate capital. The
/// threshold (`0.05`, exclusive) was chosen so deep ocean (`≈`,
/// `0.00`) and gas band (`≡`, `0.00`) are excluded but shallow sea
/// (`~`, `0.05`) remains marginally claimable — peaks (`▲`, `0.10`)
/// stay claimable but contribute almost nothing to capacity.
pub const CLAIM_HABITABILITY_THRESHOLD_NUM: i64 = 5;
pub const CLAIM_HABITABILITY_THRESHOLD_DEN: i64 = 100;

/// Returns `true` if a cell with the given habitability
/// multiplier is claimable by a civ. Used by both the BFS gate
/// (don't visit walls) and the capital-eligibility check (don't
/// found on water or gas).
#[must_use]
pub fn is_claimable_multiplier(mult: Real) -> bool {
    mult >= Real::from_ratio(
        CLAIM_HABITABILITY_THRESHOLD_NUM,
        CLAIM_HABITABILITY_THRESHOLD_DEN,
    )
}

/// Per-cell terrain glyph derived from the same fields
/// `sim/report/src/render.rs::terrain_symbol` reads — elevation,
/// `water_depth`, and the planet's `terrain_peak`. Mirrors the
/// renderer's classification so habitability decisions in
/// `sim/civ` agree with what the user sees on the viewport map.
///
/// The `~`/`░` distinction depends on the cell's neighbours
/// (coast = land within 1 cell of water), so the function reads
/// the four cardinal axial neighbours through the same `state`
/// it was given. Wraps through the torus via `HexGrid::cell_id`
/// to match `init_planet`'s torus-wrapped peak placement.
#[must_use]
pub fn terrain_glyph_at(state: &PhysicsState, planet: &Planet, cell: u32) -> char {
    let i = cell as usize;
    let elev = state.elevation().get(i).copied().unwrap_or(Real::ZERO);
    let depth = state.water_depth().get(i).copied().unwrap_or(Real::ZERO);
    // Mirror render.rs: deep water > 100 m → `≈`, any depth → `~`.
    if depth > Real::from_int(100) {
        return '\u{2248}'; // deep water
    }
    if depth > Real::ZERO {
        return '~';
    }
    // Tall-elevation glyphs only fire when the planet has a real
    // terrain peak (zero on `GaseousShell` / sub-surface ocean
    // archetypes); the renderer uses the same gate.
    let peak = planet.terrain_peak;
    let peak_70 = (peak * Real::from_int(7)) / Real::from_int(10);
    let peak_40 = (peak * Real::from_int(4)) / Real::from_int(10);
    if peak > Real::ZERO && elev > peak_70 {
        return '\u{25B2}'; // ▲ peak
    }
    if peak > Real::ZERO && elev > peak_40 {
        return '\u{25B3}'; // △ low mountain
    }
    if elev <= Real::ZERO {
        // Fallback split: zero terrain_peak → gas band; else
        // featureless plain.
        if peak == Real::ZERO {
            return '\u{2261}'; // ≡ gas band
        }
        return '\u{00B7}'; // · featureless / plain
    }
    // Land: coast vs inland by checking the four cardinal axial
    // neighbours for water. Matches render.rs's coastal heuristic.
    let grid = state.grid();
    let axial = grid.axial_of(sim_physics::CellId(cell));
    let neighbour_is_water = |dq: i32, dr: i32| -> bool {
        let nb = grid.cell_id(sim_physics::Axial {
            q: axial.q + dq,
            r: axial.r + dr,
        });
        state
            .water_depth()
            .get(nb.0 as usize)
            .copied()
            .is_some_and(|d| d > Real::ZERO)
    };
    let coastal = neighbour_is_water(0, -1)
        || neighbour_is_water(0, 1)
        || neighbour_is_water(-1, 0)
        || neighbour_is_water(1, 0);
    if coastal {
        '\u{2591}' // ░ coast
    } else {
        '\u{2592}' // ▒ inland
    }
}

/// Per-cell habitability multiplier computed straight from
/// the cell's terrain glyph. Convenience wrapper that composes
/// `terrain_glyph_at` with `habitability_multiplier`.
#[must_use]
pub fn cell_habitability(state: &PhysicsState, planet: &Planet, cell: u32) -> Real {
    habitability_multiplier(terrain_glyph_at(state, planet, cell))
}

/// `true` if `glyph` represents a water cell (deep ocean,
/// shallow sea, or coastal land that's adjacent to water). Coast
/// counts as water *and* land — the transition zone — so both
/// aquatic and terrestrial civs can claim it without needing
/// `AmphibiousConstruction` tech.
#[must_use]
pub fn is_water_glyph(glyph: char) -> bool {
    matches!(
        glyph,
        '\u{2248}' // ≈ deep ocean
        | '~'
        | '\u{2591}' // ░ coast (transition zone — both)
    )
}

/// `true` if `glyph` represents a land cell (inland, hill,
/// peak, plain, or coast). Coast counts as both — see
/// `is_water_glyph`. Gas band is neither (uniformly uninhabitable
/// via the habitability multiplier).
#[must_use]
pub fn is_land_glyph(glyph: char) -> bool {
    matches!(
        glyph,
        '\u{2592}' // ▒ inland
        | '\u{2591}' // ░ coast (transition zone — both)
        | '\u{25B3}' // △ hill
        | '\u{25B2}' // ▲ peak
        | '\u{00B7}' // · featureless plain
    )
}
