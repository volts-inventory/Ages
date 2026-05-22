# Post-fix xenobiologist re-review (PRs #73-99)

Companion to:
- `docs/post-implementation-xeno-review.md` (verdict **NEEDS_REWORK**)
- `docs/post-implementation-fixes.md` (26-PR fix programme)

Reviewer lens: xenobiology. Scope: production paths after the
P0/P1/P2/P3 backlog landed.

## Overall assessment

The integration debt the previous review flagged has been paid. The
production binary now instantiates a `PlanetEcosystem` at worldgen
(`sim/core/src/lib.rs:136-137`), steps it every tick between the
chemistry and catastrophe phases (`lib.rs:653`), threads producer-
tier biomass into civ carrying capacity (`sim/civ/src/capacity.rs:
189-204, 329-367`), routes per-civ population through the lifecycle
dispatcher (`sim/population/src/lifecycle.rs:121`), gates
catastrophe survival on `ToleranceEnvelope::match_score`
(`sim/civ/src/catastrophe/mod.rs:211-255`), feeds
`cosmic_ray_ground_flux` into speciation and HGT
(`sim/core/src/lib.rs:677-699`), seeds the dormant pool on
catastrophe and drains it per tick (`capacity.rs:347, 397`), and
runs differentiated mutualism / parasite step variants
(`ecosystem/src/lib.rs:878-969, 1007-1041`). C1-C7 are demonstrably
wired. Verdict: **APPROVED_WITH_CONDITIONS** — the xenobiology
layer is now genuinely *running* on the planet, but two issues
below (species-registry never populated; ecosystem is a planet-
wide aggregate with no per-cell biomass) limit how much of the new
behaviour can fire end-to-end in a live run.

## C1-C7 status table

| Gap | Status | Production call path |
|-----|--------|----------------------|
| **C1** sim-ecosystem unwired | **Fixed** | `sim/core/Cargo.toml:16` adds the dep. `sim/core/src/lib.rs:136-137` calls `sample_ecosystem_with_substrate(planet.seed, substrate_tag, planet_capacity)` at worldgen. `lib.rs:653` calls `ecosystem.step_with_biogeochem_at_tick(&mut state, solar_irradiance, tick)` every tick. `lib.rs:660` emits `Event::SpeciesExtinct` for each returned event. |
| **C2** civ capacity sums `Substance::Fuel` | **Fixed** | `sim/civ/src/capacity.rs:76-87` reads `self.producer_biomass`. `lib.rs:458-468` calls `ecosystem.tier_biomass(0)` per tick and passes it into `population_phase`, which threads it into `civ.step_population_per_cell` → `civ.update_producer_biomass` (`capacity.rs:282-312`). The per-cell variant (`capacity.rs:259-263`) divides producer biomass by `n_cells` so cascading extinctions starve civs proportionally. |
| **C3** lifecycle dispatch unused in production | **Fixed** | `sim/civ/src/capacity.rs:197, 220, 359` route every civ population step through `sim_population::lifecycle::step_for_lifecycle(&species.lifecycle, ...)`. Caste-aware Eusocial step at `population/src/lifecycle.rs` variants; Vertebrate path falls through bit-identically (`lifecycle.rs:130`) to preserve canary determinism. |
| **C4** ToleranceEnvelope decorative on catastrophe | **Fixed** | `sim/civ/src/catastrophe/mod.rs:211-255` reads `species.tolerance.match_score(t, ph, sal, rad, p)` and multiplies into the loss fraction (`base_loss = after_tools * (1 - survival_match)`). Cell conditions built per-event at `catastrophe/mod.rs:129-175` (volcanic uses post-eruption cell T; solar flare adds `solar_flare_radiation_boost() × cosmic_amp` to baseline; ice age applies a -50 K delta). All five catastrophe kinds call `apply_resistance_and_dormancy` so radiation-tolerant extremophiles ride out flares, narrow-envelope species don't. |
| **C5** MutualismKind / ParasiteKind tag-only | **Fixed** | `sim/ecosystem/src/lib.rs:862-969` branches on `ParasiteKind::{Macro,Micro,Virus}` — Macro adds a 10% host fertility hit on top of base flux; Micro fires an extra 5% loss only above the crowding threshold (5% × producer_capacity); Virus is *episodic* — inert between outbreaks, then -30% host on every `VIRUS_OUTBREAK_PERIOD = 100` ticks (`lib.rs:921-955`). `lib.rs:1007-1041` likewise branches on `MutualismKind::{Pollinator, SeedDisperser, Engineer}` with distinct mechanics. |
| **C6** cosmic-ray flux has no biology consumer | **Fixed** | `sim/core/src/lib.rs:677` reads `state.cosmic_ray_ground_flux()` and passes it as `cosmic_ray_multiplier` into both `step_speciation` (`speciation.rs:731`, clamped to `[1, 10]` via `clamp_cosmic_ray_multiplier`) and `step_hgt` (`hgt.rs:203`, `p_per_pair *= cosmic_mult`). Catastrophe path also amplifies post-flare radiation: `catastrophe/mod.rs:475-476`. |
| **C7** DormantPool never seeded | **Fixed** | `catastrophe/mod.rs:245-253` deposits `pop_before × base_loss × dormancy × severity` into `civ.dormant_pool.population` on every catastrophe path. `capacity.rs:347, 397` call `step_dormant_resurrection` (`capacity.rs:413`) on the per-tick population step, which drains the reservoir back into the active cohort. `pre_catastrophe_population` (`catastrophe/mod.rs:242-244`) tracks the high-water mark so multi-catastrophe runs keep an honest resurrection cap. |

## New gaps surfaced after the fixes landed

### N1 [L] — Species registry is never populated from the ecosystem

`/home/user/Ages/sim/core/src/lib.rs:151` instantiates an empty
`BTreeMap<SpeciesId, sim_species::Species>`. The comment at
`lib.rs:144-150` is honest about it ("Until the per-trait species
registry is wired in (P1+), the registry stays empty"), but it
*matters*: `step_speciation` iterates `species_registry` to derive
daughters and `step_hgt` iterates Microbial entries — both produce
zero events when the registry is empty. The `ecosystem.species`
`BTreeMap<SpeciesId, EcoSpecies>` exists in parallel but isn't
mirrored into the trait registry. Net effect: a live run will emit
`SpeciesExtinct` events (the ecosystem itself owns its species map)
but will essentially never emit `SpeciationOccurred` or
`HorizontalGeneTransfer`. The wiring is in place; the data isn't.
This is the natural follow-up to C1.

### N2 [L] — PlanetEcosystem is a planet-wide aggregate, no per-cell biomass

`EcoSpecies` (`ecosystem/src/lib.rs:289-314`) carries a single
`biomass: Real` scalar — there's no `Vec<Real>` indexed by cell.
Producer growth, predation, extinction, and tolerance gates run on
the planet as a whole. Two consequences:

- The C4 tolerance gate reads per-cell catastrophe conditions, so
  the *civ* feels heterogeneous catastrophes — but the *ecosystem*
  doesn't. A polar-ice-cap-loving extremophile and an equatorial
  thermophile see the same global biomass pool.
- The "extremophile niches don't fill until per-cell match_score
  gating lands" critique (N4 from the original review) still
  stands. There's no per-cell habitat occupancy field. Producer
  collapse in one hemisphere can't propagate as a local famine —
  it collapses globally and starves every civ.

This is architectural; per-cell biota is a multi-sprint refactor.

### N3 [M] — Civ uses civ-species `tolerance`; EcoSpecies has none

`apply_resistance_and_dormancy` reads `species.tolerance` from the
civ-bearing `Species`. The `EcoSpecies` records have habitats but
no `ToleranceEnvelope` — they're filtered through a uniform
planet-wide extinction sweep. So extremophile *ecosystem members*
don't get differential survival vs narrow-envelope ones during
catastrophes. The civ-side fix is correct in isolation but doesn't
extend into the trophic web.

### N4 [S] — Solar-flare cosmic_amp clamp is asymmetric

`catastrophe/mod.rs:475` does
`state.cosmic_ray_ground_flux().max(Real::ONE)` — guarding against
the multiplier *softening* a flare. Reasonable, but reversal
windows amplify while quiet windows don't dampen. A strong
magnetosphere really should reduce ground flux to ~0.3× nominal;
small calibration call rather than a defect.

## Remaining shallow implementations

| Was | Now |
|-----|-----|
| S1 single half-saturation | Per-pair `Interaction::half_saturation` shipped (`ecosystem/src/lib.rs:830`). Default sample sets distinct values per role (`lib.rs:1789, 1889, 1903, 1917, 1938`). Resolved. |
| S2 Lindeman double-bookkeeping | Pyramid cap dropped; per-habitat assimilation efficiency is the sole mechanism (`lindeman_assimilation_for_habitat` — Aquatic 1/30, Terrestrial 1/10). Resolved. |
| S3 producers compete by simple division | Still present — `grow_producers_with_co2` divides solar irradiance equally across producers. No canopy stratification, no shade tolerance. Not on the fix programme. |
| S4 daughters don't ecologically diverge | `apply_character_displacement` (`ecosystem/src/speciation.rs:597`) called on all three speciation paths. Resolved. |
| S5 HGT is smooth interpolation | Plasmid-sweep model in `ecosystem/src/hgt.rs:2-30`: acquisition → selection → fixation-or-loss. Resolved. |
| S6 collective quorum one-dimensional | Caste-aware per-caste minimums in `sim/civ/src/drift.rs:26-47` (Reproductive 1%, Worker 50%, Soldier 10%, Nurse 10%). Resolved. |

S3 (producer competition) is the only original shallow item still
standing. Add:

- **S7 — Decomposer biomass cap is not detritus-driven**. The
  decomposer tier still runs the same logistic cap as any other
  consumer; it doesn't track dead-biomass flux. Old deferred item,
  remains deferred.

## Remaining deferred items I'd push on in a follow-up sprint

These were called out as deferred originally and are still deferred:

- **Per-cell biota (N2 above)** — the architectural ceiling. Until
  `EcoSpecies::biomass_per_cell: Vec<Real>` exists, every other
  per-cell coupling stays cosmetic on the biology side.
- **Species-registry population (N1 above)** — must land for
  `SpeciationOccurred` / `HorizontalGeneTransfer` to ever fire in
  a live run. A 1-screen change: walk `ecosystem.species` at the
  end of `sample_ecosystem_with_substrate` and emit `Species`
  records into the trait registry. Add a startup test that the
  registry length matches the ecosystem's after worldgen.
- **Adaptive radiation directional fill** — the post-extinction
  multiplier window opens correctly (`speciation.rs:97` /
  `register_extinction_event`) but `step_speciation` doesn't read
  post-extinction *role vacancies* to direct radiation into the
  cleared niches. The window fires extra speciation, but
  daughters land statistically on top of surviving roles, not
  preferentially into the holes the extinction opened.
- **Endosymbiosis / acquired organelles** — still no
  one-shot merger event for the mitochondrion / plastid
  acquisition step.
- **Body-size allometry within demography** — `mass^0.75` metabolic
  scaling absent from per-tick food demand.
- **Red Queen co-evolution** — parasite interaction strengths
  remain static; no host-parasite arms race.

## Calibration concerns

- **Tidal heating Europa-shortfall (~25× under)** is explicitly
  documented at `tidal_heating.rs:50-67` and in the fix doc lines
  340-366. The Enceladus test pins to the published `[1, 100] GW`
  band; the Europa test pins to `[0.1, 5] TW` with a `FIXME:
  calibration` comment rather than the spec's nominal `[5, 50]
  TW`. The xeno concern: a 25× under-pin on Europa's tidal budget
  cleanly forecloses subsurface-ocean habitability on icy moons
  with the current cadence. If the project is sincere about icy-
  moon biology (P1.1 added the subsurface heat reservoir for
  exactly this), the Europa calibration gap is a sign-off blocker
  for that flavour of planet.
- **`POST_EXTINCTION_BOOST_TICKS = 100`** (`speciation.rs:97`) is
  one Earth-year of post-extinction radiation under 1 tick = 1
  month. Real adaptive radiations run 10⁵-10⁷ years. This is a
  test-cadence anchor, not a paleobiology anchor.
- **`VIRUS_OUTBREAK_PERIOD = 100`** ticks (~8 years under monthly
  cadence) for episodic virus firing is plausible for an annual
  flu-like pathogen, way too slow for a bacteriophage. Single
  constant absorbs the entire virus-host time-scale axis.
- **`SOLAR_FLARE_POP_LOSS = 10%`** baseline — now correctly
  multiplied by `(1 - match_score)`, but the 10% raw number is
  still magnitude-only-direction calibrated. A Carrington-class
  event scaling against modern Earth's grid is closer to
  infrastructure collapse than to demographic loss.
- **Cosmic-ray multiplier clamped to `[1, 10]`** is conservative.
  Magnetic reversals on Earth correlate with order-of-magnitude
  cosmic ray increases at ground level, so the upper bound is
  defensibly real. The lower bound at 1 means quiet
  magnetospheres can never *suppress* divergence — see N4.

## Approval status

**APPROVED_WITH_CONDITIONS.**

C1-C7 are all fixed in production with the call paths documented
above. A live planet now runs multi-species biology each tick,
with extinction events through the emitter, civ capacity reading
the producer pool, lifecycle dispatched per-species, tolerance
gating catastrophe survival, cosmic-ray flux modulating speciation
and HGT, dormant pools seeded and drained, and per-pair
half-saturation / per-habitat Lindeman / character displacement /
plasmid sweeps / caste quorums all running. The 26-PR fix
programme is the most concentrated load-bearing wire-up the
project has shipped.

Conditions for full sign-off:

1. **Populate `species_registry` from `ecosystem.species` at
   worldgen** so speciation + HGT events actually fire in a live
   run (currently zero — N1 above).
2. **Add a startup test** asserting at least one
   `SpeciationOccurred` or `HorizontalGeneTransfer` fires within
   16k ticks on the canary seed once #1 lands, so a future
   refactor can't re-zero the registry unnoticed.
3. **Document or fix the Europa tidal under-pin.** If icy-moon
   biology is in scope, the 25× gap needs either a calibration
   follow-up or a written "out of scope for v1" stance.

#1 and #2 are roughly 1 day of work. Per-cell biota (N2) is the
obvious next-sprint architectural lift but isn't a sign-off
blocker — the planet now runs multi-species biology that the civ
feels, which was the original "credible biosphere" promise.
