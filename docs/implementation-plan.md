# Implementation plan: expert-review roadmap

Companion doc to `docs/expert-review-roadmap.md`. Each of the 18
backlog items gets a concrete implementation sketch: files to
touch, data-structure changes, algorithm, test plan, risk
register, effort. Sequenced across 5 sprints per the roadmap.

This is the **draft plan**; xenobiologist + astrophysicist
reviewers see it before any code lands so completeness gaps
surface up-front rather than mid-sprint.

## Methodology

- One sprint at a time, in roadmap order. No skipping ahead.
- Each item has an acceptance test that lands with the
  implementation. No item is "done" until its test exists +
  passes + appears in workspace lib runs.
- Cross-cutting concerns (determinism, save/load, migration)
  documented per-sprint, not as afterthoughts.
- Expert review after each sprint. Reviewer's reject blocks
  the sprint from closing.
- Acceptance against the per-expert sign-off checklists in
  `docs/expert-review-roadmap.md`.

## Cross-cutting invariants (apply to all sprints)

1. **Q32.32 determinism preserved.** Every new RNG draw must use
   the existing per-seed `ChaCha20Rng`. No new RNG seeds beyond
   what's already in the codebase. Any new transcendental call
   uses `sim_arith::transcendental::{cos, exp, ln, sqrt}` (no
   `sin`, which the codebase doesn't have). Any new fixed-point
   division must check for divide-by-zero AND fixed-point
   overflow via `saturating_mul` / `to_real_nonneg`.

2. **Pair-flux conservation by default.** New transport laws use
   the pair-flux pattern (write `+flux` to one cell, `-flux` to
   the other) so mass / energy is bit-exact conserved. Any
   exception is documented + asserted against in a regression
   test.

3. **Operator-split ordering preserved.** New laws slot into
   `physics/src/orchestration.rs::integrate_civ_step` at a
   documented location with rationale (e.g., before / after
   which kernel and why). The conservation-invariant asserts
   wrap each new kernel.

4. **Back-compat for hand-built test fixtures.** New struct
   fields default to `Real::ZERO` or `Default::default()` so
   existing `PopulationBiology { ... }` literal constructions
   compile without modification. Production samplers populate
   the new fields with real values.

5. **No new dependencies** (no new crates, no `extern crate`
   additions). Everything stays inside the existing
   `sim_arith`, `sim_physics`, `sim_world`, `sim_species`,
   `sim_civ`, `sim_core`, `sim_recognition`, `sim_population`
   layer.

6. **Per-sprint expert review.** After each sprint's PRs land
   on `main`, the relevant expert (xeno / astro / both) runs a
   pass on the diff. Their accept/reject blocks the next
   sprint.

## Sprint 1: Numerical hygiene (Tier 1)

5 items, all S/M effort. Cleans up calibration debt from prior
PRs. Independent of each other — can ship in parallel as 5
small PRs or one rollup. Recommend parallel for review clarity.

### Item 1: Reproductive_success curve shape

**Why**: PR #30's linear `reproductive_success = 0.005 + r×0.095`
combined with parabolic `clutch_size = 1 + r²×499` produces a
mid-axis hump (~6,300 lifetime offspring at r=0.5). R-end
undershoots salmon (~120 vs ~5,000).

**Files**:
- `sim/species/src/sampling.rs:938-947` (derivation)
- `sim/population/src/lib.rs` (existing test +
  add 2 new realism tests)

**Approach**:
- Replace linear with quadratic: `success = 0.005 × (1-r)² + 0.10
  × r²`. At r=0.5, success = 0.005 × 0.25 + 0.10 × 0.25 = 0.026,
  which keeps the mid-axis under control (clutch=125 × events=16
  × success=0.026 = 52/mo lifetime offspring × 60 fertile_months
  = 3,120 lifetime — still too many for a true mid).
- Better shape: **also raise `clutch_size` cap at r=1**. New
  `clutch_size = 1 + r² × 4999` (was r² × 499). Cap at 5,000
  for true broadcast spawners. Salmon = 5,000 eggs ✓.
- At r=1: clutch=5000, events=2, success=0.10 →
  5000×2×0.10/1.2 = 833/mo, fertile=1.2 → 1,000 lifetime
  offspring. Still 5× under salmon but in the right order.
  Acceptable.
- At r=0.5: clutch=1,250, events=16, success=0.026 →
  1250×16×0.026/60 = 8.7/mo, fertile=60 → 520 lifetime offspring.
  Reasonable mid-K-ish "rat-equivalent."

**Test plan**:
- Existing `k_strategist_birth_rate_realistic_with_reproductive_success`
  passes unchanged.
- New `mid_strategist_birth_rate_realistic`: assert mid-axis
  (r=0.5) species lifetime offspring in [50, 1,000].
- New `r_strategist_birth_rate_in_broadcast_spawner_range`:
  assert r=1 species lifetime offspring in [500, 10,000].

**Risk**: existing seeds that produced specific clutch values
will shift dynamics. Acceptable — calibration honesty trumps
seed-stability across calibration changes.

**Effort**: S — 4 hours.

### Item 2: Two-pass donor-limited tide flux

**Why**: PR #23's single-pass donor cap is order-dependent.
Bulge symmetry breaks under aggressive `tide_k`. P6 regression
test exists; underlying fix doesn't.

**Files**:
- `sim/physics/src/tides.rs:236-261` (the pair-flux loop)

**Approach**:
```
Pass 1 (compute desired):
  for each pair (i, j) with i < j:
    desired_flux[i,j] = tide_k × dt × (Φ[i] - Φ[j])
    accumulate desired_outflow[i] += max(0, desired_flux[i,j])
    accumulate desired_outflow[j] += max(0, -desired_flux[i,j])

Pass 2 (scale by donor availability):
  for each cell i:
    if desired_outflow[i] > prev_w[i]:
      scale[i] = prev_w[i] / desired_outflow[i]
    else:
      scale[i] = 1.0

Pass 3 (apply scaled fluxes):
  for each pair (i, j) with i < j:
    raw = desired_flux[i,j]
    actual = if raw > 0:
      raw × scale[j]   // j is donor (Φ[j] < Φ[i])
    else:
      raw × scale[i]   // i is donor
    next_w[i] += actual
    next_w[j] -= actual
```

Mass-conservative (each `actual` written symmetrically) and
order-independent (scale computed against `prev_w`).

**Test plan**:
- Existing `tide_redistribution_donor_limited` passes (calibrate
  to triple-check bit-exact conservation under tide_k=0.5).
- Existing `tide_bulge_preserves_longitudinal_symmetry` passes
  at higher tide_k (raise the test's tide_k from 0.02 to 0.5
  and re-run).
- New `tide_redistribution_order_independent`: run the same
  setup twice with different cell-ID iteration orders, assert
  identical final `water_depth`.

**Risk**: Triple-pass costs more compute than single-pass. For
a 1,080-cell grid, ~3× the cells-iteration. Tide cadence is
infrequent (once per macro-step), so total cost increase is
modest. Acceptable.

**Effort**: M — 8 hours.

### Item 3: Adaptive dt sub-stepping in wind

**Why**: PR #30's velocity clamp at `wind.rs:215-233` distorts
physics. Real CFL stability requires subdividing dt, not
clamping `v`.

**Files**:
- `sim/physics/src/wind.rs:200-220` (the integrate loop)

**Approach**:
```
1. Compute velocity from pressure gradient (existing step).
2. Compute CFL number: max(|v_along × advect_k × dt|) over all
   pairs.
3. If CFL > 0.5:
     n_substeps = ceil(CFL / 0.5)
     dt_sub = dt / n_substeps
     for _ in 0..n_substeps:
         re-derive pressure (it depends on T which sub-steps
           don't change)
         apply acceleration + friction with dt_sub
         apply advection with dt_sub
   else:
     proceed normally with dt
```

Pressure can be derived once (T doesn't change within Wind's
own step) so the sub-step loop is cheap.

Remove the post-friction velocity clamp.

**Test plan**:
- Existing `wind_advection_conserves_energy_under_varying_column_mass`
  passes unchanged.
- Existing `wind_density_scales_all_three_coefficients` passes.
- New `wind_subdivides_dt_under_high_pressure_gradient`: build
  a state with extreme equator-pole temperature gradient on a
  thin atmosphere, drive wind once; assert sub-step count > 1
  and per-cell wind speeds aren't clamped (i.e., grow naturally
  under sub-stepping).

**Risk**: Sub-stepping in extreme cases could explode (10×
sub-steps + 1× per-macro-step = 10× compute). Cap n_substeps
at 8; if CFL would require more, log a warning + clamp to 8.
Indicates a planetary configuration that just shouldn't be
supported.

**Effort**: M — 6 hours.

### Item 4: Saturation-curve vapour cap floor

**Why**: PR #30's flat 10,000 floor at `hydrology.rs:425-438`
ignores cell saturation pressure. Hot ocean coast can hold
much more vapour than cold desert.

**Files**:
- `sim/physics/src/hydrology.rs:425-438` (the cap loop)

**Approach**:
- Add `saturation_capacity(T, substrate, scale_height_m) ->
  Real` derived from Clausius-Clapeyron: `cap = base ×
  exp((T - T_ref) / RT_ref)` clamped to a max ceiling.
- Per-cell `cap[i] = max(water_depth[i] × 100,
  saturation_capacity(T[i], substrate, scale_h))`.
- Drop the flat 10,000 floor.

**Test plan**:
- Existing `hydrology_vapour_cap_clamps_pathological_overload`
  still triggers (calibrate cap to match prior test
  expectations).
- Existing `hydrology_cycle_reaches_steady_state` passes.
- New `vapour_cap_scales_with_temperature`: build two cells
  same water_depth, different T (e.g., 200K vs 350K); assert
  hot cell has higher cap than cold cell.

**Risk**: A bad calibration could either let vapour run away
(if cap is too generous on hot cells) or starve evaporation (if
cap is too tight on cold cells). Calibrate against Earth
mid-latitude oceans: 300K cell should have cap ≈ 50,000
units (broadly comparable to current 10,000 flat floor for
near-mean cells).

**Effort**: S — 4 hours.

### Item 5: Cumulative hydrology mass-conservation assert

**Why**: PR #30's drift tolerance is per-tick. Doesn't bound
long-run cumulative drift from steady-state clamp firings.

**Files**:
- `sim/physics/src/orchestration.rs` (`Orchestrator` struct +
  `step` method)
- Add a run-end check (sim/core probably invokes a finalize
  hook).

**Approach**:
- `Orchestrator` gains `cumulative_hydrology_drift: Real` and
  `starting_hydrology_total: Option<Real>` fields.
- On first hydrology call, store the starting total.
- After each hydrology call (in debug builds), accumulate
  `abs(post - pre)` into `cumulative_hydrology_drift`.
- Provide a `cumulative_hydrology_drift_fraction() -> Real`
  accessor: `cumulative / starting`.
- At run end (sim_core finalize), debug-assert `drift_fraction
  < 0.05` (5% growth-bounded ceiling).

**Test plan**:
- New `cumulative_hydrology_drift_bounded_over_16k_ticks`: run
  16k ticks on the seed-100 canary, assert drift fraction < 5%.
- Earth-like seed: assert drift fraction < 0.01% (extremely
  tight; clamp firings should be near-zero under earth-like
  coefficients).

**Risk**: Long-runtime tests can run > 1 min and pollute CI.
Already an existing pattern (`#[ignore = "slow"]` markers on
16k-tick tests); reuse it.

**Effort**: S — 4 hours.

### Sprint 1 total

~26 hours of implementation. Sprint review: xenobiologist
not affected (numerical only); astrophysicist approves the
adaptive-dt + cap reshape + tide order-independence.

## Sprint 2: Xenobiology foundation

Two big items in parallel: **#6 multi-species ecosystems** (the
biggest xeno gap) and **#8 substrate-coupled solvent semantics**.
Independent files. Different reviewers but both xeno.

### Item 6: Multi-species ecosystems

**Why**: The biggest xeno gap. Sim has one species per planet —
no ecology, no symbiosis, no producer/consumer/decomposer,
no niche partitioning. Ecological collapse not representable.

**Files**:
- `sim/species/src/lib.rs` + new `sim/species/src/ecosystem.rs`
- `sim/world/src/planet.rs` (single `Species` → `Vec<Species>`)
- `sim/civ/src/lib.rs` (civ → species lookup needs index)
- `sim/core/src/lib.rs` (per-planet ecosystem step)
- `sim/report/src/digest/build.rs` + viewport (surface multi-species
  state in output)

**Data structures**:
```rust
// In sim_species
pub struct EcosystemRole {
    Producer,         // photosynthetic / chemoautotroph
    Consumer,         // eats other species
    Decomposer,       // breaks down dead biomass
    Mutualist,        // pairs with another species
    Parasite,         // consumes host without killing
}

pub struct InteractionMatrix {
    // matrix[a, b] = strength of a's effect on b
    // positive = a benefits b
    // negative = a harms b
    pairs: BTreeMap<(SpeciesId, SpeciesId), Real>,
}

// On Planet
pub struct Planet {
    // ... existing fields ...
    pub species_registry: Vec<Species>,
    pub interaction_matrix: InteractionMatrix,
    pub civ_bearing_species_id: SpeciesId,  // the index in
        // species_registry that civs are descended from
}

// On Species
pub struct Species {
    // ... existing fields ...
    pub role: EcosystemRole,
    pub population_pool: Pop,  // species-level biomass; civ
        // populations are a subset of the civ-bearing species
}
```

**Step function**:
- Per-tick `ecosystem_step` runs after `step_population_per_cell`
  and before catastrophe checks.
- For each species, compute per-tick `delta = base_growth +
  Σ_others (interaction_matrix[other, self] × other.pop_pool)`.
- Apply per-tick `delta`. Cap at planet `ecological_capacity`.
- If producer-tier pool drops below 10% of starting, fire
  `EcologicalCollapse` event → catastrophe (consumer-tier dies,
  civ-bearing species takes population hit).

**Worldgen**:
- `sample_planet` populates `species_registry` with 5-15 species
  (count derived from biosphere class). Each species gets a role,
  trait distribution drawn from the planet's substrate/atmosphere.
- Interactions drawn from a per-role-pair distribution: producer
  × consumer = consumer benefits (positive), producer suffers
  (negative); decomposer × everything-dead = decomposer
  benefits; mutualist × specific-partner = both benefit.
- Civ-bearing species: pick one Consumer (or higher-tier
  organism) with `cognition >= 0.3`. That species' civs are
  what the existing civ machinery operates on.

**Migration path**:
- `Planet::species` field replaced with `Planet::civ_bearing_species()`
  accessor that returns `&Species` (the existing single-species
  pattern).
- Existing call sites work through the accessor; new ecosystem
  logic operates on `species_registry`.

**Test plan**:
- New `planet_has_multi_species_registry`: assert a sampled
  Lush planet has ≥ 5 species across ≥ 3 roles.
- New `ecosystem_step_propagates_producer_collapse_to_consumer`:
  artificially crash producer pool, run ecosystem step, assert
  consumer pool drops.
- New `ecological_collapse_event_fires_on_producer_extinction`:
  drive producer to extinction, assert
  `EcologicalCollapse` event emits and civ takes a hit.
- Existing civ + species tests unchanged (back-compat via
  `civ_bearing_species()` accessor).

**Risk register**:
- **R1**: existing tests construct `Species` literally with the
  old field set. Default new fields (role=Consumer, pool=Pop::ZERO)
  so they compile.
- **R2**: determinism — adding 5-15 species per planet means
  5-15× more RNG draws at worldgen. Existing seeds will sample
  different planets. Acceptable — multi-species is a fundamental
  shift; seed-rebaseline expected.
- **R3**: report-side rendering — viewport + post-run report
  need to surface multi-species state. Add a "Biosphere" section
  to the report listing species and ecological events.
- **R4**: emergent tools currently key on the single civ-bearing
  species. With multi-species, do other species also generate
  emergent tools? No — civ-bearing species only. Keep emergent
  tool path unchanged.

**Effort**: XL — 50 hours over 1-2 weeks.

### Item 8: Substrate-coupled solvent semantics

**Why**: `Substance::{Water, Ice, Vapour}` is named for water but
relabeled per-substrate. Mechanically identical chemistry across
substrates. A xenobiologist calls this "naming, not physics."

**Files**:
- `sim/physics/src/chemistry/substance.rs` (the enum)
- `sim/physics/src/chemistry/substrate.rs` (per-substrate constants)
- `sim/physics/src/chemistry/reactions.rs` (phase transitions)
- `sim/recognition/src/templates.rs` (templates referencing
  Water etc.)
- `sim/world/src/init.rs` (per-substrate initialisation)

**Approach**:
- **Rename** `Water/Ice/Vapour` → `SolventLiquid/SolventSolid/SolventGas`
  (or keep `Water/Ice/Vapour` for back-compat but document
  intent). Recommend rename for clarity.
- Add per-substrate solvent properties: `solvent_latent_heat`,
  `solvent_freeze_threshold`, `solvent_boil_threshold`,
  `solvent_density`, `solvent_surface_tension`. Already partially
  exists in `chemistry/substrate.rs`; extend.
- Phase-transition logic reads per-substrate constants based on
  `planet.metabolic_substrate.tag()` (already threaded).
- Recognition templates referencing "water" → reference "solvent"
  (generic) or split per-substrate. Recommend generic.

**Migration path**:
- Type aliases `Water = SolventLiquid` etc. for back-compat
  during transition.
- All existing call sites continue to work.
- New code uses the new names.

**Test plan**:
- Existing physics + chemistry tests pass unchanged.
- New `silicate_solvent_freezes_at_substrate_threshold`: 1500K
  cell on silicate world doesn't freeze; 800K cell does.
- New `ammoniacal_solvent_has_correct_latent_heat`: verify
  per-substrate latent heat constants match published values.

**Risk**:
- **R1**: 30+ call sites for Water/Ice/Vapour. Renaming touches
  all of them. Use type aliases to keep old names working
  during transition.
- **R2**: Recognition template wording — templates say
  "surface_water"; should they say "surface_solvent"? Yes,
  rename for cross-substrate accuracy.

**Effort**: L — 30 hours over ~1 week.

### Sprint 2 total

~80 hours. Sprint review: xenobiologist approves multi-species
+ solvent semantics (the two biggest fixes for the "physiology
not biology" critique). Astrophysicist not directly affected.

## Sprint 3: Xeno + astro coupling

Four items in parallel: **#7 lifecycle topology**, **#9 alt
redox**, **#13 ice-albedo**, **#14 greenhouse runaway**.
Different subsystems, independent files.

### Item 7: Lifecycle-topology variants

**Why**: `PopulationBiology` brackets are vertebrate-mammalian.
Insects have egg/larva/pupa/adult. Plants have seed/seedling/
mature/senescent. Modular organisms have biomass + budding.

**Files**:
- `sim/species/src/types.rs` (Lifecycle enum)
- `sim/population/src/lib.rs` (per-lifecycle step functions)
- `sim/species/src/sampling.rs` (Lifecycle derivation)

**Approach**:
```rust
pub enum Lifecycle {
    Vertebrate,        // infant/juvenile/fertile/elder (current)
    Insect,            // egg/larva/pupa/adult; pupa is non-feeding
    Plant,             // seed/seedling/mature/senescent; high
                       //   seed clutch
    Modular,           // biomass + budding rate; no brackets
}
```

- `PopulationBiology` gains `lifecycle: Lifecycle` field.
- `Cohort` gains alternative bracket schemas — keep current
  4-bracket Vertebrate as default; add `Cohort::Insect`,
  `Cohort::Plant`, `Cohort::Modular` variants (or a single
  `BracketBag` struct with per-lifecycle accessors).
- `step_with_capacity` dispatches on `biology.lifecycle`.

**Derivation** (in sampler):
- Manipulation modes: `WebConstruct + Mandible` → Insect
  likely. `LimbGrasp + Trunk` → Vertebrate likely.
- Body plan / cognition: high cognition → Vertebrate.
- Habitat: Endolithic → Modular default.
- Random fallback: bias toward Vertebrate (most familiar).

**Test plan**:
- `insect_lifecycle_pupa_bracket_zero_food`: pupa bracket
  consumes no food, produces no offspring.
- `plant_lifecycle_seed_clutch_supermajority`: plant species
  has 80%+ of pop in seed bracket.
- `modular_lifecycle_no_brackets_total_equals_biomass`:
  modular species tracks biomass only; `total()` equals
  biomass.

**Risk**:
- **R1**: Cohort variants explode the API surface — every
  cohort operation needs per-lifecycle dispatch.
- **R2**: Migration migration (cross-bracket flow) — vertebrate
  fertile bracket and insect adult bracket are similar but not
  identical. Map carefully.

**Effort**: L — 40 hours over ~1 week.

### Item 9: Alternative redox chemistry

**Why**: `Substance::Oxidiser` is O₂-shaped. Ammonia substrate
needs N₂O₄. Methane substrate needs perchlorate. Combustion
1:1:1 stoichiometry ignores molar masses.

**Files**:
- `sim/physics/src/chemistry/substance.rs` (per-substrate
  oxidiser?)
- `sim/physics/src/chemistry/reactions.rs` (stoichiometry)
- `sim/world/src/init.rs` (oxidiser deposition)

**Approach**:
- Keep `Substance::Oxidiser` as generic "oxidising agent"
  enum slot.
- Per-substrate `oxidiser_molar_mass: Real`, `combustion_yield_ratio:
  Real` constants in `chemistry/substrate.rs`. The substrate
  determines what the Oxidiser is (O₂ for Aqueous, N₂O₄ for
  Ammoniacal, perchlorate for Hydrocarbon, silicate-melt-O for
  Silicate).
- Stoichiometry per-substrate: `combustion(fuel, oxidiser,
  substrate) → product` with substrate-dependent yields.
- `LocalisedCombustion` tool requires non-zero oxidiser of the
  appropriate substrate type.

**Migration**:
- Existing `Substance::Oxidiser` operations continue to work;
  per-substrate behavior layered on via substrate dispatch.

**Test plan**:
- `ammonia_substrate_combustion_uses_n2o4_stoichiometry`:
  verify per-substrate stoichiometry.
- `co2_atmosphere_aqueous_substrate_combustion_works`: this is
  the "Lumen-h fire-locked" case from PR #21; should now
  succeed (CO₂ atmosphere has trace O₂ from biosphere).

**Risk**:
- **R1**: Recognition templates reference "fire" with specific
  signature; per-substrate fire would need per-substrate
  templates. Defer template rework to Item 8.
- **R2**: Tool gating that requires "fire confirmed" still works
  but interprets fire per-substrate.

**Effort**: L — 30 hours over ~1 week.

### Item 13: Ice-albedo feedback

**Why**: Albedo is per-atmosphere-class constant. Real ice
reflects ~80% of light; ocean absorbs ~95%. Without per-cell
albedo, no snowball state.

**Files**:
- `sim/physics/src/radiation.rs` (per-cell albedo derivation)
- `sim/physics/src/state.rs` (per-cell albedo field?)
- `sim/world/src/init.rs` (initial albedo from terrain)

**Approach**:
- Per-cell albedo computed from `(atmosphere.base_albedo +
  ice_fraction × 0.5 + cloud_fraction × 0.2)`, clamped to
  [0, 1].
- `ice_fraction = substance(Ice)[cell] / max_ice_cap` (or
  some sensible normalisation).
- `Radiation::integrate` reads per-cell albedo instead of
  planet-wide.

**Test plan**:
- `cold_seed_slides_into_snowball`: build a cold-equilibrium
  state, run 1000 ticks, assert majority of cells acquire ice
  coverage and equilibrium temperature drops by ≥ 20K.
- `ice_albedo_feedback_amplifies_cooling`: directly add ice to
  some cells, observe temperature drop in next radiation step.

**Risk**:
- **R1**: Snowball state could lock out civilization — once
  ice-covered, civs starve. Add a tunable `albedo_ice_bonus`
  parameter so the strength of feedback can be calibrated.
- **R2**: Computational cost — per-cell albedo lookup runs
  every macro-step. Cache if needed.

**Effort**: M — 20 hours over ~3 days.

### Item 14: Greenhouse runaway / snowball bistability

**Why**: Climate is steady-state radiation balance. Real climate
has positive feedback loops. Sim has neither hot-runaway nor
snowball.

**Files**:
- `sim/physics/src/radiation.rs` (per-cell greenhouse)
- couples to vapour density from hydrology

**Approach**:
- Per-cell `greenhouse_k = atmosphere.base_greenhouse +
  vapour_fraction × greenhouse_vapour_coefficient`.
- `vapour_fraction = state.substance(Vapour)[cell] /
  saturation_cap[cell]` (uses Item 4's saturation cap).
- Radiation law uses per-cell greenhouse.

**Test plan**:
- `hot_seed_slides_into_venus_state`: build a hot seed, run
  1000 ticks, assert temperature rises further (positive
  feedback) and vapour density increases.
- `bistable_climate_basins_separate`: pick two close starting
  states, one slightly hotter than the other; assert they
  diverge over time (one to hot basin, one to cold basin).

**Risk**:
- **R1**: Runaway in fixed-point — temperature could climb past
  fixed-point ceiling. Cap absolute temperature at a sane
  ceiling (e.g., 2000K).
- **R2**: Coupling with Item 13 — ice-albedo + greenhouse-vapour
  must both land for true bistability. Sequence them in the
  same sprint.

**Effort**: M — 20 hours over ~3 days.

### Sprint 3 total

~110 hours. Sprint review: xenobiologist on items 7 + 9;
astrophysicist on items 13 + 14.

## Sprint 4: Astro long-timescale

One XL item: **#12 tectonics + erosion**. Standalone sprint
because of size.

### Item 12: Tectonics + erosion

**Why**: `state.elevation` is "permanently deferred from physics
mutation". Every planet's geography is freeze-frame forever.
Astrophysicist calls this "rocky planets without rock cycle —
incoherent."

**Files**:
- `sim/physics/src/state.rs` (mutate elevation; add
  `crustal_thickness` field?)
- `sim/physics/src/tectonics.rs` (new — plate uplift)
- `sim/physics/src/erosion.rs` (new — sediment transport)
- `sim/physics/src/orchestration.rs` (slot new laws into step)
- `sim/world/src/planet.rs` (per-cell `plate_id`?)

**Data structures**:
- `state.elevation_mut()`: now mutable (was previously immutable).
- New `state.crustal_thickness()`: per-cell thickness in km.
- New `state.plate_id()`: per-cell plate index (assigned at
  worldgen via Voronoi partition).

**Tectonics**:
- Per-tick `tectonics::integrate`:
  - For each plate boundary cell (where neighboring cells have
    different `plate_id`):
    - Compute convergence rate from mantle flow proxy (e.g.,
      uniform per-plate velocity sampled at worldgen).
    - Convergent: uplift (`elevation[cell] += uplift_k × dt`)
    - Divergent: rift formation (`elevation[cell] -= rift_k × dt`)
    - Transform: shear (no elevation change)
  - Crustal thickness conserved per-plate via pair-flux.

**Erosion**:
- Per-tick `erosion::integrate`:
  - For each cell with `water_depth > 0` flowing downhill (via
    `GravityFlow` velocity):
    - Compute sediment pickup: `sediment_pickup_k ×
      velocity_magnitude × water_depth × dt`
    - Transport sediment downhill (new substance
      `Substance::Sediment`?)
    - Deposit at depressions / low-velocity cells.
  - Net effect: elevation decreases at high-velocity cells
    (erosion), increases at deposition zones.

**Plate generation** (worldgen):
- At init, partition grid into N plates (N derived from
  planet radius / thermal age).
- Voronoi partition from N seed cells.
- Per-plate uniform velocity (random direction).

**Test plan**:
- `tectonics_uplifts_convergent_boundaries`: configure two
  plates converging, run 1000 ticks, assert elevation at
  boundary increases.
- `erosion_lowers_mountain_cells`: configure a high-elevation
  cell with water flow, assert elevation decreases over time.
- `sediment_conserved_through_transport`: drive erosion on a
  multi-cell terrain, assert total elevation + total sediment
  in transport is conserved.
- `50000_tick_planet_shows_measurable_geography_change`: run
  long simulation, assert before/after elevation maps differ.

**Risk register**:
- **R1**: Massive subsystem — new fields, new laws, new
  orchestration slots, possibly new Substance. Multi-week.
- **R2**: Determinism — plate partition must be deterministic
  from seed. Sediment transport must conserve mass bit-exactly.
- **R3**: Interaction with existing terrain features (peaks,
  hills) — terrain_peak is sampled at worldgen and reused
  throughout. Need to refresh derived metrics on elevation
  change.
- **R4**: Tests need to run long sims (50k ticks). Use
  `#[ignore = "slow"]` markers.

**Effort**: XL — 80 hours over 2 weeks.

### Sprint 4 total

~80 hours, single XL item. Sprint review: astrophysicist (the
deepest astro fix in the roadmap).

## Sprint 5: Closing the gaps

Six parallel items: **#10 cognition topology**, **#11
speciation**, **#15 Hadley cells**, **#16 tidal heating**,
**#17 atmospheric escape**, **#18 stellar variability**.
All M-effort, different subsystems.

### Item 10: Cognitive topology as real mechanic

**Why**: `CognitionTopology::{Centralized, Distributed}` is
flavor-only.

**Files**:
- `sim/civ/src/discovery/hypothesizer.rs` (step function)
- `sim/species/src/sampling.rs` (axis derivation)

**Approach**:
- Distributed topology: per-tick multi-fit (2-3 fit attempts
  per tick instead of 1). But cap `Form::Polynomial2/3`
  confidence at 0.6 (no formal abstraction).
- Centralized topology: single fit per tick. Full confidence
  on all forms.
- Wire `CognitionAxes` differently: Distributed → derive
  `social` axis dominantly; Centralized → derive `abstraction`
  axis dominantly.

**Test plan**:
- `distributed_species_discovers_faster_at_low_complexity`:
  same seed, two species (Distributed vs Centralized); after
  5000 ticks, Distributed has more confirmed relations on
  Linear/Threshold forms.
- `centralized_species_reaches_higher_abstraction_ceiling`:
  symmetric — Centralized confirms more Polynomial relations.

**Effort**: M — 16 hours.

### Item 11: Speciation / evolution events

**Why**: species traits drift but don't *split*. Depends on
Sprint 2's multi-species ecosystem (item 6).

**Files**:
- `sim/species/src/lib.rs` (speciation function)
- `sim/core/src/lib.rs` (trigger conditions)

**Approach**:
- Per-tick check: for each species, if (catastrophe streak >
  threshold OR climate-shift > threshold OR
  isolation-by-geography > threshold), trigger speciation.
- Speciation: create new species with parent's traits +
  divergent pull (±0.2 on each trait axis). Add to planet
  `species_registry`.
- Allopatric path: when geographic isolation between civ
  populations exceeds N cells, split.
- Sympatric path: when niche pressure (resource competition
  with another species) drives trait drift past threshold.

**Test plan**:
- `high_catastrophe_pressure_triggers_speciation`: subject
  species to repeated catastrophes, assert second species
  appears in registry.
- `speciated_species_inherits_parent_traits_with_divergence`:
  verify child species' traits are ±0.2 from parent.

**Effort**: L — 24 hours. Depends on item 6.

### Item 15: Hadley/Ferrel/polar cells

**Why**: Wind is local pressure-gradient + Coriolis; doesn't
organize into three-cell structure. Depends on Sprint 1's
adaptive dt (item 3).

**Files**:
- `sim/physics/src/wind.rs` (meridional overturning)
- `sim/physics/src/vertical_convection.rs` (couple)

**Approach**:
- After horizontal wind step, compute equator-pole gradient.
- Add a meridional bias proportional to:
  `latitude × (1 - 2×|latitude|/3) × gradient_strength`
- This produces:
  - Equator (lat ≈ 0): rising motion
  - Mid-latitude (lat ≈ 30°): descending motion
  - High-latitude (lat ≈ 60°): rising motion
  - Pole (lat ≈ 90°): descending motion
- Couple to `VerticalConvection` for the rising/descending
  signature.

**Test plan**:
- `hadley_cell_descends_at_30_latitude`: run sustained
  gradient, assert vertical velocity profile shows descending
  motion at mid-latitudes.

**Effort**: L — 30 hours.

### Item 16: Tidal heating

**Why**: Io is heated by Jupiter's tidal flexing. No internal-
friction heat source in current sim.

**Files**:
- `sim/physics/src/tides.rs` (add heat output)

**Approach**:
- Per-cell heat contribution = `Σ moons (mass_relative × dt ×
  friction_k × local_potential_gradient²)`
- Where `friction_k` is tunable (Io-like config gives
  significant heat; Earth-Moon gives negligible).
- Fed into `state.temperature_mut()`.

**Test plan**:
- `tidal_heating_warms_io_like_configuration`: planet with
  massive close moon shows measurable temperature elevation
  independent of insolation.

**Effort**: M — 12 hours.

### Item 17: Atmospheric escape

**Why**: Light atmospheres should escape Jeans-style. Mars-thin
worlds should be thin *because* they lost atmosphere over Gyr.

**Files**:
- `sim/physics/src/atmosphere.rs` (new — atmospheric evolution)
- `sim/world/src/planet.rs` (mutable atmosphere mass)
- `sim/physics/src/orchestration.rs` (slot in)

**Approach**:
- Per-tick atmospheric mass loss rate per substance:
  `loss = base_jeans_rate × T_exo / (gravity × molecular_mass)`
- Lighter molecules (H, He) escape faster than heavier (CO₂, N₂).
- Composition shifts toward heavier molecules over time.
- Surface pressure decays with mass loss.

**Test plan**:
- `low_gravity_hot_planet_loses_atmosphere_over_50000_ticks`:
  run long sim, assert atmospheric mass decreases.
- `high_gravity_cold_planet_retains_atmosphere`: control case.

**Effort**: M — 16 hours. Depends on Item 18 (stellar variability
provides EUV flux).

### Item 18: Stellar variability

**Why**: `stellar_luminosity` is constant. Stars flare, have
sunspot cycles, evolve through main sequence.

**Files**:
- `sim/physics/src/radiation.rs` (time-varying luminosity)
- `sim/civ/src/catastrophe/triggers.rs` (flare events)

**Approach**:
- Slow secular: `stellar_luminosity(t) = base × (1 + secular_k
  × t / star_main_sequence_lifetime)`. Earth's Sun has gone
  from 70% to 100% over 4.5 Gyr (faint-young-Sun problem).
- Fast variation: sunspot cycle (~11 yr period, ±0.1% amplitude).
- Flare events: stochastic, rare, sudden insolation burst.
  Hook into catastrophe system as `SolarFlare`.

**Test plan**:
- `stellar_luminosity_drifts_over_long_run`: 50k ticks,
  luminosity at end differs from start.
- `flare_event_fires_as_catastrophe`: artificially trigger
  flare, assert SolarFlare catastrophe emits.

**Effort**: M — 16 hours.

### Sprint 5 total

~114 hours. Sprint review: both experts (mix of xeno + astro).

## Cross-sprint concerns

### Determinism preservation

Each sprint adds new RNG draws (worldgen for plates, species
registry, etc.). Reserve a per-sprint RNG seed offset so each
sprint's RNG draws don't interleave with the prior sprint's.

```rust
let sprint_2_seed = planet_seed.wrapping_add(0x_S2_DEADBEEF);
let sprint_3_seed = planet_seed.wrapping_add(0x_S3_DEADBEEF);
// etc
```

Existing seeds get rebaselined per sprint (worldgen changes
shift everything downstream). Acceptable — each sprint is a
documented seed-rebaseline event.

### Test infrastructure scaling

By Sprint 5, the workspace will have ~600+ tests. Run time
per-test must stay bounded. Long-running tests (16k+ ticks)
must stay marked `#[ignore = "slow"]` and only run in
release-mode CI.

### Save/load migration

The project currently doesn't serialise mid-run state (it's a
batch-mode sim). If save/load is added later, all new fields
must be serialisable. Use `#[derive(Serialize, Deserialize)]`
patterns consistent with existing structs.

### Documentation updates

Each sprint adds new mechanics. Update `docs/` accordingly:
- Sprint 2: `docs/xenobiology.md` (new)
- Sprint 4: `docs/tectonics.md` (new)
- Sprint 5: `docs/climate-feedbacks.md` (new), update
  `docs/physics.md`

### Performance

Multi-species + per-cell albedo + tectonics + 3-pass tide flux
all add per-tick work. Profile after each sprint. Target: a
1080-cell grid 5000-tick run completes in < 5 min release-mode.

## Verification gates

Per-sprint expert review (xeno / astro / both as appropriate)
must approve before next sprint starts. Reviewers run a
domain pass on the merged diff; they flag completeness gaps,
realism issues, missing test coverage, or design tensions.

**Sprint 1**: physicist review (numerical correctness).
**Sprint 2**: xenobiologist review (multi-species, solvent).
**Sprint 3**: both xeno + astro (split items).
**Sprint 4**: astrophysicist review (tectonics).
**Sprint 5**: both, final integrated review.

After Sprint 5, the per-expert sign-off checklists in
`docs/expert-review-roadmap.md` must both pass.

## Risk register (project-wide)

- **R1: Scope creep.** 18 items is already an aggressive 5-
  sprint plan. Resist adding mid-sprint items unless they
  block the sprint's existing work.
- **R2: Determinism drift.** Each new RNG draw shifts seeds.
  Document each shift in the per-sprint changelog.
- **R3: Performance.** Per-tick work grows. Profile + measure
  per sprint.
- **R4: API churn.** Refactors (Lifecycle, multi-species)
  ripple through call sites. Use type aliases + deprecated
  re-exports to soften transitions.
- **R5: Test suite scaling.** 18 items × ~3 tests each = 50+
  new tests. Group related tests; reuse fixtures.
- **R6: Expert availability.** Per-sprint expert review needs
  expert turnaround. If review is slow, sprints stall. Plan
  buffer.
- **R7: Save/load** — out of scope but future-affecting. Note
  in commits.

## Effort summary

| Sprint | Hours | Weeks (1 dev) |
|---|---|---|
| 1 (numerical hygiene) | 26 | 1 |
| 2 (xeno foundation) | 80 | 2 |
| 3 (xeno + astro coupling) | 110 | 3 |
| 4 (astro tectonics) | 80 | 2 |
| 5 (closing gaps) | 114 | 3 |
| **Total** | **410** | **11** |

~11 weeks for a single dev focused on this. Parallel devs
shorten via items 6/8 in Sprint 2, items 7/9/13/14 in Sprint 3,
all of Sprint 5 — substantial parallelism possible.

## Closing acceptance

After Sprint 5, both expert sign-off checklists from
`docs/expert-review-roadmap.md` must pass. The sim will go from
"well-engineered demo with characterised limits" to "credible
representation in both lenses."

Open questions to settle before kickoff:
- **Q1**: Save/load — is it in scope for this roadmap or a
  follow-up?
- **Q2**: Performance targets — confirm the < 5 min/5000-tick
  target.
- **Q3**: Expert review cadence — sync or async?
- **Q4**: Test coverage minimum — current is ~60% line coverage;
  any of this work require raising the bar?
- **Q5**: Documentation — do we need rendered visualisations
  of the new mechanics (e.g., a sample post-run report showing
  multi-species ecosystem state)?

Resolve before Sprint 1 PR opens.
