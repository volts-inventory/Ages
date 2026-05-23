//! Initial origin-cell seeding for the nomadic species pool.
//!
//! At run start we concentrate the entire `INITIAL_NOMAD_TOTAL`
//! into a handful of well-connected high-habitability cells (the
//! "out of Africa" pattern) and let [`super::growth::step_growth`]'s
//! diffusion spread the species outward over the first few hundred
//! ticks. Earlier the pool was distributed across every habitable
//! cell weighted by habitability — every cell glowed `0` from tick
//! 0 (panspecies). Concentrating at origins gives the species a
//! visible radiation arc and lets emergence pick a coherent focal
//! region rather than a flat field.

use super::{cell_weight, is_habitat_match};
use sim_arith::Real;
use sim_species::Habitat;
use std::collections::BTreeMap;

/// Initial nomad pool: total people in the species' nomadic
/// presence at run start, split evenly across
/// `NOMAD_ORIGIN_CELL_COUNT` origin cells. Sized so each origin
/// starts at roughly its per-cell carrying capacity
/// (`NOMAD_PER_CELL_CAP × habitability ≈ 80 × 1`) rather than far
/// over it — earlier the total was 600 (= 200/origin, ~2.5× cap),
/// which forced origins to bleed people to all 6 neighbours every
/// tick from tick 0 as a transit flood. With the new total, origins
/// start at-cap and spreading is driven by genuine logistic growth
/// and density-gradient diffusion at the calibrated rates rather
/// than by a one-time over-population overflow.
pub(crate) const INITIAL_NOMAD_TOTAL: i64 = 240;

/// Origin-cell count: how many *origin* cells receive the
/// initial population. The species emerges from a small focal
/// region (an "out of Africa" pattern) and radiates outward via
/// diffusion. Pinned at 3 — single-origin would strand the
/// species when the most-habitable cell sits on a peninsula or
/// otherwise has no habitat-matching neighbours (e.g. a single
/// land cell surrounded by ocean for a terrestrial species).
/// Three origins with min-separation ≥ 4 cells gives one for
/// redundancy plus two for parallel-evolution flavour without
/// producing the uniform-everywhere look the earlier init had.
pub(crate) const NOMAD_ORIGIN_CELL_COUNT: usize = 3;

/// Seed the initial nomad pool at a small number of
/// **origin cells** (`NOMAD_ORIGIN_CELL_COUNT`). Earlier the
/// initial pool was distributed across every habitable cell
/// weighted by habitability — every cell glowed `0` from tick 0
/// (panspecies). Now we concentrate the population in the
/// most-habitable cell(s) and let `step_growth`'s diffusion
/// term spread the species outward over the first few hundred
/// ticks (out of Africa). `INITIAL_NOMAD_TOTAL` is sized so each
/// origin starts at roughly per-cell cap, so spreading is driven
/// by genuine logistic growth + diffusion rather than by an
/// initial over-population overflow.
///
/// Origin selection: the cell with the highest habitability
/// multiplier (ties broken by lowest cell id, deterministic).
/// When `NOMAD_ORIGIN_CELL_COUNT > 1`, the next-best cells are
/// taken in habitability-descending order with a minimum
/// separation matching the existing Poisson-disc rule (≥ 4
/// axial cells from any prior origin) so origins don't bunch up.
///
/// Aquatic species treat deep ocean (habitability multiplier = 0) as
/// fully habitable so they can originate offshore.
pub(crate) fn init_pops(
    state: &sim_physics::PhysicsState,
    planet: &sim_world::Planet,
    species_habitat: Habitat,
    claimed_cells: &std::collections::BTreeSet<u32>,
) -> BTreeMap<u32, Real> {
    let n = state.grid().n_cells();
    let grid = state.grid();
    // Score each candidate cell by `weight × (1 + matching_neighbours)`
    // where matching_neighbours counts its 6 hex neighbours that
    // pass the same habitat-match + non-zero-weight filter.
    // Earlier attempts scored by `weight` alone, which sometimes
    // picked a peninsula tip (most habitable but stranded with
    // no matching neighbours — diffusion couldn't propagate).
    // Multiplying by neighbour count ensures the chosen origin is
    // well-connected enough to seed an expanding population.
    let scoring_weight = |cell: u32| -> Real {
        if !is_habitat_match(state, planet, cell, species_habitat) {
            return Real::ZERO;
        }
        cell_weight(state, planet, cell, species_habitat)
    };
    let mut candidates: Vec<(u32, Real)> = Vec::new();
    for cell in 0..n {
        let cell = u32::try_from(cell).unwrap_or(u32::MAX);
        if claimed_cells.contains(&cell) {
            continue;
        }
        let weight = scoring_weight(cell);
        if weight <= Real::ZERO {
            continue;
        }
        let neighbour_count = i64::try_from(
            grid.neighbours(grid.axial_of(sim_physics::CellId(cell)))
                .iter()
                .filter(|c| scoring_weight(c.0) > Real::ZERO && !claimed_cells.contains(&c.0))
                .count(),
        )
        .unwrap_or(i64::MAX);
        let connectivity_bonus = Real::ONE + Real::from_int(neighbour_count);
        candidates.push((cell, weight * connectivity_bonus));
    }
    let mut pops = BTreeMap::new();
    if candidates.is_empty() {
        return pops;
    }
    // Sort by score descending; deterministic tie-break by cell
    // id ascending.
    candidates.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });

    // Pick up to NOMAD_ORIGIN_CELL_COUNT origins with min-distance
    // separation. Distance: axial-grid Manhattan distance.
    let min_separation: i32 = 4;
    let mut origins: Vec<u32> = Vec::with_capacity(NOMAD_ORIGIN_CELL_COUNT);
    for (cell, _) in &candidates {
        if origins.len() >= NOMAD_ORIGIN_CELL_COUNT {
            break;
        }
        let candidate_axial = grid.axial_of(sim_physics::CellId(*cell));
        let too_close = origins.iter().any(|o| {
            let other_axial = grid.axial_of(sim_physics::CellId(*o));
            let dq = (candidate_axial.q - other_axial.q).abs();
            let dr = (candidate_axial.r - other_axial.r).abs();
            dq.max(dr) < min_separation
        });
        if !too_close {
            origins.push(*cell);
        }
    }
    if origins.is_empty() {
        return pops;
    }
    // Distribute the entire INITIAL_NOMAD_TOTAL evenly across the
    // chosen origins. Floor-divided integer share to keep Q32.32
    // arithmetic exact.
    let per_origin = Real::from_int(INITIAL_NOMAD_TOTAL)
        / Real::from_int(i64::try_from(origins.len()).unwrap_or(1));
    for cell in origins {
        pops.insert(cell, per_origin);
    }
    pops
}
