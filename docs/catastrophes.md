# Catastrophes

Five cell-localized hazard kinds plus the full damage / survival /
recovery chain. Each kind has its own trigger predicate, cooldown,
and pop-loss fraction. All five flow through the same
`apply_resistance_and_dormancy` damage formula so resistance,
dormancy, and tolerance gating apply uniformly. Post-F-wave, all
five also propagate into the ecosystem via `apply_catastrophe_at_cell`.

Module layout (`sim/civ/src/catastrophe/`, split by CA5 + CB2):

| File | Concern |
|---|---|
| `mod.rs` | Facade — kind/cooldown/pop-loss constants, re-exports |
| `kind.rs` | `CatastropheKind` enum + `tag()` |
| `record.rs` | `CatastropheRecord` per-event payload |
| `triggers.rs` | Per-kind firing predicates |
| `factors.rs` | Planet-driven severity/cooldown scaling |
| `cells.rs` | Cell targeting (densest, deterministic pick, neighbours) |
| `damage.rs` | `catastrophe_cell_conditions`, `apply_resistance_and_dormancy` |
| `apply/` | Per-kind dispatchers (CB2 split) |
| `apply/volcanic.rs`, `asteroid.rs`, `disease.rs`, `solar_flare.rs`, `ice_age.rs` | One file per kind |
| `apply/mod.rs` | `check_and_apply` dispatcher (volcanic → disease → asteroid → solar flare → ice age, first hit wins) |

## The five kinds

| Kind | Trigger (`triggers.rs`) | Cooldown (years) | Pop loss | Cell scope |
|---|---|---|---|---|
| **Volcanic** | Crust temperature breaches local solidus | 200 | 5% | Single cell + neighbours; resets cell fuel, drops T 50K |
| **Disease** | Crowding density past per-substrate threshold; age floor 300y | 500 | 30% | Densest claimed cell + spreads to adjacent claimed |
| **Asteroid** | Deterministic per-tick low probability | 5,000 | 40% | Impact cell + neighbours; +5 radiation boost |
| **SolarFlare** | High stellar irradiance + weak local magnetosphere | 800 | 10% | Every cell; radiation = flare_magnitude × cosmic_ray_ground_flux |
| **IceAge** | Sustained planet-mean temperature drop | 4,000 | 20% | Every cell; -ICE_AGE_TEMP_DROP_K |

Cooldowns are per-month-tick (post-T1, see `sim/civ/src/catastrophe/
mod.rs:43-48`). Disease has a 300-year age floor so newly-founded
civs aren't insta-plagued.

Substrate scales per-kind: see `factors.rs`. Disease severity tracks
biosphere richness, volcanic cooldown tracks crust mineral content,
ice-age severity tracks atmospheric heat capacity.

## Damage formula

`damage::apply_resistance_and_dormancy` (post-P0.4 + F3) computes:

```
base_loss = raw_frac × (1 - civ.apply_catastrophe_resistance(...))
loss_after_dormancy = base_loss × (1 - dormancy × severity)
loss_after_tolerance = loss_after_dormancy × (1 - tolerance.match_score(cell))
```

Three layers of attenuation:

1. **Tool-based resistance** — civs unlock catastrophe-mitigating
   tools (PermanentMasonry, DefensiveFortification, AdaptiveAgronomy,
   etc.). `civ.apply_catastrophe_resistance(raw_frac)` reduces the
   headline severity before any biology gates kick in.
2. **Dormancy** (P1.3) — species with high `dormancy_capability`
   (e.g. tardigrade-grade extremophiles) shrug off catastrophes via
   a damage-reduction multiplier *and* deposit the surviving fraction
   into a `DormantPool` reservoir. The pool drains back to active
   population over hundreds of ticks via `step_dormant_resurrection`.
3. **Tolerance** (P0.4) — `species.tolerance.match_score(cell_T,
   cell_pH, cell_sal, cell_rad, cell_p)` scales the remaining loss
   by the cell's fit to the species envelope. An extremophile with
   `radiation_max = 20` keeps ~100% of population on a flare cell
   that rad=4; a narrow-envelope aqueous species loses everything.

`severity_factor` is currently pinned at 1.0 for all five kinds (see
`DORMANCY_SEVERITY_FACTOR` in `damage.rs`). A future polish pass
could expose it per-kind so shallow events bypass dormancy benefit.

## Cell-condition probe

`catastrophe_cell_conditions(state, planet, cell, temp_delta, extra_rad)`
returns the `(T, pH, salinity, radiation, pressure)` tuple fed into
`tolerance.match_score`. Per-cell `T` and `p` come from the physics
state (Pa → atm); pH and salinity use substrate baselines (no per-
cell ocean-chemistry field exists yet).

For radiation-driven events (SolarFlare):

```rust
let cosmic_amp = state.cosmic_ray_ground_flux().clamp(0.2, 5.0);
let post_flare_rad = baseline_radiation_flux() + solar_flare_radiation_boost() * cosmic_amp;
```

The bidirectional clamp (T8) means strong magnetospheres dampen
flare damage by up to 5× *below* baseline; magnetic-reversal windows
amplify by up to 5× *above* baseline. (Pre-T8, the floor was at 1.0,
so strong magnetospheres couldn't reduce damage.)

## Ecosystem propagation (T2)

After the civ-side pop loss, the dispatcher calls
`ecosystem.apply_catastrophe_at_cell(...)` with the same cell
conditions. The trophic web feels the catastrophe too:

| Kind | Ecosystem effect |
|---|---|
| Volcanic | Drain producer biomass at affected cell only |
| Asteroid | Drain biomass at impact + neighbours; +5 rad → species below `radiation_max` lose more |
| SolarFlare | Drain across all cells, scaled by tolerance + cosmic flux |
| IceAge | Drain across all cells with temperature drop applied to match_score |
| Disease | **Skipped** — disease is host-biology-internal, doesn't strip the trophic web |

Pre-T2 only volcanic touched the ecosystem (via `reduce_at_cell`,
no tolerance gate). Now all four reach the trophic web through the
tolerance-gated entry point.

## DormantPool seeding (P1.3)

When a catastrophe fires:

```
pop_before = civ.cohort.total()
killed = pop_before × loss_after_tolerance
dormant_seeded = killed × species.dormancy_capability × severity_factor
civ.dormant_pool.population += dormant_seeded
civ.dormant_pool.entered_tick = tick
```

So a `dormancy = 0.9` species under a full-severity catastrophe
diverts 90% of would-be casualties into cryptobiosis instead of
death. The resurrect step (`step_dormant_resurrection`) drains the
pool back into the active cohort at ~1%/tick, capped at the
pre-catastrophe population level. Result: mass-extinction recovery
is representable; a tardigrade-grade species can rebuild from a
catastrophe that would wipe a narrow species permanently.

## Magnetic-reversal amplification on SolarFlare

`state.cosmic_ray_ground_flux() = 1 / (dipole_strength + 0.1)`.
During normal periods, dipole_strength = 1.0 → flux ≈ 0.91 (close to
baseline). During reversals (`DipoleState::Reversing`), the strength
drops to 0.1 → flux ≈ 5.0, amplifying both the radiation gate on
tolerance and the speciation/HGT rates (via the cosmic-ray multiplier
in `step_speciation` / `step_hgt`).

## Tests

- `apply/<kind>.rs` per-kind tests for cooldown / trigger /
  dispatcher correctness
- `extremophile_species_survives_solar_flare_better_than_aqueous` —
  P0.4 anchor: tolerant species survives ≥3× more
- `dormant_species_survives_catastrophe_at_reduced_rate` — P1.3
- `mass_extinction_recovery_via_seed_bank_resurrection` — full loop
- `non_volcanic_catastrophes_now_affect_ecosystem` (T2) — confirms
  flare/asteroid/ice-age touch trophic web
- `extremophile_eco_species_survives_solar_flare_better` (T2 echo)
- `strong_magnetosphere_suppresses_flare_damage` (T8 bidirectional clamp)

## Event emission

Every fire emits `Event::CatastropheFired { kind, civ_id, fraction_lost,
tick }` via the run's `Emitter`. The post-run digest aggregates by
kind into the catastrophe histogram (`docs/report.md`). The narrator
(`sim/report/src/narration.rs`) renders each as a one-line story
beat: `"Year 1023: catastrophe Volcanic fired on civ Karnan — 5%
population loss."`
