# Population

Per-cell heterogeneous cohorts evolve under a 4-bracket age model
(infant / juvenile / fertile / elder). Rates derive from per-species
biology — clutch size, lifespan, per-bracket survival — not from
homo-sapiens-pinned defaults. The legacy vertebrate step is one of
seven `Lifecycle` variants; eusocial colonies, semelparous
broadcast-spawners, microbial doubling, modular biomass, and plants
each route through a topology-specific per-tick function.

Crate-side detail: [`sim/population/README.md`](../sim/population/README.md).
Substrate sampling: [world.md](world.md). Per-species biology
sampling: [species.md](species.md). Catastrophes that hit cohorts:
[catastrophes.md](catastrophes.md).

## The 4-bracket cohort

`Cohort` ([`sim/population/src/cohort.rs:19`](../sim/population/src/cohort.rs))
replaces the prior scalar `count` with explicit age structure:

```rust
pub struct Cohort {
    pub infant: Pop,
    pub juvenile: Pop,
    pub fertile: Pop,
    pub elder: Pop,
    pub civ_membership: Option<u32>,
}
```

Only `fertile` reproduces; only `fertile` carries full economic /
military weight downstream. Births land in `infant` and age forward
each tick via per-bracket promotion rates. Civ-tagged and
stateless cohorts coexist in the same `BTreeMap<cell, Cohort>`;
post-collapse remnants live as `civ_membership: None` until a
successor civ absorbs them or they decay.

`Pop` is Q32.32 fixed-point (`sim_arith::Pop`). Sub-integer births
and deaths accumulate every tick, so a cohort that loses ~0.3
person per tick to mortality steadily decays rather than
ratchetting on integer boundaries.

### Bracket arithmetic primitives

The cohort module owns every bracket-scoped operation so the
per-tick step in `dynamics.rs` only has to talk about birth /
survival / aging math
([`sim/population/src/cohort.rs:70-262`](../sim/population/src/cohort.rs)):

| Method | Purpose |
|--------|---------|
| `total()` | Bracket-agnostic sum. |
| `weighted_demand(biology)` | `Σ bracket × food_multiplier`. Used by the food-security ratio. |
| `add_fertile(delta)` | Deposit a scalar pop as adult founders (nomad absorption at civ founding). |
| `deposit_distributed(count, biology)` | Split a scalar pop across all four brackets per the species' bracket fractions (mixed-age nomad absorption post-founding). |
| `scale_in_place(factor)` | Identical multiplier to every bracket (mass-conserving shrink). |
| `split_off_fraction(f)` | Take fraction `f` of every bracket into a new cohort (BFS expansion seeding). |
| `migrate_family_to(dst, fertile_to_move)` | Adults drag dependent infants + juveniles proportionally; elders stay rooted. |
| `migrate_balanced_to(dst, total)` | Proportional slice of every bracket. Preserves age structure; drains saturated cores into hollow interior holes. |
| `merge_in(other)` | Refugee folding when a civ sheds a cell. |
| `shrink_to(target)` | Proportional shrink to a headcount target (catastrophe loss with floor). |
| `floor_at_zero()` | Defensive clamp after subtractive ops (war casualties). |

The two migration primitives encode different demographic policies:
`migrate_family_to` models a productive-age family unit relocating
(elders too senescent to migrate), while `migrate_balanced_to`
models the slow intra-civ rebalancing flow between adjacent claimed
cells — drain only the productive brackets and source cells
demographically collapse (elders age out with no fertile to replace
them, the cell falls below the prune floor, saturated cores
gradually hollow into pruned interior holes).

## Food-weighted demand

Each bracket draws food at a different rate
([`sim/species/src/sampling.rs:942-947`](../sim/species/src/sampling.rs)):

| Bracket | Multiplier |
|---------|-----------|
| Infant | 0.30 |
| Juvenile | 0.60 |
| Fertile | 1.00 (reference) |
| Elder | 0.90 |

The per-cell capacity formula compares `weighted_demand` (not raw
`total()`) against the cell's capacity, so an age-skewed cohort
with many dependents feels stress harder than a fertile-heavy one.
`food_security` ([`sim/population/src/dynamics.rs:476-489`](../sim/population/src/dynamics.rs))
returns `1 − max(0, demand/capacity − 1)`, clamped to `[0, 1]`:
demand at or below capacity gives `1.0`; demand at 2× capacity
gives `0.0`. Capacity ≤ 0 forces extinction through the step.

## PopulationDynamics — per-species rate derivation

`PopulationDynamics`
([`sim/population/src/dynamics.rs:32-71`](../sim/population/src/dynamics.rs))
caches the per-tick rates derived from `PopulationBiology` +
lifespan + cognition + sociality. All rates are per-month
(1 tick = 1 month) pinned to `BASELINE_MONTHS_PER_YEAR = 12`
([`protocol/src/header.rs:128`](../protocol/src/header.rs)); a
planet's `orbital_period_months` drives year-of-tick display but
not per-tick rate calibration.

```rust
pub struct PopulationDynamics {
    pub birth_rate: Real,
    pub infant_survival_per_tick: Real,
    pub juvenile_survival_per_tick: Real,
    pub fertile_survival_per_tick: Real,
    pub elder_survival_per_tick: Real,
    pub infant_to_juvenile: Real,
    pub juvenile_to_fertile: Real,
    pub fertile_to_elder: Real,
    pub stress_factor: Real,
    pub food_multipliers: [Real; 4],
    pub mortality_reduction: [Real; 4],
    pub birth_rate_multiplier: Real,
}
```

### `for_species` — pure derivation

`PopulationDynamics::for_species(biology, lifespan_years, cognition, sociality)`
([`sim/population/src/dynamics.rs:91-222`](../sim/population/src/dynamics.rs))
is a pure function — same inputs always produce the same rate
struct. The shape:

1. **Bracket durations**:
   `bracket_months = lifespan_years × bracket_fraction × 12`,
   floored at 1 month so divides don't blow up.
2. **Birth rate**: three back-compat tiers based on which biology
   fields the species literal carries —
   - New biology (`events > 0 && success > 0`):
     `(clutch × events × success) / fertile_months`
   - Mid-tier (`events > 0`, `success == 0`):
     `(clutch × events) / fertile_months` (PR #29 back-compat)
   - Legacy (`events == 0`): `clutch / fertile_months`
   `saturating_mul` guards against Q32.32 overflow for hyper-r
   broadcast-spawner tails.
3. **Per-tick survival**: derived from the species' per-bracket
   window-survival via `per_tick = exp(ln(window_survival) / months)`
   — the time-resampling identity `S = s^(1/months)` made stable
   in fixed-point.
4. **Fertile bracket window-survival**:
   `0.85 + 0.10 × cognition` (range [0.85, 0.95]) — smarter
   species lose fewer adults to medicine + agriculture.
5. **Elder bracket window-survival**: flat `0.30` (senescence
   dominates; death is programmed, not preventable by hygiene). A
   species with `eldership_fraction = 0` collapses to immediate
   elder death.
6. **Aging-out rates**: `1 / months_in_bracket`. No fertile→elder
   promotion when the species has no elder bracket.
7. **Stress factor**: `5 − sociality − cognition`, floored at 2;
   range [2, 5]. Mutual aid + adaptive behaviour buffer the death
   amplification under food shortfall.
8. **Tech fields** (`mortality_reduction`, `birth_rate_multiplier`)
   default to neutral; the civ refreshes them from its unlocked
   tools each tick before calling `step_with_capacity`.

## step_with_capacity — per-tick step

`PopulationDynamics::step_with_capacity(cohort, capacity)`
([`sim/population/src/dynamics.rs:264-380`](../sim/population/src/dynamics.rs))
is the vertebrate baseline step. Order of operations:

1. **Demand + food security**: `Σ multiplier × bracket`, then
   `security = food_security(demand, capacity)`,
   `stress = 1 − security`.
2. **Mortality compose**: each bracket's per-tick survival
   combines three terms via `combine(survival, starvation, reduction)`:
   - Tech mortality reduction scales the baseline `(1 − survival)`
     by `(1 − reduction)`.
   - Stress amplification: `× (1 + stress × stress_factor)`.
   - Additive starvation mortality: `stress × 0.10` scaled
     per-bracket (`infant × 2.0`, `juvenile × 1.5`, `fertile × 1.0`,
     `elder × 1.5`) — models "no food kills you in months
     regardless of how healthy you were."
3. **Births**: `fertile × birth_rate × birth_rate_multiplier ×
   security`, clamped at the **5× fertile recruit ceiling** (no
   real species recruits more than 5× its fertile population in a
   single month; this is the load-bearing safety net for
   r-strategist tails and Q32.32 overflow).
4. **Survival application**: each bracket multiplied by its
   composed survival; births land in `infant`.
5. **Aging promotions**: a fraction of each non-fertile bracket
   promotes to the next stage. Elder has no destination —
   attrition is already in survival.
6. **Floor at zero**: defensive clamp.

The order matters: survival applies *before* aging so a starving
bracket loses people who would have aged up.

`step` (uncoupled variant, line 386) treats capacity as
effectively infinite — used by cohorts whose region has no
biological-stock-driven capacity, and by tests.

## Life expectancy

`life_expectancy_months()`
([`sim/population/src/dynamics.rs:421-467`](../sim/population/src/dynamics.rs))
returns the expected lifespan at birth under unstressed conditions
with currently-applied tech mortality reduction. It's a pure
function of the dynamics struct — no allocation, no mutation —
modelling each bracket as a competing-hazards problem:

- Per tick an individual either dies (`1 − s`), promotes to the
  next bracket (`s × r`), or stays (`s × (1 − r)`).
- Expected sojourn time: `1 / (1 − s × (1 − r))`.
- Probability of reaching the next bracket alive:
  `s × r / (1 − s × (1 − r))`.

Total expectancy is the sum over brackets of
`P(reach bracket) × E[time in bracket]`. Tech effects flow through
`mortality_reduction`: a high-tier medical civ's expectancy
reflects sanitation + medicine directly. The civ engine emits
`CivLifeExpectancyChanged` when `life_expectancy_months`
crosses a delta threshold off the last emitted value.

## Lifecycle dispatch — 7 variants

`step_for_lifecycle(lifecycle, lifecycle_state, dynamics, cohort, capacity)`
([`sim/population/src/lifecycle.rs:121-163`](../sim/population/src/lifecycle.rs))
routes each tick to the variant's step function. The
`Vertebrate` arm is bit-identical to `step_with_capacity`; every
other variant supplies a minimal-correct step shaped around the
variant's biology.

### Variant table

| Variant | State storage | Step function | Key behaviour |
|---------|---------------|---------------|---------------|
| `Vertebrate` | `LifecycleState::None` | `step_with_capacity` | 4-bracket cohort baseline. |
| `Aquatic { semelparous: true }` | `LifecycleState::None` | `step_aquatic_semelparous` | One-shot fertile-window spawn collapsed to the tick; adults → 0 post-spawn. |
| `Aquatic { semelparous: false }` | `LifecycleState::None` | `step_aquatic_iteroparous` | Vertebrate step + 70% metamorphosis cull on juvenile→fertile promotion. |
| `Insect` | `LifecycleState::None` | `step_insect` | Vertebrate step + extra 5%/tick adult mortality (adult phase brief). |
| `Plant` | `LifecycleState::None` | `step_plant` | Elder survival lifted to ≥ fertile (durable senescent stage); infant survival halved (high seed failure). |
| `Eusocial { castes }` | `LifecycleState::Eusocial(colony)` | `step_eusocial` | Per-caste headcount in `BTreeMap`; only `Reproductive` produces births; sterile castes consume food but never reproduce. |
| `Microbial { fission_strategy }` | `LifecycleState::Microbial(biomass)` | `step_microbial` | Single biomass scalar; doubles per generation time (Binary 1 tick, Budding 2, Conjugation 4). |
| `Modular` | `LifecycleState::Modular(biomass)` | `step_modular` | Single biomass scalar; logistic-style growth `dN = r × N × (1 − N/K)` at `r = 5%`. |

When the supplied `lifecycle_state` doesn't match the `Lifecycle`
variant (e.g. `None` for an Eusocial species), the dispatcher
falls through to the vertebrate 4-bracket step so a partially-wired
call produces a well-defined trajectory rather than panicking.

### Per-variant fidelity notes

**Semelparous aquatic** (`step_aquatic_semelparous`,
[`sim/population/src/lifecycle.rs:194-240`](../sim/population/src/lifecycle.rs)):
the per-event spawn collapses the entire fertile-window lifetime
allotment to one tick (`birth_rate × 12 × birth_rate_multiplier ×
fertile × security`), clamped at the 5× recruit ceiling. Fertile
and elder brackets drop to zero after spawn — Pacific salmon /
mayfly-like adults.

**Iteroparous aquatic** (line 242-263): runs the vertebrate step
then estimates juveniles that just promoted (`pre_juvenile ×
juvenile_to_fertile`) and culls 70% of that count from the new
fertile pool — the metamorphosis bottleneck.

**Insect** (line 275-281): vertebrate step + multiplicative 5%
adult attrition — insects' adult phase is the briefest stage of
their life-history.

**Plant** (line 294-308): clones the dynamics struct with elder
survival lifted to `max(fertile, elder)` (plants' senescent stage
is durable, not fragile) and infant survival halved by a 50% seed-
failure factor before delegating to the vertebrate step.

**Eusocial** (line 366-405): per-caste `BTreeMap<CasteRole, Pop>`
ensures deterministic iteration. Demand uses the fertile food
multiplier for *every* caste (sterile castes are adult-bodied).
Only the `Reproductive` caste produces births; survival applies
uniformly across all castes. Worker / Soldier / Nurse populations
monotonically decay without resupply unless the caller routes
births into them by overriding `caste_birth_target`.

**Microbial** (line 418-447): generation-time doubling. Per-tick
factor is hard-coded from `2^(1/N)` for `N ∈ {1, 2, 4}` (Binary,
Budding, Conjugation). Under shortage the effective factor blends
linearly: `1 + (factor − 1) × security`.

**Modular** (line 452-466): logistic-style discrete growth
`N_{t+1} = N + r × N × max(0, 1 − N/K)` at `r = 5%`. Capacity ≤ 0
triggers a 10% die-back per tick.

## Substrate-derived demographics

Every demographic constant derives from the planet's substrate +
biosphere + species traits (see
[`sim/civ/src/demographics.rs`](../sim/civ/src/demographics.rs)):

| Constant | Driver | Default |
|----------|--------|---------|
| Founding floor | `founding_min_population(biosphere, cognition)` | `50 + 35×bio_pressure + 15×(1−cognition)` |
| Carrying capacity per unit | `carrying_capacity_per_unit(cognition, cell_count)` | `50_000 × cognition_factor × resolution_factor` |
| Migration pressure threshold | `migration_pressure_threshold(sociality)` | `0.55 + 0.20×sociality`, range `[0.55, 0.75]` |
| Birth-rate biosphere multiplier | `biosphere_birth_factor_for_planet(planet)` | `base × (1 − 0.1×tilt/90) × lum_norm` |
| Lifespan rescaling | derived inside `for_species` via bracket months | `BASELINE_MONTHS_PER_YEAR / lifespan_years × ...` |

`carrying_capacity_per_unit`
([`sim/civ/src/demographics.rs:123`](../sim/civ/src/demographics.rs))
defaults to **50,000 / unit** at the reference 36×30 = 1080-cell
grid resolution and rescales inversely with `cell_count` so total
planet capacity is grid-resolution-invariant. Cognition contributes
a narrow `[0.85, 1.15]` factor — low-cognition species pay a
small capacity tax; high-cognition gain a small bonus. Most of the
cognition signal lives elsewhere (hypothesizer cadence, stress
factor). Gravity is not multiplied here: native species are adapted
to their home gravity; penalising them against 1g Earth is Earth-
centrism.

The 20× lift from the prior 2,500 baseline puts paleolithic civs
at ~50k per cell (city-state density). The tech-tier multiplier
stack (see [`sim/civ/src/tools.rs`](../sim/civ/src/tools.rs))
carries agricultural civs to ~M/cell, industrial to ~10M/cell,
modern/future to hundreds of M/cell.

### Substrate metabolism — biological time scale

Per-cell biological rates and per-streak cooldowns are scaled by a
substrate-derived `metabolism` factor
([`sim/world/src/types.rs:215-223`](../sim/world/src/types.rs))
so a silicate civ unfolds across ~5× more ticks than an aqueous
one:

| Substrate | Metabolism | Effective time-scale |
|-----------|-----------|----------------------|
| Aqueous | 1.0 | baseline |
| Ammoniacal | 0.5 | 2× longer |
| Hydrocarbon | 0.4 | 2.5× longer |
| Silicate | 0.2 | 5× longer |

Physics catastrophes (asteroid, solar flare, ice age, volcanic)
are *not* scaled — they're external to biology and keep raw
cooldowns. Disease *is* scaled because it's a crowding-driven
biological event.

## Nomadic species pool

Before any civ exists, the species spreads as a nomadic population
across habitable cells. Cells with population above a floor but
unclaimed by any civ render as `0` glyphs in the viewport.
`SpeciesNomadsChanged` events emit on substantial nomadic-pool
shifts.

When a civ's BFS expansion reaches a nomadic cell, it absorbs the
nomad cohort into the civ's per-cell cohort (via
`Cohort::deposit_distributed` so age structure spreads correctly).
The nomadic pool continues outside claimed territory.

### Habitat-priority diffusion

Nomadic spreading isn't uniform. Three origin cells score on
`habitability × connectivity` at run start. From those origins,
density-gradient diffusion prefers habitat-matching neighbours;
non-habitat cells decay at 1/500 per tick. The nomadic pool
naturally piles up in habitable bands and avoids dead terrain
without explicit walls.

The base diffusion rate (`NOMAD_DIFFUSION_NUM/DEN`, currently
1/100 per tick at the 80-yr baseline, see
[`sim/core/src/nomads/growth.rs:70`](../sim/core/src/nomads/growth.rs))
is rescaled per species by `BASELINE_LIFESPAN / lifespan_years`,
clamped `[0.25, 4.0]`. Range expansion via band fission scales
with generational turnover, so a 4-yr r-strategist diffuses up to
4× faster than a baseline species and a 200-yr K-strategist crawls
at 0.4×.

## Per-cell heterogeneous dynamics

Each cell of a civ evolves independently with cell-local seasonal
capacity and habitability:

- **Cell capacity** = `base_capacity × habitability_multiplier ×
  seasonal_multiplier × (1 + Σ tool_capacity_effects)`.
- **Births** scale with biosphere multiplier, fertile-cohort
  population, and `birth_rate_multiplier` from civ tech.
- **Deaths** scale with stress factor (food shortfall, age,
  catastrophe presence) and are reduced by per-bracket
  `mortality_reduction` from civ tech.

## Gradient-driven migration

Cells whose pressure (`weighted_demand / capacity`) exceeds the
migration-pressure threshold shed population into adjacent cells
with headroom via `Cohort::migrate_balanced_to`. Pair-flux
conservation: every unit leaving cell A arrives at cell B; nothing
is created or destroyed in transit.

`tech_augmented_migration_threshold`
([`sim/civ/src/demographics.rs:170`](../sim/civ/src/demographics.rs))
lifts the threshold for tech-rich civs (urban planning, irrigation)
by up to 1.10× the base, capped at 0.92, so high-tech civs
tolerate 55–92% fill before spillover instead of the base 55–75%.

## Civ founding from nomad density

Civs emerge when:

- A nomadic region accumulates sufficient density.
- The species has crossed the relevant tech-readiness gate.
- A habitable centroid cell exists.

Founding draws from the nomadic pool — the new civ claims a small
ring of cells centred on the high-density centroid and absorbs
the nomadic cohorts there via `Cohort::deposit_distributed`. The
remaining nomadic population continues outside the new claim.

## Seasonal capacity

Cell capacity multiplies by a seasonal factor keyed to the
planet's `orbital_period_months` and the cell's hemisphere.
`fertile`-band cells get the highest swing; deep-cold cells stay
nearly flat. A planet with extreme axial tilt has steep seasonal
multipliers — births bunch in spring, die-offs bunch in winter.

## Dormancy + catastrophe survival

Tardigrade-grade species carry a `dormancy_capability ∈ [0, 1]`
on the `Species` struct. A catastrophe's realised damage is
reduced by `(1 − dormancy × severity_factor)`
([`sim/species/src/types.rs:624-631`](../sim/species/src/types.rs)).
At full severity (`severity_factor = 1`), a `dormancy = 0.9`
species takes ~10× less damage than `dormancy = 0`.

The surviving-but-dormant fraction lands in a `DormantPool`
([`sim/species/src/types.rs:562-607`](../sim/species/src/types.rs))
that revives at 1%/tick back into the active cohort, capped at
the pre-event population so the active pool never overshoots its
pre-catastrophe level. `DormantPool::resurrect_step` is
deterministic Q32.32 — no float, no HashMap.

Catastrophes also gate on `ToleranceEnvelope::match_score` — a
radiation burst, for instance, multiplies per-tick mortality by
`tolerance.match_score(local_conditions)` so extremophile species
shaped to high-radiation niches survive intact while
narrower-envelope species are wiped out. See [species.md](species.md).

## Catastrophes hit cells

Catastrophes (volcanic, disease, asteroid, solar flare, ice age)
hit specific cells, not the whole civ. Disease targets the
densest cell; asteroid lands on a deterministic `(seed, tick)`-
keyed cell. Per-cell cohort takes a population hit via
`Cohort::shrink_to(target)` so age structure is preserved; the
civ may or may not collapse depending on how much of its total
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
- `CivLifeExpectancyChanged(civ_id, life_expectancy_months_q32)` —
  fired when the civ's `dynamics.life_expectancy_months()` crosses
  a delta off the last emitted value.
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
