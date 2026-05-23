# Recognition

Recognition translates emergent physics state into discrete named
phenomena that civs can hypothesise about. There is **no authored
phenomenon catalogue** — phenomena emerge from running the physics:
tides happen because water moves under gravitational pull,
lightning from charge accumulation and discharge, fire from
oxidiser + fuel + activation energy. The recognition layer is the
*labelling* of these emergent patterns, not their *generation*.

For deeper detail per crate:
[`sim/recognition/src/lib.rs`](../sim/recognition/src/lib.rs) for
the `RecognitionLibrary` + `Signature` machinery,
[`sim/recognition/src/templates.rs`](../sim/recognition/src/templates.rs)
for the 39 authored templates, and
[`sim/recognition/README.md`](../sim/recognition/README.md) for
the crate-level overview. For how civs derive law coefficients
from recognition events see [discovery.md](discovery.md); for how
species sensoria gate which templates a civ can perceive see
[species.md](species.md).

## RecognitionLibrary + templates

A `RecognitionTemplate` is `(id, name, signature, tags, channels)`
([`lib.rs:313`](../sim/recognition/src/lib.rs)). The library is
constructed once per run in
`RecognitionLibrary::earth_like_default`
([`templates.rs:20`](../sim/recognition/src/templates.rs)) and
ships **43 authored templates** at boot (ids 1–39 plus the
per-substrate `surface_solvent_*` block at 50–53).

Each tick, `RecognitionLibrary::scan_with_discovered`
([`lib.rs:404`](../sim/recognition/src/lib.rs)) iterates
templates × cells in deterministic order and emits a
`Firing { template_id, cell }` for every cell whose physics state
matches the template's signature.

### Template catalogue (39 authored)

- **Earth-baseline** (ids 1–14) — `fire`, `lightning_buildup`,
  `ice_present`, `vapour_present`, `surface_water`,
  `magnetic_field_strong`, `thermal_gradient`, `flood_zone`,
  `cold_zone`, `fertile_land`, `auroral_activity`,
  `harmonic_resonance`, `static_field_gradient`, `tidal_extremum`.
- **Planet-archetype-specific** (ids 15–24) — `cryovolcanism`,
  `ice_quake`, `pressure_storm`, `metallic_hydrogen_signal`,
  `piezoelectric_pulse`, `magnetic_lodestone`, `hydrocarbon_seep`,
  `superconductor_resonance`, `reducing_storm`, `hazy_obscuration`.
- **Seasonal** (ids 25–28) — `seasonal_thaw`, `polar_winter`,
  `equatorial_wet`, `axial_extremum`. All read `MonthIn` modulo
  the planet's `orbital_period_months`.
- **Substrate-specific** (ids 29–35) — `silicate_resonance`,
  `methane_seep`, `ammoniacal_storm`, `tidally_locked_terminator`,
  `cryo_lake`, `crystal_growth`, `aurora_polar`.
- **Substrate-neutral solvent-cycle** (ids 36–39) —
  `solvent_humid_band`, `desiccated_band`, `condensation_storm`,
  `windy_strait`. Renamed from earlier Earth-water-centric names
  (`tropical_moist` / `dry_zone`) so they fire correctly on
  methane / ammonia / silicate substrates.
- **Per-substrate surface-solvent** (ids 50–53) —
  `surface_solvent_water`, `surface_solvent_ammonia`,
  `surface_solvent_methane`, `surface_solvent_silicate_melt`
  (see [per-substrate surface_solvent templates](#per-substrate-surface-solvent-templates) below).

## Signatures

`Signature` is a small enum covering the matchable shapes
([`lib.rs:187`](../sim/recognition/src/lib.rs)):

| Variant | Meaning |
|---------|---------|
| `Above(field, threshold)` | Field reading exceeds threshold. |
| `Below(field, threshold)` | Field reading below threshold. |
| `AbsAbove(field, threshold)` | `\|reading\|` exceeds threshold (used for signed-charge templates). |
| `All(subs)` | Composite — every sub-signature matches. |
| `Any(subs)` | Composite — at least one sub-signature matches. |
| `MonthIn(start, end)` | `tick % orbital_period_months` falls in the range (supports wrap-around). |
| `Hemisphere(h)` | Cell row lies in the named hemisphere — used with `MonthIn` for seasonally-flipping templates. |
| `InClimateBand(band)` | Climate-relative band match (DeepCold / Cold / ProductiveBand / Hot). |
| `AboveIgnition` | Cell temperature exceeds the planet-derived combustion-ignition threshold. |
| `TidallyLockedTerminator` | Cell sits in the day-night boundary band on a tidally-locked planet. |

`Field` is the channel selector
([`lib.rs:34`](../sim/recognition/src/lib.rs)):
`Temperature`, `Charge`, `WaterDepth`, `Substance(s)`,
`MagneticMagnitude` (scalar `\|B\|`), `WindMagnitude` (scalar
`\|v\|`).

## Channels

`ChannelKind` ([`lib.rs:294`](../sim/recognition/src/lib.rs))
enumerates the **15 modality channels** that mediate between
recognition and species perception:

`AcousticAir`, `AcousticWater`, `Seismic`, `VisualLight`,
`VisualPolarization`, `Bioluminescent`, `ChemicalPheromone`,
`ChemicalTaste`, `Tactile`, `ElectricField`, `MagneticSense`,
`InfraredThermal`, `RadioNative`, `Gestural`, `Postural`.

Distinct from the **physics channels** the hypothesizer fits
against, listed in
[`sim/civ/src/discovery/channels.rs:13`](../sim/civ/src/discovery/channels.rs):
`Temperature`, `WaterDepth`, `ChargeMagnitude`, `Elevation`,
`Fuel`, `Oxidiser`, `Vapour`, `Ice`, `Fossil`, `MagneticField`.
The recognition `ChannelKind` is a sensory channel (how the
species perceives); the discovery `Channel` is a fittable physics
axis. The mapping from a species' `ModalityKind` to perceivable
discovery channels lives in `channels_for_modality`
([`channels.rs:107`](../sim/civ/src/discovery/channels.rs)).

Each `RecognitionTemplate` declares `channels: &[ChannelKind]` —
the sensory modalities that natively perceive it. A `fire` firing
is perceivable through `VisualLight`, `InfraredThermal`,
`ChemicalTaste`, and `Tactile`; a `magnetic_field_strong` firing
requires `MagneticSense`.

## ModalityKind gating

A civ only "sees" a firing if the species' `ModalityKind` set
intersects the template's `channels`. This is the **sensorium
gate** — different species, different sciences.

Per-species perceivability is computed via
`Species::perceivable_firings`
([`sim/species/src/species.rs:256`](../sim/species/src/species.rs))
which filters firings through `Species::can_perceive(template_id)`.
The civ-level `Civ::perceivable_firings`
([`sim/civ/src/observation.rs:121`](../sim/civ/src/observation.rs))
unions the species baseline with `extra_perceivable_templates` —
the latter grows when a sensorium-extending tool unlocks a new
`ChannelKind`.

A pure-tactile species sees `fire` (Tactile is in `fire`'s
channels) but not `magnetic_field_strong` (no `MagneticSense`).
A bioluminescent / postural-only species perceives nothing —
output-only modalities don't grant perception — and falls back to
a minimum-viable `Temperature + Elevation` contact set
([`channels.rs:168`](../sim/civ/src/discovery/channels.rs)).

### Two-pass filtering at run start

1. **Physical-presence pass** — does the physics for this run ever
   produce the signature? Templates whose required field never
   reaches threshold are dropped from the per-civ candidate set.
2. **Sensorium pass** — split into `perceivable-now` (default
   species sensorium can detect) vs `latent` (physics produces it
   but the species can't sense it without instrument tech).

Latent templates lie dormant until the civ invents the relevant
instrument (telescope, microscope, magnetometer-equivalent), at
which point they promote to perceivable-now and the recognition
layer begins firing them for that civ.

## Sensorium-extending tech

Five `ToolKind`s grant new `ChannelKind`s on unlock (see
`ToolKind::granted_channels` in
[`sim/civ/src/tech/identity.rs:493`](../sim/civ/src/tech/identity.rs)):

| Tool | Grants |
|------|--------|
| `ThermalSensor` | `InfraredThermal` |
| `FieldSensor` | `ElectricField` |
| `MagneticSensor` | `MagneticSense` |
| `DistanceImaging` | — (range extension; no new channel) |
| `RemoteAcoustic` | — (range extension; no new channel) |

On unlock, the civ's `extra_perceivable_templates` set grows to
include every authored + discovered template whose `channels`
intersect the newly-granted modalities. The hypothesizer's
candidate set is refreshed via
`refresh_perceivable_with_channels`
([`sim/civ/src/discovery/hypothesizer.rs:438`](../sim/civ/src/discovery/hypothesizer.rs))
so subsequent ticks accumulate samples on the newly-visible
templates. See [tech.md#sensorium-extending-tools](tech.md#sensorium-extending-tools)
for the per-tool unlock thresholds.

## Climate bands relative to planet

`Signature::InClimateBand` reads bands derived from the actual
cell-temperature distribution
(`mean ± gradient/2`, quartered into `DeepCold` / `Cold` /
`ProductiveBand` / `Hot`).

The `PlanetContext` ([`lib.rs:119`](../sim/recognition/src/lib.rs))
carries `mean_temperature` and `temperature_gradient` per-run; the
band computation is in `PlanetContext::band_match`
([`lib.rs:155`](../sim/recognition/src/lib.rs)).

A 232 K sub-surface ocean's `polar_winter` fires on its own
gradient — not silently never-fires because 232 < 240. A 380 K
desert civ's `cold_zone` sits at ~340 K — its own cold, not
Earth's. See also [`sim/world/src/climate.rs`](../sim/world/src/climate.rs)
for upstream calibration.

## Seasonal modulation

`Signature::MonthIn` reads
`(tick % ctx.orbital_period_months) as month`. A 16-month world's
`polar_winter` fires across 3/16 of *its* year, not 3/12 of an
Earth year. The seasonal-insolation table
`t_eq_per_row_per_season` is sized to `[rows × orbital_months]`.

`polar_winter` ([`templates.rs:507`](../sim/recognition/src/templates.rs))
is hemisphere-scoped via composition: `Cold` band AND
`(Northern AND MonthIn(11, 1))` OR `(Southern AND MonthIn(5, 7))`.
The signature fires per-cell so only the wintering hemisphere
darkens at a time.

## Combustion ignition relative to atmosphere

`Signature::AboveIgnition` reads the planet's
`ctx.ignition_threshold` (set in `sim_core::build_laws` from
atmosphere richness). Replaces the Earth-fixed 500 K threshold
previously hardcoded in the `fire` template:

- Oxidising atmospheres: ~500 K.
- Hazy atmospheres: ~700 K.
- Reducing atmospheres: ~900 K (effectively above habitable cell
  temperatures — `fire` never fires).

The latter is the substrate-divergence pivot for the combustion
chain — see [tech.md#per-substrate-availability](tech.md#per-substrate-availability).

## Per-substrate surface_solvent templates

The original `surface_water` (template 5) is retained for legacy
compatibility. Four substrate-specific templates fire on
`Field::WaterDepth` (a solvent-agnostic measure of standing
surface liquid) with substrate-appropriate thresholds and channel
sets ([`templates.rs:836-883`](../sim/recognition/src/templates.rs)):

| Id | Name | Depth threshold | Sensory channels |
|---:|------|----------------:|------------------|
| 50 | `surface_solvent_water` | 1.0 m | `VisualLight`, `Tactile` |
| 51 | `surface_solvent_ammonia` | 1.0 m | `ChemicalTaste`, `Tactile`, `AcousticAir` |
| 52 | `surface_solvent_methane` | 0.3 m | `AcousticWater`, `Tactile`, `ChemicalTaste` |
| 53 | `surface_solvent_silicate_melt` | 0.2 m + `T > 1687 K` | `Tactile`, `Seismic`, `VisualLight` |

The methane template's lower threshold reflects Titan's
typical-< 1 m lake depths; the silicate template gates on
temperature so warm-water cells on Earth-like planets never
accidentally fire it. The point: an ammonia-substrate civ
perceives its solvent through chemical-taste and acoustics
where an aqueous civ perceives water through sight and touch —
the recognition layer surfaces structurally different sciences
for the same emergent phenomenon.

## DiscoveredTemplate

When a civ confirms a `ThresholdStep` law on a `(template,
channel)` pair AND no existing template's
`Signature::Above`/`Below`/`AbsAbove` covers the same
`(field, threshold)` within 20% relative tolerance, the species
canon adopts a new `DiscoveredTemplate`
([`lib.rs:346`](../sim/recognition/src/lib.rs)).

### Discovery rule

The full rule lives in
[`sim/civ/src/discovery/emergence.rs:267`](../sim/civ/src/discovery/emergence.rs):

1. The fitted form must be `ThresholdStep` (`params[2]` is the
   cutpoint).
2. The relation's `channel` must map to a recognition `Field`
   (`channel_to_field`, line 96). `Elevation` returns `None` —
   the static planet feature isn't a tickable cell field.
3. The cutpoint must not be within 20% of any existing
   `Signature::Above`/`Below`/`AbsAbove` on the same field
   (`already_covered`, line 133). Stops the discovery from
   re-deriving authored templates over and over.
4. No previously-discovered template covers the same pair.

When all four hold, mint a `DiscoveredTemplate` with:

- `id` = `species.next_discovered_template_id` (then increment).
- `signature` = `Signature::Above(field, threshold)`.
- `channels` = `natural_channels(field)` — the modalities through
  which that field is naturally perceived.
- `tags` = `[FormTag::Threshold]` so downstream form selection
  prefers threshold-style fits on the new template.

The id space splits at `DISCOVERED_TEMPLATE_ID_START = 1000`
([`lib.rs:371`](../sim/recognition/src/lib.rs)) — authored
templates occupy 1..999, discovered start at 1000.

### Cadence

`is_emergence_tick` ([`emergence.rs:201`](../sim/civ/src/discovery/emergence.rs))
fires every `EMERGENCE_CHECK_PERIOD_TICKS = 600` (~50 sim-years
at 12 ticks/year). The substrate-aware variant
`is_emergence_tick_for_metabolism` stretches the period by the
inverse of the planet's metabolism so slow-substrate worlds run
the same number of checks per generation as fast ones.

### Inheritance

Discovered templates enter `Species::discovered_templates`. They
fire through the same `scan_with_discovered` pipeline as authored
templates ([`lib.rs:404`](../sim/recognition/src/lib.rs)) and
contribute their `tags` to the species' available form vocabulary.

Subsequent civs of the same species inherit the broader
vocabulary; the discovered set persists across civ collapse
boundaries. This is how a species *invents new phenomena* —
naming the threshold pattern it fitted, not just discovering laws
over pre-named ones.

`TemplateDiscovered { species_id, template_id, signature, source }`
emits when a new discovered template is minted.

## Different worlds, different sciences

A piezoelectric rocky world, a hydrocarbon-rich one, and a
silicate magma world host structurally different scientific
traditions because crust + atmosphere + biosphere + species
modalities shape:

- **What fires** — physical-presence pass drops impossible
  templates (a no-oxidiser world never confirms `fire`; a
  zero-magnetosphere world never confirms `magnetic_field_strong`).
- **What's perceivable** — sensorium pass gates by species
  modalities; a tactile-only species literally cannot see
  `auroral_activity`.
- **What gets discovered first** — observation order is set by
  baseline regional templates and which channels figures sample
  most.
- **Which forms are available** — the form vocabulary itself is
  gated by which structural `FormTag`s the civ's perceivable
  templates carry. A civilisation that never observes a `Periodic`
  template literally cannot propose `PeriodicSine`. See
  [discovery.md#form-vocabulary](discovery.md#form-vocabulary).
