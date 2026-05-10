# Recognition

Recognition translates emergent physics state into discrete named
phenomena that civs can hypothesise about. There is **no authored
phenomenon catalogue** — phenomena emerge from running the physics:
tides happen because water moves under gravitational pull,
lightning from charge accumulation and discharge, fire from
oxidiser + fuel + activation energy.

For deeper detail per crate:
[`sim/recognition/README.md`](../sim/recognition/README.md). For
how civs derive law coefficients from recognition events see
[discovery.md](discovery.md). For how species sensoria gate which
templates a civ can perceive see [species.md](species.md).

## Templates

A template is `(id, name, signature)`. The signature is matched
against per-cell physics state per tick; on match, a
`(template_id, cell)` event fires.

39 authored templates ship at boot:

- **Earth-baseline** — `fire`, `lightning_buildup`, `fertile_land`,
  `cold_zone`, `coastal_swing`, `harmonic_resonance`,
  `magnetic_field_strong`, etc.
- **Archetype-specific** — `cryovolcanism`,
  `metallic_hydrogen_signal`, `piezoelectric_pulse`,
  `magnetic_lodestone`, `superconductor_resonance`.
- **Seasonal** — `seasonal_thaw`, `polar_winter`,
  `equatorial_wet`, `axial_extremum`. Read month-of-year via
  `Signature::MonthIn` + `Hemisphere`.
- **Substrate-native** — `silicate_resonance`, `methane_seep`,
  `ammoniacal_storm`, `cryo_lake` (Hydrocarbon),
  `crystal_growth` (Silicate), `aurora_polar` (cross-substrate).
- **Tidally-locked** — `tidally_locked_terminator` fires across the
  day-night boundary band on `is_tidally_locked` planets.
- **M1b physics templates** — `solvent_humid_band`,
  `desiccated_band`, `condensation_storm`, `windy_strait` (read
  the new wind / hydrology / pressure fields). Substrate-neutral
  names (renamed from the original `tropical_moist`/`dry_zone`
  set) to apply across non-aqueous substrates.

## Signatures

`Signature` is a small enum covering the matchable shapes:

- `Above(field, threshold)` — `field` reading exceeds threshold.
- `Below(field, threshold)` — `field` reading below threshold.
- `Between(field, low, high)` — `field` within band.
- `Climate(band)` — climate band (DeepCold / Cold /
  ProductiveBand / Hot) matches.
- `MonthIn(set)` + `Hemisphere(h)` — seasonal-template machinery.
- `TidallyLockedTerminator` — composite signature for tidal-lock
  worlds.
- `TemporalDelta(field, threshold)` — fires on per-tick deltas
  (drives diffusion-coefficient recovery via the measurement
  track).

`Field` is the channel index — `Temperature`, `Charge`,
`WaterDepth`, `Vapour`, `Pressure`, `MagneticMagnitude`,
`WindMagnitude`, `UpperTemperature`, etc.

## Two-pass filtering at run start

1. **Physical-presence pass** — does the physics for this run ever
   produce the signature? Templates whose required field never
   reaches threshold are dropped.
2. **Sensorium pass** — split into `perceivable-now` (default
   species sensorium can detect) vs `latent` (physics produces
   it but the species can't sense it without instrument tech).

Latent templates dormant until the civ invents the relevant
instrument (telescope, microscope, magnetometer-equivalent), at
which point they promote to perceivable-now and the recognition
layer begins firing them.

## Climate bands relative to planet

Recognition templates that name climate-relative phenomena read
bands derived from the actual cell-temperature distribution
(`mean ± gradient/4` quartered into DeepCold / Cold /
ProductiveBand / Hot). A 232 K sub-surface ocean's "polar winter"
fires on its own gradient instead of silently never-firing because
232 < 240. See
[`sim/world/src/climate.rs`](../sim/world/src/climate.rs).

## Seasonal modulation

`Signature::MonthIn` reads `tick % orbital_period_months`. A
16-month world's polar winter fires across 3/16 of *its* year, not
3/12 of an Earth year. The seasonal-insolation table
`t_eq_per_row_per_season` is sized to `[rows × orbital_months]`.

## Emergent recognition templates

When a civ confirms a `ThresholdStep` law on a `(template,
channel)` pair AND no existing template's `Signature::Above`
covers the same `(field, threshold)` within 20% tolerance, the
species canon adopts a new `DiscoveredTemplate`:

- `TemplateDiscovered` event emits.
- The new template enters the species recognition library at the
  next run-start filter.
- Subsequent civs of the same species inherit the broader
  vocabulary; the discovered set persists across civ collapse
  boundaries.

This is how a species *invents new phenomena* — naming the
threshold pattern they fitted, not just discovering laws over
pre-named ones.

## Different worlds, different sciences

A piezoelectric rocky world, a hydrocarbon-rich one, and a silicate
magma world host structurally different scientific traditions
because crust + atmosphere + biosphere + species modalities shape:

- **What fires** — physical-presence pass drops impossible
  templates.
- **What's perceivable** — sensorium pass gates by species.
- **What gets discovered first** — observation order is set by
  baseline regional templates and which channels figures sample
  most.

The form vocabulary itself is gated by which structural tags the
civ's perceivable templates carry, so civilisations that never
observe a `Periodic` template literally cannot fit sinusoidal
forms. See [discovery.md](discovery.md#form-vocabulary).
