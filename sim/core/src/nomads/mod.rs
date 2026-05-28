//! Nomadic species-population layer. The species occupies
//! cells across the planet *outside* any civ's territory. The
//! viewport renders these as `0` glyphs ("nomads"); a civ
//! expanding via `compute_territory` absorbs nomads from each
//! newly-claimed cell into its cohort. The pool also grows on
//! its own (slow logistic toward per-cell carrying capacity)
//! so a region that loses its civ doesn't go permanently
//! unpopulated — nomads slowly re-fill the empty land.
//!
//! Earlier the species existed *only* through its civs: empty
//! land carried no people, so a planet with one collapsed civ
//! looked deserted. This module separates the species presence (this
//! layer) from civ presence (claimed cells), so the viewport
//! can show "the species lives here even though no civ does"
//! and emergent civ founding can read densities here.
//!
//! The module is split by phase so each file stays focused:
//!
//! - [`init`] — initial origin-cell seeding (`init_pops`)
//! - [`growth`] — per-tick logistic + diffusion (`step_growth`)
//! - [`emergence`] — sustained-density gates + civ founding
//!   (`update_pressure_streak`, `scan_for_emergence`,
//!   `ambient_emergence`, `drain_observations_for_cells`)
//! - [`absorption`] — nomad → civ cohort transfer
//!   (`absorb_into_civ`)
//! - [`observation`] — per-cell per-template observation tally
//!   (`accumulate_observation`)

use sim_arith::Real;
use sim_species::Habitat;
use sim_world::{is_land_glyph, is_water_glyph, terrain_glyph_at};

pub(crate) mod absorption;
pub(crate) mod emergence;
pub(crate) mod growth;
pub(crate) mod init;
pub(crate) mod observation;

// Re-export the previous flat `nomads::*` surface so call-sites
// in `phases`, `setup`, `run_tick`, and `tick_steps` keep working
// unchanged.
pub(crate) use absorption::{FOUNDING_ABSORB_LOSS, absorb_into_civ};
pub(crate) use emergence::{
    ambient_emergence, drain_observations_for_cells, scan_for_emergence, update_pressure_streak,
    EMERGENT_FOUNDING_COOLDOWN_TICKS,
};
pub(crate) use growth::step_growth;
pub(crate) use init::init_pops;
pub(crate) use observation::accumulate_observation;

// Template / threshold constants consumed only by the nomad unit
// tests (reached via `use super::*` in `tests.rs`); their submodules
// use them through the direct const path. Gated behind `cfg(test)` so
// neither a normal build nor the test build flags the re-export as
// unused. Constants used solely within their defining submodule are
// not re-exported here at all.
#[cfg(test)]
pub(crate) use growth::{
    GROWTH_FERTILE_TEMPLATE_ID, GROWTH_FERTILE_THRESHOLD, GROWTH_FIRE_TEMPLATE_ID,
    GROWTH_FIRE_THRESHOLD, GROWTH_SEASONAL_TEMPLATE_ID, GROWTH_SEASONAL_THRESHOLD,
    GROWTH_SOLVENT_TEMPLATE_ID, GROWTH_SOLVENT_THRESHOLD, GROWTH_THERMAL_TEMPLATE_ID,
    GROWTH_THERMAL_THRESHOLD, NOMAD_DIFFUSION_BASELINE_LIFESPAN_YEARS,
};
#[cfg(test)]
pub(crate) use init::{INITIAL_NOMAD_TOTAL, NOMAD_ORIGIN_CELL_COUNT};

/// Whether `cell`'s terrain matches the species' native habitat.
/// Water glyphs for aquatic, land for terrestrial, both for
/// amphibious. Coast counts as both — transition zone.
///
/// Shared helper because [`init::init_pops`], [`growth::step_growth`],
/// [`emergence::ambient_emergence`], and the per-cell pressure /
/// cluster thresholds all need the same biome-match predicate.
pub(crate) fn is_habitat_match(
    state: &sim_physics::PhysicsState,
    planet: &sim_world::Planet,
    cell: u32,
    species_habitat: Habitat,
) -> bool {
    let glyph = terrain_glyph_at(state, planet, cell);
    if glyph == '\u{2261}' {
        return false; // gas band — uninhabitable
    }
    match species_habitat {
        Habitat::Aquatic => is_water_glyph(glyph),
        // Airborne lives on land; flight enables crossing wrong-
        // biome cells via tech-gated transit, not native habitation.
        Habitat::Terrestrial
        | Habitat::Airborne
        | Habitat::Subterranean
        | Habitat::Endolithic => is_land_glyph(glyph),
        Habitat::Amphibious => true,
    }
}

/// Habitability weight used by every nomad-phase helper: the cell's
/// raw habitability multiplier, overridden to `1.0` for aquatic
/// species in deep-ocean cells (whose nominal multiplier is `0`)
/// so an aquatic species can originate / live offshore.
pub(crate) fn cell_weight(
    state: &sim_physics::PhysicsState,
    planet: &sim_world::Planet,
    cell: u32,
    species_habitat: Habitat,
) -> Real {
    let mult = sim_world::cell_habitability(state, planet, cell);
    if matches!(species_habitat, Habitat::Aquatic) && mult == Real::ZERO {
        Real::ONE
    } else {
        mult
    }
}

#[cfg(test)]
mod tests;
