# Physics

Deterministic physics on a hex grid. Operator-split time stepping;
real (SI) units throughout, threaded through `sim_arith::Real`
(Q32.32 fixed-point).

For deeper detail per crate:
[`sim/physics/README.md`](../sim/physics/README.md). For how
recognition templates read physics state see
[recognition.md](recognition.md). For how civs derive law
coefficients from observations see [discovery.md](discovery.md).

## Grid

Hex grid; default `36 × 30 = 1080` cells (configurable via
`--grid-width` / `--grid-height`). Pair-flux is the workhorse
spatial operator: every law that moves stuff between neighbouring
cells expresses the move as a signed pair flux that conservation-
checks to zero summed across both endpoints.

Per-cell state lives in `PhysicsState` ([`sim/physics/src/state.rs`](../sim/physics/src/state.rs)):
surface + subsurface temperature, pressure, charge, water depth,
wind velocity `(v_q, v_r, v_w)`, magnetic field `(B_q, B_r, B_z)`,
upper-atmosphere temperature, snow / sea-ice / cloud fractions,
cloud type byte, crustal remanence, per-cell local magnetic
shielding, plate id, crust thickness, and the per-cell substance
inventory (9 channels — see below).

## Real arithmetic

`sim_arith::Real` is Q32.32 fixed-point. Every channel, every
threshold, every accumulator. No `f64` outside `sim_arith`.
Transcendentals (`sin`, `cos`, `exp`, `ln`, `sqrt`, `pow`) live in
`sim_arith::transcendental` and use Taylor / table-driven
implementations that are byte-stable across platforms.

Apparatus clamp ladders (see [tech.md](tech.md#experiment-apparatus))
are tuned to keep heat / fluid / charge perturbations inside the
linear-response regime so apparatus presence doesn't violate the
planet's energy budget over long runs and doesn't overflow Q32.32
fit accumulators.

## Substances

Nine per-cell substance channels in fixed order
([`sim/physics/src/chemistry/substance.rs`](../sim/physics/src/chemistry/substance.rs)).
New substances append at the end so existing indices survive
schema bumps.

| idx | Substance | Notes |
|----:|-----------|-------|
| 0 | `Water` | Solvent in liquid phase (semantically: the substrate's solvent). |
| 1 | `Ice` | Frozen solvent. |
| 2 | `Vapour` | Gaseous solvent. C-C-coupled greenhouse channel. |
| 3 | `Fuel` | Biological combustible; renewable via `BiofuelRegrowth` (ash → fuel + oxidiser). Civ carrying-capacity proxy. |
| 4 | `Oxidiser` | Atmospheric O₂-equivalent. Replenished alongside `Fuel`. |
| 5 | `Ash` | Combustion residue. Drawn down by regrowth. |
| 6 | `Fossil` | Buried hydrocarbons. Non-renewable; combusts at +200 K over biofuel ignition. |
| 7 | `CO2` | Atmospheric carbon dioxide. Carbon-silicate loop moves it independently of the water cycle. |
| 8 | `Methane` | Atmospheric CH₄. Per-tick photolysis decay; greenhouse-active. |

The `Water` / `Ice` / `Vapour` triple is **substrate-relative**:
on a methane world it represents liquid / solid / gas methane
freezing at 91 K, on a silicate world molten / crystalline /
vaporised rock at 1687 K. `Chemistry::for_planet(P, T_ignite,
substrate_tag)` builds the per-substrate phase boundaries and
latent-heat coefficients.

## Laws

`sim_core::build_laws(&planet)` returns the full law roster. Each
law implements `Law::integrate(state, dt)`
([`sim/physics/src/laws.rs`](../sim/physics/src/laws.rs)) — one per-
tick pass over the grid. Operator splitting: laws run in a fixed
order so the per-tick update is bit-deterministic.

| Law | What it does | Notes |
|-----|--------------|-------|
| `Heat` | Diffusion `α ∇²T` between cells. | Pair-flux upwind; α from substrate + biosphere. |
| `Radiation` | Per-cell radiative balance + per-substance greenhouse. | Stefan-Boltzmann `T_eq` table indexed `[row][season]`; per-cell albedo rescale; per-cell H₂O/CO₂/CH₄ greenhouse with C-C coupling on vapour; cirrus vs stratus cloud forcing; CH₄ photolysis decay; eccentricity insolation swing; diurnal modulation; `LockingMode::Synchronous` great-circle day-night gradient for locked worlds. ([`sim/physics/src/radiation/mod.rs`](../sim/physics/src/radiation/mod.rs)) |
| `Wind` | Pressure-gradient-driven wind + heat advection. | Pair-flux upwind temperature transport. Vacuum-guarded on `Atmosphere::None`. |
| `Hydrology` | Evap → vapour advect → precipitate, with latent-heat coupling. | Pressure-aware boil via barometric `P(h) = P₀·exp(-h/H)` + Clausius-Clapeyron; quadratic sub-boil evap drive; per-cell vapour cap from quartic saturation curve. Vacuum-guarded. |
| `Tides` | Per-moon gravitational tides on water depth. | Multi-moon superposition; bulge swept by `state.macro_step()`; donor-limited pair-flux (bit-exact). |
| `Magnetism` | 3D vector magnetic field + Markov reversal chain. | `(B_q, B_r, B_z)` per cell; per-cell local shielding includes crustal remanence umbrella ([`magnetism.rs`](../sim/physics/src/magnetism.rs)). |
| `MagneticReversal` | Polarity Markov chain. | `Normal ↔ Reversing ↔ Reversed`; trial probability `1 / (250 000 yr × 12 mo)` per tick; envelope decays to 0.1× over the 12 000-tick reversal window. |
| `Lorentz` | Wind × magnetic field deflects charged motion. | Boris pusher (implicit-symplectic rotation) conserves `|v|` regardless of step size. Charge advection along wind via pair-flux upwind. |
| `Coriolis` | 3D planet-rotation deflection. | Full cross-product `F = -2 Ω × v` on `(v_q, v_r, v_w)`. Per-cell Ω components derived from latitude via `cos(φ)` / `sin(φ)`; `omega.0 = |Ω|·sin(tilt)` carries the axial-tilt component. ([`coriolis.rs`](../sim/physics/src/coriolis.rs)) |
| `Hadley` | Hadley / Ferrel / polar cells from angular-momentum conservation. | Cell count emerges from the Rhines-length closure tied to the Held-Hou Hadley edge; slow rotators get 1 cell, Earth-like 3, rapid rotators capped at `MAX_CELLS_PER_HEMISPHERE`. ([`hadley.rs`](../sim/physics/src/hadley.rs)) |
| `VerticalConvection` | 1.5D vertical atmosphere stack. | `upper_temperature` field; per-cell lapse rate emerges from convective exchange + radiative cooling. Writes `v_w` for Hadley + Coriolis coupling. |
| `Clouds` | Cloud fraction + type from saturation × updraft. | Cirrus over rising air / high elevation; stratus everywhere else. Updraft proxy = surface vs upper-layer ΔT. ([`clouds.rs`](../sim/physics/src/clouds.rs)) |
| `IceAlbedo` | Snow + sea-ice + cloud effective albedo. | Sigmoid freeze-line transition (5 K width) drives snowball bifurcation; max-channel composition. ([`albedo.rs`](../sim/physics/src/albedo.rs)) |
| `Tectonics` | Plate kinematics + slab pull + uplift + erosion. | Per-cell plate id + crust thickness; convergent / divergent boundary uplift; slope × precipitation fluvial erosion; ridge-cooling depth; F6 substrate-aware subduction. ([`tectonics/mod.rs`](../sim/physics/src/tectonics/mod.rs)) |
| `apply_isostasy` | Airy isostatic adjustment. | `h_surface = h_base + (ρ_m/ρ_c − 1) × thickness`; continental factor ≈ 0.22, oceanic ≈ 0.10. ([`isostasy.rs`](../sim/physics/src/isostasy.rs)) |
| `Volcanism` | Boundary + hot-spot CO₂ / H₂O outgassing. | Deterministic per-cell rate at plate boundaries; SplitMix64 hot-spot trial elsewhere at 10⁻⁵ per tick. ([`volcanism.rs`](../sim/physics/src/volcanism.rs)) |
| `Weathering` | Arrhenius CO₂ drawdown by silicate weathering. | `base × exp(Ea/R × (1/T_ref − 1/T)) × (water + vapour)`; closes the carbon-silicate thermostat against `Volcanism`. ([`weathering.rs`](../sim/physics/src/weathering.rs)) |
| `Fluid` (`GravityFlow`) | Bulk gravity-driven mass flow. | Built from a `Mechanics` config; pair-flux conservation. |
| `EM` (`Electromagnetism`) | Per-cell charge field used by lightning / discharge templates. | Reads true magnetic-field magnitude, not a pre-Magnetism proxy. |
| `ResonanceField` | Speculative per-cell resonance / attention field `Ψ`, relaxing toward an EM-driven equilibrium + pair-flux neighbour diffusion. | Additive substrate for the field/resonance civilizational lever — no legacy law reads it, so installing it leaves every existing channel bit-identical. Couplings derive from crust piezoelectric fraction + magnetosphere + atmosphere density. ([`resonance.rs`](../sim/physics/src/resonance.rs); see [archetype.md](archetype.md)) |
| `Chemistry` | Phase boundaries + combustion + biofuel regrowth. | Substrate-aware thresholds via `Chemistry::for_planet`; per-substrate Arrhenius prefactor (Aqueous 1, Ammoniacal 0.4, Hydrocarbon 0.05, Silicate 5). |
| `apply_tidal_heating` | Per-moon eccentric-orbit dissipation. | `H = (21/2) k₂/Q × R⁵ n⁵ e² / G`; per-substrate dimensional multiplier (F6) + Laplace resonance × 8 boost for Ganymede-class radii. ([`tidal_heating/mod.rs`](../sim/physics/src/tidal_heating/mod.rs)) |
| `subsurface_conduction_step` | Two-reservoir surface↔subsurface relaxation. | Routes Aqueous / Hydrocarbon 90 %, Ammoniacal 60 %, Silicate 30 %, default 80 % of tidal heat into the subsurface. |
| `atmospheric_escape_step` | Four-channel atmospheric loss. | Jeans / hydrodynamic / photochemical / ion (see below). Per-cell shielding reads local field; iterates `ATMOSPHERIC_SUBSTANCES` light-first. ([`atmospheric_escape/mod.rs`](../sim/physics/src/atmospheric_escape/mod.rs)) |

## Operator splitting

Per civ-sim tick (= one sim-month) the orchestrator
([`orchestration.rs`](../sim/physics/src/orchestration.rs)) walks
`macro_steps_per_step` macro-steps (default 30, one per sim-day).
Within each macro-step, laws apply in fixed coupling order:

1. `Tides` (lunar bulge displacement, donor-limited pair-flux).
2. `GravityFlow` (post-tide water settles).
3. `Heat` (diffusion).
4. `Radiation` (per-cell T_eq relaxation; reads post-heat T).
5. `Wind`, `VerticalConvection`, `Hadley`, `Coriolis`, `Lorentz`
   (atmosphere dynamics).
6. `Hydrology` (evap / advect / precip; closes water cycle).
7. `Magnetism` + `MagneticReversal` (polarity envelope).
8. `IceAlbedo`, `Clouds` (read post-Hydrology vapour state).
9. `Tectonics` + `apply_isostasy` (slow geology, per-tick).
10. `Volcanism`, `Weathering` (close carbon-silicate thermostat).
11. `apply_tidal_heating` + `subsurface_conduction_step`.
12. `Chemistry` (every `chemistry_macros_per_substep`; sees
    post-physics state).
13. `EM` (per macro-step).
14. `atmospheric_escape_step` (last so it reflects post-reaction
    composition).

Order is fixed and bit-deterministic; every law reads the prior
law's `next` state.

## Substrate-relative physics

Physics constants derive from the planet's substrate, not Earth
defaults
([`sim/physics/src/chemistry/substrate.rs`](../sim/physics/src/chemistry/substrate.rs)):

- `Chemistry::for_planet(substrate_tag)` — methane freezes at
  91 K, silicate at 1687 K, ammonia at 195 K. Aqueous routes
  through Clausius-Clapeyron for pressure-varying boil; every
  substrate now does so via `substrate_boiling_point_k`.
- Per-substrate latent heats (`L_FUSION_*`, `L_VAPORIZATION_*`).
- Per-substrate gas constants (`R_SPECIFIC_*`) drive substrate-
  relative C-C.
- Per-substrate `c_p` and `cell_thermal_mass_kg` lift the
  water-c_p hardcode out of latent-heat plumbing.
- Substrate-coupled Arrhenius prefactor scales combustion +
  biofuel-regrowth rates (cold solvents slow, hot solvents fast).
- Substrate-derived bulk density on `Planet::density`
  (silicate ≈ 5, aqueous ≈ 1, ammoniacal ≈ 0.7, hydrocarbon ≈ 0.5
  g/cm³).

## Per-substance greenhouse + Clausius-Clapeyron

`Radiation` ([`radiation/greenhouse.rs`](../sim/physics/src/radiation/greenhouse.rs))
adds a per-cell dynamic greenhouse term on top of the per-row
baseline:

```text
greenhouse[cell] = vapour[cell] × H2O_K
                 + co2[cell]    × CO2_K
                 + ch4[cell]    × CH4_K
                 + cloud_forcing(cloud_type, cloud_fraction)
```

H₂O is the C-C-coupled feedback channel: the saturation cap
([`hydrology::saturation_vapour_cap`](../sim/physics/src/hydrology.rs))
grows quartically in `T/T_ref`, so warming lifts the vapour
ceiling → more vapour → more greenhouse → more warming. Above a
Komabayashi-Ingersoll-like threshold the loop diverges and the
cell slides into a Venus-style runaway. The cap scales with
surface pressure via `greenhouse_cap_scaled` so a Venus-equivalent
seed plateaus in the 700-770 K literature band.

CO₂ is linear (long-lived). CH₄ decays by `~0.999` per tick
(photolysis, real lifetime ≈ 10 years).

Cirrus contributes more greenhouse forcing than stratus (high-
altitude ice vs low liquid water). `Radiation::with_lapse_inputs`
threads gravity + cirrus altitude so a high-gravity world gets a
steeper dry adiabatic lapse → cooler cirrus tops → stronger
longwave trap.

## Hadley / Ferrel / polar cells

`Hadley` ([`hadley.rs`](../sim/physics/src/hadley.rs)) derives the
zonal cell count from the Rhines-length closure tied to the
Held-Hou Hadley edge:

```text
L_rhines = π · sqrt(U / β)
β        = 2Ω / R          (equatorial)
U        = ΩR · sin²(lat_h)
N_cells  ≈ √2 / sin(lat_h)
```

Slow rotators (`lat_h → π/2`) get one pole-to-pole cell per
hemisphere; Earth-like (24 h, 6371 km) gets 3 (Hadley + Ferrel +
polar); rapid rotators saturate at `MAX_CELLS_PER_HEMISPHERE`.

`apply_hadley_circulation` then applies angular-momentum
conservation on poleward-moving parcels: `M = Ω · r · cos²(lat) +
u · cos(lat)` is conserved, so a parcel leaving the equator with
`u = 0` arrives at the cell boundary as a westerly jet. Shear
instability caps each cell's poleward reach; that's where the
next cell starts.

## Tidal heating

`moon_tidal_heat_rate` ([`tidal_heating/formula.rs`](../sim/physics/src/tidal_heating/formula.rs))
evaluates the textbook eccentric-tide formula

```text
H = (21/2) × (k₂/Q) × R⁵ × n⁵ × e² / G
```

in Q32.32-safe natural units (Earth radii, radians per macro-
step), absorbing SI constants into `tidal_dimensional_calibration`.
`MoonHeating::rocky` sets `k₂/Q = 0.003` (Earth, Io); `icy` sets
`0.0003` (Europa, Enceladus).

Per-substrate dimensional multiplier (F6) corrects the Io-anchored
calibration for icy ocean moons: Aqueous / Hydrocarbon × 25,
Ammoniacal / Silicate × 1. `laplace_resonance_multiplier` adds an
× 8 boost for Ganymede-class radii (0.39-0.45 Earth radii) on
Aqueous / Hydrocarbon / Ammoniacal moons — captures the
Io-Europa-Ganymede pumping that sustains non-zero eccentricity
against tidal damping.

Heat distribution (`distribute_heat_to_cells`) splits between
surface and subsurface reservoirs by substrate: Aqueous /
Hydrocarbon route 90 % into the subsurface (Europa, Titan);
Silicate 30 % (Io's surface volcanism); Ammoniacal 60 %; default
80 %. `subsurface_conduction_step` is a two-reservoir relaxation
kernel that lets surface and subsurface temperatures exchange.

Calibration anchors (test-pinned):
- Io: 50-200 TW.
- Europa (with `Aqueous` substrate): 5-20 TW.
- Enceladus: 1-100 GW.
- Ganymede (Laplace-pumped, Aqueous): 0.5-5 TW.
- Callisto (non-resonant): ≈ 0 GW.

Cumulative tidal heat matches cumulative orbital-energy loss
(P3.8 invariant): `synchronous_eccentricity_damping_rate` shares
the same calibration constant as `H`, so
`H = -dE_orbit/dt` to 1 % over a 100-tick damping window.

## Atmospheric escape

Four channels per tick per substance
([`atmospheric_escape/mod.rs`](../sim/physics/src/atmospheric_escape/mod.rs)):

1. **Jeans (thermal)** — rate ∝ `exp(-λ)` with
   `λ = m × v_esc² / (2 × k_B × T_exo)`. Mass-explicit so heavy
   species (CO₂, 44 amu) retain exponentially better than light
   ones (H, 1 amu) — Earth H/He retention ratio > 10³.
   `exobase_temperature(T_surface, EUV)` lifts the surface T to
   the exobase before evaluating λ (Earth ≈ 3.5× surface, Mars
   ≈ 2.0×).
2. **Hydrodynamic blow-off** — `EUV × thermal_factor × weight`.
   Fires meaningfully only for young hot atmospheres; stripped
   Mars's primordial atmosphere.
3. **Photochemical** — `UV × ozone_shield × weight`. Dominant
   Mars-today channel. `ozone_shield = 1/(1 + B_local)` proxies
   the magnetosphere-protected ozone layer.
4. **Ion escape** — `(1/(1 + B_local)) × weight`. Strong dipole
   suppresses (Earth); none enables (Mars). Reads per-cell local
   shielding so crustal-remanence umbrellas (Mars southern
   highlands) get geographically structured protection.

A shared `MAVEN_CALIBRATION_SCALE` (1/10 000) shrinks absolute
per-channel rates into the MAVEN-observed kg/s range (Mars ion ≈
44 kg/s, photochem ≈ 3 kg/s, both within one OOM of measurement).

Lighter substances iterate first via `ATMOSPHERIC_SUBSTANCES` so
composition shifts emerge naturally: methane / vapour deplete
before oxidiser before CO₂.

## Magnetism + reversals + crustal remanence

`Magnetism::init_field` writes a latitude-dependent dipole pattern
per cell at planet build. `Magnetism::init_local_field` lays a
SplitMix64-driven per-cell remanence on top of the dipole,
gated on `crust_thickness / REMANENCE_REF_THICKNESS_KM` — thick
continental crust holds more frozen-in magnetisation, so highland
cells get a stronger local shielding signal than thin oceanic
ones. Local field is clamped to `[0, 1.5]`.

`MagneticReversal` is a Markov chain over
`Normal → Reversing → Reversed`. Trial probability `1 /
(250 000 yr × 12 mo)` per tick (Earth-like geomagnetic cadence);
when a trial fires the state holds `Reversing` for 12 000 ticks
(~1000 years) during which `dipole_strength` decays linearly to
0.1 at midpoint and ramps back. The inverse-coupled
`cosmic_ray_ground_flux ≈ 1 / dipole_strength` peaks at ~5× during
the deepest part of a reversal.

## Ice-albedo bifurcation

`IceAlbedo` ([`albedo.rs`](../sim/physics/src/albedo.rs)) writes
three per-cell channels: `snow_fraction`, `sea_ice_fraction`,
`cloud_fraction`. Per-cell effective albedo is the maximum across
base surface + each channel (multiplicative `(1 - snow)`
suppression on sea ice).

The freeze-line transition is a 5 K-wide sigmoid centred on the
substrate freeze point:

```text
freeze_drive(T) = sigmoid_real((T_freeze − T) / 5)
```

Narrow enough to amplify modest perturbations into runaway ice
growth (the positive feedback that drives snowball bifurcation);
smooth enough to keep the `Radiation` relaxation differentiable
at the boundary.

## Plate tectonics

`Tectonics` ([`tectonics/mod.rs`](../sim/physics/src/tectonics/mod.rs))
sequences per tick: slab-pull velocity update → boundary uplift /
divergence → fluvial erosion → crust age + ridge cooling →
substrate-aware subduction → isostasy.

Per-cell state: `plate_id`, `crust_thickness`, `crust_age`,
`subducted_mass`, `h_base`. `MIN_PLATES / MAX_PLATES` bracket the
worldgen sampler; continental crust starts at ≈ 35 km thickness
(`CONTINENTAL_THICKNESS_KM`), oceanic at ≈ 7 km
(`OCEANIC_THICKNESS_KM`).

Slab pull derives plate velocity from per-boundary slab-density
contrast (oceanic-continental vs oceanic-oceanic), capped at
`max_plate_velocity`. Subduction transfers mass from the thinner
crust into the thicker plate per tick under F6 substrate scaling.
Erosion is `slope × precipitation × erodibility`; uplift is
boundary-driven.

## Cloud microphysics

`Clouds` ([`clouds.rs`](../sim/physics/src/clouds.rs)) authors
`cloud_fraction` and `cloud_type` per cell:

- `cloud_fraction` relaxes toward a target derived from
  vapour-saturation ratio × updraft proxy (surface T − upper
  layer T). Above the `supersaturation_threshold` (≈ 0.9 of cap)
  cells with rising air grow cover; below, cover decays slowly.
- `cloud_type` tips into `Cirrus` over high-elevation cells or
  strong updrafts; otherwise `Stratus`. Cirrus contributes lower
  albedo (~0.2) and stronger greenhouse forcing; stratus the
  reverse (~0.5 albedo, weak greenhouse).

Both fields feed back into `Radiation` (greenhouse weight by
type) and `IceAlbedo` (per-cell effective albedo).

## Mass-radius-density coupling

Sprint 5 Item 21 separated mass and radius from a pre-sampled
`gravity` scalar. `Planet::gravity()`
([`sim/world/src/planet.rs`](../sim/world/src/planet.rs)) computes
`g = EARTH_G × M / R²` (Earth units). `Planet::escape_velocity()`
returns `sqrt(2 × g × R)` in km/s. `Planet::density(substrate)`
returns the substrate's bulk-density anchor — silicate ≈ 5,
aqueous ≈ 1, ammoniacal ≈ 0.7, hydrocarbon ≈ 0.5 g/cm³.

Downstream coupling: `Tides::for_gravity` and `Wind::for_gravity`
read the derived gravity; `PlanetEscapeParams` reads escape
velocity; tidal Love numbers consume substrate density.

## 3D Coriolis with axial tilt

`Coriolis` ([`coriolis.rs`](../sim/physics/src/coriolis.rs))
applies the full cross-product `F = -2 Ω × v` on the 3D velocity
vector. Per-cell local-frame components derive from a single `Ω`
vector:

```text
Ω_north = Ω · cos(φ)    (max at equator, zero at poles)
Ω_up    = Ω · sin(φ)    (zero at equator, max at poles)
```

The axial tilt decomposes the inertial-frame `|Ω|` across world
(x, z) axes: `omega.0 = |Ω| · sin(tilt)`, `omega.2 = |Ω| ·
cos(tilt)`. Boris-pusher rotation conserves `|v|` regardless of
step size — the same scheme used in `Lorentz`.

## Vacuum guards

`Wind`, `Hydrology`, `Coriolis`, and `Hadley` short-circuit on
`Atmosphere::None` worlds via `has_atmosphere`. `Lorentz` stays
active because charged-particle motion doesn't require a fluid
medium.

## Conservation invariants

The orchestrator ([`orchestration.rs`](../sim/physics/src/orchestration.rs))
snapshots three conservation totals before each kernel and asserts
per-tick + cumulative drift under tight bounds:

- `Tides`: `Σ water_depth` (bit-exact under donor-limited pair-flux).
- `Hydrology`: `Σ water_depth + Σ vapour` (evap + advect +
  condense only redistribute between channels).
- `Chemistry`: `Σ over all substances` (combustion 1+1→2,
  regrowth 2→1+1, phase transitions all stoichiometrically
  conservative).

Per-tick `debug_assert!` trips on drift ≥ 1e-6 in a single
integrate. `OrchestratorState` carries signed cumulative drifts
across ticks and asserts the running total stays under
`1e-6` (`1e-4` for hydrology, which has documented clamps that
LSB-truncate under extreme substrate coefficients). Weathering's
intentional CO₂ removal and Volcanism's CO₂ / H₂O addition are
tracked separately so they don't register as leaks.

Zero-cost in release builds (asserts are `#[cfg(debug_assertions)]`-
gated); budget ~30 s of overhead on the 16k-tick canary in debug.

## Determinism

- Every law uses `sim_arith::Real` arithmetic; no `f64`.
- Every randomness path seeds from a single root seed +
  field-tagged offsets (`REVERSAL_SALT`, `REMANENCE_SALT`,
  `VOLCANISM_SALT`, `PLATE_SALT`).
- Iteration order through any structure is deterministic
  (`BTreeMap`, sorted `Vec`, canonical `(r, q)` cell order).
- Single-threaded integration; no parallel reductions.

Same `(seed, grid)` pair → byte-for-byte identical post-physics
state at every tick.
