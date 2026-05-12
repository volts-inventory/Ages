# Tech tree

A real DAG, not a flat list. 58 tools across 5 tiers. Each tool
defines five prereq fields and contributes to eight effect
categories. Both static (`ToolKind` enum) and emergent (species-
level dynamic registry) tools fold through the same effect
aggregator.

For deeper detail per crate, see
[`sim/civ/src/tech/`](../sim/civ/src/tech/) and
[`sim/civ/README.md`](../sim/civ/README.md).

## Five tiers

| Tier | Era | Examples (`ToolKind` variants) |
|------|-----|--------------------------------|
| 1 | Stone-age | `LocalisedCombustion`, `ContactWeapon`, `RangedMomentumWeapon`, `SimpleShelter`, `FoodProcessing` |
| 2 | Settlement-era | `BulkCultivation`, `AnimalSymbiosis`, `BulkStorage`, `MaterialRefining`, `ExperimentApparatus` |
| 3 | Pre-industrial | `MetalSmelting`, `Printing`, `OpticalLens`, `MagneticCompass`, sensorium tools (`ThermalSensor`, `RemoteAcoustic`, `FieldSensor`, `DistanceImaging`, `MagneticSensor`) |
| 4 | Industrial | `SteamPower`, `ChemicalSynthesis`, `Electromagnetism`, `InternalCombustion` |
| 5 | Information-age | `DigitalComputation`, `InformationNetworking`, `GeneticManipulation`, `OrbitalReach`, plus the transcendence trio `BioelectricResonator`, `FieldPropulsionEngine`, `MetamaterialLattice` |

The full registry (59 tools as of this writing) is in
[`sim/civ/src/tech/identity.rs`](../sim/civ/src/tech/identity.rs);
the example column above is illustrative, not exhaustive.

Tier-5 tools are narrative milestones rather than simulated
capabilities — the project's vision boundary holds (no
consciousness physics, no FTL). Their unlock gates the
`transcendence` run-end condition.

## Per-tool spec

Each tool carries a set of per-tool gating functions on `ToolKind`
(in [`sim/civ/src/tech/specs.rs`](../sim/civ/src/tech/specs.rs))
and contributes to eight effect categories
(in [`sim/civ/src/tech/effects.rs`](../sim/civ/src/tech/effects.rs)).
Per-tool `manipulation_prereqs` express which body-plan modes
suffice to fabricate each tool — replacing the prior global
`ToolExtension`-only gate. Tier-1 applied-knowledge tools accept
a broad palette (limbs, tentacles, mandibles, web-construct,
chemical-secretion, electric-discharge, etc.) so every body plan
has at least one tier-1 entry point; tier-2+ instrument tools and
`ExperimentApparatus` keep the strict `ToolExtension` requirement
so the "different sciences for different bodies" boundary lives
on instrument science rather than the entire tree.

### Gates

| Function | Meaning |
|----------|---------|
| `manipulation_prereqs` | Body-plan modes the species must possess at least one of. Empty slice = no manipulation gate. |
| `relation_prereqs` | `(template_id, channel)` pairs that must be confirmed in the civ's knowledge state. |
| `tool_prereqs` | Other `ToolKind`s that must already be unlocked. |
| `observation_threshold` | Minimum cumulative observation count on the civ's hypothesizer. |
| `literacy_floor` | Minimum civ literacy scalar `[0, 1]`. |
| `species_maturity_floor` | Minimum species-cumulative confirmed-relations count (gates tier-5). |
| `crust_prereqs` | Optional list of `Crust` variants the planet must carry (e.g. magnetic compass needs `RareEarth`). |
| `resource_prereqs` | Optional list of `(Substance, mass_fraction)` pairs from the cell's substance inventory. |

The DAG of `tool_prereqs` is invariant-checked at boot; cycles
panic.

### Effects

| Category | What it modifies |
|----------|------------------|
| `capacity` | Per-cell carrying capacity multiplier. |
| `food_crisis` | Resistance to food-crisis collapse. |
| `war_strength` | Per-cell skirmish weighting. |
| `seasonal_floor` | Below-threshold seasonal-capacity floor. |
| `catastrophe_resistance` | Severity reduction on catastrophes. |
| `literacy` | Multiplicative literacy bump. |
| `expansion_rate` | BFS territory expansion speed. |
| `transmission_fidelity` | Comprehension-decay slope on inheritance. |

Effects fold across all unlocked tools (static + emergent) via
the effect aggregator.

## Strict prereq path

A tool unlocks the moment all five prereq fields evaluate true
for the civ. `TechUnlocked` event emits with the tool kind, civ
id, and tick.

## Serendipitous unlocks

Parallel to the strict path: when **exactly one prereq is
missing**, a per-tick deterministic dice roll (base ~1e-5, scaled
by literacy and accumulated science) admits the unlock. Tech as
exploration network rather than a strict gate. Mirrors real
historical near-misses where the pieces almost-fit.

## Sensorium-extending tools

A subset of tier-2 / tier-3 tools extend the species sensorium —
when unlocked, latent recognition templates promote to
`perceivable-now`. Examples: `thermal_sensor`, `remote_acoustic`,
`field_sensor`, `distance_imaging`, `magnetic_sensor`.

Each has hybrid unlock gates: cumulative observation count +
literacy floor + relation prereqs:

| `ToolKind` | Observation threshold | Literacy floor |
|------------|----------------------:|---------------:|
| `ThermalSensor` | 30 000 | 0.20 |
| `RemoteAcoustic` | 30 000 | 0.20 |
| `FieldSensor` | 75 000 | 0.35 |
| `DistanceImaging` | 75 000 | 0.35 |
| `MagneticSensor` | 140 000 | 0.55 |

(Detail in [`sim/civ/src/tech/specs.rs`](../sim/civ/src/tech/specs.rs).)

## Experiment apparatus

`ToolKind::ExperimentApparatus` is a tier-2 capability tool that
gates **controlled-conditions intervention** alongside passive
observation. Closes the gap that real science is also
intervention, not only observation.

### Unlock prereqs

- Strict `manipulation_prereqs = [ToolExtension]` — the apparatus
  is the most demanding tier-2 instrument and keeps the prior
  global rule for itself. Pseudopod / chemical-secretion / web /
  burrow / jet species are excluded from this tool even though
  they can reach tier-1 applied knowledge through their native
  manipulation modes.
- Cumulative observation count ≥ 30 000.
- Literacy ≥ 0.30.
- Confirmed `fire` relation.

### Mechanics

The `sim_civ::apparatus` module ships
`Apparatus { cell, clamp_channel, measure_channel }` records (one
apparatus per civ at unlock). The allocator picks the lowest-
population claimed cell deterministically.

Per tick:

1. **Pre-physics** — `write_apparatus_clamps` overwrites the
   apparatus cell's clamp channel with one of four ladder values
   keyed by `tick % 4`.
2. **Physics integration** — runs as normal, with the clamp held
   for the tick.
3. **Post-physics** — `record_apparatus_samples` reads the cell's
   measure-channel and feeds the `(clamp, response)` pair into
   the first active figure's hypothesizer measurement track via
   `Hypothesizer::record_experimental_measurement`.

### Sample weighting

Apparatus measurements weigh **2×** in the fit accumulator and
increment the `experimental_count_by_relation` sidecar. When the
relation later confirms, `MeasurementConfirmed.is_experimental`
fires `true` if any apparatus contributions reached it.

### Linear-response sizing

Clamp ladders are sized to keep heat / fluid / charge
perturbations inside the linear-response regime so apparatus
presence doesn't violate the planet's energy budget over long
runs and doesn't overflow Q32.32 fit accumulators. The original
wider 250–500 K ladder overflowed accumulators on long smokes;
tightened to 250–360 K.

### Why it matters

A tactile-only `ToolExtension`-bearing species can derive its
planet's heat-diffusion `α` from a clean clamped-T-then-relax
experiment. A no-tool species (chemical secretion, pseudopod-
only) stays observation-only forever. Different sciences for
different bodies — the project's central theme stays sharp.

## Emergent tools

Static `ToolKind` enum stays in place; **dynamic tools live in a
parallel species-level registry**. When a civ accumulates ≥ 5
confirmed relations on a single channel, an `EmergentToolProposed`
event fires; if the cluster passes a stability check, a
`ToolDiscovered` event commits the tool with effects scaled by
cluster size.

Effect aggregators fold static + dynamic with the same combinator,
so a species with 4 emergent tools experiences them identically
to 4 unlocked static tools.

The dynamic registry persists across civ collapse boundaries (it's
species-scoped, not civ-scoped). Successor civs inherit the
species' tool canon.

## Tech-tree events

- `TechUnlocked(civ_id, tool_kind, via_serendipity)` — strict or
  serendipitous unlock.
- `ToolDiscovered(species_id, tool_id, channel, cluster_size,
  effects)` — emergent tool committed to the species canon.
- `TemplateDiscovered(species_id, template_id, signature, source)` —
  emergent template committed (see
  [recognition.md#emergent-recognition-templates](recognition.md#emergent-recognition-templates)).
- `MeasurementConfirmed(... is_experimental: true)` — a relation
  whose confirmation included apparatus contributions.
