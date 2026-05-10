# Claude session history

Chronological record of substantive work shipped via Claude Code in
the `volts-inventory/Ages` repository. Each entry names the PR, the
merge SHA, and what the pass actually delivered. New entries land at
the bottom.

This file is the project's **why** log — a future contributor (human
or AI) reading this can reconstruct how the simulation grew. For
operational rules see [`AGENTS.md`](AGENTS.md); for current state see
[`PLANNING.md`](PLANNING.md); for resumption see PLANNING's `Status`
section.

## October–November 2025: M6 + vision-delivery + biography session

A single multi-day session that took the project from "M5 closed,
M6 unstarted" to "all seven planned milestones shipped + every line
of the AGENTS.md vision has shipping code behind it." Twelve PRs
merged into `main` (#23 through #32, plus a baseline `#22` hook fix
that pre-dated the session).

### PR #23 — M6 post-run markdown report

`fc8c501` and follow-up cleanups merged as `97c951b`.

The first big landing: `sim/report` ships the full pipeline (NDJSON
parser → digest aggregator → markdown renderer) plus the
`ages-report` binary. Sections: planet card, species card, run
summary, per-civ chapters with fitted equations (Q33/Q34),
refinements (Q35), transmission table, contact + conflict, Q13
highlight reel.

Protocol: added `PlanetDerived` event; `RelationConfirmed` gained
`template_name` + `channel`; renamed `CatastropheFired::kind` to
`catastrophe_kind` to fix a pre-existing serialisation collision
with the `Event` enum's `tag = "kind"` discriminant (the duplicate
field broke deserialisation on every catastrophe event).

Smoke seed 42 / 5000 ticks: 394-line report. M6 acceptance met
end-to-end.

### Q50 breakaway transmission + Q13 trivial-constant filter

`56c2e3d`. The M6 smoke surfaced a real bug: `KnowledgeTransmitted`
was firing zero times on seed 42 despite 10 successor civs founding,
because every successor on that seed comes through Q48-v3's
concurrent-breakaway path — and that path was the one founding
route that didn't call `transmission::transmit_from_parent`.
Knowledge surviving collapse was the project's central Q47 promise;
breakaway successors were silently restarting tabula rasa. Wired
the transmission call into the breakaway path. 178 transmissions
on seed 42 / 5000 after the fix.

Highlight reel hygiene: 28 trivial `y = 0 constant` first-of-kind
pins were drowning the seed-42 reel. New `is_trivial_constant`
filter skips constant fits with `|param[0]| < 16` raw Q32.32 bits.
Inheritance pins collapse to one line per `(source, dest, tick)`.
The reel now reads as a biography.

### Q12 run-end + population timeline + `--cli=highlights`

`d2439ad`. Three vision-stated deliverables that AGENTS.md / docs
declared but were never wired:

1. **Q12 run-end taxonomy** (Decided in `q12.md`): the sim was
   hardcoded to emit `RunEnd { reason: "fixed_horizon" }`. Now
   after every `CivLifecycle` phase the loop checks for
   `species_extinction` (no active civ + low total pop) and
   `stagnation` (no active civ for 1000 consecutive ticks).
   Inhospitable seeds now end at tick ~99 with the truthful reason.

2. **Population timeline** report section: sparse arc derived from
   `CivFounded` / `CivCollapsed` / `CatastropheFired` events.

3. **`--cli=highlights` mode**: new `sim_events::FilterEmitter` +
   `is_highlight_event` predicate. Long runs become tail-able.

### Habitable-planet bias + periodic Snapshot events

`8052cb2`. A 30-seed sweep showed the prior planet sampler produced
multi-civ runs only **10%** of the time — the vision's
"civilizations rise and fall over thousands of years" was only
delivered for ~1 in 10 random seeds. Two root causes:

1. Composition / atmosphere / biosphere distributions were
   uniform-uniform-uniform, with most of the cube producing
   `biosphere = None`.
2. `terrain_peak` could land below `sea_level` → no land cells →
   no fuel → carrying_capacity = 0 → instant collapse.

Both fixed: composition biased 60% rocky / 25% ocean; rocky/ocean
atmosphere biased toward oxidising/reducing/hazy; biosphere when
allowed favours Sparse/Lush/Hyper; `terrain_peak ≥ sea_level + 1500m`
on rocky worlds. New 30-seed sweep: **73% multi-civ**.

Also added `protocol::Snapshot` (the fourth output channel from the
vision, never shipped). Every 500 ticks the sim emits a state-digest
event with active/collapsed civ ids + totals.

### PR #23 (continued) — Biography pass

`9c5396f`. The project is named *Ages* and the report should read
like a species' biography. Four additions on the report layer:

1. **Year framing**: `1 sim tick = 1 species-year` (one
   `integrate_civ_year` per tick), so "year 348" not "tick 348"
   throughout.
2. **Ages of the species** section. New `sim/report/src/ages.rs`
   derives emergent eras from event milestones — Foundational →
   Empirical → Refinement → Tool → Concurrent → Successor — not
   authored thresholds. The project's namesake earned by emergence.
3. **Memorable figures** cross-civ rankings.
4. **Species canon** consolidated knowledge state at run-end —
   partitioned into non-trivial findings and a collapsed list of
   trivial absences.

PR #23 covered all five commits above (M6 + Q50 + Q12 + habitable
bias + biography). Merged as `97c951b`.

### PR #24 — Spatial planet map + recognition vocabulary breadth

`1e95e88` merged as `e66ef8e`.

- New `protocol::PlanetMap` event carries grid dims + per-cell
  elevation + water_depth. Report renders a hex-tessellation ASCII
  map in the planet card (`^` peak / `m` mountain / `.` land / `~`
  shallow / `≈` deep).
- Three new recognition templates: `flood_zone`, `cold_zone`,
  `fertile_land`. Mirrored in `sim_species::template_channels`.
- Caught a pre-existing `fit_inverse_square` overflow on small-x
  samples; added a guard.
- Generalised the trivial-fit filter beyond `constant` to catch
  `linear → y = 0·x + 0` and other all-zero-coefficient fits.

### PR #25 — Doc drift cleanup

`a87f73f` merged as `a4281fc`. Audit pass found three real drifts:

1. PLANNING.md header was stuck in M3 mode — claimed "Active
   milestone: M6" with M3 listed as last commit. Replaced with a
   current `Status` section.
2. `docs/architecture.md` `--cli` modes wrong (5 modes claimed,
   3 shipped).
3. `docs/architecture.md` snapshot model overpromised (full-state
   replay; reality is digest checkpoint events).

Plus MANIFEST line counts updated.

### PR #26 — Archetype coverage: Crust + cognition topology + field templates

`ac419ad` merged as `99e7ce2`. A user-supplied "field-and-resonance
civilization" worldbuilding example surfaced substrate gaps. Five
additions so reports can express that kind of biography:

- `Crust` enum on `Planet` (Basaltic / Hydrocarbon / Piezoelectric /
  Ferrous / RareEarth).
- Fossil fuels decoupled from biosphere: Hydrocarbon crust
  contributes buried fuel; the others don't. "No combustion path"
  worlds are now a real sim outcome.
- `cognition_topology` species trait (`Centralized | Distributed`).
- Three new recognition templates: `auroral_activity`,
  `harmonic_resonance`, `static_field_gradient`.
- Industrial Age in the timeline (5+ tech unlocks).

Cross-seed survey: seed 37 produces a near-perfect "field-and-
resonance" world — piezoelectric crust + distributed cognition +
electric-field modality + 10 civs / 19 tech unlocks / all 7 ages.

### PR #27 — Tier-5 capabilities + emergent thousand-year transcendence

`d6cd221` merged as `0835ee5`. Brings the user-supplied story's
late-game capabilities (field-mediated propulsion, atomic-precision
metamaterials, bioelectric instrumentation, civilizational phase
transition) into the sim as **emergent civilizational milestones**
keyed off accumulated grounded state — without faking the underlying
physics.

Three new tier-5 tools (`bioelectric_resonator`, `field_propulsion_
engine`, `metamaterial_lattice`), narrative milestones with crust +
substrate gates. Q12 `transcendence` run-end fires when the species
has sustained at-least-one-civ-with-all-tier-5 for 2000 continuous
ticks AND has accumulated 3000+ confirmed relations across all civs.

User pushback during the iteration ("shouldn't transcendence
naturally take thousands of years? not tiers?") drove a key
correction: an earlier draft authored a "tier-N requires tier-(N-1)"
progression rule that violated Q40's emergence-over-authoring
principle. Reverted. The thousands-of-years arc now emerges purely
from the species-cumulative + sustained-window gates. Seed 26
transcends at year 9925.

### PR #28 — Audit-driven doc drift pass 2

`eea39d2` merged as `432fa6a`. Six doc drifts after PRs #26 and
#27 — sim/civ + sim/world + sim/recognition + sim/species +
protocol READMEs hadn't kept up; docs/architecture.md didn't
mention the Q12 run-end taxonomy.

### PR #29 — Pass 1: cognition fork + catastrophes + planet props

`38ec8d8` merged as `9c75294`. Three biography-improving additions:

1. **Distributed-cognition fork**: +10% cognition trait at
   derivation. Ripples through Q34 tolerance + min-sample
   formulas via existing machinery — no new fork code.
2. **3 new catastrophe variants**: `Asteroid` (rare deterministic-
   prime firing window, 40% loss, mystical/reformist push),
   `SolarFlare` (high luminosity + weak magnetosphere, 10% loss,
   empirical push), `IceAge` (cold + maturity gate, 20% loss,
   communitarian/hierarchical push).
3. **2 new planet properties**: `axial_tilt_deg` and
   `day_length_hours`. Flavour-only; reserved for seasonal/diurnal
   physics.

### PR #30 — Pass 2: Q9 taboo + Q25 focus culture-axis wiring

`6407775` merged as `7804d2c`. Two more Q-decision behavioural
hooks wired up:

- **Q25 focus_weight** compounded onto Q26's suppression in the
  hypothesizer's confidence multiplier. Empirical/reformist civs
  confirm more relations per unit fit-quality.
- **Q9 taboo attenuation**: high-mystical civs credit fewer
  firings to `firings_by_template` via deterministic
  per-(tick, civ, template, cell) hash bucket. No RNG threading.

### PR #31 — Pass 3: per-figure doubt + communicativeness

`73ec05d` merged as `ea1b8ae`. Two new per-figure personality
scalars beyond the existing charisma + curiosity (Q59):

- **`doubt`** drives Q35 refinement aggressiveness via per-figure
  `switch_margin` scaling (`1.5 - doubt`).
- **`communicativeness`** boosts Q50 transmission comprehension
  by `1 + 0.3 × max(communicativeness)` of the parent civ's
  figures.

Cosmetic exposure in the report deferred to the next PR.

### PR #32 — Audit + Pass 3 finish + tides template

`bfa17cb` merged as `3888510`. Combined pass:

- **Audit**: cross-seed sweep distribution unchanged from before
  Pass 1/2/3 (23 fixed_horizon / 7 species_extinction / 0
  stagnation across 30 seeds). Substrate-aligned long runs reach
  transcendence at year 9855 (seed 184) and 11570 (seed 116) —
  thousand-year arc preserved.
- **Pass 3 finish**: `FigureBorn` protocol extended with the
  Q59 personality scalars. Memorable Figures now lists each
  prolific scientist's full personality vector + three
  superlatives (Most charismatic / Boldest skeptic / Most
  communicative).
- **Tides**: `tidal_extremum` recognition template (id 14) —
  deep-water-belt approximation pending lunar-gravity SWE physics.
- Doc drift fixed across 6 files.

## May 2026: recognition + discovery breadth session

A single session that broadened the recognition catalogue across
non-Earth planet archetypes (Q71), added the seasonal axis (Q72),
and gave civs a continuous-law measurement track parallel to the
firing-relation track (Q73). Three Q-decisions shipped end-to-end
via two commits on `claude/document-deferred-tasks-i3bIZ`.

### Q71 — Planet-archetype-specific recognition templates

Pre-existing recognition catalog had 14 templates that were mostly
Earth-equivalent (only `harmonic_resonance` was genuinely non-
Earth). SubSurfaceOcean and GaseousShell civs observed almost
nothing — their physics state didn't trip the Earth-leaning catalog.

Added 10 templates whose physics signatures fire reliably on the
right archetype:

| ID | Name | Archetype |
|---:|---|---|
| 15 | `cryovolcanism` | Sub-surface ocean |
| 16 | `ice_quake` | Sub-surface ocean |
| 17 | `pressure_storm` | Gaseous shell |
| 18 | `metallic_hydrogen_signal` | Gaseous shell |
| 19 | `piezoelectric_pulse` | Piezoelectric crust |
| 20 | `magnetic_lodestone` | Ferrous crust |
| 21 | `hydrocarbon_seep` | Hydrocarbon crust |
| 22 | `superconductor_resonance` | RareEarth + cold |
| 23 | `reducing_storm` | Reducing atmosphere |
| 24 | `hazy_obscuration` | Hazy atmosphere |

User pushback during the iteration ("Don't planet properties
determine phenomenon existence and occurrence? Like magnetic and
size and composition. Make sure it's true") drove the key
correction: the templates were initially flavour-only because
crust / atmosphere / magnetosphere / composition didn't actually
imprint into per-cell physics state. `init_planet` now writes
crust → charge baseline (Hydrocarbon 2 / Piezoelectric 12 /
Ferrous 15 / RareEarth 6), magnetosphere → planet-wide charge
(0/1/3), atmosphere → vapour baseline above sea level, and
composition-specific imprints — SubSurfaceOcean routes the water
column to `Substance::Ice` (otherwise chemistry's first-tick
freeze cascade re-melts it via latent-heat spike); GaseousShell
imprints a 700 K + vapour-5 + magnetosphere-tracked charge
column (15/35/70 — below the EM discharge threshold from
`build_laws`, otherwise the imprint self-zaps every tick).
Hydrocarbon crust fuel bonus bumped to +4 so even Sparse-
biosphere hydrocarbon planets exceed the seep threshold while no
non-hydrocarbon planet does.

Cross-seed verified across 11 seeds spanning every archetype:
seeds 4 (RareEarth/Hazy), 7 (Hydrocarbon/Reducing), 11 + 25
(GaseousShell), 12 (rocky/Hydrocarbon), 23 (Ferrous), 35
(rare_earth/thin), 47 (SubSurfaceOcean), 116 (Piezoelectric),
250 (Hazy), 854 (cold rare_earth → superconductor_resonance).

### Q72 — Seasonal recognition templates

Q67 wired seasonal physics but no template observed it. Q72
added 4 month-keyed templates and a new `Signature::MonthIn(start,
end)` variant supporting wrap-around (`MonthIn(11, 1)` =
Nov/Dec/Jan):

| ID | Name | When | Cell condition |
|---:|---|---|---|
| 25 | `seasonal_thaw` | months 2–4 | 273–290 K |
| 26 | `polar_winter` | months 11–1 | < 240 K |
| 27 | `equatorial_wet` | months 5–9 | water depth 0.5–3 m |
| 28 | `axial_extremum` | month 0 | < 250 K |

`RecognitionLibrary::scan` now takes a `tick: u64`. All four
templates carry `FormTag::Periodic` so the form-vocabulary
derivation (Q53) unlocks `Periodic` for any civ that perceives
one — letting them fit sinusoidal/triangular forms to seasonal
data.

### Q73 — Measurement relations: continuous-law recovery

User asked "do civs actually learn from observing the phenomena
and physics?" — the honest answer was "partly". Firing relations
sample `(x = channel reading, y = did the template fire 0/1)` and
recover thresholds. They don't recover the underlying continuous
laws (heat-conduction's α, EM's conductivity, gravity-flow's k).

Q73 added a parallel **measurement relation** track. Continuous
`(x = channel reading, y = channel reading)` samples flow into
the same `fit::fit` machinery. `MeasurementChannel::{Direct,
NeighbourMean}` selects either the cell's own value or the
6-neighbour mean. Default catalogue auto-seeded for every civ:
`temperature ↔ neighbour_mean(temperature)`,
`water_depth ↔ neighbour_mean(water_depth)`,
`charge_magnitude ↔ neighbour_mean(charge_magnitude)`,
`temperature ↔ elevation`.

`MeasurementConfirmed` event mirrors `RelationConfirmed` with y/x
channel tags instead of template_id; cosmology pushes (Q24) and
plateau detection (Q49) treat measurement confirmations as
discoveries. `relation_id` namespace `1_000_000+` keeps measurements
disjoint from firing relations' `template_id × 16 + channel` space.
`params_in_real_units` rescales by both `y_scale` and `x_scale`
(firing relations only need x — binary y has no scale).

Verified on seed 1 (rocky): civ 1 confirms `temperature ↔
neighbour_mean(temperature)` as `Linear` with slope ≈ 1.235. The
deviation from 1.0 is physical signal — polar cells are colder
than the mean of their neighbours (which include warmer mid-
latitude rows), so a slope > 1 is the correct fit. The civ has
*measured* their planet's temperature gradient asymmetry. New
unit test `measurement_confirms_temperature_smoothing_law`
covers the synthetic-equilibrium case.

### Branch + commits

Both shipped on `claude/document-deferred-tasks-i3bIZ`:

- `3c952a4` — Q71 + Q72 archetype + seasonal templates
- `664b944` — Q73 measurement relations

Workspace clippy `--workspace -- -D warnings` clean; sim-civ
(98 tests), protocol, sim-recognition, sim-world, sim-species,
sim-physics, sim-events, sim-report, sim-knowledge, sim-culture
all green.

### Follow-up: review-driven defenses

A subsequent session ran a review pass on Q71/Q72/Q73 and flagged
three risks (commit `b225bd0`, pushed straight to `main`):

1. **Q73 trivial-fit filter for measurements.** Identity-on-
   uniform fits (`slope ≈ 1, residual ≈ 0` on a sub-surface
   ocean's flat thermal field) used to confirm and clutter the
   species canon. Now rejected at confirm time via a sample-
   variance check (`is_trivial_measurement` in
   `sim/civ/src/discovery.rs`); genuine gradient fits still
   flow.
2. **Q72 polar_winter per-hemisphere.** The original signature
   `MonthIn(11,1) AND Below(temp, 240)` fired both hemispheres
   simultaneously. New `Signature::Hemisphere(Northern|Southern)`
   and `Signature::Any(...)` primitives let `polar_winter` fire
   northern cells in months 11/0/1 and southern cells in months
   5/6/7, with shoulder seasons silent everywhere.
3. **Q71 charge-baseline regression test.** The crust ×
   magnetosphere imprint matrix is brittle — every value must
   sit below `discharge_threshold_for(magnetosphere)` AND inside
   the firing window of its target template. New
   `discharge_threshold_for(Magnetosphere)` helper in
   `sim/world` is the shared source of truth (consumed by
   `sim_core::build_laws`). New regression test
   `q71_imprints_satisfy_discharge_and_template_invariants`
   walks all 15 (Crust × Magnetosphere) pairings plus 3
   GaseousShell variants and asserts both invariants. The test
   caught a real collision at write time: RareEarth + Strong
   magnetosphere produces charge 9, outside the original
   `superconductor_resonance` window (5, 8). Window widened to
   (5, 10), preserving discrimination against Hydrocarbon's
   max of 5 and Piezoelectric's min of 12.

Workspace tests + clippy `--workspace -- -D warnings` clean.
Smoke seed 42 / 100 years: 836 `RelationConfirmed`, 10
`MeasurementConfirmed`, 40 `polar_winter` firings — pipeline
non-trivial after the filter.

## May 2026 (continued): vision-audit pass — Q74/Q75/Q77 + 5 deferred

The user asked for a vision review against "as close to real life
as possible, least hardcoded, units learn." A targeted audit
surfaced 14 hardcoded-constant findings (recognition thresholds,
carrying capacity, founding pop, migration, birth multipliers)
plus four "units learn" gaps (temporal derivatives, compositional
relations, inherited-knowledge re-validation, predict + falsify).
Three Q-decisions implemented end-to-end; five drafted for
follow-up sessions.

### Q74 — Relativise recognition thresholds to planet climate

`Signature::InClimateBand(ClimateBand)` and
`Signature::AboveIgnition` plus a per-run `PlanetContext` derived
from the actual cell-temperature distribution (not the planet's
stated mean — Q71's GaseousShell imprint pins cells to a 700 K
column the sampler doesn't know about). Bands quartered around
the normalised offset `o = (T - mean) / gradient`:
`DeepCold ≤ -0.5`, `Cold < -0.25`, `ProductiveBand` ±0.25, `Hot > 0.25`.

Nine templates relativised: `fire` (planet-derived ignition),
`thermal_gradient` (Hot), `cold_zone` (DeepCold),
`harmonic_resonance` (ProductiveBand), `superconductor_resonance`
(DeepCold + RareEarth charge), `reducing_storm` (Hot + low
oxidiser), `hazy_obscuration` (ProductiveBand), `polar_winter`
(Cold + hemisphere + month), `axial_extremum` (DeepCold + month 0).

Cross-seed verified: a 232 K sub-surface ocean now observes its
own polar_winter, productive-band, and Hot zones — none of which
the absolute-Kelvin thresholds let it see before. The framework's
"different worlds, different sciences" promise applies to
recognition itself, not just downstream knowledge.

### Q75 — Temporal-derivative measurements

`MeasurementChannel::Laplacian(Channel)` computes
`Σ(neighbour - self)` (the discrete Laplacian); `TemporalDelta(Channel)`
returns `current - previous`, requiring a `prev_state` snapshot.
sim/core takes the snapshot before `PhysicsIntegration` each tick;
`Hypothesizer::observe_cells` accepts `prev_state: Option<&PhysicsState>`
and threads it through measurement-channel reads.

Three new default measurement candidates:
`delta_temperature ↔ laplacian_temperature` (heat-conduction α),
`delta_charge ↔ laplacian_charge` (EM conductivity),
`delta_water_depth ↔ laplacian_elevation` (gravity-flow rate).

Verified on seed 42 / 500 years: civs confirm
`delta_temperature ↔ laplacian_temperature` and
`delta_water_depth ↔ laplacian_elevation` linear fits — the
underlying continuous laws, not just equilibrium relations.

### Q77 — Substrate-derived demographics

Four flat constants replaced with substrate-derived helpers:

- `founding_min_population(biosphere, cognition)` — was 100 flat,
  now 50 + biosphere_pressure×50 + (1-cog)×25 (range ~63-138).
- `carrying_capacity_per_unit(biosphere, gravity, cognition)` —
  was 50 flat, now 40 × (1+biosphere_richness) × g_factor ×
  (0.7+0.3×cog) (Earth-equivalent recovers ~50).
- `migration_pressure_threshold(sociality)` — was 0.85 flat,
  now 0.6 + 0.3×sociality (range 0.6-0.9).
- `biosphere_birth_factor_for_planet(planet)` — biosphere base
  modulated by axial-tilt and luminosity factors.

`Civ::configure_substrate` installs the per-civ values at
founding; sim/core calls it for inaugural and every successor /
breakaway. Calibration preserves the project's ~70% multi-civ-run
rate from the 30-seed habitability sweep.

### Deferred Q-decisions

Five drafted but deferred-implementation (each documented with
the subdecisions that need a focused pass):

- **Q76** Compositional relations — confirmed laws as basis
  terms in downstream candidates. Civs build hierarchical
  theories instead of atomic relations.
- **Q78** Inherited-knowledge re-validation — successor civs
  re-test parent's transmitted relations against own
  observations; failed re-fits emit `RelationFalsified` /
  `RelationLapsed`.
- **Q79** Predict + falsify — confirmed relations make
  predictions about next-tick state; sustained mispredictions
  force-trigger Q35 refinement.
- **Q80** Calendar from planet orbit — replace
  `MONTHS_PER_YEAR = 12` with per-planet derivation from
  `orbital_period × day_length`.
- **Q81** Parametric planet sampler — replace enum-bucketed
  archetypes (Composition / Atmosphere / Crust / Magnetosphere)
  with continuous parameter vectors for open-ended planet
  generation. Biggest pivot; needs its own session.

## May 2026 (continued): Q85-Q89 deepen-the-substrate sweep

User said: "Do it all right?" — meaning every remaining
follow-up. Five more waves on top of the Q83-Q81 sweep,
each its own PR.

### Q85 — Substrate-specific Clausius-Clapeyron + per-substrate latent heats

`substrate_boiling_point_k(tag, pressure)` runs Clausius-Clapeyron
for any substrate. `SubstrateProperties` carries the full
phase + latent-heat profile per substrate. Methane evaporation
releases ~25% of water's heat; silicon melts release ~5×.
Per-substrate `c_p` deferred as a follow-up.

### Q86 — Tidally-locked planets + terminator template

`Planet::is_tidally_locked()` (day_length >= 1000h). Sampler
weights ~5% of seeds into the tidally-locked archetype. New
`Signature::TidallyLockedTerminator` matches cells in the q ≈
width/4 / 3·width/4 longitude band on tidally-locked planets.
Template id 32. Seed 39 (3150h day): terminator fires 2172
times in 5 years.

### Q87 — More substrate-native recognition templates

- 33 `cryo_lake` (Hydrocarbon, T 90-180 K + WaterDepth > 0.5)
- 34 `crystal_growth` (Silicate, T 800-1500 K + |Charge| > 8)
- 35 `aurora_polar` (cross-substrate, |Charge| > 10 + Cold)

### Q88 — Multi-level residual chains

Q76's one-level-deep auto-generation extends to 3 levels.
`MeasurementCandidate::residual_depth` tracks chain depth;
`MAX_RESIDUAL_DEPTH = 3` caps recursion. Civs derive
Newton-to-Mercury-perihelion-style theory hierarchies.

### Q89 — Per-civ scientific-lifecycle counters in the report

`CivChapter` gains `revalidated_count`, `lapsed_count`,
`falsified_count`. Per-civ chapter renders "Knowledge dynamics:
revalidated 221 inherited laws, lapsed 19, falsified 48 of its
own." Knowledge evolution is now legible in the chapter.

### PR landings

- PR #60 — Q85 (`a199869`)
- PR #61 — Q86 (`705f7e2`)
- PR #62 — Q87 (`f574a70`)
- PR #63 — Q88 (`3b9ddf0`)
- PR #64 — Q89 (this PR)

Workspace tests + clippy `--workspace -- -D warnings` clean
across every PR.

## May 2026 (continued): Q83/Q84/Q79/Q78/Q76/Q80/Q81 follow-up sweep

User said: "Do all of those, don't stop asking about session." I
went through every "follow-up" deferred from prior Q-decisions and
shipped them as six waves, each its own PR.

### Q83 — Substrate-relative chemistry physics

`Chemistry::for_planet(pressure, ignition, substrate_tag)`. Per-
substrate freeze/boil thresholds: Aqueous 273.15/373.15 K, Ammoniacal
195.4/239.8 K, Hydrocarbon 90.7/111.7 K, Silicate 1687/3538 K.
`Substance::{Water, Ice, Vapour}` keeps its enum but now carries
"solvent liquid/solid/gas" semantics — on a methane world `Water`
represents liquid methane.

### Q84 — Substrate-specific recognition templates

`silicate_resonance` (id 29), `methane_seep` (id 30),
`ammoniacal_storm` (id 31). Silicate seed 25 fires resonance 3360
times in 5 years; Hydrocarbon seed 3 fires methane_seep 2100
times; Ammoniacal seed 7 fires ammoniacal_storm 120 times.

### Q79 — Predict + falsify

`ConfirmedRelation` gains `initial_residual` snapshot and
`falsification_streak`. When live RMSE exceeds 1.5× initial for
30 ticks, emit `RelationFalsified` and force-trigger Q35
refinement faster than the slow confidence-streak path. Seed 42 /
1000 years: 1074 falsifications drove 248 refinement proposals
(122 confirmed).

### Q78 — Inherited-knowledge re-validation

Transmitted relations carry `inherited_from_tick` /
`inherited_from_civ_id`. After 50 ticks the successor re-fits
the inherited form on its own samples; pass = `RelationRevalidated`,
fail = `RelationLapsed`. Streaks reset on inheritance. Seed 42 /
1000 years: 653 transmitted, 575 revalidated, 54 lapsed (8% lapse
rate).

### Q76 — Compositional relations

Auto-generates residual children when a measurement confirms.
Source's form + params + x_channel snapshotted in `ResidualBasis`
(frozen, not live re-evaluated). Civs build hierarchical theory:
T explained by neighbour-mean → residual explained by elevation.
Seed 42 / 30 years: 2917 measurement confirms, 2907 residual
children. 4-second runtime — no perf regression.

### Q80 — Per-planet calendar

`Planet::orbital_period_months` (sampled in [8, 16]) drives Q72
`Signature::MonthIn` modulo. A 16-month world's polar_winter
fires for 3/16 of its orbital period. Q67 internal cadence
(1-tick = 1-species-month) holds; year-bearing constants like
`STAGNATION_THRESHOLD_TICKS` stay 12-month-calibrated.

### Q81 — Within-substrate correlation polish

`temperature_gradient` covaries with `axial_tilt_deg`
(linear interp: tilt=0 → [5,25] K spread; tilt=45 → [20,50] K).
Integer-only sampling per Q36. Full enum-replacement Q81 deferred
— Q82 took most of its air.

### Q74 + Q77 polish

Closed as documented-deferral in their own docs. The
substantive Q74 climate-band tuning shipped in PR #51; the Q77
calibration tightening shipped in PR #52. The remaining
follow-ups (per-archetype band coefficient, per-civ founding
in the report) are documented as low-payoff render churn.

### PR landings

- PR #54 — Q83 + Q84 (`3e0f9ac`)
- PR #55 — Q79 (`30596b9`)
- PR #56 — Q78 (`4af70a6`)
- PR #57 — Q76 (`da64e34`)
- PR #58 — Q80 (`6b53272`)
- PR #59 — Q81 + Q74/Q77 polish (this PR)

Workspace tests + clippy `--workspace -- -D warnings` clean
across every PR.

## May 2026 (continued): Q82 — `MetabolicSubstrate` axis, every seed habitable

User asked why life would spawn on uninhabitable worlds. Honest
answer: it doesn't, but the simulation was generating species on
`BiosphereClass::None` worlds and watching them die at year 99
with `species_extinction`. Game-mechanic theatre. The proposed
fix — a habitability filter — would have been Earth-water-
chauvinist, since silicon-substrate life or methane-substrate
life would never get a chance.

### Q82 decision

New `MetabolicSubstrate` enum with four variants: `Aqueous`
(water, 250-400 K), `Ammoniacal` (ammonia, 195-240 K),
`Hydrocarbon` (methane / ethane, 90-180 K), `Silicate` (silicon
crystal, 800-1500 K). Each has a `temperature_range()` and an
`atmosphere_compatible()` predicate. Sampler picks substrate
first (60/15/15/10 weighted), then constrains temperature /
atmosphere / composition / crust to the substrate's window.

Every seed now produces a habitable world of *some* chemistry.
`BiosphereClass::None` is no longer a normal-sampling outcome.
`Planet::is_habitable()` is trivially true by construction; kept
as a public predicate for downstream consumers.

### Verification

- Silicate seed 11: 1292 K mean temperature, hyper-biodiverse
  silicate biosphere, 2 civs founded over 200 years, ran to
  fixed_horizon.
- 51-seed substrate distribution: 32 aqueous (63%), 9 hydrocarbon
  (18%), 5 silicate (10%), 5 ammoniacal (10%) — close to target
  weights.
- 13-seed @ 500yr sweep: extinctions drop 7 → 5 vs PR #52
  baseline. The remaining extinctions are real `food_crisis`
  collapses on substrate-viable worlds, not inhabitability
  theatre.

### Follow-ups (in q82.md)

- Q83 substrate-relative chemistry physics (replace water-
  freeze-and-boil with substrate-specific solvent dynamics).
- Q84 substrate-specific recognition templates
  (`silicate_resonance`, `methane_seep`, etc.).
- Q81 partly superseded — substrate is now the meaningful axis;
  continuous-parameter sampling is less urgent.

## Vision-delivery scorecard

Every line of [`AGENTS.md`](AGENTS.md)'s vision statement now has
shipping code behind it.

| Vision claim | Status |
|---|---|
| Headless Rust simulation | ✓ |
| Biography of a species across full history | ✓ (M6 + biography + Pass 3) |
| Planet sampled from a seed | ✓ (~73% habitable seeds) |
| Species evolves to fit it | ✓ (Q47 derivation) |
| Civs rise and fall over thousands of years | ✓ (typical seed: 9–12 civs / 5000 years) |
| Each derives different physics from ours | ✓ (species canon shows planet-specific constants) |
| Knowledge survives collapses | ✓ (Q50 + breakaway transmission) |
| Post-run report covers full multi-civ history | ✓ (M6) |
| Recognition templates → named phenomena | ✓ (28 templates: Q71 archetype + Q72 seasonal) |
| Civs learn continuous laws from surroundings | ✓ (Q73 measurement relations recover SI coefficients) |
| Species sensoria gate perceivable | ✓ |
| Named figures fit forms (Q32) | ✓ (12 forms; Q35 refinement; Q59 personality) |
| Output: NDJSON events | ✓ |
| Output: periodic snapshots | ✓ (digest events every 500 ticks) |
| Output: live CLI stream | ✓ (quiet/all/highlights) |
| Output: markdown post-run report | ✓ |
| Determinism (same seed = same run) | ✓ |

## Out of scope (deliberately)

Per AGENTS.md principle 4 ("quantitative depth, not tokens" —
discoveries grounded in real physics) and the explicit `Deferred`
list, the sim does not simulate:

- Consciousness as engineerable physics
- Field-mediated propulsion / inertial management as physics
  (tier-5 capabilities are narrative milestones)
- Civilizational phase transitions to non-matter substrate
  (`transcendence` run-end ends the run; the sim doesn't model
  the post-transition mode)
- FTL or atomic-precision metamaterial manufacturing as physics
- Inter-planet civ contact (one species per planet per run)
- LLM narration anywhere

## Where to read deeper

- [`README.md`](README.md) — public-facing project overview
- [`AGENTS.md`](AGENTS.md) — operational rules + routing
- [`PLANNING.md`](PLANNING.md) — current state + recent passes
- [`docs/architecture.md`](docs/architecture.md) — process layout +
  Q12 run-end + post-run report
- [`docs/MANIFEST.md`](docs/MANIFEST.md) — doc index
- [`docs/decisions/INDEX.md`](docs/decisions/INDEX.md) — Q-decision
  status pointer
