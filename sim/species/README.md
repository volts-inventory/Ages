# sim/species

Species traits, derivation, and sensorium gating. The species is the
persistent unit of a run; civilizations rise and fall within it. The
species is *derived* from the planet (not sampled independently) so
its sensorium and manipulation modes reflect the niche the planet
provides.

## Status

- **M3 shipped here**: `Species` struct + `derive(planet, recognition_lib)`
  with environment-gated modality vector, biosphere-tuned channel
  count, manipulation modes filtered by composition, deterministic
  Q-fixed-point trait scalars (cognition, sociality, lifespan,
  communication fidelity, t0_loss). Sensorium gating exposed as
  `Species::can_perceive(template_id)` and
  `Species::perceivable_firings(&firings)`; the run loop in
  `sim/core` filters cohort observations through this gate so a
  species sees only firings whose channels intersect its modality
  set.
- **M3 wired**: a `SpeciesDerived` protocol event lands at run start
  carrying the trait scalars (Q32.32 raw bits for bit-exact
  determinism), the modality and manipulation kinds, and the
  perceivable-template id list.
- **`cognition_topology` field** (`Centralized | Distributed`):
  cephalopod-vs-vertebrate substrate distinction. Sampled 70/30
  toward Centralized; Distributed surfaces the cephalopod
  archetype (the "field-and-resonance" species the project's
  vision archetype describes). Currently flavour-only — surfaced
  in `SpeciesDerived` and the post-run report's species card.
  Behavioural fork (cosmology drift speed, refinement
  aggressiveness) reserved for a later pass.

## Modalities (15 channels)

`acoustic_air`, `acoustic_water`, `seismic`, `visual_light`,
`visual_polarization`, `bioluminescent`, `chemical_pheromone`,
`chemical_taste`, `tactile`, `electric_field`, `magnetic_sense`,
`infrared_thermal`, `radio_native`, `gestural`, `postural`. Each
channel carries `(range_m, fidelity, bandwidth)`. Environment gates
filter the channel pool before sampling: sub-surface ocean cuts
visual_light and gestural; no-atmosphere planets cut acoustic_air
and chemical_pheromone; no-magnetosphere cuts radio_native and
magnetic_sense; etc. Biosphere class sets target channel count
(HyperBiodiverse 5–7, Lush 3–5, Sparse 2–3, None 1).

## Manipulation modes (12 modes)

`limb_grasp`, `tentacle`, `mouth_beak`, `tongue_prehensile`,
`trunk`, `mandible`, `fluid_jet`, `tool_extension`, `web_construct`,
`burrow`, `electric_discharge`, `chemical_secretion`. Composition
gates the candidate pool (rocky vs ocean vs gaseous body plans);
biosphere class sets count.

## Sensorium gating

`template_channels(template_id)` maps each recognition template to
the modality channels that natively sense it. A species perceives a
template iff the intersection with its own modality set is
non-empty. Latent templates (no native channel) are unobservable
until sensorium-extending tech lands.

## Determinism

`derive(planet, lib)` is a pure function of `planet.seed` plus the
library's template ids; same seed → identical species. Internal RNG
streams XOR a constant into the planet seed so species sampling
can't entangle with mid-run physics RNG draws.

## Cited by

[docs/species.md](../../docs/species.md),
[docs/tech.md](../../docs/tech.md) (sensorium-tech unlock table).
