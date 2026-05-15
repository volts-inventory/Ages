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

use sim_arith::Real;
use sim_species::Habitat;
use sim_world::{is_land_glyph, is_water_glyph, terrain_glyph_at};
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

/// Per-cell carrying-capacity ceiling for nomads. Multiplied
/// by the cell's habitability multiplier and capped here. A later tuning
/// bumped from 8 → 80 so densely-populated cells reach the
/// emergent-civ founding threshold over a few hundred ticks of
/// growth — the "Sumatra grew until it became a civilisation"
/// dynamic.
pub(crate) const NOMAD_PER_CELL_CAP: i64 = 80;

/// Logistic growth coefficient. Per tick, a cell's
/// nomad pop moves this fraction of the gap toward its cap.
/// 1/200 = 0.5% per tick → ~6%/yr compounded → cells reach 99%
/// of cap after ~920 ticks. The rate originally bumped 1/200 → 1/100
/// to outpace the unconditional non-habitat decay that drained
/// the species before civ-founding density was reached. With
/// strict-block tier-0 transit (no decay-leak from pre-tech
/// species) and per-cell tech bonuses (`thermal_gradient` +10%,
/// `seasonal_thaw` +10%), the rate has been re-halved back to
/// 1/200 for more realistic pre-industrial pacing — pre-tech
/// cells crawl at hunter-gatherer pace; tech-rich cells reach
/// near-current speeds via the bonuses.
pub(crate) const NOMAD_GROWTH_NUM: i64 = 1;
pub(crate) const NOMAD_GROWTH_DEN: i64 = 200;

/// Emergent founding **minimum population** floor: a cell
/// can't coalesce into a civilisation unless its nomadic pop is
/// at least this many people. Combined with the tech
/// threshold below — a cell with high tech but only 5 people
/// is still a hunter-gatherer band, not a civilisation.
///
/// Acts as the absolute floor on substrate-poor seeds where
/// `NOMAD_PER_CELL_CAP × habitability` may itself be below 20.
/// On normal seeds the relative saturation gate
/// (`EMERGENT_PRESSURE_NUM/DEN`) dominates.
pub(crate) const EMERGENT_FOUNDING_POP: i64 = 20;

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

// We removed `NOMAD_TECH_PER_FIRING_*`: tech is now derived
// from per-template observation counts × cognition × sociality
// in `tech_score()`, not pre-multiplied per firing.

/// Cooldown between successive emergent foundings. Prevents
/// every cell on the planet from coalescing into a civ on the
/// same tick once growth equalises across the map. 600 ticks
/// (~50 sim-years) so foundings stay once-per-generation events
/// rather than the pre-tightening cadence of one every 4 years
/// (which produced 7-11 concurrent civs on small-grid seeds —
/// well above the historical Iron-Age cap of ~5 concurrent
/// civilisations on Earth).
pub(crate) const EMERGENT_FOUNDING_COOLDOWN_TICKS: u64 = 600;

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

/// Density-gradient diffusion rate at the baseline lifespan. The
/// fraction of the pop *gap* between source and neighbour that
/// transfers each tick. At 1/100 = 1% gradient closure per tick
/// for an 80-yr species:
///
/// - Cells at cap don't shed to cells at cap (no gradient).
/// - Cells well above cap (transit) shed strongly to lower-pop
///   neighbours.
/// - Cells well below cap receive from higher-pop neighbours.
///
/// This is *real* density-equalising flow rather than
/// constant-fraction diffusion — populations naturally
/// concentrate in habitat-rich cells, overflow to neighbours
/// when they hit cap, and migrate from over-pressured to
/// under-pressured cells.
///
/// The baseline rate is rescaled per-species via
/// `lifespan_diffusion_scale` — a species' generational turnover
/// drives band-fission / emigration cadence. Short-lived
/// r-strategists diffuse faster (more generations per sim-year =
/// more fission events); long-lived K-strategists diffuse slower
/// (a 200-yr species reorganises its range across centuries, not
/// decades).
pub(crate) const NOMAD_DIFFUSION_NUM: i64 = 1;
pub(crate) const NOMAD_DIFFUSION_DEN: i64 = 100;

/// Baseline lifespan (years) used for the diffusion rescale. A
/// species at this lifespan diffuses at `NOMAD_DIFFUSION_NUM/DEN`
/// per tick exactly; shorter lifespans scale up, longer scale
/// down. Anchored at 80 to match the same Earth-vertebrate
/// reference implicit in the rest of the demographics tuning
/// (e.g. the elder-bracket survival baselines in
/// `PopulationDynamics::for_species`).
pub(crate) const NOMAD_DIFFUSION_BASELINE_LIFESPAN_YEARS: i64 = 80;

/// Tech-gated transit thresholds (in `tech_score` units, the same
/// per-cell observation pressure that gates civ founding via
/// `EMERGENT_FOUNDING_TECH = 50`). A species' max per-cell tech
/// across its habitat cells determines its global wrong-biome
/// transit competence. Combined with the habitat's innate base
/// tier (Airborne = 1, others = 0), this maps to a single
/// `species_tier ∈ {0, 1, 2, 3}` that governs:
///
/// - whether wrong-biome diffusion is allowed at all (tier ≥ 1),
/// - the per-tick decay rate of pop already in wrong-biome cells
///   (lower decay → longer crossings).
///
/// Tiers correspond to archaeological dispersal stages:
/// - 0 = no boats / no flight → strict habitat confinement
/// - 1 = log-floats / coastal rafts → 1-cell crossings
/// - 2 = sailing rafts / Wallacea-style island chains → 2-3 cells
/// - 3 = open-ocean navigation → many-cell crossings
pub(crate) const TRANSIT_TIER_1_TECH: i64 = 10;
pub(crate) const TRANSIT_TIER_2_TECH: i64 = 25;
pub(crate) const TRANSIT_TIER_3_TECH: i64 = 50;

/// Per-template growth-effect thresholds. A cell that has
/// accumulated this many observations of the named template
/// unlocks the corresponding cap or growth-rate bonus in
/// `step_growth`. Substrate-agnostic by construction — these
/// templates name *physical mechanisms* whose effects translate
/// across metabolisms (water for aqueous life, methane for
/// hydrocarbon life, ammonia for ammoniacal life, etc.).
///
/// Effects are additive; a cell with multiple unlocked templates
/// stacks all bonuses. The numeric thresholds are tuned so a
/// phenomenon-rich cell unlocks the corresponding tech in the
/// first few hundred ticks of being populated, not the first
/// few thousand.
pub(crate) const GROWTH_FIRE_TEMPLATE_ID: u32 = 1;
pub(crate) const GROWTH_FIRE_THRESHOLD: u64 = 20;
pub(crate) const GROWTH_THERMAL_TEMPLATE_ID: u32 = 7;
pub(crate) const GROWTH_THERMAL_THRESHOLD: u64 = 15;
pub(crate) const GROWTH_FERTILE_TEMPLATE_ID: u32 = 10;
pub(crate) const GROWTH_FERTILE_THRESHOLD: u64 = 15;
pub(crate) const GROWTH_SEASONAL_TEMPLATE_ID: u32 = 25;
pub(crate) const GROWTH_SEASONAL_THRESHOLD: u64 = 10;
pub(crate) const GROWTH_SOLVENT_TEMPLATE_ID: u32 = 36;
pub(crate) const GROWTH_SOLVENT_THRESHOLD: u64 = 10;

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
    let cell_weight = |cell: u32| -> Real {
        if !is_habitat_match(state, planet, cell, species_habitat) {
            return Real::ZERO;
        }
        let mult = sim_world::cell_habitability(state, planet, cell);
        if matches!(species_habitat, Habitat::Aquatic) && mult == Real::ZERO {
            Real::ONE
        } else {
            mult
        }
    };
    let mut candidates: Vec<(u32, Real)> = Vec::new();
    for cell in 0..n {
        let cell = u32::try_from(cell).unwrap_or(u32::MAX);
        if claimed_cells.contains(&cell) {
            continue;
        }
        let weight = cell_weight(cell);
        if weight <= Real::ZERO {
            continue;
        }
        let neighbour_count = i64::try_from(
            grid.neighbours(grid.axial_of(sim_physics::CellId(cell)))
                .iter()
                .filter(|c| cell_weight(c.0) > Real::ZERO && !claimed_cells.contains(&c.0))
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

/// Per-tick growth + diffusion. Replaces the earlier
/// "every habitable cell auto-grows toward cap" behaviour (which
/// spawned nomads in cells that had never been populated).
///
/// Two phases per tick:
///
/// 1. **Logistic growth on populated cells.** Each cell with a
///    non-trivial population grows toward its per-cell cap:
///    `next = cur + (cap - cur) × NOMAD_GROWTH`. Empty cells stay
///    empty.
/// 2. **Neighbour diffusion.** Each populated cell sheds
///    `NOMAD_DIFFUSION_NUM / NOMAD_DIFFUSION_DEN` of its
///    population to each habitable neighbour. The neighbour
///    receives the share; the source cell loses it. Diffusion
///    capped so a cell can't lose more than half its pop in a
///    single tick (regardless of neighbour count).
///
/// Net result: nomads radiate outward from the origin cell(s)
/// over the first few hundred ticks, mimicking the way
/// populations spread across continents in real biology rather
/// than spontaneously appearing everywhere from tick 0.
///
/// Cells claimed by civs stay untouched (civ-claim absorption is
/// `absorb_into_civ`'s job).
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(crate) fn step_growth(
    pops: &mut BTreeMap<u32, Real>,
    state: &sim_physics::PhysicsState,
    planet: &sim_world::Planet,
    species_habitat: Habitat,
    observations: &BTreeMap<u32, BTreeMap<u32, u64>>,
    cognition: Real,
    sociality: Real,
    lifespan_years: Real,
    claimed_cells: &std::collections::BTreeSet<u32>,
) {
    let growth = Real::from_ratio(NOMAD_GROWTH_NUM, NOMAD_GROWTH_DEN);
    let diffuse = Real::from_ratio(NOMAD_DIFFUSION_NUM, NOMAD_DIFFUSION_DEN)
        * lifespan_diffusion_scale(lifespan_years);
    let species_t = species_tier(pops, observations, species_habitat, cognition, sociality);
    let decay = decay_for_tier(species_t);
    let cap_base = Real::from_int(NOMAD_PER_CELL_CAP);
    let grid = state.grid();

    // Helper: per-cell carrying capacity. Returns cap in people.
    // Zero for wrong-habitat (where the species can transit but
    // not establish), zero for claimed cells (civs handle their
    // own pop dynamics), zero for gas band.
    let cell_cap = |cell: u32| -> Real {
        if claimed_cells.contains(&cell) {
            return Real::ZERO;
        }
        let glyph = sim_world::terrain_glyph_at(state, planet, cell);
        if glyph == '\u{2261}' {
            return Real::ZERO;
        }
        if !is_habitat_match(state, planet, cell, species_habitat) {
            return Real::ZERO;
        }
        let mult = sim_world::cell_habitability(state, planet, cell);
        let weight = if matches!(species_habitat, Habitat::Aquatic) && mult == Real::ZERO {
            Real::ONE
        } else {
            mult
        };
        let cap_bonus = cell_cap_bonus(observations.get(&cell));
        cap_base * weight * (Real::ONE + cap_bonus)
    };

    // Phase 1 — logistic growth in habitat cells, decay in
    // non-habitat cells. The two cases are partitioned by cap:
    //   cap > 0: pop grows toward cap at logistic rate
    //   cap = 0: pop decays at NOMAD_NON_HABITAT_DECAY rate
    let populated: Vec<u32> = pops.keys().copied().collect();
    for cell in &populated {
        let cap = cell_cap(*cell);
        let cur = pops.get(cell).copied().unwrap_or(Real::ZERO);
        let next = if cap > Real::ZERO {
            if cur >= cap {
                cur
            } else {
                let growth_bonus = cell_growth_bonus(observations.get(cell));
                let cell_growth = growth * (Real::ONE + growth_bonus);
                cur + (cap - cur) * cell_growth
            }
        } else {
            // Non-habitat / claimed / gas: decay.
            cur * (Real::ONE - decay)
        };
        if next > Real::from_ratio(1, 10) {
            pops.insert(*cell, next);
        } else {
            pops.remove(cell);
        }
    }

    // Phase 2 — density-gradient diffusion. Pop flows from
    // higher-density cells to lower-density. "Density" is
    // measured as raw population (not pop/cap) so a cell at cap
    // 80 with 80 people is *full* and won't push to a neighbour
    // at cap 80 with 80 people (zero gradient), but *will* push
    // to a neighbour at cap 80 with 0 people (full gradient).
    //
    // The transfer per neighbour-pair is `(src - neigh) ×
    // diffuse / 2` — half because the pair sees the same
    // gradient from the other side; splitting prevents
    // double-counting when both ends iterate.
    // Habitat-aware diffusion: a populated cell prefers habitat-
    // matching neighbours. If any habitat-matching neighbour
    // exists, ONLY they receive — populations don't voluntarily
    // migrate into inhospitable terrain when better land is
    // adjacent. If NO habitat-matching neighbour exists (the
    // source is on a peninsula tip / island surrounded by
    // unsuitable cells), the population transits through to
    // non-habitat cells, hoping to reach the next habitat-
    // matching cell beyond.
    let mut transfers: BTreeMap<u32, Real> = BTreeMap::new();
    let snapshot: Vec<(u32, Real)> = pops.iter().map(|(c, p)| (*c, *p)).collect();
    let half = Real::from_ratio(1, 2);
    let neighbour_passable = |n_cell: u32| -> bool {
        if claimed_cells.contains(&n_cell) {
            return false;
        }
        let glyph = sim_world::terrain_glyph_at(state, planet, n_cell);
        glyph != '\u{2261}'
    };
    for (source, source_pop) in &snapshot {
        if *source_pop <= Real::from_ratio(1, 10) {
            continue;
        }
        let source_axial = grid.axial_of(sim_physics::CellId(*source));
        let raw_neighbours: Vec<u32> = grid
            .neighbours(source_axial)
            .iter()
            .map(|c| c.0)
            .filter(|n| neighbour_passable(*n))
            .collect();
        // Partition neighbours: habitat-matching first, then
        // transit-only.
        let (habitat_neighbours, transit_neighbours): (Vec<u32>, Vec<u32>) =
            raw_neighbours.iter().partition(|n| {
                is_habitat_match(state, planet, **n, species_habitat) && cell_cap(**n) > Real::ZERO
            });
        // Use habitat-matching neighbours if any exist. Fall back
        // to transit (wrong-biome) neighbours only when the
        // species has tech-tier ≥ 1 (boats / flight / equivalent);
        // a tier-0 species is strictly habitat-confined and cannot
        // push pop into wrong biome at all.
        let target_neighbours = if !habitat_neighbours.is_empty() {
            habitat_neighbours
        } else if species_t >= 1 {
            transit_neighbours
        } else {
            Vec::new()
        };
        // First pass: compute the candidate per-neighbour amounts.
        // Second pass: enforce the half-pop-per-tick cap on the
        // sum; if the source would shed more than half its pop
        // across all neighbours combined this tick, scale every
        // amount down proportionally so the totals respect the
        // documented contract (cell can't lose more than half its
        // pop in a single tick regardless of neighbour count).
        let mut candidates: Vec<(u32, Real)> = Vec::with_capacity(6);
        let mut total_out = Real::ZERO;
        for n_cell in target_neighbours {
            let neigh_pop = pops.get(&n_cell).copied().unwrap_or(Real::ZERO);
            if *source_pop <= neigh_pop {
                continue;
            }
            let gap = *source_pop - neigh_pop;
            let amount = gap * diffuse * half;
            total_out = total_out + amount;
            candidates.push((n_cell, amount));
        }
        let cap_out = *source_pop * half;
        let scale = if total_out > cap_out && total_out > Real::ZERO {
            cap_out / total_out
        } else {
            Real::ONE
        };
        for (n_cell, amount) in candidates {
            let scaled = amount * scale;
            *transfers.entry(*source).or_insert(Real::ZERO) =
                *transfers.entry(*source).or_insert(Real::ZERO) - scaled;
            *transfers.entry(n_cell).or_insert(Real::ZERO) =
                *transfers.entry(n_cell).or_insert(Real::ZERO) + scaled;
        }
    }
    for (cell, delta) in transfers {
        let cur = pops.get(&cell).copied().unwrap_or(Real::ZERO);
        let next = (cur + delta).max(Real::ZERO);
        if next > Real::from_ratio(1, 10) {
            pops.insert(cell, next);
        } else {
            pops.remove(&cell);
        }
    }
}

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
pub(crate) const AMBIENT_NOMAD_SEED_POP: i64 = 8;

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

/// Absorb every nomad on the listed cells into the civ's cohort.
/// Runs at civ founding *and* after each `claim_cells` call so
/// territory expansion converts nomads into civ population. Returns
/// the absorbed total (tests use this; callers may ignore).
pub(crate) fn absorb_into_civ(
    pops: &mut BTreeMap<u32, Real>,
    civ: &mut sim_civ::Civ,
    cells: impl IntoIterator<Item = u32>,
    biology: &sim_species::PopulationBiology,
) -> Real {
    let mut total = Real::ZERO;
    // Deposit the absorbed nomadic pop into the per-cell
    // `region_cohorts` for the gained cell, distributed across
    // the four age brackets per the species's biology fractions.
    // Nomadic groups are mixed-age — they don't all magically
    // become fertile adults on civ contact.
    for cell in cells {
        if let Some(p) = pops.remove(&cell) {
            total = total + p;
            let p_pop = sim_arith::Pop::from_real(p);
            if let Some(cohort) = civ.region_cohorts.get_mut(&cell) {
                cohort.deposit_distributed(p_pop, biology);
            } else {
                // Cell isn't (yet) in `region_cohorts` — shouldn't
                // happen since `absorb_into_civ` is called on
                // gained cells right after `claim_cells`/
                // `expand_via_overflow` seed them, but be safe.
                let mut c = sim_civ::Cohort::empty_with_civ(civ.id);
                c.deposit_distributed(p_pop, biology);
                civ.region_cohorts.insert(cell, c);
            }
        }
    }
    if total > Real::ZERO {
        civ.cohort.deposit_distributed(sim_arith::Pop::from_real(total), biology);
    }
    total
}

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

/// Derive a scalar tech score from a cell's per-template
/// observation counts. Score = `cognition × sociality × Σ counts`.
/// Cells with rich phenomena observed many times score high; a
/// barren cell observed once or twice doesn't. Used for
/// emergence threshold checks; same shape as the legacy scalar but
/// derived from the per-template counts so the threshold and the
/// civ's inherited knowledge are consistent.
fn tech_score(
    cell_observations: Option<&BTreeMap<u32, u64>>,
    cognition: Real,
    sociality: Real,
) -> Real {
    let total: u64 = cell_observations.map_or(0, |m| m.values().copied().sum());
    if total == 0 {
        return Real::ZERO;
    }
    cognition * sociality * Real::from_int(i64::try_from(total).unwrap_or(i64::MAX))
}

/// Per-cell carrying-capacity bonus from accumulated tech-relevant
/// observations. Returns an additive fraction (0.25 = +25%); the
/// caller multiplies cap by `(1 + bonus)`. Substrate-agnostic
/// templates: fire (cooking expands edible food across whatever
/// chemistry the planet supports), `fertile_land` (proto-agriculture
/// — substrate-rich soil regardless of substrate), `solvent_humid_band`
/// (reliable liquid solvent for the species' metabolism — water,
/// methane, or ammonia depending on the metabolic substrate).
fn cell_cap_bonus(cell_observations: Option<&BTreeMap<u32, u64>>) -> Real {
    let Some(obs) = cell_observations else {
        return Real::ZERO;
    };
    let mut bonus = Real::ZERO;
    if obs.get(&GROWTH_FIRE_TEMPLATE_ID).copied().unwrap_or(0) >= GROWTH_FIRE_THRESHOLD {
        bonus = bonus + Real::from_ratio(10, 100);
    }
    if obs.get(&GROWTH_FERTILE_TEMPLATE_ID).copied().unwrap_or(0)
        >= GROWTH_FERTILE_THRESHOLD
    {
        bonus = bonus + Real::from_ratio(25, 100);
    }
    if obs.get(&GROWTH_SOLVENT_TEMPLATE_ID).copied().unwrap_or(0)
        >= GROWTH_SOLVENT_THRESHOLD
    {
        bonus = bonus + Real::from_ratio(10, 100);
    }
    bonus
}

/// Per-cell logistic-growth-rate bonus from accumulated tech-
/// relevant observations. Returns an additive fraction (0.10 =
/// +10%); the caller multiplies the base growth rate by
/// `(1 + bonus)`. Substrate-agnostic templates: `thermal_gradient`
/// (climate-aware shelter reduces environmental mortality
/// regardless of which extremes the planet has), `seasonal_thaw`
/// (cyclical-resource awareness lets the species plan around
/// substrate-thaw / freeze cycles whatever they involve).
fn cell_growth_bonus(cell_observations: Option<&BTreeMap<u32, u64>>) -> Real {
    let Some(obs) = cell_observations else {
        return Real::ZERO;
    };
    let mut bonus = Real::ZERO;
    if obs.get(&GROWTH_THERMAL_TEMPLATE_ID).copied().unwrap_or(0)
        >= GROWTH_THERMAL_THRESHOLD
    {
        bonus = bonus + Real::from_ratio(10, 100);
    }
    if obs.get(&GROWTH_SEASONAL_TEMPLATE_ID).copied().unwrap_or(0)
        >= GROWTH_SEASONAL_THRESHOLD
    {
        bonus = bonus + Real::from_ratio(10, 100);
    }
    bonus
}

/// Wrong-biome transit tier from a tech score (the per-cell or
/// max-cell observation pressure × cognition × sociality). Maps
/// to the dispersal-stage thresholds in `TRANSIT_TIER_*_TECH`.
#[allow(clippy::bool_to_int_with_if)]
fn tech_tier_from_score(score: Real) -> u32 {
    if score >= Real::from_int(TRANSIT_TIER_3_TECH) {
        3
    } else if score >= Real::from_int(TRANSIT_TIER_2_TECH) {
        2
    } else if score >= Real::from_int(TRANSIT_TIER_1_TECH) {
        1
    } else {
        0
    }
}

/// Habitat's innate (no-tech) wrong-biome transit ability. Airborne
/// species fly natively across 1 wrong-biome cell even at zero
/// tech; everyone else must learn transit before any crossing.
fn habitat_base_tier(habitat: Habitat) -> u32 {
    match habitat {
        Habitat::Airborne => 1,
        Habitat::Aquatic | Habitat::Terrestrial | Habitat::Amphibious => 0,
    }
}

/// Species-wide wrong-biome transit tier. Combines the habitat's
/// innate base tier (flight = +1) with the maximum tech score
/// across populated habitat cells. Used by `step_growth` to gate
/// wrong-biome diffusion and scale transit decay.
fn species_tier(
    pops: &BTreeMap<u32, Real>,
    observations: &BTreeMap<u32, BTreeMap<u32, u64>>,
    habitat: Habitat,
    cognition: Real,
    sociality: Real,
) -> u32 {
    let mut max_score = Real::ZERO;
    for cell in pops.keys() {
        let s = tech_score(observations.get(cell), cognition, sociality);
        if s > max_score {
            max_score = s;
        }
    }
    habitat_base_tier(habitat).saturating_add(tech_tier_from_score(max_score))
}

/// Per-tick decay rate for nomads in wrong-biome cells, scaled by
/// the species' transit tier. Lower decay → pop survives longer
/// in transit → longer crossing range. Tier 0 keeps the proven
/// Baseline (1/500 ≈ 30-yr half-life) — at tier 0 the strict
/// block in `step_growth` already prevents wrong-biome diffusion,
/// so this rate only governs pop in cells that became wrong-
/// biome mid-run (e.g. terrain shifts). Tiers 2+ stretch the
/// half-life so high-tech species can chain crossings further.
fn decay_for_tier(tier: u32) -> Real {
    match tier {
        0 | 1 => Real::from_ratio(1, 500),
        2 => Real::from_ratio(1, 1_000),
        _ => Real::from_ratio(1, 2_000),
    }
}

/// Per-species multiplier on the baseline diffusion rate. A
/// species' generational turnover sets how often bands fission
/// and emigrate to a neighbouring cell, so diffusion scales as
/// `BASELINE_LIFESPAN / lifespan_years`. Clamped `[0.25, 4.0]` so
/// pathological short / long lifespans don't blow up the per-tick
/// budget. A 4-yr r-strategist hits the upper cap at 4×; a 200-yr
/// K-strategist lands at 0.4× and crawls; an 80-yr baseline
/// species runs at 1.0×.
fn lifespan_diffusion_scale(lifespan_years: Real) -> Real {
    let baseline = Real::from_int(NOMAD_DIFFUSION_BASELINE_LIFESPAN_YEARS);
    let lifespan = lifespan_years.max(Real::ONE);
    let raw = baseline / lifespan;
    let lo = Real::from_ratio(25, 100);
    let hi = Real::from_int(4);
    raw.max(lo).min(hi)
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
) -> Real {
    let mult = sim_world::cell_habitability(state, planet, cell);
    let weight = if matches!(species_habitat, Habitat::Aquatic) && mult == Real::ZERO {
        Real::ONE
    } else {
        mult
    };
    let saturation = Real::from_int(NOMAD_PER_CELL_CAP)
        * weight
        * Real::from_ratio(EMERGENT_PRESSURE_NUM, EMERGENT_PRESSURE_DEN);
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
) -> Real {
    let mult = sim_world::cell_habitability(state, planet, cell);
    let weight = if matches!(species_habitat, Habitat::Aquatic) && mult == Real::ZERO {
        Real::ONE
    } else {
        mult
    };
    Real::from_int(NOMAD_PER_CELL_CAP)
        * weight
        * Real::from_ratio(EMERGENT_CLUSTER_NUM, EMERGENT_CLUSTER_DEN)
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
) {
    let alive: std::collections::BTreeSet<u32> = pops.keys().copied().collect();
    streak.retain(|cell, _| alive.contains(cell));
    for (&cell, &pop) in pops {
        let threshold = pressure_threshold(state, planet, species_habitat, cell);
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
        let saturation = pressure_threshold(state, planet, species_habitat, cell);
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
                let nbr_threshold =
                    cluster_threshold(state, planet, species_habitat, nbr_id);
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

/// Whether `cell`'s terrain matches the species' native habitat.
/// Water glyphs for aquatic, land for terrestrial, both for
/// amphibious. Coast counts as both — transition zone.
fn is_habitat_match(
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
        Habitat::Terrestrial | Habitat::Airborne => is_land_glyph(glyph),
        Habitat::Amphibious => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_physics::HexGrid;
    use sim_world::{
        Atmosphere, BiosphereClass, Composition, Crust, Magnetosphere, MetabolicSubstrate,
    };
    use std::collections::BTreeSet;

    fn ocean_planet(_width: u32, _height: u32) -> sim_world::Planet {
        sim_world::Planet {
            seed: 1,
            name: "TestOcean".to_string(),
            gravity: Real::from_int(10),
            composition: Composition::OceanWorld,
            mean_temperature: Real::from_int(290),
            temperature_gradient: Real::from_int(15),
            terrain_peak: Real::from_int(2_000),
            sea_level: Real::from_int(1_500),
            atmosphere: Atmosphere::Oxidising,
            atmospheric_composition: sim_world::AtmosphericComposition::vacuum(),
            surface_pressure: Real::from_ratio(101_325, 1_000),
            biosphere: BiosphereClass::Lush,
            biosphere_density: Real::from_ratio(7, 10),
            crustal_composition: sim_world::CrustalComposition::empty(),
            magnetosphere: Magnetosphere::Strong,
            crust: Crust::Basaltic,
            stellar_luminosity: Real::ONE,
            moon_count: 1,
            moons: vec![],
            orbital_eccentricity_x100: 5,
            axial_tilt_deg: Real::from_int(23),
            day_length_hours: Real::from_int(24),
            orbital_period_months: 12,
            metabolic_substrate: MetabolicSubstrate::Aqueous,
            substrate_perturbation: Real::ZERO,
            terrain_centre_q: 0,
            terrain_centre_r: 0,
        }
    }

    fn populated_state(
        planet: &sim_world::Planet,
        width: u32,
        height: u32,
    ) -> sim_physics::PhysicsState {
        let mut state = sim_physics::PhysicsState::new(HexGrid::new(width, height));
        sim_world::init_planet(&mut state, planet);
        state
    }

    /// `init_pops` concentrates the starting population in
    /// `NOMAD_ORIGIN_CELL_COUNT` cells, not spread across every
    /// habitable cell.
    #[test]
    fn init_pops_seeds_only_origin_cells() {
        let planet = ocean_planet(8, 6);
        let state = populated_state(&planet, 8, 6);
        let claimed = BTreeSet::new();
        let pops = init_pops(&state, &planet, Habitat::Aquatic, &claimed);
        // Earlier: `pops.len()` would equal the number of
        // habitable cells (~tens). Now: at most
        // NOMAD_ORIGIN_CELL_COUNT.
        assert!(
            pops.len() <= NOMAD_ORIGIN_CELL_COUNT,
            "init_pops should seed at most {} origins; got {}",
            NOMAD_ORIGIN_CELL_COUNT,
            pops.len()
        );
        // Total population sums to (approximately) INITIAL_NOMAD_TOTAL.
        let total: Real = pops.values().copied().fold(Real::ZERO, |a, b| a + b);
        let expected = Real::from_int(INITIAL_NOMAD_TOTAL);
        let diff = if total > expected {
            total - expected
        } else {
            expected - total
        };
        assert!(
            diff < Real::from_int(1),
            "total pop {total:?} ≠ {expected:?}"
        );
    }

    /// Default cognition × sociality used by `step_growth` tests.
    /// Keeps tests independent of species sampling — these are
    /// nomad-mechanic tests, not species-derivation tests.
    fn test_traits() -> (Real, Real) {
        (Real::from_ratio(50, 100), Real::from_ratio(60, 100))
    }

    /// Lifespan used by `step_growth` tests. Pinned at the
    /// diffusion baseline so the rescale factor is exactly 1.0
    /// — tests targeting other knobs stay independent of the
    /// per-species diffusion rescale.
    fn test_lifespan() -> Real {
        Real::from_int(NOMAD_DIFFUSION_BASELINE_LIFESPAN_YEARS)
    }

    /// `step_growth` diffuses population to neighbouring
    /// cells. After one tick from a single-origin seed, neighbour
    /// cells carry non-zero nomadic pop.
    #[test]
    fn step_growth_diffuses_to_neighbours() {
        let planet = ocean_planet(8, 6);
        let state = populated_state(&planet, 8, 6);
        let claimed = BTreeSet::new();
        let mut pops = init_pops(&state, &planet, Habitat::Aquatic, &claimed);
        let initial_count = pops.len();
        assert!(initial_count <= NOMAD_ORIGIN_CELL_COUNT);
        let observations = BTreeMap::new();
        let (cog, soc) = test_traits();
        step_growth(
            &mut pops,
            &state,
            &planet,
            Habitat::Aquatic,
            &observations,
            cog,
            soc,
            test_lifespan(),
            &claimed,
        );
        assert!(
            pops.len() > initial_count,
            "step_growth should spread nomads to neighbours; \
             before: {initial_count}, after: {}",
            pops.len()
        );
    }

    /// Regression guard: empty cells DO NOT spontaneously
    /// generate population. Earlier, `step_growth` ran on every
    /// habitable cell and grew toward cap regardless of presence —
    /// so a cell with zero pop became cap×growth in one tick.
    #[test]
    fn step_growth_does_not_spontaneously_populate_empty_cells() {
        let planet = ocean_planet(8, 6);
        let state = populated_state(&planet, 8, 6);
        let claimed = BTreeSet::new();
        let mut pops: BTreeMap<u32, Real> = BTreeMap::new();
        let observations = BTreeMap::new();
        let (cog, soc) = test_traits();
        step_growth(
            &mut pops,
            &state,
            &planet,
            Habitat::Aquatic,
            &observations,
            cog,
            soc,
            test_lifespan(),
            &claimed,
        );
        assert!(
            pops.is_empty(),
            "empty pops should stay empty; got {} cells",
            pops.len()
        );
    }

    /// Tech-tier 0 species (no observations) cannot push pop into
    /// wrong-biome cells even when isolated. This is the strict
    /// habitat-confinement gate — pre-tech species stay on the
    /// connected habitat component containing each origin cell.
    #[test]
    fn step_growth_strict_block_at_tier_zero() {
        // Aquatic species on an ocean world: water-only diffusion.
        // Verify that with zero observations, pop does NOT enter
        // any land cells.
        let planet = ocean_planet(8, 6);
        let state = populated_state(&planet, 8, 6);
        let claimed = BTreeSet::new();
        let mut pops = init_pops(&state, &planet, Habitat::Aquatic, &claimed);
        let observations = BTreeMap::new();
        let (cog, soc) = test_traits();
        // Run many ticks so any leak would accumulate.
        for _ in 0..50 {
            step_growth(
                &mut pops,
                &state,
                &planet,
                Habitat::Aquatic,
                &observations,
                cog,
                soc,
                test_lifespan(),
                &claimed,
            );
        }
        for (cell, pop) in &pops {
            if !is_habitat_match(&state, &planet, *cell, Habitat::Aquatic)
                && *pop > Real::from_ratio(1, 10)
            {
                panic!(
                    "tier-0 species leaked pop {pop:?} into wrong-biome cell {cell}"
                );
            }
        }
    }

    /// Airborne species have innate +1 base tier from flight, so
    /// even at zero tech they can transit through wrong-biome
    /// (water) cells. Verifies the flight bonus path.
    #[test]
    fn step_growth_airborne_crosses_water_at_zero_tech() {
        let planet = ocean_planet(8, 6);
        let state = populated_state(&planet, 8, 6);
        let claimed = BTreeSet::new();
        // Airborne lives on land, like terrestrial.
        let mut pops = init_pops(&state, &planet, Habitat::Airborne, &claimed);
        if pops.is_empty() {
            // Tiny ocean grid may have no land origin; skip.
            return;
        }
        let observations = BTreeMap::new();
        let (cog, soc) = test_traits();
        // Run ticks to let flight-transit fire.
        let mut saw_water_pop = false;
        for _ in 0..200 {
            step_growth(
                &mut pops,
                &state,
                &planet,
                Habitat::Airborne,
                &observations,
                cog,
                soc,
                test_lifespan(),
                &claimed,
            );
            for (cell, pop) in &pops {
                if !is_habitat_match(&state, &planet, *cell, Habitat::Airborne)
                    && *pop > Real::from_ratio(1, 10)
                {
                    saw_water_pop = true;
                    break;
                }
            }
            if saw_water_pop {
                break;
            }
        }
        assert!(
            saw_water_pop,
            "airborne species should transit into wrong-biome \
             cells via innate flight even at zero tech"
        );
    }

    /// Terrestrial species with tech-tier ≥ 1 (≥10 tech score)
    /// unlocks wrong-biome transit. Mirror of the airborne test
    /// but via learned tech rather than innate ability.
    #[test]
    fn step_growth_terrestrial_unlocks_transit_with_tech() {
        let planet = ocean_planet(8, 6);
        let state = populated_state(&planet, 8, 6);
        let claimed = BTreeSet::new();
        let mut pops = init_pops(&state, &planet, Habitat::Terrestrial, &claimed);
        if pops.is_empty() {
            return;
        }
        // Inject enough observations into one populated cell to
        // push tech_score above TRANSIT_TIER_1_TECH (= 10).
        // tech_score = cog × soc × Σ counts; (0.5)(0.6) = 0.30, so
        // need Σ counts ≥ 34 for score ≥ 10.
        let (cog, soc) = test_traits();
        let mut observations: BTreeMap<u32, BTreeMap<u32, u64>> = BTreeMap::new();
        let seed_cell = *pops.keys().next().unwrap();
        observations
            .entry(seed_cell)
            .or_default()
            .insert(0, 50);
        let mut saw_water_pop = false;
        for _ in 0..200 {
            step_growth(
                &mut pops,
                &state,
                &planet,
                Habitat::Terrestrial,
                &observations,
                cog,
                soc,
                test_lifespan(),
                &claimed,
            );
            for (cell, pop) in &pops {
                if !is_habitat_match(&state, &planet, *cell, Habitat::Terrestrial)
                    && *pop > Real::from_ratio(1, 10)
                {
                    saw_water_pop = true;
                    break;
                }
            }
            if saw_water_pop {
                break;
            }
        }
        assert!(
            saw_water_pop,
            "terrestrial with tech ≥ tier 1 should transit \
             through wrong-biome cells"
        );
    }

    /// `cell_cap_bonus` returns 0 for cells with no observations or
    /// observations below all unlock thresholds — this is the
    /// regression guard that ensures pre-tech cells use the
    /// baseline cap unchanged.
    #[test]
    fn cell_cap_bonus_zero_below_thresholds() {
        assert_eq!(cell_cap_bonus(None), Real::ZERO);
        let mut obs = BTreeMap::new();
        // All template counts well below their thresholds.
        obs.insert(GROWTH_FIRE_TEMPLATE_ID, GROWTH_FIRE_THRESHOLD - 1);
        obs.insert(GROWTH_FERTILE_TEMPLATE_ID, GROWTH_FERTILE_THRESHOLD - 1);
        obs.insert(GROWTH_SOLVENT_TEMPLATE_ID, GROWTH_SOLVENT_THRESHOLD - 1);
        assert_eq!(cell_cap_bonus(Some(&obs)), Real::ZERO);
    }

    /// Cap bonuses stack additively across templates. Fire (10%)
    /// plus `fertile_land` (25%) plus `solvent_humid_band` (10%)
    /// rounds to ~45% (with small Q32.32 rounding error since
    /// 0.1, 0.25, 0.1 aren't binary-exact fractions).
    #[test]
    fn cell_cap_bonus_stacks_across_templates() {
        let mut obs = BTreeMap::new();
        obs.insert(GROWTH_FIRE_TEMPLATE_ID, GROWTH_FIRE_THRESHOLD);
        obs.insert(GROWTH_FERTILE_TEMPLATE_ID, GROWTH_FERTILE_THRESHOLD);
        obs.insert(GROWTH_SOLVENT_TEMPLATE_ID, GROWTH_SOLVENT_THRESHOLD);
        let bonus = cell_cap_bonus(Some(&obs));
        let expected = Real::from_ratio(45, 100);
        let diff = if bonus > expected {
            bonus - expected
        } else {
            expected - bonus
        };
        assert!(
            diff < Real::from_ratio(1, 1000),
            "expected ~0.45 cap bonus from fire+fertile+solvent, got {bonus:?}"
        );
    }

    /// Growth-rate bonuses stack: `thermal_gradient` (+10%) +
    /// `seasonal_thaw` (+10%) ≈ +20%.
    #[test]
    fn cell_growth_bonus_stacks_across_templates() {
        let mut obs = BTreeMap::new();
        obs.insert(GROWTH_THERMAL_TEMPLATE_ID, GROWTH_THERMAL_THRESHOLD);
        obs.insert(GROWTH_SEASONAL_TEMPLATE_ID, GROWTH_SEASONAL_THRESHOLD);
        let bonus = cell_growth_bonus(Some(&obs));
        let expected = Real::from_ratio(20, 100);
        let diff = if bonus > expected {
            bonus - expected
        } else {
            expected - bonus
        };
        assert!(
            diff < Real::from_ratio(1, 1000),
            "expected ~0.20 growth bonus from thermal+seasonal, got {bonus:?}"
        );
    }

    /// A cell with `thermal_gradient` + `seasonal_thaw` observations
    /// fills toward cap faster than a baseline cell. Verifies the
    /// growth bonus actually accelerates logistic fill in
    /// `step_growth`, not just the helper function.
    #[test]
    fn step_growth_growth_bonus_accelerates_filling() {
        let planet = ocean_planet(8, 6);
        let state = populated_state(&planet, 8, 6);
        let claimed = BTreeSet::new();
        let (cog, soc) = test_traits();
        // Two parallel scenarios with the same starting population:
        // one with growth-bonus observations, one without.
        let mut pops_baseline = init_pops(&state, &planet, Habitat::Aquatic, &claimed);
        if pops_baseline.is_empty() {
            return;
        }
        let mut pops_boosted = pops_baseline.clone();
        let obs_baseline: BTreeMap<u32, BTreeMap<u32, u64>> = BTreeMap::new();
        let mut obs_boosted: BTreeMap<u32, BTreeMap<u32, u64>> = BTreeMap::new();
        for cell in pops_boosted.keys() {
            let mut cell_obs = BTreeMap::new();
            cell_obs.insert(GROWTH_THERMAL_TEMPLATE_ID, GROWTH_THERMAL_THRESHOLD);
            cell_obs.insert(GROWTH_SEASONAL_TEMPLATE_ID, GROWTH_SEASONAL_THRESHOLD);
            obs_boosted.insert(*cell, cell_obs);
        }
        // Run both scenarios for the same number of ticks. With
        // INITIAL_NOMAD_TOTAL/NOMAD_ORIGIN_CELL_COUNT = 80, origins
        // start at-or-above per-cell cap (cap is 80×weight,
        // weight ≤ 1) so origin growth is a no-op or small; the
        // growth-bonus delta shows up on diffused-into neighbours
        // that grow logistically toward cap.
        for _ in 0..50 {
            step_growth(
                &mut pops_baseline,
                &state,
                &planet,
                Habitat::Aquatic,
                &obs_baseline,
                cog,
                soc,
                test_lifespan(),
                &claimed,
            );
            step_growth(
                &mut pops_boosted,
                &state,
                &planet,
                Habitat::Aquatic,
                &obs_boosted,
                cog,
                soc,
                test_lifespan(),
                &claimed,
            );
        }
        // Compare a non-origin cell that only grew via diffusion +
        // logistic fill in both runs. Pick any cell present in both.
        let baseline_total: Real = pops_baseline
            .values()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        let boosted_total: Real = pops_boosted
            .values()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        assert!(
            boosted_total > baseline_total,
            "growth-bonus run should accumulate more total pop than \
             baseline; baseline={baseline_total:?} boosted={boosted_total:?}"
        );
    }

    /// Lifespan diffusion rescale: 1.0 at the baseline lifespan,
    /// >1 for shorter-lived species (capped at 4×), <1 for longer-
    /// lived (floored at 0.25×). Pinned so the seed-495 Ylithar
    /// case (177-yr lifespan) lands clearly below 1× rather than
    /// at the historic flat-rate behaviour.
    #[test]
    fn lifespan_diffusion_scale_brackets() {
        let baseline = lifespan_diffusion_scale(Real::from_int(
            NOMAD_DIFFUSION_BASELINE_LIFESPAN_YEARS,
        ));
        assert_eq!(baseline, Real::ONE, "baseline lifespan should map to 1.0×");
        let r_strategist = lifespan_diffusion_scale(Real::from_int(4));
        assert_eq!(
            r_strategist,
            Real::from_int(4),
            "4-yr species should hit the 4× upper cap"
        );
        let k_strategist = lifespan_diffusion_scale(Real::from_int(400));
        assert_eq!(
            k_strategist,
            Real::from_ratio(25, 100),
            "400-yr species should hit the 0.25× lower cap"
        );
        let ylithar = lifespan_diffusion_scale(Real::from_int(177));
        assert!(
            ylithar < Real::ONE && ylithar > Real::from_ratio(25, 100),
            "177-yr species should land between the lower cap and 1.0×; got {ylithar:?}"
        );
    }

    /// `step_growth` with a long-lived species spreads slower than
    /// with a baseline-lifespan species under identical inputs.
    /// Anchors the seed-495 fix: Ylithar-like species (177y) no
    /// longer fill a continent in 55 years.
    #[test]
    fn step_growth_long_lived_species_diffuses_slower() {
        let planet = ocean_planet(8, 6);
        let state = populated_state(&planet, 8, 6);
        let claimed = BTreeSet::new();
        let (cog, soc) = test_traits();
        let observations = BTreeMap::new();
        let mut pops_baseline = init_pops(&state, &planet, Habitat::Aquatic, &claimed);
        if pops_baseline.is_empty() {
            return;
        }
        let mut pops_long = pops_baseline.clone();
        let baseline_lifespan = test_lifespan();
        let long_lifespan = Real::from_int(200);
        for _ in 0..30 {
            step_growth(
                &mut pops_baseline,
                &state,
                &planet,
                Habitat::Aquatic,
                &observations,
                cog,
                soc,
                baseline_lifespan,
                &claimed,
            );
            step_growth(
                &mut pops_long,
                &state,
                &planet,
                Habitat::Aquatic,
                &observations,
                cog,
                soc,
                long_lifespan,
                &claimed,
            );
        }
        assert!(
            pops_long.len() <= pops_baseline.len(),
            "long-lived species should occupy ≤ as many cells as baseline; \
             baseline={} long={}",
            pops_baseline.len(),
            pops_long.len()
        );
    }
}
