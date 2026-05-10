# sim/recognition

Pattern recognition: physics state → named phenomena. Templates define
*what counts* as a recognizable phenomenon; the physics produces
whatever it produces, and templates fire when the physics happens to
match.

There is **no authored phenomenon catalogue**. Tides happen because
water moves under gravity; lightning happens because charge accumulates
and discharges; fire happens because oxidiser + fuel exceeds activation
energy. The recognition layer translates that emergent behaviour into
discrete events the civ-layer can hypothesise about.

## Status

- **M2 shipped**: template-driven `Signature` matching over physics
  fields. Five initial templates: `fire`, `lightning_buildup`,
  `ice_present`, `vapour_present`, `surface_water`.
- **Vocabulary expansion**: 14 templates now ship in
  `earth_like_default()`. Beyond the M2 baseline, latent / advanced
  templates: `magnetic_field_strong`, `thermal_gradient` (M3
  sensorium-tech latents); `flood_zone`, `cold_zone`, `fertile_land`
  (broader habitat phenomena); `auroral_activity`,
  `harmonic_resonance`, `static_field_gradient` (field-and-resonance
  archetype phenomena: charge auroras, atmospheric oscillation
  modes, EM background); `tidal_extremum` (deep-water belts,
  approximation pending lunar-gravity SWE physics).
- **SI landed**: thresholds in SI units — `fire` at 500 K
  (lowest Earth ignition), `surface_water` at >1 m. Charge stays
  in arbitrary sim-units pending Coulomb-law structuring.

## Template anatomy

Each template specifies:

- **id**, **name** (technical; per-civ language layered on top).
- **signature** — pattern looked for in physics state. Variants:
  `Above(field, threshold)`, `Below(field, threshold)`,
  `AbsAbove(field, threshold)`, `All([signatures])`.
- **sensorium gates** — which species sensoria can perceive it,
  with what fidelity, plus instrument-tier extensions (M3 wiring).
- **observation depth tiers** — notice → characterise → quantify,
  each with data requirements (e.g. quantify needs N samples
  spanning K parameter values).

Templates are authored once in a small library; the actual phenomena
recognized in a given run depend entirely on what the physics
produces.

## Two-pass filtering at run start

1. **Physical-presence pass** — for each template, check whether any
   region's physics state could plausibly contain the signature
   (e.g. `tides` needs a fluid layer + significant moon-gravity
   gradient; absent on a moonless dry planet).
2. **Sensorium pass** — split recognized templates into:
   - `perceivable-now`: detectable by the species' default sensorium
     in the founding region.
   - `latent`: physics produces it, but the species can't sense it
     without sensorium-extending tech.

Latent templates dormant until the civ invents the relevant
instrument (telescope, microscope, magnetometer-equivalent), at
which point they promote to perceivable-now and the recognition layer
begins firing them.

## What this gives us

- A planet that lacks `stars` (sub-surface ocean) never has the
  recognition template for "stars" fire.
- A planet with `lodestone-attraction` and `bio-electricity` but no
  `fire` reaches electromagnetism long before metallurgy because
  those signatures fire while `fire` never does.
- Paradigms (combustion / biology / field) emerge from which
  templates fire on which planet, in which order, observed by which
  species.

## Authoring guidance

Recognition library grows continuously. The current 13 templates
mix Earth-familiar (fire, lightning, ice, vapour, surface water) /
Earth-rare (auroras, harmonic resonance, static field gradient,
fertile land) / sensorium-latent (thermal gradient, magnetic field).
Future additions land as the discovery pipeline + new sensoria
demand them. Plausible exotics: cryogenic chemistry, sub-surface
acoustic propagation, plasma weather, methane-organic chemistry.

Each template's *signature* is the real authoring work — defining
what to look for in physics state. Library will live in
`data/recognition/` (schema-validated YAML); for M2 it's hardcoded in
`RecognitionLibrary::earth_like_default()`.

## Cited by

[docs/recognition.md](../../docs/recognition.md).
