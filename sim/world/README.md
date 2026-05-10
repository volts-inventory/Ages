# sim/world

Planet sampling, regions, and the run-start sequence. Every property
either feeds the physics engine, the regional substance inventory, or
the recognition templates that turn physics state into named phenomena.
The sim **never branches on "is this a Europa-like world"** — each
property is a knob.

## Status

- **`Planet` struct + `sample_planet(seed)`** shipped, deterministic.
- **SI units**: gravity in m/s², temperature in K, pressure in
  Pa, terrain / sea-level in m, stellar irradiance in W/m². Field
  doc comments document the unit + Earth reference.
- **Regions** scaffolded as cells of the hex grid; full biome /
  substance-inventory model lands when crate-level work needs it.
- **Crust enum** (`Basaltic | Hydrocarbon | Piezoelectric | Ferrous |
  RareEarth`): drives fuel availability and gates which late-game
  tools (`field_propulsion_engine`, `metamaterial_lattice` in
  `sim_civ::tech`) can be built. Decouples fossil fuels from
  biosphere — Hydrocarbon crust contributes buried fuel; the other
  variants don't, so a Sparse-biosphere planet without Hydrocarbon
  crust has very limited Substance::Fuel and cannot easily develop
  combustion-driven tech (the "no combustion path" worlds the
  vision describes).
- **Habitability bias** (see module-level doc): composition /
  atmosphere / biosphere distributions tuned so ~70-75% of seeds
  produce multi-civ runs; `terrain_peak ≥ sea_level + 1500m` on
  rocky worlds prevents low-peak-high-sea sampling from erasing all
  land.
- **`axial_tilt_deg`** (0–45°) and **`day_length_hours`** (4–200)
  fields on `Planet`. Sampled per seed; surfaced in the
  `PlanetDerived` event and the planet card. Currently flavour-
  only; reserved for a future seasonal/diurnal physics pass
  that would couple them to per-cell temperature swings.

## Planet (sampled fields)

The seed determines a planet at run start. Some properties are direct
physics inputs; others (twilight length, eclipse geometry, climate
regime) are *derived* by running the physics. Listed below as "what
you can pin per-run" — categories are illustrative, not exhaustive.

**Astrophysical / orbital**: stellar type, solar activity cycle,
orbital position, eccentricity, sky companions, comet flux,
supernova history, ring system, moons, magnetosphere, day/year/tilt,
meteor flux.

**Bulk planet**: mass / gravity, composition, surface pressure,
atmosphere class, hydrology, continent fragmentation, tectonic
regime, geological activity, crust mineralogy, climate regime,
cosmic-ray exposure.

**Biosphere**: biosphere class, photosynthesis chemistry, megafauna,
pollinator-dispersal, lignin, fermentable substrates, pathogen
pressure, bioluminescence prevalence, native psychoactives, toxin
profile.

**Resource priors**: combustible fuels, ferromagnetic minerals,
exposed metals, piezoelectric crystals, salt, clay, glass precursors,
fossils, rare-earths, radioactives.

**Existential hazard priors**: asteroid risk, supervolcano risk,
stellar-flare risk.

The fields actually present in the `Planet` struct today are a subset
sufficient for M1a/M1b/M2 physics + recognition. The rest are added
as later milestones need them.

## Regions

The planet is partitioned into regions; each region is one cell (or
patch) in the physics grid. Each region carries biome, substance
inventory, climate tag, adjacency, and global-coordinate position
(used by gravity, illumination, fluid flow). Hex grid resolution
is ~2 500 cells default.

## Species derivation

Species derives **after** physics warm-up + recognition-template
filter. Inputs: planet properties, region map, the
physically-present recognized-phenomenon set. Compatibility is
guaranteed by construction — the species couldn't have evolved here
if it weren't compatible.

Traits include: body plan, manipulation modes, sensorium,
communication channel (multi-modal), respiration, lifespan,
reproductive cadence, sociality, cognition baseline. Modality and
manipulation lists are sampled from compatible options with seeded
variation.

One species per planet for M3 onwards. Multi-species and inter-species
contact deferred. **The species is the run's persistent unit** — it
persists across rise and fall of civilizations on the planet.

(Species traits are documented here because `sim/species/` is still a
stub; once it gets real code, the traits lift to `sim/species/README.md`.)

## Run-start order

Deterministic flow at run start:

1. Sample planet from seed.
2. Build region map (grid cells with biome, substances, climate,
   adjacency, position).
3. Initialise physics with planet inputs and regional substances.
4. Spin up physics for a deterministic warm-up so atmosphere, oceans,
   biology stocks reach equilibrium. Civ founding waits on this so
   it doesn't start during a transient.
5. Filter recognition-template library by physical presence (does
   this planet's physics ever produce the signature?).
6. Derive species from {planet, regions, recognizable phenomena}.
   Includes communication channel and manipulation modes.
7. Filter recognized phenomena by sensorium gates → split into
   `perceivable-now` and `latent`.
8. Initialise civ in a region matching the species' niche.

Each step is pure (function of previous state + seeded RNG); the same
seed reproduces the same world bit-for-bit.

## Cited by

[docs/world.md](../../docs/world.md),
[docs/species.md](../../docs/species.md),
[docs/physics.md](../../docs/physics.md).
