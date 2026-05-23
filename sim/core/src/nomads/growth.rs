//! Per-tick logistic growth + density-gradient diffusion for the
//! nomadic species pool.
//!
//! Two phases per tick:
//!
//! 1. **Logistic growth on populated cells.** Each cell with a
//!    non-trivial population grows toward its per-cell cap:
//!    `next = cur + (cap - cur) × NOMAD_GROWTH`. Empty cells stay
//!    empty.
//! 2. **Neighbour diffusion.** Pop flows from higher-density cells
//!    to lower-density along habitat-matching neighbours, with
//!    tech-gated wrong-biome transit and per-tier decay.
//!
//! Net result: nomads radiate outward from the origin cell(s)
//! over the first few hundred ticks, mimicking the way populations
//! spread across continents in real biology rather than
//! spontaneously appearing everywhere from tick 0.

use super::{cell_weight, is_habitat_match};
use sim_arith::Real;
use sim_species::Habitat;
use std::collections::BTreeMap;

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
        let weight = cell_weight(state, planet, cell, species_habitat);
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

/// Derive a scalar tech score from a cell's per-template
/// observation counts. Score = `cognition × sociality × Σ counts`.
/// Cells with rich phenomena observed many times score high; a
/// barren cell observed once or twice doesn't. Used for
/// emergence threshold checks; same shape as the legacy scalar but
/// derived from the per-template counts so the threshold and the
/// civ's inherited knowledge are consistent.
pub(super) fn tech_score(
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
pub(super) fn cell_cap_bonus(cell_observations: Option<&BTreeMap<u32, u64>>) -> Real {
    let Some(obs) = cell_observations else {
        return Real::ZERO;
    };
    let mut bonus = Real::ZERO;
    if obs.get(&GROWTH_FIRE_TEMPLATE_ID).copied().unwrap_or(0) >= GROWTH_FIRE_THRESHOLD {
        bonus = bonus + Real::percent(10);
    }
    if obs.get(&GROWTH_FERTILE_TEMPLATE_ID).copied().unwrap_or(0) >= GROWTH_FERTILE_THRESHOLD {
        bonus = bonus + Real::percent(25);
    }
    if obs.get(&GROWTH_SOLVENT_TEMPLATE_ID).copied().unwrap_or(0) >= GROWTH_SOLVENT_THRESHOLD {
        bonus = bonus + Real::percent(10);
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
pub(super) fn cell_growth_bonus(cell_observations: Option<&BTreeMap<u32, u64>>) -> Real {
    let Some(obs) = cell_observations else {
        return Real::ZERO;
    };
    let mut bonus = Real::ZERO;
    if obs.get(&GROWTH_THERMAL_TEMPLATE_ID).copied().unwrap_or(0) >= GROWTH_THERMAL_THRESHOLD {
        bonus = bonus + Real::percent(10);
    }
    if obs.get(&GROWTH_SEASONAL_TEMPLATE_ID).copied().unwrap_or(0) >= GROWTH_SEASONAL_THRESHOLD {
        bonus = bonus + Real::percent(10);
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
        Habitat::Aquatic
        | Habitat::Terrestrial
        | Habitat::Amphibious
        // Subterranean + Endolithic don't fly across wrong-biome
        // cells; they tunnel through, which the current model
        // treats as 0-tier surface crossing.
        | Habitat::Subterranean
        | Habitat::Endolithic => 0,
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
pub(super) fn lifespan_diffusion_scale(lifespan_years: Real) -> Real {
    let baseline = Real::from_int(NOMAD_DIFFUSION_BASELINE_LIFESPAN_YEARS);
    let lifespan = lifespan_years.max(Real::ONE);
    let raw = baseline / lifespan;
    let lo = Real::percent(25);
    let hi = Real::from_int(4);
    raw.max(lo).min(hi)
}
