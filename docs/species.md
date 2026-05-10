# Species

The species is the run's persistent unit. Civs come and go; the
species persists. Per-species traits drive sensorium, manipulation,
demographics, and cosmology baseline.

For deeper detail per crate:
[`sim/species/README.md`](../sim/species/README.md). For how
species traits gate which templates are perceivable see
[recognition.md](recognition.md). For how civs drift off the
species baseline see [civ.md](civ.md#per-civ-species-drift).

## Derivation order

At run start, after the planet samples and recognition library
filters by physical presence, the species is derived from
`{planet, regions, recognizable phenomena}`:

1. **Modality** — the dominant sense channel. Sampled weighted
   over planet conditions: aquatic worlds favour pressure,
   sub-surface worlds favour echolocation, etc.
2. **Manipulation** — the dominant manipulation mode. `Hand`,
   `Tentacle`, `Pseudopod`, `ToolExtension` (prerequisite for
   experiment apparatus), `ChemicalSecretion`.
3. **Habitat** — `Surface`, `Aquatic`, `Subsurface`, `Aerial`,
   etc.
4. **Cognition topology** — `Centralized`, `Distributed`,
   `Hivemind`, etc., paired with a tier (low / medium / high).
   High-cognition species cycle hypothesis attempts faster.
5. **Sociality** — `Solitary`, `Pair`, `Pack`, `Colony`,
   `Eusocial`. Drives founding floor and breakaway thresholds.
6. **Communication channel** + fidelity — drives literacy
   floors and inter-civ comprehension.
7. **Lifespan** — sampled and feeds birth/death rate scaling.
8. **Cosmology bias (`initial_cosmology`)** — anchors the
   species' deep worldview baseline.

Trait sampling lives in `sim/species/src/sampling.rs`; the entry
point is `sim_species::derive`.

## Sensorium gating

Recognition templates split into `perceivable-now` and `latent`
based on the species' sensorium. A photic species can perceive
visible-light templates by default; a tactile-only species cannot
detect them until it invents a sensorium-extending tool (telescope,
photic-sensor, microscope). When the unlock fires, latent templates
promote to perceivable-now and recognition begins firing them.

Sensorium-tool unlock gates on a hybrid threshold: cumulative
observation count + literacy floor + relation prereqs. Per-tool
detail in [tech.md](tech.md).

## Cognition-derived discovery cadence

High-cognition species cycle hypothesis attempts faster — the
hypothesizer's per-tick attempt budget scales with cognition
tier. A high-cognition civ can chase several rivals
simultaneously; a low-cognition civ waits multiple ticks between
attempts.

## Lifespan-relative population rates

Birth and death rescale by `80 / lifespan_years` clamped
`[0.25, 4.0]` — a 200-year species reproduces at 0.4×; a 20-year
species at 4×. No homo-sapiens-pinned defaults.

Lifespan also feeds the literacy modifier: shorter lifespans → less
intergenerational transfer per individual → lower comprehension on
inherited artefacts, all else equal.

## Substrate-derived demographics

Founding floor, carrying capacity per fuel-unit, migration
pressure threshold, and birth-rate biosphere multiplier all derive
from `(biosphere, gravity, cognition, sociality, axial_tilt,
luminosity)`. Detail in [population.md](population.md).

## Per-civ drift

Each civ has cognition / sociality / lifespan / communication-
fidelity deltas off the species baseline. Inaugural civs zero
them; each successor inherits its parent's deltas plus a
deterministic per-generation perturbation. The species drifts as
it begets successors. `SpeciesDrift` events emit on each new
civ's deltas.

A long species history thus produces a *lineage* of related civs
where the underlying species itself slowly mutates.

## Cosmology baseline + religion

The species carries an `initial_cosmology` bias that anchors all
of its civs' five-axis cosmology — the *slow-drift, deep
worldview* layer. On top of that, each civ founds with its own
three-axis religion vector (theology / ritual / sacred-time) that
drifts much faster. See [culture.md](culture.md) for the
mechanics.

## Species-level events

- `SpeciesDerived` — emitted at run start with full trait set.
- `SpeciesNomadsChanged` — nomadic-pool population shifts.
- `SpeciesDrift` — per-civ drift deltas off the baseline.
- `SpeciesCosmologyBias` — emitted with the species'
  `initial_cosmology`.
- `TemplateDiscovered` — species canon adopts a new emergent
  template (see [recognition.md#emergent-recognition-templates](recognition.md#emergent-recognition-templates)).
- `ToolDiscovered` — species canon adopts a new emergent tool (see
  [tech.md#emergent-tools](tech.md#emergent-tools)).
