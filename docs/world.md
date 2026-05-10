# World model

What the sim treats as "the world." For deeper detail per crate:

- Planet sampling, terrain init, climate → [`sim/world/README.md`](../sim/world/README.md).
- Recognition templates → [recognition.md](recognition.md).
- Physics laws + grid → [physics.md](physics.md).

For minds and civs see [civ.md](civ.md).

## What exists in a run

Per run, the sim materialises:

1. **A planet** sampled from the seed (substrate, gravity,
   atmosphere, hydrology, biosphere, magnetosphere, terrain priors,
   moons, axial tilt, day length, orbital period).
2. **A hex-grid map** (36×30 = 1080 cells by default;
   configurable via `--grid-width` / `--grid-height`)
   carrying biome, substance inventory, climate, adjacency.
3. **A physics state** evolving deterministically on the grid.
4. **A recognition-template library** filtered by physical presence
   and species sensorium.
5. **A species** derived from {planet, regions, recognized
   phenomena}. See [species.md](species.md).
6. **One or more civilizations** within the species, founded
   emergently when nomadic density crosses thresholds. See
   [civ.md](civ.md).

The sim **never branches on "is this a Europa-like world"**
directly — every property feeds either the physics engine, the
regional substance inventory, or the recognition templates that turn
physics state into named phenomena.

## Substrate-first sampling

Every seed produces life of *some* chemistry. The sampler picks the
metabolic substrate first, then constrains every other planet
property to that substrate's tolerance. `Planet::is_habitable()` is
true by construction.

| Substrate | Temperature window | Solvent | Notes |
|-----------|--------------------|---------|-------|
| Aqueous | 240–340 K | H₂O | Earth-like; broadest sampling weight. |
| Ammoniacal | 195–240 K | NH₃ | Cold worlds; ammonia-based metabolism. |
| Hydrocarbon | 90–180 K | CH₄ / C₂H₆ | Titan-style cryogenic. |
| Silicate | 800–1500 K | molten rock | Magma-world life. |

Each seed shifts the substrate's freeze/boil baseline by ±5%, so
two aqueous worlds can have different exact phase points.

## Continuous compositions

Categorical enums survive only as summary labels; the actual physics
+ recognition reads continuous vectors:

- **`AtmosphericComposition`** — 9-channel mass-fraction vector
  (N₂ / O₂ / CO₂ / CH₄ / NH₃ / H₂O / H₂ / Ar / other) sampled per
  planet from substrate-aware baselines, perturbed and renormalised.
- **`CrustalComposition`** — 7-channel mineral vector (Silicate /
  Carbonate / Iron / Sulfide / Halide / Oxide / Other).
- **`biosphere_density`** — continuous `[0, 1]` scalar.
- **`Atmosphere::scale_height_m`** + **`Atmosphere::density_x100`**
  per atmosphere variant — drives barometric pressure decay used by
  Hydrology's pressure-aware boil.

## Terrain

Multi-peak terrain via piecewise cones:

- 3–5 peaks per planet, primary anchored at
  `terrain_centre_q/r`, secondaries scattered via Poisson-disc
  rejection sampling (min-distance `max(3, max(w, h) / (n × 2))`,
  up to 200 attempts per peak).
- 80/20 height split between primary and secondaries.
- Steep summit slopes; shallow ≤ 50 m/cell coastal slopes preserve
  the renderer's `~` shallow-water band.

The world painter (`init.rs`) labels each cell with a glyph based
on `(elevation, water_depth)`:

| Glyph | Meaning | Habitability multiplier |
|-------|---------|-------------------------|
| `≈` | deep ocean | 0.00 |
| `≡` | gas-giant cloud band | 0.00 |
| `~` | shallow water | 0.05 |
| `░` | coast | 1.20 |
| `·` | low-relief / sub-surface ocean / oceanic basin floor | 1.00 |
| `▒` | land | 0.90 |
| `△` | hill | 0.60 |
| `▲` | peak | 0.10 |

Habitability multiplies per-cell carrying capacity. Civs cannot
claim cells below the threshold (currently 0.05) — deep ocean and
gas band act as walls; founding centroids relocate off
uninhabitable terrain at all three founding sites (inaugural
emergence, stateless re-founding, breakaway). See
[`sim/world/habitability.rs`](../sim/world/src/habitability.rs).

## Climate bands relative to planet

Recognition templates that name climate-relative phenomena
(`fertile_land`, `cold_zone`, `polar_winter`, `harmonic_resonance`,
etc.) read bands derived from the actual cell-temperature
distribution: `(mean ± gradient/4)` quartered into DeepCold / Cold
/ ProductiveBand / Hot. A 232 K sub-surface ocean's "polar winter"
fires on its own gradient instead of silently never-firing because
232 < 240. See
[`sim/world/src/climate.rs`](../sim/world/src/climate.rs).

## Tidally-locked planets

~5% of seeds sample into `is_tidally_locked`. Their
`tidally_locked_terminator` template fires across the day-night
boundary band. Diurnal cycling reads `day_length_hours`, so
tidally-locked planets get permanent day/night asymmetry while
Earth-like worlds average out.

## Moons

`Planet::moons` is a `Vec<Moon>` carrying mass and orbital period.
Multi-moon tidal superposition produces spring/neap-style
interference in the Tides law.

## Run-start order

Deterministic flow at run start:

1. Sample planet from seed (substrate-first).
2. Build hex-grid map.
3. Initialise physics with planet inputs and per-cell substances.
4. Spin up physics for a deterministic warm-up so atmosphere,
   oceans, biology stocks reach equilibrium. Civ founding waits on
   this so it doesn't start during a transient.
5. Filter recognition templates by physical presence.
6. Derive species from {planet, regions, recognizable phenomena}.
7. Filter recognized phenomena by sensorium gates → split into
   `perceivable-now` (default sensorium can detect) and `latent`
   (instrument-gated).
8. Seed nomadic species pool across habitable cells. Civs emerge
   later when density crosses thresholds.

Each step is pure (function of previous state + seeded RNG); the
same seed reproduces the same world bit-for-bit.

## One species per planet

One species per planet for the current build. Multi-species and
inter-species contact is deferred. The species is the run's
persistent unit; civs are bounded episodes within it (see
[species.md](species.md) and [civ.md](civ.md)).
