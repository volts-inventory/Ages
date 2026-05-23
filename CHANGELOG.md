# Changelog

Reverse-chronological highlights of user-visible changes. Not every
commit — see `git log` for that. Versions are retroactive: this
project tagged its first release at 1.0.0 once Sprint 5 dual-expert
sign-off landed.

## Unreleased

- Nothing yet.

## 1.0.0 (2026-05-23) — Ship-It

First tagged release. Sprint 5 closed with unconditional `SHIP_IT`
from both xeno and astro reviewers; the cross-planet test matrix
passes end-to-end on 12 substrate / spectral-class combinations.

### Cross-planet simulation
- End-to-end tests pass for 12 planet classes: Earth, Mars, Venus,
  Titan (hydrocarbon substrate), Ammoniacal substrate, super-Earth
  gravity, hot Jupiter (extreme params), M-dwarf HZ tidally-locked,
  Europa, Ganymede, Callisto (tidal heating), and silicate lava
  world.
- Calibration suite: MAVEN absolute escape rates for Mars, Venus
  runaway-greenhouse plateau, Ganymede + Callisto tidal heating,
  Earth jet velocity tightened to 30 m/s ±20%, snowball-recovery
  timescale, magnetic-reversal cadence per-month.
- Sprint 5 round-5 dual-expert sign-off (`SHIP_IT` unconditional
  from both xeno and astro reviewers).

### Code organisation
- Code-organisation audit (CA / CB / CC waves): split oversized
  modules (`sim/core/src/lib.rs`, `sim/core/src/tests.rs`,
  `sim/ecosystem/src/lib.rs`, `sim/civ/src/conflict.rs`,
  `sim/civ/src/catastrophe.rs`, `sim/civ/src/apply.rs`,
  `sim/physics/src/tectonics.rs`, `sim/physics/src/radiation.rs`,
  `sim/physics/src/atmospheric_escape.rs`,
  `sim/physics/src/tidal_heating.rs`,
  `sim/ecosystem/src/planet.rs`,
  `sim/population/src/lib.rs`, `sim/report/src/viewport/emitter.rs`,
  `sim/core/src/nomads.rs`) into per-concern submodules. Public
  re-exports preserved; downstream call sites unchanged.
- `pub` visibility tightened on internals (CA8).
- `sim-events` and `sim-report` moved to workspace-form dependency
  declarations.

### Narration
- `--narration` streaming flag emits human-readable prose to stdout
  while the sim runs (file emitter still receives the full event
  stream).
- `--replay-narration <log>` replays an archived NDJSON as prose
  without re-running the sim.

### Viewport + report
- Viewport and digest surface the new Sprint-5 dynamics
  (tidal-locking state, spectral type, magnetic reversal,
  atmospheric escape channels).

## 0.9.5 (2026-05-19) — Calibration fix loop

Post-Sprint-5 fix wave (C1–C4) targeting absolute-rate regressions
the cross-planet tests surfaced.

- **fix-C1** Pressure-scaled greenhouse cap closes the Venus
  runaway plateau within the calibration band.
- **fix-C2** `MAVEN_CALIBRATION_SCALE` scales Jeans + hydrodynamic
  escape to MAVEN-measured Mars rates.
- **fix-C3** Boosted `co2_greenhouse_k` lets snowball-state worlds
  recover within geologic timescales instead of locking in.
- **fix-C4** Laplace-resonance pumping multiplier puts Ganymede's
  tidal heat budget in the right order of magnitude relative to
  Io / Europa / Callisto.

## 0.9.0 (2026-05-15) — Plan v2 implementation complete

35 items across 5 sprints implemented (Items 1–24 plus the
sub-items 6a/6b, 7a/7b, 11a, 12a–e, 14a, 18a). The implementation
plan v2 was signed off by xeno + astro reviewers at item-31.

### Sprint 5 — Stellar evolution + atmosphere + worldgen (Items 15–24)
- Spectral types + SED + HZ migration + red-giant phase.
- Age-dependent EUV decay on `Star`.
- Tidal-locking dynamics with eccentricity damping.
- Worldgen tidal-locking-state sampler from moon + rotation.
- Mass-radius-density coupling with derived gravity + escape velocity.
- Full 3D Coriolis with vertical-rotation coupling.
- Cloud microphysics (cirrus / stratus) coupled to albedo + greenhouse.
- Angular-momentum-conserving Hadley / Ferrel / polar cells.
- Corrected tidal-heating formula with k2/Q + radius + eccentricity.
- Multi-channel atmospheric escape (Jeans + hydrodynamic + photochemical + ion).
- Magnetic reversal Markov chain + cosmic-ray flux coupling.

### Sprint 4 — Tectonics + erosion (Item 12)
- Plates + crust + boundaries foundation.
- Subduction at convergent oceanic-continental boundaries.
- Per-cell `crust_age` + sqrt(age) ocean-floor depth.
- Airy isostasy linking crust thickness to surface elevation.
- Slab-pull dynamics evolving plate velocities at subducting edges.
- Volcanic CO2 + H2O emission at boundaries + hot-spots.

### Sprint 3 — Speciation, HGT, greenhouse, weathering (Items 10, 11, 13, 14)
- 4-way `CognitionTopology` + comm-channel transmission speed.
- Speciation events with 5 triggers + allometric daughter drift.
- Horizontal gene transfer for microbial species.
- Sigmoid + bimodal ice-albedo with snow / sea-ice / cloud channels.
- Per-substance greenhouse with Clausius-Clapeyron H2O coupling.
- Carbon-silicate weathering thermostat with T + precipitation coupling.

### Sprint 2 — Ecosystem, lifecycle, dormancy (Items 6–9)
- 7-variant `Lifecycle` enum with per-variant step routing.
- Multi-species ecosystem with typed roles + interactions.
- `ToleranceEnvelope` + catastrophe-survival match.
- `dormancy_capability` + catastrophe-survival reduction + seed-bank resurrection.
- Solvent solubility + kinetics + per-substrate templates.
- Multi-oxidiser ladder + reduction-potential partition + syntrophy.
- Extinction rule with biomass-streak threshold + `SpeciesExtinct` event.
- CO2 substance + producer / consumer / decomposer biogeochem coupling.

### Sprint 1 — Q32 + physics foundations (Items 1–5)
- Quadratic reproductive success + 5000-clutch cap.
- Two-pass donor-limited tide flux.
- Adaptive dt sub-stepping with CFL acoustic-speed bound.
- Saturation-curve vapour cap replacing flat 10k floor.
- Cumulative-drift accumulator for slow-leak conservation check.

## 0.8.0 (2026-05-12) — F-wave (post-implementation fixes)

Fix backlog from the round-3 dual-expert review.

- **F1** Populate `species_registry` from `ecosystem.species` + canary test.
- **F2** Per-cell biomass on `EcoSpecies` for heterogeneous catastrophes.
- **F3** `ToleranceEnvelope` on `EcoSpecies`.
- **F4** Magic-constant ladder with origin + cross-planet status (`docs/internal/magic-constants.md`).
- **F5** Re-pin slow canary tests off seed 42.
- **F6** Per-substrate tidal calibration to fix Europa shortfall.
- **F7** Derive Hadley cell count from Rhines-length closure.
- Round-3 dual-expert review: BOTH APPROVED.

## 0.7.0 (2026-05-09) — T-wave (targeted physics + worldgen tightening)

T1–T21 items spread across physics, civ, ecosystem, and
worldgen. Highlights:

- **T1** Per-month magnetic reversal cadence.
- **T2** Route all catastrophe kinds through `apply_catastrophe_at_cell`.
- **T3** Thread `Planet::gravity()` into `tide_k` + `wind_k`.
- **T4** Exobase T (not surface T) for Jeans escape.
- **T5** Cirrus greenhouse from cloud-top T × lapse rate.
- **T7** Refined `EcosystemRole` → `Lifecycle` mapping.
- **T8** Bidirectional `cosmic_amp` clamp.
- **T9** Biome-class-weighted initial cell biomass.
- **T11** Venus runaway-plateau calibration test.
- **T12** Mars-MAVEN absolute escape-rate calibration test.
- **T13** Snowball-recovery timescale calibration test.
- **T14** Earth jet velocity tightened to 30 m/s ±20%.
- **T15** Titan-class hydrocarbon-substrate end-to-end test.
- **T16** Super-Earth gravity end-to-end test.
- **T17** Hot Jupiter extreme-params overflow test.
- **T18** M-dwarf HZ tidally-locked planet end-to-end test.
- **T19** Ganymede + Callisto tidal-heating calibration tests.
- **T20** Silicate lava world end-to-end test.
- **T21** Ammoniacal-substrate end-to-end test.

## 0.6.0 (2026-05-05) — P3 / extension wave

- **P3.1** Differentiated `MutualismKind` / `ParasiteKind` step.
- **P3.2** Character displacement for sister species.
- **P3.3** Plasmid-sweep HGT model.
- **P3.4** Caste-aware `Collective` quorum.
- **P3.5** Per-cell magnetic shielding via crustal remanence.
- **P3.6** Hadley band edges from Held-Hou closure.
- **P3.7** Crustal-type-dependent base albedo.
- **P3.8** Link tidal heating to eccentricity damping.

## 0.5.0 (2026-05-02) — P0–P2 wave (post-impl review backlog)

- **P0.1** Wire `sim-ecosystem` into production tick.
- **P0.2** Wire `HadleyCirculation` into orchestration pipeline.
- **P0.3** Route civ tick through `step_for_lifecycle`.
- **P0.4** Route catastrophe damage through `tolerance.match_score`.
- **P0.5** Civ carrying capacity tracks live producer biomass.
- **P0.6** Fix Q32 overflow + restore civ formation on canary seeds.
- **P1.1** Subsurface heat reservoir + conduction for tidal heating.
- **P1.2** Bind cosmic-ray flux to speciation + HGT rates.
- **P1.3** Seed `DormantPool` during catastrophes + per-tick resurrect.
- **P1.4** HZ migration drives cell habitability + biome class drift.
- **P1.5** Per-cell day-night radiation gradient for synchronous worlds.
- **P1.6** Coriolis omega decomposed by axial tilt for tilted-axis worlds.
- **P2.1** Dimensional `cal_factor` + Europa / Enceladus tidal-heating calibration tests.
- **P2.2** Explicit `molecular_mass` Jeans escape + H/He fractionation.
- **P2.3** Arrhenius temperature factor for carbon-silicate weathering.
- **P2.4** Faint-young-sun ZAMS = 0.70 in `bolometric_scale_at_age`.
- **P2.5** Single-source Lindeman ratio per habitat (no post-step cap).
- **P2.6** Per-pair `Interaction::half_saturation` + canonical calibration.
- `find_seed.rs` dev tool: brute-forces viable seeds after worldgen RNG shifts.

## 0.4.0 (2026-04-22) — Expert-review roadmap (waves 1–3)

- Expert-review wave 3 P1–P8 priorities accepted.
- Integrated expert-review roadmap (physics + civ + xeno + astro).
- Implementation plan v2 (xeno + astro signed off).

## 0.3.0 (2026-04-15) — Q32 overflow + discovery + culture

- Q32 overflow guards + `events_per_window` reformulation of birth_rate.
- Tighten `channels_for_modality` + wire production hypothesizer through it.
- Conservation invariants + hemisphere refactor + saturation-pressure hydrology retry.
- Wind-density coupling + energy-conserving advection + wind-coupled gravity flow.
- Alliance formation + dissolution rules + `AllianceFormed` / `AllianceDissolved` events.

## 0.2.0 (2026-04-08) — Multi-civ + economy + viewport polish

- M7: environmental feedback on species drift.
- M8 part 1: per-civ surplus accumulator + war + food + catastrophe integration.
- M8 part 2: trade routes between peaceful civs + per-tick surplus flow.
- Track A: viewport readability sprint (pop trend, latest unlock, war tag, legend, density mode).
- Track B: sim plausibility (falsification window, founder loss, tech-aware migration, hierarchy size).
- Track C: kinship un-freeze (generation + grudge) + narrator causal links.
- Track C #12: multi-component hierarchical conflict strength.
- `Pop` type introduced; pop scale lifted to support billion-scale civs.
- Per-tool `manipulation_prereqs` across all 58 tools fixed civ tech progression.

## 0.1.0 (2026-03-25) — First commit + viewport + tech

- Initial workspace: `protocol/`, `sim/arith`, `sim/core`,
  `sim/events`, `sim/physics`, `sim/world`, `sim/recognition`,
  `sim/species`, `sim/ecosystem`, `sim/civ`, `sim/population`,
  `sim/report`, `ages` binary.
- Live ASCII viewport with themed boxes for planet / species /
  events / per-civ panels.
- Tech tree gated on civ experimentation + species maturity.
- Pacing + war + viewport polish + attribution cleanup.
- Standard open-source README.
