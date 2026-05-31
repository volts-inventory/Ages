//! Per-tick logistic growth + physical wave-of-advance diffusion for
//! the nomadic species pool.
//!
//! Two phases per tick:
//!
//! 1. **Logistic growth on populated cells.** Each cell with a
//!    non-trivial population grows toward its per-cell cap along a
//!    Verhulst S-curve: `next = cur + r·cur·(1 − cur/cap)`, with `r`
//!    derived per-species from reproductive biology and bounded to a
//!    realistic *per-generation* multiple — so a small founder band
//!    ramps slowly before saturating and a long-lived, slow-maturing
//!    species fills habitat over centuries, not decades. Empty cells
//!    stay empty.
//! 2. **Neighbour diffusion.** Pop flows down density gradients to
//!    habitat-matching neighbours at a *physically-derived* rate: a
//!    band's descendants resettle a characteristic distance each
//!    generation (an ecological dispersal coefficient `D = L²/4T_gen`),
//!    which — divided by the planet's real per-cell area — gives the
//!    per-tick migration fraction. Per-pair terrain friction (slope,
//!    marginal biome) slows the front; tech-gated wrong-biome transit
//!    and per-tier decay still govern crossings.
//!
//! Net result: nomads radiate outward from the origin cell(s) as a
//! Fisher–Skellam wave of advance whose speed (`v = 2√(rD)`) emerges
//! from species biology and planet geometry — a continent fills over
//! millennia, the way real demic diffusion spread hominins across
//! Earth, rather than blanketing the planet in a few sim-decades.

use super::is_habitat_match;
use sim_arith::Real;
use sim_species::Habitat;
use std::collections::BTreeMap;

/// Pre-civilisational forager occupancy as a fraction of a cell's
/// *baseline civ carrying capacity* (`sim_civ::baseline_cell_capacity`
/// — biosphere × terrain × area × cognition, with no tech or tools).
/// The flat `NOMAD_PER_CELL_CAP = 80` this replaces was biosphere-
/// blind: 80 foragers in a continent-scale cell, while a civ founding
/// on the same land thinks in thousands-to-billions. Anchoring the
/// forager cap to a fraction of the settled-baseline capacity makes a
/// saturated wilderness cell host a realistic population for its area
/// *and* — critically — keeps it below what the founding civ's cells
/// can feed, so absorbing the saturated nomads never over-founds.
///
/// Set to `1/2`: foragers occupy half the settled-baseline density
/// (foraging supports fewer people per km² than the agriculture a
/// fresh civ already has). The logistic growth/diffusion shapes and
/// emergence thresholds are all relative to this cap, so the
/// founding-pacing dynamics are unchanged — only the absolute scale
/// moved up, biosphere-aware, in lockstep with the civ engine.
pub(crate) const FORAGER_CAPACITY_FRACTION: (i64, i64) = (1, 2);

/// Per-cell forager carrying capacity: `FORAGER_CAPACITY_FRACTION ×
/// sim_civ::baseline_cell_capacity(...)`. Zero where foragers can't
/// establish (claimed by a civ, gas band, or wrong habitat — though
/// callers on the populated set rarely hit those). Shared by the
/// growth step and the emergence saturation / cluster thresholds so
/// every "is this cell full?" decision reads the same ceiling.
///
/// `producer_biomass` is the live `PlanetEcosystem::tier_biomass(0)`
/// the civ capacity path also reads, threaded in from the tick loop.
pub(crate) fn cell_forager_cap(
    state: &sim_physics::PhysicsState,
    planet: &sim_world::Planet,
    cell: u32,
    tick: u64,
    species_habitat: Habitat,
    cognition: Real,
    producer_biomass: Real,
    survivability: Real,
) -> Real {
    let glyph = sim_world::terrain_glyph_at(state, planet, cell);
    if glyph == '\u{2261}' {
        return Real::ZERO;
    }
    if !is_habitat_match(state, planet, cell, species_habitat) {
        return Real::ZERO;
    }
    let baseline = sim_civ::baseline_cell_capacity(
        cognition,
        producer_biomass,
        state,
        cell,
        tick,
        planet,
        species_habitat,
    );
    // `survivability` (climate/pressure/atmosphere fit) scales the
    // forager ceiling the same way it scales a civ's carrying capacity,
    // so a marginally-habitable world hosts smaller wilderness
    // populations — and, because the founding saturation/cluster gates
    // read this same cap, the village a civ must reach before emerging
    // is correspondingly smaller too.
    baseline
        * Real::from_ratio(FORAGER_CAPACITY_FRACTION.0, FORAGER_CAPACITY_FRACTION.1)
        * survivability
}

// The intrinsic per-capita logistic growth rate `r` (per tick) is no
// longer a flat constant. It is derived per-species from the
// reproductive biology via
// `sim_population::PopulationDynamics::intrinsic_growth_rate` (clutch
// size, reproductive cadence, offspring survival, lifespan) — the
// same chain that drives the civ cohort — so r-strategists fill empty
// habitat fast and K-strategists crawl. In the Verhulst term
// `dN = r·N·(1 − N/cap)`, `r` is the max per-tick growth fraction at
// low density; growth proportional to `N` gives a true S-curve (slow
// cold start, fast middle, long taper) rather than the old
// gap-relaxation rule that leapt to a fixed fraction of the planetary
// cap in the first tick. The derived `r` is clamped to
// `[GROWTH_R_MIN, GROWTH_R_MAX]` in that helper. Per-cell tech bonuses
// (`thermal_gradient`, `seasonal_thaw`) still multiply it via
// `cell_growth_bonus`.

// Diffusion is no longer a flat per-tick gradient-closure constant.
// It is derived per-species, per-planet as a physical *wave of
// advance*: a band's descendants resettle a characteristic distance
// each generation, giving an ecological dispersal coefficient
// `D = L²/(4·T_gen)` (km²/yr); the per-tick, per-neighbour migration
// fraction is then `D·Δt / cell_area`, where the grid cell's physical
// area comes from the planet's actual radius. See `dispersal_fraction`.

/// Earth's mean radius in km. Anchors the physical size of a grid
/// cell, which converts a species' ecological dispersal coefficient
/// (km²/yr) into the per-tick, per-neighbour migration fraction the
/// diffusion phase applies. (`sim_world` keeps the metre value
/// privately for escape-velocity; the diffusion model only needs km.)
pub(crate) const EARTH_RADIUS_KM: i64 = 6_371;

/// RMS distance (km) a *terrestrial walker's* descendants resettle
/// over one generation — the physical anchor for spread speed. Chosen
/// so the emergent Fisher–Skellam wave of advance (`v = 2√(rD)`, with
/// `D = L²/4T_gen`) for a median species lands near the ~1 km/yr
/// demic-diffusion speed measured for real hominin and Neolithic-
/// farming expansions: a continent (tens of hundreds-of-km-wide cells)
/// fills over ~10–50k years rather than the decades the old flat
/// 1%/tick gradient bleed produced. Other locomotion modes scale off
/// this baseline in `dispersal_km_per_generation`.
pub(crate) const DISPERSAL_KM_PER_GEN_BASELINE: i64 = 30;

/// Elevation-difference friction scale (metres). A neighbour
/// `ELEV_FRICTION_HALF_M` higher or lower than the source receives
/// half the migration flow it would across flat ground:
/// `friction = scale / (scale + |Δelevation|)`. Mountains and rifts
/// slow the colonisation front without hard-blocking it.
pub(crate) const ELEV_FRICTION_HALF_M: i64 = 1_000;

/// Minimum population (people) for a cell to count as present.
/// Below this the cell is pruned from the pool. Set far below the
/// 150-person `NOMAD_DISPLAY_FLOOR_POP` render threshold so the
/// leading edge of a wave of advance *accumulates* its (initially
/// fractional) migrants tick over tick instead of being deleted each
/// step — a cell only renders as occupied once it has genuinely
/// grown past the display floor, which under the physical diffusion
/// rate takes centuries, not the single tick the old 0.1 threshold
/// + 1%/tick flow produced.
const NOMAD_PRESENCE_EPSILON: (i64, i64) = (1, 1_000);

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
///    non-trivial population grows toward its per-cell cap along a
///    Verhulst S-curve: `next = cur + r·cur·(1 − cur/cap)`, where
///    `r` is the species' biology-derived intrinsic rate, bounded to
///    a realistic per-generation growth multiple. A small founder
///    band ramps slowly before saturating; empty cells stay empty.
/// 2. **Neighbour diffusion.** Each populated cell sheds
///    `gap · dispersal_fraction · terrain_friction` of the density
///    gap to each habitat-matching neighbour, where
///    `dispersal_fraction` is the physically-derived per-tick
///    migration fraction (`D·Δt / cell_area`). The neighbour
///    receives the share; the source cell loses it. Still capped so
///    a cell can't shed more than half its pop in a single tick.
///
/// Net result: nomads radiate outward from the origin cell(s) as a
/// wave of advance whose speed emerges from the species' reproductive
/// rate and dispersal distance and the planet's real cell size —
/// filling a continent over millennia rather than sim-decades.
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
    biology: &sim_species::PopulationBiology,
    cognition: Real,
    sociality: Real,
    lifespan_years: Real,
    producer_biomass: Real,
    survivability: Real,
    tick: u64,
    claimed_cells: &std::collections::BTreeSet<u32>,
) {
    // Intrinsic per-capita logistic rate derived from the species'
    // reproductive biology — the same `PopulationDynamics` chain the
    // civ cohort uses — so clutch size, reproductive cadence, and
    // lifespan drive nomadic fill speed (r-strategists explode,
    // K-strategists crawl) rather than a one-size-fits-all constant.
    let growth = sim_population::PopulationDynamics::intrinsic_growth_rate(
        biology,
        lifespan_years,
        cognition,
        sociality,
    );
    // Physically-derived per-tick, per-neighbour migration fraction:
    // a wave of advance keyed to the species' dispersal distance per
    // generation and the planet's actual cell size, not a flat
    // constant. Friction (terrain / habitat) is applied per-pair below.
    let diffuse = dispersal_fraction(
        planet,
        state.grid(),
        species_habitat,
        biology,
        lifespan_years,
    );
    let species_t = species_tier(pops, observations, species_habitat, cognition, sociality);
    let decay = decay_for_tier(species_t);
    let grid = state.grid();

    // Helper: per-cell carrying capacity. Returns cap in people.
    // Zero for wrong-habitat (where the species can transit but
    // not establish), zero for claimed cells (civs handle their
    // own pop dynamics), zero for gas band. The biosphere-coupled
    // forager cap (`cell_forager_cap`) already returns zero for gas /
    // wrong-habitat; the claimed-cell guard stays here since the
    // shared helper is habitat/biosphere-only.
    let cell_cap = |cell: u32| -> Real {
        if claimed_cells.contains(&cell) {
            return Real::ZERO;
        }
        let cap_bonus = cell_cap_bonus(observations.get(&cell));
        cell_forager_cap(
            state,
            planet,
            cell,
            tick,
            species_habitat,
            cognition,
            producer_biomass,
            survivability,
        ) * (Real::ONE + cap_bonus)
    };

    // Phase 1 — logistic growth in habitat cells, decay in
    // non-habitat cells. The two cases are partitioned by cap:
    //   cap > 0: pop grows toward cap at the logistic rate, but is
    //            hard-clamped to never exceed the *current* cap. The
    //            cap is the instantaneous (seasonal) carrying capacity,
    //            so on a high-tilt world a cell's cap collapses every
    //            harsh season; clamping sheds the excess immediately
    //            rather than letting nomads ratchet at the seasonal
    //            peak. Net effect: pop tracks the seasonal *trough*
    //            (slow to grow, instant to shed), the same trough that
    //            limits a civ's sustained population — so the
    //            sustained-saturation founding gate only fires on
    //            cells whose cap is *stable*, where the forager pop a
    //            civ absorbs ≈ what its cells actually feed. Without
    //            the clamp, nomads pile up at the seasonal maximum and
    //            a founding civ over-absorbs ~100s× its trough cap.
    //   cap = 0: pop decays at the non-habitat decay rate.
    let populated: Vec<u32> = pops.keys().copied().collect();
    for cell in &populated {
        let cap = cell_cap(*cell);
        let cur = pops.get(cell).copied().unwrap_or(Real::ZERO);
        let next = if cap > Real::ZERO {
            let growth_bonus = cell_growth_bonus(observations.get(cell));
            let cell_growth = growth * (Real::ONE + growth_bonus);
            // True Verhulst logistic: dN = r·N·(1 − N/cap). The growth
            // term is proportional to the *current* population, so a
            // small founder band grows in proportion to its own size —
            // a slow cold start that accelerates to a peak near N=cap/2
            // and tapers as the cell saturates (the classic S-curve).
            // This is the realistic-biology replacement for the earlier
            // gap-relaxation rule `cur + (cap − cur)·r`, whose increment
            // was set by `cap` rather than `N` and so leapt a handful of
            // founders straight to a fixed fraction of the *planetary*
            // carrying capacity in a single tick — billions within
            // decades regardless of how few individuals existed.
            //
            // `occupancy = N/cap` is left unclamped so the term goes
            // negative when a seasonal cap collapse leaves `cur > cap`;
            // the hard clamp below then sheds the surplus to the trough
            // immediately (see the phase comment above), preserving the
            // "slow to grow, instant to shed" behaviour the founding
            // gate relies on.
            let occupancy = cur.saturating_div(cap);
            let grown = cur + cell_growth * cur * (Real::ONE - occupancy);
            if grown > cap {
                cap
            } else {
                grown
            }
        } else {
            // Non-habitat / claimed / gas: decay.
            cur * (Real::ONE - decay)
        };
        if next > Real::from_ratio(NOMAD_PRESENCE_EPSILON.0, NOMAD_PRESENCE_EPSILON.1) {
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
        if *source_pop <= Real::from_ratio(NOMAD_PRESENCE_EPSILON.0, NOMAD_PRESENCE_EPSILON.1) {
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
            // Per-pair friction: habitat suitability of the destination
            // glyph × an elevation-difference penalty. A climb into
            // mountains or a push into marginal biome flows slower than
            // a step across equivalent flat habitat.
            let friction = terrain_friction(state, planet, *source, n_cell, species_habitat);
            let amount = gap * diffuse * friction;
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
        if next > Real::from_ratio(NOMAD_PRESENCE_EPSILON.0, NOMAD_PRESENCE_EPSILON.1) {
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

/// Characteristic dispersal distance (km) a band's descendants
/// resettle over one generation, by locomotion mode. Scales off the
/// terrestrial-walker baseline: flight ranges farthest, open-water
/// swimming and amphibious movement reach further than walking, and
/// burrowers / endoliths creep. This `L` feeds the dispersal
/// coefficient `D = L²/(4·T_gen)`.
pub(super) fn dispersal_km_per_generation(habitat: Habitat) -> Real {
    let base = Real::from_int(DISPERSAL_KM_PER_GEN_BASELINE);
    let scale = match habitat {
        Habitat::Airborne => Real::from_int(4),
        Habitat::Aquatic => Real::from_int(2),
        Habitat::Amphibious => Real::from_ratio(3, 2),
        Habitat::Terrestrial => Real::ONE,
        // Burrowers / rock-borers advance through the substrate, not
        // across it — far slower than a surface walker.
        Habitat::Subterranean | Habitat::Endolithic => Real::from_ratio(1, 4),
    };
    base * scale
}

/// Per-tick, per-neighbour migration fraction — the physical wave of
/// advance. A species' descendants resettle `L =
/// dispersal_km_per_generation` over one generation `T_gen`, giving
/// an ecological diffusion coefficient `D = L²/(4·T_gen)` (km²/yr). On
/// a grid whose cells span `cell_area` km² (derived from the planet's
/// real radius), the explicit-diffusion fraction crossing one cell
/// boundary per tick is `D·Δt / cell_area`, with `Δt = 1/12 yr` (one
/// month). The result is tiny (≈10⁻⁶) compared with the old flat
/// 1%/tick, so a continent fills over millennia and long-lived,
/// slow-maturing or coarse-cell species crawl while fast-maturing
/// r-strategists on small worlds spread quickly — all emergent, no
/// privileged constant.
pub(super) fn dispersal_fraction(
    planet: &sim_world::Planet,
    grid: &sim_physics::HexGrid,
    habitat: Habitat,
    biology: &sim_species::PopulationBiology,
    lifespan_years: Real,
) -> Real {
    let months_per_year =
        Real::from_int(i64::try_from(protocol::BASELINE_MONTHS_PER_YEAR).unwrap_or(12));
    let generation_ticks =
        sim_population::PopulationDynamics::generation_ticks(biology, lifespan_years);
    let generation_years = generation_ticks / months_per_year;
    let l = dispersal_km_per_generation(habitat);
    // D = L² / (4 · T_gen)  [km²/yr]
    let d = (l * l) / (Real::from_int(4) * generation_years);
    // cell_area = 4π R² / num_cells   [km²]
    let r_km = planet.radius * Real::from_int(EARTH_RADIUS_KM);
    let four_pi = Real::from_ratio(12_566, 1_000);
    let surface = four_pi * r_km * r_km;
    let num_cells = Real::from_int(i64::from(grid.width()) * i64::from(grid.height())).max(Real::ONE);
    let cell_area = surface / num_cells;
    // dt = one tick = 1/months_per_year of a year.
    let dt = Real::ONE / months_per_year;
    (d * dt) / cell_area.max(Real::ONE)
}

/// Fraction of the nominal flow that crosses into a *wrong-biome*
/// (transit) neighbour, before the elevation penalty. Crossing
/// inhospitable terrain — open water for a walker, land for a swimmer
/// — is slow and costly but not impossible for a transit-capable
/// (tier ≥ 1 / innately-flighted) species; the caller already gates
/// *whether* transit is allowed, and per-tier decay attrits the pop
/// mid-crossing. This is deliberately *not* the destination's habitat
/// suitability (which is ~0 for deep ocean) — that would conflate
/// "can't settle here" with "can't cross here" and block boats /
/// flight entirely.
pub(crate) const TRANSIT_FRICTION: (i64, i64) = (1, 10);

/// Per-source→neighbour migration friction in `(0, 1]`: the fraction
/// of the nominal flow that actually crosses this boundary, combining
/// terrain attractiveness with an elevation-difference penalty (a
/// steep climb or drop slows the front). For a *habitat-matching*
/// neighbour the attractiveness is the destination glyph's habitat
/// suitability (coast richest, peaks marginal); for a *wrong-biome*
/// neighbour it's the flat `TRANSIT_FRICTION` crossing cost. Flat,
/// in-habitat steps run at ~1.0.
pub(super) fn terrain_friction(
    state: &sim_physics::PhysicsState,
    planet: &sim_world::Planet,
    source: u32,
    neighbour: u32,
    habitat: Habitat,
) -> Real {
    let attractiveness = if is_habitat_match(state, planet, neighbour, habitat) {
        let glyph = sim_world::terrain_glyph_at(state, planet, neighbour);
        sim_species::habitat_glyph_multiplier(habitat, glyph)
    } else {
        Real::from_ratio(TRANSIT_FRICTION.0, TRANSIT_FRICTION.1)
    };
    let elevation = state.elevation();
    let (src_idx, ngh_idx) = (source as usize, neighbour as usize);
    let elev_factor = match (elevation.get(src_idx), elevation.get(ngh_idx)) {
        (Some(&a), Some(&b)) => {
            let delta = if a >= b { a - b } else { b - a };
            let scale = Real::from_int(ELEV_FRICTION_HALF_M);
            scale / (scale + delta)
        }
        _ => Real::ONE,
    };
    attractiveness * elev_factor
}
