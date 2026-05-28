# Recognition

Recognition translates emergent physics state into discrete named
phenomena that civs can hypothesise about. There is **no authored
phenomenon catalogue** in the sense of "this run knows about exactly
these phenomena up front" — phenomena emerge from running the
physics. The
[`RecognitionLibrary`](../sim/recognition/src/lib.rs) ships a
default template set that captures the *shapes* the physics layer
reliably produces; civs then discover regularities *within* that
firing stream and may mint new templates as their canon grows.

For deeper detail per crate:
[`sim/recognition/README.md`](../sim/recognition/README.md). For how
civs derive law coefficients from recognition events see
[discovery.md](discovery.md). For how species sensoria gate which
templates a civ can perceive see [species.md](species.md).

## RecognitionLibrary + templates

A `RecognitionTemplate` is `(id, name, signature, tags, channels)`
([`sim/recognition/src/lib.rs:312`](../sim/recognition/src/lib.rs)):

- `id: u32` — stable identifier; authored templates occupy 1..=53,
  discovered templates start at
  `DISCOVERED_TEMPLATE_ID_START = 1000`
  ([`lib.rs:371`](../sim/recognition/src/lib.rs)).
- `name: &'static str` — snake-case label surfaced in events.
- `signature: Signature` — the pattern matched per-cell per-tick.
- `tags: &'static [FormTag]` — structural tags driving form
  vocabulary (see [discovery.md#form-vocabulary](discovery.md#form-vocabulary)).
- `channels: &'static [ChannelKind]` — modality channels that
  natively sense this template.

`RecognitionLibrary::earth_like_default()`
([`templates.rs:20`](../sim/recognition/src/templates.rs)) carries
the M2/M3 default set of ~40 authored templates covering signatures
the physics layer reliably produces:

- **Earth-baseline** — `fire` (id 1), `lightning_buildup` (2),
  `ice_present` (3), `vapour_present` (4), `surface_water` (5),
  `magnetic_field_strong` (6), `thermal_gradient` (7), `flood_zone`
  (8), `cold_zone` (9), `fertile_land` (10).
- **Field-and-resonance** — `auroral_activity` (11),
  `harmonic_resonance` (12), `static_field_gradient` (13),
  `tidal_extremum` (14).
- **Archetype-specific** — `cryovolcanism` (15), `ice_quake` (16),
  `pressure_storm` (17), `metallic_hydrogen_signal` (18),
  `piezoelectric_pulse` (19), `magnetic_lodestone` (20),
  `hydrocarbon_seep` (21), `superconductor_resonance` (22),
  `reducing_storm` (23), `hazy_obscuration` (24).
- **Seasonal** — `seasonal_thaw` (25), `polar_winter` (26),
  `equatorial_wet` (27), `axial_extremum` (28). Read month-of-year
  via `Signature::MonthIn` + `Hemisphere`.
- **Substrate-native** — `silicate_resonance` (29), `methane_seep`
  (30), `ammoniacal_storm` (31), `tidally_locked_terminator` (32),
  `cryo_lake` (33), `crystal_growth` (34), `aurora_polar` (35).
- **Substrate-neutral solvent-cycle** — `solvent_humid_band` (36),
  `desiccated_band` (37), `condensation_storm` (38), `windy_strait`
  (39). Names describe the *solvent* (water on Earth, methane on
  Titan, ammonia on cold-reducing worlds, etc.) so civs on any
  substrate correctly label their observations.
- **Per-substrate surface-solvent** (ids 50–53) — see
  [Per-substrate surface_solvent templates](#per-substrate-surface_solvent-templates).

The library's `scan` and `scan_with_discovered` methods
([`lib.rs:392`](../sim/recognition/src/lib.rs)) iterate
template-major then cell-major per tick and emit a `Firing
{ template_id, cell }` per match. Authored templates fire first,
discovered templates second; ordering is stable as the discovered
set grows mid-run.

## Channels

`ChannelKind` ([`lib.rs:294`](../sim/recognition/src/lib.rs)) is
the 15-variant enum naming the sensory modalities a species (or a
species + civ tool) may possess. Recognition templates carry
`channels: &'static [ChannelKind]` declaring which modalities can
natively sense them; the civ's perceivable-template set is the
templates whose channel list intersects the union of species-native
modalities and civ-tool-granted channels.

The spec calls out the discovery-pipeline channel selectors
(physics `Channel`, [`sim/civ/src/discovery/channels.rs`](../sim/civ/src/discovery/channels.rs))
which are a *different* enum living next to recognition's
`ChannelKind`:

- **Recognition `ChannelKind`** — what a species senses (mapped to
  biological organs):
  `AcousticAir`, `AcousticWater`, `Seismic`, `VisualLight`,
  `VisualPolarization`, `Bioluminescent`, `ChemicalPheromone`,
  `ChemicalTaste`, `Tactile`, `ElectricField`, `MagneticSense`,
  `InfraredThermal`, `RadioNative`, `Gestural`, `Postural`.
- **Discovery `Channel`** — what the hypothesizer reads from
  physics state:
  `Temperature`, `WaterDepth`, `ChargeMagnitude`, `Elevation`,
  `Fuel`, `Oxidiser`, `Vapour`, `Ice`, `Fossil`, `MagneticField`,
  `Resonance` (see [discovery.md#channels](discovery.md#channels)).

The `channels_for_modality` table in
[`sim/civ/src/discovery/channels.rs:107`](../sim/civ/src/discovery/channels.rs)
maps each `ModalityKind` to the set of discovery `Channel`s the
species can sample through it — the bridge between "what I can
sense" and "what fields I can fit laws over."

## Signatures

`Signature` ([`lib.rs:186`](../sim/recognition/src/lib.rs)) is the
small enum of matchable shapes:

| Variant | Matches |
|---------|---------|
| `Above(Field, Real)` | `Field` reading exceeds threshold. |
| `Below(Field, Real)` | `Field` reading below threshold. |
| `AbsAbove(Field, Real)` | `|reading|` exceeds threshold. |
| `All(Vec<Signature>)` | All sub-signatures match. |
| `Any(Vec<Signature>)` | At least one sub-signature matches. |
| `MonthIn(start, end)` | `tick % orbital_period_months` in inclusive range (supports wrap-around). |
| `Hemisphere(Northern/Southern)` | Cell's row falls in the named half of the grid (rows < `height/2` are northern). |
| `InClimateBand(ClimateBand)` | Cell temperature falls in the named climate band. |
| `AboveIgnition` | Cell temperature exceeds the planet's combustion-ignition threshold (atmosphere-derived in `sim_core::build_laws`). |
| `TidallyLockedTerminator` | Cell sits in the terminator band on a tidally-locked planet (columns within 1 of `width/4` or `3·width/4`). |

`Field` ([`lib.rs:33`](../sim/recognition/src/lib.rs)) selects what
physics scalar the signature reads:
`Temperature`, `Charge`, `WaterDepth`, `Substance(Substance)`,
`MagneticMagnitude`, `WindMagnitude`, `Resonance`. `MagneticMagnitude`
and `WindMagnitude` derive on-the-fly from per-cell vector components
(`sqrt(B_q² + B_r²)` / `sqrt(v_q² + v_r²)`); `Resonance` reads the
speculative per-cell resonance field (see below).

`Field::Resonance` reads the per-cell resonance field added for the
field/resonance civilizational lever. Two templates gate on it —
`resonance_field_active` (id 54, fires above 1 unit) and
`attention_coherence` (id 55, the sustained high-field state above 5
units) — both read through the `ElectricField` / `MagneticSense` /
`RadioNative` channels, so a field-sensing biology on a resonance-rich
world does genuine resonance science. See
[archetype.md](archetype.md).

## ModalityKind gating

Each `RecognitionTemplate` carries `channels: &'static
[ChannelKind]`. A civ's perceivable-template set is computed as:

```
perceivable_now = templates whose channels ∩ available_channels ≠ ∅
available_channels = species_modality_channels ∪ tool_granted_channels
```

`species_modality_channels` comes from `Species::modalities`
(the species sensorium); `tool_granted_channels` comes from
`ToolKind::granted_channels`
([`sim/civ/src/tech/identity.rs:493`](../sim/civ/src/tech/identity.rs))
unioned across `Civ::unlocked_tools`.

Templates whose channel list never intersects available channels
are **latent** — physics still produces the signature but the civ
can't sense it. Latent templates promote to perceivable-now when a
relevant sensorium-extending tool unlocks (see
[Sensorium-extending tech](#sensorium-extending-tech)).

The civ's hypothesizer candidate cross-product
(see [discovery.md#two-parallel-sample-tracks](discovery.md#two-parallel-sample-tracks))
is built from `perceivable_template_ids × perceivable_channels` —
both axes restricted by the sensorium so a magnetic-sense species
and a visual-light species draw structurally different
observational manifolds.

## Sensorium-extending tech

A subset of `ToolKind` grants new `ChannelKind` channels on unlock
via the `granted_channels` method
([`sim/civ/src/tech/identity.rs:493`](../sim/civ/src/tech/identity.rs)):

| `ToolKind` | Tier | Grants |
|------------|-----:|--------|
| `ThermalSensor` | 2 | `InfraredThermal` |
| `FieldSensor` | 3 | `ElectricField` |
| `MagneticSensor` | 4 | `MagneticSense` |

When the tool unlocks, the civ's `available_channels` set widens
and the perceivable-template set is recomputed. The
hypothesizer's `refresh_perceivable_with_channels`
([`sim/civ/src/discovery/hypothesizer.rs:438`](../sim/civ/src/discovery/hypothesizer.rs))
then regenerates the candidate cross-product: existing
`(template, channel)` pairs keep their sample buffers and
confirmations (stable `relation_id` from `relation_id_for`
preserves them); new pairs get fresh empty buffers.

Two tools register as sensorium-extending in `prereq_channels` but
grant no new channel — they extend *range* on an existing channel:

- `DistanceImaging` (tier 3, requires native `VisualLight`) —
  telescopes / microscopes / cartographic optics.
- `RemoteAcoustic` (tier 2, requires native `AcousticAir` or
  `AcousticWater`) — sonar / echolocation / horn-and-drum
  networks.

These add no new templates to perceivable-now until the template
catalog grows range-gated phenomena; their effect on the civ is
through the discovery-rate, expansion-rate, and transmission-
fidelity bonuses they carry
([discovery.md#tech-multiplier](tech.md#tech-multiplier-discovery-rate)).

Tier-5 narrative tools (`BioelectricResonator`,
`FieldPropulsionEngine`, `MetamaterialLattice`) require existing
channels as prereqs but grant none — their effect on the sim is
the `TechUnlocked` event itself plus the transcendence trigger.

## Per-substrate surface_solvent templates

Recognition Sprint 2 Item 8 added a substrate-aware family of
"standing surface liquid" templates so civs on non-aqueous worlds
name their own solvent rather than re-using `surface_water` for
ammonia or methane. The originals share the same `Above(WaterDepth,
threshold)` machinery — `Field::WaterDepth` is *solvent-agnostic*
in the model (the surface column of whatever phase-changes from the
planet's solvent, regardless of chemistry).

| Id | Name | Threshold | Channels |
|---:|------|-----------|----------|
| 50 | `surface_solvent_water` | `> 1 m` | `VisualLight`, `Tactile` |
| 51 | `surface_solvent_ammonia` | `> 1 m` | `ChemicalTaste`, `Tactile`, `AcousticAir` |
| 52 | `surface_solvent_methane` | `> 0.3 m` | `AcousticWater`, `Tactile`, `ChemicalTaste` |
| 53 | `surface_solvent_silicate_melt` | `> 0.2 m` AND `T > 1687 K` | `Tactile`, `Seismic`, `VisualLight` |

The original `surface_water` (id 5) is retained for legacy
compatibility and substrate-divergence backward-chaining (its
confirmed-relation gate is named in several `tool_prereqs`
chains). The per-substrate variants cover:

- **Water** — standard 1 m floor.
- **Ammonia** — same 1 m floor; ammonia is comparably fluid at its
  liquid range.
- **Methane** — shallow lakes (Titan's lakes average < 1 m);
  threshold lowered to 0.3 m.
- **Silicate melt** — magma ponds are thin sheets; 0.2 m floor
  combined with a > 1687 K temperature gate (silicate freeze) so
  warm-water cells on Earth-like planets never accidentally trip
  it.

The channel sets reflect substrate physics: methane reads through
`AcousticWater` (sound carries through liquid hydrocarbons),
ammonia reads strongly through `ChemicalTaste` (volatile signatures
in the atmosphere), silicate melt reads through `Seismic` (the
crust transmits structural vibration).

See
[`sim/recognition/src/templates.rs:836`](../sim/recognition/src/templates.rs)
for the authored definitions.

## DiscoveredTemplate

When a civ confirms a `ThresholdStep` law on a `(template, channel)`
pair, the species canon may adopt a new `DiscoveredTemplate`
([`sim/recognition/src/lib.rs:346`](../sim/recognition/src/lib.rs)).
This is how a species **invents new phenomena** — naming the
threshold pattern they fitted, not just discovering laws over
pre-named ones.

`DiscoveredTemplate` carries the same `Signature` machinery as
authored templates plus a few biographical fields:

```rust
pub struct DiscoveredTemplate {
    pub id: u32,              // ≥ DISCOVERED_TEMPLATE_ID_START (1000)
    pub name: String,         // e.g. "discovered_field_temperature_civ_3_t1200"
    pub signature: Signature, // typically Signature::Above(field, threshold)
    pub tags: Vec<FormTag>,   // typically vec![FormTag::Threshold]
    pub channels: Vec<ChannelKind>,
    pub discovered_at_tick: u64,
    pub discovered_by_civ_id: u32,
    pub origin_template_id: u32, // the static template the fit was performed against
}
```

### Discovery rule

`propose_discovered_templates` in
[`sim/civ/src/discovery/emergence.rs:221`](../sim/civ/src/discovery/emergence.rs)
runs per-civ at every `EMERGENCE_CHECK_PERIOD_TICKS = 600`
(≈ 50 sim-years; substrate-metabolism-aware via
`is_emergence_tick_for_metabolism`). For each confirmed relation in
the civ's active figures' hypothesizers (sorted by `relation_id`
for determinism):

1. The fitted form must be `ThresholdStep` (the cutpoint sits at
   `params[2]` in fit-space; `params_in_real_units()` rescales it
   back to SI).
2. The relation's `channel` must map to a recognition `Field` via
   `channel_to_field` — `Elevation` returns `None` (no
   recognition-side equivalent yet) so elevation-channel laws
   don't mint templates.
3. The `(field, threshold)` must not already be covered by any
   authored template's `Signature::Above` / `Below` / `AbsAbove`
   within `EMERGENCE_THRESHOLD_TOLERANCE = 20%` relative (the
   `max(|threshold|, 1.0)` scale keeps the gap positive near zero).
4. No previously-discovered template covers the same pair.

When all four hold, mint a `DiscoveredTemplate` with:
- `id = species.next_discovered_template_id` (then increment).
- `signature = Signature::Above(field, threshold)`.
- `channels = natural_channels(field)` — heat reads through
  `InfraredThermal` + `VisualLight`, charge through
  `ElectricField`, water depth through `Tactile` +
  `AcousticWater`, substance through `ChemicalTaste`, magnetic
  magnitude through `MagneticSense`, wind magnitude through
  `Tactile` + `AcousticAir`.
- `tags = [FormTag::Threshold]` so downstream form selection
  prefers threshold-style fits on the new template.

The species canon (`Species::discovered_templates`) is the
species-level state; subsequent civs of the same species inherit
the broader vocabulary and can chain further laws on top of it.
The discovered set persists across civ collapse boundaries.

### Subsequent firing

Discovered templates feed into
`RecognitionLibrary::scan_with_discovered`
([`lib.rs:404`](../sim/recognition/src/lib.rs)) which scans
authored templates first, then any species-discovered templates the
caller threads in. Discovered templates produce the same `Firing`
records as authored ones, so downstream consumers (per-cell
observation accumulators, civ hypothesizers, tool-prereq lookups,
post-run report) treat them uniformly via `template_id`.

A `TemplateDiscovered` event fires on mint
(see [events](#events) below).

## Two-pass filtering at run start

1. **Physical-presence pass** — does the physics for this run ever
   produce the signature? Templates whose required field never
   reaches threshold on any cell are dropped from the run's active
   library.
2. **Sensorium pass** — split surviving templates into
   `perceivable-now` (species-native channels intersect) vs
   `latent` (physics produces the signature but the species can't
   sense it without instrument tech).

Latent templates promote to perceivable-now when the relevant
sensorium-extending tool unlocks.

## Climate bands relative to planet

`ClimateBand` ([`lib.rs:107`](../sim/recognition/src/lib.rs))
derives from a normalised offset `o = (T - mean) / gradient` that
sits in approximately `[-0.5, +0.5]` across the planet. Quartering
that range:

- `DeepCold` → `o ≤ -0.5` (polar boundary).
- `Cold` → `o < -0.25` (subpolar; includes `DeepCold`).
- `ProductiveBand` → `-0.25 ≤ o ≤ 0.25` (mid-latitude).
- `Hot` → `o > 0.25` (subtropical and equator).

The `band_match` method at
[`lib.rs:155`](../sim/recognition/src/lib.rs) implements the actual
match. Bands derive from the `PlanetContext` handed to
`RecognitionLibrary::scan` (built once at run start in `sim/core`
from the sampled planet; never mutated during the run). A 200 K
sub-surface ocean's "polar winter" reads relative to its own
gradient, not absolute 240 K — every world's polar / temperate /
tropical cells map to the same band labels.

## Seasonal modulation

`Signature::MonthIn(start, end)` reads
`tick % orbital_period_months`
([`lib.rs:222`](../sim/recognition/src/lib.rs)). A 16-month world's
polar winter fires across 3/16 of *its* year, not 3/12 of an Earth
year. `PlanetContext.orbital_period_months` is the per-planet
modulo; tests default to 12 via `PlanetContext::earth_like()`.

Wrap-around ranges are supported — `MonthIn(11, 1)` fires on
months 11, 0, and 1 (Nov / Dec / Jan).

## Different worlds, different sciences

Crust + atmosphere + biosphere + species modalities together shape:

- **What fires** — physical-presence pass drops impossible
  templates. A reducing-atmosphere world never fires `fire` because
  `AboveIgnition` is gated on the planet-derived ignition threshold
  and `Above(Oxidiser, 0)` is gated on atmospheric chemistry.
- **What's perceivable** — sensorium pass gates by species + civ
  tool channels.
- **What gets discovered first** — observation order is set by
  baseline regional templates and which channels figures sample
  most.

The form vocabulary itself is gated by which structural tags the
civ's perceivable templates carry, so civilisations that never
observe a `Periodic` template literally cannot fit sinusoidal
forms. See [discovery.md#form-vocabulary](discovery.md#form-vocabulary).

## Events

- `TemplateDiscovered(species_id, template_id, signature, source)`
  — emergent template committed to the species canon. Carries the
  authored or previously-discovered template id whose confirmed-fit
  produced the proposal.

Firing events themselves are not emitted into the protocol log
(too noisy at thousands of fires per tick); they live as
`Firing` records inside the `RecognitionLibrary::scan` return
value and feed the hypothesizer's `observe_cells` pass.
