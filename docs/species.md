# Species

The species is the run's persistent unit. Civs come and go; the
species persists across collapse and succession. Species traits
drive sensorium, manipulation, demographics, cognition cadence,
ecosystem role, lifecycle, environmental tolerance, dormancy, and
cosmology baseline. The species is *derived* from
`{planet, regions, recognizable phenomena}` — not sampled
independently — so its shape reflects the niche the planet provides.

Crate-side detail: [`sim/species/README.md`](../sim/species/README.md).
Per-civ drift off the species baseline: [civ.md](civ.md#per-civ-species-drift).
How traits gate which templates are perceivable:
[recognition.md](recognition.md). How the lifecycle dispatches each
tick: [population.md](population.md#lifecycle-dispatch--7-variants).
Trophic interactions and the multi-species ecosystem step:
[`sim/ecosystem/README.md`](../sim/ecosystem/README.md).

## The Species struct

`Species` ([`sim/species/src/species.rs:11-183`](../sim/species/src/species.rs))
carries:

```rust
pub struct Species {
    pub seed: u64,
    pub name: String,
    pub cognition: Real,                  // aggregate scalar (= cognition_axes.average())
    pub cognition_axes: CognitionAxes,    // working_memory / abstraction / social
    pub sociality: Real,
    pub communication_fidelity: Real,
    pub lifespan_years: Real,
    pub modalities: Vec<Modality>,        // 15-channel sensorium subset
    pub manipulation_modes: Vec<Manipulation>, // 12-mode body-plan subset
    pub perceivable_templates: BTreeSet<u32>,
    pub t0_loss: Real,
    pub cognition_topology: CognitionTopology, // 4-way substrate
    pub habitat: Habitat,                 // 6-variant domain
    pub discovered_templates: BTreeMap<u32, DiscoveredTemplate>,
    pub next_discovered_template_id: u32,
    pub dynamic_tool_registry: BTreeMap<u32, DynamicTool>,
    pub next_dynamic_tool_id: u32,
    pub initial_cosmology: [Real; 5],
    pub biology: PopulationBiology,
    pub tolerance: ToleranceEnvelope,     // temp/pH/salinity/rad/pressure
    pub lifecycle: Lifecycle,             // 7-variant life-history
    pub role: EcosystemRole,              // trophic / functional role
    pub dormancy_capability: Real,
    pub plasmids: BTreeMap<u32, Plasmid>, // microbial HGT registry
    pub next_plasmid_id: u32,
    pub is_extant: bool,
}
```

`derive(planet, recognition_lib)`
([`sim/species/src/derive.rs:20`](../sim/species/src/derive.rs))
is a pure function of `planet.seed` plus the recognition library's
template ids; same seed → identical species. The internal RNG
stream XORs `0xCAFE_BABE_DEAD_BEEF` into the planet seed so species
sampling can't entangle with mid-run physics RNG draws.

## CognitionAxes — three independent axes

Collapsing cognition to a single scalar would make a working-
memory-strong cephalopod-equivalent and a social-cognition-strong
canine-equivalent interchangeable in every downstream formula.
`CognitionAxes` ([`sim/species/src/types.rs:801-882`](../sim/species/src/types.rs))
splits cognition into three orthogonal axes, each in `[0, 1]`:

| Axis | Drives |
|------|--------|
| `working_memory` | Hypothesizer cadence, per-fit complexity tolerance. |
| `abstraction` | Tool-tier reachability (tier-3+ tools require formal abstraction), Occam-penalty leniency. |
| `social` | Knowledge-transmission decay, contact-driven law diffusion. |

`from_scalar_with_seed(c, seed)` perturbs each axis by an
independent `[-0.15, +0.15]` offset derived via a SplitMix64-style
hash of `(seed, axis_idx)` so no new RNG stream is introduced. The
three offsets are zero-summed before clamping to `[0, 1]` so
`average()` exactly recovers the input scalar (within a tiny
clamping drift for inputs near the extremes). The legacy
`Species::cognition` scalar reads `cognition_axes.average()` for
backward compatibility.

## CognitionTopology — four substrates

`CognitionTopology` ([`sim/species/src/types.rs:687-783`](../sim/species/src/types.rs))
captures *where* cognition lives. Distribution at derivation
([`sim/species/src/derive.rs:54-65`](../sim/species/src/derive.rs)):
70% Centralized, 15% DistributedRedundant, 10% Collective, 5%
Acentric.

| Variant | Archetype | Attempt-period multiplier | Knowledge-decay multiplier | Abstraction cap | Isolation penalty |
|---------|-----------|---------------------------|----------------------------|-----------------|-------------------|
| `Centralized` | Vertebrate (one brain) | 1.0 (baseline) | 1.0 | 1.0 | 1.0 |
| `DistributedRedundant` | Cephalopod (parallel ganglia) | 0.7 (parallel sensors fire faster) | 1.0 | 0.6 (no integrator) | 1.0 |
| `Collective` | Eusocial hive | 1.0 | 1.0 | 1.0 | 0.05 (single members can't think) |
| `Acentric` | Slime mold / substrate trace | 5.0 (very slow) | 0.2 (substrate IS memory) | 1.0 | 1.0 |

`DistributedRedundant` also receives a `×1.10` cognition bump at
derivation ([`sim/species/src/derive.rs:73-82`](../sim/species/src/derive.rs))
— their distributed nervous systems give a small (+10%) bonus on
the aggregate scalar, ceilinged at 1.0.

`CognitionTopology::transmission_speed_for_modality(kind)`
(line 763-781) returns a per-modality multiplier in `[0.1, 1.0]`:
acoustic / visual / radio / EM = 1.0; bioluminescent /
gestural / postural = 0.8; seismic = 0.7; chemical = 0.2;
tactile = 0.1. Wired into transmission comprehension so a
chemical-pheromone species inherits less of its predecessor's
knowledge per tick than an acoustic species.

## Modalities — 15 channels

`ModalityKind` ([`sim/species/src/types.rs:10-46`](../sim/species/src/types.rs)):

```text
AcousticAir, AcousticWater, Seismic,
VisualLight, VisualPolarization, Bioluminescent,
ChemicalPheromone, ChemicalTaste, Tactile,
ElectricField, MagneticSense, InfraredThermal,
RadioNative, Gestural, Postural
```

Each channel carries `(range_m, fidelity, bandwidth)` per
`default_modality` ([`sim/species/src/sampling.rs:613`](../sim/species/src/sampling.rs))
— e.g. VisualLight `(5000m, 0.9, 60)`, Tactile `(1m, 0.9, 50)`,
RadioNative `(10000m, 0.2, 100)`.

`modality_supported(kind, planet)` ([`sim/species/src/sampling.rs:303`](../sim/species/src/sampling.rs))
gates the channel pool on planet conditions before sampling:

- Sub-surface ocean cuts `VisualLight`, `VisualPolarization`,
  `Gestural`.
- No-atmosphere planets cut `AcousticAir`, `ChemicalPheromone`.
- No-magnetosphere cuts `MagneticSense`; `RadioNative` requires
  Strong magnetosphere.
- `Bioluminescent` and `ChemicalTaste` require a biosphere.
- `Tactile` and `InfraredThermal` are universal.

Biosphere class sets the target channel count
([`sim/species/src/sampling.rs:571-610`](../sim/species/src/sampling.rs)):
HyperBiodiverse 5–7, Lush 3–5, Sparse 2–3, None 1. Tactile is the
universal baseline — any biosphere with ≥ 1 channel includes it,
to guarantee the species can perceive *something* (otherwise a
random pick that missed every visual / thermal / electric channel
leaves a species with no observations to seed discoveries on).

## Manipulation — 12 modes

`ManipulationKind` ([`sim/species/src/types.rs:86-100`](../sim/species/src/types.rs)):

```text
LimbGrasp, Tentacle, MouthBeak, TonguePrehensile,
Trunk, Mandible, FluidJet, ToolExtension,
WebConstruct, Burrow, ElectricDischarge, ChemicalSecretion
```

Each mode carries `(force_n, precision_m, dexterity_score,
dof_count)` per `default_manipulation`
([`sim/species/src/sampling.rs:739`](../sim/species/src/sampling.rs))
— e.g. LimbGrasp `(200N, 0.01m, 0.8 dex, 5 DOF)`, ToolExtension
`(500N, 0.0001m, 1.0 dex, 10 DOF)`, FluidJet
`(40N, 0.1m, 0.3 dex, 1 DOF)`.

Planet composition gates the candidate pool
([`sim/species/src/sampling.rs:687-735`](../sim/species/src/sampling.rs)):

- **OceanWorld / SubSurfaceOcean**: Tentacle, MouthBeak, FluidJet,
  ToolExtension, ElectricDischarge, ChemicalSecretion.
- **GaseousShell**: FluidJet, WebConstruct, ToolExtension,
  ChemicalSecretion.
- **Rocky**: LimbGrasp, Tentacle, MouthBeak, TonguePrehensile,
  Trunk, Mandible, ToolExtension, WebConstruct, Burrow,
  ChemicalSecretion.

Biosphere class sets count: HyperBiodiverse 2–4, Lush 1–3,
Sparse / None 1. `ToolExtension` is the prerequisite for tier-1+
material culture (experiment apparatus, tier-3+ tools).

## Habitat — 6 domains

`Habitat` ([`sim/species/src/types.rs:649-662`](../sim/species/src/types.rs)):

- **Aquatic** — water-bound (oceanic / sub-surface).
- **Terrestrial** — land-evolved default.
- **Amphibious** — crosses both domains natively.
- **Airborne** — land-evolved with flight capability;
  innate +1 wrong-biome transit tier (untrained crosses 1 water /
  non-habitat cell).
- **Subterranean** — primary habitat below-surface excavated
  space; treats land as native; gains constant subsurface
  temperature buffering. Morphological cousin: `Burrow`.
- **Endolithic** — substrate-bound rock-pore life; native for
  Silicate substrates where the "habitat" is the rock itself.

`derive_habitat`
([`sim/species/src/sampling.rs:507-549`](../sim/species/src/sampling.rs))
walks a precedence chain — water-native planet + AcousticWater
(but no AcousticAir) → Aquatic; both acoustic channels →
Amphibious; LimbGrasp + FluidJet → Amphibious; AcousticWater +
(FluidJet | Tentacle) without AcousticAir → Aquatic regardless of
planet; otherwise → Terrestrial. The Airborne fall-through fires
only on a narrow shape: non-water-native rocky planet, AcousticAir
but no AcousticWater, and exactly one `LimbGrasp` manipulator (the
"flight-forelimb" morphology — wings co-evolved with grasping).

Habitat gates territorial claims in
`sim_core::compute_territory`: a civ can natively only claim cells
matching its habitat until it unlocks
`ToolKind::AmphibiousConstruction`, after which it can claim cells
in either domain.

## ToleranceEnvelope — 5-axis environmental gate

`ToleranceEnvelope` ([`sim/species/src/types.rs:260-356`](../sim/species/src/types.rs))
defines the cell conditions a species can occupy and survive:

```rust
pub struct ToleranceEnvelope {
    pub temp_range: (Real, Real),       // Kelvin
    pub ph_range: (Real, Real),         // 0 = strong acid, 14 = strong base
    pub salinity_range: (Real, Real),   // g/L dissolved solids
    pub radiation_max: Real,            // relative; Earth surface ≈ 1.0
    pub pressure_range: (Real, Real),   // atm; Earth surface = 1.0
}
```

`contains(t, ph, sal, rad, p)` is the hard gate; `match_score`
returns a `[0, 1]` fit using the *smallest-margin axis* as the
limiting fit (biology is gated by the weakest link). Each per-axis
score peaks at 1.0 at the centre of the range and falls linearly
toward 0.0 at either edge; out-of-range values return 0.0.

Catastrophe survival multiplies by `tolerance.match_score(local_conditions)`
so extremophile species shaped to high-radiation or high-
temperature niches differentially survive radiation bursts /
thermal pulses that wipe out narrower-envelope species
([`sim/species/src/sampling.rs:1174-1183`](../sim/species/src/sampling.rs)).

### Per-substrate defaults

`substrate_default_envelope`
([`sim/species/src/sampling.rs:1006-1047`](../sim/species/src/sampling.rs)):

| Substrate | temp (K) | pH | salinity (g/L) | rad_max | pressure (atm) |
|-----------|----------|-----|----------------|---------|----------------|
| Aqueous | 273–373 | 5–9 | 0–50 | 0.5 | 0.5–2 |
| Ammoniacal | 195–240 | 9–12 | 0–100 | 0.8 | 0.5–5 |
| Hydrocarbon | 91–117 | 3–7 | 0–10 | 1.2 | 1–10 |
| Silicate | 1687–3538 | 0–14 | 0–200 | 5.0 | 1–100 |

`derive_tolerance_envelope(seed, substrate)`
([`sim/species/src/sampling.rs:1067-1120`](../sim/species/src/sampling.rs))
starts from the substrate default and applies ±20% per-axis
deterministic jitter via a SplitMix64-style hash of
`(seed, axis_idx)`. Each axis gets an independent offset so
individual species within a substrate end up as distinguishable
extremophiles or generalists. Edges are re-ordered so `lo <= hi`
stays an invariant even if the random offsets cross.

## Lifecycle — 7 variants

`Lifecycle` ([`sim/species/src/types.rs:1063-1105`](../sim/species/src/types.rs))
determines which per-tick step function the population engine runs.
Defaults to `Vertebrate` for back-compat. See
[population.md](population.md#lifecycle-dispatch--7-variants) for
the per-variant step semantics:

| Variant | Carries | Step semantics |
|---------|---------|----------------|
| `Vertebrate` | — | 4-bracket cohort baseline (legacy). |
| `Aquatic { semelparous: bool }` | one-shot vs iteroparous flag | Big spawn → adult death (Pacific salmon) or vertebrate step + 70% metamorphosis cull. |
| `Insect` | — | Egg / larva / pupa / adult mapped to 4 brackets + extra adult mortality. |
| `Eusocial { castes: Vec<CasteRole> }` | caste roster | Per-caste `BTreeMap` headcount; only `Reproductive` reproduces. |
| `Plant` | — | Seed / seedling / mature / senescent; durable elder, halved seed survival. |
| `Microbial { fission_strategy: Fission }` | Binary / Budding / Conjugation | Single biomass; doubles per generation time (1 / 2 / 4 ticks). |
| `Modular` | — | Single biomass; logistic growth at `r = 5%`. |

`CasteRole`
([`sim/species/src/types.rs:1019-1034`](../sim/species/src/types.rs)):
`Reproductive`, `Worker`, `Soldier`, `Nurse`. Ord-derived for
deterministic BTreeMap iteration.

`Fission`
([`sim/species/src/types.rs:1037-1047`](../sim/species/src/types.rs)):
`Binary` (bacteria / archaea, fastest), `Budding` (yeast),
`Conjugation` (slowest but unlocks an HGT bonus).

The literal variants pair with the r/K classification in
`derive_population_biology` — a future polish pass can route r=1
broadcast-spawner species to `Aquatic { semelparous: true }` and
social insects to `Eusocial`, etc. The current `derive` hard-codes
`Lifecycle::Vertebrate` at species genesis
([`sim/species/src/derive.rs:160`](../sim/species/src/derive.rs));
non-Vertebrate variants are wired but await a sampler routing
pass.

## EcosystemRole — trophic / functional role

`EcosystemRole` ([`sim/species/src/types.rs:368-404`](../sim/species/src/types.rs))
drives the per-tick multi-species ecosystem step (Lindeman pyramid,
functional response, keystone detection) and worldgen role-
distribution constraints:

| Variant | Carries | Trophic tier | `is_consumer()` |
|---------|---------|--------------|-----------------|
| `Producer { metabolism: ProducerMetabolism }` | Photoautotroph / Chemoautotroph / Mixotroph | 0 (pyramid base) | false |
| `PrimaryConsumer` | — | 1 | true |
| `SecondaryConsumer` | — | 2 | true |
| `ApexConsumer` | — | 3 | true |
| `Detritivore` | — | off-pyramid | true |
| `Saprotroph` | — | off-pyramid | true |
| `Mutualist { kind: MutualismKind }` | Pollinator / SeedDisperser / Engineer / Generic | off-pyramid | true |
| `Parasite { kind: ParasiteKind }` | Macro / Micro / Virus | off-pyramid | true |

Civ-bearing species are always a consumer tier with cognition ≥ 0.3
(filtered at worldgen). `derive` currently fixes
`EcosystemRole::PrimaryConsumer` at species genesis
([`sim/species/src/derive.rs:161`](../sim/species/src/derive.rs));
non-civ ecosystem species sampled by `sim_ecosystem` populate the
full role distribution.

### Interaction matrix

`InteractionMatrix`
([`sim/species/src/types.rs:527-548`](../sim/species/src/types.rs))
is a sparse `BTreeMap<(SpeciesId, SpeciesId), Interaction>`.
`Interaction`
([`sim/species/src/types.rs:440-463`](../sim/species/src/types.rs))
carries `(kind, strength, functional_response, half_saturation)`
where `half_saturation` is a fraction of producer capacity (apex
predators saturate fast → low k; small specialists saturate
slowly → high k; back-compat default 0.5). `InteractionKind`:
Predation, Competition, Mutualism, Commensalism, Parasitism,
HabitatModification. `FunctionalResponse`: Linear, Saturating,
Sigmoidal.

## PopulationBiology — life-history parameters

`PopulationBiology` ([`sim/species/src/types.rs:900-1010`](../sim/species/src/types.rs))
holds the per-species life-history fields the 4-bracket cohort
step reads:

```rust
pub struct PopulationBiology {
    pub clutch_size: Real,                 // [1, 5000]
    pub infant_fraction: Real,             // [0.01, 0.10]
    pub maturity_fraction: Real,           // [0.04, 0.40]
    pub eldership_fraction: Real,          // [0.0, 0.30]
    pub infant_survival: Real,             // [0.05, 0.95]
    pub juvenile_survival: Real,           // [0.20, 0.99]
    pub food_multipliers: [Real; 4],       // [0.30, 0.60, 1.00, 0.90]
    pub events_per_fertile_window: Real,   // [2, 30]
    pub reproductive_success: Real,        // [0.005, 0.10]
}
```

`fertile_fraction = 1 − infant − maturity − eldership`, always
positive by sampling-time clamps.

### Derivation from species traits

`derive_population_biology`
([`sim/species/src/sampling.rs:843-996`](../sim/species/src/sampling.rs))
maps `(cognition, sociality, lifespan_years, habitat,
cognition_topology, manipulation_modes)` onto an r/K-strategy axis
in `[0, 1]` (0 = pure K, 1 = pure r):

- Low sociality → r (1 − sociality, weighted ⅓)
- Short lifespan → r (`1 − lifespan / 100`, clamped, weighted ⅓)
- r-leaning manipulation (ChemicalSecretion, WebConstruct,
  FluidJet, Mandible, Burrow) → r; LimbGrasp / Trunk /
  ToolExtension → K (`manipulation_r_lean`, weighted ⅓)
- Aquatic habitat → `+0.10` r; Airborne → `−0.10` r

The r-axis then drives:

| Field | Formula |
|-------|---------|
| `clutch_size` | `1 + r² × 4999` (quadratic so mid-axis stays modest at ~1250) |
| `infant_fraction` | `0.01 + (1 − r) × 0.09` |
| `maturity_fraction` | `0.04 + (1 − r) × 0.31` + Centralized bonus +5% |
| `eldership_fraction` | `sociality × cognition × 0.30 × (1 − r)`, ≤ 0.30 |
| `infant_survival` | `0.05 + (1 − r) × 0.90` |
| `juvenile_survival` | `0.20 + (1 − r) × 0.79` |
| `events_per_fertile_window` | `(1 − r) × 30 + r × 2` |
| `reproductive_success` | `(1 − r)² × 0.005 + r² × 0.10` |

The `fertile_fraction` floor scales per-strategy:
`0.30 − r × 0.20` — pure-K keeps the historical 0.30 floor; pure-r
drops to 0.10 (a mayfly's hours of fertile life as a fraction of
total life, not 30%). Excess `(infant + maturity + eldership)` is
trimmed from `maturity_fraction` then `eldership_fraction` to
respect the floor.

The `reproductive_success` factor and `events_per_fertile_window`
together keep birth rates in the realistic ~0.001–0.01/month band
for K-strategists (real human ≈ 0.0005/mo per fertile adult; sim
sits 2–5× above for sociality headroom) and ~5–90/month for
r-strategist broadcast-spawners. Without them, the legacy
`clutch / fertile_months` formula overshot K-strategist births by
~500× and the recruit-ceiling clamp at
`step_with_capacity` was the load-bearing limiter. See
[population.md](population.md#populationdynamics--per-species-rate-derivation).

## Dormancy

`dormancy_capability ∈ [0, 1]` is sampled at derivation as
`raw² × raw` (i.e. the square of a unit sample, skewed strongly
toward 0). Most species cannot enter cryptobiosis; tardigrade-grade
dormancy (`> 0.9`) appears only for the top decile of seeds
([`sim/species/src/derive.rs:120-121`](../sim/species/src/derive.rs)).

A catastrophe's realised damage is reduced by
`(1 − dormancy × severity_factor)`
([`sim/species/src/types.rs:624-631`](../sim/species/src/types.rs));
the surviving-but-dormant fraction lands in a `DormantPool` that
revives at 1%/tick capped at the pre-event level. See
[population.md](population.md#dormancy--catastrophe-survival).

## Plasmids — microbial HGT

`Plasmid` ([`sim/species/src/types.rs:505-520`](../sim/species/src/types.rs))
models horizontal gene transfer as a selection event rather than
smooth interpolation. An HGT trial *deposits* a plasmid in the
recipient's `plasmids` registry with a donor trait value; each
tick the plasmid is evaluated against local conditions and either
sweeps (the species' actual trait snaps to `trait_value`) or is
lost (probabilistically removed in proportion to misfit).
Per-species monotonic `next_plasmid_id` allocator ensures
concurrent acquisitions never alias. Default empty registry keeps
existing literals compilable.

## Per-substrate sampling defaults + per-species jitter

The species' shape derives from the planet substrate via two
hand-offs:

1. **Substrate baseline**: `substrate_default_envelope` /
   per-substrate biology defaults give each `MetabolicSubstrate`
   a distinct biological "window" (aqueous water-temperature
   life; silicate molten-lattice life; etc.).
2. **Per-species jitter**: a SplitMix64-style hash of
   `(seed, axis_idx)` applies ±20% jitter on tolerance axes and
   `[-0.15, +0.15]` zero-summed jitter on cognition axes. The
   jitter is deterministic (no RNG state) so replay stays
   bit-identical.

The substrate `metabolism()` factor (Aqueous 1.0, Ammoniacal 0.5,
Hydrocarbon 0.4, Silicate 0.2) then scales every biological /
societal rate downstream — see [population.md](population.md#substrate-metabolism--biological-time-scale).

## Discovered templates + dynamic tools

Two per-species discovery registries persist across civ collapse
boundaries so successor civs can rediscover (rather than duplicate)
their predecessors' work
([`sim/species/src/species.rs:87-104`](../sim/species/src/species.rs)):

**`discovered_templates: BTreeMap<u32, DiscoveredTemplate>`** —
Civs propose new templates from observation regularities; the
proposals graduate into this map and become first-class
recognition firings, indistinguishable from authored templates
downstream. Indexed starting at
`DISCOVERED_TEMPLATE_ID_START = 1000` so the id space stays
disjoint from authored template ids. The `BTreeMap` keeps
iteration deterministic.

**`dynamic_tool_registry: BTreeMap<u32, DynamicTool>`** — When a
civ accumulates a coherent cluster of confirmed relations on a
single channel, it proposes a `DynamicTool`
([`sim/species/src/types.rs:121-160`](../sim/species/src/types.rs))
whose effects scale with the cluster's depth. Indexed starting at
`DYNAMIC_TOOL_ID_START = 1000`
([`sim/species/src/types.rs:236`](../sim/species/src/types.rs)) so
the id space is disjoint from the static `ToolKind` enum (1..=58).

`DynamicTool` carries `(id, name, tier, channel_focus,
relation_prereqs, resource_prereqs, effects,
discovered_at_tick, discovered_by_civ_id)`. The `name +
channel_focus + relation_prereqs` are derived deterministically
from the discovering civ id + tick + proposing-cluster signature
— same seed → same dynamic tools across replays.

`DynamicToolEffects`
([`sim/species/src/types.rs:174-232`](../sim/species/src/types.rs))
mirrors all 14 effect categories that static `ToolKind` can grant
— capacity multiplier, food-crisis bonus, war strength,
seasonal floor, catastrophe resistance, literacy, expansion rate,
transmission fidelity, per-bracket mortality reduction, lifespan
extension, discovery-rate bonus, cohesion bonus, migration speed,
fertility bonus. Defaults to identity (capacity ×1.0, all bonuses
0) so the discovery pipeline can stay untouched until a polish
pass wants emergent medicine / senescence treatment to fire from
real cluster magnitudes. See [tech.md](tech.md).

## Cosmology baseline + religion

The species carries an `initial_cosmology: [Real; 5]` bias
(`[empirical, communitarian, reformist, mystical, hierarchical]`)
that anchors all of its civs' five-axis cosmology — the *slow-
drift, deep worldview* layer. `derive_initial_cosmology`
([`sim/species/src/sampling.rs:401-491`](../sim/species/src/sampling.rs))
applies additive bias rules (then clamps to ±0.50 so the starting
position never out-shouts in-life drift):

- **Sociality** > 0.6 → +communitarian 0.20; < 0.3 → −0.10.
- **Cognition** > 0.6 → +empirical 0.15; < 0.3 → +mystical 0.10.
- **Communication fidelity** < 0.4 → +mystical 0.15 (oral
  traditions tilt toward mystery); > 0.7 → +empirical 0.10.
- **Habitat**: Aquatic → +communitarian 0.15; Subterranean →
  +communitarian 0.20; Endolithic → +communitarian 0.10;
  Terrestrial / Airborne → +reformist 0.05.
- **Rich sensorium** (≥ 4 modalities) → +empirical 0.05.
- **Axial tilt** > 30° → +reformist 0.15; < 10° → +communitarian
  0.05.
- **Crust** RareEarth / Piezoelectric → +empirical 0.05.

`hierarchical` stays at zero — species-trait-based bias on that
axis has no clean justification; cosmology drift reads
catastrophe events to push it instead.

On top of the cosmology baseline, each civ founds with its own
three-axis religion vector (theology / ritual / sacred-time) that
drifts much faster. See [culture.md](culture.md).

## Per-civ drift

Each civ has `cognition / sociality / lifespan / communication
fidelity` deltas off the species baseline. Inaugural civs zero
them; each successor inherits its parent's deltas plus a
deterministic per-generation perturbation. The species drifts as
it begets successors. `SpeciesDrift` events emit on each new
civ's deltas. A long species history thus produces a *lineage* of
related civs where the underlying species itself slowly mutates.
See [civ.md](civ.md#per-civ-species-drift).

## Sensorium gating

Recognition templates split into `perceivable-now` and `latent`
based on the species' sensorium. A species perceives template `t`
iff `template_channels(t.id) ∩ modalities` is non-empty
([`sim/species/src/sampling.rs:21-294`](../sim/species/src/sampling.rs)).
A photic species can perceive visible-light templates by default;
a tactile-only species cannot detect them until it invents a
sensorium-extending tool (telescope, photic-sensor, microscope).
When the unlock fires, latent templates promote to perceivable-
now and recognition begins firing them.

Sensorium-tool unlock gates on a hybrid threshold: cumulative
observation count + literacy floor + relation prereqs. Per-tool
detail in [tech.md](tech.md).

## Cognition-derived discovery cadence

High-cognition species cycle hypothesis attempts faster — the
hypothesizer's per-tick attempt budget scales with
`cognition_axes.working_memory`
([`sim/civ/src/demographics.rs:196`](../sim/civ/src/demographics.rs)).
A high-cognition civ can chase several rivals simultaneously; a
low-cognition civ waits multiple ticks between attempts. The
substrate `metabolism` factor then stretches the period further:
silicate civs unfold across ~5× more ticks than aqueous ones.

## Extinction

`is_extant: bool` flips to `false` when the per-species biomass
pool stays below `EXTINCTION_THRESHOLD` for
`EXTINCTION_CONFIRMATION_TICKS` in a row (Sprint 2 Item 6a). The
record stays in the per-planet registry for history / replay
determinism but is skipped by the ecosystem step. Defaults to
`true` for back-compat with literal `Species { ... }` constructions.

## Species-level events

- `SpeciesDerived` — emitted at run start with full trait set
  (Q32.32 raw bits for bit-exact determinism).
- `SpeciesNomadsChanged` — nomadic-pool population shifts.
- `SpeciesDrift` — per-civ drift deltas off the baseline.
- `CivLifeExpectancyChanged` — per-civ life-expectancy crosses a
  delta threshold off the last emitted value.
- `SpeciesCosmologyBias` — emitted with the species'
  `initial_cosmology`.
- `TemplateDiscovered` — species canon adopts a new emergent
  template (see [recognition.md#emergent-recognition-templates](recognition.md#emergent-recognition-templates)).
- `ToolDiscovered` — species canon adopts a new emergent tool (see
  [tech.md#emergent-tools](tech.md#emergent-tools)).
