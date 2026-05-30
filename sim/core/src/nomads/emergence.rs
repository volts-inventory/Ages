//! Emergent civ founding from saturated nomadic cells, plus the
//! background "ambient" seeding that re-fills regions emptied by
//! civ absorption.
//!
//! Founding pipeline (each tick):
//! 1. [`update_pressure_streak`] increments the per-cell streak
//!    counter for cells holding `pop ≥ pressure_threshold`.
//! 2. [`scan_for_emergence`] picks the best candidate cell that
//!    satisfies the full set of gates (sustained density,
//!    cluster, tech, civ-distance) — see the function's docstring
//!    for the formal list.
//! 3. The caller in `tick_steps::emergent_founding` consumes the
//!    pick and runs civ creation + [`drain_observations_for_cells`]
//!    + [`super::absorption::absorb_into_civ`].
//!
//! Separately, [`ambient_emergence`] runs once every
//! `AMBIENT_NOMAD_CHECK_TICKS` and seeds a single empty habitable
//! cell with a tiny kernel population, so a region depopulated by
//! civ absorption or collapse can re-fill over decades rather than
//! staying empty forever.

use super::{growth::tech_score, is_habitat_match};
use sim_arith::Real;
use sim_species::Habitat;
use std::collections::BTreeMap;

/// Emergent founding **minimum population** floor: a cell
/// can't coalesce into a civilisation unless its nomadic pop is
/// at least this many people. Combined with the tech
/// threshold below — a cell with high tech but only a handful of
/// people is still a hunter-gatherer band, not a civilisation.
///
/// Acts as the absolute floor on substrate-poor seeds where the
/// relative `cell_forager_cap × habitability` may itself be small.
/// On normal seeds the relative saturation gate
/// (`EMERGENT_PRESSURE_NUM/DEN`) dominates. Scaled up alongside the
/// biosphere-coupled forager cap (which now reads in the thousands
/// rather than the old flat ~80) so founding still requires a real
/// village, not 20 stragglers.
pub(crate) const EMERGENT_FOUNDING_POP: i64 = 300;

/// Relative saturation gate: a cell must hold at least this
/// fraction of its own `NOMAD_PER_CELL_CAP × habitability`
/// before it can spawn a civ. 80% means the cell is a genuinely
/// saturated village (Çatalhöyük-density) rather than a 25%-
/// filled hunter band — matches the historical agricultural-
/// revolution → first-cities transition where surplus + density
/// drove the urban centralisation. Without this gate a fertile
/// cell spawned a civ the moment nomads first arrived; with it,
/// the cell has to actually fill up first.
pub(crate) const EMERGENT_PRESSURE_NUM: i64 = 80;
pub(crate) const EMERGENT_PRESSURE_DEN: i64 = 100;

/// Cluster-density gate: a candidate cell must have at least
/// `EMERGENT_CLUSTER_MIN_NEIGHBOURS` of its 6 hex neighbours each
/// holding `pop ≥ EMERGENT_CLUSTER_NUM/DEN × cap`. Models
/// "village + satellite settlements" — a single dense cell
/// surrounded by empty terrain is a transient peak, not a
/// civilisation core. The numeric thresholds (30% of cap, ≥2
/// neighbours) match the archaeological "hinterland of feeding
/// villages within walking distance of an urban core" pattern.
pub(crate) const EMERGENT_CLUSTER_NUM: i64 = 30;
pub(crate) const EMERGENT_CLUSTER_DEN: i64 = 100;
pub(crate) const EMERGENT_CLUSTER_MIN_NEIGHBOURS: usize = 2;

/// Sustained-density gate: the cell must have held the
/// saturation threshold for at least this many ticks before it
/// can spawn a civ. 60 ticks ≈ 5 sim-years — long enough that
/// a transient demographic peak (e.g. nomads passing through)
/// can't trigger a civ; short enough that a cell that genuinely
/// fills up doesn't wait a generation to spawn one.
pub(crate) const EMERGENT_SUSTAINED_TICKS: u64 = 60;

/// Emergent founding **tech threshold**: cumulative per-cell
/// observation pressure (in `cell_tech` units, see
/// `accumulate_tech`) required for a civilisation to emerge.
/// Pinned at 50 — at typical species `cognition × sociality`
/// (~0.25-0.50 product) × per-cell recognition-firing rates
/// (~1-3 firings/tick on a habitable cell with active phenomena
/// like water, fire, vapour, biomass) this lands at ~100-300
/// ticks of accumulation. A barren cell (no fires, no water
/// cycle, no life signal) accumulates slowly or not at all. A
/// rich cell crosses quickly. Species' cognition + sociality
/// gate the rate, so a low-cognition species takes much longer
/// to learn the same physics.
pub(crate) const EMERGENT_FOUNDING_TECH: i64 = 50;

/// Cooldown between successive emergent foundings. Prevents
/// every cell on the planet from coalescing into a civ on the
/// same tick once growth equalises across the map. 600 ticks
/// (~50 sim-years) so foundings stay once-per-generation events
/// rather than the pre-tightening cadence of one every 4 years
/// (which produced 7-11 concurrent civs on small-grid seeds —
/// well above the historical Iron-Age cap of ~5 concurrent
/// civilisations on Earth).
pub(crate) const EMERGENT_FOUNDING_COOLDOWN_TICKS: u64 = 600;

/// How often to seed a fresh nomadic population in an empty
/// habitable cell. 60 ticks ≈ 5 sim-years on the 1-tick = 1-month
/// cadence — slow enough that ambient seeding doesn't dominate
/// growth + diffusion (which are per-tick), fast enough that a
/// continent that lost its nomads to civ absorption can re-seed
/// over a few sim-decades rather than staying empty for centuries.
pub(crate) const AMBIENT_NOMAD_CHECK_TICKS: u64 = 60;

/// Founding pop dropped into a freshly-seeded ambient cell.
/// Smaller than `INITIAL_NOMAD_TOTAL / origins.len()` (typical
/// per-origin share at sim start) so ambient seeding is a kernel
/// for diffusion + growth to amplify, not a pre-formed band.
/// Scaled up in lockstep with the biosphere-coupled forager cap so
/// the kernel stays the same ~10%-of-cap fraction it always was —
/// preserving the slow-trickle re-seed pacing despite the larger
/// absolute populations.
pub(crate) const AMBIENT_NOMAD_SEED_POP: i64 = 120;

/// Background nomadic emergence in unclaimed habitable cells.
/// Runs once per `AMBIENT_NOMAD_CHECK_TICKS` ticks after
/// `step_growth` + `absorb_into_civ`. Picks a single deterministic
/// unclaimed habitable cell with no current nomadic population and
/// no civ claim, and seeds it with `AMBIENT_NOMAD_SEED_POP`.
///
/// Why "ambient" rather than just relying on diffusion: when a
/// civ collapses + then absorbs all its surrounding nomads as it
/// grew, the cells around it can become genuinely empty — no
/// neighbouring populated cells to diffuse from. Without ambient
/// seeding such a region stays empty forever, which contradicts
/// the "species persists across civ collapses" promise. Ambient
/// seeding provides a slow-trickle mechanism that mirrors the
/// historical reality of off-grid migrant bands wandering into
/// vacated regions.
///
/// Determinism: cell pick is hashed from `tick + planet_seed` so
/// the same (seed, grid) pair always seeds the same cell at the
/// same tick. Iteration starts at the hashed offset and walks
/// forward through the grid in canonical id order, picking the
/// first qualifying cell. Empty grids and full-civ-claim grids
/// both no-op.
pub(crate) fn ambient_emergence(
    pops: &mut BTreeMap<u32, Real>,
    state: &sim_physics::PhysicsState,
    planet: &sim_world::Planet,
    species_habitat: Habitat,
    claimed_union: &std::collections::BTreeSet<u32>,
    tick: u64,
) {
    if !tick.is_multiple_of(AMBIENT_NOMAD_CHECK_TICKS) {
        return;
    }
    let n = state.grid().n_cells();
    if n == 0 {
        return;
    }
    // Determinism: Knuth multiplicative hash of (tick ^ seed) → start
    // offset. Same seed + same tick → same offset across replays.
    let mix = (tick ^ planet.seed).wrapping_mul(2_654_435_761);
    let start = (mix as usize) % n;
    for i in 0..n {
        let cell = u32::try_from((start + i) % n).unwrap_or(0);
        if claimed_union.contains(&cell) {
            continue;
        }
        if pops.get(&cell).is_some_and(|p| *p > Real::ZERO) {
            continue;
        }
        // Habitability gates: gas band is a hard wall for every
        // habitat; the species's native habitat must match the
        // cell's domain (terrestrial → land, aquatic → water,
        // amphibious → either); habitability multiplier must be
        // non-zero so the seeded pop has somewhere to land.
        let glyph = sim_world::terrain_glyph_at(state, planet, cell);
        if glyph == '\u{2261}' {
            continue;
        }
        if !is_habitat_match(state, planet, cell, species_habitat) {
            continue;
        }
        let mult = sim_world::cell_habitability(state, planet, cell);
        if mult == Real::ZERO && !matches!(species_habitat, Habitat::Aquatic) {
            continue;
        }
        pops.insert(cell, Real::from_int(AMBIENT_NOMAD_SEED_POP));
        return;
    }
}

/// Per-cell relative saturation threshold used by the founding
/// gate. Returns the absolute population required for the cell
/// to be considered "saturated" — `EMERGENT_PRESSURE_NUM/DEN ×
/// NOMAD_PER_CELL_CAP × habitability(cell)`, floored at
/// `EMERGENT_FOUNDING_POP` (the absolute floor that protects
/// substrate-poor seeds where the relative threshold would be
/// trivially low).
fn pressure_threshold(
    state: &sim_physics::PhysicsState,
    planet: &sim_world::Planet,
    species_habitat: Habitat,
    cell: u32,
    tick: u64,
    cognition: Real,
    producer_biomass: Real,
    survivability: Real,
) -> Real {
    let cap = super::growth::cell_forager_cap(
        state,
        planet,
        cell,
        tick,
        species_habitat,
        cognition,
        producer_biomass,
        survivability,
    );
    let saturation = cap * Real::from_ratio(EMERGENT_PRESSURE_NUM, EMERGENT_PRESSURE_DEN);
    let floor = Real::from_int(EMERGENT_FOUNDING_POP);
    if saturation > floor {
        saturation
    } else {
        floor
    }
}

/// Per-cell cluster-neighbour threshold — `EMERGENT_CLUSTER_NUM/DEN
/// × NOMAD_PER_CELL_CAP × habitability(cell)`. Lower than the
/// pressure threshold; neighbour cells just need to be a non-
/// trivial supporting population, not saturated themselves.
fn cluster_threshold(
    state: &sim_physics::PhysicsState,
    planet: &sim_world::Planet,
    species_habitat: Habitat,
    cell: u32,
    tick: u64,
    cognition: Real,
    producer_biomass: Real,
    survivability: Real,
) -> Real {
    let cap = super::growth::cell_forager_cap(
        state,
        planet,
        cell,
        tick,
        species_habitat,
        cognition,
        producer_biomass,
        survivability,
    );
    cap * Real::from_ratio(EMERGENT_CLUSTER_NUM, EMERGENT_CLUSTER_DEN)
}

/// Update the per-cell sustained-density streak counter. Cells
/// holding `pop ≥ pressure_threshold(cell)` get their streak
/// incremented; cells below threshold have their streak removed
/// (reset to zero on next check). Run once per tick before
/// `scan_for_emergence`.
pub(crate) fn update_pressure_streak(
    streak: &mut BTreeMap<u32, u64>,
    pops: &BTreeMap<u32, Real>,
    state: &sim_physics::PhysicsState,
    planet: &sim_world::Planet,
    species_habitat: Habitat,
    tick: u64,
    cognition: Real,
    producer_biomass: Real,
    survivability: Real,
) {
    let alive: std::collections::BTreeSet<u32> = pops.keys().copied().collect();
    streak.retain(|cell, _| alive.contains(cell));
    for (&cell, &pop) in pops {
        let threshold = pressure_threshold(
            state,
            planet,
            species_habitat,
            cell,
            tick,
            cognition,
            producer_biomass,
            survivability,
        );
        if pop >= threshold {
            *streak.entry(cell).or_insert(0) += 1;
        } else {
            streak.remove(&cell);
        }
    }
}

/// Scan the nomad pool for cells that have crossed the emergent-
/// founding criteria. A cell qualifies when ALL of:
/// (a) `pop ≥ pressure_threshold(cell)` — relative saturation
///     gate (the cell is genuinely a Çatalhöyük-density village,
///     not a 25%-filled hunter band)
/// (b) `streak[cell] ≥ EMERGENT_SUSTAINED_TICKS` — the
///     saturation has held for ≥ 5 sim-years, ruling out
///     transient demographic peaks
/// (c) ≥ `EMERGENT_CLUSTER_MIN_NEIGHBOURS` of the 6 hex
///     neighbours each hold `pop ≥ cluster_threshold(neighbour)`
///     — village + satellite settlements, not an isolated peak
/// (d) `tech_score(observations[cell]) ≥ EMERGENT_FOUNDING_TECH`
///     — local nomads have learnt enough physics to pass it
///     down as tradition / building / settlement
/// (e) the cell isn't claimed by any civ
/// (f) the cell is at least 4 hex-axial cells from any existing
///     civ centroid (the distant-spawn rule extended)
///
/// Tie-break: highest tech, then highest pop, then smallest cell
/// id. Deterministic.
#[allow(clippy::too_many_arguments)]
pub(crate) fn scan_for_emergence(
    pops: &BTreeMap<u32, Real>,
    observations: &BTreeMap<u32, BTreeMap<u32, u64>>,
    streak: &BTreeMap<u32, u64>,
    cognition: Real,
    sociality: Real,
    state: &sim_physics::PhysicsState,
    planet: &sim_world::Planet,
    species_habitat: Habitat,
    tick: u64,
    producer_biomass: Real,
    survivability: Real,
    civ_centroids: &[u32],
    civ_claims: &std::collections::BTreeSet<u32>,
) -> Option<u32> {
    let tech_threshold = Real::from_int(EMERGENT_FOUNDING_TECH);
    let min_distance_from_centroid: i64 = 4;
    let grid = state.grid();
    let mut best: Option<(Real, Real, u32)> = None;
    for (&cell, &pop) in pops {
        if civ_claims.contains(&cell) {
            continue;
        }
        // Reject candidates whose terrain has drifted out from
        // under the nomadic pop. The `pops` map carries population
        // independent of habitability — a coast cell that nomads
        // colonised tick 0 can be flooded to deep ocean by tick 600
        // and still show up here as saturated. Without this gate,
        // `compute_territory`'s centroid override force-claims the
        // now-uninhabitable cell, founding the civ on a cap-0 phantom.
        if !crate::territory::is_habitat_claimable_at(state, planet, cell, species_habitat) {
            continue;
        }
        let saturation = pressure_threshold(
            state,
            planet,
            species_habitat,
            cell,
            tick,
            cognition,
            producer_biomass,
            survivability,
        );
        if pop < saturation {
            continue;
        }
        if streak.get(&cell).copied().unwrap_or(0) < EMERGENT_SUSTAINED_TICKS {
            continue;
        }
        // Cluster check: how many neighbours are themselves at
        // ≥ cluster threshold?
        let nbrs = grid.neighbours(grid.axial_of(sim_physics::CellId(cell)));
        let dense_neighbours: usize = nbrs
            .iter()
            .filter(|nbr| {
                let nbr_id = nbr.0;
                let nbr_pop = pops.get(&nbr_id).copied().unwrap_or(Real::ZERO);
                let nbr_threshold = cluster_threshold(
                    state,
                    planet,
                    species_habitat,
                    nbr_id,
                    tick,
                    cognition,
                    producer_biomass,
                    survivability,
                );
                nbr_pop >= nbr_threshold
            })
            .count();
        if dense_neighbours < EMERGENT_CLUSTER_MIN_NEIGHBOURS {
            continue;
        }
        let cell_tech = tech_score(observations.get(&cell), cognition, sociality);
        if cell_tech < tech_threshold {
            continue;
        }
        let cell_axial = grid.axial_of(sim_physics::CellId(cell));
        let too_close = civ_centroids.iter().any(|&c| {
            let ca = grid.axial_of(sim_physics::CellId(c));
            i64::from((cell_axial.q - ca.q).abs() + (cell_axial.r - ca.r).abs())
                < min_distance_from_centroid
        });
        if too_close {
            continue;
        }
        let key = (cell_tech, pop, cell);
        let better = match &best {
            None => true,
            Some((bt, bp, _)) => key.0 > *bt || (key.0 == *bt && key.1 > *bp),
        };
        if better {
            best = Some(key);
        }
    }
    best.map(|(_, _, c)| c)
}

/// Drain per-template observations from `cells` into a
/// merged `BTreeMap<template_id, count>`. Used at emergence to
/// pour everything the civ's claimed cells have learnt into the
/// new civ's `observations` field. Drained cells are removed
/// from the nomad observation map (the knowledge has been
/// "transferred" to the civ — no double-counting).
pub(crate) fn drain_observations_for_cells(
    observations: &mut BTreeMap<u32, BTreeMap<u32, u64>>,
    cells: impl IntoIterator<Item = u32>,
) -> BTreeMap<u32, u64> {
    let mut merged: BTreeMap<u32, u64> = BTreeMap::new();
    for cell in cells {
        if let Some(cell_obs) = observations.remove(&cell) {
            for (tmpl, count) in cell_obs {
                *merged.entry(tmpl).or_insert(0) += count;
            }
        }
    }
    merged
}
