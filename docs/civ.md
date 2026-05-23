# Civilization model

A civilization is a bounded collectivity within a species — it
founds, runs its course, often collapses, and is succeeded by
other civs that inherit its territory and (partial) knowledge.
Multiple civs may exist concurrently. The **species** is the
run's persistent unit; civs come and go.

The `Civ` struct lives in
[`sim/civ/src/lib.rs:86`](../sim/civ/src/lib.rs). `Civ` impls are
split across role-focused sibling modules: `state` (constructors /
accessors), `drift` (per-civ species drift), `founding` (refound +
literacy + substrate config), `tools` (effect aggregators +
sensorium gating), `lifecycle` (collapse evaluation + cohesion
drift), `territory` (per-cell pop + migration), `capacity`
(carrying capacity), `observation` (per-figure observe + step +
cosmology + form refresh).

For deeper detail per crate, see
[`sim/civ/README.md`](../sim/civ/README.md). For cosmology /
religion / kinship / transmission see [culture.md](culture.md).
For tech and tools see [tech.md](tech.md). For catastrophes see
[catastrophes.md](catastrophes.md). For the discovery pipeline see
[discovery.md](discovery.md). For population dynamics see
[population.md](population.md). For the world see
[world.md](world.md).

## Species as the persistent unit

```
species  ──── persistent ── traits, sensorium, lifespan baseline,
                            cognition distribution, communication
   │
   ├── nomadic pool   ──  unclaimed cohorts spread across habitable
   │                      cells; rendered as `0` glyphs
   │
   ├── named figures  ──  per-civ individuals with knowledge graphs;
   │                      observation logs, hypotheses, discoveries
   │
   └── civilizations  ──  bounded collectivities; multiple may exist
                          concurrently or sequentially
```

Each civ tracks per-civ drift on the species baseline
(`cognition_delta`, `sociality_delta`, `lifespan_delta_years`,
`communication_fidelity_delta` at
[`lib.rs:367-380`](../sim/civ/src/lib.rs)) so a long chain of
successors drifts the species' effective traits.

## Founding

No hardcoded "inaugural civ". Civs found themselves emergently
when a nomadic region accumulates sufficient density, the
species has crossed the relevant tech-readiness gate, and a
habitable centroid cell is available.

Inaugural founding emits `CivFounded` with
`parent_civ_id = None`. Breakaway and stateless re-founding paths
emit a `parent_civ_id` when the new civ inherits state. Founding
constants (all at
[`lib.rs:624-626`](../sim/civ/src/lib.rs)):
`FOUNDING_MIN_POPULATION = 100`,
`RECENT_REMNANT_WINDOW_TICKS = 250 yr`,
`FOUNDING_MIN_DARK_AGE_TICKS = 50 yr`.

A successor's `territory_centroid` is shifted off any collision
with the parent's via `pick_successor_centroid`
([`lib.rs:67`](../sim/civ/src/lib.rs)) — preferring an adjacent
hex neighbour, else any other claimed cell, else the figure-
derived fallback. `Civ::refound_from_stateless`
([`founding.rs:30`](../sim/civ/src/founding.rs)) re-tags the
stateless cohort to the new id, derives a fresh `NameGrammar`
from `(species_seed, civ_id, modalities)`, allocates a founding
band with new per-figure hypothesizers, and seeds a fresh
religion vector via `religion::founding_religion`.

## Per-civ species drift

Inaugural civs zero the deltas; each successor inherits its
parent's deltas plus a deterministic per-generation perturbation:
`SUCCESSOR_DRIFT_TRAIT_STEP = ±0.02` on unit-range traits and
`SUCCESSOR_DRIFT_LIFESPAN_STEP_YEARS = ±1` on lifespan
([`lib.rs:636,643`](../sim/civ/src/lib.rs)). `SpeciesDrift`
events emit on each new civ's deltas.

M7 layers catastrophe-driven selection bias on top: each
catastrophe folds `selection_bias` (capped at
`SELECTION_BIAS_CHANNEL_CEILING = 0.15` per channel) into the
successor's inherited deltas via
`inherit_species_drift_with_environment`.

## Internal life: cohesion + civil war

A per-civ `[0, 1]` cohesion scalar (`cohesion` at
[`lib.rs:335`](../sim/civ/src/lib.rs)) tracks internal stability.
It drifts toward an equilibrium driven by size (larger civs lose
cohesion faster), food security (shortfalls erode), dogmatism
(stabilises within-civ but punishes heretical confirmations), and
literacy (record-keeping holds polity together). Drift logic in
`update_cohesion`
([`lifecycle.rs:256`](../sim/civ/src/lifecycle.rs)).

When `cohesion < CIVIL_WAR_COHESION_FLOOR = 0.10` for
`CIVIL_WAR_STREAK_TICKS = 75 yr` (stretched by substrate
metabolism — see [population.md](population.md)), the civ
collapses with reason `civil_war`
([`lib.rs:570,574`](../sim/civ/src/lib.rs)). `CohesionShifted`
events emit on ≥ 0.05 absolute drift
(`COHESION_EMIT_THRESHOLD`).

## Breakaway

Two breakaway paths, both producing a successor civ that inherits
parent state:

- **Cohesion-driven breakaway.** When cohesion sits in
  `[CIVIL_WAR_COHESION_FLOOR, COHESION_BREAKAWAY_TRIGGER]`
  (0.10–0.35) for `COHESION_BREAKAWAY_STREAK_TICKS = 40 ×
  MONTHS_PER_YEAR`, the civ forks: a regional faction takes
  `COHESION_BREAKAWAY_SHARE = 30%` of the parent's population
  and starts at `COHESION_BREAKAWAY_INITIAL = 0.85` cohesion; the
  parent recovers `COHESION_PARENT_RECOVERY = +0.15` (disgruntled
  faction left). Constants at
  [`sim/civ/src/lib.rs:594-611`](../sim/civ/src/lib.rs).
- **Dogmatic breakaway.** When the parent's cosmology dogmatism
  axis exceeds threshold and a heretical hypothesis is force-
  rejected, a fork emerges centered on the heretical view.

The breakaway streak counter (`cohesion_breakaway_streak` at
[`sim/civ/src/lib.rs:347`](../sim/civ/src/lib.rs)) resets to zero
when cohesion exits the zone (above trigger or below floor).

## Collapse

Per-civ collapse reasons (the `CollapseReason` enum at
[`sim/civ/src/lib.rs:493`](../sim/civ/src/lib.rs)):

| Reason | Trigger |
|--------|---------|
| `FoodCrisis` | `food_security <= 0.3` for `FOOD_CRISIS_STREAK_TICKS = 100yr` |
| `KnowledgePlateau` | no confirmed/refinement event for `PLATEAU_WINDOW_TICKS = 500yr` |
| `CulturalLock` | `dogmatism > 0.85` for `CULTURAL_LOCK_STREAK_TICKS = 250yr` with no refinements |
| `TerritoryTooSmall` | `claimed_cells.len() <= 1` for `TINY_TERRITORY_STREAK_TICKS = 2yr` |
| `CivilWar` | cohesion-driven (above) |
| `Depopulation` | aggregate pop ≤ 1 for `DEPOPULATION_STREAK_TICKS = 2yr` |

Constants at
[`sim/civ/src/lib.rs:536-563`](../sim/civ/src/lib.rs). Multiple
streaks may compound; whichever crosses threshold first wins.
The check loop is `check_collapse_with_terrain` in
[`sim/civ/src/lifecycle.rs:119`](../sim/civ/src/lifecycle.rs).

Cell-by-cell loss in war is accumulated against
`CONFLICT_DEFEAT_FLOOR = 50`; per-skirmish caps cap single-event
population loss at 60% (`CONFLICT_HIERARCHY_BONUS` ceiling in
[`sim/civ/src/conflict/war.rs`](../sim/civ/src/conflict/war.rs)).

## Successor + stateless re-founding

When a civ collapses, its territory unclaims and its named
figures die. Knowledge can survive across the boundary via the
inheritance window (see
[discovery.md](discovery.md#transmission-and-mythologization)).

Stateless re-founding fires when:

- Stateless cohort population ≥ `FOUNDING_MIN_POPULATION` (100).
- Most recent collapse within `RECENT_REMNANT_WINDOW_TICKS` (250
  baseline-years, stretched by substrate metabolism).
- At least `FOUNDING_MIN_DARK_AGE_TICKS` (50 baseline-years,
  stretched by substrate metabolism) have passed since collapse.

## Active law engagement

Confirmed relations track a falsification streak — sustained
mispredictions force refinement faster than the residual-only
path (`RelationFalsified`). Successors re-validate inherited
knowledge after `REVALIDATION_WINDOW_TICKS = 50` ticks (set in
[`sim/civ/src/discovery/hypothesizer.rs:40`](../sim/civ/src/discovery/hypothesizer.rs));
failed re-fits emit `RelationLapsed` and drop the relation. Wrong
knowledge dies; right knowledge strengthens. Detail in
[discovery.md](discovery.md).

## Inter-civ contact

Two civs make contact when their claimed-cell sets touch
(Manhattan distance ≤ 1) for the first time. `CivContact` emits,
recording both civ ids. Contact is mirrored into both civs'
`contact_history` set
([`sim/civ/src/lib.rs:184`](../sim/civ/src/lib.rs)). Contact is a
hard prerequisite for war — the kinship-weighted belligerence
machinery (see [culture.md](culture.md)) gates on
`emitted_contacts` containing the pair.

## Catastrophes (5 kinds)

The `CatastropheKind` enum lives at
[`sim/civ/src/catastrophe/kind.rs`](../sim/civ/src/catastrophe/kind.rs);
the per-tick orchestrator is `check_and_apply` in
[`sim/civ/src/catastrophe/apply/mod.rs`](../sim/civ/src/catastrophe/apply/mod.rs).
Module layout:

| Module | Responsibility |
|--------|----------------|
| `kind` | `CatastropheKind` enum + telemetry tag |
| `record` | `CatastropheRecord` per-event payload |
| `triggers` | per-kind firing predicates |
| `factors` | planet-driven severity/cooldown scaling |
| `cells` | impact-site selection + hex-neighbour expansion |
| `damage` | tolerance-gate cell conditions + resistance/dormancy applicator |
| `apply` | per-tick orchestrator |

Cooldowns and per-event population-loss floors live in
[`sim/civ/src/catastrophe/mod.rs:43-55`](../sim/civ/src/catastrophe/mod.rs):

| Kind | Cooldown | Base loss | Trigger |
|------|----------|-----------|---------|
| **Volcanic** | 200 yr | 5% | cell with `|charge| > 80` AND `T > 600 K` |
| **Disease** | 500 yr | 30% | civ at ≥ 80% crowding AND age ≥ 300 yr |
| **Asteroid** | 5,000 yr | 40% | every ~4733 yr (prime period) |
| **SolarFlare** | 800 yr | 10% | weak/no magnetosphere, high luminosity, period scales with stellar spectral class |
| **IceAge** | 4,000 yr | 20% | mean T ≤ 260 K AND civ age ≥ 1000 yr |

All cooldown constants are scaled `× MONTHS_PER_YEAR` since the
sim ticks at 1 month per tick. Trigger predicates live in
[`sim/civ/src/catastrophe/triggers.rs`](../sim/civ/src/catastrophe/triggers.rs).

### Per-cell + tolerance gating

Catastrophes act on a per-cell impact site (densest-claimed or
deterministic per-tick pick from
[`sim/civ/src/catastrophe/cells.rs`](../sim/civ/src/catastrophe/cells.rs))
and ripple to hex neighbours. Each affected cell builds a
`(temperature, pH, salinity, radiation, pressure)` tuple via
`catastrophe_cell_conditions` in
[`sim/civ/src/catastrophe/damage.rs:78`](../sim/civ/src/catastrophe/damage.rs)
that's fed to the species' `ToleranceEnvelope::match_score`:

```text
base_loss      = raw_frac × (1 − civ_tool_resistance) × (1 − match_score)
after_dormancy = base_loss × (1 − dormancy × severity)
```

A perfect envelope fit (extremophile shaped for the cell's
conditions) zeroes the damage; outside-envelope species take the
full hit. Catastrophe-specific deltas — ice-age `−50 K`, solar-
flare radiation boost, asteroid radiation boost — apply on top of
baselines in
[`damage.rs:25-56`](../sim/civ/src/catastrophe/damage.rs).

### DormantPool seed-bank survival (P1.3)

`apply_resistance_and_dormancy`
([`damage.rs:160`](../sim/civ/src/catastrophe/damage.rs)) deposits
the would-be casualties absorbed by dormancy into
`civ.dormant_pool.population`:

```text
seeded = pop_before × base_loss × dormancy_capability × severity
```

`pre_catastrophe_population` (high-water mark) bumps to track the
largest active cohort ever observed. `DormantPool::resurrect_step`
(from
[`sim/species/src/types.rs:590`](../sim/species/src/types.rs))
drains the pool back into the active cohort at
`revive_rate = 1%`/tick, capped at `pre_event_target` so the
active pool never exceeds the pre-catastrophe level it's
recovering toward. Empty by default — only catastrophe-driven
dormancy ever populates it.

## Conflict: war / alliance / grudge / assessment

The `conflict` module is folder-split. Re-exports keep the
pre-split surface intact for sim/core (see
[`sim/civ/src/conflict/mod.rs`](../sim/civ/src/conflict/mod.rs)):

| Module | Responsibility |
|--------|----------------|
| `war` | `resolve`, casualty math, per-cell flip logic, strength scoring |
| `alliance` | `propose_alliance`, drift-based dissolution, trust decay |
| `grudge` | per-pair grudge accumulator with lazy decay |
| `assessment` | Q-war belligerence (`assess_pair`, `decide_war`) |

### Belligerence (assessment)

Per-pair belligerence
([`assessment.rs:188`](../sim/civ/src/conflict/assessment.rs)):

```text
drive        = 0.45·pressure + 0.25·opportunity + 0.30·dominance
belligerence = drive × (1 − KINSHIP_DAMPENER · kinship)
```

- `pressure = pop / (cap × 1.25)`
- `opportunity = (defender_cap − defender_pop) / attacker_pop`
- `dominance = strength_a / (strength_a + strength_b)`

Hysteresis: `WarDeclared` at `belligerence ≥ 0.25`,
`PeaceConcluded` at `belligerence < 0.15`
([`assessment.rs:51,56`](../sim/civ/src/conflict/assessment.rs)).
`KINSHIP_DAMPENER = 0.10` — identical-cosmology pairs see 90%
of their drive land; alien pairs see the full drive. Kinship
formula in [culture.md#kinship](culture.md#kinship-read-by-belligerence).

### War resolution

`conflict::war::resolve`
([`war.rs:192`](../sim/civ/src/conflict/war.rs)) runs cell-by-cell
skirmishes every `CONFLICT_CHECK_TICKS = 75`. Casualty fraction:

```text
loss_frac = CONFLICT_MIN_LOSS (0.10)
          + winner_hier × CONFLICT_HIERARCHY_BONUS × size (≤ +0.30)
          + tech_gap (≤ +0.30, capped at 60% per skirmish)
```

Casualties hit **fertile** bracket first (combat-age adults), with
30% spillover into juveniles. Infants and elders die through
follow-on famine/displacement. Cells flip when loser's per-cell
cohort drops below `CELL_FLIP_FLOOR = 25`.

`hierarchical_strength`
([`war.rs:70`](../sim/civ/src/conflict/war.rs)) is a 4-channel
composite: cosmology 40% (`cosmology.hierarchical`) + religion
20% (`|theology| + |ritual|` magnitude) + kinship/cohesion 25% +
economic 15% (`surplus / aggregate_pop`, saturating at 1.0).
`hierarchy_size_factor` gates the casualty bonus by
`log10(claimed_cells.len()) / 2` capped at 1.0 — a 1-cell band
gets no organisational edge, a 100-cell empire hits the cap.

### Alliance

`propose_alliance`
([`alliance.rs:97`](../sim/civ/src/conflict/alliance.rs)) requires
five cumulative conditions: cosmology distance <
`ALLIANCE_FORM_COSMO_GAP = 0.40`, religion distance <
`ALLIANCE_FORM_RELIGION_GAP = 0.40`, not at war, mutual
`contact_history` entries, cooldown
(`ALLIANCE_FORM_COOLDOWN_TICKS = 200`) clear since any prior
dissolution.

Dissolution: cosmology drift past
`ALLIANCE_DISSOLVE_COSMO_GAP = 0.60`, trust scalar below
`ALLIANCE_TRUST_FLOOR = 0.20`, or war misalignment. Trust decays
per check by `0.5 × (cosmo + religion gaps) / 2`.

Mutual alliance flags short-circuit war resolution
([`war.rs:203`](../sim/civ/src/conflict/war.rs)). Asymmetric
flags do not short-circuit.

### Grudge

Per-pair grudge accumulator with lazy decay
([`grudge.rs`](../sim/civ/src/conflict/grudge.rs)). Each skirmish
bumps asymmetrically: `GRUDGE_BUMP_WINNER = 0.05` (decays at
`1/1000`/tick), `GRUDGE_BUMP_LOSER = 0.10` (decays at
`5/10_000`/tick). Loser holds longer.
`GRUDGE_CEILING = 0.60` clamps per-side accumulation. The decayed
grudge subtracts from kinship in `kinship_pair` so a pair at war
for decades reads as fundamentally hostile in kinship space even
if religion has re-converged.

## Apparatus + cultural lock

### Apparatus

`apparatus::Apparatus`
([`apparatus.rs:57`](../sim/civ/src/apparatus.rs)) is a single
experiment cell + the clamp/measure channel pairing. When
`ToolKind::ExperimentApparatus` unlocks, one apparatus is
allocated inside the civ's claimed cells. Each tick the
apparatus **clamps** one physics channel at one of four ladder
values pre-physics (`Channel::clamp_ladder`), physics integrates,
the apparatus **reads** the post-integration value at the same
cell, and the civ's hypothesizer ingests `(clamp_value,
response)` as a sample on the controlled-x distribution instead
of whatever planetary heterogeneity passive observation provides.

This is the Galileo-style "hold height fixed, measure fall time"
intervention — a clean diffusion experiment recovers heat-
conduction `α` in dozens of ticks instead of thousands. The
clamp ladder stays in raw physics units (250–360 K for
temperature, etc.) and below charge-discharge thresholds so the
apparatus doesn't fire artificial lightning. Successor civs do
**not** inherit apparatus — each successor rebuilds on re-unlock.

### Cultural lock

When `cosmology.dogmatism() ≥ CULTURAL_LOCK_DOGMA = 0.85` and no
refinement-confirmed event has fired within
`CULTURAL_LOCK_STREAK_TICKS = 250 yr` (stretched by metabolism),
the civ collapses with `CollapseReason::CulturalLock`. Logic in
[`lifecycle.rs`](../sim/civ/src/lifecycle.rs);
`cultural_lock_streak` and `last_refinement_tick`
([`lib.rs:261,265`](../sim/civ/src/lib.rs)) drive the gate. The
streak resets whenever dogmatism drops below floor OR a
refinement confirms.

## Tech tree (tools + dynamic-tool registry)

The static `ToolKind` enum
([`sim/civ/src/tech/mod.rs:45`](../sim/civ/src/tech/mod.rs)) has
~70 variants across five tiers — sensorium tools, stone-age (tier
1, fire / weapons / dwellings), settlement (tier 2, cultivation /
storage / writing), pre-industrial (tier 3, gunpowder / clockwork
/ jurisprudence), industrial (tier 4, mechanisation / chemistry /
materials), and information-age + narrative-trio (tier 5, digital
/ orbital / `BioelectricResonator` / `FieldPropulsionEngine` /
`MetamaterialLattice`). See [tech.md](tech.md) for the full
tier-by-tier catalog.

Tech submodules (in
[`sim/civ/src/tech/`](../sim/civ/src/tech/)): `consumption` (per-
tick resource draw), `effects` (per-tool multipliers), `gating`
(unlock gates — `is_unlocked`, observation + literacy + relation
prereqs), `identity` (tool id / display name),
`specs/{manipulation,relations,tools}` (per-tool specs).

Each `ToolKind` carries `manipulation_prereqs` (body-plan
affordance), `relation_prereqs` (`(template_id, ChannelKind)`
pairs the civ must have *confirmed*), `tool_prereqs` (earlier
unlocked `ToolKind`s — longer chains form the actual tech tree),
and observation + literacy thresholds (per-tier).

`Civ::unlocked_tools` is a `BTreeSet<ToolKind>`. Effects fold via
`tools` module aggregators: `tech_multiplier`,
`tool_war_strength_multiplier`,
`tool_transmission_fidelity_bonus`,
`apply_catastrophe_resistance`.

### Dynamic-tool registry

The static enum gives every species the same tech tree. **Dynamic
tools** layer per-species emergent tools on top — when a civ
accumulates a coherent cluster of confirmed relations on a
single recognition channel, it proposes a `DynamicTool` whose
effects scale with the cluster depth
([`sim/civ/src/discovery/tool_emergence.rs`](../sim/civ/src/discovery/tool_emergence.rs)).

Dual storage:

- **Species-level registry** — `Species::dynamic_tool_registry`
  + `next_dynamic_tool_id`. Tools proposed by any civ of the
  species enter here once.
- **Per-civ unlocks** — `Civ::unlocked_dynamic_tools`
  ([`lib.rs:139`](../sim/civ/src/lib.rs)) — owned `Vec` of copies
  (keeps effect aggregators `&self`-only). Sorted by tool id for
  deterministic effect-fold order.

The effect aggregators fold static + dynamic tools with the same
combinator (product or sum). Cadence of the emergence scan:
`TOOL_EMERGENCE_CHECK_PERIOD_TICKS = 600` (same as template
emergence).

## Discovery / hypothesizer pipeline

The discovery layer is folder-split in
[`sim/civ/src/discovery/`](../sim/civ/src/discovery/): `channels`
(physics + measurement channel enums + stable `relation_id_for`
namespace), `events` (`HypothesisEvent` wire-format), `types`
(`CandidateRelation`, `MeasurementCandidate`,
`ConfirmedRelation`, `RefinementState`, `ResidualBasis`),
`hypothesizer` (per-civ state machine + fit/confirm/refine
cycle), `emergence` (emergent recognition templates from
confirmed `ThresholdStep` fits), `tool_emergence` (dynamic-tool
proposals from clustered confirmations).

`Hypothesizer`
([`hypothesizer.rs:85`](../sim/civ/src/discovery/hypothesizer.rs))
accumulates per-cell `(channel reading, fired?)` samples for a
fixed set of `(template, channel)` candidates, periodically
attempts fits across the form vocabulary, and confirms a relation
when a fit clears `exp(-1)` confidence. Confirmed relations are
first-class state on the civ. Key constants:
`SUSTAINED_TRIGGER_TICKS = 50` (refinement trigger),
`REFINEMENT_COOLDOWN_TICKS = 100`, `REVALIDATION_WINDOW_TICKS =
50` (inherited relations re-validate), `MAX_RESIDUAL_DEPTH = 3`,
`DEFAULT_FALSIFICATION_TRIGGER_TICKS = 30` (overwritten by civ-
aware callers via `streak_ticks_for_metabolism(120, metabolism)`).

A per-civ `Hypothesizer` lives on each named figure
([`figures.rs`](../sim/civ/src/figures.rs)); the pipeline drives
through the `observation` module which orchestrates per-figure
observe + step + cosmology hooks + form refresh per tick.

`MeasurementCandidate::residual_basis`
([`types.rs:37`](../sim/civ/src/discovery/types.rs)) lets a child
candidate fit the *residual* of an earlier confirmed measurement
— `direct_y − R.predict(x_R)`. The basis is frozen at compose
time so future refinement of the source doesn't retroactively
invalidate child fits.

Confirmed relations track a `falsification_streak` — when RMSE
on the latest sample window exceeds `1.5×` the confirm-time
residual, the streak increments. Once it crosses
`falsification_trigger_ticks` (metabolism-scaled), the law is
mispredicting fast enough that the civ skips the slower
confidence-streak path and force-triggers refinement immediately.
Successors re-validate inherited knowledge after
`REVALIDATION_WINDOW_TICKS`; failed re-fits emit
`RelationLapsed` and drop the relation. See
[discovery.md](discovery.md).

## Surplus / economy (M8)

`Civ::surplus`
([`lib.rs:419`](../sim/civ/src/lib.rs)) is a per-civ accumulator
of productive output above subsistence, stepped each tick by
`economy::step_surplus` in
[`sim/civ/src/economy.rs:80`](../sim/civ/src/economy.rs). Units
are dimensionless Q32.32 in the same scale as
`aggregate_population`.

Key knobs from
[`sim/civ/src/economy.rs`](../sim/civ/src/economy.rs):

| Constant | Value | Meaning |
|----------|-------|---------|
| `SURPLUS_UTILIZATION_FLOOR` | 0.70 | Cells above this begin accumulating |
| `SURPLUS_GAIN_PER_TICK` | 0.001 | Per-pop gain at full saturation |
| `SURPLUS_WAR_DRAIN_PER_TICK` | 0.0015 | Per-war drain |
| `SURPLUS_CATASTROPHE_DRAIN_FRAC` | 0.40 | Per-catastrophe drain |
| `SURPLUS_CEILING_FRAC` | 5 | Cap = 5 × aggregate pop |
| `SURPLUS_FOOD_BUFFER_BONUS` | 0.20 | Max food-security buffer contribution |
| `SURPLUS_WAR_BONUS_CAP` | 0.15 | Max war-strength modifier |
| `SURPLUS_EMIT_DELTA_FLOOR` | 50 | `CivSurplusChanged` gate (people) |

Consumers: `lifecycle::check_collapse_with_terrain` (food-
security buffer — surplus rides out lean ticks);
`conflict::strength` ([`war.rs:140`](../sim/civ/src/conflict/war.rs))
via `surplus_war_strength_modifier` (well-fed troops fight
better); `catastrophe::apply` via `drain_surplus_on_catastrophe`
(reserves absorb shocks first). `trade_flow_between` ferries
surplus between civs as part of the peaceful-diffusion path.

## ecological_resilience scalar from producer biomass (P0.5)

`Civ::ecological_resilience`
([`lib.rs:461`](../sim/civ/src/lib.rs)) is a Q32.32 scalar in
`[0, 2]`:

```text
ecological_resilience = clamp(producer_biomass / initial_producer_biomass, 0, 2)
```

`producer_biomass` is the live tier-0 primary-producer biomass
read from the `PlanetEcosystem` each tick (set by
`step_population_per_cell` before the cohort step).
`initial_producer_biomass` is captured at the civ's first step
call so each civ judges its own ecosystem against its own
baseline (successors re-anchor at their own founding).

`cell_capacity` consumes it to scale per-cell capacity by the
civ's share of the planet's producer pool. A biosphere crash
starves the civ; a thriving biosphere lifts cap.
`Event::CivResilienceTick` emits on drift ≥
`RESILIENCE_EMIT_DELTA_FLOOR = 0.05`
([`lib.rs:587`](../sim/civ/src/lib.rs)).

## DormantPool seed-bank survival (P1.3)

Covered above in [Catastrophes](#catastrophes-5-kinds).
`Civ::dormant_pool`
([`lib.rs:477`](../sim/civ/src/lib.rs)) is a per-civ seed-bank
for tardigrade-grade species — catastrophes seed it, 1%/tick
revive drains back into the active cohort capped at
`pre_catastrophe_population` (high-water mark). Empty by
default; only catastrophe-driven dormancy populates it. Successor
civs start fresh.

## 4-way CognitionTopology

The species' `CognitionTopology`
([`sim/species/src/types.rs:688`](../sim/species/src/types.rs))
selects between four organisational archetypes:

| Variant | Attempt period | Knowledge decay | Abstraction cap | Isolation penalty |
|---------|---------------|-----------------|-----------------|-------------------|
| `Centralized` | 1.0× | 1.0× | 1.0 | 1.0 |
| `DistributedRedundant` | 0.7× | 1.0× | 0.6 | 1.0 |
| `Collective` | 1.0× | 1.0× | 1.0 | 0.05 |
| `Acentric` | 5.0× | 0.2× | 1.0 | 1.0 |

Civ-level consumers:
`attempt_period_for_cognition_and_topology`
([`demographics.rs:214`](../sim/civ/src/demographics.rs)) folds
the topology multiplier into per-figure hypothesis cadence;
`drift.rs:280` applies the `isolation_penalty` when a Collective
species' civ population drops below `COLLECTIVE_QUORUM_POP` (a
single hive member without the swarm cannot think); cross-
generation transmission decay scales by `knowledge_decay_multiplier`
(Acentric species' substrate IS the memory); `abstraction_cap`
enforces a hard tier-3+ symbolic-structure ceiling for
DistributedRedundant species (no single integrator to synthesise
tier-3+ abstractions).

`Centralized` is the baseline (human-equivalent). `Acentric`
trades slow per-attempt cadence for cumulative substrate-encoded
knowledge that survives generations far better than oral or
cortical stores (slime-mold / xenobiological persistence
archetype).

## 7-variant Lifecycle dispatch

The species' `Lifecycle` enum
([`sim/species/src/types.rs:1064`](../sim/species/src/types.rs))
has seven variants. Each civ caches the matching `LifecycleState`
at founding via `configure_lifecycle_state`
([`founding.rs:235`](../sim/civ/src/founding.rs)) — caste
headcounts for `Eusocial`, biomass scalar for `Microbial` and
`Modular`, `LifecycleState::None` for the variants that read only
the 4-bracket cohort.

| Variant | Per-tick step |
|---------|---------------|
| `Vertebrate` | Legacy 4-bracket cohort (infant/juvenile/fertile/elder); bit-for-bit preserved |
| `Aquatic { semelparous }` | Metamorphosis bottleneck; semelparous = mass-spawn + 100% adult mortality, iteroparous = frog-like adult persistence |
| `Insect` | Egg/larva/pupa/adult — 4 distinct stages with per-stage lifespan and progression rate |
| `Eusocial { castes }` | Queen + worker castes; only `Reproductive` produces offspring; sterile castes consume + contribute economic/military weight |
| `Plant` | Seed/seedling/mature/senescent; high seed mortality, low senescent mortality, dispersal-driven seed flow to neighbour cells |
| `Microbial { fission_strategy }` | Doubling-time microbe — single biomass scalar; `fission_strategy` modulates doubling rate |
| `Modular` | Colonial / coral-equivalent — single biomass scalar that grows and dies as a unit; reproduction by budding |

Dispatch is routed through `step_for_lifecycle` in
`sim_population::lifecycle`, called by `capacity.rs` at the
per-cell step sites. Non-Vertebrate species run their real
lifecycle's step in production rather than the vertebrate
4-bracket step.

## Concurrent-civ knowledge diffusion

Peaceful concurrent civs can transmit relations to each other
(distinct from across-collapse transmission). Diffusion mechanics
share the comprehension-decay path (modulated by communication-
channel modality speed) — see [culture.md](culture.md) and
[discovery.md](discovery.md). Implementation:
`transmission::diffuse_between` in
[`sim/civ/src/transmission.rs:163`](../sim/civ/src/transmission.rs).

## Run-end is species-level

Civ collapse is **not** a run-end. The species persists, knowledge
artefacts persist, successor civs can emerge. Run-end conditions
are species-level: extinction, stagnation, transcendence, fixed
horizon. See
[architecture.md#run-end-taxonomy](architecture.md#run-end-taxonomy).

## Ages (emergent labels only)

Age labels are computed post-hoc from civ knowledge state and
surface in the post-run report timeline. Two civs may never share
the same labels — a sub-surface civ skips "Bronze" entirely and
lands in "Crystalline-Acoustic" or similar. Ages can span
multiple civs that share material culture. Logic in
[`sim/report/src/ages.rs`](../sim/report/src/ages.rs).
