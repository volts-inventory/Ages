# Tech tree

A real DAG, not a flat list. **71 static `ToolKind` variants** across
five tiers plus a parallel **per-species dynamic tool registry** that
mints tools at runtime when a civ's hypothesizer accumulates a deep
enough cluster of confirmed relations on a single channel. Both
catalogues fold through the same effect aggregator; both consume
substrate density through the same mass-conservative chemistry mirror.

For deeper detail per crate, see
[`sim/civ/src/tech/`](../sim/civ/src/tech/) and
[`sim/civ/README.md`](../sim/civ/README.md). The authoritative `ToolKind`
list lives in
[`sim/civ/src/tech/identity.rs`](../sim/civ/src/tech/identity.rs)
(the `ALL` constant at line 10 carries 71 entries).

## Five tiers

| Tier | Era | Examples (`ToolKind` variants) |
|------|-----|--------------------------------|
| 1 | Stone-age / animal-level tech | `LocalisedCombustion`, `ContactWeapon`, `RangedMomentumWeapon`, `SimpleShelter`, `FoodProcessing`, `FluidGathering`, `BasicTextiles`, `StoneWorking`, `OrganizedHunting`, `BasicHealing` |
| 2 | Settlement-era + capability instruments | `BulkCultivation`, `AnimalSymbiosis`, `BulkStorage`, `MaterialRefining`, `CulturalEncoding`, `FluidControl`, `WatercraftConstruction`, `PermanentMasonry`, `TradeNetworks`, `UrbanConstruction`, `ThermalSensor`, `RemoteAcoustic`, `ExperimentApparatus`, `HerbalMedicine`, `AnimalHusbandry`, `PreservedFood`, `WindPower` |
| 3 | Pre-industrial + sensorium tier-3 | `ChemicalProjectile`, `PrecisionTimekeeping`, `MechanicalAdvantage`, `LongRangeNavigation`, `WrittenJurisprudence`, `AbstractMathematics`, `ArtisanalSpecialisation`, `DefensiveFortification`, `MotivePropulsion`, `AmphibiousConstruction`, `FieldSensor`, `DistanceImaging`, `AcousticEngineering`, `HydraulicWorks`, `CodexTradition` |
| 4 | Industrial + sensorium tier-4 | `Mechanisation`, `LongRangeCommunication`, `ChemicalSynthesis`, `MedicalIntervention`, `AdvancedMaterials`, `HeavyTransport`, `PowerGeneration`, `AnalyticalEngines`, `MassLiteracy`, `AerialTransport`, `MagneticSensor`, `PrecisionInstruments`, `DistributedNetworks`, `BiomimeticDesign`, `GeneCultureCoevolution` |
| 5 | Information-age + transcendence trio + bio-engineering peer | `DigitalComputation`, `InformationNetworking`, `GeneticManipulation`, `OrbitalReach`, `AdvancedMedicine`, `MaterialFabrication`, `AutonomousSystems`, `EnergyStorage`, `CryogenicEngineering`, `OrganicSynthesis`, `BioelectricResonator`, `FieldPropulsionEngine`, `MetamaterialLattice`, `EcosystemEngineering` |

Tier assignment is enumerated per-variant in `ToolKind::tier()`
([`identity.rs:275`](../sim/civ/src/tech/identity.rs)).

The transcendence trio at tier 5 — `BioelectricResonator`,
`FieldPropulsionEngine`, `MetamaterialLattice` (constants in
`ToolKind::TIER_FIVE` at
[`identity.rs:99`](../sim/civ/src/tech/identity.rs)) — drives the
`transcendence` run-end check. Unlocking all three after a sustained
existence marks "this civ reached the tech-tree summit." All three are
narrative milestones, not simulated capabilities — the project's
vision boundary holds (no consciousness physics, no FTL).

`EcosystemEngineering` is the **bio-engineering peer** to the
transcendence trio: a no-fire, no-fusion route to late-game capacity
through deliberate biosphere shaping. It carries the same 3000
species-maturity floor as the trio (see *Species-maturity floor*
below) and the same 0.55 literacy gate.

## Per-tool spec

Each `ToolKind` carries a fixed set of per-tool methods enumerated as
big match arms in
[`sim/civ/src/tech/specs/`](../sim/civ/src/tech/specs/) and contributes
to twelve effect categories enumerated in
[`sim/civ/src/tech/effects.rs`](../sim/civ/src/tech/effects.rs).

### Gates

| Function | Meaning | Source |
|----------|---------|--------|
| `prereq_channels` | `ChannelKind`s a species must natively possess (empty = no native-channel gate). | [`identity.rs:372`](../sim/civ/src/tech/identity.rs) |
| `manipulation_prereqs` | Body-plan modes that suffice to fabricate the tool (empty = no manipulation gate; tier-1 tools accept ~10 modes each, tier-5 narrow to ~3–5). | [`specs/manipulation.rs:32`](../sim/civ/src/tech/specs/manipulation.rs) |
| `relation_prereqs` | `(template_id, ChannelKind)` pairs that must be **confirmed** in the civ's `Hypothesizer`. The lookup is template-level — any confirmed relation matching `template_id` satisfies the gate (the channel is narrative documentation). | [`specs/relations.rs:31`](../sim/civ/src/tech/specs/relations.rs) |
| `tool_prereqs` | Other `ToolKind`s that must already be in `unlocked_tools`. DAG-invariant: every prereq has strictly lower `tier()` than the dependent tool. | [`specs/tools.rs:19`](../sim/civ/src/tech/specs/tools.rs) |
| `min_civ_confirmed_relations` | Minimum count of confirmed relations the civ itself has fit. Tier ladder: 0 / 5 / 15 / 50 / 200. | [`specs/mod.rs:71`](../sim/civ/src/tech/specs/mod.rs) |
| `min_civ_experimental_relations` | Minimum count of relations confirmed with at least one apparatus sample contributing. Ladder: 0 / 0 / 3 / 12 / 80. | [`specs/mod.rs:191`](../sim/civ/src/tech/specs/mod.rs) |
| `literacy_floor` | Minimum civ `literacy_score()` scalar. Tier ladder: 0.00 / 0.15 / 0.30 / 0.50 / 0.65 (sensorium variants and the alt-path block use intermediate floors of 0.20, 0.30, 0.35, 0.55). | [`specs/mod.rs:279`](../sim/civ/src/tech/specs/mod.rs) |
| `species_maturity_floor` | Minimum `total_confirmed_relations` summed across every civ in the run. 3000 for all 14 tier-5 tools; 0 for tier ≤ 4. | [`specs/mod.rs:416`](../sim/civ/src/tech/specs/mod.rs) |
| `crust_prereqs` | Optional `Crust` whitelist. `FieldPropulsionEngine` needs `Piezoelectric`/`Ferrous`/`RareEarth`; `MetamaterialLattice` needs `Piezoelectric`/`RareEarth`; everything else is `None`. | [`specs/mod.rs:443`](../sim/civ/src/tech/specs/mod.rs) |
| `resource_prereqs` | `(Substance, threshold)` pairs requiring minimum summed density across `claimed_cells`. Hard gate (serendipity cannot bypass). | [`specs/mod.rs:481`](../sim/civ/src/tech/specs/mod.rs) |

The `is_buildable` function in
[`gating.rs:85`](../sim/civ/src/tech/gating.rs) folds the body-plan +
species-channel + planet-feature + crust gates; `is_unlocked` at
[`gating.rs:162`](../sim/civ/src/tech/gating.rs) adds the
civ-maturity counts, literacy floor, relation prereqs, and
tool-prereq checks.

### Effects

Twelve effect categories. `*_multiplier` returns a multiplicative
factor (`Real::ONE` = no effect); `*_bonus` returns an additive shift
(`Real::ZERO` = no effect). Civ-level aggregators in `tools.rs` fold
each tool's contribution across `unlocked_tools` and the dynamic
registry.

| Method | Modifies | Headline tools |
|--------|----------|----------------|
| `capacity_multiplier` | Per-cell carrying capacity. | `BulkCultivation` ×5.0, `Mechanisation` ×10.0, `ChemicalSynthesis` ×3.0, `MedicalIntervention` ×2.0, `AdvancedMedicine` ×3.0, `GeneCultureCoevolution` ×3.0, `EcosystemEngineering` ×2.0. |
| `food_crisis_resistance_bonus` | Lowers the `FOOD_CRISIS_THRESHOLD` floor. | `BulkStorage` +0.12, `PreservedFood` +0.10, `GeneCultureCoevolution` +0.15, `EcosystemEngineering` +0.20. |
| `war_strength_bonus` | Multiplier on `conflict::strength`. | `ChemicalProjectile` +0.20, `DefensiveFortification` +0.15, `ContactWeapon`/`RangedMomentumWeapon` +0.10. |
| `seasonal_floor_bonus` | Lifts the per-cell carrying-capacity floor in extreme months. | `SimpleShelter` +0.10, `EcosystemEngineering` +0.15. |
| `catastrophe_resistance_bonus` | Reduces population loss to volcanism / epidemics / storms. | `MedicalIntervention` +0.15, `AdvancedMedicine` +0.15, `EcosystemEngineering` +0.15. |
| `literacy_bonus` | Additive lift on `literacy_score`. | `MassLiteracy` +0.20, `WrittenJurisprudence` +0.15, `CodexTradition` +0.12, `CulturalEncoding` +0.10. |
| `transmission_fidelity_bonus` | Boosts inter-civ comprehension. | `LongRangeCommunication` +0.15, `InformationNetworking` +0.15, `MassLiteracy` +0.15. |
| `expansion_rate_bonus` | BFS territory growth speed. | `OrbitalReach` +0.30, `FieldPropulsionEngine` +0.30, `HeavyTransport`/`AerialTransport` +0.20. |
| `cohesion_bonus` | Lifts civ cohesion equilibrium (delays civil war / breakaway). | `DistributedNetworks` +0.12, `MassLiteracy` +0.10, `InformationNetworking` +0.10. |
| `migration_speed_bonus` | Intra-civ migration rate multiplier. | `FieldPropulsionEngine` +0.30, `HeavyTransport`/`AerialTransport` +0.20. |
| `discovery_rate_bonus` | Multiplier on the hypothesizer's `attempt_period` cadence (faster fits per tick). | `DigitalComputation` +0.20, `AnalyticalEngines` +0.15, `PrecisionInstruments` +0.15, `ExperimentApparatus` +0.10. |
| `fertility_bonus` | Multiplier on biological birth rate. | `AdvancedMedicine` +0.10, `MedicalIntervention` +0.10. |
| `mortality_reduction_per_bracket` | Per-bracket `[infant, juvenile, fertile, elder]` mortality cut. | `MedicalIntervention` [15,15,10,5]%, `AdvancedMedicine` [15,15,15,10]%, `BasicHealing` [15,10,5,0]%. |
| `lifespan_extension_factor` | Biological lifespan multiplier (raises the cap, not realised expectancy). | `GeneticManipulation` +0.20, `BioelectricResonator` +0.10, `AdvancedMedicine` +0.10. |

## Strict prereq path

A tool unlocks the moment every gate in `is_unlocked` passes. The
function emits `TechUnlocked` (or rather, the sim/core tick step
emits it once `is_unlocked` returns `true` for the first time on a
given tool). Buildable + civ-maturity + literacy + relation prereqs
+ tool prereqs + resource floor all in.

## Serendipitous unlocks

Parallel to the strict path:
`serendipity_missing_prereqs` in
[`gating.rs:243`](../sim/civ/src/tech/gating.rs) returns `Some(1)`
when **exactly one** prereq is missing (relation OR tool prereq) and
the rest of the gates pass. The literacy floor relaxes to 75% of the
strict floor for serendipity; resource prereqs and civ-maturity
floors remain hard.

When `serendipity_missing_prereqs` returns `Some(1)`,
`serendipity_roll` in
[`gating.rs:329`](../sim/civ/src/tech/gating.rs) gates a per-tick
deterministic ChaCha20 dice roll keyed on
`(planet_seed, civ_id, tool_id, tick)`. Base probability per million
is 25 (`2.5e-5`/tick), scaled by literacy (×0.5 at 0, ×2.0 at 1.0)
and accumulated science (×1 to ×5 in proportion to confirmed
relations / 100), clamped to `[1e-6, 1e-3]`. Over thousands of
ticks a mature civ sees roughly 1.5–3 stumbled-onto unlocks per
lifetime — the "Newton's apple" beats become recognisable
narrative features.

## Per-substrate availability

Substrate divergence shows up in three layered gates:

1. **`prereq_channels`** ([`identity.rs:372`](../sim/civ/src/tech/identity.rs))
   — sensorium-extending tools (`DistanceImaging`, `RemoteAcoustic`)
   demand the species already perceive in that domain;
   `BioelectricResonator` needs `ElectricField` or `MagneticSense`.
   Tier-1+ capability tools mostly leave this empty — substrate
   divergence rides on `relation_prereqs` and `tool_prereqs` chains
   instead.
2. **`relation_prereqs`** ([`specs/relations.rs`](../sim/civ/src/tech/specs/relations.rs))
   — substrate locks live here. `LocalisedCombustion` demands
   confirmed `fire` (template 1, requires `Above(Oxidiser, 0)` — a
   deep-ocean methane/ammonia world never observes ignition and is
   genuinely locked out of the combustion branch). `ChemicalSynthesis`
   and `MaterialFabrication` demand confirmed `hydrocarbon_seep`
   (template 21) — non-`Crust::Hydrocarbon` worlds are locked out of
   the petrochemical lineage. `OrganicSynthesis` requires confirmed
   `hydrocarbon_seep` alone (fossil-substrate-locked).
3. **`tool_prereqs`** ([`specs/tools.rs`](../sim/civ/src/tech/specs/tools.rs))
   — transitively inherits the substrate locks. `AerialTransport`
   chains through `MaterialRefining` (combustion-locked), so
   `OrbitalReach` inherits that lock too. By contrast, `Mechanisation`
   chains through `MechanicalAdvantage` only (no combustion), and the
   alt-path block (`WindPower`, `HydraulicWorks`, `BiomimeticDesign`,
   `DistributedNetworks`, `GeneCultureCoevolution`, `EcosystemEngineering`)
   keeps a leaner industrial age reachable for no-fire civs.

The `experiment_apparatus` substrate gate uses confirmed
`tidal_extremum` (template 14, perceivable via `Tactile`) rather than
`fire` — reachable on every habitable world so the
controlled-conditions epistemology isn't combustion-locked. See the
inline comment at
[`specs/relations.rs:295`](../sim/civ/src/tech/specs/relations.rs).

## Sensorium-extending tools

A subset of tier-2 / tier-3 / tier-4 tools extend the species
sensorium by granting new `ChannelKind` channels (the
`granted_channels` method at
[`identity.rs:493`](../sim/civ/src/tech/identity.rs)). When a tool
unlocks, the civ's perceivable-template set unions the granted
channels with species-native modalities; templates whose own
channel list now intersects fire for the civ.

| `ToolKind` | Tier | Grants | `min_civ_confirmed_relations` | `literacy_floor` |
|------------|-----:|--------|------------------------------:|-----------------:|
| `ThermalSensor` | 2 | `InfraredThermal` | 5 | 0.20 |
| `RemoteAcoustic` | 2 | (none — extends acoustic range) | 5 | 0.20 |
| `FieldSensor` | 3 | `ElectricField` | 15 | 0.35 |
| `DistanceImaging` | 3 | (none — extends visual range) | 15 | 0.35 |
| `MagneticSensor` | 4 | `MagneticSense` | 50 | 0.55 |

Sensorium unlocks pair with the
`refresh_perceivable_with_channels` call in
[`hypothesizer.rs:438`](../sim/civ/src/discovery/hypothesizer.rs) so
the candidate cross-product expands on the spot: new `(template,
channel)` pairs get fresh sample buffers; templates the civ already
perceived keep their existing samples and confirmations.

`DistanceImaging` and `RemoteAcoustic` grant no new channel — their
mechanical effect is a **range** lift on existing perception. They
register as unlocked but contribute only their discovery-rate +
expansion-rate bonuses until the template catalog grows.

## Experiment apparatus

`ToolKind::ExperimentApparatus` (tier-2) gates
**controlled-conditions intervention** alongside passive observation
— the closest analogue to Galileo-style "clamp x, measure y"
science the project carries.

### Unlock prereqs

- `manipulation_prereqs` accepts every `ManipulationKind` (per the
  arm at [`specs/manipulation.rs:506`](../sim/civ/src/tech/specs/manipulation.rs))
  — `ChemicalSecretion` runs controlled-concentration baths,
  `WebConstruct` weaves a calibrated chamber, `FluidJet` holds a
  pressure clamp, `ElectricDischarge` clamps field strength,
  `Burrow` excavates a controlled-volume cell.
- `min_civ_confirmed_relations` ≥ 5, `min_civ_experimental_relations` ≥ 0
  (you can't be experimentally-locked out of building the first
  apparatus).
- `literacy_floor` ≥ 0.30.
- `relation_prereqs`: confirmed `tidal_extremum` (template 14) on
  `Tactile` channel — reachable on every habitable world.
- `tool_prereqs`: none. The apparatus is a primitive intervention
  device.

### Mechanics

`sim_civ::apparatus` ships `Apparatus { cell, clamp_channel,
measure_channel }` records (one apparatus per civ at unlock). The
allocator picks the lowest-population claimed cell deterministically.

Per tick:

1. **Pre-physics** — `write_apparatus_clamps` overwrites the
   apparatus cell's clamp channel with one of four ladder values
   keyed by `tick % 4`.
2. **Physics integration** — runs as normal, with the clamp held.
3. **Post-physics** — `record_apparatus_samples` reads the cell's
   measure-channel and feeds the `(clamp, response)` pair into the
   first active figure's hypothesizer via
   `Hypothesizer::record_experimental_measurement`
   ([`hypothesizer.rs:317`](../sim/civ/src/discovery/hypothesizer.rs)).

### Sample weighting

Apparatus measurements push into the rolling sample buffer **twice**
per call ([`hypothesizer.rs:333`](../sim/civ/src/discovery/hypothesizer.rs))
to express the information-density advantage of controlled samples,
and increment `experimental_count_by_relation`. When the relation
later confirms, `ConfirmedMeasurement.is_experimental = true` if any
apparatus contributions reached the fit pool. The
`min_civ_experimental_relations` gate then counts only relations
flagged this way — a civ that never builds `ExperimentApparatus`
tops out at tier 2 by construction.

## Tech multiplier (discovery rate)

Each tool's `discovery_rate_bonus`
([`effects.rs:593`](../sim/civ/src/tech/effects.rs)) folds into a
`(1 + Σbonus)` multiplier the civ threads through
`Hypothesizer::step_with_cosmology_doubt_and_rate`
([`hypothesizer.rs:690`](../sim/civ/src/discovery/hypothesizer.rs))
each tick. The hypothesizer's `attempt_period` is divided by this
multiplier (floor 1 tick) when scheduling the next fit attempt for
each candidate / measurement, so a civ with `+0.50` aggregate runs
roughly 1.5× more candidate-fit attempts per unit time. The raw
`attempt_period` stays canonical for the streak-threshold formula
(`SUSTAINED_TRIGGER_TICKS / attempt_period`) so refinement calibration
is unaffected.

Effectively, the **tech multiplier reduces the hypothesis tolerance
indirectly** by shortening the time between fit attempts: with more
attempts per unit time on the same rolling sample window, the civ
clears the `confidence ≥ exp(-1)` confirm threshold sooner, and a
near-confirm fit that the unscaled civ would have lost to a window
rollover gets surfaced. Combined with `MassLiteracy` widening the
contributor pool and `AnalyticalEngines` / `DigitalComputation`
formalising reasoning, an industrial civ can hit ~`+0.30` aggregate
discovery-rate bonus — a meaningful tightening of the
science-iteration loop.

## Dynamic tool registry (emergent tools)

Static `ToolKind` enum stays in place; **dynamic tools live in a
parallel species-level registry** (`Species::dynamic_tool_registry`)
and per-civ owned vectors (`Civ::unlocked_dynamic_tools`). The
mechanism is dual-mode: existing match-arm machinery untouched, but
the effect aggregators fold both catalogues with the same
combinator.

### Discovery rule

`propose_dynamic_tools` in
[`tool_emergence.rs:292`](../sim/civ/src/discovery/tool_emergence.rs)
runs per-civ at every `TOOL_EMERGENCE_CHECK_PERIOD_TICKS = 240`
(roughly 20 sim-years on the 12-ticks-per-year cadence; substrate-
metabolism-aware via `is_tool_emergence_tick_for_metabolism`).

1. Group the civ's confirmed relations by `Channel` (the 10-variant
   discovery channel; see [discovery.md](discovery.md#channels)).
2. For each channel with at least `EMERGENT_TOOL_CLUSTER_SIZE = 5`
   confirmed relations:
   - **First proposal** on the channel: mint a tool.
   - **Refinement** (the channel already has a dynamic tool): the
     new cluster must have grown by at least
     `EMERGENT_TOOL_REFINEMENT_STEP = 5` templates beyond the prior
     largest proposal. A civ that takes its Temperature canon from
     5 → 10 → 15 → 20 templates gets a sequence of progressively
     stronger thermal tools.

### Per-channel flavour

`effects_for_cluster` in
[`tool_emergence.rs:182`](../sim/civ/src/discovery/tool_emergence.rs)
sizes effects with a `scale = min(cluster, 20) / 20` factor. Every
channel shares a scientific-instrument baseline (`capacity ×1.20`,
`literacy +0.05`, `transmission_fidelity +0.05` at saturation) plus
a per-channel flavour layered on top:

- `Temperature` → +`seasonal_floor` + `catastrophe_resistance`.
- `ChargeMagnitude` → +`discovery_rate` (electromagnetic
  instrumentation).
- `WaterDepth` → +`food_crisis` + `migration_speed` (hydrology).
- `Elevation` → +`expansion_rate` (cartography).
- `Fuel` → extra `capacity` (combustion engineering).
- `Oxidiser` → +`war_strength` (reactive chemistry).
- `Vapour` → +`mortality_reduction` on infant + juvenile
  (sanitation leap).
- `Ice` → +`seasonal_floor` + `catastrophe_resistance` (cryogenic
  preservation).
- `Fossil` → extra `capacity` (fossil-fuel energy).
- `MagneticField` → +`expansion_rate` + `discovery_rate`
  (planetary-field navigation).

### Substance gate

Dynamic tools focused on a substance channel
(`Fuel`/`Oxidiser`/`Vapour`/`Ice`/`Fossil`) carry a flat resource
prereq of `DYNAMIC_TOOL_RESOURCE_THRESHOLD = 1` unit summed across
territory — the gate is "civ has access to the substrate," not a
tier-graduated stockpile. Abstract-channel
(`Temperature`/`ChargeMagnitude`/...) clusters carry no resource
gate.

Dynamic tools are tagged `tier = 5` by convention but emerge from
runtime cluster discovery, not authored progression.

## Per-tick resource consumption

`apply_tool_consumption` in
[`consumption.rs:74`](../sim/civ/src/tech/consumption.rs) draws
`Fuel` / `Fossil` density down across the civ's claimed cells for
every unlocked tool (static + dynamic) whose `resource_prereqs`
names a consumable substance. The draw is **mass-conservative**
against the chemistry layer: each unit of fuel drawn pulls a unit of
`Oxidiser` and produces 2 units of `Ash` (the exact combustion
mirror). Each cell's draw is capped by available fuel + oxidiser so
densities never go negative.

`Water`, `Ice`, `Vapour` are read-only at this layer — tools may
require them present in territory (`resource_prereqs`) but tool
ownership doesn't itself draw them down. The per-cell per-tool
per-tick rate is `1 / 100_000`, calibrated so an 8000-tick run with
5 unlocked Fuel-tools on a 5-cell claim drains well inside the
`BiofuelRegrowth` recovery envelope.

## Tech-tree events

- `TechUnlocked(civ_id, tool_kind, via_serendipity)` — strict or
  serendipitous unlock; the `via_serendipity` flag separates
  prereq-complete unlocks from `serendipity_roll` wins.
- `ToolDiscovered(species_id, tool_id, channel, cluster_size,
  effects)` — emergent dynamic tool committed to the species
  registry.
- `TemplateDiscovered(species_id, template_id, signature, source)` —
  emergent recognition template committed (see
  [recognition.md#emergent-recognition-templates](recognition.md#emergent-recognition-templates)).
- `MeasurementConfirmed(... is_experimental: true)` — a measurement
  relation whose confirmation included apparatus contributions.

The post-run report walks these to render per-civ knowledge
chapters with fitted equations, tier ladders, and the dynamic
catalogue each species accumulated over its run.
