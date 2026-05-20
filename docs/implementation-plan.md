# Implementation plan: expert-review roadmap (v2)

Companion doc to `docs/expert-review-roadmap.md`. **Revision 2** —
incorporates xenobiologist + astrophysicist feedback on v1. Major
shape changes:

- **Backlog grew from 18 → 24 items** (six entirely new from astro)
- **Items 6, 7, 12, 13, 14, 15, 16, 17, 18 substantially rewritten**
  to address physics-accuracy + biology-system gaps the v1 missed
- **Effort estimate revised from 410h / 11 weeks → ~700h / 17 weeks**
- **Sequencing reshuffled** to put dependencies first
  (extinction before speciation, weathering before greenhouse,
  CFL acoustic-speed in Sprint 1)

This doc is the planned-implementation source-of-truth. Each item
has technical approach, test plan, risk register, effort. Both
experts pre-sign-off on v2 before any Sprint 1 code lands.

## Methodology

(unchanged from v1)

- One sprint at a time, in roadmap order. No skipping ahead.
- Each item has an acceptance test that lands with the
  implementation. No item is "done" until test exists + passes.
- Per-sprint expert review. Reviewer rejects block sprint
  close.
- Quantitative tests against published values where applicable
  (Io heat budget, snowball ice-line latitude, faint-young-Sun
  factor, etc.), not just direction-of-change behavioural
  tests.

## Cross-cutting invariants (apply to all sprints)

(unchanged from v1, plus one addition)

1. Q32.32 determinism preserved.
2. Pair-flux conservation by default.
3. Operator-split ordering preserved.
4. Back-compat for hand-built test fixtures.
5. No new dependencies.
6. Per-sprint expert review.
7. **NEW: quantitative-anchor tests.** Behavioural tests
   ("number X changes in direction Y") get paired with
   calibration tests ("number X falls within [A, B] for
   Earth-analog parameters") wherever published values exist.

## Sprint 1: numerical hygiene

7 items (was 5). Two additions from astro: 1a, 1b.

### Item 1: Reproductive_success curve shape

(unchanged from v1)

Quadratic success curve + 5,000-cap clutch_size. Companion
realism tests for mid (r=0.5) and r-end (r=1.0) seeds.

**Effort**: S — 4 hours.

### Item 2: Two-pass donor-limited tide flux

(refined from v1 per astro feedback)

Three-pass scheme as v1, **PLUS**:
- New test `tide_bulge_two_moons_different_declinations_preserves_
  cos2_shape` — verify declination cos²-shape preserved under
  multi-moon interference.
- New test `tide_spring_neap_beat_envelope_preserved_under_donor_
  cap` — verify amplitude beat pattern survives donor capping.

**Effort**: M — 10 hours (was 8, +2 for additional tests).

### Item 3: Adaptive dt sub-stepping in wind

(refined per astro feedback — Items 1a, 1b folded in)

**Item 1a addition**: CFL criterion includes acoustic speed.
Real atmospheric CFL is `(|u| + c_s) Δt / Δx < 1`. Compute
`c_s ≈ sqrt(R T / M_atm)` per cell (or planet-wide approximation
from atmosphere class). Sub-step count includes the acoustic
term.

**Item 1b**: Remove the n_substep=8 silent cap. Either:
- Fail loudly: `panic!` in debug, emit warning + skip wind
  step in release with a flag.
- Or document "configurations beyond N=16 substeps unsupported"
  and refuse to simulate (sample new planet at worldgen).

Recommend the second — at worldgen check that
`max_steady_state_wind_speed < 16 × c_s × dx / dt` and reject
planets that violate.

**Pressure re-derivation inside sub-step loop**: T is advected
within the wind step, so pressure must update inside the loop,
not just once at the top.

**New test**: `wind_sub_step_converges_with_n_under_pressure_
gradient` — drive a strong gradient, run wind with N=1, 2, 4,
8 sub-steps; result should converge as N grows (asymptotic
limit reached around N=4 for Earth-like seeds).

**Effort**: L — 16 hours (was M=6, +10 for acoustic + pressure
re-derive + convergence test).

### Item 4: Saturation-curve vapour cap floor

(unchanged from v1)

**Effort**: S — 4 hours.

### Item 5: Cumulative hydrology mass-conservation assert

(unchanged from v1)

**Effort**: S — 4 hours.

### Sprint 1 total

~38 hours (was 26). Sprint review: physicist (numerical) +
astrophysicist (CFL + acoustic speed validation).

## Sprint 2: xenobiology foundation (major rewrite)

**Substantial reshape from v1.** Original Items 6 + 8 expanded
to 7 items per xeno feedback. Adds: typed roles, typed
interactions, recognition-template rework moved IN (not deferred),
extinction rule, biogeochemical-cycle channels.

### Item 6: Multi-species ecosystems (v2)

**Rewritten per xeno feedback.**

**Why**: v1's single `Consumer` role + signed-scalar interaction
matrix produced a flat 2-tier "biosphere with labels." Real food
webs need trophic pyramids, typed interactions, and keystone
detection.

**Data structures**:

```rust
// In sim_species
pub enum EcosystemRole {
    Producer { metabolism: ProducerMetabolism },
    PrimaryConsumer,     // herbivore-equivalent
    SecondaryConsumer,   // carnivore-1
    ApexConsumer,        // top predator
    Detritivore,         // consumes dead biomass
    Saprotroph,          // breaks down fixed carbon (fungi-equivalent)
    Mutualist { kind: MutualismKind },
    Parasite { kind: ParasiteKind },
}

pub enum ProducerMetabolism {
    Photoautotroph,    // sunlight-driven
    Chemoautotroph,    // chemical-energy-driven (vent ecosystems)
    Mixotroph,         // both
}

pub enum MutualismKind {
    Pollinator,
    SeedDisperser,
    Engineer,        // habitat-modifying (beavers, corals,
                     // mycorrhizae)
    Generic,         // direct biomass exchange
}

pub enum ParasiteKind {
    Macro,   // worms, fleas
    Micro,   // bacteria, protists
    Virus,   // requires host cellular machinery
}

pub struct Interaction {
    pub kind: InteractionKind,
    pub strength: Real,
    pub functional_response: FunctionalResponse,
}

pub enum InteractionKind {
    Predation,        // Lotka-Volterra cycling
    Competition,      // exclusion-equilibrium
    Mutualism,        // both benefit
    Commensalism,     // one benefits, other indifferent
    Parasitism,       // one benefits, one suffers
    HabitatModification,  // engineering effect
}

pub enum FunctionalResponse {
    Linear,           // Type I
    Saturating,       // Type II (most realistic)
    Sigmoidal,        // Type III
}

pub struct InteractionMatrix {
    pub pairs: BTreeMap<(SpeciesId, SpeciesId), Interaction>,
}
```

**Per-tick step**:
- For each species, compute `delta` using functional-response
  appropriate to each pair (saturating predation, not linear).
- Apply carrying capacity per role via Lindeman 10:1 pyramid:
  producer biomass × 0.1 = consumer ceiling.
- Detect keystone species via network centrality
  (betweenness over the interaction graph).

**Worldgen**:
- Per-planet 8-20 species (was 5-15), with at minimum: 2
  Producers, 3 PrimaryConsumers, 2 SecondaryConsumers, 1
  ApexConsumer, 1 Detritivore, 1 Saprotroph, plus 1-3
  Mutualists and 1-5 Parasites.
- Trophic pyramid enforced at sampling: per-tier biomass cap
  follows 10:1 ratio.
- Civ-bearing species: pick one Consumer (any tier) with
  cognition ≥ 0.3.

**Tests** (more rigorous than v1):
- `planet_has_trophic_pyramid_with_lindeman_ratio` — assert
  consumer biomass ≈ 0.1 × producer biomass.
- `predator_prey_pair_exhibits_lotka_volterra_cycles` — set up
  isolated pair, run 1000 ticks, verify oscillation.
- `keystone_species_removal_causes_cascade_disproportionate_
  to_biomass` — remove a low-biomass high-centrality species,
  assert larger collapse than removing equal biomass of
  peripheral species.
- `producer_collapse_propagates_to_consumer_tiers` — verify
  cascade.
- `competition_pair_excludes_at_equilibrium` — distinct
  dynamics from predation.

**Effort**: XL — 70 hours (was 50, +20 for typed interactions
+ pyramid enforcement + functional responses).

### Item 6a (new): Extinction rule

**Why**: Per xeno feedback — Speciation (Item 11) without
extinction → unbounded registry growth. Must land **before**
speciation.

**Approach**:
- Per-tick check: if any species' `population_pool` drops below
  `EXTINCTION_THRESHOLD` (e.g., 0.001 × planet capacity) and
  stays there for `EXTINCTION_CONFIRMATION_TICKS`, mark
  extinct.
- Extinct species stays in registry (for history) but `is_extant
  = false`. Ecosystem step skips it.
- New `SpeciesExtinct { tick, species_id, cause }` event.

**Tests**:
- `extinct_species_stops_contributing_to_ecosystem`
- `extinction_cascade_from_keystone_removal`
- `extinction_event_emits_on_pool_collapse`

**Effort**: M — 14 hours.

### Item 6b (new): Biogeochemical-cycle coupling

**Why**: Per xeno feedback — Producers shouldn't "create biomass
from base_growth." They should fix atmospheric CO₂. Couples
ecosystem to atmospheric composition.

**Approach**:
- Producer biomass growth requires CO₂ + (sunlight OR chemical
  energy). Per-tick: `producer_growth = min(co2_available,
  energy_available, base_potential)`.
- Consumer growth consumes producer biomass, returns CO₂ via
  respiration.
- Decomposer consumes dead biomass, returns CO₂ + frees nutrients.
- Couples to `Substance::Vapour` and (new) `Substance::CO2` —
  must split CO₂ from generic Vapour.

**Tests**:
- `producer_growth_consumes_atmospheric_co2`
- `consumer_respiration_returns_co2_to_atmosphere`
- `decomposer_chain_balances_carbon_budget`

**Effort**: L — 30 hours.

### Item 7: Lifecycle topology variants (v2)

**Substantially expanded per xeno feedback.**

```rust
pub enum Lifecycle {
    Vertebrate,                              // 4-bracket (current)
    Aquatic { semelparous: bool },          // tadpole-frog OR salmon
    Insect,                                  // egg/larva/pupa/adult
    Eusocial { castes: Vec<CasteRole> },     // queen + sterile workers
    Plant,                                   // seed/seedling/mature/sen
    Microbial { fission_strategy: Fission }, // binary, budding
    Modular,                                 // colonial — biomass + bud
}

pub enum CasteRole {
    Reproductive,    // queens, drones
    Worker,          // sterile, forages
    Soldier,         // sterile, defensive
    Nurse,           // tends young
}

pub enum Fission {
    Binary,          // bacteria, archaea
    Budding,         // yeast
    Conjugation,     // some prokaryotes — HGT path
}
```

**Resolves Item 1 contradiction**: r=1 broadcast spawner now
routes through `Aquatic { semelparous: true }` lifecycle.
Single-spawn-and-die step function handles it correctly.

**Tests** (more thorough than v1):
- `aquatic_semelparous_lifecycle_single_spawn_then_death`
- `eusocial_lifecycle_castes_track_independently`
- `microbial_binary_fission_doubles_per_generation_time`
- `plant_alternation_of_generations_if_complex`

**Effort**: L — 48 hours (was 40, +8 for additional variants).

### Item 7a (new): Tolerance envelopes

**Why**: Per xeno feedback — Extremophile niche specialisation
not representable. Mass-extinction differential survival not
representable.

**Approach**:
```rust
pub struct ToleranceEnvelope {
    pub temp_range: (Real, Real),
    pub ph_range: (Real, Real),
    pub salinity_range: (Real, Real),
    pub radiation_max: Real,
    pub pressure_range: (Real, Real),
}
```

- `Species::tolerance: ToleranceEnvelope` (sampled per substrate
  defaults, perturbed per-species).
- Habitat occupancy gates on cell conditions ∩ tolerance.
- Catastrophe survival multiplied by `species.tolerance_match(local
  conditions)` — extremophiles survive radiation events, etc.

**Tests**:
- `extremophile_species_occupies_high_radiation_cells`
- `mass_extinction_differential_survival_by_tolerance`

**Effort**: M — 16 hours.

### Item 7b (new): Dormancy / cryptobiosis

**Why**: Per xeno feedback — Tardigrade-grade survival of
catastrophes is a documented seed-bank mechanism that lets
biospheres recover after asteroid impacts. Without it, every
catastrophe is a hard population kill.

**Approach**:
- `Species::dormancy_capability: Real` ∈ [0, 1].
- Catastrophe damage = `base_damage × (1 - dormancy ×
  catastrophe_severity_factor)`.
- Dormant population stays at low metabolism, can resurrect over
  hundreds of ticks post-event.

**Tests**:
- `dormant_species_survives_catastrophe_at_reduced_rate`
- `seed_bank_resurrection_repopulates_post_extinction_event`

**Effort**: S — 8 hours.

### Item 8: Substrate-coupled solvent semantics (v2)

**Expanded per xeno feedback** — phase transitions alone are
"naming, not physics." Add solubility + reaction kinetics shifts.

**Approach** (v1 plus):
- `solvent_solubility[Substance]: Real` — per-substrate
  table indicating dissolution propensity of each substance
  in the solvent. Water dissolves many salts; ammonia dissolves
  different things; liquid methane dissolves almost nothing.
  Affects chemistry kernel availability of reactions.
- `solvent_reaction_kinetics_prefactor: Real` — per-substrate
  Arrhenius prefactor multiplier. Cold solvents (liquid
  methane) have slower reactions than warm (liquid water).
- **Recognition templates per-substrate**: rename
  `surface_water` template to `surface_solvent`, but signature
  varies per substrate. Civs on liquid-methane world see a
  "methane-surface" pattern; civs on water world see "water-
  surface" — semantically separate templates with substrate-
  specific signatures.

**Tests** (more rigorous than v1):
- `methane_substrate_solubility_excludes_most_substances`
- `ammoniacal_solvent_reaction_kinetics_match_published`
- `recognition_template_surface_solvent_per_substrate_signature`

**Effort**: XL — 50 hours (was L=30, +20 for solubility +
kinetics + template rework).

### Item 9: Alternative redox chemistry (v2)

**Expanded per xeno feedback** — multiple oxidisers per
substrate, reduction-potential ladder, syntrophy.

**Approach** (v1 plus):
- Per-substrate `Vec<Oxidiser>` (not single).
- Each oxidiser has `reduction_potential: Real` (V) and
  `available_density: Real`.
- Chemolithotroph producers (added in Item 6b's
  `Chemoautotroph` ProducerMetabolism) partition by which
  oxidiser they reduce — high-potential first (oxygen),
  then sequentially lower-potential as those deplete.
- **Syntrophy**: implement via Item 6's
  `Interaction::Mutualism` — H₂-producing bacteria pair with
  methanogens that consume H₂. Neither can survive alone in
  the niche.

**Tests** (more rigorous):
- `chemolithotroph_species_partition_by_reduction_potential`
- `syntrophy_pair_extinction_when_separated`
- `co2_atmosphere_combustion_works_via_alt_oxidiser`

**Effort**: L — 40 hours (was 30, +10 for multi-oxidiser +
syntrophy).

### Sprint 2 total

~286 hours (was 80). The big xeno sprint. Sprint review:
xenobiologist primary, astrophysicist on biogeochemical cycles
(Item 6b couples to atmospheric composition).

## Sprint 3: xeno + astro coupling (major rewrite)

**Reshape per both expert feedback.** 13 → 14 then now → 17
adds carbon-silicate weathering thermostat (was missing).
Items 13, 14 substantially rewritten with correct physics.

### Item 10: Cognitive topology as real mechanic (v2)

**Refined per xeno feedback** — four-way topology, not binary.

```rust
pub enum CognitionTopology {
    Centralized,         // vertebrate brain
    DistributedRedundant, // octopus, 2/3 neurons in arms
    Collective,           // eusocial colony mind
    Acentric,            // slime mold — chemical gradients, no neurons
}
```

- **Centralized**: deep abstraction, serial reasoning. High
  `abstraction` axis, low parallel discovery rate.
- **DistributedRedundant**: parallel sensing/processing per
  body part. Multi-fit per tick, but limited integration —
  `abstraction` capped at 0.6.
- **Collective**: very high `social` axis, requires colony
  presence for cognition (drops to near-zero in isolated
  individuals).
- **Acentric**: very slow but persistent. Long `attempt_period`
  but no forgetting; cumulative knowledge survives generations
  better.

Plus: communication-channel coupling. Chemical-comm species
have slow long-distance correlation; bioluminescent species
fast but line-of-sight; vibrational fast but short-range.
Hypothesizer transmission speed depends on comm channel.

**Tests** (more orthogonal):
- `collective_species_cognition_drops_in_isolation`
- `acentric_species_retains_knowledge_across_generations_better`
- `comm_channel_modality_affects_transmission_speed`

**Effort**: L — 30 hours (was M=16, +14 for four-way + comm
coupling).

### Item 11: Speciation events (v2)

**Substantially expanded per xeno feedback** — add allopatric/
sympatric/polyploid paths, founder effect, post-extinction
adaptive radiation, correlated trait drift.

**Triggers**:
- Geographic isolation (allopatric) — populations separated by
  > N cells without contact for > M ticks.
- Niche pressure (sympatric) — competition with another
  species on overlapping resources, trait drift in opposing
  directions.
- Polyploidy (plant-only) — instant, rare event for `Lifecycle::
  Plant` species.
- Founder effect — small bottleneck population with allele
  drift toward fixation differently.
- Post-extinction adaptive radiation — rate boosted 5× for
  100 generations after mass-extinction event.

**Trait divergence**: correlated, not random. Use allometry
matrix:
```rust
fn divergence_pull(parent_traits, axis_idx, seed):
    // Body-mass-correlated traits change together:
    // bigger body → longer lifespan → slower metabolism
    // → larger clutch
```

**Tests** (more rigorous):
- `allopatric_isolation_triggers_speciation`
- `niche_pressure_drives_sympatric_speciation`
- `polyploidy_speciation_only_for_plant_lifecycle`
- `founder_effect_rapid_drift_in_bottleneck`
- `post_extinction_radiation_rate_5x_for_100_generations`
- `daughter_species_traits_correlated_via_allometry`

**Effort**: L — 40 hours (was 24, +16 for additional triggers
+ allometry).

### Item 11a (new): Horizontal gene transfer

**Why**: Per xeno feedback — Dominant evolution mode for
prokaryotes (99% of Earth's biospheric history). Vertical
speciation alone misses this.

**Approach**:
- For `Lifecycle::Microbial` species, per-tick low probability
  trait-swap with co-located other Microbial species.
- Probability proportional to cell-overlap × time-overlap.
- Speeds adaptation in prokaryote-equivalent niches.

**Tests**:
- `hgt_propagates_trait_between_colocated_microbial_species`
- `hgt_only_fires_for_microbial_lifecycle`

**Effort**: M — 12 hours.

### Item 13: Ice-albedo feedback (v2)

**Substantially rewritten per astro feedback** — linear ramp
can't produce bifurcation. Need sigmoid + bimodal channels.

**Approach**:
- Per-cell albedo from three channels:
  - `snow_fraction: Real` (over land or ice)
  - `sea_ice_fraction: Real` (gray, not white)
  - `cloud_fraction: Real` (already in atmosphere)
- Sigmoid transition at freeze threshold:
  `albedo = base + 0.5 × sigmoid((T_freeze - T) / 5)` so albedo
  jumps quickly across the freeze line, not linearly.
- Snow accumulates on ice; sea-ice without snow is darker.
- Melt-ponds in summer (sea_ice partially melted) further darken.

**Tests** (calibration-anchored):
- `albedo_step_at_freeze_threshold_produces_bifurcation`
- `cold_seed_with_marginal_temp_falls_into_one_of_two_basins_
  not_intermediate`
- `snowball_ice_line_at_30_latitude_under_solar_constant_loss`
  (real snowball-Earth modelling result)

**Effort**: L — 30 hours (was M=20, +10 for sigmoid + multi-
channel + bifurcation test).

### Item 14: Greenhouse runaway / snowball bistability (v2)

**Substantially rewritten per astro feedback** — linear vapour
coupling can't produce real Venus runaway. Need Clausius-
Clapeyron exponential.

**Approach**:
- Separate `Substance::CO2`, `Substance::H2O_Vapour`,
  `Substance::CH4` channels (was lumped as Vapour).
- Per-cell greenhouse from sum of per-substance contributions:
  `greenhouse = Σ (substance_density × substance_greenhouse_k)`
- H₂O contribution exponential in T (Clausius-Clapeyron):
  `h2o_density = saturation_pressure_at(T)` — couples *both*
  ways through T.
- CO₂ contribution linear (long-lived gas, not T-coupled).
- CH₄ contribution short-lived (photolysis decay).

**Tests** (calibration-anchored):
- `hot_seed_slides_into_venus_state_via_h2o_runaway`
- `runaway_threshold_at_published_T_temp` (Komabayashi-
  Ingersoll limit)
- `co2_thermostat_held_by_weathering_negative_feedback`
  (requires Item 14a)

**Effort**: L — 40 hours (was M=20, +20 for per-substance
separation + Clausius-Clapeyron coupling).

### Item 14a (new): Carbon-silicate weathering thermostat

**Why**: Per astro feedback — Without this, every Earth-like
seed drifts toward Venus over Gyr. The weathering rate
accelerates with T + precipitation, so it acts as a negative
feedback that holds CO₂ at habitable levels.

**Approach**:
- Per-tick CO₂ consumption rate (via silicate weathering):
  `weathering = base × T_factor × precipitation_factor`
- T_factor: Arrhenius-like increase with cell T.
- Precipitation_factor: high in wet cells (high water_depth +
  vapour), zero in dry.
- CO₂ removed from atmosphere over geological timescales.
- Volcanism (Item 12d) returns CO₂; weathering removes it.
  Balance sets equilibrium CO₂ → equilibrium T → equilibrium
  Earth.

**Tests**:
- `weathering_rate_increases_with_temperature`
- `weathering_increases_with_precipitation`
- `weathering_thermostat_holds_earth_like_at_300k_equilibrium`
  (long-run test)

**Effort**: M — 20 hours.

### Sprint 3 total

~242 hours (was 110). Sprint review: xeno + astro both, with
particular attention to whether bistability tests pass.

## Sprint 4: astro long-timescale (major rewrite)

**Substantially expanded per astro feedback** — v1's Item 12
was missing four critical rock-cycle mechanisms. Now broken
into 5 sub-items.

### Item 12: Tectonics + erosion (v2 core)

(rewritten — only uplift + fluvial erosion as v1 said)

**Effort**: L — 50 hours (was 80, but split — subduction etc.
now separate sub-items).

### Item 12a (new): Subduction

**Why**: Per astro feedback — Convergent oceanic-continental
boundaries consume oceanic crust. Without this, plate area is
conserved forever and ocean basins can't be destroyed. Single
most important rock-cycle mechanic.

**Approach**:
- For convergent boundaries between plates with `crust_type ∈
  {Oceanic, Continental}`: identify denser plate (oceanic) and
  mark it consumed.
- Per-tick crust-area transfer at convergent boundary.
- Sinking crust returns mantle-buffered minerals via volcanism
  (couples to Item 12d).

**Tests**:
- `oceanic_continental_convergence_consumes_oceanic_crust`
- `ocean_basin_can_be_completely_consumed_over_geological_time`

**Effort**: M — 20 hours.

### Item 12b (new): Crust_age + ocean-floor age

**Approach**:
- New per-cell `crust_age: u64` field (ticks since formation).
- At divergent (ridge) boundaries, new oceanic crust spawns
  with `age = 0`.
- Oceanic crust depth scales as `depth = base + 350 × √age`
  (real ridge-cooling formula, scaled units).

**Tests**:
- `ridge_crust_starts_age_zero`
- `ocean_depth_increases_with_crustal_age`

**Effort**: S — 8 hours.

### Item 12c (new): Isostasy

**Why**: Crust thickness drives surface elevation via Airy
isostasy. Without it, thickening crust doesn't lift, erosion
doesn't trigger rebound.

**Approach**:
- `h_surface = h_base + (ρ_mantle / ρ_crust - 1) × thickness`
  (Airy formula, scaled).
- Update elevation after any tectonic / erosion update.

**Tests**:
- `crustal_thickening_lifts_surface_elevation`
- `erosion_triggers_isostatic_rebound`

**Effort**: M — 12 hours.

### Item 12d (new): Volcanism + outgassing

**Why**: Per astro feedback — Closes the carbon-silicate cycle
loop. Convergent + divergent boundaries emit CO₂ + H₂O.

**Approach**:
- Per-tick volcanic emission rate at active boundary cells.
- Returns CO₂ + H₂O to atmosphere.
- Hot-spot volcanism (non-boundary) as rare random event.
- Couples to Item 14a (weathering removes; volcanism adds).

**Tests**:
- `volcanism_emits_co2_at_subduction_zones`
- `weathering_volcanism_balance_holds_earth_like_co2`

**Effort**: M — 18 hours.

### Item 12e (new): Slab-pull plate dynamics

**Why**: Per astro feedback — Frozen-in plate velocities make
tectonics a snapshot, not a cycle. Real plates accelerate /
decelerate via slab pull at subduction zones.

**Approach**:
- Plate velocity per-tick adjusted by sum of slab-pull forces
  at its subducting edges.
- Slab pull magnitude ∝ slab length × density contrast.

**Tests**:
- `plate_velocities_evolve_via_slab_pull`
- `subduction_zone_initiation_changes_plate_velocity`

**Effort**: L — 30 hours.

### Sprint 4 total

~138 hours (was 80). Sprint review: astrophysicist (the deepest
astro fix in the roadmap).

## Sprint 5: closing the gaps (major rewrite + new items)

**Expanded per both expert feedback.** Items 16, 17, 18 each
rewritten with correct physics. Plus 6 new items 19-24.

### Item 15: Hadley/Ferrel/polar cells (v2)

**Substantially rewritten per astro feedback** — v1's prescribed
bias was cosmetic. Need angular-momentum-emergent.

**Approach**:
- After horizontal wind step, compute angular-momentum
  conservation per air parcel moving meridionally.
- Poleward-moving parcels conserve `(Ω r cos² lat + u cos lat)`
  → westerly jets aloft.
- Subsidence at jet shear instability → mid-latitude descending
  zone.
- Number of cells emerges from `rotation_rate × planet_radius`:
  slow rotator → one Hadley cell; rapid rotator → 3 cells.

**Tests**:
- `slow_rotator_has_one_pole_to_pole_hadley_cell`
- `earth_like_rotation_has_three_cell_structure`
- `ferrel_cell_eddy_driven_not_thermally_direct`

**Effort**: XL — 50 hours (was L=30, +20 for true emergent
mechanic).

### Item 16: Tidal heating (v2 — fixed formula)

**Rewritten per astro feedback — v1 formula was physically wrong.**

**Correct formula**:
```
H = (21/2) × (k₂/Q) × R⁵ × n⁵ × e² / G
```
- k₂: Love number (per-substrate, ~0.3 for rocky)
- Q: tidal quality factor (~100 for rocky, ~1000 for icy)
- R: body radius (new field — requires Item 21)
- n: mean motion = 2π / orbital_period
- e: orbital eccentricity (must add `eccentricity` field
  to Moon — currently missing)
- G: gravitational constant

**Approach**:
- Add `Moon::eccentricity: Real` field.
- Compute per-moon heating rate via correct formula.
- Distribute heat across cell temperatures.
- Couple to tidal-locking: locked moons in circular orbits
  have e ≈ 0 → minimal heating. Eccentric / resonance-pumped
  orbits (Io-Laplace) sustain e via gravitational forcing
  from other moons.

**Tests** (calibration-anchored):
- `circular_orbit_moon_produces_zero_tidal_heating`
- `io_like_configuration_global_heat_flux_in_50_to_200_tw_range`
  (matches Io's real ~100 TW)

**Effort**: L — 30 hours (was M=12, +18 for correct formula +
radius dependency + eccentricity).

### Item 17: Atmospheric escape (v2 — multi-channel)

**Substantially rewritten per astro feedback** — v1's Jeans-only
won't reproduce Mars's loss history.

**Approach** (four channels):
1. **Jeans (thermal)**: as v1.
2. **Hydrodynamic blow-off**: for hot young atmospheres with
   high XUV flux (from Item 18a EUV channel).
3. **Photochemical**: O from H₂O photolysis, etc. — primary
   non-thermal loss for Mars today.
4. **Ion escape**: charged species escape along open magnetic
   field lines. Coupled to `state.magnetic_field()` — planets
   with strong fields lose less via this channel.
- Composition shifts via differential rates (light first).

**Tests**:
- `mars_analog_loses_atmosphere_via_combined_channels_at_realistic_rate`
- `magnetic_field_protection_reduces_ion_escape`
- `low_gravity_hot_planet_loses_h_first_then_o_then_co2`

**Effort**: L — 36 hours (was M=16, +20 for multi-channel +
magnetic-field coupling).

### Item 18: Stellar variability (v2 — SED + star types)

**Expanded per astro feedback** — v1 was a stub.

**Approach** (v1 plus):
- Add `Star::spectral_type: SpectralType { M, K, G, F, A }`.
- Per-type flare rates (M-dwarfs 100× G-dwarfs).
- Add SED breakdown: separate `bolometric_luminosity`,
  `euv_flux`, `uv_flux`, `visible_flux`, `ir_flux`. Each
  evolves with main-sequence age.
- HZ edge migration with luminosity drift.
- Post-main-sequence: red-giant brightening (1000× over Myr
  at end-of-MS).

**Tests**:
- `m_dwarf_flare_rate_100x_g_dwarf`
- `habitable_zone_edge_migrates_outward_over_gyr`
- `red_giant_phase_renders_inner_planets_uninhabitable`

**Effort**: L — 30 hours (was M=16, +14 for SED + star types
+ HZ migration).

### Item 18a (new): EUV flux channel

**Why**: Per astro feedback — Item 17 needs EUV for hydrodynamic
escape; Item 18 must expose it.

**Approach**:
- `Star::euv_flux: Real`, evolves with main-sequence age.
- Young stars: 10-100× modern EUV.
- Drops with main-sequence age following `t^(-1.5)` (real
  stellar EUV decay).
- Read by Item 17 for hydrodynamic escape rate.

**Tests**:
- `young_star_high_euv_drives_hydrodynamic_atmosphere_loss`

**Effort**: S — 6 hours.

### Item 19 (new): Tidal-locking dynamics

**Why**: Per astro feedback — Tidal locking sets eccentricity
damping. Important for Items 16, 17. Currently `day_length_hours`
is a static worldgen-only value.

**Approach**:
- Per-tick tidal-locking force from each moon.
- Worldgen samples `locking_state ∈ {Synchronous, Resonance(p,q),
  FreeRotator}`.
- Locked worlds have permanent day/night faces; sub-stellar
  point is fixed.
- Damping of eccentricity over time toward circular orbit
  (unless resonance-pumped).

**Tests**:
- `tidally_locked_moon_eccentricity_damps_to_zero`
- `laplace_resonance_pumps_eccentricity_to_steady_state`
- `tidally_locked_planet_has_fixed_sub_stellar_point`

**Effort**: L — 32 hours.

### Item 20 (new): Magnetic-field reversals

**Why**: Per astro feedback — Earth's dipole flips every ~250 kyr.
Affects cosmic-ray ground flux + sky-glow + (via Item 17)
atmospheric escape.

**Approach**:
- Per-tick Markov chain for dipole reversal state: {Normal,
  Reversing, Reversed}.
- Reversal events take ~1000 ticks; during reversal, dipole
  weakens → cosmic-ray ground flux up → mutation rate up
  (couples to species drift in Item 11).
- Couples to atmospheric escape via Item 17 ion-channel.

**Tests**:
- `magnetic_reversal_occurs_on_average_every_250000_ticks`
- `reversal_event_weakens_field_for_1000_tick_window`
- `cosmic_ray_ground_flux_inverse_to_field_strength`

**Effort**: M — 20 hours.

### Item 21 (new): Planetary mass-radius-density coupling

**Why**: Per astro feedback — Currently `gravity` is a single
scalar; a high-G planet might be high-mass or high-density,
with very different escape velocities + atmospheric retention.

**Approach**:
- Replace `Planet::gravity` with `(mass, radius)` pair.
- Density derived per substrate: `density = substrate_density_constant`.
- `gravity = G × mass / radius²` (computed accessor).
- Escape velocity exposed for Item 17.
- Love number `k₂` for Item 16 depends on radius + substrate.

**Tests**:
- `gravity_correctly_derived_from_mass_and_radius`
- `escape_velocity_correct_for_earth_analog`
- `mass_radius_relation_per_substrate_yields_correct_density`

**Effort**: M — 18 hours.

### Item 22 (new): Vertical Coriolis (full 3D rotation)

**Why**: Per astro feedback — Existing Coriolis is 1D-q-only.
Full 3D rotation drives proper atmospheric circulation.

**Approach**:
- Replace `Ω_z` scalar with `(Ω_x, Ω_y, Ω_z)` vector.
- Couple to `VerticalConvection`.
- Enables proper Hadley/Ferrel emergence in Item 15.

**Tests**:
- `vertical_coriolis_component_active`
- `coriolis_3d_couples_to_vertical_convection`

**Effort**: M — 20 hours.

### Item 23 (new): Cloud microphysics

**Why**: Per astro feedback — Clouds drive actual albedo and
greenhouse coupling. Items 13/14 treat clouds as constant.

**Approach**:
- Per-cell cloud fraction derived from vapour saturation +
  vertical convection.
- Cloud albedo contribution to Item 13.
- Cloud greenhouse contribution to Item 14.
- Cloud type: low-albedo-high-greenhouse (cirrus) vs high-
  albedo-low-greenhouse (stratus) by altitude.

**Tests**:
- `cloud_fraction_rises_with_vapour_supersaturation`
- `cloud_type_albedo_greenhouse_correctly_signed`

**Effort**: L — 30 hours.

### Item 24 (new): Tidal-locking-state worldgen sampling

**Why**: Per astro feedback — Mercury 3:2, synchronous, free-
rotator regimes should be sampled at worldgen.

**Approach**:
- Sampler examines moon mass + orbital period + planet rotation
  rate; assigns one of {Synchronous, Resonance(p,q), FreeRotator}.
- Couples to Item 19 dynamics + Item 18 day_length.

**Tests**:
- `close_massive_moon_samples_synchronous_locking`
- `mercury_analog_samples_3_2_resonance`

**Effort**: S — 8 hours.

### Sprint 5 total

~310 hours (was 114). Sprint review: both experts, final
integrated review with the per-expert sign-off checklists.

## Cross-sprint concerns

(v1 unchanged plus additions)

### Performance

By v2 final sprint: multi-species (8-20 per planet) × full
ecosystem step + per-cell albedo + tectonics + 3-pass tide flux
+ 3D Coriolis + cloud microphysics + per-substance separations
all add per-tick work. Target relaxed to: 1080-cell grid
5000-tick run completes in < 10 min release-mode (was 5 min).

### Test scaling

By Sprint 5: ~80 new tests on top of existing 440. Run time
budget: full lib suite under 30s in debug, 2 min in release.
Long integration tests stay `#[ignore = "slow"]`.

### Substance enum growth

Sprint 2 adds `Substance::CO2`, splits `Vapour` → multiple.
Sprint 5 may add more (per substrate). Coordinate the enum
expansion in one PR to avoid merge churn.

### Determinism

New RNG draws per sprint:
- Sprint 2: 8-20 species per planet × multiple traits = ~200
  new draws per worldgen.
- Sprint 4: Voronoi plates = ~50 new draws per worldgen.
- Sprint 5: tidal locking state, magnetic reversal seed, etc.
  ~20 new draws.

Total worldgen RNG shift: ~270 new draws per planet (vs ~50
currently). Existing seeds rebaseline once per sprint.

## Verification gates

(unchanged from v1)

Per-sprint expert review (xeno / astro / both as appropriate)
must approve before next sprint starts. Final post-Sprint-5
review against per-expert sign-off checklists.

## Risk register (updated)

- **R1: Scope creep** (now realised) — backlog grew 18 → 24.
  Resist further growth unless review surfaces blockers.
- **R2: Determinism drift** — each sprint shifts seeds. Document
  rebaseline per sprint.
- **R3: Performance** — relax target to < 10 min/5000-tick.
  Profile per sprint.
- **R4: API churn** — Substance enum growth + Lifecycle variants
  + multi-species refactor are concurrent. Coordinate enum
  changes.
- **R5: Test scaling** — 80+ new tests. Group by sprint; reuse
  fixtures.
- **R6: Expert availability** — per-sprint review needs
  turnaround. Buffer.
- **R7: Save/load** — out of scope. Note in commits.
- **R8 (new): Calibration drift** — quantitative-anchor tests
  pin specific values (Io heat ~100 TW, snowball ice line
  ~30° latitude). Published values may change; track citation
  sources.

## Effort summary (v2)

| Sprint | Hours | Weeks (1 dev) |
|---|---|---|
| 1 (numerical hygiene) | 38 | 1 |
| 2 (xeno foundation) | 286 | 6 |
| 3 (xeno + astro coupling) | 242 | 5 |
| 4 (astro tectonics) | 138 | 3 |
| 5 (closing gaps) | 310 | 7 |
| **Total** | **1,014** | **22** |

Wait — that's more than the previous summary. Let me account:
- Sprint 2 includes the major Item 6 rewrite + new 6a/6b + Item
  7 expansion + new 7a/7b + Item 8 expansion + Item 9 expansion.
- Sprint 5 includes major Items 15/16/17/18 rewrites + new
  18a/19/20/21/22/23/24.

The honest estimate is ~22 weeks single-dev. Substantial
parallelism possible — most Sprint 5 items independent;
Sprint 2 has 6 vs 6a vs 6b serial but 7, 7a, 7b, 8, 9 mostly
parallel.

**Realistic schedule: 14-18 weeks** with reasonable parallelism
(2-3 parallel implementers).

## Open questions to settle before kickoff

(unchanged from v1)

- Q1: Save/load — in scope or follow-up?
- Q2: Performance targets — confirm < 10 min/5000-tick.
- Q3: Expert review cadence — sync or async?
- Q4: Test coverage minimum.
- Q5: Documentation — rendered visualisations?
- **Q6 (new): Substance enum stability** — frequent enum changes
  break match exhaustiveness. Coordinate.
- **Q7 (new): Calibration source-of-truth** — which published
  values are authoritative for the quantitative-anchor tests?

Resolve before Sprint 1 PR opens.

## Closing acceptance

After Sprint 5, both expert sign-off checklists from
`docs/expert-review-roadmap.md` must pass. Plus, both experts
must re-review the final v2 plan to confirm completeness
before kickoff.
