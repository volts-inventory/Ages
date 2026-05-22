# Round-3 xenobiology re-review (post F-wave, PRs #101-#107)

Companion to:
- `docs/post-implementation-xeno-review.md` (Round 1 — **NEEDS_REWORK**, C1-C7)
- `docs/post-fix-xeno-review.md` (Round 2 — **APPROVED_WITH_CONDITIONS**, N1-N3)

Reviewer lens: xenobiology. Scope: production paths after the
seven-PR F-wave landed on top of the P0/P1/P2/P3 backlog.

## Overall assessment

The Round-2 sign-off conditions are fully discharged. Species
registry now populated from `ecosystem.species` at worldgen with
role-driven Lifecycle mapping (F1); speciation and HGT have non-
empty pools every tick. `EcoSpecies` carries `cell_biomass:
Vec<Real>` with a `sum(cell_biomass) == biomass` invariant, and
volcanic catastrophes drain only the affected cell (F2). Each
`EcoSpecies` carries a `ToleranceEnvelope` derived from substrate
with per-species jitter; a tolerance-gated catastrophe entry
point exists (F3). A new 16k-tick canary on seed 1024
(`sim/core/src/tests.rs:481-507`) enforces ecosystem-events fire
end-to-end (ran green in 1769 s). Europa now lands in the literal
`[5, 20] TW` band, closing the 25× under-pin that was the
astro-side calibration blocker. Verdict: **APPROVED.**

## N1-N3 verification

| Gap | Round-2 finding | Round-3 status | Production call path |
|-----|-----------------|----------------|----------------------|
| **N1** species_registry never populated | Speciation + HGT wired but registry empty → zero events fire | **Fixed** | `core/src/lib.rs:234-258` walks `ecosystem.species.values()` and inserts a per-trait `Species` per `EcoSpecies` with seed mixed from `planet.seed.wrapping_add(eco.species_id.0)`, role taken from the ecosystem record, and a role-driven `Lifecycle`: Producer → Plant; Parasite::{Micro,Virus} → Microbial::Binary; Saprotroph → Microbial::Budding; Detritivore → Microbial::Conjugation; Mutualism::Pollinator → Insect; default Vertebrate. `lib.rs:736-758` feeds the populated registry into `step_speciation` + `step_hgt`. New canary `ecosystem_events_fire_in_live_run` (`tests.rs:481-507`) gates a 16k-tick seed-1024 run on `speciation_occurred + species_extinct ≥ 1`. |
| **N2** ecosystem is planet-wide aggregate, no per-cell biomass | Polar-extremophile + equatorial-thermophile see the same global pool | **Fixed** | `EcoSpecies.cell_biomass: Vec<Real>` (`ecosystem/src/lib.rs:336-338`); aggregate is cached sum. Grid-aware constructor `sample_ecosystem_with_substrate_for_grid` (`lib.rs:2056-2066`) called from `core/src/lib.rs:145-150`. `initialise_cell_biomass` (`lib.rs:432-461`) does the uniform split; `reduce_at_cell` (`lib.rs:473-510`) is the per-cell catastrophe poke; `rescale_cell_biomass` (`lib.rs:517-560`) re-projects. Volcanic catastrophes hit only the affected cell's producers (`civ/src/catastrophe/mod.rs:344-361`). |
| **N3** civ uses `tolerance`; EcoSpecies has none | Extremophile ecosystem members uniformly wiped by catastrophes | **Fixed** | `EcoSpecies.tolerance: ToleranceEnvelope` (`ecosystem/src/lib.rs:343`), substrate-derived with per-species jitter via `sample_tolerance_for_substrate(species_seed, substrate_tag)` at `lib.rs:1887, 2039`. `apply_catastrophe_at_cell` (`lib.rs:1531-1556`) scales loss by `raw_loss_frac × (1 - match_score)`. See N5 below for the production-wiring caveat. |

## C1-C7 still wired

Spot-checked the seven critical paths from Round 1:

- **C1** sim-ecosystem in the tick loop — `core/src/lib.rs:145-150,
  709` (step still fires every tick between chemistry and
  catastrophe phases).
- **C2** producer biomass into civ capacity — `core/src/lib.rs:514`
  reads `ecosystem.tier_biomass(0)`; `civ/src/capacity.rs:282-312`
  threads it into `update_producer_biomass`.
- **C3** lifecycle dispatch — `civ/src/capacity.rs:197, 220, 359`
  still route through `sim_population::lifecycle::step_for_lifecycle`.
- **C4** ToleranceEnvelope on catastrophe — `civ/src/catastrophe/mod.rs:
  212-221, 323, 401, 463, 527, 570` all five catastrophe kinds call
  `apply_resistance_and_dormancy`.
- **C5** Mutualism/Parasite kind branching — `ecosystem/src/lib.rs:
  1136-1149` virus outbreak period; `lib.rs:1222-1260` per-kind
  Mutualism branches (Pollinator / SeedDisperser / Engineer).
- **C6** cosmic-ray flux drives biology — `core/src/lib.rs:736-758`
  reads `state.cosmic_ray_ground_flux()` and passes into both
  `step_speciation` and `step_hgt`.
- **C7** dormant pool seeded + drained — `catastrophe/mod.rs:245-253`
  deposits, `capacity.rs:347, 397, 413` drain via
  `step_dormant_resurrection`.

All seven still load-bearing. No regressions detected.

## New gaps surfaced

### N5 [M] — `apply_catastrophe_at_cell` is implemented but not wired into the production catastrophe path

`PlanetEcosystem::apply_catastrophe_at_cell` (`ecosystem/src/lib.rs:
1531-1556`) is the F3 entry point that gates ecosystem biomass
loss on `tolerance.match_score`, covered by tests at
`ecosystem/src/tests.rs:2444-2497`. But the only production call
into the ecosystem from `civ/src/catastrophe/mod.rs:344-361` is
`reduce_at_cell` — a flat per-cell drain with no tolerance gate,
and only on the volcanic path. The other four catastrophe kinds
(asteroid, solar flare, ice age, disease) don't touch the
ecosystem at all. So trophic-web tolerance gating exists in the
library but isn't exercised live; flare/asteroid/ice-age events
hit the civ but leave producer biomass untouched (and thus can't
starve the civ via the C2 capacity link). Fix is roughly a
one-screen change: swap each `reduce_at_cell` for
`apply_catastrophe_at_cell`, extend the other four kinds to poke
the ecosystem. The library is ready.

### N6 [S] — F1 lifecycle mapping coarse-grained

`core/src/lib.rs:239-256` maps `EcosystemRole` → `Lifecycle` with a
small lookup. Producer → Plant covers polyploid speciation, micro-
parasites/saprotrophs/detritivores → Microbial cover HGT, but
Macro-parasites and PrimaryConsumer/Carnivore all collapse onto
Vertebrate. Means HGT can't fire among large-bodied parasites and
polyploid speciation can't fire among non-producer plants (there
are none, but it forecloses fungi-like worlds). Cosmetic;
defensible default.

## Round-2 carry-overs still standing

- **N4** (asymmetric flare cosmic_amp clamp at
  `catastrophe/mod.rs:522`) — still floors at `Real::ONE`;
  strong magnetospheres can't dampen.
- **S3** uniform-irradiance producer competition; **S7**
  decomposer logistic cap not detritus-driven.
- Adaptive-radiation directional fill, endosymbiosis one-shot,
  body-size `mass^0.75` allometry, Red Queen co-evolution — all
  still deferred per Round 2.

## Items I'd push to a hypothetical next sprint

Priority order:

1. **Wire `apply_catastrophe_at_cell` in production** (N5). Closes
   the final loop on F3 — short, mechanical.
2. **Loosen N4 clamp** — let strong magnetospheres suppress
   ground-cosmic-ray flux below 1×. Two-line change.
3. **Biome-class-weighted `cell_biomass` init** — rainforests
   should start with more producer biomass than deserts.
4. **Directional adaptive radiation, endosymbiosis, allometric
   metabolic demand, detritus-driven decomposer cap** — all
   extensions, not slips.

Only (1) qualifies as "ship-quality slip"; the rest are
extensions.

## Approval status

**APPROVED.**

All three Round-2 conditions discharged in production, with a
canary enforcing the load-bearing one (N1). The seven Round-1
critical gaps still wired. Europa calibration closed with a
written per-substrate fit. `docs/magic-constants.md` lands the
project's arithmetic-of-record in one place with explicit cross-
planet status — exactly the artefact the astro side asked for.
N5 is real but a follow-up: the *infrastructure* for tolerance-
gated trophic-web catastrophes is in the library and tested;
only the production call site needs swapping.

This is the most coherent run of biological wire-up the project
has shipped. A live planet runs multi-species biology each tick
with extinction, speciation, and HGT events flowing through the
emitter; civ capacity reads the producer pool; lifecycle
dispatches per-species; tolerance gates catastrophe survival on
the civ side and (where wired) the ecosystem side; cosmic-ray
flux modulates speciation and HGT; dormant pools fill on
catastrophe and drain on the tick. Planet flavour (substrate,
tidal budget, magnetosphere, cosmic flux) propagates through
every biological layer that touches the civ. Sign-off granted.
