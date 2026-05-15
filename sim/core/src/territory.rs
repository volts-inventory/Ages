//! Territory helpers. Pure functions: `target_cell_count`
//! sizes the BFS frontier from a civ's population; `compute_territory`
//! does the deterministic hex-grid sweep gated on habitability;
//! `pick_habitable_cell` walks outward from a candidate centroid
//! until it finds a claimable cell.

use sim_civ::Civ;
use sim_physics::HexGrid;
use std::collections::BTreeSet;

/// Founding-territory floor: minimum cells a civ claims at founding,
/// regardless of how much its capacity-driven `target_cell_count`
/// would otherwise return. A founding band whose `pop / per_cell_cap`
/// rounds to 1 still gets `FOUNDING_TERRITORY_FLOOR` cells so a fresh
/// civ has elbow room to grow before the per-cell logistic step locks
/// it into a single saturated cell — and so it doesn't immediately
/// trip `tiny_territory_streak` (≤ `TINY_TERRITORY_CELLS = 1`).
const FOUNDING_TERRITORY_FLOOR: usize = 4;

/// `target_cell_count(civ, max)` = `clamp(ceil(pop / per_cell_cap),
/// FOUNDING_TERRITORY_FLOOR, max)` where `per_cell_cap` is the civ's
/// approximate per-cell carrying capacity:
/// `carrying_capacity_per_unit × tech_multiplier × tool_capacity_multiplier`.
/// Reads the same per-civ scalars `cell_capacity` does (in
/// `sim/civ/src/capacity.rs`); skips the per-cell terms (fuel,
/// terrain habitability, seasonal) since territory sizing wants a
/// representative civ-wide cap, not a per-cell one.
///
/// Replaces the older flat `PEOPLE_PER_CELL = 5` constant — territory
/// ambition now matches the dynamic capacity the population step
/// actually evolves toward, so a high-cap (lush + low-grav + tech)
/// civ targets fewer cells per person than a low-cap (sparse + hi-grav)
/// one. Bounded below by `FOUNDING_TERRITORY_FLOOR` so founding
/// bands always start with elbow room, and above by `max` so high-pop
/// civs can't claim cells off the grid. Computed entirely from
/// Q32.32 raw bits so the result is deterministic across platforms.
pub(crate) fn target_cell_count(civ: &Civ, max: usize) -> usize {
    let approx_cap =
        civ.carrying_capacity_per_unit * civ.tech_multiplier * civ.tool_capacity_multiplier();
    // Cap floor: integer floor of approx_cap, clamped to ≥ 1 so we
    // never divide by zero. A degenerate civ with zero capacity per
    // unit (no fuel, no tech) just falls back to the founding floor.
    // `approx_cap` is `Real` (Q32.32, i64); promote to i128 so the
    // population arithmetic (which is `Pop`, i128) divides cleanly.
    let cap_bits: i128 = i128::from(approx_cap.raw().to_bits());
    let cap_floor: i128 = (cap_bits >> 32).max(1);
    // Q32.32/Q96.32: raw bits = value × 2^32, so bits >> 32 is
    // floor(value) for non-negative inputs. Population is non-negative
    // by construction (population dynamics clamp at zero).
    let pop_bits: i128 = civ.cohort.total().raw().to_bits();
    let pop_floor: i128 = (pop_bits >> 32).max(0);
    let n: i128 = (pop_floor + cap_floor - 1) / cap_floor;
    let n_usize = usize::try_from(n).unwrap_or(usize::MAX);
    n_usize.clamp(FOUNDING_TERRITORY_FLOOR.min(max.max(1)), max.max(1))
}

/// Per-cell predicate: is this cell claimable for this species'
/// habitat (with the civ's current tech state)?
///
/// The gate composition:
/// - Aquatic species: water cells (deep `≈`, shallow `~`, coast
///   `░`) are claimable by default. Land cells require
///   `ToolKind::AmphibiousConstruction`. Gas band stays a hard
///   wall.
/// - Terrestrial species: land cells (`▒`, `░`, `△`, `▲`, `·`)
///   above the habitability multiplier threshold are claimable. Water
///   cells require `ToolKind::AmphibiousConstruction`. Gas
///   stays a hard wall.
/// - Amphibious species: both domains; only the multiplier
///   threshold gates (deep ocean and gas excluded).
fn cell_claimable(
    cell: u32,
    state: &sim_physics::PhysicsState,
    planet: &sim_world::Planet,
    civ: &sim_civ::Civ,
    species_habitat: sim_species::Habitat,
) -> bool {
    let glyph = sim_world::terrain_glyph_at(state, planet, cell);
    // Gas band is always uninhabitable (no surface, no fluid
    // body) regardless of habitat or tech — it's the "no civ
    // could ever live here" floor.
    if glyph == '\u{2261}' {
        return false;
    }
    if !civ.can_claim_glyph(glyph, species_habitat) {
        return false;
    }
    // Universal multiplier check, but skip for aquatic
    // species on water cells: deep ocean has multiplier 0 in the
    // universal table because terrestrial civs can't live there;
    // aquatic civs need that band to BE their habitat. Same logic
    // for amphibious civs claiming deep water.
    let is_water = sim_world::is_water_glyph(glyph);
    let aquatic_or_amphibious = matches!(
        species_habitat,
        sim_species::Habitat::Aquatic | sim_species::Habitat::Amphibious
    );
    if is_water && aquatic_or_amphibious {
        return true;
    }
    let mult = sim_world::cell_habitability(state, planet, cell);
    sim_world::is_claimable_multiplier(mult)
}

/// Civ-free habitability check: would this cell be claimable for a
/// species with the given habitat, ignoring any tool unlocks (e.g.
/// `AmphibiousConstruction`)? Used by the emergent-founding path,
/// which picks a centroid *before* any civ exists — so the
/// civ-bearing `cell_claimable` can't apply. Gates on the same
/// glyph + habitat + multiplier composition as `cell_claimable`,
/// just without the civ tool layer.
///
/// Without this gate, `scan_for_emergence` can return a cell that
/// looked habitable when nomads first colonised it but has since
/// flooded (hydrology pushes water onto low coastal land over time)
/// or otherwise lost its terrain — and the centroid override in
/// `compute_territory` then force-claims the now-uninhabitable cell
/// at founding, leaving the civ with a cap-0 phantom claim that
/// drains its founding population until `prune_empty_cells` reclaims
/// it a hundred sim-years later.
pub(crate) fn is_habitat_claimable_at(
    state: &sim_physics::PhysicsState,
    planet: &sim_world::Planet,
    cell: u32,
    species_habitat: sim_species::Habitat,
) -> bool {
    let glyph = sim_world::terrain_glyph_at(state, planet, cell);
    if glyph == '\u{2261}' {
        return false;
    }
    let habitat_ok = match species_habitat {
        sim_species::Habitat::Aquatic => sim_world::is_water_glyph(glyph),
        sim_species::Habitat::Terrestrial | sim_species::Habitat::Airborne => {
            sim_world::is_land_glyph(glyph)
        }
        sim_species::Habitat::Amphibious => true,
    };
    if !habitat_ok {
        return false;
    }
    let aquatic_or_amphibious = matches!(
        species_habitat,
        sim_species::Habitat::Aquatic | sim_species::Habitat::Amphibious
    );
    if sim_world::is_water_glyph(glyph) && aquatic_or_amphibious {
        return true;
    }
    let mult = sim_world::cell_habitability(state, planet, cell);
    sim_world::is_claimable_multiplier(mult)
}

/// Deterministic BFS from `centroid`, returning the first `target`
/// cells visited in (depth, sorted-cell-id) order. Hex-grid
/// neighbours wrap on the torus. Sorting neighbours by cell id
/// at each step is what gives the sweep its bit-for-bit
/// determinism: same centroid + same target → same set, every run.
///
/// Cells whose terrain habitability multiplier is below
/// `is_claimable_multiplier` (deep ocean `≈`, gas band `≡`) are
/// treated as walls — never visited, never claimed. Peaks `▲`
/// stay claimable but contribute almost no capacity.
///
/// Aquatic civs are gated to water cells, terrestrial civs
/// to land cells (coast counts as both — transition zone).
/// Unlocking `ToolKind::AmphibiousConstruction` lifts the
/// restriction so the civ can BFS into the other domain.
///
/// `forbidden` cells are visited (so the BFS can BFS *through*
/// other civs' land en route to a frontier) but never claimed
/// (so a breakaway's territory stays strictly its own, no
/// multi-claim disputes with the parent at founding). Pass an
/// empty set for paths where overlap can't happen by construction
/// (emergent first civ, stateless refound after total collapse).
///
/// The capital (`centroid`) is forced into the result regardless
/// of its terrain or `forbidden` membership so a degenerate input
/// can still expand into an empty claim set rather than panicking;
/// sim/core's founding pipeline runs `pick_habitable_cell` /
/// the breakaway's seized-cell centroid path so the centroid is
/// virtually always claimable + unclaimed in production.
pub(crate) fn compute_territory(
    centroid: u32,
    target: usize,
    grid: &HexGrid,
    state: &sim_physics::PhysicsState,
    planet: &sim_world::Planet,
    civ: &sim_civ::Civ,
    species_habitat: sim_species::Habitat,
    forbidden: &BTreeSet<u32>,
) -> BTreeSet<u32> {
    let mut out = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut queue: std::collections::VecDeque<u32> = std::collections::VecDeque::new();
    queue.push_back(centroid);
    visited.insert(centroid);
    while let Some(cell) = queue.pop_front() {
        if out.len() >= target {
            break;
        }
        let claimable = cell_claimable(cell, state, planet, civ, species_habitat);
        let claimable_now = (claimable && !forbidden.contains(&cell)) || cell == centroid;
        if claimable_now {
            out.insert(cell);
        }
        // Expansion gate: walk through claimable cells (whether
        // forbidden or not — a breakaway can BFS *across* the
        // parent's land to reach the frontier beyond) and the
        // centroid override. Unclaimable cells (deep ocean, gas)
        // are walls — never visited, never expanded through.
        if claimable || cell == centroid {
            let axial = grid.axial_of(sim_physics::CellId(cell));
            let mut nbs: Vec<u32> = grid.neighbours(axial).iter().map(|c| c.0).collect();
            nbs.sort_unstable();
            for n in nbs {
                if visited.insert(n) {
                    queue.push_back(n);
                }
            }
        }
    }
    out
}

/// Deterministic search for a habitable cell starting from
/// `seed`. Walks outward through the hex BFS frontier in
/// canonical (depth, cell-id) order until it finds a cell whose
/// habitability multiplier passes the claim threshold AND
/// whose terrain matches the species' native habitat.
/// Returns `seed` itself if already habitable, otherwise the
/// first claimable cell encountered. Falls back to `seed` if no
/// cell in the grid is habitable (gas-giant / open-ocean planets)
/// so callers don't need to handle a "no land at all" panic — the
/// civ will still get the BFS-empty claim and either fail the
/// founding floor or collapse on `TerritoryTooSmall`.
pub(crate) fn pick_habitable_cell(
    seed: u32,
    grid: &HexGrid,
    state: &sim_physics::PhysicsState,
    planet: &sim_world::Planet,
    civ: &sim_civ::Civ,
    species_habitat: sim_species::Habitat,
) -> u32 {
    let claimable =
        |cell: u32| -> bool { cell_claimable(cell, state, planet, civ, species_habitat) };
    if claimable(seed) {
        return seed;
    }
    let mut visited = BTreeSet::new();
    let mut queue: std::collections::VecDeque<u32> = std::collections::VecDeque::new();
    queue.push_back(seed);
    visited.insert(seed);
    while let Some(cell) = queue.pop_front() {
        if cell != seed && claimable(cell) {
            return cell;
        }
        let axial = grid.axial_of(sim_physics::CellId(cell));
        let mut nbs: Vec<u32> = grid.neighbours(axial).iter().map(|c| c.0).collect();
        nbs.sort_unstable();
        for n in nbs {
            if visited.insert(n) {
                queue.push_back(n);
            }
        }
    }
    seed
}

/// Per-habitat habitability score for cell selection. For
/// terrestrial species the standard multiplier applies (coast
/// 1.20, plain 1.00, inland 0.90, hill 0.60, peak 0.10). For
/// aquatic species the band gets re-keyed: deep ocean 1.00 (their
/// natural deep-water home), shallow 0.80 (less productive), coast
/// 1.20 (richest — fish + tidepool feeding), land scores 0 (their
/// claim gate filtered it out anyway). Amphibious species land in
/// between (coast 1.20, land 1.00, deep water 0.80).
fn score_for_habitat(glyph: char, habitat: sim_species::Habitat) -> sim_arith::Real {
    use sim_arith::Real;
    use sim_species::Habitat;
    match habitat {
        // Airborne lives on land like terrestrial (flight is for
        // wrong-biome transit, not preferred habitat) so both pull
        // straight from the per-glyph habitability table.
        Habitat::Terrestrial | Habitat::Airborne => sim_world::habitability_multiplier(glyph),
        Habitat::Aquatic => match glyph {
            '\u{2248}' => Real::ONE,          // ≈ deep ocean — the deep-water home
            '~' => Real::from_ratio(80, 100), // ~ shallow sea
            '\u{2591}' => Real::from_ratio(120, 100), // ░ coast — richest (tidal feeding)
            _ => Real::ZERO,
        },
        Habitat::Amphibious => match glyph {
            '\u{2591}' => Real::from_ratio(120, 100), // ░ coast — best of both
            '\u{2592}' | '\u{00B7}' => Real::ONE,     // ▒ inland / · plain
            '~' | '\u{2248}' => Real::from_ratio(80, 100), // ~ shallow / ≈ deep
            '\u{25B3}' => Real::from_ratio(60, 100),  // △ hill
            '\u{25B2}' => Real::from_ratio(10, 100),  // ▲ peak
            _ => Real::ZERO,
        },
    }
}

/// Pick a habitable cell as far as possible from a set of
/// `occupied` cells. Used for successor / breakaway founding so
/// the new civ spawns on a different continent (or at least far
/// from its parent's territory) — the Sumatra-vs-China pattern
/// rather than the cell-next-door pattern earlier code produced.
///
/// Algorithm: for every claimable cell, compute the minimum
/// hex-axial distance to any cell in `occupied` (∞ if the
/// occupied set is empty). Pick the cell with the largest
/// min-distance (max-min); break ties by smallest cell id.
/// Falls back to `fallback` if no cell is claimable for this
/// species.
pub(crate) fn pick_distant_habitable_cell(
    fallback: u32,
    grid: &HexGrid,
    state: &sim_physics::PhysicsState,
    planet: &sim_world::Planet,
    civ: &sim_civ::Civ,
    species_habitat: sim_species::Habitat,
    occupied: &BTreeSet<u32>,
) -> u32 {
    let n = grid.n_cells();
    let mut best: Option<(i64, sim_arith::Real, u32)> = None;
    for cell in 0..n {
        let cell = u32::try_from(cell).unwrap_or(u32::MAX);
        if occupied.contains(&cell) {
            continue;
        }
        if !cell_claimable(cell, state, planet, civ, species_habitat) {
            continue;
        }
        let cell_axial = grid.axial_of(sim_physics::CellId(cell));
        let min_dist = if occupied.is_empty() {
            i64::MAX
        } else {
            occupied
                .iter()
                .map(|&o| {
                    let oa = grid.axial_of(sim_physics::CellId(o));
                    i64::from((cell_axial.q - oa.q).abs() + (cell_axial.r - oa.r).abs())
                })
                .min()
                .unwrap_or(i64::MAX)
        };
        let glyph = sim_world::terrain_glyph_at(state, planet, cell);
        let score = score_for_habitat(glyph, species_habitat);
        // Sort key: (max distance from occupied, then habitability,
        // then smallest cell id). Smaller cell id wins ties so
        // the result is deterministic.
        let key = (min_dist, score, cell);
        let better = match &best {
            None => true,
            Some((bd, bs, _)) => key.0 > *bd || (key.0 == *bd && key.1 > *bs),
        };
        if better {
            best = Some((key.0, key.1, cell));
        }
    }
    best.map_or(fallback, |(_, _, c)| c)
}
