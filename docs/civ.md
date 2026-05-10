# Civilization model

A civilization is a bounded collectivity within a species — it
founds, runs its course, often collapses, and is succeeded by other
civs that inherit its territory and (partial) knowledge. Multiple
civs may exist concurrently. The species persists; civs come and go.

For deeper detail per crate:
- Civ entity + lifecycle + culture + tech + catastrophes →
  [`sim/civ/README.md`](../sim/civ/README.md).
- Cohorts + population → [`sim/population/README.md`](../sim/population/README.md).

For the world the species lives in see [world.md](world.md).
For the discovery pipeline see [discovery.md](discovery.md). For
cosmology / religion / war see [culture.md](culture.md). For tech
and tools see [tech.md](tech.md). For catastrophes see
[catastrophes.md](catastrophes.md).

## Species as the persistent unit

The **species** is the run's persistent unit. Civilizations are
bounded collectivities within the species that come and go. Real
history works that way — Sumer → Akkad → Babylon → ... → modern,
all on the same species, with dark ages and renaissances between —
and so does this sim.

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

## Founding

No hardcoded "inaugural civ". Civs found themselves emergently when:

- A nomadic region accumulates sufficient density.
- The species has crossed the relevant tech-readiness gate.
- A habitable centroid cell is available (the founding helper
  `pick_habitable_cell` relocates the figure-derived founding
  centroid off uninhabitable terrain).

Inaugural founding emits `CivFounded` with `parent_civ_id = None`;
breakaway and stateless re-founding paths emit a `parent_civ_id`
when the new civ inherits state.

A successor's `territory_centroid` is shifted off any collision
with the parent's — preferring an adjacent hex neighbour, else any
other claimed cell, else the figure-derived fallback — so the
viewport's smallest-claimed-cell heuristic resolves to distinct
centroid letters.

## Per-civ species drift

Each civ has cognition / sociality / lifespan / communication-
fidelity deltas off the species baseline. Inaugural civs zero them;
each successor inherits its parent's deltas plus a deterministic
per-generation perturbation. The species drifts as it begets
successors. `SpeciesDrift` events emit on each new civ's deltas.

## Internal life: cohesion + civil war

A per-civ `[0, 1]` cohesion scalar tracks internal stability. It
drifts toward an equilibrium driven by:

- **Size** — larger civs lose cohesion faster.
- **Food security** — chronic shortfalls erode cohesion.
- **Dogmatism** — high cosmology dogmatism stabilizes within-civ
  but punishes heretical confirmations.
- **Literacy** — record-keeping holds the polity together.

When cohesion stays below 0.10 for 75 baseline-years, the civ
collapses with reason `civil_war`. `CohesionShifted` events emit on
≥ 0.05 absolute drift.

## Breakaway

Two breakaway paths, both producing a successor civ that inherits
parent state:

- **Cohesion-driven breakaway.** When cohesion sits in
  `[0.10, 0.35]` for 40 baseline-years, the civ forks: a regional
  faction takes 30% of the parent's population and starts at 0.85
  cohesion (fresh authority); the parent recovers +0.15
  (disgruntled faction left).
- **Dogmatic breakaway.** When the parent's cosmology dogmatism
  axis exceeds a threshold and a heretical hypothesis is force-
  rejected, a fork emerges centered on the heretical view.

## Collapse

Per-civ collapse reasons (separate enum from run-end reasons):

- `food_crisis` — sustained capacity shortfall.
- `cultural_lock` — stagnation under high dogmatism.
- `conflict` — wartime population loss past defeat floor.
- `civil_war` — cohesion-driven (above).
- `catastrophe` — direct loss to a cell-localized event.
- `territory_too_small` — `claimed_cells.len() == 1` for ≥ 24
  baseline-months (auto-collapse on territory loss).

Cell-by-cell loss in war is accumulated against
`CONFLICT_DEFEAT_FLOOR`; per-skirmish caps prevent flapping.

## Successor + stateless re-founding

When a civ collapses, its territory unclaims and its named figures
die. Knowledge can survive across the boundary (see
[discovery.md](discovery.md#transmission-and-mythologization) and
the inheritance window).

Stateless re-founding admits a new civ centred on a remnant cohort
when:

- Stateless cohort population ≥ founding floor.
- Most recent collapse within `RECENT_REMNANT_WINDOW` (currently
  3000 ticks = 250 baseline-years).
- At least `FOUNDING_MIN_DARK_AGE` (600 ticks = 50 baseline-years)
  have passed since collapse.

## Active law engagement

Confirmed relations track a falsification streak — sustained
mispredictions force refinement faster than the residual-only path
(`RelationFalsified`). Successors re-validate inherited knowledge
after a 50-tick window; failed re-fits emit `RelationLapsed` and
drop the relation. Wrong knowledge dies; right knowledge
strengthens. Detail in [discovery.md](discovery.md).

## Inter-civ contact

Two civs make contact when their claimed-cell sets touch (Manhattan
distance ≤ 1) for the first time. `CivContact` emits, recording
both civ ids. Contact is a hard prerequisite for war — the kinship-
weighted belligerence machinery (see [culture.md](culture.md))
gates on `emitted_contacts` containing the pair.

## Concurrent-civ knowledge diffusion

Peaceful concurrent civs can transmit relations to each other
(distinct from across-collapse transmission). Diffusion mechanics
share the comprehension-decay path — see
[discovery.md](discovery.md).

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
lands in "Crystalline-Acoustic" or similar. Ages can span multiple
civs that share material culture. Logic in
[`sim/report/src/ages.rs`](../sim/report/src/ages.rs).
