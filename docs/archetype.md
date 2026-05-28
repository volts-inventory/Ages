# Civilizational archetypes

The *developmental path* a civilization takes is not authored. Ages
scores every run across an **open space of 11 peer levers** — the
foundational resources / sciences a civilization can organize around
— with no privileged default and no fallback. The classifier then
labels the run (pure / hybrid / emergent), bends it through a
cognition overlay, and resolves a divergent **endpoint** at
transcendence. This generalizes what used to be an implicit
combustion (fire → industry) bias into a flat, symmetric framework
that directly serves "emergence over authoring" (see
[PROJECT.md](PROJECT.md)).

The classifier lives in
[`sim/civ/src/archetype.rs`](../sim/civ/src/archetype.rs)
(`Lever`, `LeverScores`, `ArchetypeLabel`, `CognitionMode`,
`classify_world_species`, `refine_with_run`, `classify_realized`,
`endpoint_for`). The resonance physics it leans on lives in
[`sim/physics/src/resonance.rs`](../sim/physics/src/resonance.rs); for
how field-sensing civs do resonance science see
[recognition.md](recognition.md) and [discovery.md](discovery.md).

## The 11 levers

A `Lever` ([`archetype.rs:34`](../sim/civ/src/archetype.rs)) is the
foundational resource / science a developing civilization leans on.
All eleven are scored identically as **peer dimensions** — none is a
default, none is a fallback. `Lever::ALL` is the canonical order; it
also doubles as the deterministic tiebreak when two levers score
equal.

| Lever | `name()` | Signature it reads from |
|-------|----------|-------------------------|
| `Combustion` | `combustion` | Oxidising atmosphere + hydrocarbon crust + accessible fuel. |
| `FieldResonance` | `field_resonance` | Piezoelectric crust + strong dipole + a field-sensing biology. |
| `Biochemical` | `biochemical` | Life-dense, ore/fuel-poor, aqueous world. |
| `Cryogenic` | `cryogenic` | Cold solvent (hydrocarbon / ammoniacal) + meagre insolation. |
| `Mechanical` | `mechanical` | Abundant tidal / wind kinetic energy on a no-fire world. |
| `Hydraulic` | `hydraulic` | Wet, dense-atmosphere water world. |
| `ExoticChemistry` | `exotic_chemistry` | Reducing / hazy chemistry on a non-water solvent. |
| `PlasmaEm` | `plasma_em` | Strong dipole + electrically active air (not piezo). |
| `Gravitational` | `gravitational` | Many / large moons + high gravity. |
| `Photonic` | `photonic` | Bright star + light-sensing biology + clear air. |
| `Nuclear` | `nuclear` | Radiogenic crust + an unshielded, radiation-hardened niche. |

The first five (`Combustion`, `FieldResonance`, `Biochemical`,
`Cryogenic`, `Mechanical`) were the originally-authored attractors;
the other six widen the space. The point of the rework is that they
are now scored on the same footing — `combustion` is one option among
eleven, not the assumed baseline that everything else deviates from.

## Open classification

`classify` ([`archetype.rs:307`](../sim/civ/src/archetype.rs)) reads
the ranked score vector and emits one of three `ArchetypeLabel`
shapes ([`archetype.rs:149`](../sim/civ/src/archetype.rs)). The space
is **open** — it mirrors the engine's existing philosophy for
emergent recognition templates and dynamic tools, so a developmental
path nobody authored is still detected and given a name.

| Label | Condition | `name()` example |
|-------|-----------|------------------|
| `Pure(lever)` | One lever clearly dominates: `top ≥ 0.40` **and** `top − second ≥ 0.12`. | `combustion` |
| `Hybrid(a, b)` | Two co-dominant levers, neither clearly ahead: `top ≥ 0.25` **and** `second ≥ 0.25`. | `field_resonance/cryogenic` |
| `Emergent { dominant, secondary, tertiary }` | No clear winner — a novel mix. Signature-named from the top three dimensions. | `emergent_mechanical_hydraulic_photonic` |

The thresholds (`pure_floor` 0.40, `pure_margin` 0.12, `hybrid_floor`
0.25) live as `Real` ratios so the whole classification stays
fixed-point. `LeverScores::ranked`
([`archetype.rs:137`](../sim/civ/src/archetype.rs)) sorts by score
descending and breaks ties by `Lever::ALL` order, so the label is
byte-stable across replays.

Emergent labels are signature-named the same way emergent recognition
templates and dynamic tools are — `emergent_{dominant}_{secondary}_{tertiary}`.
This is the load-bearing property: the framework does not collapse
unknown paths onto the nearest authored attractor, it surfaces them as
their own labelled archetype. A 12-seed prior sweep yields 12
**distinct** labels spanning every lever family — evidence the space
isn't quietly defaulting to combustion.

## Cognition overlay

A species' cognition topology yields a `CognitionMode`
([`archetype.rs:91`](../sim/civ/src/archetype.rs)) that is **orthogonal
to the resource lever** — a collective or substrate-distributed mind
can sit on *any* lever. It is modelled as an overlay rather than a
competing lever precisely so the "collective intelligence" and
"information / substrate" paths compose with all eleven resource
levers instead of crowding them out.

| `CognitionMode` | `name()` | Derived from `CognitionTopology` | Reading |
|-----------------|----------|----------------------------------|---------|
| `Individual` | `individual` | `Centralized`, `DistributedRedundant` | One integrated mind per individual. |
| `Collective` | `collective` | `Collective` | Eusocial hive — the colony is the cognitive unit. |
| `SubstrateDistributed` | `substrate_distributed` | `Acentric` | Slime-mold / acentric; the medium is the memory. The literal information/substrate overlay. |

`cognition_mode` ([`archetype.rs:337`](../sim/civ/src/archetype.rs))
does the mapping. The overlay's effect lands at the endpoint: a
collective or substrate-distributed mind bends its fate inward toward
silence (see [Endpoints](#endpoints)).

## Prior vs realized scoring

A lever score is built in two stages: a **prior** from the world +
species, then a **refinement** from the realized run trajectory.

### Prior (run start)

`score_world_species`
([`archetype.rs:350`](../sim/civ/src/archetype.rs)) scores every lever
from static world + species signals — atmosphere, crust + crustal
composition, metabolic substrate, magnetosphere, biosphere class +
density, stellar luminosity, moon count, gravity, plus the species'
sensory modalities, radiation tolerance, and cognition topology. Each
contribution is a small `Real` ratio `add`-ed onto the lever and
clamped to `[0, 1]`; scores need not sum to one. A few examples:

- An oxidising atmosphere adds 0.50 to `Combustion`; a hydrocarbon
  crust adds another 0.30.
- A piezoelectric crust adds 0.40 to `FieldResonance`; a
  field-sensing biology (`ElectricField` / `MagneticSense` /
  `RadioNative`) adds 0.22.
- A hyper-biodiverse, ore- and fuel-poor aqueous world stacks
  `Biochemical`.
- Weak, distant sunlight (`< 800 W/m²`) and a cold solvent feed
  `Cryogenic`.

`classify_world_species`
([`archetype.rs:501`](../sim/civ/src/archetype.rs)) packages the prior
scores, the open label, and the cognition overlay into an
`ArchetypeProfile`. This is what the run-start `ArchetypeDerived`
event reports.

### Realized refinement

`refine_with_run`
([`archetype.rs:554`](../sim/civ/src/archetype.rs)) turns the prior
into the *realized* archetype using two run signals — the branches the
civ actually climbed, not just the ones its world made likely:

- **Confirmed-relation channels** — each `(Channel, count)` pair adds
  `0.02` per confirmed relation on that channel's lever, capped at
  `0.35` per channel so a flood on one channel can't peg a lever
  alone. `channel_lever`
  ([`archetype.rs:514`](../sim/civ/src/archetype.rs)) maps fuel /
  oxidiser / fossil → `Combustion`, charge / resonance →
  `FieldResonance`, magnetic field → `PlasmaEm`, water depth / vapour
  → `Hydraulic`, ice → `Cryogenic`, elevation → `Mechanical`.
  Temperature is thermodynamically neutral (every lever reads it) so
  it points at no lever.
- **Unlocked tools** — a flat `0.10` membership bonus per tool on its
  lever's branch. `tool_lever`
  ([`archetype.rs:530`](../sim/civ/src/archetype.rs)) maps the
  combustion / field / biochemical / cryogenic / mechanical /
  hydraulic / photonic tool lineages onto their levers.

`classify_realized`
([`archetype.rs:585`](../sim/civ/src/archetype.rs)) re-scores the
prior with these signals and classifies the result. This is what
resolves the endpoint at transcendence (the realized profile is read
from the transcending civ's tool roster — a tier-5 roster is a strong
lever signal). The whole path is pure `Real` (Q32.32) arithmetic with
no RNG, so the realized label is deterministic across replays.

## Resonance physics + science

The field/resonance lever is now first-class because the physics it
depends on exists. A new per-cell **resonance field** `Ψ` was added to
the physics state ([`sim/physics/src/state.rs:63`](../sim/physics/src/state.rs))
as an *additive, speculative substrate* — no existing law reads it, so
installing it leaves every legacy channel (temperature, charge,
climate) bit-identical.

`ResonanceField`
([`sim/physics/src/resonance.rs`](../sim/physics/src/resonance.rs)) is
the evolution law. Per cell, the field relaxes toward an
electromagnetic-driven equilibrium
`piezo_gain × (field_coupling·|B| + charge_coupling·|charge|)` and
diffuses to neighbours via the same sum-conserving pair-flux scheme as
charge diffusion. Couplings derive per-planet from the crust's
piezoelectric fraction, the magnetosphere factor, and an
atmosphere-density-derived propagation coefficient — so the field is
prominent on a piezoelectric-crust, strong-dipole, dense-atmosphere
world and vanishing on a basaltic, no-dipole one. See
[physics.md#laws](physics.md#laws) for where it sits in the law
roster.

Its consumers are new:

- **Recognition** gained `Field::Resonance`
  ([`sim/recognition/src/lib.rs:59`](../sim/recognition/src/lib.rs))
  plus two templates: `resonance_field_active` (id 54, fires above 1
  unit) and `attention_coherence` (id 55, the sustained high-field
  state above 5 units). Both read through the `ElectricField` /
  `MagneticSense` / `RadioNative` channels. See
  [recognition.md](recognition.md).
- **Discovery** gained `Channel::Resonance`
  ([`sim/civ/src/discovery/channels.rs:47`](../sim/civ/src/discovery/channels.rs)),
  reachable by `ElectricField`, `MagneticSense`, and `RadioNative`
  modalities. See [discovery.md#channels](discovery.md#channels).

The result is that a field-sensing species on a resonance-rich world
does **genuine resonance science** through the same hypothesizer
pipeline as any other lever — it fires the resonance templates,
samples the resonance channel, fits laws, and unlocks the
field-lineage tools. Resonance is the one lever with full science
treatment today; all eleven are scored and reach endpoints, but the
others do not yet have a dedicated physics field or discovery channel.

## Endpoints

At the transcendence threshold (all tier-5 tools sustained for
`TRANSCENDENCE_SUSTAINED_TICKS`) each archetype reaches a **different**
endpoint — not one shared singularity. `endpoint_for`
([`archetype.rs:220`](../sim/civ/src/archetype.rs)) resolves the fate
from the realized profile's dominant lever (the pure lever, the
leading half of a hybrid, or the dominant dimension of an emergent
signature). It returns a stable machine `mode` tag plus a narrated
`description`.

| Dominant lever | `endpoint_mode` | Fate |
|----------------|-----------------|------|
| `Combustion` | `combustion_industrial_apex` | An industrial-combustion apex on a finite fuel endowment — expansion vs exhaustion, not a guarantee. |
| `FieldResonance` | `field_resonance_matter_transition` | A matter-transition through resonance fields — a transformation that draws whatever attends the field ("watchers"). |
| `Biochemical` | `biochemical_biosphere_merge` | Minds and living world fuse into one metabolism; life seeds itself outward as panspermia. |
| `Cryogenic` | `cryogenic_deep_time` | A slow, patient deep-time civilization built to outlast its dimming star. |
| `Mechanical` | `mechanical_crossover` | A mechanical-computational crossover into a constructed successor intelligence. |
| `Hydraulic` | `hydraulic_world_engineering` | Planet-scale fluid + thermal engineering — oceans and climate run as one machine. |
| `ExoticChemistry` | `exotic_synthesis` | An exotic-solvent synthesis path mastering reactions no carbon-aqueous world would attempt. |
| `PlasmaEm` | `plasma_magnetospheric_ascendance` | A magnetospheric ascendance tapping plasma + field energy at industrial scale. |
| `Gravitational` | `gravitational_engineering` | Tidal / gravitational megastructure engineering — orbital mechanics as a tool. |
| `Photonic` | `photonic_uplift` | Starlight harvested and shaped into the civilization's primary medium of work and thought. |
| `Nuclear` | `nuclear_epoch` | A radiogenic nuclear epoch — decay as the power source for a radiation-born civilization. |

The cognition overlay then bends the fate: a `Collective` mind turns
inward, its outward signal thinning toward silence; a
`SubstrateDistributed` mind dissolves into its own medium, leaving
little for outside observers to read. An `Individual` mind keeps the
base fate. The `endpoint_mode` tag is unchanged by the overlay — only
the narrated `description` gains the inward / silence flavour.

The existing species-level `transcendence` run-end reason is
**unchanged** — the endpoint is an additive narration layer on top of
it, not a new run-end. See
[civ.md#run-end-is-species-level](civ.md#run-end-is-species-level).

## Wire / observability

Two additive protocol events
([`protocol/src/world_events.rs:208`](../protocol/src/world_events.rs),
[`protocol/src/lib.rs:153`](../protocol/src/lib.rs)) carry the
archetype onto the event log. Both are purely additive — no existing
event changed.

| Event | Emitted | Carries |
|-------|---------|---------|
| `ArchetypeDerived` | Once at run start ([`sim/core/src/setup.rs:325`](../sim/core/src/setup.rs)) from the world+species prior. | `label`, `dominant_lever`, `secondary_lever`, `cognition_mode`, `lever_names`, and `lever_scores_q32` (Q32.32 raw bits parallel to `lever_names` in canonical order; display via `i64 as f64 / 2^32`). |
| `ArchetypeEndpoint` | At transcendence ([`sim/core/src/run_tick.rs:447`](../sim/core/src/run_tick.rs)) for the civ that crossed the threshold. | `civ_id`, `civ_name`, `label`, `dominant_lever`, `cognition_mode`, `endpoint_mode` tag, and the narrated `description`. |

## Surfacing in the narrators

Both narrators surface the two events with no LLM in the loop:

- **Rust prose narrator**
  ([`sim/report/src/narration.rs:158`](../sim/report/src/narration.rs))
  renders `ArchetypeDerived` as "Developmental archetype — {label}
  (dominant lever {lever}, {cognition} cognition)." and
  `ArchetypeEndpoint` as "{civ} reaches its endpoint — {description}".
- **Python narrator** ([`narrate.py`](../narrate.py)) folds
  `archetype_derived` into the run's **opening** (the developmental
  archetype the world and people lean on) and renders
  `archetype_endpoint` in the **closing** as the run's climactic
  divergent fate.

So a run opens by stating its developmental archetype and closes by
rendering the endpoint fate that archetype reached.
</content>
</invoke>
