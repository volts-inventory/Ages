# Expert-review roadmap

Integrated backlog from the multi-pass expert review series
(reviews 1–5). One reviewer per lens, plus a roll-up reviewer that
sequenced the priorities.

## Status

Through PRs #21–#30, the project has shipped:

- Cell-capacity formula reworked: gravity dropped, cognition
  widened, biosphere double-count removed, seasonal smoothed
  against substrate `temperature_range`, grid-resolution-invariant.
- Migration + spread: balanced cohort migration, expansion uses
  base threshold (not augmented), expansion seed floored, tech-
  augmented threshold tightened.
- Viewport: density-by-civ-color, nomads white, log clarity.
- Tech tree widened: 59 → 71 authored tools + uncapped per-channel
  emergent refinement.
- Four numerical/foundation bugs from review 2: tide declination
  wired, habitat tables reconciled, donor-limited tide flux,
  perturbed `CognitionAxes`.
- Physics wave: wind density coupling complete, energy-conserving
  advection (column-mass-weighted), wind-coupled gravity flow,
  conservation invariants in debug builds, hemisphere refactor
  documenting the convention disagreement, saturation-pressure
  hydrology retry, alliance formation + dissolution + cooldown,
  sensor-aware discovery wired through production (with new
  `Channel::MagneticField`), Q32.32 saturating arithmetic, K-end
  birth-rate calibration via `reproductive_success` factor.

What's left is what review 5 surfaces: the calibrations need
honesty extended past the K endpoint; tides need real two-pass
order-independence; xenobiology + astrophysics each have
substantial subsystem gaps that the current sim asserts in
comments but doesn't realise in math.

## Critique by lens

### Physics

Solid bones: pair-flux conservation, Q32.32 determinism,
operator-split orchestration with debug-asserted invariants. Wind
+ tide + hydrology + EM all wired and verified.

Outstanding (review 4 numerical issues):

- `events_per_fertile_window` calibration only fixed at the K
  endpoint; mid-axis produces 6,300 lifetime offspring, r-end
  undershoots salmon by 40×.
- Donor-limited tide flux is order-dependent under aggressive
  `tide_k`; bulge symmetry breaks. Test exists (regression
  canary), fix doesn't.
- Wind CFL guard clamps `|v|` instead of subdividing `dt` —
  distorts thin-atmosphere dynamics.
- Per-cell vapour cap has a flat 10,000 floor that's independent
  of cell saturation pressure; dry cells can hold "atmospheric
  moisture" they physically shouldn't.
- Hydrology conservation tolerance is per-tick only; doesn't
  bound cumulative drift over long runs.

### Civilization

Foundations real: alliances form + dissolve with cooldown,
cosmology + religion drive epistemic suppression, multi-civ
contact/war/cohesion all wired, conservation invariants on
substance transport.

Outstanding (carried forward from earlier reviews):

- Economy is scalar `surplus` diffusion — no goods, no trade-
  driven specialisation, no comparative advantage.
- Cohesion is one-dimensional — no regional fragmentation
  (centroid always survives; "periphery secedes" not
  representable).
- War only fires on territorial overlap — no resource / ideology /
  dynastic claim wars.
- Religion → epistemics is wired but not heavily exercised by
  consumers.
- Cross-civ migration: refugees on collapse exist as a stateless-
  population pool; ongoing diasporas / cross-civ Völkerwanderung
  don't.

### Xenobiology

**Verdict: the sim is physiology, not biology.** A xenobiologist
would push back on:

- `Substance::Water/Ice/Vapour` is the solvent regardless of
  substrate. A silicate-substrate world's "Vapour" is rock vapor
  at 1500 K but uses the same Clausius-Clapeyron formula and
  latent heat constants modulo a substrate lookup.
- `Substance::Oxidiser` is O₂-shaped universally. Ammonia-substrate
  life has to use N₂O₄ or some real oxidiser, but the chemistry
  kernel treats Oxidiser as a generic "burns with fuel" reagent.
  Combustion at 1:1:1 stoichiometry is a hack ignoring molar
  masses.
- **One species per planet.** Real biospheres are multi-trophic
  webs. No producer/consumer/decomposer, no symbiosis, no
  mutualism, no parasitism, no niche partitioning.
- `PopulationBiology` bracket schema is vertebrate-mammalian
  (infant/juvenile/fertile/elder). An insect has egg/larva/pupa/
  adult with the pupa as a non-feeding stage. A plant has seed/
  seedling/mature/senescent. A modular colonial organism (coral,
  mycelium, slime mold) doesn't have brackets — it has biomass.
  Forcing every species through one schema discards body-plan
  diversity.
- `CognitionTopology::{Centralized, Distributed}` is enum-only
  flavor. No behavioural fork. A distributed swarm mind and a
  centralized vertebrate brain run identical hypothesizer code.
- No speciation, no evolution. Species traits drift but the
  species doesn't *split*.
- No redox alternatives. Earth has chemolithotrophs (sulfur,
  iron, methane oxidation); a real xenobio sim needs alternative
  electron acceptors as first-class chemistry.

### Astrophysics

**Verdict: atmospheric simulation on a planet that doesn't
change.** Solid weather, missing geology and long-timescale
dynamics. An astrophysicist would push back on:

- **No tectonics.** `state.elevation` is "permanently deferred
  from physics mutation". On the project's geological timescales,
  every planet's geography is freeze-frame forever.
- **No greenhouse runaway / snowball states.** Climate is
  essentially steady-state radiation balance. No positive feedback
  loops that produce bistable climate.
- **No ice-albedo feedback.** Albedo is per-atmosphere-class
  constant, not per-cell.
- **No tidal heating.** Io is heated by Jupiter's tidal flexing.
  No internal-friction heat source on the rock.
- **No Hadley/Ferrel/polar cells.** Wind is pressure-gradient +
  Coriolis but doesn't organize into three-cell circulation.
- **No atmospheric escape.** Light atmospheres (H₂, He) should
  escape Jeans-style. Mars-thin worlds should be thin *because*
  they lost atmosphere over Gyr.
- **No stellar variability.** `stellar_luminosity` is constant.
  Real stars flare, have sunspot cycles, evolve through main
  sequence (luminosity changes 2-3× over Gyr).
- **No magnetic reversals.** Earth's dipole flips every ~250 kyr.
- **Planet has gravity but no radius.** A high-G planet might be
  high-mass or high-density — these have different escape
  velocities, different atmospheric retention.
- **Coriolis is 1D-q-only.** Vertical component drives 3D
  circulation; the 2D approximation combined with missing Hadley
  organisation produces unphysical wind patterns.

## Integrated backlog

Sequencing rules:

- **Tier 1** ships first — small fixes, unblock confidence,
  cheap parallelism.
- **Tier 2 + Tier 3** can largely run in parallel — they touch
  different subsystems (biology vs planetary physics).
- **Multi-week items** flagged with effort estimates so the
  scheduler knows when to commit a full sprint vs slot a single
  PR.

Format: `**Item** (effort) — Why it matters. Acceptance: …`

### Tier 1: numerical / immediate fix (from review 4)

#### 1. Reproductive_success curve shape (S)

Why: PR #30 fixed K-end overshoot (~500×) but the linear-success
× parabolic-clutch product creates a mid-axis explosion (6,300
lifetime offspring at r=0.5) and undershoots the r-end (120 vs
salmon's ~5,000).

Fix: either (a) make `reproductive_success` quadratic in `r`:
`0.005 × (1-r)² + 0.10 × r²`, or (b) keep linear success but
raise `clutch_size` cap at r=1 to ~5,000.

Acceptance: companion realism tests for mid (r=0.5) and r-end
(r=1.0) seeds alongside the existing K test. Lifetime offspring
within plausible ranges for the species archetype.

#### 2. Two-pass donor-limited tide flux (M)

Why: PR #23's single-pass donor cap is order-dependent. Pair
iteration order determines which neighbour gets full flux when
both want to pull from the same donor. Bulge symmetry breaks
under aggressive `tide_k`. P6's symmetry test passes today
only because earth-like `tide_k` is too low to engage the cap.

Fix: pass 1 computes desired outflows against `prev_w` without
donor check. Pass 2 applies pro-rata scaling when desired outflow
exceeds donor stock. Mass-conservative + order-independent +
no clamps. ~30 LoC.

Acceptance: existing symmetry test passes at `tide_k = 0.5`
(currently calibrated at 0.02). Add `tide_redistribution_order_
independent` test that runs the same physics in two different
cell-ID orderings and asserts identical water_depth.

#### 3. Adaptive dt sub-stepping in wind (M)

Why: PR #30's `|v_q|, |v_r| < 0.5 / (advect_k × dt)` clamp at
`wind.rs:215-233` is a band-aid. Real CFL stability requires
subdividing dt when the gradient demands speed beyond CFL, not
clamping the velocity itself. Clamping caps wind speed regardless
of physical driver and breaks energy/momentum balance.

Fix: post-acceleration, compute `cfl_max = max(|v_along × advect_k|)
× dt`. If > 0.5, halve dt and run wind twice (or N times). Pair-
flux conservation preserved across sub-steps.

Acceptance: existing `wind_advection_conserves_energy_under_
varying_column_mass` still passes. New test:
`wind_subdivides_dt_under_high_pressure_gradient` confirms wind
sub-steps under a strong gradient instead of clamping.

#### 4. Saturation-curve vapour cap floor (S)

Why: PR #30 made cap per-cell (good) but kept the 10,000 flat
floor (bad). A 200 K Hydrocarbon-world desert cell has very
little water-vapour-equivalent capacity; a 350 K hot ocean coast
has lots. The floor flattens this.

Fix: derive per-cell cap from temperature + atmospheric scale
height. `cap = max(water_depth × 100, saturation_capacity(T,
substrate, scale_h))`. Drop the flat 10,000 floor.

Acceptance: existing pathological-overload test still triggers
the cap. New test: `vapour_cap_scales_with_temperature` asserts
hot cells get higher caps than cold cells at equal water_depth.

#### 5. Cumulative hydrology mass assert (S)

Why: PR #30's drift tolerance is per-tick. Small drifts can
accumulate over thousands of ticks (especially when the vapour
clamp fires steadily under extreme substrates). The per-tick
assert won't catch a 10% mass loss over 16k ticks.

Fix: separate cumulative-drift accumulator (`Σ |post_wv -
pre_wv|`) in `Orchestrator`. Run-end check against a growth-
bounded ceiling (e.g., 5% of starting total). ~10 LoC.

Acceptance: cumulative drift over 16k-tick canary run stays
< 5% of starting mass. Earth-like seeds drift < 0.01%.

### Tier 2: xenobiology

#### 6. Multi-species ecosystems (XL)

Why: the biggest xeno gap. Real biospheres are multi-trophic
webs. The sim has one species per planet, so there's no
ecology — no producer/consumer/decomposer, no symbiosis,
mutualism, parasitism, niche partitioning. Ecological collapse
(keystone-species extinction → cascade) isn't representable.

Fix: per-planet `Vec<Species>` (was single `Species`). Add
trophic-role enum: `Producer`, `Consumer`, `Decomposer`,
`Mutualist`. Per-pair interaction matrix: predation strength,
mutualism strength, competition strength. Each species has its
own population pool; civs feed off the producer tier; ecosystem
stability gates demographic stability via a new
`ecological_resilience` scalar.

Effort: 1-2 weeks. Touches species/world/civ/core. Justified by
the magnitude of the unlock.

Acceptance: a typical planet hosts 5-15 species. Civ-bearing
species sees population pressure modulated by producer-tier
biomass. Ecological catastrophe (e.g., producer-tier collapse →
consumer-tier dies → civ starves) becomes a real failure mode.

#### 7. Lifecycle-topology variants (L)

Why: `PopulationBiology` bracket schema forces every species
through a vertebrate-mammalian infant/juvenile/fertile/elder
pipeline. Real organisms have egg/larva/pupa/adult, seed/
seedling/mature/senescent, or no brackets at all (modular
colonial organisms have biomass + budding).

Fix: `PopulationBiology` gains `Lifecycle` enum (`Vertebrate`,
`Insect`, `Plant`, `Modular`). Each variant carries its own
bracket schema and step function. Sampler picks Lifecycle from
body-plan / manipulation-mode signals at species genesis.

Effort: ~1 week. Touches species + population + civ aggregators.

Acceptance: a sampled insect-equivalent species has a pupa
bracket with zero food multiplier and zero reproduction (it's
metamorphosing). A modular species step function tracks biomass
not brackets.

#### 8. Substrate-coupled solvent semantics (L)

Why: `Substance::{Water, Ice, Vapour}` is named for water but
relabeled per-substrate. A silicate-substrate world's "Water" is
liquid silicate; the chemistry uses water-derived latent heat
divided by a substrate lookup. This is naming, not physics.

Fix: rename `Water/Ice/Vapour` to `Solvent::{Liquid, Solid,
Gas}` (or split per-substrate substance types if the renames
break too many APIs). Chemistry kernel reads substrate-specific
latent heat, density, surface tension. Per-substrate equations
for phase transitions.

Effort: ~1 week. Touches chemistry kernel + world init +
recognition templates + ~30 call sites.

Acceptance: A silicate-substrate world's solvent freezes/boils
at substrate-appropriate temperatures with substrate-appropriate
latent heat. Recognition templates that look for "water-equivalent
phenomena" work for any substrate.

#### 9. Alternative redox chemistry (L)

Why: `Substance::Oxidiser` is O₂-shaped universally. Ammonia-
substrate life needs N₂O₄. Earth has chemolithotrophs using
sulfur, iron, methane oxidation. Combustion at 1:1:1 stoichiometry
is a hack ignoring molar masses.

Fix: per-substrate oxidiser. Add `Substance::AltOxidiser` (or
split per substrate). Per-substrate combustion stoichiometry +
ignition threshold. `LocalisedCombustion` tool gates on whichever
oxidiser the planet's atmosphere actually has.

Effort: ~1 week. Touches chemistry + recognition templates +
tools.

Acceptance: a CO₂-atmosphere planet (no O₂) supports combustion
via an alternative chemical pathway if one exists, or genuinely
locks combustion out if it doesn't. The current "stuck at 22
tools" pattern goes away.

#### 10. Cognitive topology as real mechanic (M)

Why: `CognitionTopology::{Centralized, Distributed}` is read in
exactly one place (event payload string). A swarm-mind species
and a vertebrate-brain species run identical hypothesizer code.

Fix: Distributed civs get faster parallel discovery (multiple
candidate fits per tick) but slower formal abstraction (lower
ceiling on `Form::Polynomial2/3` confidence). Centralized civs
the inverse. Wire `CognitionAxes` differently per topology — a
Distributed species derives `social` axis dominantly, a
Centralized one derives `abstraction` dominantly.

Effort: ~3-4 days. Touches species sampler + hypothesizer step
+ form selection.

Acceptance: two seeds with identical scalar `cognition` but
different topology show measurably different tech-tree
trajectories (Distributed reaches sensor tools faster; Centralized
reaches Mathematics-tier faster).

#### 11. Speciation / evolution events (L)

Why: species traits drift but never *split*. Real evolution has
speciation as the headline process.

Fix: when environmental pressure (catastrophe streak, climate
shift, isolation by water-crossing) exceeds a threshold, the
species splits into two with divergent trait pulls. New species
enters the planet's `Vec<Species>` registry. Allopatric (geographic)
+ sympatric (niche) speciation paths.

Effort: ~1 week. Touches species + world + civ inheritance.
Depends on #6 (multi-species ecosystems).

Acceptance: A long run (10,000+ ticks) on a high-pressure seed
produces ≥2 species; their trait distance grows over time;
descendant civs inherit the daughter species' traits, not the
parent's.

### Tier 3: astrophysics

#### 12. Tectonics + erosion (XL)

Why: biggest astro gap. `state.elevation` is "permanently
deferred from physics mutation". On geological timescales, every
planet's geography is freeze-frame forever. No mountain uplift,
no erosion, no sediment, no continental drift.

Fix: new `tectonics` law (mantle convection-driven uplift at
plate-boundary cells) + new `erosion` law (sediment transport
coupled to `GravityFlow`'s water_depth changes, mountain →
plain over Myr). Both mutate `state.elevation()`. Probably needs
a per-cell `crustal_thickness` field.

Effort: 1-2 weeks. New subsystem. Touches physics + world.

Acceptance: a 50,000-tick run on a young-planet seed shows
measurable elevation change (mountains erode toward the sea,
new mountains uplift at active boundaries).

#### 13. Ice-albedo feedback (M)

Why: albedo is per-atmosphere-class constant. Real ice cells
reflect ~80%; ocean cells absorb ~95%. Without per-cell albedo,
no snowball state.

Fix: per-cell albedo = base(atmosphere) + ice_fraction × ice_
boost. `Radiation::integrate` reads per-cell albedo. Snowball
states emerge when high-ice cells reflect insolation back to
space.

Effort: 2-3 days. Touches radiation + state.

Acceptance: a cold seed run shows bistable climate — once ice
coverage exceeds a threshold, equilibrium drops to "snowball"
(near-uniform ice cover, very low temperature). Test:
`ice_albedo_feedback_produces_snowball_state` confirms cold
worlds slide into snowball.

#### 14. Greenhouse runaway / snowball bistability (M)

Why: climate is steady-state radiation balance. Real climate has
positive feedback (high vapour → more greenhouse → warmer →
more vapour → runaway; cold → ice → high albedo → colder →
snowball). The sim has neither basin.

Fix: per-cell greenhouse offset = base(atmosphere) + vapour ×
vapour_greenhouse_coefficient. Couples vapour density back into
the radiation law. Combined with #13 (ice-albedo), gives both
hot and cold runaway basins.

Effort: 2-3 days. Touches radiation + couples to hydrology
vapour field.

Acceptance: a hot seed run can slide into "Venus state" (near-
uniform high temperature, dense vapour atmosphere). A cold seed
can slide into snowball. Both basins are stable for thousands
of ticks once entered.

#### 15. Hadley/Ferrel cells via meridional circulation (L)

Why: wind is local pressure-gradient + Coriolis. Real
atmospheres organize into three-cell structure (rising at
equator, descending at ~30°, rising at ~60°, descending at
poles). Without this, "wind" is local gradient relaxation, not
planetary circulation.

Fix: add a meridional (r-direction) overturning term to wind
that creates the three-cell pattern under rotation. Driven by
equator-pole temperature gradient. Probably needs a vertical-
convection coupling that the existing `VerticalConvection` law
can extend.

Effort: ~1 week. Touches wind + vertical convection + climate.

Acceptance: under sustained equator-pole gradient, wind develops
the three-cell zonal pattern; r-direction flow at descending
latitudes is poleward, rising-latitude flow is equatorward.

#### 16. Tidal heating (M)

Why: Io is heated by Jupiter's tidal flexing. Moons orbiting
giants should generate significant internal heat from tidal
friction. The sim has tides on `water_depth` but no internal-
friction heat source.

Fix: new term in `Tides::integrate` that adds a per-cell heat
source proportional to `Σ moons (mass × period⁻³ × eccentricity²
× local_potential_gradient²)`. Feeds into `state.temperature_mut()`.

Effort: ~3 days. Touches tides + state.

Acceptance: a planet with a massive close moon (e.g., Io-like
configuration) shows measurable temperature elevation from
tidal heating, independent of stellar insolation.

#### 17. Atmospheric escape (M)

Why: light atmospheres (H₂, He) escape Jeans-style at rates
depending on gravity + temperature + molecular mass. Mars-thin
should be thin *because* it lost atmosphere over Gyr.

Fix: per-tick atmospheric mass loss derived from per-substance
molecular weight + cell temperature + gravity. `surface_pressure`
decays slowly. Composition shifts (lighter molecules escape
first). Probably wired through `Atmosphere` rather than per-cell.

Effort: ~1 week. Touches atmosphere + world over geological
timescales.

Acceptance: a low-gravity hot planet shows measurable atmospheric
thinning over a 50,000-tick run. A high-gravity cold planet
retains atmosphere indefinitely.

#### 18. Stellar variability (M)

Why: `stellar_luminosity` is constant. Real stars flare,
have sunspot cycles, evolve through main sequence (luminosity
changes 2-3× over Gyr).

Fix: `stellar_luminosity` becomes time-varying. Slow secular
evolution (faint-young-Sun → modern → eventually post-main-sequence
brightening). Fast variation (sunspot cycle on decadal scale,
flares on per-tick stochastic). Hook flare events into the
catastrophe layer.

Effort: ~4-5 days. Touches radiation + catastrophe.

Acceptance: a long run shows stellar luminosity drift; flare
events fire as catastrophes; very long runs show the star
evolving toward red-giant stage.

## Dependency graph

Items with prerequisites:

- **#11 speciation** depends on **#6 multi-species** (needs a
  registry to add new species to).
- **#14 greenhouse runaway** depends on **#13 ice-albedo** (both
  need per-cell albedo / greenhouse, and bistability needs both
  basins to be implementable in the same radiation law).
- **#15 Hadley cells** depends on **#3 adaptive wind dt** (the
  three-cell structure needs longer-timescale stable integration;
  current CFL clamp would distort it).
- **#12 tectonics** depends on nothing in this list but needs
  the existing fluid-momentum work to handle erosion's water-
  driven sediment transport.
- **#17 atmospheric escape** depends on **#18 stellar
  variability** (escape rates depend on stellar wind / EUV flux
  which is part of variability).

All other items are independent.

## Suggested sequencing

**Sprint 1 (numerical hygiene)**: 1 + 2 + 3 + 4 + 5. Cheap.
Cleans up the foundations.

**Sprint 2 (xeno foundation)**: 6 (multi-species — the big one)
in parallel with 8 (substrate solvent semantics — independent
files).

**Sprint 3 (xeno + astro coupling)**: 7 (lifecycle topology) +
9 (alt redox) + 13 (ice albedo) + 14 (greenhouse runaway).
Parallel — different subsystems.

**Sprint 4 (astro long-timescale)**: 12 (tectonics + erosion).
Standalone XL item — likely a multi-PR sprint on its own.

**Sprint 5 (closing the gaps)**: 10 (cognition topology) + 11
(speciation) + 15 (Hadley cells) + 16 (tidal heating) + 17
(atmospheric escape) + 18 (stellar variability). Parallel.

Total estimated effort: ~6-10 weeks. After this, the sim becomes
something a xenobiologist + astrophysicist would call a credible
representation of their domains. Before this, it's "well-engineered
simulation with characterised limits" in each.

## Notes on what NOT to do

- **Don't widen `PopulationBiology` schema before #7**.
  Hand-authored extensions to vertebrate brackets pile up
  technical debt that the Lifecycle enum will collapse.
- **Don't add new tools to the tech tree** until #8 + #9 land.
  The current Earth-shaped tree assumes O₂ combustion + water
  solvent; new tools authored against those assumptions will
  need re-keying.
- **Don't bolt on individual climate feedbacks** (e.g., just
  ice-albedo without greenhouse). The bistability point is to
  *get to two stable basins*; one-sided feedbacks just push
  equilibrium without creating the second basin.

## Acceptance: when can both experts sign off?

A xenobiologist would sign off when:
1. Multiple species coexist on a planet with documented
   interactions.
2. Lifecycles vary by body plan, not all run vertebrate-
   schema.
3. Solvent + redox chemistry actually differ per substrate.
4. Cognitive topology drives behavioural divergence.
5. Speciation events occur under environmental pressure.

An astrophysicist would sign off when:
1. Planet geography changes over time (tectonics + erosion).
2. Climate has identifiable runaway basins (Venus / snowball).
3. Stellar luminosity evolves; flares occur.
4. Atmospheric circulation has three-cell structure.
5. Moons cause measurable tidal heating + (optionally) tidal
   locking.

Both checklists are now scoped, sequenced, and effort-estimated.
The choices made by review 5 are not the only valid path — but
they're a defensible one, and they bring the sim from "well-
engineered demo" to "credible representation in both lenses."
