# sim/population

Cohorts and population dynamics. **Population is owned by the
species, not by individual civs.** Each cohort carries a
`civ_membership` tag that is either a civ ID or `stateless`
(between civs, after a collapse).

## Status

- **M2 shipped**: cohort + simple birth/death dynamics tied to a
  single civ scaffold.
- **M3 pending**: full carrying-capacity model with tech multipliers,
  migration gradient, full population dynamics across multiple civs
  on the same species.

## Cohorts

Age brackets per settlement (0-5, 6-15, 16-45, 46+). Births, deaths,
migrations operate on cohorts; civ membership changes when a cohort
migrates into a different civ's territory or when its region's civ
founds / collapses.

Intelligence / curiosity distributions are properties of the
**species** that drift slowly via biological evolution. Per-civ
cultural pressure (M4) can shift the realised distribution within
that civ's members without changing the species baseline.

## Carrying capacity and tech multipliers

Each region has a carrying capacity that scales with civ technology:

`region_capacity = base_capacity(biome, substances) × civ_tech_multiplier`

- **`base_capacity`** is set at planet/region generation from biome +
  regional substance inventory + the equilibrium biological-stock
  the physics produces. Lush forest > arid steppe > tundra.
  Sub-surface ocean has its own scale.
- **`civ_tech_multiplier`** starts at 1.0 (hunter-gatherer baseline)
  and grows multiplicatively as the civ acquires capabilities:
  - Cultivation discoveries → 10–100× depending on which crops
    are domesticated and which biomes support them.
  - Animal domestication → bonus food and transport.
  - Irrigation → unlocks marginal land, additional multiplier.
  - Storage / preservation (salt, drying, fermentation) → smoothes
    seasonal famines (lowers death rate rather than raising
    capacity).
  - Sanitation / medicine → reduces pathogen-driven death rate.
  - Industrial agriculture → another order-of-magnitude jump.

## Population dynamics per tick

For each cohort in each settlement:

- **Births** = `f(species reproductive rate, food security)` where
  `food_security = 1 - max(0, pop_in_region / region_capacity - 1)`.
- **Deaths** = `f(species lifespan baseline × civ medicine
  multiplier, pathogen pressure × civ sanitation multiplier, food
  security, conflict)`.
- **Migration** = pressure from over-capacity regions to adjacent
  below-capacity regions. M3 uses a simple gradient; M5 expands
  with cultural and political drivers.

No hard population ceiling — capacity grows with tech indefinitely.
Floor at zero (species extinction; ends the run).

## Coupling to discovery

Larger populations support more named figures (per
`sim/knowledge/README.md` cap formula) and grow cohort-level
observation pools. Discoveries that raise carrying capacity
(agriculture) or lower death rate (medicine) feed back into the
discovery loop. Civs that overshoot carrying capacity before
discovering agriculture suffer Malthusian crashes.

## Cited by

[docs/population.md](../../docs/population.md),
[docs/species.md](../../docs/species.md).
