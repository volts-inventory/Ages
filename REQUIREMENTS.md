# Ages — Requirements Specification

A consolidated, implementation-independent specification of *intended*
functionality, written so the project can be rebuilt cleanly in a fresh
repository. This document describes **what the system should do**, distilled
from the prior prototype's design docs. Where the prototype's behavior is
known to diverge from intent, the intended end-state is specified here and the
gap is flagged explicitly (see §19).

Requirements use "shall" for hard requirements. Quantitative values (grid
sizes, thresholds, ranges) are the design targets; they are tunable but are
recorded here because the prototype's emergent behavior was calibrated around
them.

---

## 1. Vision

Ages is a deterministic, headless simulator that writes **the biography of an
alien species across thousands of years on a procedurally generated planet**.

Given a seed, it:

1. Samples a complete, internally consistent planet (substrate, atmosphere,
   terrain, magnetism, star, orbit, moons).
2. Steps real physics on a hex grid (heat, fluids, hydrology, magnetism,
   tides, radiation, tectonics, atmospheric escape).
3. Evolves a single species fitted to that planet's niche.
4. Lets civilizations rise, expand, contact, trade, war, discover the physics
   of their world, collapse, and seed successors — all within that one
   species, across thousands of years.
5. Emits every structural transition as a structured event stream, then
   renders a written history.

**No LLM. No API keys. No network. Same seed always produces the same
history, byte-for-byte.**

The target audience is people who like emergent worlds, alternative-physics
toys, replayable seeds, and reading a story a computer made on its own.

---

## 2. Guiding principles

Every feature shall trace back to at least one:

1. **Physical / biological grounding over game-mechanic abstraction.** Civs
   derive math from real simulated physics; species evolve to fit niches; no
   fudge constants where derivation is possible.
2. **Emergence over authoring.** Phenomena emerge from law combinations on
   planet-specific conditions. Paradigms emerge from physics + observation
   order. Civs emerge from population + triggers. Developmental paths emerge
   from world + species + trajectory.
3. **The species is the protagonist; civs are episodes within its history.**
   Sumer → Akkad → Babylon-style arcs are first-class.
4. **Quantitative depth, not tokens.** Discoveries are fitted functional forms
   with real parameters. Wrong hypotheses are first-class. Refinement is
   open-ended.
5. **No hand-holding output, no LLM.** Outputs are structured: NDJSON +
   snapshots + live CLI + markdown report + optional single-sentence
   narration.
6. **Determinism as a contract.** Same `(seed, grid)` pair = byte-for-byte
   identical NDJSON. Physics, fits, and RNG all thread through fixed-point
   arithmetic.

---

## 3. System architecture

### 3.1 Process model

The simulator is a single headless process emitting a structured event stream.
There is **no live GUI** and **no LLM** anywhere. Consumers (live CLI viewport,
post-run markdown report, prose narrator) are pure functions of the event
stream and never feed back into simulation.

```
headless sim ──┬──> NDJSON event log (canonical record, always written)
               ├──> live stdout stream (filtered/formatted per CLI mode)
               └──> live narration stream (optional, single-sentence prose)
                         │
   NDJSON log ───────────┼──> post-run markdown report generator
                         └──> offline narration replay
```

### 3.2 Crate layout (suggested workspace)

The prototype used a Rust workspace; a rebuild may differ but should preserve
the acyclic dependency layering. Domain crates:

| Crate | Responsibility |
|-------|----------------|
| `arith` | Fixed-point `Real` (Q32.32) + `Pop` (Q64.32) types and transcendentals. The **only** path for real arithmetic. |
| `world` | Planet sampling (substrate-first), terrain init, climate, habitability, tidal locking. |
| `physics` | Heat / fluid / hydrology / magnetism / Lorentz / Coriolis / tides / radiation / convection / chemistry / tectonics / tidal heating / atmospheric escape on a hex grid. Operator-splitting orchestrator. |
| `recognition` | Phenomenon templates + signature matching against physics state. |
| `species` | Species derivation; sensorium gating; cognition topology. |
| `ecosystem` | Functional-group ecosystem (trophic web + biogeochem + extinction + centrality), HGT, speciation. |
| `population` | Cohort dynamics, substrate-derived demographics, lifecycle, migration, nomads. |
| `civ` | Civ lifecycle, tech, conflict, hypothesizer, apparatus, religion, cosmology, catastrophes, archetype. |
| `events` | Emitter trait + NDJSON / tee / filter / throttle adapters. |
| `report` | Post-run markdown report; live ASCII viewport; narration. |
| `core` | Tick loop, phase walking, run orchestration, law assembly, nomads. |
| `protocol` | Wire schema (the sim↔consumer contract). |
| `ages` | Run binary + report binary. |

Dependency edges shall be acyclic; `arith` is a leaf, `protocol` depends only
on `arith`, `events` depends only on `protocol`, the report crate is a pure
consumer of the event stream.

### 3.3 Determinism contract (cross-cutting, mandatory)

1. The same `(seed, grid_width, grid_height)` tuple shall produce a
   byte-for-byte identical NDJSON log, regardless of CLI mode, tick rate, or
   render settings. `(seed, w, h)` — not seed alone — is the determinism key.
2. All physics and fitting arithmetic shall use fixed-point `Real` (Q32.32,
   ±~2.1e9 range, ~2.3e-10 LSB). A `Pop` type (Q64.32) carries large
   population counts. **No `f64` outside the arithmetic crate.** Display-side
   consumers may lossy-convert raw bits to `f64` for rendering only.
3. Transcendentals (`sin`, `cos`, `exp`, `ln`, `sqrt`, `pow`) shall use
   table/Taylor implementations that are byte-stable across platforms.
4. Randomness shall thread a single seeded `ChaCha20Rng` for the main draw
   sequence. Side-streams (per-field jitter that must not perturb the main
   sequence) shall use salted SplitMix64 hashes of stable ids/seeds. No
   `thread_rng()`, no system time inside the sim loop.
5. No `HashMap` iteration in decision paths — use `BTreeMap` or sorted
   iteration. Canonical cell order is fixed.
6. Physics integration shall be single-threaded unless a parallel-
   deterministic reduction strategy exists.
7. CI shall enforce: a **determinism test** (same seed twice, logs compared
   byte-for-byte), a **performance regression budget**, and a **divergence
   test** (distinct seeds produce distinct phenomenon sets and discovery
   profiles).

### 3.4 Tick semantics

- **One civ-sim tick = one sim-month.** `BASELINE_MONTHS_PER_YEAR = 12` pins
  all biological/societal rate derivation.
- Year length is per-planet (`orbital_period_months` sampled in `[8, 16]`);
  it drives the seasonal-template modulo and Y/M display only.
- `--years N` is a convenience that multiplies by `orbital_period_months`;
  `--ticks N` is the raw override.
- Within each tick the physics engine takes many smaller operator-split
  sub-steps (default ~30 macro-steps per tick, one per sim-day).

### 3.5 Per-tick phase order

Events within a tick are ordered by phase, then by sorted entity ids within a
phase. The iteration order *is* the emission order — no write-time sort. Fixed
phase order:

1. **TickStart** — epoch marker.
2. **PhysicsIntegration** — operator-split sub-steps; aggregated (not per-cell)
   physics events. Apparatus cells write clamp values into physics state at the
   start of this phase, before the prev-state snapshot.
3. **PatternRecognition** — recognized-phenomenon updates from physics state.
4. **CohortObservations** — discovery sampling phase A.
5. **FigureObservations** — discovery sampling phase B; apparatus
   `(clamp, response)` pairs fed to the hypothesizer measurement track.
6. **HypothesisTesting** — fit attempts, confirmation, refinement, falsification.
7. **Discovery** — discovery event emission.
8. **CapabilityEvaluation** — tech-ladder advance, latent → perceivable.
9. **PopulationDynamics** — births, deaths, migrations.
10. **CivLifecycle** — founding, collapse, succession, breakaway, contact,
    trade, inter-civ knowledge transmission, catastrophes.

Within each phase: iterate active civs by sorted civ id, then regions by
region id, then figures by figure id, then phenomenon ids by id. Collapsed
civs are read-only (not iterated in active phases) but preserved for the
report.

### 3.6 Run-end taxonomy

The run ends with exactly one reason:

- `species_extinction` — total cohort population below the founding floor with
  no active civ.
- `stagnation` — extended dark age (no active civ for ~1000 baseline-years ×
  12 ticks).
- `transcendence` — at least one civ sustained all three tier-5 "transcendence"
  tools for ~2000 baseline-years × 12 ticks, gated by tier-5 species maturity
  (3000 cumulative confirmed relations).
- `fixed_horizon` — `max_ticks` reached without any of the above.
- `user_stop` — viewport quit (`q`).

Per-civ collapse reasons are a separate enum and do not end the run.

### 3.7 Run-start sequence (fixed, deterministic)

Sample planet → build hex grid → init physics with planet inputs + per-cell
substances → deterministic physics warm-up to equilibrium (civ founding waits
on this) → filter recognition templates by physical presence → derive species
→ split phenomena into perceivable-now vs latent by sensorium → seed nomadic
species pool across habitable cells → emit `ArchetypeDerived`.

---

## 4. CLI and output surface

### 4.1 Binaries

- **`ages`** — the simulator. `cargo run --release -- --seed 42 --years 100`.
  Defaults: write NDJSON to `runs/run.ndjson`, stream every event to stdout
  (`--cli all`).
- **`ages-report`** — render an NDJSON log to markdown.
  `ages-report --in events.ndjson --out report.md`; shall also support
  stdin/stdout piping.

### 4.2 `ages` flags

| Flag | Meaning |
|------|---------|
| `--seed <n>` | World seed; sole determinant of map geography. |
| `--years <n>` | Run horizon in years (× `orbital_period_months`). |
| `--ticks <n>` | Run horizon in ticks (1 tick = 1 month). |
| `--config` | Interactive planet builder (12 prompts; see §4.4). |
| `--cli <mode>` | stdout verbosity: `quiet` / `all` (default) / `highlights` / `viewport` / `viewport-density`. |
| `--narration` | Live single-sentence prose to stdout; owns stdout (mutually exclusive with `--cli`). NDJSON file still gets the full stream. |
| `--replay-narration <log>` | Read a saved NDJSON log and narrate; skips the sim entirely. |
| `--tick-rate-ms <ms>` | Real-time throttle on the streaming sink only; never throttles the file write. |
| `--frame-every-ticks <n>` | Viewport: paint one frame per n ticks (default 50). |
| `--grid-width <w>` / `--grid-height <h>` | Override default grid. Changes the seed-to-world mapping. |
| `--out <path>` | NDJSON output path. |

The NDJSON file is written at full speed regardless of mode; same `(seed, grid)`
produces byte-identical NDJSON regardless of `--cli` mode or tick rate.

### 4.3 Scripts

- **`run.sh`** — one-shot launcher: generate a random seed, build release,
  open the viewport for a 5000-year run, archive NDJSON to
  `runs/{date}-{seed}.ndjson`. `./run.sh <seed>` re-runs a specific world.
- **`narrate.py`** — standalone pure-stdlib Python 3 prose narrator over a
  saved NDJSON log, no dependencies, slightly richer prose templating than the
  in-binary narrator.

### 4.4 `--config` interactive planet builder

Prompts 12 attributes in order; option `0`/empty Enter keeps the seed default.
**Map geography always comes from `--seed`** — prompts override planet-level
scalars only. Substrate/atmosphere overrides auto-resample atmospheric and
crustal compositions for internal consistency. Conflicting choices surface a
non-blocking warning and proceed.

1. Substrate (aqueous / ammoniacal / hydrocarbon / silicate)
2. Atmosphere (none / thin / oxidising / reducing / hazy)
3. Mean surface temperature
4. Surface gravity
5. Stellar host (M / K / G / F / A)
6. Axial tilt
7. Day length
8. Year length (8–16 months)
9. Moon count (0–4)
10. Magnetosphere (none / weak / strong)
11. Crust mineral (basaltic / hydrocarbon / piezoelectric / ferrous / rare-earth)
12. Biosphere richness (sparse / lush / hyperbiodiverse)

---

## 5. World layer

### 5.1 Purpose

Sample a complete, internally consistent planet from a seed (substrate-first)
and materialize a hex-grid map plus the planet-level scalars feeding physics,
the per-cell substance inventory, and recognition. The layer never branches on
a planet "type"; every property feeds physics, the substance inventory, or
recognition.

### 5.2 Functional requirements

1. `sample_planet(seed)` shall pick a `MetabolicSubstrate` first, then constrain
   every other property to that substrate's tolerance, so `is_habitable()` is
   true by construction. **Every seed produces life of some chemistry.**
2. Build a hex grid of default **36 × 30 = 1080 cells**, configurable.
3. The grid is a sampling resolution over a planet of physical area `4πR²`;
   each terrain glyph is a regional category, not a single landform.
4. Magnitudes and event rates shall scale with planet area where physical:
   carrying capacity × `radius²` (cached on the civ at founding); terrain
   relief and biosphere density × `radius`; volcanism, tectonic rates, and
   catastrophe cadences × `radius²`; worldgen variety scales with area (up to
   ~3 continents, ~6 island chains, 3 lake basins/continent at radius 2.0; all
   zero at radius 1.0, so Earth-radius is a byte-for-byte no-op).
5. `substrate_perturbation ∈ [-0.05, +0.05]` shifts the substrate's nominal
   freeze/boil points per seed without changing the substrate enum.
6. Per-substrate `metabolism()` returns a `(0,1]` time-scale factor (Aqueous
   1.0, Ammoniacal 0.5, Hydrocarbon 0.4, Silicate 0.2) that multiplies every
   per-tick biological/societal rate.
7. Sample mass + radius (Earth units) per substrate; derive
   `gravity() = EARTH_G × M/R²`, `escape_velocity() = sqrt(2gR)`, and bulk
   `density`.
8. Sample a host `Star` (spectral class, per-band SED, age, lifetime).
   Stellar evolution scales irradiance by age; habitable-zone edges migrate
   outward as the star brightens.
9. Classify a tidal-locking regime (`Synchronous` / `Resonance{p,q}` /
   `FreeRotator`) and evolve moon eccentricity per tick by regime.
10. Per-cell habitability = `habitability_multiplier(glyph) × hz_factor`;
    civ claims gated at `CLAIM_HABITABILITY_THRESHOLD = 0.05`.
11. Generate multi-peak terrain (3–5 peaks, piecewise cones + Poisson-disc
    rejection sampling) with `terrain_peak` above sea level so land always
    exists.
12. Derive climate bands from the actual cell-temperature distribution
    (`mean ± gradient/4`, quartered) so band-relative phenomena fire on each
    world's own gradient, not absolute thresholds.
13. `seasonal_temperature_offset(tick, cell, planet, grid)` is a pure function:
    a per-cell K offset driven by `axial_tilt × month-in-year` triangular wave
    scaled by polar gradient.
14. `sample_planet_with_overrides(seed, overrides)` allows optional per-field
    replacement of planet-level scalars only; map geography is never
    overridable. Substrate/atmosphere overrides re-sample conditional fields
    from a salted substream.
15. Exactly **one species per planet**; multi-species and inter-species/inter-
    planet contact are out of scope.

### 5.3 Data model

- **Planet**: `substrate`, mass + radius (Earth units), atmosphere, hydrology,
  biosphere, magnetosphere, crust, terrain priors, `moons`, `axial_tilt_deg`,
  `day_length_hours`, `orbital_period_months`, `star`, `locking_state`.
  Derived: `gravity()`, `escape_velocity()`, `density()`, `is_habitable()`,
  `is_tidally_locked()` (= `day_length_hours >= 1000`).
- **AtmosphericComposition**: continuous mass-fraction vector of **9 channels**
  — N₂ / O₂ / CO₂ / CH₄ / NH₃ / H₂O / H₂ / Ar / other; sampled per
  `(Atmosphere, Substrate)` baseline, perturbed ±10%, renormalised.
- **CrustalComposition**: continuous fractions — silicate / hydrocarbon /
  piezoelectric / ferrous / rare_earth / ice / other.
- **Star**: spectral class, SED fractions `{euv, uv, visible, ir}`, mass,
  lifetime, luminosity, flare rate, age.
- **LockingState**: `Synchronous | Resonance{p,q} | FreeRotator`.
- **Moon**: relative mass, orbital period, inclination, `eccentricity ∈ [0,0.10]`.

### 5.4 Key parameters

- **Substrate windows** (temperature / solvent / atmosphere): Aqueous
  250–400 K / H₂O / any non-`None`; Ammoniacal 195–240 K / NH₃ / Reducing|Thin;
  Hydrocarbon 90–180 K / CH₄·C₂H₆ / Reducing|Hazy|Thin; Silicate 800–1500 K /
  molten rock / any incl. `None`.
- **Stellar distribution / lifetime(Gyr) / lum(L☉) / flare**: M 60% / 1000 /
  0.04 / 100×; K 20% / 25 / 0.4 / 10×; G 12% / 10 / 1.0 / 1×; F 5% / 5 / 2.5 /
  0.3×; A 3% / 2 / 12 / 0.1×. Worldgen samples `age ∈ [0, 0.9×lifetime)`.
- **Habitability multipliers by glyph**: deep ocean `≈` 0.00; gas-giant band
  `≡` 0.00; shallow water `~` 0.05; coast `░` 1.20; plain `·` 1.00; inland
  land `▒` 0.90; hill `△` 0.60; peak `▲` 0.10.
- **orbital_period_months ∈ [8, 16]**.
- 11 planet-class scenarios are first-class validation targets: Earth-analog,
  Mars, Venus, Titan, Ammoniacal cold-reducing, super-Earth (2 g), hot Jupiter,
  M-dwarf locked HZ, Europa, Ganymede/Callisto (tidal heating), Silicate lava
  world.

---

## 6. Physics layer

### 6.1 Purpose

Evolve deterministic, substrate-relative, SI-unit (Q32.32) state on the hex
grid via operator-split time-stepping, producing per-cell fields that
recognition reads and conservation invariants hold against.

### 6.2 Functional requirements

1. Maintain per-cell `PhysicsState`; integrate via `Law::integrate(state, dt)`
   passes in a fixed operator-split order. Each law reads the prior law's
   `next` state.
2. Every spatial-transfer law expresses inter-cell movement as a signed pair
   flux that conservation-checks to zero across both endpoints.
3. `build_laws(&planet)` returns the full law roster derived from the planet
   (substrate, atmosphere-derived ignition threshold, gravity, etc.).
4. Per tick, walk `macro_steps_per_step` macro-steps (default 30, one per
   sim-day), applying laws in the fixed coupling order below.
5. Physics constants derive from the planet's substrate, not Earth defaults
   (phase boundaries, latent heats, gas constants, `c_p`, cell thermal mass,
   Arrhenius prefactor, bulk density).
6. The `Water`/`Ice`/`Vapour` triple is substrate-relative (the solvent's
   liquid/solid/gas; water ~273 K, methane ~91 K, silicate ~1687 K).
7. **Radiation** applies per-cell radiative balance plus per-substance
   greenhouse; H₂O is Clausius-Clapeyron-coupled (quartic saturation cap,
   pressure-scaled) so runaway feedback can diverge (Venus plateau 700–770 K);
   CO₂ linear; CH₄ decays ~0.999/tick (photolysis).
8. **Hadley** derives zonal cell count from a Rhines-length / Held-Hou closure
   (1 cell slow rotators, 3 Earth-like, capped for rapid rotators), conserving
   angular momentum on poleward parcels.
9. **Coriolis** applies the full `F = -2 Ω × v` cross-product; both Coriolis
   and **Lorentz** use a Boris pusher conserving `|v|` regardless of step size.
10. **Tidal heating** evaluates `H = (21/2)(k₂/Q) R⁵ n⁵ e² / G` in Q32.32-safe
    natural units with per-substrate and Laplace-resonance multipliers;
    subsurface conduction routes the substrate-specified heat fraction into a
    subsurface reservoir. Calibration anchors: Io 50–200 TW, Europa 5–20 TW,
    Ganymede 0.5–5 TW, Callisto ≈ 0.
11. **Atmospheric escape** applies four channels per tick per substance (Jeans,
    hydrodynamic blow-off, photochemical, ion), mass-explicit and shielding-
    aware, light-first.
12. **Magnetism** writes a latitude-dependent dipole plus per-cell remanence;
    **MagneticReversal** runs a `Normal ↔ Reversing ↔ Reversed` Markov chain
    (trial ≈ `1/(250000 yr × 12)`/tick, ~12000-tick reversing window, dipole
    decaying to 0.1 at midpoint), with `cosmic_ray_ground_flux ≈ 1/dipole`
    peaking ~5× during reversal.
13. **IceAlbedo** writes snow / sea-ice / cloud fractions; effective albedo is
    the max across channels; the freeze-line is a 5 K sigmoid centred on the
    substrate freeze point.
14. **Tectonics** sequences per tick: slab-pull velocity → boundary
    uplift/divergence → fluvial erosion → crust age + ridge cooling →
    substrate-aware subduction → isostasy. **Volcanism** + **Weathering**
    (Arrhenius CO₂ drawdown) close the carbon-silicate thermostat.
15. **Clouds** author cloud fraction + type (Cirrus / Stratus), with type-
    dependent albedo and greenhouse.
16. Wind / Hydrology / Coriolis / Hadley short-circuit on `Atmosphere::None`
    worlds (vacuum guard); Lorentz stays active.
17. Four **diagnostic lever-substrate fields** — `ResonanceField` (Ψ),
    `SolarInsolation`, `TidalStress`, `SurfaceRadiation` — are additive,
    rewritten each tick, and read by no legacy law, so installing them leaves
    all existing channels bit-identical (see §17).
18. The orchestrator snapshots conservation totals before each kernel and
    asserts per-tick + cumulative drift bounds in debug builds (zero release
    cost).

### 6.3 Data model

- **PhysicsState** (per cell): surface + subsurface temperature, pressure,
  charge, water depth, wind velocity `(v_q, v_r, v_w)`, magnetic field
  `(B_q, B_r, B_z)`, upper temperature, snow / sea-ice / cloud fractions,
  cloud type, remanence, local magnetic shielding, plate id, crust thickness,
  crust age, subducted mass, base elevation, plus the 9-channel substance
  inventory.
- **Substance channels** (fixed append-only index order): 0 `Water`, 1 `Ice`,
  2 `Vapour` (C-C-coupled greenhouse), 3 `Fuel` (renewable; capacity proxy),
  4 `Oxidiser`, 5 `Ash`, 6 `Fossil` (non-renewable; ignites +200 K over
  biofuel), 7 `CO2`, 8 `Methane` (photolysis decay).

### 6.4 Conservation invariants

- Tides conserve `Σ water_depth` bit-exact.
- Hydrology conserves `Σ water_depth + Σ vapour`.
- Chemistry conserves `Σ over all substances` (combustion 1 Fuel + 1 Oxidiser
  → 2 Ash; regrowth 2 → 1 + 1; phase transitions stoichiometric). Weathering's
  CO₂ removal and Volcanism's CO₂/H₂O addition are tracked separately (not
  counted as leaks).
- Cumulative tidal heat matches cumulative orbital-energy loss to 1% over a
  100-tick window.
- Debug asserts: per-tick drift < 1e-6, cumulative drift < 1e-6 (1e-4 for
  hydrology's documented LSB-truncating clamps).

---

## 7. Recognition layer

### 7.1 Purpose

Translate emergent physics state into discrete named phenomena (firings) that
civs hypothesize about. There is no authored catalogue of "what this run
knows"; a default template set captures the shapes physics reliably produces,
civs discover regularities within the firing stream, and species may mint new
templates.

### 7.2 Functional requirements

1. Ship a default library of **~40 authored templates** (ids 1..=53) whose
   signatures the physics layer reliably produces.
2. `scan` iterates template-major then cell-major per tick, emitting a
   `Firing { template_id, cell }` per match; authored templates fire first,
   discovered templates second, stable as the discovered set grows.
3. Each `RecognitionTemplate` matches a `Signature` per cell per tick and
   declares the `channels` (modalities) that natively sense it.
4. **Two-pass filtering at run start**: (a) physical-presence pass drops
   templates whose field never reaches threshold on any cell; (b) sensorium
   pass splits survivors into perceivable-now (species channels ∩ template
   channels ≠ ∅) vs latent.
5. Latent templates promote to perceivable-now when a sensorium-extending tool
   unlocks; on promotion the hypothesizer recomputes the candidate
   cross-product, preserving existing `(template, channel)` buffers (stable
   relation id) and giving new pairs fresh buffers.
6. The civ hypothesizer candidate cross-product is
   `perceivable_template_ids × perceivable_channels`, both axes sensorium-
   restricted, so differently-sensing species draw structurally different
   observational manifolds.
7. Climate-band membership is derived from a normalised offset
   `o = (T − mean)/gradient`, quartered, from an immutable per-run
   `PlanetContext`.
8. `Signature::MonthIn(start, end)` reads `tick % orbital_period_months` and
   supports wrap-around ranges.
9. **Emergent templates**: when a civ confirms a `ThresholdStep` law on a
   `(template, channel)` pair, mint a `DiscoveredTemplate` (per civ, every
   ~600 ticks, metabolism-aware) iff: the form is `ThresholdStep`; the
   channel maps to a recognition `Field`; the `(field, threshold)` is not
   within 20% of any authored signature; and no prior discovered template
   covers the pair. Minted templates take id ≥ 1000, signature
   `Above(field, threshold)`, persist in the species canon across collapse
   boundaries, and feed `scan` identically to authored ones.
10. A reducing-atmosphere world never fires `fire` (gated on planet-derived
    `AboveIgnition` + `Above(Oxidiser, 0)`); impossible templates are dropped
    by the physical-presence pass.
11. The form vocabulary is gated by the structural tags of a civ's perceivable
    templates — a civ that never observes a `Periodic` template cannot fit
    sinusoidal forms.
12. `Firing` records are **not** emitted to the protocol log (too noisy); they
    live in the `scan` return value and feed the hypothesizer. A
    `TemplateDiscovered` event fires on mint.

### 7.3 Data model

- **Signature** variants: `Above/Below/AbsAbove(Field, Real)`,
  `All/Any(Vec<Signature>)`, `MonthIn(start, end)`,
  `Hemisphere(Northern|Southern)`, `InClimateBand(band)`, `AboveIgnition`,
  `TidallyLockedTerminator`.
- **Field**: `Temperature`, `Charge`, `WaterDepth`, `Substance(s)`,
  `MagneticMagnitude`, `WindMagnitude`, `Resonance`.
- **ChannelKind** (15 sensory modalities): AcousticAir, AcousticWater, Seismic,
  VisualLight, VisualPolarization, Bioluminescent, ChemicalPheromone,
  ChemicalTaste, Tactile, ElectricField, MagneticSense, InfraredThermal,
  RadioNative, Gestural, Postural.
- **DiscoveredTemplate**: id ≥ 1000, name, signature, tags, channels,
  discovered-at tick, discovering civ, origin template; stored in the species.

---

## 8. Species layer

### 8.1 Purpose

The species is the run's persistent unit, derived **deterministically from the
planet** (not sampled independently), so its shape reflects the niche the
planet provides. Its traits drive sensorium, manipulation, demographics,
cognition cadence, ecosystem role, lifecycle, tolerance, dormancy, and a
cosmology baseline.

### 8.2 Functional requirements

1. Derive a species purely from `{planet seed, recognition template ids}`;
   identical inputs produce a bit-identical species. The species-sampling RNG
   XORs a constant into the seed so species sampling cannot entangle with
   mid-run physics draws.
2. Split cognition into three orthogonal axes (`working_memory`, `abstraction`,
   `social`), each `[0,1]`; scalar `cognition = axes.average()`. Per-axis
   jitter is `[-0.15, +0.15]`, zero-summed before clamping.
3. Assign a `CognitionTopology` with distribution Centralized 70% /
   DistributedRedundant 15% / Collective 10% / Acentric 5%.
   DistributedRedundant gets a ×1.10 cognition bump (ceilinged at 1.0).
4. Gate the modality pool on planet conditions before sampling; always include
   `Tactile` when any biosphere yields ≥1 channel.
5. Gate the manipulation pool on planet composition; `ToolExtension` is the
   prerequisite for tier-3+ material culture and the experiment apparatus.
6. Derive a `Habitat` by a fixed precedence chain; habitat gates territorial
   claims (a civ may natively claim only habitat-matching cells until it
   unlocks `AmphibiousConstruction`).
7. Derive a 5-axis `ToleranceEnvelope` from the substrate default ± 20% per-axis
   jitter (`lo <= hi` maintained). `contains()` is a hard occupancy gate;
   `match_score()` returns `[0,1]` fit via the weakest-link axis (peaks 1.0 at
   range centre, linear to 0.0 at edges, 0.0 out of range).
8. Derive `PopulationBiology` from traits via an r/K axis.
9. Sample `dormancy_capability ∈ [0,1]` strongly skewed toward 0
   (tardigrade-grade `>0.9` only in the top decile of seeds).
10. Derive `initial_cosmology: [Real;5]` bias via additive rules, clamped
    ±0.50 per axis (see §8.4).
11. Maintain two cross-civ-persistent registries — `discovered_templates` and
    `dynamic_tool_registry` — indexed from id 1000.
12. Flip `is_extant = false` when biomass stays below extinction threshold for
    the confirmation window; retain the record for history.
13. The substrate `metabolism()` factor scales all downstream
    biological/societal rates.

### 8.3 Data model

- **Species**: seed, name, `cognition` (scalar), `cognition_axes`, `sociality`,
  `communication_fidelity`, `lifespan_years`, `modalities`,
  `manipulation_modes`, `perceivable_templates`, `cognition_topology`,
  `habitat`, `discovered_templates`, `dynamic_tool_registry`,
  `initial_cosmology`, `biology` (PopulationBiology), `tolerance`, `lifecycle`,
  `role` (EcosystemRole), `dormancy_capability`, `plasmids`, `is_extant`.
- **CognitionTopology** (variant → attempt-period mult / knowledge-decay mult /
  abstraction cap / isolation penalty): Centralized 1.0/1.0/1.0/1.0;
  DistributedRedundant 0.7/1.0/0.6/1.0; Collective 1.0/1.0/1.0/0.05; Acentric
  5.0/0.2/1.0/1.0.
- **ToleranceEnvelope**: temp (K), pH (0–14), salinity (g/L), radiation_max
  (Earth ≈ 1.0), pressure (atm).
- **PopulationBiology**: clutch size, infant/maturity/eldership fractions,
  infant/juvenile survival, food multipliers, events per fertile window,
  reproductive success. Invariant: `fertile_fraction > 0`.

### 8.4 Key derivation rules

- **Modality count by biosphere**: HyperBiodiverse 5–7, Lush 3–5, Sparse 2–3,
  None 1. Gating: sub-surface ocean cuts visual; no-atmosphere cuts acoustic-
  air/pheromone; no-magnetosphere cuts magnetic sense; RadioNative requires
  Strong magnetosphere; bioluminescent/chemical-taste require a biosphere;
  tactile/infrared universal.
- **Habitat (6)**: Aquatic, Terrestrial, Amphibious, Airborne, Subterranean,
  Endolithic (native to Silicate).
- **Substrate default tolerance envelopes** (temp / pH / sal / rad / press):
  Aqueous 273–373 / 5–9 / 0–50 / 0.5 / 0.5–2; Ammoniacal 195–240 / 9–12 /
  0–100 / 0.8 / 0.5–5; Hydrocarbon 91–117 / 3–7 / 0–10 / 1.2 / 1–10; Silicate
  1687–3538 / 0–14 / 0–200 / 5.0 / 1–100; ± 20% jitter.
- **r/K derivation** (axis 0 = pure K … 1 = pure r): equally-weighted drivers
  (low sociality, short lifespan, manipulation r-lean) + habitat adjustment
  (Aquatic +0.10 r, Airborne −0.10 r), driving every demographic field.
- **Cosmology bias** (axes empirical / communitarian / reformist / mystical /
  hierarchical): high sociality → +communitarian; high cognition → +empirical;
  low cognition or low comm-fidelity → +mystical; aquatic/subterranean →
  +communitarian; high axial tilt → +reformist; etc. `hierarchical` stays 0 at
  genesis (driven by catastrophe events later).

---

## 9. Population layer

### 9.1 Purpose

Evolve per-cell heterogeneous age-structured cohorts each tick under a
4-bracket model whose rates derive from per-species biology, with seven
lifecycle variants, gradient-driven migration, a pre-civ nomadic pool,
seasonal capacity, and catastrophe/dormancy handling.

### 9.2 Functional requirements

1. Represent each cell's population as a 4-bracket `Cohort` (`infant`,
   `juvenile`, `fertile`, `elder`) + optional `civ_membership`; only `fertile`
   reproduces and carries full economic/military weight.
2. Store population as Q64.32 `Pop` so sub-integer births/deaths accumulate.
3. Civ-tagged and stateless cohorts coexist in one `BTreeMap<cell, Cohort>`;
   post-collapse remnants persist as `civ_membership: None` until absorbed or
   decayed.
4. Capacity comparison uses food-weighted demand `Σ bracket × food_multiplier`
   (0.30 / 0.60 / 1.00 / 0.90), not raw headcount.
5. `food_security = 1 − max(0, demand/capacity − 1)` clamped `[0,1]`; capacity
   ≤ 0 forces extinction.
6. Rates are cached per-month and pinned to `BASELINE_MONTHS_PER_YEAR = 12`
   (orbital period affects display only). Per-tick survival derives from
   window-survival via `per_tick = exp(ln(window_survival)/months)`. Elder
   window-survival is flat 0.30 (programmed senescence).
7. Per-tick step order: demand + food-security → compose per-bracket survival
   (tech mortality reduction, stress amplification, additive starvation) →
   births (`fertile × birth_rate × multiplier × security`, clamped at the **5×
   fertile recruit ceiling**) → apply survival (births land in infant) → aging
   promotions → floor at zero. Survival applies before aging.
8. Dispatch each tick by lifecycle variant; mismatched state falls through to
   the vertebrate step (no panic).
9. `life_expectancy_months()` is a pure competing-hazards computation
   reflecting tech mortality reduction.
10. Cell capacity = `base_capacity × habitability_multiplier ×
    seasonal_multiplier × (1 + Σ tool_capacity_effects)`.
11. Cells whose pressure exceeds the migration threshold shed population to
    adjacent cells with headroom, conserving pair-flux.
12. Catastrophes hit specific cells (not whole civs), preserving age structure;
    disease targets the densest cell, asteroid a deterministic `(seed, tick)`
    cell.
13. Dormancy survivors land in a `DormantPool` reviving at 1%/tick, capped at
    pre-event population; resurrection is deterministic Q32.32.

### 9.3 Lifecycle variants (7)

Vertebrate (baseline 4-bracket); Aquatic{semelparous} (one-shot spawn,
fertile+elder → 0) / Aquatic{iteroparous} (vertebrate + 70% metamorphosis cull
on juvenile→fertile); Insect (vertebrate + extra adult mortality); Plant (elder
survival ≥ fertile, infant halved by seed failure); Eusocial{castes} (per-caste
map; only Reproductive breeds); Microbial{fission} (single biomass, generation
doubling: Binary 1 tick / Budding 2 / Conjugation 4); Modular (logistic colonial
biomass).

### 9.4 Key parameters

- Founding floor `50 + 35×bio_pressure + 15×(1−cognition)`.
- Carrying capacity `50,000 × cognition_factor × resolution_factor` (reference
  1080 cells; rescales inversely with cell count).
- Migration-pressure threshold `0.55 + 0.20×sociality` (tech-augmented up to
  0.92).
- Tech-tier capacity stack: paleolithic ~50k/cell, agricultural ~M/cell,
  industrial ~10M/cell, modern hundreds of M/cell.
- Substrate metabolism time-scaling: Aqueous 1× / Ammoniacal 2× / Hydrocarbon
  2.5× / Silicate 5×.
- Nomad diffusion base 1/100 per tick at 80-yr-lifespan baseline, lifespan-
  rescaled `[0.25, 4.0]`; non-habitat decay 1/500 per tick; density-gradient
  diffusion prefers habitat-matching neighbours.

---

## 10. Ecosystem layer

### 10.1 Purpose

Build a per-planet biota of 8–20 typed species with a typed pairwise
interaction matrix and run a per-tick multi-species step (Lindeman trophic
pyramid, Holling functional responses, keystone detection, biogeochem CO₂
loop), plus horizontal gene transfer (HGT) and speciation.

### 10.2 Functional requirements

1. Sample 8–20 species honoring role-distribution minima (≥2 Producers, ≥3
   PrimaryConsumers, ≥2 SecondaryConsumers, ≥1 ApexConsumer, ≥1 Detritivore,
   ≥1 Saprotroph, 1–3 Mutualists, 1–5 Parasites; cap 20). Starting biomass
   already respects the 10:1 Lindeman pyramid. Use a dedicated salted RNG
   stream and `BTreeMap`/`BTreeSet` only.
2. Wire canonical interaction edges (consumers prey on the tier below
   Saturating; same-tier competition; mutualists pair the first producer;
   parasites target the first primary consumer; detritivore + saprotroph
   habitat-modify all producers).
3. Per-tick flux via `FunctionalResponse`: Linear `s·prey`; Saturating Type-II
   `s·prey/(k+prey)`; Sigmoidal Type-III `s·prey²/(k²+prey²)`.
4. Predation assimilates gross flux through a per-habitat Lindeman efficiency
   (30:1 aquatic, 10:1 terrestrial, 6.7:1 amphibious/airborne) — the pyramid
   emerges from this single ratio with no corrective cap.
5. Producers drift toward `producer_capacity` at the growth rate when ungrazed;
   non-producers pay passive decay. Chemoautotroph growth is additionally
   capped by oxidiser availability.
6. Flag a species extinct (`SpeciesExtinct`, cause `PopulationCollapse`) when
   biomass < `0.001 × capacity` for the confirmation window (~12 ticks); retain
   the record.
7. Maintain per-cell biomass with invariant `sum(cell_biomass) == biomass`;
   per-cell catastrophe pokes drain only the targeted slice, scaled by
   `(1 − tolerance.match_score(local_conditions))`.
8. Detect keystone species via betweenness centrality above 0.15 of max.
9. Biogeochem loop returns carbon to atmospheric CO₂: consumers respire 1%/tick,
   decomposers liberate 0.5%/tick (gated on ≥1 decomposer); producers are net
   sinks.
10. HGT runs for Microbial species as a two-phase selection event (acquisition
    deposit → per-tick sweep-vs-loss), never smooth interpolation; at most one
    trait axis snaps to the donor per sweep.
11. Support five speciation triggers (allopatric, sympatric, polyploid,
    founder-effect, post-extinction radiation) with deterministic daughter-
    trait inheritance (±5% per axis). A `SpeciesExtinct` opens a 100-tick ×5
    adaptive-radiation window. HGT/speciation rates are modulated by the
    magnetosphere-derived cosmic-ray multiplier (strong field suppresses the
    mutation pump).

### 10.3 EcosystemRole (8 + sub-kinds)

Producer{Photoautotroph|Chemoautotroph|Mixotroph}, PrimaryConsumer,
SecondaryConsumer, ApexConsumer, Detritivore, Saprotroph,
Mutualist{Pollinator|SeedDisperser|Engineer|Generic},
Parasite{Macro|Micro|Virus}. The civ-bearing species is always a consumer tier
with cognition ≥ 0.3 (worldgen filter).

---

## 11. Civilization layer

### 11.1 Purpose

A civilization is a bounded, transient collectivity within a persistent
species. Civs found, run, collapse, and are succeeded by other civs that
inherit territory and partial knowledge. Multiple civs may exist concurrently
or sequentially. Civ collapse is **not** a run-end condition.

### 11.2 Functional requirements

1. Support multiple concurrent and sequential civs per species; no hardcoded
   inaugural civ.
2. Found a civ emergently when a nomadic region accumulates density, the
   species crosses the tech-readiness gate, and a habitable centroid exists.
   Inaugural founding has `parent_civ_id = None`; breakaway / stateless
   re-founding set the parent.
3. Track per-civ drift on the species baseline (cognition, sociality, lifespan,
   communication fidelity). Inaugural civs zero deltas; each successor inherits
   parent deltas + deterministic per-generation perturbation
   (`±0.02` traits, `±1` year lifespan), plus catastrophe selection bias capped
   `0.15` per channel. Emit `SpeciesDrift` per new civ.
4. Maintain per-civ cohesion `[0,1]` drifting toward an equilibrium driven by
   size, food security, dogmatism, literacy.
5. Collapse a civ when any per-civ trigger crosses threshold (first streak
   wins; streaks may compound).
6. Support two breakaway paths (cohesion-driven, dogmatic), each producing a
   successor inheriting parent state, with a centroid shifted off the parent.
7. Re-found a civ from a stateless cohort when population, recency, and dark-age
   conditions are met.
8. Register inter-civ contact when claimed-cell sets touch (Manhattan ≤ 1) for
   the first time; contact is a **hard prerequisite for war**.
9. Resolve inter-civ conflict cell-by-cell on a fixed skirmish cadence
   (casualty math, cell flips, alliances, grudges).
10. Persist the species and knowledge artifacts across civ collapse. On
    collapse: unclaim territory, kill named figures, allow knowledge to survive
    only via the transmission/inheritance window.
11. Accumulate per-civ economic surplus above subsistence, feeding food-
    security buffering, war strength, and catastrophe absorption. Peaceful
    contacted civs open trade routes smoothing surplus until war or collapse.
12. Scale per-cell carrying capacity by an ecological-resilience scalar `[0,2]`
    derived from the civ's share of primary-producer biomass.

### 11.3 Collapse triggers

| Reason | Trigger |
|--------|---------|
| FoodCrisis | `food_security ≤ 0.3` for ~100 yr |
| KnowledgePlateau | no confirmed/refinement event for ~500 yr |
| CulturalLock | `dogmatism > 0.85` for ~250 yr with no refinements |
| TerritoryTooSmall | `claimed_cells ≤ 1` for ~2 yr |
| CivilWar | cohesion `< 0.10` sustained ~75 yr |
| Depopulation | aggregate pop ≤ 1 for ~2 yr |

(Year windows scale by substrate metabolism.)

### 11.4 Breakaway

- **Cohesion-driven**: cohesion in `[0.10, 0.35]` for ~40 yr → fork; faction
  takes 30% of parent pop, starts at 0.85 cohesion; parent recovers +0.15.
- **Dogmatic**: cosmology dogmatism over threshold AND a heretical hypothesis
  force-rejected → fork centered on the heretical view.

### 11.5 Conflict / war / alliance / grudge

- **Belligerence**: `drive = 0.45·pressure + 0.25·opportunity + 0.30·dominance`;
  `belligerence = drive × (1 − 0.10·kinship)`. `WarDeclared` at ≥ 0.25,
  `PeaceConcluded` at < 0.15 (hysteresis).
- **War resolution** (cadence ~75 ticks): `loss_frac = 0.10 + winner_hierarchy
  bonus (≤ +0.30) + tech_gap (≤ +0.30)`, capped 60%/skirmish. Casualties hit
  the fertile bracket first. Cells flip when the loser's per-cell cohort drops
  below the flip floor (~25). `hierarchical_strength` = cosmology 40% + religion
  20% + kinship/cohesion 25% + economic 15%. Mutual alliances short-circuit war.
- **Alliance**: forms on five cumulative conditions (cosmology + religion
  distance < 0.40, not at war, mutual contact, cooldown clear). Dissolves on
  drift past 0.60, trust < 0.20, or war misalignment.
- **Grudge**: per ordered pair, asymmetric (loser holds longer), lazy decay,
  ceiling 0.60; subtracts from kinship.

---

## 12. Culture layer

### 12.1 Purpose

A two-layer cultural model: a slow-drift, species-anchored **cosmology** (deep
worldview constraining plausible science) and a fast-divergent, civ-keyed
**religion** (drives intra-species war via kinship), plus the transmission
machinery ferrying knowledge across collapse boundaries.

### 12.2 Cosmology — 5 slow-drift axes

`empirical` (mystical ↔ measurement), `communitarian` (individualist ↔
communitarian), `reformist` (dogmatic ↔ open-to-revision), `mystical`
(mechanistic ↔ mystical), `hierarchical` (egalitarian ↔ hierarchical); each
`[-1,1]`. `dogmatism() = (magnitude / sqrt(5)).clamp01()`. Every civ inherits
the species' `initial_cosmology` at founding. Hypothesis-engagement events push
the axes by a small table; `CosmologyShifted` re-emits on L2 drift ≥ 0.50.

### 12.3 Religion — 3 fast-divergent axes

`theology` (monist ↔ pluralist), `ritual` (pragmatic ↔ liturgical),
`sacred_time` (cyclical ↔ eschatological). Each civ receives a fresh founding-
figure-driven vector (deterministic per-civ jitter + figure-trait offsets from
charisma/doubt/curiosity). Religion drifts at **3×** cosmology magnitude on the
same events; `ReligionShifted` re-emits on drift ≥ 0.20.

### 12.4 Form preferences

Both layers bias which fitted forms a civ confirms. A `combined_form_distance`
in `[0,1]` (higher = more heretical) suppresses confidence during fitting:
e.g. empirical cosmology rewards ThresholdStep/Logarithmic; mystical rewards
PeriodicSine; reformist rewards Logistic/PowerLaw; monist theology rewards
unifying forms; liturgical ritual rewards step semantics; cyclical sacred-time
rewards periodic.

### 12.5 Kinship

Weighted closeness: religion 0.60 + non-hierarchical cosmology 0.15 + literacy
0.15 + hierarchical cosmology 0.10, attenuated by generation closeness
(`exp(−|Δgen|/8)`) and the decayed per-pair grudge. Kinship dampens
belligerence multiplicatively.

### 12.6 Transmission across collapse

- **Comprehension** = `(1 − linguistic_distance) × age_decay × tier_factor(0.7)
  × comm_speed × per-civ multipliers`. Linguistic distance is Jaccard over the
  two civs' name-grammar atom sets. Comm-speed is the species' max channel
  speed (acoustic/visual/radio 1.0 … tactile 0.1). Per-civ multipliers:
  communicativeness boost, settlement-persistence ladder (0.85–1.30), tool-
  transmission fidelity (≤ ×1.5).
- **Banding**: score > 0.15 → relation transfers (confidence scaled by
  comprehension, held as confirmed pending revalidation); 0.03–0.15 →
  **Mythologized** (no transfer; perturb successor cosmology along one axis,
  60% mystical-biased; emit `RelationMythologized`); ≤ 0.03 → lost.
- **Inheritance revalidation**: transmitted relations enter confirmed tagged
  with inheritance metadata and `confidence *= comprehension`. After ~50 ticks
  the successor re-fits: passes graduate to native, failures emit
  `RelationLapsed` and drop.
- **Concurrent peaceful diffusion**: between concurrent peaceful civs (both
  hierarchical axis < 0.40), each tick exchanges a fraction of confirmed
  relations — same linguistic factor, no age decay, skipping already-confirmed
  relations.

---

## 13. Discovery / science layer

### 13.1 Purpose

Derive named scientific laws and their SI coefficients from emergent physics.
Each named figure inside a civ owns a `Hypothesizer` that observes physics,
proposes functional-form hypotheses against sensed channels, confirms /
refines / falsifies them, and gates tech unlocks. Discoveries are **fits
against real simulated data, not authored truth tokens.**

### 13.2 Functional requirements

1. Each `NamedFigure` owns a `Hypothesizer`; samples and confirmations are
   attributed per figure.
2. Run two parallel candidate tracks per hypothesizer: **firing relations**
   (binary — did a template fire?) and **measurement relations** (continuous —
   physics channel vs channel).
3. Maintain a rolling per-relation sample window capped at 200.
4. Throttle fit attempts per relation (`attempt_period`, default 20 ticks,
   species-derived, divided by the tech discovery-rate multiplier, floored at
   1 tick).
5. Restrict the candidate cross-product to `perceivable_template_ids ×
   perceivable_channels`, both sensorium-constrained.
6. Store samples in fit-space (`raw / channel_scale`); rescale confirmed params
   to SI for emission.
7. Per tick per figure, run four phases: firings → sample collection → fit
   attempts → residual cascade.
8. Fold cosmology suppression + empirical/reformist focus weighting into
   candidate confidence before the confirm check; scale each figure's
   cosmology/religion push by its charisma.

### 13.3 Form vocabulary (12 forms)

Constant, Linear, Logarithmic, ExpDecay, ExpGrowth, PowerLaw, InverseSquare,
ThresholdStep, Polynomial2, Polynomial3, Logistic, PeriodicSine. Each carries a
param arity, a minimum-sample floor `k`, and a base tolerance (5%–30%). Forms
are unlocked by the structural `FormTag`s of the civ's perceivable templates
(baseline always grants Constant + Linear); a species lacking a tag literally
cannot propose that form. Closed-form fits use Gauss-Jordan on the normal
equations; ThresholdStep uses search-based fitting.

### 13.4 Confirmability

```
residual   = RMSE(y_i, f(x_i))
tolerance  = base_per_form × (1 / intelligence) / sqrt(n)
confidence = exp(−residual / tolerance)              [confirmable iff ≥ exp(-1)]
n_min      = ceil(k_form / intelligence)
```

Cosmology weighting: `confidence = (raw × suppression × focus).min(1.0)`, where
`suppression = clamp(1 − dogmatism × form_distance, 0, 1)` and
`focus = 1 + 0.25·empirical + 0.25·reformist`.

### 13.5 Lifecycle

- **Confirm**: first form clearing `confidence ≥ exp(-1)` becomes the confirmed
  primary → `RelationConfirmed` / `MeasurementConfirmed` (latter flags
  `is_experimental` if apparatus contributed).
- **Refine**: a confirmed relation tracks a `low_confidence_streak`
  (`≤ exp(-2)`) and a `falsification_streak` (RMSE > 1.5× confirm-time
  residual). On trigger, an Occam-adjusted alternative search; if the best
  alternative beats the active form by `switch_margin × doubt_scale`, emit
  `RefinementProposed` and begin probation. Probation resolves to
  `RefinementConfirmed` (swap form, old → rivals) or `RefinementRejected`
  (revert, 100-tick cooldown).
- **Falsify**: `falsification_streak` reaching its trigger emits `Falsified`
  and forces refinement faster than the confidence path.
- **Theory hierarchy**: on measurement confirm at depth < 3, auto-generate a
  residual child fitting `y − basis_prediction` (frozen basis). Max depth 3.
- **Rivals**: a per-relation pool of competing confirmed hypotheses; a rival is
  accepted only if its form duplicates neither the primary nor an existing
  rival; the best rival can displace the primary.
- **Revalidation**: inherited relations re-fit after ~50 ticks → `Revalidated`
  (graduate) or `Lapsed` (drop).

### 13.6 Experiments / apparatus

`record_experimental_measurement` pushes each `(x, y)` pair into the buffer
**twice** (information-density advantage) and bumps an experimental counter; a
relation confirmed with any apparatus contribution is tagged `is_experimental`.
The apparatus clamps one channel through a ladder of values and measures the
relaxation response (see §14.7).

### 13.7 Channels

14 discovery read channels (distinct from sensory `ChannelKind`): Temperature,
WaterDepth, ChargeMagnitude, Elevation, Fuel, Oxidiser, Vapour, Ice, Fossil,
MagneticField, Resonance, Optics, Tidal, Radiogenic (last four are the
archetype lever substrates). A `channels_for_modality` bridge maps each sensory
modality to the discovery channels it can sample; empty unions fall back to
Temperature + Elevation.

---

## 14. Tech layer

### 14.1 Purpose

A real DAG of tools unlocking from confirmed science, body-plan capability,
sensorium, literacy, and resources. It mints both authored static tools and
runtime-emergent dynamic tools, all folding through one effect aggregator and
one mass-conservative chemistry mirror.

### 14.2 Functional requirements

1. Enumerate ~58–71 static tool variants across **5 tiers** (stone-age →
   settlement → pre-industrial → industrial → information-age/transcendence).
2. Maintain a parallel per-species **dynamic tool registry** minting tools at
   runtime from confirmed-relation clusters.
3. Unlock a static tool the moment all gates pass; emit `TechUnlocked(civ,
   tool, via_serendipity=false)`.
4. Support **serendipitous unlocks** when exactly one prereq is missing, via a
   deterministic per-tick dice roll, emitting `via_serendipity=true`.
5. Fold every tool's contribution across the civ's tools + dynamic registry
   into the effect aggregators.
6. Enforce DAG invariance: every tool prereq is strictly lower tier.
7. Draw down Fuel/Fossil per tick mass-conservatively for fuel-consuming tools.
8. Guarantee a no-fire alt-path to late-game capacity (WindPower, HydraulicWorks,
   BiomimeticDesign, EcosystemEngineering, etc.) so non-combustion worlds reach
   an industrial age.

### 14.3 Gates

| Gate | Meaning |
|------|---------|
| `prereq_channels` | Native sensory channels required. |
| `manipulation_prereqs` | Body-plan modes that can fabricate (tier-1 ~10 modes; tier-5 ~3–5). |
| `relation_prereqs` | Confirmed `(template, channel)` relations — substrate locks live here. |
| `tool_prereqs` | Other tools unlocked (strictly lower tier). |
| `min_civ_confirmed_relations` | Civ confirmed-relation count: 0 / 5 / 15 / 50 / 200 by tier. |
| `min_civ_experimental_relations` | Apparatus-backed relations: 0 / 0 / 3 / 12 / 80. |
| `literacy_floor` | 0.00 / 0.15 / 0.30 / 0.50 / 0.65 by tier. |
| `species_maturity_floor` | Sum of confirmed relations across all civs: **3000** for all tier-5 tools. |
| `crust_prereqs` | Crust whitelist (e.g. field-propulsion needs piezoelectric/ferrous/rare-earth). |
| `resource_prereqs` | Substance threshold summed over claimed cells; hard gate (serendipity cannot bypass). |

### 14.4 Substrate divergence

Three layered gates make sciences diverge by world: sensorium-extending tools
require the domain channel; `relation_prereqs` impose substrate locks
(combustion needs confirmed `fire` → deep-ocean methane/ammonia worlds locked
out; chemical synthesis needs `hydrocarbon_seep`); `tool_prereqs` inherit those
locks transitively. The experiment apparatus uses `tidal_extremum` (reachable
on every habitable world), so it is not combustion-locked.

### 14.5 Effect categories

`*_multiplier` (multiplicative) / `*_bonus` (additive): capacity, food-crisis
resistance, war strength, seasonal floor, catastrophe resistance, literacy,
transmission fidelity, expansion rate, cohesion, migration speed, discovery
rate, fertility, per-bracket mortality reduction, lifespan extension. The
discovery-rate aggregate divides the hypothesizer's `attempt_period`.

### 14.6 Serendipity

When exactly one relation-or-tool prereq is missing (literacy floor relaxed to
75%; resource + maturity floors stay hard), a deterministic ChaCha20 roll keyed
on `(planet_seed, civ_id, tool_id, tick)` — base ~2.5e-5/tick, scaled by
literacy (×0.5–×2.0) and accumulated science (×1–×5), clamped `[1e-6, 1e-3]` —
may unlock the tool. Mature civs see ~1.5–3 serendipitous unlocks per lifetime.

### 14.7 Experiment apparatus (tier 2)

Gated by confirmed `tidal_extremum`, ≥5 confirmed relations, literacy ≥ 0.30,
any manipulation mode. One `Apparatus { cell, clamp_channel, measure_channel }`
per civ, allocated to the lowest-population claimed cell. Per tick: pre-physics
clamp write (ladder keyed by `tick % 4`) → physics integrates with clamp held →
post-physics `(clamp, response)` fed to the first active figure's hypothesizer.
Successors do not inherit apparatus.

### 14.8 Dynamic tools

`propose_dynamic_tools` runs per civ every ~240 ticks (metabolism-aware): group
confirmed relations by channel; a channel with ≥5 confirmed relations mints a
tool; refinement requires the cluster grow by ≥5 more. Effects scale by
`min(cluster, 20) / 20`, with per-channel flavour. Substance-channel clusters
carry a flat resource gate; abstract-channel clusters do not. Dynamic tools are
tier-5 by convention.

### 14.9 Tier-5 transcendence trio

`BioelectricResonator`, `FieldPropulsionEngine`, `MetamaterialLattice` —
sustaining all three drives the `transcendence` run-end. These are **narrative
milestones, not simulated capabilities** (no consciousness physics, no FTL).
`EcosystemEngineering` is the bio-engineering peer (no-fire late-game route).

### 14.10 Resource consumption

`apply_tool_consumption` draws Fuel/Fossil down across claimed cells for every
fuel-consuming tool, mass-conservatively (1 Fuel + 1 Oxidiser → 2 Ash; capped
by availability). Water/Ice/Vapour are read-only (may be required present, not
drawn).

---

## 15. Catastrophes

### 15.1 Purpose

Five cell-localized hazard kinds plus a uniform damage / survival / recovery
chain, each with its own trigger predicate, cooldown, and pop-loss fraction.

### 15.2 The five kinds

| Kind | Trigger | Cooldown (yr) | Base loss | Scope |
|------|---------|---------------|-----------|-------|
| Volcanic | Cell crust breaches solidus (`\|charge\| > 80`, `T > 600 K`) | 200 | 5% | Cell + neighbours; resets fuel, −50 K |
| Disease | Crowding past per-substrate threshold; age ≥ 300 yr | 500 | 30% | Densest claimed cell + spread |
| Asteroid | Deterministic low per-tick probability (~5000-yr period) | 5,000 | 40% | Impact + neighbours; +5 radiation |
| SolarFlare | High irradiance + weak local magnetosphere | 800 | 10% | All cells; radiation scaled by cosmic flux |
| IceAge | Sustained planet-mean temperature drop (mean T ≤ 260 K, age ≥ 1000 yr) | 4,000 | 20% | All cells; −temperature drop |

Cooldowns are per-month-tick (× 12). Dispatch precedence: volcanic → disease →
asteroid → solar flare → ice age; first hit wins.

### 15.3 Damage chain

Three sequential attenuation layers:

```
base_loss            = raw_frac × (1 − civ_tool_resistance)
loss_after_dormancy  = base_loss × (1 − dormancy × severity)
loss_after_tolerance = loss_after_dormancy × (1 − tolerance.match_score(cell))
```

1. **Tool resistance** — catastrophe-mitigating tools reduce headline severity.
2. **Dormancy** — high-dormancy species divert the surviving fraction into a
   `DormantPool` that drains back at ~1%/tick, capped at pre-catastrophe pop
   (enables mass-extinction recovery).
3. **Tolerance** — the cell's fit to the species envelope scales the remaining
   loss; a perfect-fit extremophile takes ~0 damage.

All kinds except Disease (host-biology-internal) propagate into the ecosystem,
draining biomass at affected cells scaled by `(1 − match_score)`. SolarFlare
radiation scales bidirectionally with the cosmic-ray flux clamp `[0.2, 5.0]`
(strong dipole dampens up to 5×; reversal windows amplify up to 5×, also
multiplying speciation/HGT). Every fire emits `CatastropheFired`; the report
aggregates a per-kind histogram.

---

## 16. Civilizational-archetype framework

### 16.1 Purpose

A run's developmental path is not authored. Every run is scored across an open
space of **11 peer levers** (no privileged default, no fallback), classified
pure / hybrid / emergent, bent through a cognition overlay, and resolved to a
divergent endpoint at transcendence.

### 16.2 The 11 levers

`Combustion`, `FieldResonance`, `Biochemical`, `Cryogenic`, `Mechanical`,
`Hydraulic`, `ExoticChemistry`, `PlasmaEm`, `Gravitational`, `Photonic`,
`Nuclear` — scored identically as peers. Canonical order is the deterministic
tiebreak.

### 16.3 Functional requirements

1. Score every lever in two stages: a **prior** (world + species statics at run
   start — atmosphere, crust, substrate, magnetosphere, biosphere, luminosity,
   moons, gravity, sensorium, cognition) and a **realized refinement** (run
   trajectory — +0.02 per confirmed relation on the channel's lever capped 0.35;
   +0.10 per unlocked tool on its branch). Clamp each score `[0,1]`; scores need
   not sum to 1.
2. Classify the ranked vector: **Pure** (`top ≥ 0.40` and `top − second ≥
   0.12`); **Hybrid** (`top ≥ 0.25` and `second ≥ 0.25`); else **Emergent**,
   signature-named `emergent_{dominant}_{secondary}_{tertiary}` — unforeseen
   paths are detected and labelled, never collapsed onto the nearest authored
   attractor.
3. Derive a **cognition overlay** (`Individual` / `Collective` /
   `SubstrateDistributed`) from cognition topology, orthogonal to the lever —
   any mode can sit on any lever.
4. All classification arithmetic in `Real`, no RNG — labels are byte-stable
   across replays.
5. Resolve a **distinct endpoint per archetype** at transcendence from the
   realized dominant lever (e.g. combustion → industrial apex; field-resonance
   → matter-transition; biochemical → biosphere merge; cryogenic → deep time;
   mechanical → computational crossover). The cognition overlay bends a
   collective/substrate-distributed mind's narrated fate inward toward silence
   (the `endpoint_mode` tag is unchanged; only the description changes).
6. Maintain four **additive per-cell physics fields** (FieldResonance Ψ,
   SolarInsolation, TidalStress, SurfaceRadiation), each with recognition
   templates and a discovery channel so an attuned species does real science on
   that substrate. They are read by no legacy law (every existing channel stays
   bit-identical).
7. Emit two additive events: `ArchetypeDerived` (run start — label, dominant /
   secondary lever, cognition mode, per-lever scores) and `ArchetypeEndpoint`
   (transcendence — civ, label, dominant lever, cognition, endpoint tag,
   narrated description). The `transcendence` run-end reason is unchanged.

---

## 17. Event protocol

### 17.1 Requirements

1. Schemas live in `protocol/` and are the authoritative sim↔consumer contract.
   **No ad-hoc fields on either side.**
2. The log is NDJSON: exactly one JSON event object per line.
3. A `SCHEMA_VERSION` is emitted in the `RunStart` header so a consumer can
   refuse a newer-version log; bump it on incompatible changes.
4. The run header carries seed, schema version, sim version, run length.
5. Emission order equals caller iteration order; emitters never reorder, batch,
   or sort. The emitter stack supports fan-out (tee), predicate filtering
   (powers `--cli highlights`), and throttling (sleeps only on the tick-end
   boundary, reads wall-clock only, never throttles the file emitter).
6. Fixed-point fields are carried as raw Q32.32 bits with a `_q32` suffix;
   display consumers lossy-convert to `f64`.

### 17.2 Event domains

Run lifecycle (`RunStart`, `Tick{phase}`, `Snapshot`, `RunEnd{reason}`);
planet/worldgen (`PlanetDerived`, `PlanetMap`); species/ecosystem
(`SpeciesDerived`, `SpeciesNomadsChanged`, `SpeciesDrift`, `SpeciationEvent`,
`HgtEvent`, `SpeciesExtinct`); civ lifecycle (`CivFounded`, `CivCollapsed`,
`CivTerritoryChanged`, breakaway, refound, `CivContact`); figures
(`FigureBorn` with charisma/curiosity/doubt/communicativeness); tech
(`TechUnlocked`, `ToolDiscovered`, `TemplateDiscovered`); discovery
(`RelationConfirmed`, `MeasurementConfirmed{is_experimental}`,
`RelationFalsified`, `RelationRevalidated`, `RelationLapsed`,
`RelationMythologized`, refinement proposed/confirmed/rejected, rival
proposed/displaced); culture (`CosmologyShifted`, `ReligionShifted`,
`CohesionShifted`); demographics (`CivResilienceTick`,
`CivLifeExpectancyChanged`, `CivSurplusChanged`); conflict (`WarDeclared`,
`PeaceConcluded`, `ConflictResolved`, `AllianceFormed`, `AllianceDissolved`,
trade route opened/closed); knowledge (`RelationTransmitted`, diffused);
catastrophe (`CatastropheFired{kind}`); archetype (`ArchetypeDerived`,
`ArchetypeEndpoint`).

### 17.3 Snapshots

Emit a `Snapshot` every `SNAPSHOT_INTERVAL_TICKS` (~500–6000 ticks) carrying
tick, active + collapsed civ ids, total species population, and running totals
(confirmed relations, refinements, catastrophes, tech unlocks, transmissions,
diffusions). A snapshot is **not** a full state checkpoint.

---

## 18. Output channels

### 18.1 Live viewport (`--cli viewport` / `viewport-density`)

A live alternate-screen ASCII rendering sharing the post-run report's frame
renderer, so live and keyframe views look identical.

- 74-column box, left map zone + right sidebar; default map 36×30, 1 char/cell.
- Caption: `Y{year} M{month} · {N} civ · {F}F/{C}C · {pop}p`.
- Glyph precedence: centroid → multi-owner `#` → single-owner cell → nomad →
  terrain. Terrain glyphs adapt to a `SurfacePhase` (Earthlike / Lava / IceCap)
  derived from substrate + mean temperature.
- Sidebar: legend (mode-aware), species panel, per-civ panels (sorted by pop
  desc) showing identity / tier / tool count / last tool / founded year / cells
  / pop + trend arrow / cohesion / life expectancy / war-peace / belief axes /
  active wars.
- The viewport is a pure observer: it mirrors only a curated event subset into
  snapshots, paints on the tick-end boundary every `frame_every` ticks, never
  feeds resize or pacing back into the sim, and leaves the NDJSON unchanged.
  `viewport-density` renders claimed cells as density blocks (` ░ ▒ ▓ █`).

### 18.2 Post-run markdown report (`ages-report`)

A pure function of the event log (same log → byte-identical markdown). Sections:
run header; planet card (spectral type, luminosity, HZ edges, gravity, escape
velocity, locking, crust, substrate, atmosphere, temperature, moons) + ASCII
map; species card; species-level views (emergent "ages of the species",
memorable cross-civ figures, species canon); run/ecosystem summary (counts +
speciation/HGT/catastrophe histograms, mean resilience); per-civ chapters
(founding, peak pop/cells, tech tier, catastrophes survived, figures, tech
ladder, discoveries with fitted equations, refinements, cosmology/religion
drift, collapse cause, successors); inter-civ transmission; contact/conflict
(war campaigns, trade routes); concurrent diffusion; population timeline;
highlight reel. Spatial keyframes are reconstructed at fixed intervals via the
shared frame renderer.

**Highlight scoring**: unconditional pins (founding, collapse, catastrophe,
tech-tier crossings, first-of-kind discoveries, transmissions) plus a scored
tail (`0.4·novelty + 0.3·magnitude + 0.2·figure-significance + 0.1·arc-
coherence`; top 5%, capped 25).

### 18.3 Prose narrator

Per-event single-sentence prose to stdout, shared across the live (`--narration`)
and replay (`--replay-narration`) paths. Maintains a `NarratorState` tracking
names so later lines use names not ids. Returns nothing for per-tick markers,
snapshots, and per-cell territory updates (signal-to-noise). `narrate.py` is a
standalone pure-stdlib Python 3 reimplementation with slightly richer prose.

### 18.4 Channels enumerated

(1) NDJSON event log (canonical, always written); (2) live stdout stream;
(3) snapshot events embedded in the NDJSON; (4) post-run markdown report;
(5) prose narration (live or replay).

---

## 19. Known intent-vs-prototype gaps

The prototype flagged these as places where current code diverges from intent.
A clean rebuild should target the **intended** end-state:

1. **Species lifecycle / ecosystem role at genesis.** The prototype's species
   derivation hard-coded `Lifecycle::Vertebrate` and
   `EcosystemRole::PrimaryConsumer`. Intended: the sampler routes the seven
   lifecycle variants and the full non-civ role distribution from substrate +
   traits. (The seven lifecycle *steps* and ecosystem role machinery already
   exist; they were simply not wired at genesis.)
2. **Dynamic tool effects.** `DynamicToolEffects` defaulted to identity pending
   an emergent-effects pass. Intended: emergent tools carry per-channel
   effects scaled by cluster size (§14.8).
3. **Fit metric.** The confirm threshold uses RMSE + exponential confidence at
   `exp(-1)`. A reduced-χ² migration was scoped but blocked on per-channel σ
   being carried alongside measurements. Intended: reduced-χ² once σ is
   threaded.
4. **Lever physics coverage.** Only four of the eleven levers (field-resonance,
   photonic, gravitational, nuclear) have dedicated physics fields + discovery
   channels; the other seven are scored from prior + existing channels.
   Intended direction: dedicated substrates for more levers as consumers
   appear.

---

## 20. Out of scope (deliberate boundaries)

- LLM in the loop (hard rule — downstream consumers may add one over the event
  log).
- Native-GUI live UI (terminal-only; a live *ASCII* viewport ships).
- Networked / multi-process sims (single-process by design).
- Save/load beyond snapshot digests.
- Mod hooks.
- Per-individual general population (the sim models named figures + cohorts,
  not the general population as agents).
- Inter-planet civ contact; multi-species worlds (single-planet, single-species
  by design).
- Quantum-scale physics; turbulent fluid regimes; real-meteorology fidelity;
  consciousness physics; FTL.
- Audio.

---

## Appendix A — Representative run shape

A seed-42 / 5000-year run should produce on the order of: ~14 civs, 7000+
confirmed relations, 1800+ knowledge transmissions across collapse boundaries,
4000+ conflict skirmishes grouped into war campaigns, and a ~1000-line report.
These are calibration anchors, not hard requirements.
