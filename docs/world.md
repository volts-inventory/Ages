# World model

What the sim treats as "the world." For deeper detail per crate:

- Planet sampling, terrain init, climate → [`sim/world/README.md`](../sim/world/README.md).
- Physics laws + grid → [physics.md](physics.md).
- Recognition templates → [recognition.md](recognition.md).

For minds and civs see [civ.md](civ.md).

## What exists in a run

Per run, the sim materialises:

1. **A planet** sampled from the seed (substrate, mass + radius,
   atmosphere, hydrology, biosphere, magnetosphere, crust, terrain
   priors, moons, axial tilt, day length, orbital period, host
   star).
2. **A hex-grid map** (36 × 30 = 1080 cells by default;
   configurable via `--grid-width` / `--grid-height`) carrying
   biome, substance inventory, climate, adjacency, terrain glyph.
3. **A physics state** evolving deterministically on the grid.
4. **A recognition-template library** filtered by physical
   presence and species sensorium.
5. **A species** derived from `{planet, regions, recognized
   phenomena}`. See [species.md](species.md).
6. **One or more civilizations** within the species, founded
   emergently when nomadic density crosses thresholds. See
   [civ.md](civ.md).

The sim **never branches on "is this a Europa-like world"**
directly — every property feeds either the physics engine, the
regional substance inventory, or the recognition templates that
turn physics state into named phenomena.

## Grid is a sampling resolution

A planet has a real physical size set by its sampled `radius` (in
Earth radii) — surface area scales as `4πR²`, so a 1.4-Earth-radius
world has roughly twice the area of Earth. The hex grid samples that
surface at a fixed resolution (default 36 × 30 = 1080 cells) regardless
of how big the planet actually is.

The per-cell terrain GLYPH is therefore a *regional category*, not a
single landform. On the default grid one `▲` cell stands for whatever
peak-class terrain occupies its ~half-million-km² patch — that can be
a lone summit on a fine grid or an entire mountain range on a coarse
one. The renderer reads the same patch regardless of physical size.

Dynamics live on the cell fields and evolve per tick: climate,
hydrology, tectonics, weathering, volcanism, catastrophes. Magnitudes
and per-tick event rates **scale with planet area where physical**:

- Carrying capacity multiplies by `planet_area_factor = radius²`
  (cached on `Civ` at founding); a bigger planet hosts more
  individuals at the same per-cell biome richness.
- Terrain relief and biosphere density scale linearly with `radius`
  so taller mountains and deeper basins emerge as the planet grows.
- Volcanism, tectonic uplift/divergence, and catastrophe cadences
  (asteroid / solar-flare / ice-age / volcanic) multiply per-tick
  rates by `radius²` so regions visibly churn proportionally more on
  larger worlds.
- Worldgen variety scales with area too: extra continents (up to
  ~3 at radius 2.0), island chains (up to ~6), and interior lake
  basins (up to 3 per continent) all open up beyond Earth-radius
  and stay zero at radius = 1.0.

Earth-radius (factor 1.0) is a no-op everywhere: every coefficient
collapses to its legacy value, and Earth-radius seeds reproduce
byte-for-byte across releases.

Cross-link: [`archetype.md`](archetype.md) sketches how an archetype
(field-and-resonance, photonic, gravitational, nuclear) reads the
planet through that area-scaled physics — the archetype names *what*
gets surfaced; the grid-as-sampling layer here governs *how big and
how often*.

## Substrate-first sampling

`sample_planet(seed)` picks a `MetabolicSubstrate` first, then
constrains every other property to that substrate's tolerance.
`Planet::is_habitable()` is true by construction (the substrate
guarantees the atmosphere class is compatible).

| Substrate | Temperature window | Solvent | Atmosphere set |
|-----------|--------------------|---------|----------------|
| Aqueous | 250–400 K | H₂O | non-`None` (any) |
| Ammoniacal | 195–240 K | NH₃ | `Reducing` / `Thin` |
| Hydrocarbon | 90–180 K | CH₄ / C₂H₆ | `Reducing` / `Hazy` / `Thin` |
| Silicate | 800–1500 K | molten rock | any incl. `None` |

`substrate_perturbation ∈ [-0.05, +0.05]` shifts the substrate's
nominal freeze + boil points per seed — Aqueous water on seed 42
might freeze at 273.5 K, on seed 100 at 270.7 K. The substrate
enum stays the same; what varies is the exact phase-transition
temperature within the substrate's tolerance window.
Per-substrate `metabolism()` returns a `(0, 1]` time-scale factor
(Aqueous 1.0, Ammoniacal 0.5, Hydrocarbon 0.4, Silicate 0.2) that
multiplies every per-tick biological / societal rate, so slow
chemistries unfold over proportionally longer arcs.

## Mass / radius / gravity

Sprint 5 Item 21 replaced the pre-sampled `gravity` scalar with
mass + radius in Earth units. Sampling bands per substrate:

| Substrate | mass (Earth) | radius (Earth) |
|-----------|--------------|----------------|
| Aqueous | 0.5–2.0 | 0.8–1.4 |
| Silicate | 0.5–2.5 | 0.7–1.3 |
| Ammoniacal | 0.5–2.0 | 0.9–1.6 |
| Hydrocarbon | 0.3–1.5 | 0.6–1.3 |

Derived accessors on `Planet`:
- `gravity() = EARTH_G × M / R²` (Earth ≈ 9.81 m/s², range ~1.7–50 m/s²).
- `escape_velocity() = sqrt(2gR)` in km/s (Earth ≈ 11.18).
- `density(substrate)`: silicate ≈ 5, aqueous ≈ 1, ammoniacal ≈ 0.7,
  hydrocarbon ≈ 0.5 g/cm³.

Downstream couplings: `Tides::for_gravity` and `Wind::for_gravity`
read the derived `g`; `PlanetEscapeParams` reads escape velocity;
tidal Love numbers consume substrate density;
`Radiation::with_lapse_inputs` reads `g` for the dry adiabatic
lapse rate (cirrus greenhouse).

## Continuous compositions

Categorical enums survive as summary labels; physics + recognition
read continuous mass-fraction vectors
([`sim/world/src/composition.rs`](../sim/world/src/composition.rs)):

- **`AtmosphericComposition`** — N₂ / O₂ / CO₂ / CH₄ / NH₃ / H₂O /
  H₂ / Ar / other. Sampled per `(Atmosphere, MetabolicSubstrate)`
  baseline, perturbed ±10 % per channel, renormalised.
  `Atmosphere::from_composition` recovers the categorical label
  from any mixture.
- **`CrustalComposition`** — silicate / hydrocarbon / piezoelectric
  / ferrous / rare_earth / ice / other.
- **`biosphere_density`** — continuous `[0, 1]` scalar.
- **`Atmosphere::scale_height_m`** + **`Atmosphere::density_x100`**
  per atmosphere variant — drives barometric pressure decay used
  by Hydrology's pressure-aware boil.

## Host star

`Planet::star: Star` carries spectral class + per-band SED + age
([`sim/world/src/star.rs`](../sim/world/src/star.rs)).

| Spectral | Distribution | Mass (M☉) | Lifetime (Gyr) | Lum (L☉) | Flare rate |
|----------|-------------:|-----------|---------------:|---------:|-----------:|
| M | 60 % | 0.08–0.45 | 1000 | 0.04 | 100× |
| K | 20 % | 0.45–0.80 | 25 | 0.4 | 10× |
| G | 12 % | 0.80–1.04 | 10 | 1.0 | 1× |
| F | 5 % | 1.04–1.4 | 5 | 2.5 | 0.3× |
| A | 3 % | 1.4–2.1 | 2 | 12 | 0.1× |

`SedFractions { euv, uv, visible, ir }` per spectral type splits
bolometric output by band. EUV is dominated by chromospheric /
coronal activity and decouples from the bolometric MS drift —
it follows its own `(1 + age / 0.1 Gyr)^(-1.5)` decay
(`euv_decay_factor`), so young G dwarfs emit ~100× modern Sun's
ionising flux and old G dwarfs ≈ 0.001×.

### Stellar evolution

`bolometric_scale_at_age(age, lifetime)` returns the multiplier on
the ZAMS irradiance:

- ZAMS (age = 0): `× 0.70` (faint-young-sun anchor).
- MS drift (age < 0.95 × lifetime): linear from 0.70 → 1.40.
- Red-giant ramp (0.95–1.0 × lifetime): 1.40 → 1000.
- Past MS end: capped at 1000.

At `age_gyr = 4.5, lifetime = 10` (modern Sun analog) the factor
is ~1.03 — within 5 % of present-day output.

Worldgen samples `age_gyr ∈ [0, 0.9 × lifetime)` so most planets
sit mid-MS; tests construct via `Star::with_age` for explicit
post-MS or red-giant scenarios.

### HZ migration

`star.hz_inner_edge_au()` and `hz_outer_edge_au()` scale with
`sqrt(L / 1361 W/m²)` (Kasting moist-greenhouse / maximum-
greenhouse boundaries — 0.95 AU / 1.37 AU for the present Sun).
Both edges drift outward as the star brightens. `habitability::
hz_factor(star, distance_au)` returns 1.0 inside the HZ;
`distance / inner` below; `outer / distance` above.

`cell_habitability = terrain_multiplier × hz_factor` is the per-
cell habitability scalar; the HZ component is per-planet, so it
shifts the whole map's habitability in lockstep as the star ages.
`apply_hz_biosphere_drift` additionally one-shots a biosphere-
class downgrade if the sampled orbital distance lands well outside
the HZ at planet build.

## Tidal-locking regime

`Planet::locking_state: LockingState` ∈ `{Synchronous,
Resonance { p, q }, FreeRotator}`. The sampler in
`sample_locking_state` ([`sampling.rs`](../sim/world/src/sampling.rs))
classifies:

1. **Synchronous** if the first moon is close (`period < 100`
   macros) and massive (`mass_relative > 0.10`).
2. **`Resonance { p, q }`** if `day_length_hours /
   orbital_period_hours` lands within ±5 % of 3/2 or 2/3 (the
   Mercury-style spin-orbit lock).
3. **`Resonance { 3, 2 }`** via a 5 % SplitMix64 variety jitter
   (salted with `LOCKING_SALT` so it doesn't disturb the main
   ChaCha draw sequence).
4. Otherwise **`FreeRotator`** (Earth's regime).

`step_eccentricity_damping` ([`tidal_locking.rs`](../sim/world/src/tidal_locking.rs))
drains `Moon::eccentricity` per tick:

- `Synchronous` damps fast via `synchronous_eccentricity_damping_rate`.
- `FreeRotator` damps slowly (1/10 of synchronous).
- `Resonance { .. }` does *not* damp — Laplace-type gravitational
  forcing from other bodies (Io-Europa-Ganymede) sustains
  non-zero `e`.

`sub_stellar_point(planet, macro_step)` returns the sub-stellar
lat/lon coordinate. `Synchronous` planets pin it at `(0, 0)`;
everything else rotates it with `macro_step`. The sub-stellar
trig feeds `Radiation::with_locking` so a tidally-locked world
gets a great-circle day-night gradient rather than a zonal mean.

`Planet::is_tidally_locked()` returns `day_length_hours >= 1000`
as a coarse predicate for templates / report layers.

## Moons

`Planet::moons: Vec<Moon>` carries per-moon `mass_relative_x100`,
`orbital_period_macros`, `inclination_deg_x10`, `eccentricity`.
Multi-moon tidal superposition produces spring/neap-style
interference in `Tides`; eccentricity drives heat dissipation in
`apply_tidal_heating`.

Per-moon periods sampled from an Earth-system-inspired set: first
moon Earth-Moon-like (28 macros), second Phobos-like (13), third
Io-like (79), fourth ultra-fast (7). `eccentricity ∈ [0, 0.10]`
at sample; Item 19's per-tick damping evolves it.

## Habitability

`cell_habitability(state, planet, cell)`
([`habitability.rs`](../sim/world/src/habitability.rs)) composes
the per-cell terrain glyph with the planet-wide HZ factor:

```text
cell_habitability = habitability_multiplier(terrain_glyph) × hz_factor
```

Per-glyph multipliers:

| Glyph | Meaning | Multiplier |
|-------|---------|-----------:|
| `≈` | deep ocean | 0.00 |
| `≡` | gas-giant cloud band | 0.00 |
| `~` | shallow water | 0.05 |
| `░` | coast | 1.20 |
| `·` | plain / sub-surface ocean / basin floor | 1.00 |
| `▒` | inland land | 0.90 |
| `△` | hill | 0.60 |
| `▲` | peak | 0.10 |

`CLAIM_HABITABILITY_THRESHOLD = 0.05` gates which cells civs can
claim — deep ocean and gas band act as walls; founding centroids
relocate off uninhabitable terrain at all three founding sites
(inaugural emergence, stateless re-founding, breakaway).

## Terrain

Multi-peak via piecewise cones in `init_planet`
([`init.rs`](../sim/world/src/init.rs)):

- 3–5 peaks per planet, primary anchored at
  `terrain_centre_q/r`, secondaries scattered via Poisson-disc
  rejection sampling (min-distance `max(3, max(w, h) / (n × 2))`,
  up to 200 attempts per peak).
- 80 / 20 height split between primary and secondaries.
- Steep summit slopes; shallow ≤ 50 m/cell coastal slopes preserve
  the renderer's `~` shallow-water band.

`terrain_glyph_at` mirrors the renderer's classification so
habitability decisions agree with what the viewport draws.

`terrain_peak` is constrained to land above `sea_level`: rocky
worlds need peak ≥ sea + 1500 m, ocean worlds peak ≥ sea + 500 m.
Without that, low-peak high-sea samples erase all land cells and
the biosphere has nowhere to deposit fuel — `carrying_capacity`
= 0 → instant collapse.

## Climate bands relative to planet

Recognition templates that name climate-relative phenomena
(`fertile_land`, `cold_zone`, `polar_winter`,
`harmonic_resonance`, etc.) read bands derived from the actual
cell-temperature distribution: `(mean ± gradient/4)` quartered
into DeepCold / Cold / ProductiveBand / Hot. A 232 K sub-surface
ocean's "polar winter" fires on its own gradient instead of
silently never-firing because 232 < 240. See
[`climate.rs`](../sim/world/src/climate.rs).

`seasonal_temperature_offset(tick, cell, planet, grid)` is a pure
function of `(tick, cell, planet, grid)` that returns a per-cell
K offset driven by `axial_tilt_deg × month-in-year` triangular
wave, scaled by `temperature_gradient` at the pole.
`orbital_period_months` (sampled in `[8, 16]`) is the per-planet
year length for calendar / seasonal indexing — civ-tick cadence
stays at 1 tick = 1 month.

## Planet-class coverage

11 end-to-end planet scenarios are exercised in test:

- **Earth-analog** — default canary fixture (`earth_like_planet`,
  G-dwarf at 4.5 Gyr, 1 g, 1361 W/m²).
- **Mars-analog** — sampling sweep + atmospheric-escape
  calibration anchors (`mars_like()` params).
- **Venus-equivalent** — Reducing / `surface_pressure ~ 9 MPa`
  runaway-greenhouse worlds via the C-C-coupled vapour feedback;
  plateaus in the 700-770 K literature band.
- **Titan-analog** (`titan_analog_run_produces_credible_state`) —
  Hydrocarbon substrate, Hazy atmosphere, 94 K.
- **Ammoniacal cold reducing**
  (`ammoniacal_analog_run_produces_credible_state`) — NH₃
  substrate at ~220 K.
- **Super-Earth (2 g)**
  (`super_earth_run_with_2g_gravity_does_not_overflow`) — direct
  Q32.32 overflow probe; `tide_k` / `wind_k` measurably differ
  from Earth baseline.
- **Hot Jupiter** (`hot_jupiter_extreme_params_do_not_overflow`)
  — 1500 K surface, 10 W/m² EUV, 60 km/s escape velocity.
- **M-dwarf locked HZ planet**
  (`m_dwarf_hz_locked_planet_runs_cleanly`) — 0.5 M⊕, 0.7 R⊕,
  `LockingState::Synchronous`, M-dwarf host with 100× flare rate.
- **Europa-class icy moon** — Aqueous substrate + icy `k₂/Q` +
  F6 substrate multiplier lands 5-20 TW.
- **Ganymede / Callisto** — Laplace-pumped vs non-resonant icy
  moons; Ganymede 0.5-5 TW, Callisto ≈ 0.
- **Silicate lava world** (`lava_world_runs_with_silicate_substrate`)
  — Silicate substrate, `Atmosphere::None` admitted, 1500 K.

## User overrides (`--config`)

`sample_planet_with_overrides(seed, &PlanetOverrides)` wraps the
substrate-first pipeline with optional per-field replacements
from the `ages --config` interactive prompt. Every field on
`PlanetOverrides` is `Option<T>`; `None` keeps the seed value.

**Map geography is not overridable** — elevation, water depth,
sea level, terrain peak and the per-cell layout always come from
`--seed`. Only planet-level scalars are exposed: substrate,
atmosphere, mean temperature, gravity, spectral type, axial tilt,
day length, year length (orbital months), moon count, magneto-
sphere, crust, biosphere richness.

Coherence: when the user overrides substrate or atmosphere, the
substrate-conditional fields (`atmospheric_composition`,
`crustal_composition`) re-sample from a salted RNG substream
(`OVERRIDE_SALT`) so the planet stays internally consistent
without disturbing the main worldgen draw order. An aqueous→
hydrocarbon substrate flip on seed 42 gets a methane-rich air
mix even though the user only picked the substrate.

Gravity override: setting `gravity_g_x100 = g × 100` sets `mass
= g_relative` and `radius = 1.0`, so the derived `g = M / R²`
matches the requested value exactly (Earth-relative units).

Spectral type override rebuilds `Star::with_age` at the chosen
type while preserving the sampled bolometric irradiance + age,
so HZ calculations stay consistent.

## Run-start order

Deterministic flow at run start:

1. Sample planet from seed (substrate-first via `sample_planet`
   or `sample_planet_with_overrides` when `--config` was used).
2. Build hex-grid map.
3. Initialise physics with planet inputs and per-cell substances
   (`init_planet`).
4. Spin up physics for a deterministic warm-up so atmosphere,
   oceans, biology stocks reach equilibrium. Civ founding waits
   on this so it doesn't start during a transient.
5. Filter recognition templates by physical presence.
6. Derive species from `{planet, regions, recognizable phenomena}`.
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
