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

Per-cell state is layered into channels — temperature, pressure,
charge, water depth, vapour, wind velocity (q, r), magnetic field
(q, r, z), upper-atmosphere temperature, plus per-cell substance
inventories.

## Real arithmetic

`sim_arith::Real` is Q32.32 fixed-point. Every channel, every
threshold, every accumulator. No `f64` outside `sim_arith`.
Transcendentals (`sin`, `cos`, `exp`, `ln`, `sqrt`, `powf`) live in
`sim_arith::transcendental` and use Taylor / table-driven
implementations that are byte-stable across platforms.

Apparatus clamp ladders (see [tech.md](tech.md#experiment-apparatus))
are tuned to keep heat / fluid / charge perturbations inside the
linear-response regime so apparatus presence doesn't violate the
planet's energy budget over long runs and doesn't overflow Q32.32
fit accumulators.

## Laws

`sim_core::build_laws(&planet)` returns a `Laws` struct with 12
fields. Each law is a per-tick `apply` over the grid. Operator
splitting: every law reads `prev` state and writes `next`,
producing a deterministic update independent of order *within*
the per-tick step.

| Law | What it does | Notes |
|-----|--------------|-------|
| `Heat` | Diffusion `α ∇²T` between cells. | Pair-flux upwind; α from substrate + biosphere. |
| `Radiation` | Stellar-driven radiative balance per row. | Per-row Stefan-Boltzmann `T_eq` + per-tick relaxation; eccentric-orbit modulation `1/(1−e·cos)²`; sub-solar-latitude shift via `axial_tilt × month-of-year`; diurnal modulation via `day_length_hours` (tidally-locked planets get permanent day/night asymmetry). Replaces "diffuse the seeded gradient forever". |
| `Wind` | Pressure-gradient-driven wind + heat advection. | Writes pressure + velocity fields; pair-flux upwind temperature transport. Vacuum-guarded on `Atmosphere::None`. |
| `Hydrology` | Evaporation → vapour transport → precipitation, with latent-heat coupling. | Surface water + Vapour + wind advection unified; per-cell pressure-aware boil via barometric `P(h) = P₀·exp(-h/H)` + substrate Clausius-Clapeyron; evaporation cools source, condensation warms receiver. Vacuum-guarded. |
| `Tides` | Lunar (multi-moon) gravitational tides on water depth. | Sub-lunar / antipodal bulges sweep `water_depth` via pair-flux conservation; multi-moon `cos(2θ_m)` superposition produces spring/neap interference. Clocked off `state.macro_step()`. |
| `Magnetism` | Planetary magnetic field as a vector. | `(B_q, B_r, B_z)` per cell. Latitude-dependent dipole + diurnal modulation; pole-to-equator field-magnitude ratio matches real dipoles. |
| `Lorentz` | Wind × magnetic field deflects velocity. | Per-cell `q · v × B` kick; reads true 3D `B`. Charge advection along the wind velocity field via pair-flux upwind transport — closes the M1b coupling triangle. Boris pusher (implicit-symplectic rotation) replaces explicit Euler so `|v|` is conserved regardless of step size. |
| `Coriolis` | Planet-rotation deflection of wind. | Mirror-symmetric N/S deflection via `Ω_z = sin(lat) · k · 24/day_length`. Boris-pusher rotation. Vacuum-guarded. |
| `VerticalConvection` | 1.5D vertical atmosphere stack. | `upper_temperature` field; per-cell lapse rate emerges from convective exchange + radiative cooling. |
| `Fluid` (`GravityFlow`) | Bulk gravity-driven mass flow. | Built from a `Mechanics` config; pair-flux conservation. |
| `EM` (`Electromagnetism`) | Per-cell charge field used by lightning / discharge templates. | Reads true magnetic-field magnitude, not a pre-Magnetism proxy. |
| `Chemistry` | Phase boundaries + substance reactions. | Substrate-aware thresholds via `Chemistry::for_planet(substrate_tag)`. |

## Operator splitting

Per civ-sim tick:

1. Snapshot `prev` state.
2. Apparatus cells (if any) overwrite their clamp channels with
   ladder values keyed by `tick % 4`.
3. Laws apply in fixed order, each reading `prev` and writing
   `next`.
4. State commits.
5. Pattern recognition reads the new state and the
   `prev`-state delta to fire templates that depend on temporal
   derivatives.

This produces deterministic per-tick updates with no order-of-law
sensitivity within the tick.

## Substrate-relative physics

Physics constants and thresholds derive from the planet's
substrate, not Earth defaults:

- **`Chemistry::for_planet(substrate_tag)`** — methane worlds
  freeze at 91 K, silicate at 1687 K, ammonia at 195 K. Aqueous
  routes through Clausius-Clapeyron for pressure-varying boil.
- **Per-substrate latent heats** — `L_FUSION_*` and
  `L_VAPORIZATION_*` constants per substrate; latent-heat coupling
  in Hydrology uses the right energy per phase change.
- **Per-substrate gas constants** — `R_SPECIFIC_*` per substrate;
  drives substrate-relative Clausius-Clapeyron.
- **Per-substrate cell thermal mass** — `SubstrateProperties.c_p`
  and `cell_thermal_mass_kg` lift the water-c_p hardcode out of
  the latent-heat plumbing.
- **Substrate-relative chemistry thresholds in `Chemistry`** — every
  substrate gets pressure-varying boil + correct phase-change
  energy.

## Vacuum guards

Wind, Hydrology, and Coriolis short-circuit on `Atmosphere::None`
worlds via `has_atmosphere`. Airless planets stop running fluid
dynamics (Lorentz remains active because charged-particle motion
doesn't require a fluid medium).

## Determinism

- Every law uses `sim_arith::Real` for arithmetic; no `f64`.
- Every randomness path seeds from a single root seed +
  field-tagged offsets.
- Iteration order through any structure is deterministic
  (`BTreeMap`, sorted `Vec`).
- Single-threaded integration; no parallel reductions.

Same `(seed, grid)` pair → byte-for-byte identical post-physics
state at every tick.
