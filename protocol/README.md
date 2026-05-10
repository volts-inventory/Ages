# protocol

JSON schemas for events and snapshots. **Single source of truth for
the sim ↔ consumer contract.** Rust types are derived via `serde`
from the schemas; downstream consumers (post-run report generator,
ad-hoc analysis scripts, future renderers) parse against the same
schemas.

## Hard rule

Changes to sim events go through `protocol/`. **No ad-hoc fields**
on either side.

## Schema versioning

Bumped on incompatible changes. The current `SCHEMA_VERSION` constant
is emitted in the `RunStart` header so a consumer can refuse to parse
a newer-version log it doesn't understand.

## Notable event fields

- **`PlanetDerived.crust`** — snake-case tag for `sim_world::Crust`
  (`basaltic` / `hydrocarbon` / `piezoelectric` / `ferrous` /
  `rare_earth`). Drives fuel-density placement and gates which
  late-game tools the species can build.
- **`PlanetDerived.axial_tilt_deg_q32`** + **`day_length_hours_q32`**
  — `Q32.32` raw bits. Currently flavour-only; reserved for
  seasonal/diurnal physics.
- **`SpeciesDerived.cognition_topology`** — `centralized` or
  `distributed`. Surfaces the vertebrate-vs-cephalopod substrate
  distinction so reports can characterise the species.
  `Distributed` species also get a +10% cognition trait bump that
  ripples through tolerance / minimum-sample formulas.
- **`FigureBorn`** — named-figure event with personality
  scalars (`charisma_q32`, `curiosity_q32`, `doubt_q32`,
  `communicativeness_q32`). `doubt` scales refinement
  aggressiveness; `communicativeness` boosts transmission
  comprehension.
- **`CatastropheFired.catastrophe_kind`** — five kinds:
  `volcanic` / `disease` / `asteroid` / `solar_flare` / `ice_age`.
- **`PlanetMap`** — per-cell elevation + water_depth in row-major
  order, emitted once at run start so the post-run report can
  draw an ASCII map of the world.
- **`Snapshot`** — periodic state-digest checkpoint (every 500
  ticks). See **Snapshots** below.
- **Run-end reasons** (`RunEnd.reason`): `species_extinction` /
  `stagnation` / `transcendence` / `fixed_horizon` / `user_stop`.
- **`MeasurementConfirmed.is_experimental`** — `true` when at
  least one sample in the relation's fit pool came from a civ-built
  experiment apparatus (`ToolKind::ExperimentApparatus`); `false`
  for purely observational fits. `#[serde(default)]` for back-compat
  with pre-apparatus event logs.

## Snapshots

`Snapshot` is one of the four output channels named in the project
vision. It's a state-digest event the sim emits every
`sim_core::SNAPSHOT_INTERVAL_TICKS` (currently 500): tick, active +
collapsed civ ids, total species population, running totals for
confirmed relations / refinements / catastrophes / tech unlocks /
knowledge transmissions / knowledge diffusions. It's not a full
sim-state checkpoint (no physics grid, no per-figure hypothesizers)
— just a coarse digest useful for cross-checking event-derived
totals and providing fast-forward anchors for offline tools. A full
replay snapshot would need a separate, larger payload; defer until a
consumer actually needs it.

## Cited by

`docs/architecture.md` (event vs snapshot model, post-run report).
