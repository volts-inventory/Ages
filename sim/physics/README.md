# sim/physics

Deterministic physics engine: laws, spatial grid, time-stepping,
state. Coefficients are seeded per-planet — structural shapes fixed,
parametric values vary. Phenomena emerge from law combinations on
planet-specific conditions; they are **not** authored catalogue
entries.

## Status

- **M1a shipped**: mechanics + gravity + fluids + heat. Pair-flux
  bit-exact mass / heat conservation.
- **M1b shipped**: simplified EM (charge diffusion + lightning) and
  6-substance chemistry (water/ice/vapour, fuel/oxidiser/ash) with
  latent-heat coupling.
- **SI landed**: temperatures in K, pressures in Pa, gravity in
  m/s², energies in J. `Chemistry::for_planet` derives water's
  boiling point from `surface_pressure_pa` via Clausius-Clapeyron
  using `arith::transcendental::ln`. Latent heats stored as named
  J/kg constants; per-step ΔT uses an effective cell thermal mass.

## Encoded laws

Each law family implements `Law::integrate(state, dt)` and is invoked
by the operator-splitting orchestrator at fixed sub-step ratios.

- **Mechanics + gravity** (`mechanics.rs`) — Newtonian motion;
  gravity exponent fixed at 2 (structural). Per-planet `gravity`
  taken directly from `Planet.gravity` (m/s²).
- **Fluid dynamics** (`fluid.rs`) — gravity-driven mass redistribution
  with bit-exact pair-flux. Stateful velocity, wave dynamics, and
  heat-fluid advection coupling deferred to M1.5/M2.
- **Heat conduction** (`heat.rs`) — Fourier-law diffusion via
  pair-flux. `alpha` derived from atmosphere class.
- **Chemistry** (`chemistry.rs`) — 6 named substances; phase
  transitions (water/ice/vapour) carry latent heat; combustion
  fires when `T > ignition` with fuel + oxidiser present. Mass
  conserved bit-exactly.
- **Electromagnetism** (`em.rs`) — per-cell charge with pair-flux
  diffusion; lightning fires when `|charge| > discharge_threshold`,
  releasing heat. Charge-fluid advection and magnetic-field dynamics
  deferred (waits on stateful fluid velocity).

## Spatial grid (`grid.rs`)

Hex lattice over the planet using axial `(q, r)` coordinates.
Default ~2 500 cells per Earth-sized planet; configurable 500–10 000.
Each cell carries: temperature, pressure, fluid velocity, charge,
named-substance densities, biological-stock densities, illumination.
Six adjacency edges per cell drive flux. Hex chosen over square for
fluid-flow isotropy.

Goldberg-sphere wrap-around and adaptive refinement deferred as
escape hatches.

## Time-stepping (`laws.rs` + `state.rs`)

Operator splitting from M1a:

- **Macro-step** = one sim-day; civ-sim tick = one sim-year ≈ 365
  macro-steps.
- **Defaults** (configurable via `OrchestrationConfig`):
  - Fluid: ~50 sub-steps per macro-step (CFL-bound).
  - Heat: 1 sub-step per macro-step.
  - Chemistry: 1 sub-step per ~3 macro-steps.
  - EM: 1 sub-step per macro-step.
- **Splitting**: Lie (sequential, first-order). Strang second-order
  is the escape-hatch upgrade if accuracy artefacts surface.
- **Coupling order within a macro-step**:
  1. Fluid sub-steps (gravity each sub-step)
  2. Heat (sees post-fluid velocity for advection)
  3. Chemistry (post-heat state)
  4. EM (post-fluid charge)
- **Determinism**: fixed sub-step counts, fixed coupling order,
  fixed dt per family. No state-dependent branching.

## Per-planet law coefficients

`build_laws(&Planet)` in `sim/core` constructs all four law structs
from sampled planet properties:

- Gravity → `Mechanics`, `GravityFlow`.
- Atmosphere class → `HeatConduction.alpha`, EM `conductivity`,
  `Chemistry.ignition_threshold` (in K, range 500–1 000 000).
- Magnetosphere → EM `discharge_threshold`.
- Surface pressure (Pa) → water boiling point via Clausius-Clapeyron
  in `chemistry::water_boiling_point_k`.

Different seeds → different planets → different coefficients →
different physics outcomes. Structural shapes (which laws exist,
their equation forms) stay fixed.

## Cited by

[docs/physics.md](../../docs/physics.md) (scope, grid,
time-stepping, seeded variation, SI cascade).
