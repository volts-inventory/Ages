# Tech tree

A real DAG, not a flat list. **71 static tools across 5 tiers**
plus a **per-species dynamic registry** for emergent tools civs
propose from their own observational regularities. Both the static
`ToolKind` enum and the dynamic registry fold through the same
effect aggregator — a species with 4 emergent thermal-instrument
tools experiences them identically to 4 unlocked static tools.

For deeper detail see
[`sim/civ/src/tech/`](../sim/civ/src/tech/) and
[`sim/civ/README.md`](../sim/civ/README.md). Per-tool gates live in
[`sim/civ/src/tech/specs/`](../sim/civ/src/tech/specs/); the gating
predicates that consume them are in
[`sim/civ/src/tech/gating.rs`](../sim/civ/src/tech/gating.rs).

## Five tiers

| Tier | Era | Headline `ToolKind`s |
|------|-----|----------------------|
| 1 | Stone-age / foraging | `LocalisedCombustion`, `ContactWeapon`, `RangedMomentumWeapon`, `SimpleShelter`, `FoodProcessing`, `FluidGathering`, `BasicTextiles`, `StoneWorking`, `OrganizedHunting`, `BasicHealing` |
| 2 | Settlement-era | `BulkCultivation`, `AnimalSymbiosis`, `BulkStorage`, `MaterialRefining`, `CulturalEncoding`, `FluidControl`, `WatercraftConstruction`, `PermanentMasonry`, `TradeNetworks`, `UrbanConstruction`, `ExperimentApparatus`, `ThermalSensor`, `RemoteAcoustic`, `HerbalMedicine`, `AnimalHusbandry`, `PreservedFood`, `WindPower` |
| 3 | Pre-industrial | `ChemicalProjectile`, `PrecisionTimekeeping`, `MechanicalAdvantage`, `LongRangeNavigation`, `WrittenJurisprudence`, `AbstractMathematics`, `ArtisanalSpecialisation`, `DefensiveFortification`, `MotivePropulsion`, `AmphibiousConstruction`, `FieldSensor`, `DistanceImaging`, `AcousticEngineering`, `HydraulicWorks`, `CodexTradition` |
| 4 | Industrial | `Mechanisation`, `LongRangeCommunication`, `ChemicalSynthesis`, `MedicalIntervention`, `AdvancedMaterials`, `HeavyTransport`, `PowerGeneration`, `AnalyticalEngines`, `MassLiteracy`, `AerialTransport`, `MagneticSensor`, `PrecisionInstruments`, `DistributedNetworks`, `BiomimeticDesign`, `GeneCultureCoevolution` |
| 5 | Information-age | `DigitalComputation`, `InformationNetworking`, `GeneticManipulation`, `OrbitalReach`, `AdvancedMedicine`, `MaterialFabrication`, `AutonomousSystems`, `EnergyStorage`, `CryogenicEngineering`, `OrganicSynthesis`, `EcosystemEngineering`, plus the transcendence trio `BioelectricResonator`, `FieldPropulsionEngine`, `MetamaterialLattice` |

The full registry is in
[`sim/civ/src/tech/identity.rs`](../sim/civ/src/tech/identity.rs)
(see `ToolKind::ALL`, line 10). Tier-1 tools chain through no
prereqs (parallel, observation-only); each subsequent tier builds
on strict-lower-tier `tool_prereqs`. The DAG invariant is
checked at boot by the `tool_prereqs_form_a_dag` test.

Tier-5 tools split into two groups. The 11 information-age tools
(`DigitalComputation` through `EcosystemEngineering`) are mostly
extrapolations of tier-4 chains. The transcendence trio
(`BioelectricResonator` / `FieldPropulsionEngine` /
`MetamaterialLattice`) are narrative milestones — the project's
vision boundary holds (no consciousness physics, no FTL). All
tier-5 tools share the species-cumulative `species_maturity_floor`
of 3000 confirmed relations
([`specs/mod.rs:416`](../sim/civ/src/tech/specs/mod.rs)).

## Per-tool spec

Each tool carries eight unlock gates (in
[`sim/civ/src/tech/specs/`](../sim/civ/src/tech/specs/)) and
contributes to up to thirteen effect categories (in
[`sim/civ/src/tech/effects.rs`](../sim/civ/src/tech/effects.rs)).
The unlock predicate is `is_unlocked` in
[`sim/civ/src/tech/gating.rs:162`](../sim/civ/src/tech/gating.rs);
the strict-path serendipity-roll counterpart is
`serendipity_missing_prereqs` at line 243 of the same file.

### Gates

| Function | Meaning |
|----------|---------|
| `manipulation_prereqs` | Body-plan modes the species must possess at least one of. Empty slice = no manipulation gate. Replaces the retired global `ToolExtension`-only gate so chemical-secretion / web-construct / fluid-jet / burrow / electric-discharge bodies can reach tier-1 applied knowledge. ([`specs/manipulation.rs`](../sim/civ/src/tech/specs/manipulation.rs)) |
| `relation_prereqs` | `(template_id, ChannelKind)` pairs that must be confirmed in the civ's hypothesizer. The lookup is template-level (template-id match); the `ChannelKind` documents the narrative sensory modality. ([`specs/relations.rs`](../sim/civ/src/tech/specs/relations.rs)) |
| `tool_prereqs` | Other `ToolKind`s that must already be unlocked. ([`specs/tools.rs`](../sim/civ/src/tech/specs/tools.rs)) |
| `min_civ_confirmed_relations` | Tier-keyed civ-maturity floor — confirmed firing + measurement relations across the civ's active figures. tier-1: 0; tier-2: 5; tier-3: 15; tier-4: 50; tier-5: 200. ([`specs/mod.rs:71`](../sim/civ/src/tech/specs/mod.rs)) |
| `min_civ_experimental_relations` | Of the civ's confirmed relations, how many must be **apparatus-supported** (`ConfirmedMeasurement.is_experimental == true`). tier-≤ 2: 0; tier-3: 3; tier-4: 12; tier-5: 80. A civ that never builds `ExperimentApparatus` tops out at tier-2 by construction. ([`specs/mod.rs:191`](../sim/civ/src/tech/specs/mod.rs)) |
| `literacy_floor` | Civ's `literacy_score()` minimum. Range from 0 (tier-1 + `CulturalEncoding`) through 0.15 / 0.30 / 0.50 / 0.55 / 0.65 across tier-2 / tier-3 / tier-4 / transcendence / information-age. ([`specs/mod.rs:279`](../sim/civ/src/tech/specs/mod.rs)) |
| `species_maturity_floor` | Species-cumulative confirmed-relations count. 0 for tier-≤ 4; 3000 for every tier-5 tool. ([`specs/mod.rs:416`](../sim/civ/src/tech/specs/mod.rs)) |
| `crust_prereqs` | Optional list of `Crust` variants the planet must carry. `FieldPropulsionEngine` needs `Piezoelectric` / `Ferrous` / `RareEarth`; `MetamaterialLattice` needs `Piezoelectric` / `RareEarth`. ([`specs/mod.rs:443`](../sim/civ/src/tech/specs/mod.rs)) |
| `resource_prereqs` | Optional list of `(Substance, summed-density floor)` pairs against the civ's claimed-cell substance inventory. Combustion-derived chains gate on `Fuel`; petrochemistry on `Fossil`; water control on `Water`. ([`specs/mod.rs:481`](../sim/civ/src/tech/specs/mod.rs)) Hard gate — no serendipity bypass. |

### Effects

| Category | What it modifies |
|----------|------------------|
| `capacity_multiplier` | Per-cell carrying capacity multiplier (`BulkCultivation` ×5, `Mechanisation` ×10). |
| `food_crisis_resistance_bonus` | Lowers the food-crisis collapse threshold. |
| `war_strength_bonus` | Per-cell skirmish weighting. |
| `seasonal_floor_bonus` | Below-threshold seasonal-capacity floor. |
| `catastrophe_resistance_bonus` | Severity reduction on catastrophes. |
| `literacy_bonus` | Additive lift to civ literacy. |
| `expansion_rate_bonus` | BFS territory expansion speed. |
| `transmission_fidelity_bonus` | Comprehension-decay slope on inter-civ inheritance. |
| `discovery_rate_bonus` | Multiplier on the hypothesizer's fit cadence. |
| `cohesion_bonus` | Lifts cohesion equilibrium (delays civil war / breakaway). |
| `migration_speed_bonus` | Per-tick fraction of fertile adults redistributing under pressure. |
| `fertility_bonus` | Per-tick birth-rate multiplier. |
| `lifespan_extension_factor` | Biological lifespan extension (modern medicine only). |
| `mortality_reduction_per_bracket` | `[infant, juvenile, fertile, elder]` per-tick mortality cuts. |

Effects fold across all unlocked tools (static + dynamic) via the
civ-level aggregators in
[`sim/civ/src/tools.rs`](../sim/civ/src/tools.rs).

## Per-substrate availability

Substrate divergence is enforced through three orthogonal gates:

1. **`relation_prereqs`** — a no-fire seed (deep-ocean methane /
   ammonia world that never observes ignition) genuinely never
   confirms the `fire` template (id 1) and so cannot unlock
   `LocalisedCombustion`, `FoodProcessing`, `BulkStorage`,
   `MaterialRefining`, `ChemicalProjectile`, `ChemicalSynthesis`,
   `AdvancedMaterials`, `MaterialFabrication`, or `OrganicSynthesis`.
   It reaches industry via the alternate `MechanicalAdvantage` →
   `Mechanisation` path (tidal mechanics → leverage → engines),
   skipping the combustion chemistry branch entirely.
2. **`resource_prereqs`** — `LocalisedCombustion` needs ≥ 1 unit
   of summed `Substance::Fuel` across claimed cells; petrochemistry
   tools demand ≥ 1–5 of `Substance::Fossil` (only `Crust::Hydrocarbon`
   seeds this). The check sums substance density across all claimed
   cells via `claim_substance_total`
   ([`gating.rs:31`](../sim/civ/src/tech/gating.rs)) — non-extractive
   (checking density doesn't deplete it).
3. **`crust_prereqs` / `is_buildable`** — `MagneticSensor` and
   `FieldPropulsionEngine` need `has_magnetosphere`; `FieldSensor`
   needs `has_atmosphere_or_ocean`; `FieldPropulsionEngine` /
   `MetamaterialLattice` additionally need specific crust variants.

The combined effect: an aqueous-Lush civ has the broadest tool
catalogue; a piezoelectric / silicate civ trades the combustion
branch for the field-coupled transcendence chain;
hydrocarbon-substrate civs uniquely access `ChemicalSynthesis` /
`MaterialFabrication` / `OrganicSynthesis`.

## Tech multiplier

`discovery_rate_bonus` (summed across unlocked tools) multiplies
the hypothesizer's `attempt_period` divisor. A civ with
`ExperimentApparatus` (+0.10), `AbstractMathematics` (+0.10),
`AnalyticalEngines` (+0.15), `PrecisionInstruments` (+0.15) runs
the fit loop at ~1.5× cadence and tightens the per-fit tolerance
window through cleaner samples. The threading is via
`Hypothesizer::step_with_cosmology_doubt_and_rate` in
[`sim/civ/src/discovery/hypothesizer.rs:690`](../sim/civ/src/discovery/hypothesizer.rs).

## Strict prereq path

`is_unlocked` returns true iff every gate above passes
simultaneously:

1. `is_buildable` (manipulation + native-channel + crust + planet
   feature).
2. `civ_confirmed_count ≥ min_civ_confirmed_relations`.
3. `civ_experimental_count ≥ min_civ_experimental_relations`.
4. `civ_literacy ≥ literacy_floor`.
5. Every `(template_id, _)` in `relation_prereqs` has at least one
   matching confirmed relation.
6. Every `ToolKind` in `tool_prereqs` is present in
   `unlocked_tools`.

`TechUnlocked(civ_id, tool_id, via_serendipity=false)` emits.

## Serendipitous unlocks

Real innovation has "lucky leapfrogs": Newton spots the apple
before Principia, alchemists stumble onto chemistry before
atomic theory. `serendipity_missing_prereqs`
([`gating.rs:243`](../sim/civ/src/tech/gating.rs)) returns
`Some(1)` when:

- `is_buildable` passes (hard physics + biology gates are
  irrelaxable).
- The civ-maturity floors pass (no leapfrogging past
  experimentation + time).
- The literacy floor passes at 75% of the strict requirement.
- **Exactly one** prereq is missing across `relation_prereqs` +
  `tool_prereqs` combined.

When all hold, `serendipity_roll`
([`gating.rs:329`](../sim/civ/src/tech/gating.rs)) does a per-tick
deterministic dice roll keyed on
`(planet_seed, civ_id, tool_id, tick)` via ChaCha20. Base
probability is `2.5e-5` per tick, scaled by
`(0.5 + 1.5 × literacy)` and `min(5, 1 + confirmed_relations/100)`,
clamped to `[1e-6, 1e-3]`. A mature civ sees ~1.5–3 serendipitous
unlocks per lifetime across the 71-tool catalogue —
"Newton's-apple beats" become recognisable narrative features.

`TechUnlocked(civ_id, tool_id, via_serendipity=true)` emits.
Resource-prereq gates do **not** participate in serendipity; the
substrate must be present in territory.

## Sensorium-extending tools

Five `ToolKind`s grant new perceptual `ChannelKind`s on unlock
(via `granted_channels` in
[`identity.rs:493`](../sim/civ/src/tech/identity.rs)). Latent
recognition templates (templates the species can't natively
perceive) promote to perceivable-now and the recognition layer
begins firing them for the civ:

| `ToolKind` | Tier | Grants | Civ-maturity floor | Literacy floor |
|------------|-----:|--------|-------------------:|---------------:|
| `ThermalSensor` | 2 | `InfraredThermal` | 5 confirmed | 0.20 |
| `RemoteAcoustic` | 2 | — (range extension) | 5 confirmed | 0.20 |
| `FieldSensor` | 3 | `ElectricField` | 15 confirmed + 3 experimental | 0.35 |
| `DistanceImaging` | 3 | — (range extension) | 15 confirmed + 3 experimental | 0.35 |
| `MagneticSensor` | 4 | `MagneticSense` | 50 confirmed + 12 experimental | 0.55 |

`ThermalSensor` / `RemoteAcoustic` are tier-2 (the spec was
relaxed during F-wave when civs stalled at tier-2 across slow
substrates). See [recognition.md](recognition.md) for how granted
channels promote latent templates to perceivable.

## Experiment apparatus

`ToolKind::ExperimentApparatus` is the **tier-2 gateway to
intervention-supported epistemology**. Pre-apparatus civs only
observe; with apparatus, they clamp a physics channel on one of
their cells through a four-value ladder and read the response.

### Unlock prereqs

- `manipulation_prereqs` accepts every `ManipulationKind` — a
  clamp-and-measure rig is a function, not a body-plan-specific
  form. `ChemicalSecretion` runs controlled-concentration baths,
  `WebConstruct` weaves a calibrated chamber, `FluidJet` holds a
  pressure clamp, `ElectricDischarge` clamps field strength,
  `Burrow` excavates a controlled-volume cell.
- `min_civ_confirmed_relations = 5`, `literacy_floor = 0.30`.
- Relation prereq: confirmed `tidal_extremum` (template id 14) on
  the `Tactile` channel. Swapped from the earlier `fire` prereq
  during F-wave because `fire` is locked to oxidising atmospheres;
  every habitable world produces tidal periodicity in deep water
  ([`specs/relations.rs:295`](../sim/civ/src/tech/specs/relations.rs)).

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
   `Hypothesizer::record_experimental_measurement`
   ([`hypothesizer.rs:317`](../sim/civ/src/discovery/hypothesizer.rs)).

### Sample weighting

Apparatus measurements are pushed into the fit buffer **twice**
(reflecting the higher information-density of controlled samples)
and increment `experimental_count_by_relation`. When the relation
confirms, `ConfirmedMeasurement.is_experimental` fires true if
any apparatus contributions reached it — the civ's epistemology
was intervention-supported even if observation samples co-dominate
the fit pool.

### Linear-response sizing

Clamp ladders are sized to keep heat / fluid / charge perturbations
inside the linear-response regime so apparatus presence doesn't
violate the planet's energy budget over long runs and doesn't
overflow Q32.32 fit accumulators. The original wider 250–500 K
ladder overflowed on long smokes; tightened to 250–360 K.

### Why it matters

A tactile-only `ToolExtension`-bearing species derives its planet's
heat-diffusion `α` from a clean clamped-T-then-relax experiment.
A no-tool species (chemical-secretion / pseudopod-only) stays
observation-only forever. "Different sciences for different
bodies" plays out on **which late-game branches a species can
fabricate**, not on whether it does science at all.

## Emergent tools (dynamic registry)

The static `ToolKind` enum stays in place; **dynamic tools live in
a parallel species-level registry**
(`Species::dynamic_tool_registry`). The discovery rule is in
[`sim/civ/src/discovery/tool_emergence.rs`](../sim/civ/src/discovery/tool_emergence.rs).

### Discovery rule

Per-civ check at every `TOOL_EMERGENCE_CHECK_PERIOD_TICKS = 240`
(`is_tool_emergence_tick`, line 132). Substrate-aware variant
stretches the period by the inverse of the planet's metabolism.

1. Group the civ's confirmed relations by `Channel`.
2. For each channel with ≥ `EMERGENT_TOOL_CLUSTER_SIZE = 5`
   confirmed relations:
   - **Refinement gate** — if the species already has a dynamic
     tool keyed on this channel, the new cluster must have grown
     by ≥ `EMERGENT_TOOL_REFINEMENT_STEP = 5` templates beyond the
     prior tool's cluster. Lets a civ progress from 5-template
     thermal apparatus → 10-template refined thermal apparatus →
     15-template apparatus, etc.
   - Mint a `DynamicTool` with effects scaled by cluster size
     (saturating at `EMERGENT_TOOL_MAX_SCALE_CLUSTER = 20`).

### Effect flavour by channel

Every channel shares a scientific-instrument baseline (capacity
×1.20 cap, +0.05 literacy, +0.05 transmission). On top, each
channel adds a flavour profile so emergent tools carry the
character of *what the species figured out*
([`tool_emergence.rs:182`](../sim/civ/src/discovery/tool_emergence.rs)):

| Channel | Flavour |
|---------|---------|
| `Temperature` | Seasonal floor + catastrophe resistance (thermal buffering). |
| `ChargeMagnitude` | Discovery-rate bonus (EM instrumentation). |
| `WaterDepth` | Food-crisis + migration speed (hydrology). |
| `Elevation` | Expansion rate (cartography). |
| `Fuel` | Capacity stack (combustion engineering). |
| `Oxidiser` | War strength (propellants). |
| `Vapour` | Infant + juvenile mortality (sanitation leap). |
| `Ice` | Seasonal floor + catastrophe (cryogenic preservation). |
| `Fossil` | Capacity stack (fossil-fuel energy). |
| `MagneticField` | Expansion rate + discovery rate (compasses). |

Magnitudes are intentionally smaller than the static `ToolKind`
headlines so emergent tools complement rather than upstage
authored tools.

### Persistence

The dynamic registry is **species-scoped** (not civ-scoped). Once
a species mints `dynamic_thermal_apparatus_civ_3_t1200`, successor
civs of the same species inherit it — the species' tool canon
persists across civ collapse boundaries. Substance-channel
clusters carry a `resource_prereqs` floor of 1 unit summed across
territory; abstract-channel clusters carry none.

## Tech-tree events

- `TechUnlocked { civ_id, tool_id, via_serendipity }` — strict or
  serendipitous unlock.
- `EmergentToolProposed { species_id, channel, cluster_size, … }` —
  pre-mint signal for the report.
- `ToolDiscovered { species_id, tool_id, channel, cluster_size,
  effects }` — emergent tool committed to the species canon.
- `TemplateDiscovered { species_id, template_id, signature, source }` —
  emergent recognition template committed (see
  [recognition.md#emergent-recognition-templates](recognition.md#emergent-recognition-templates)).
- `MeasurementConfirmed { … is_experimental: bool }` — a relation
  whose confirmation included apparatus contributions.

The post-run report walks these to render per-civ tech chapters
with the order tools unlocked, which were serendipitous, and what
emergent tools the species discovered through its own
observational regularities.
