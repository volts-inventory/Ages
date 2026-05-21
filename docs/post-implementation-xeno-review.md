# Post-implementation xenobiologist review (Items 1-24)

Reviewer lens: xenobiology. Scope: 35 PRs landing Items 1-24 of
`docs/implementation-plan.md` (v2). All file references are
absolute paths into the production tree.

## Overall assessment

The 35-PR programme has produced a substantial body of biology
code: multi-species ecosystems with typed roles, a Lindeman
pyramid, Holling functional responses, Brandes betweenness for
keystone detection, seven lifecycle variants, tolerance envelopes,
dormant seed banks, five-trigger speciation with allometric drift,
HGT, a CO2 biogeochem channel, and a four-way cognition topology.
As a library, it is genuinely impressive — the right *shapes* are
present. But the planet the user actually simulates does not see
any of it. **The `sim-ecosystem` crate is not a dependency of
`sim-core`, `sim-civ`, `sim-world`, or the `ages` binary** (see
`/home/user/Ages/sim/core/Cargo.toml:12-22` and the workspace
graph). Every species, interaction matrix, biogeochem coupling,
speciation event, HGT trial, and extinction sweep lives downstream
of code paths that production runs never reach. The same holds for
`sim_population::lifecycle::step_for_lifecycle` — called only from
its own unit tests; production civ ticks bypass it (see
`/home/user/Ages/sim/civ/src/capacity.rs:154, 171, 254`). I cannot
sign off on "multi-species credible biology" when the binary still
runs single-species, vertebrate-schema-only, Fuel-mediated biology.
Status: NEEDS_REWORK.

## Critical gaps

Ranked by severity. S=small, M=medium, L=large, XL=sprint-scale.

### C1 [XL] — `sim-ecosystem` is unwired from production

The entire Sprint 2 + Sprint 3 xeno foundation is dead code on the
binary path. `PlanetEcosystem::step` and
`step_with_biogeochem_at_tick`
(`/home/user/Ages/sim/ecosystem/src/lib.rs:267, 319`) are exercised
only by `sim-ecosystem`'s own tests. Sprint 2 acceptance ("a
typical planet hosts 5-15 species; civ-bearing species sees
population pressure modulated by producer-tier biomass") is **not
met in production** — only in standalone tests. Fix: add
`sim-ecosystem` as a dep of `sim-core`; instantiate
`PlanetEcosystem` per planet in worldgen; call the biogeochem step
inside the main law list; surface `SpeciationOccurred`,
`SpeciesExtinct`, HGT events through the existing `Emitter`.

### C2 [XL] — Civ carrying capacity does not read producer biomass

`Civ::carrying_capacity` (`/home/user/Ages/sim/civ/src/capacity.rs:
40-60`) sums per-cell `Substance::Fuel`. Producers can crash to
zero in `PlanetEcosystem` and the civ doesn't feel it. Net effect:
cascading extinctions and keystone collapses cannot starve a
civilisation — the whole point of multi-trophic modelling.

### C3 [L] — Lifecycle dispatch never runs in production

`sim_population::lifecycle::step_for_lifecycle`
(`/home/user/Ages/sim/population/src/lifecycle.rs:37`) is the
public dispatcher for the seven `Lifecycle` variants. Production
calls `dynamics.step_with_capacity(...)` directly
(`capacity.rs:154, 171, 254`). Every civ — eusocial, microbial,
modular, plant, insect, aquatic-semelparous — runs the vertebrate
4-bracket step. An ant-civ with caste structure simulates as a
vertebrate. The Item 7 acceptance test ("an insect-equivalent has
a pupa bracket with zero food multiplier and zero reproduction")
is unit-tested in `lifecycle.rs` but does not affect production.

### C4 [L] — `ToleranceEnvelope` is decorative on the catastrophe path

`apply_catastrophe` (`/home/user/Ages/sim/species/src/sampling.rs:
1146`) is documented "synthetic for now (no full catastrophe
pipeline yet)." Production catastrophe wiring at
`/home/user/Ages/sim/civ/src/catastrophe/mod.rs:91-102` calls
`apply_catastrophe_with_dormancy` but never reads
`species.tolerance`. An extremophile with `radiation_max = 20`
survives a solar flare exactly as much as a narrow-envelope
aqueous species. Item 7a passes its unit tests in
`species/src/tests.rs:170-280` but does not affect any production
tick.

### C5 [M] — `MutualismKind` and `ParasiteKind` are tag-only enums

`Pollinator | SeedDisperser | Engineer | Generic` all run the
identical mutualism delta
(`/home/user/Ages/sim/ecosystem/src/lib.rs:676-685`). `Macro |
Micro | Virus` parasites likewise behaviourally identical
(lines 1124-1143 are pure flavour tags). Same critique the v1
review made about `CognitionTopology` ("enum-only flavor") that
Item 10 was supposed to dissolve — but here it re-emerges for the
other typed enums. Engineers should be HabitatModification on
substrate; viruses should have host specificity and episodic
outbreaks.

### C6 [M] — No astro-to-mutation pathway

Item 20 surfaces `state.cosmic_ray_ground_flux()`
(`/home/user/Ages/sim/physics/src/state.rs:751`). The
implementation-plan promised this drives mutation rate up during
reversals → couples to Item 11 speciation. Searching biology
crates for any consumer returns zero hits. `divergence_pull` in
`speciation.rs` has no UV/EUV/cosmic input. Magnetic reversals
are atmospherically interesting and biologically inert. Solar
flares fire in the catastrophe layer
(`/home/user/Ages/sim/civ/src/catastrophe/triggers.rs:64`) but
their pop-loss is a fixed 10% — independent of species, of
tolerance, of EUV magnitude. The astro layer is not driving the
biology.

### C7 [M] — Dormancy reservoir is never seeded by catastrophes

`DormantPool::resurrect_step`
(`/home/user/Ages/sim/species/src/types.rs:534`) implements the
seed-bank mechanism, but `apply_resistance_and_dormancy`
(`/home/user/Ages/sim/civ/src/catastrophe/mod.rs:91`) uses
dormancy as a damage-reduction multiplier and never *populates*
the dormant pool with the surviving-but-dormant fraction. So
tardigrade-grade species shrug off the catastrophe but their
slow-resurrection reservoir is never created. The mass-extinction
recovery scenario is not representable.

## Shallow implementations

- **S1 — Single half-saturation across all predator-prey pairs**.
  `K_HALF_SAT = 0.5 × producer_capacity`
  (`ecosystem/src/lib.rs:65`) shared globally; realistic values
  vary 0.1×-0.4× by pair. Breaks the "Lotka-Volterra cycles match
  published values" claim Item 6 acceptance relies on.
- **S2 — Lindeman 10:1 is both a hard cap and an assimilation
  efficiency**. `enforce_lindeman_pyramid`
  (`ecosystem/src/lib.rs:721`) scales each tier ≤ 0.1× the lower
  tier post-step; predation also assimilates at 10% (line 662).
  Double-bookkeeping: a calibrated assimilation should make the
  cap redundant. The single ratio across all habitats is also a
  hazard — aquatic systems run 30:1.
- **S3 — Producers compete for sunlight by simple division**.
  `grow_producers_with_co2` (`ecosystem/src/lib.rs:527-558`)
  divides `solar_irradiance / n_prod` equally; real photoautotroph
  competition is by canopy stratification and shade tolerance.
- **S4 — Speciation daughters don't ecologically diverge**.
  `derive_daughter_species` (`ecosystem/src/speciation.rs:524`)
  produces allometry-correlated trait deltas (good), but parent
  and daughter run identical interaction matrices. No character
  displacement — sister species shouldn't compete for identical
  resources at full strength, but they do.
- **S5 — HGT is smooth interpolation, not selection**.
  `step_hgt` (`/home/user/Ages/sim/ecosystem/src/hgt.rs:98`)
  interpolates traits at 5%/trial. Real prokaryote HGT is a
  selection event: a plasmid sweeps or it's lost. No payload, no
  sweep, no plasmid loss under sub-optimal conditions.
- **S6 — Collective quorum is one-dimensional**. `drift.rs:16`
  defines a single total head-count threshold; real eusocial
  colonies have *caste* quorums — 10,000 queens with 0 workers is
  functionally extinct but reads "above quorum."

## Missing tests

- **Apex predator removal cascade**: `extinction_cascade_from_
  keystone_removal` (`ecosystem/src/tests.rs:975`) removes a
  *producer*, not an apex; mesopredator-release dynamics
  uncovered.
- **Mass extinction recovery / adaptive radiation end-to-end**:
  the rate-5× test verifies the multiplier in isolation, not the
  full "extinction clears species → 100-generation burst fills
  vacated roles" shape. The speciation step doesn't read
  post-extinction biomass to direct radiation into empty roles.
- **CO2 thermostat closure over long runs** (Items 6b+14a+12d
  jointly): no integration test runs ecosystem+atmosphere for
  tens of kiloticks asserting CO2 bounded. (Given C1, such a
  test cannot run today.)
- **Producer collapse → civ famine**: direct consequence of C2.
- **Allopatric speciation across a tectonic event** (Items 11+12):
  subduction destroys land bridge → populations isolate →
  speciation. The two systems don't share a coordinate frame.
- **HGT under syntrophy**: methanogen acquires H2-producer
  pathway, breaks the dependence. Not represented.
- **Niche partitioning under competition**: exclusion is tested
  (`tests.rs:392`), coexistence under resource partitioning is
  not.
- **Cognitive-topology divergent tech trajectories**: Item 10
  promised "identical cognition + different topology = different
  trajectories"; tests check single-axis multipliers, not
  end-to-end divergence.

## Calibration drift risks

Invariant #7 promised quantitative-anchor tests; several systems
still test direction only.

- **Producer growth 2%/tick** (`ecosystem/src/lib.rs:70`) — no NPP
  anchor per Earth-analog seed.
- **Consumer decay 1%/tick** (lib.rs:75) — fixed across body
  sizes; metabolic-scaling (mass^0.75) absent.
- **K = 0.5 × capacity** — see S1.
- **Lindeman 10:1** — terrestrial-vertebrate-appropriate;
  unrepresentative for aquatic systems.
- **Microbial doubling test** (`population/src/lifecycle.rs:467`)
  asserts ±5% but tick=1 month is wildly off for real binary
  fission (20 min – hours). Step is calibrated to tick cadence,
  not biology.
- **Polyploidy 1/100000** (`speciation.rs:76`) — tuned for test
  cadence, not anchored to published plant speciation rates.

## Deferred items

- **Archaea as a distinct lifecycle**. Folded into Microbial via
  `Fission` variants. Their ether-linked membrane chemistry and
  extremophile dominance want first-class representation,
  especially on pre-photosynthetic CO2-poor seeds.
- **Secondary endosymbiosis / acquired organelles**. Mitochondria
  / plastids / kleptoplasty are one-shot evolutionary leaps that
  reshape the producer-consumer landscape qualitatively. Sprint
  2 gave Mutualism but not the merger event.
- **Body-size allometry within demography**. Allometry matrices
  drive between-species drift; the per-tick population step is
  body-size-agnostic — no `mass^0.75` metabolic scaling on food
  demand.
- **Pathogen co-evolution / Red Queen dynamics**. Parasite
  interaction strengths are static; no host-pathogen arms race.
- **Decomposer biomass not driven by dead-biomass flux**.
  Saprotrophs return CO2 correctly but their biomass cap is the
  same logistic as any consumer, not tracking detritus
  availability.

## New items

- **N1 [XL] — Integrate `sim-ecosystem` into the production
  tick**. Add as `sim-core` dep; instantiate `PlanetEcosystem` per
  planet at worldgen via `sample_ecosystem`; drive
  `step_with_biogeochem_at_tick` from `sim_core::Orchestrator`;
  forward extinction / speciation / HGT events through `Emitter`.
- **N2 [L] — Wire producer biomass into civ carrying capacity**.
  Replace `Substance::Fuel` sum in `Civ::carrying_capacity` with
  `PlanetEcosystem::tier_biomass(0)` scaled to claimed cells.
  Adds the `ecological_resilience` scalar Item 6 promised.
- **N3 [L] — Plumb `step_for_lifecycle` through the civ step**.
  Replace `dynamics.step_with_capacity` at `capacity.rs:154, 171,
  254`; store variant-specific state (caste counts, microbial /
  modular biomass) on `Civ` rather than synthesising at call
  site.
- **N4 [M] — Per-cell tolerance gating on habitat occupancy**.
  Multiply per-cell demographic capacity by
  `species.tolerance.match_score(...)` against cell conditions.
  Extremophile niches don't fill until this lands.
- **N5 [M] — Cosmic-ray flux → mutation rate**. Bind
  `state.cosmic_ray_ground_flux()` to a multiplicative divergence
  term in `step_speciation`'s allopatric + sympatric paths.
  Likewise on solar-flare events.
- **N6 [M] — Per-pair half-saturation**. Add
  `half_saturation: Real` to `Interaction`; calibrate canonical
  pairs (wolf-deer, lynx-hare) against published values.
- **N7 [S] — Solar-flare differential survival by tolerance**.
  `catastrophe/mod.rs` should call `tolerance.radiation_score`
  against post-flare cell radiation; replaces flat 10%
  `SOLAR_FLARE_POP_LOSS`.
- **N8 [L] — Differentiated MutualismKind / ParasiteKind step**.
  Pollinators couple to plant clutch size; engineers shift
  per-cell match_score; viruses apply to one host species with
  episodic amplification; macro vs micro vary host-mortality.
- **N9 [M] — Apex-down + endosymbiosis tests**. Add
  `apex_removal_releases_mesopredator_biomass`,
  `adaptive_radiation_after_mass_extinction_repopulates_vacated_
  roles`, and a `secondary_endosymbiosis_creates_mixotroph_
  lineage` once the merger event exists.
- **N10 [L] — Calibration anchors**. Producer NPP per
  Earth-analog seed vs published GPP; microbial doubling per
  solvent chemistry; polyploidy rate per Myr vs literature.

## Approval status

**NEEDS_REWORK.**

The data structures and per-tick math for a credible xenobiology
layer have largely been written, and several pieces (speciation,
HGT, biogeochemistry) would pass a domain expert's review *in
isolation*. But the integration step that converts "a library in
`sim/ecosystem`" into "a planet that actually has multiple species
interacting per tick under the run's catastrophes, mutations, and
tectonic events" did not happen. The production binary still
simulates the same single-species, vertebrate-bracketed,
Fuel-substance-mediated biology that the expert review flagged as
inadequate at Sprint 0. Items 6, 6a, 6b, 7, 7a, 7b, 9, 11, 11a —
all the xeno items — pass their own crate's tests but do not run
on the live planet. Until C1-C4 are addressed (sim-ecosystem
integrated, civ capacity reads producer biomass, lifecycle
dispatch wired, tolerance envelope on catastrophe survival), the
xenobiologist sign-off checklist in
`docs/expert-review-roadmap.md` items 1-5 cannot be honestly
checked off.

Re-review when N1-N3 land at minimum, with a runtime-trace
demonstrating that a live planet's `PlanetEcosystem::step` fires
each tick and produces species-level events visible in the report
layer.
