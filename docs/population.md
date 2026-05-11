# Population

Per-cell heterogeneous cohorts evolve independently with cell-local
seasonal capacity and habitability. Substrate-derived demographics
replace flat Earth defaults. Nomadic species pools spread across
habitable terrain before any civ exists.

For deeper detail per crate:
[`sim/population/README.md`](../sim/population/README.md). For
substrate sampling see [world.md](world.md). For migration triggers
in conflict see [culture.md](culture.md).

## Cohort layout

`Cohort` is the per-cell population record
([`sim/population/src/lib.rs`](../sim/population/src/lib.rs)):

```rust
pub struct Cohort {
    pub count: Real,                      // population in `Real` (Q32.32)
    pub civ_membership: Option<u32>,      // Some(civ_id) or None for stateless
}
```

Births and deaths happen in fractional amounts each tick, which
is why `count` is `Real` rather than an integer. Per-civ
state — capacity, food shortfall, age-structured death rates —
lives on the `Civ` struct in `sim_civ`, not on `Cohort` itself.

Civ-tagged cohorts and stateless cohorts coexist in the same
`BTreeMap<cell, Cohort>`. The species-wide nomadic pool is a
separate `BTreeMap<cell, Real>` in `sim_core::nomads`, tracking
unclaimed-cell populations that diffuse across habitable terrain.

## Substrate-derived demographics

Every demographic constant derives from the planet's substrate +
biosphere + species traits. No homo-sapiens-pinned defaults.

| Constant | Driven by |
|----------|-----------|
| Founding floor | Sociality + biosphere density |
| Carrying capacity (per fuel-unit) | Biosphere density × cognition × `(1 / gravity)`, baseline 2500 |
| Migration pressure threshold | Sociality (0.55 – 0.75 band) |
| Birth-rate biosphere multiplier | Biosphere density × axial-tilt seasonal swing × luminosity × substrate metabolism |
| Lifespan rescaling | `80 / lifespan_years` clamped `[0.25, 4.0]` |

A 200-year species reproduces at 0.4×; a 20-year species at 4×.

### Substrate metabolism — biological time scale

Per-cell biological **rates** (birth, civ-claim cadence, hypothesis
attempts, cohesion drift, religion/cosmology drift) and per-streak
**cooldowns** (civil-war / food-crisis / cultural-lock /
knowledge-plateau / tiny-territory / depopulation thresholds, plus
the disease catastrophe cooldown) are multiplied / divided by a
substrate-derived `metabolism` factor so a silicate civ unfolds
across ~5× more ticks than an aqueous one. Aqueous is the
calibration baseline (1.0):

| Substrate | Metabolism | Effective time-scale |
|-----------|-----------|----------------------|
| Aqueous | 1.0 | baseline |
| Ammoniacal | 0.5 | 2× longer |
| Hydrocarbon | 0.4 | 2.5× longer |
| Silicate | 0.2 | 5× longer |

Physics catastrophes (asteroid / solar flare / ice age / volcanic)
are *not* scaled — they're external to biology and keep raw
cooldowns. Disease is scaled because it's a crowding-driven
biological event.

### Per-cell capacity rescale

The base `carrying_capacity_per_unit` is **2500** (5× the prior
500). Each 36×30 grid cell represents ~470,000 km²; the lift lets
a 200k-population civ live in ~20 densely-packed cells (dense core +
sparse frontier) rather than uniformly claiming ~120 cells. The
migration-pressure threshold is correspondingly lowered to the
0.55–0.75 band so claim activity continues — cells densify to
~60–75% of cap before spilling into neighbours.

## Nomadic species pool

Before any civ exists, the species spreads as a nomadic population
across habitable cells. Cells with population above a floor but
unclaimed by any civ render as `0` glyphs in the viewport.

`SpeciesNomadsChanged` events emit on substantial nomadic-pool
shifts.

When a civ's BFS expansion reaches a nomadic cell, it absorbs the
nomad cohort into the civ's per-cell cohort. The nomadic pool
continues outside claimed territory.

## Habitat-priority diffusion

Nomadic spreading isn't uniform. Three origin cells score on
`habitability × connectivity` at run start. From those origins,
density-gradient diffusion prefers habitat-matching neighbours;
non-habitat cells decay 1/500 per tick. So the nomadic pool
naturally piles up in habitable bands and avoids dead terrain
without explicit walls.

The base diffusion rate (`NOMAD_DIFFUSION_NUM/DEN`, currently
1/100 per tick at the 80-yr baseline) is rescaled per species by
`BASELINE_LIFESPAN / lifespan_years`, clamped `[0.25, 4.0]`. Range
expansion via band fission scales with generational turnover, so a
4-yr r-strategist diffuses up to 4× faster than a baseline species
and a 200-yr K-strategist crawls at 0.4×. This keeps long-lived
species from saturating an entire continent in a few decades while
still letting fast-clutch life carpet the planet quickly.

## Per-cell heterogeneous dynamics

Each cell of a civ evolves independently with cell-local seasonal
capacity and habitability:

- **Cell capacity** = `base_capacity × habitability_multiplier ×
  seasonal_multiplier × (1 + Σ tool_capacity_effects)`.
- **Births** scale with biosphere multiplier and adult-cohort
  population.
- **Deaths** scale with stress factor (food shortfall, age,
  catastrophe presence).

## Gradient-driven migration

Cells whose pressure (population / capacity) exceeds the
migration-pressure threshold shed population into adjacent cells
with headroom. Pair-flux conservation: every unit leaving cell A
arrives at cell B; nothing is created or destroyed in transit.

## Civ founding from nomad density

Civs emerge when:

- A nomadic region accumulates sufficient density.
- The species has crossed the relevant tech-readiness gate.
- A habitable centroid cell exists.

Founding draws from the nomadic pool — the new civ claims a small
ring of cells centred on the high-density centroid and absorbs
the nomadic cohorts there into its per-cell cohorts. The
remaining nomadic population continues outside the new claim.

## Seasonal capacity

Cell capacity multiplies by a seasonal factor keyed to the
planet's `orbital_period_months` and the cell's hemisphere.
`fertile`-band cells get the highest swing; deep-cold cells stay
nearly flat. A planet with extreme axial tilt has steep seasonal
multipliers — births bunch in spring, die-offs bunch in winter.

## Catastrophes hit cells

Catastrophes (volcanic, disease, asteroid, solar flare, ice age)
hit specific cells, not the whole civ. Disease targets the
densest cell; asteroid lands on a deterministic `(seed, tick)`-
keyed cell. Per-cell cohort takes a population hit; the civ may
or may not collapse depending on how much of its total
population sat in the affected cells. See
[catastrophes.md](catastrophes.md).

## Multi-tick wars

Border conflicts resolve cell-by-cell across multiple skirmish
events with marching-front semantics. Each cell flips when the
loser cohort's per-cell population crosses `CELL_FLIP_FLOOR`. The
post-run report aggregates skirmishes into "war campaigns" — see
[culture.md](culture.md#multi-tick-wars) and
[report.md](report.md).

## Population events

- `SpeciesNomadsChanged(species_id, total_nomads, delta)` — pool
  drift.
- `CivTerritoryChanged(civ_id, claimed_cells_added, claimed_cells_lost)`
  — territory delta from BFS expansion, war loss, or breakaway.
- `CatastropheFired(...)` — cell-localized; population effect
  visible via subsequent `CivTerritoryChanged` and the next
  `Snapshot`.
- `CivCollapsed(civ_id, reason)` — collapse types covering food
  crisis, civil war, conflict, catastrophe, cultural lock,
  territory too small.

The post-run report walks `Snapshot` deltas and per-civ events to
render the population timeline (a sparse arc derived from
founding / collapse / catastrophe events).
