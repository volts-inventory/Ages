# Catastrophes

Five cell-localized hazard kinds — Volcanic, Disease, Asteroid,
SolarFlare, IceAge. Each has its own per-tick trigger predicate,
per-kind cooldown, base population-loss fraction, and per-cell
damage propagation pattern. Severity scales with the planet's
substrate so disease severity comes out of biosphere richness,
volcanic cooldown out of crust mineralogy, ice-age severity out
of mean temperature.

For deeper detail per file, see
[`sim/civ/src/catastrophe/`](../sim/civ/src/catastrophe/) and
[`sim/civ/README.md`](../sim/civ/README.md). After CA5+CB2 the
crate is split across `kind`, `record`, `damage`, `apply`,
`triggers`, `factors`, `cells`, with `apply` further split into
one file per kind.

## Module layout

| File | Responsibility |
|------|----------------|
| `mod.rs` | Cooldown / pop-loss constants + public re-exports. |
| `kind.rs` | `CatastropheKind` enum + the `.tag()` string used in `CatastropheFired::catastrophe_kind`. |
| `record.rs` | `CatastropheRecord { kind, fraction_lost }` — what `check_and_apply` returns to the caller. |
| `triggers.rs` | Per-kind firing predicates: `volcanic_fires`, `disease_fires`, `asteroid_fires`, `solar_flare_fires`, `ice_age_fires`. |
| `factors.rs` | Planet-driven severity/cooldown scalars (`disease_severity_factor`, `volcanic_cooldown_factor`, `ice_age_severity_factor`). |
| `cells.rs` | Cell-targeting helpers — `densest_claimed_cell`, `deterministic_cell_pick`, `hex_neighbors`, `apply_to_cell_and_neighbors`. |
| `damage.rs` | The shared `apply_resistance_and_dormancy` formula + `catastrophe_cell_conditions` builder feeding the tolerance gate. Also owns the per-kind radiation / temperature delta constants. |
| `apply/mod.rs` | `check_and_apply` — per-tick orchestrator that dispatches across the five per-kind handlers. |
| `apply/{volcanic,disease,asteroid,solar_flare,ice_age}.rs` | Per-kind handler — owns the cooldown gate, trigger call, cell-target, cosmology push, and (for everyone but disease) the ecosystem coupling call. |

## Five kinds

| Kind | Trigger (`triggers.rs`) | Cell selection | Severity scaling |
|------|-------------------------|----------------|------------------|
| `Volcanic` | Any cell with `|charge| > 80` AND `temperature > 600 K` ([`triggers.rs:18`](../sim/civ/src/catastrophe/triggers.rs#L18)) | The firing cell + its 6 hex neighbours (claimed only) | Crust composition (`volcanic_cooldown_factor`) stretches the cooldown — basaltic = 1.0×, hydrocarbon = 0.8×, piezoelectric = 1.4×, ferrous = 1.1×, rare-earth = 1.5× ([`factors.rs:27`](../sim/civ/src/catastrophe/factors.rs#L27)) |
| `Disease` | Civ crowding ≥ 80% of carrying capacity AND civ age ≥ `DISEASE_AGE_FLOOR_TICKS` (stretched by metabolic substrate) ([`triggers.rs:32`](../sim/civ/src/catastrophe/triggers.rs#L32)) | Densest claimed cell + adjacent claimed cells | Biosphere class (`disease_severity_factor` — none 0.2×, sparse 0.6×, lush 1.0×, hyper-biodiverse 1.5×) |
| `Asteroid` | Deterministic firing window: `tick.is_multiple_of(4733 × MONTHS_PER_YEAR)` AND `tick > 0` ([`triggers.rs:53`](../sim/civ/src/catastrophe/triggers.rs#L53)) | Deterministic per-(tick, civ_id) hash via `deterministic_cell_pick`, with 2× damage at the impact cell + 0.5× at adjacent claimed cells | Base 40% pop loss; asteroid radiation boost (5.0) added on the ecosystem side so narrow-envelope species lose the most |
| `SolarFlare` | Weak/no magnetosphere AND stellar luminosity > 1500 W/m² AND `tick.is_multiple_of(period.max(1))` where `period = 1567 × MONTHS_PER_YEAR / flare_rate` keyed off the host star's spectral type ([`triggers.rs:78`](../sim/civ/src/catastrophe/triggers.rs#L78)) | Densest claimed cell as the representative reading; full-civ aggregate effect | Stellar luminosity gates the trigger; magnetic-reversal `cosmic_ray_ground_flux` clamps to `[0.2, 5.0]` and multiplies the flare radiation boost (1.0) — strong dipole attenuates up to 5×, weak/reversing field amplifies up to 5× |
| `IceAge` | Planet mean temperature ≤ 260 K AND civ age ≥ 1000 baseline-years AND `tick.is_multiple_of(2917 × MONTHS_PER_YEAR)` ([`triggers.rs:112`](../sim/civ/src/catastrophe/triggers.rs#L112)) | Densest claimed cell with a 50 K cold-snap delta applied to the cell conditions | `ice_age_severity_factor` scales linearly with how far the planet sits below 273 K (every 20 K = +1×; capped at 60% loss) ([`factors.rs:43`](../sim/civ/src/catastrophe/factors.rs#L43)) |

`CatastropheFired { kind, civ_id, cell, severity, tick }` is
emitted on every fire, plus follow-up `CivTerritoryChanged` if
the catastrophe unclaims cells.

## Per-month scaled cooldowns

Each kind's cooldown lives in
[`catastrophe/mod.rs:43`](../sim/civ/src/catastrophe/mod.rs#L43)
and is multiplied by `protocol::MONTHS_PER_YEAR` so the
year-equivalent cadence stays right under the "1 tick = 1 month"
calendar:

| Kind | Cooldown | Notes |
|------|----------|-------|
| `Volcanic` | 200 baseline-years | Multiplied by `volcanic_cooldown_factor(crust)` per cell |
| `Disease` | 500 baseline-years | Stretched by `streak_ticks_for_metabolism(.., substrate.metabolism())` — silicate civs see plagues at ~5× longer absolute cadence than aqueous ones, so per-generation hit rate stays constant across substrates |
| `Asteroid` | 5000 baseline-years | Combined with the deterministic firing window, lands an asteroid every ~5000-10000 ticks per civ |
| `SolarFlare` | 800 baseline-years | Per-kind cooldown caps the realised flare frequency for high-rate spectral classes; the trigger period (spectral-rate-divided) determines the cadence *between* cooldown windows |
| `IceAge` | 4000 baseline-years | Rare on the run timescale |

`DISEASE_AGE_FLOOR_TICKS = 300 × MONTHS_PER_YEAR` — the
civ-age floor for disease, stretched by the substrate metabolism
factor for the same reason as the cooldown.

## Pop-loss base fractions

Raw base fractions, in
[`catastrophe/mod.rs:51`](../sim/civ/src/catastrophe/mod.rs#L51).
These feed `Real::from((numerator, denominator))` and pass
through the damage formula before landing on the cohort:

| Kind | Base loss | After full damage formula? |
|------|-----------|----------------------------|
| `Volcanic` | 5% | `raw × (1 − tools) × (1 − match) × (1 − dormancy × severity)` |
| `Disease` | 30% | Multiplied by `disease_severity_factor` before the formula |
| `Asteroid` | 40% | Then 2× at impact cell + 0.5× at adjacent claimed cells |
| `SolarFlare` | 10% | Cell-aggregate; same formula |
| `IceAge` | 20% | Multiplied by `ice_age_severity_factor`, capped at 60% |

## Damage formula

`apply_resistance_and_dormancy`
([`damage.rs:160`](../sim/civ/src/catastrophe/damage.rs#L160))
turns the raw base fraction into the realised cohort hit:

```
base_loss      = raw_frac × (1 − civ_tool_resistance) × (1 − match_score)
realised_loss  = base_loss × (1 − dormancy × severity)
```

- **`raw_frac`** — the kind's base loss after planet-side
  severity scalars are applied (biosphere for disease,
  temperature for ice age).
- **`civ_tool_resistance`** — `Civ::apply_catastrophe_resistance`
  sum of unlocked tools with the `catastrophe_resistance` effect
  (PermanentMasonry, MedicalIntervention, etc.). Clamped to a
  floor so a fully-shielded civ still takes residual damage.
- **`match_score`** — `species.tolerance.match_score(t, ph, sal,
  rad, p)` against the cell's post-event conditions. `1.0` ⇒
  perfect envelope fit ⇒ zero damage; `0.0` ⇒ outside envelope
  ⇒ full damage.
- **`dormancy`** — `species.dormancy_capability` in `[0, 1]`.
  Tardigrade-grade species (~0.9) take ~10× less damage from
  catastrophes than narrow-envelope (`dormancy = 0`) species.
- **`severity`** — `DORMANCY_SEVERITY_FACTOR` pinned at `Real::ONE`
  ([`damage.rs:18`](../sim/civ/src/catastrophe/damage.rs#L18))
  for all five kinds; a future polish pass can expose a per-kind
  table if shallow events should bypass dormancy benefit.

`match_score = 1` ⇒ zero damage; `match_score = 0` ⇒ full
damage. The rearrangement (vs. the pre-P1.3 form that applied
`(1 − match_score)` last) exposes `base_loss` — the loss
fraction the species would suffer *without* its dormancy trait
— so it can route the diverted casualties into the seed bank
(see below).

## Per-cell catastrophe application

Catastrophes are heterogeneous: a volcanic eruption on cell N
hits cell N + its 6 hex neighbours; an asteroid hits the impact
cell + neighbours with a damage gradient; disease hits the
densest cell + neighbours. The shared helper is
`apply_to_cell_and_neighbors`
([`cells.rs:70`](../sim/civ/src/catastrophe/cells.rs#L70)):

```rust
apply_to_cell_and_neighbors(
    civ,
    grid_width, grid_height,
    center,                  // u32 cell index
    center_frac,             // Real — fraction drop at the center
    neighbor_frac,           // Real — fraction drop at each neighbour
    claimed_only,            // bool — restrict to civ's claimed cells
)
```

Neighbour lookups use `hex_neighbors`
([`cells.rs:47`](../sim/civ/src/catastrophe/cells.rs#L47)) with
the same 6-axial-offset table as `sim_core::compute_territory`,
torus-wrapped, so catastrophe geometry agrees with territory
expansion.

### Per-kind cell-targeting

- **Volcanic** — fires at the first cell satisfying the
  charge/temperature predicate, then `drop_cell_pop(center,
  frac)` (no neighbour propagation in the current code —
  ecosystem coupling is per-cell at `cell_conds`).
- **Disease** — `densest_claimed_cell` is the origin;
  `center_frac = frac × 2`, `neighbour_frac = frac`,
  `claimed_only = true`.
- **Asteroid** — `deterministic_cell_pick(civ, tick)` hashes
  `(tick, civ_id)` into an index over the claimed-cell set;
  `center_frac = frac × 2`, `neighbour_frac = frac / 2`,
  `claimed_only = true`.
- **SolarFlare** — densest-cell reading drives the tolerance
  gate; the cohort-side effect is a uniform aggregate shrink
  (`cohort.shrink_to(target)`) because flare disruption hits
  the whole hemisphere.
- **IceAge** — densest-cell reading with a 50 K cold-snap
  delta drives tolerance; cohort-side effect is the same
  uniform aggregate shrink.

For civs without any claimed cells (legacy fixtures / tests),
each handler falls back to a uniform `cohort.shrink_to` so the
catastrophe still has an effect.

## Tolerance-gated survival

`catastrophe_cell_conditions`
([`damage.rs:78`](../sim/civ/src/catastrophe/damage.rs#L78))
builds the `(temperature, pH, salinity, radiation, pressure)`
tuple a catastrophe-affected cell exposes to the tolerance
envelope. Two of the five axes drive the catastrophe
differential:

- **temperature** — cell's read-out temperature plus the
  catastrophe's `temp_delta_k` (`-50 K` for ice age, `0` for
  the others; volcanic mutates cell temp by `-50 K` directly).
- **radiation** — `baseline_radiation_flux() + extra_rad`
  where `extra_rad` is `0` for most kinds, but `1.0 × cosmic_amp`
  for solar flare (`cosmic_amp` clamped to `[0.2, 5.0]` from
  the magnetic-reversal state) and `5.0` for asteroid (ground-
  level prompt gamma + activation products).

The other three axes (pH, salinity, pressure) sit at substrate
defaults — pinned to envelope centres so they read as non-
binding gates under default planets. A future per-cell ocean-
chemistry field can plug in here.

### Extremophile survival

The headline observable: a species whose tolerance envelope
contains the post-event cell conditions rides out the
catastrophe. Tests:

- **Solar flare + radiation extremophile**
  ([`apply/solar_flare.rs:117`](../sim/civ/src/catastrophe/apply/solar_flare.rs#L117))
  — same flare, two civs differing only in `tolerance`. The
  extremophile (`radiation_max = 20`) takes ~18× less damage
  than the aqueous-default (`radiation_max = 0.5`) species and
  retains strictly more population.
- **Cell-outside-envelope full damage**
  ([`apply/mod.rs:120`](../sim/civ/src/catastrophe/apply/mod.rs#L120))
  — a species whose envelope is nowhere near the catastrophe
  cell takes the full `raw_frac` loss (no tools, no dormancy).
- **Centre-of-envelope zero damage**
  ([`apply/mod.rs:149`](../sim/civ/src/catastrophe/apply/mod.rs#L149))
  — a species at the exact centre of every axis takes zero
  loss regardless of `raw_frac`.

## DormantPool seeding + resurrection

P1.3 — when a high-dormancy species takes a catastrophe, the
people the catastrophe would have killed without the dormancy
trait don't simply vanish. They enter cryptobiosis and join
`civ.dormant_pool.population`
([`sim/civ/src/lib.rs:477`](../sim/civ/src/lib.rs#L477)).

`apply_resistance_and_dormancy` deposits

```
dormant_seeded = pop_before × base_loss × dormancy × severity
```

into the dormant pool
([`damage.rs:194`](../sim/civ/src/catastrophe/damage.rs#L194))
— equivalent to `pop_before × (base_loss − after_dormancy)`,
i.e. the headcount the dormancy multiplier just diverted out of
the death column. `entered_tick` is set so post-run telemetry
can locate the cryptobiosis event.

`civ.pre_catastrophe_population` is also bumped to track the
high-water mark of the civ's active population. This becomes the
resurrection cap so a civ that suffers multiple consecutive
catastrophes can recover to the largest cohort it ever held,
not just its founder population.

### Resurrection

`Civ::step_dormant_resurrection`
([`sim/civ/src/capacity.rs:413`](../sim/civ/src/capacity.rs#L413))
drains the reservoir at 1%/tick back into the active cohort,
distributed proportionally so the resurrection doesn't pile on
one cell. Driven each tick from the capacity step
([`capacity.rs:342`](../sim/civ/src/capacity.rs#L342)).

The mass-extinction recovery test
([`apply/mod.rs:190`](../sim/civ/src/catastrophe/apply/mod.rs#L190))
fires a 100% catastrophe at a `dormancy = 0.9` species, then
runs 500 ticks of resurrection — active population recovers to
≥ 99% of pre-event.

## Ecosystem coupling

Four of the five kinds (everything except disease) also drain
`PlanetEcosystem` biomass at the catastrophe cell via
`apply_catastrophe_at_cell(raw_frac, t, ph, sal, rad, p)` on the
per-cell conditions tuple. Disease stays biology-internal per
spec — pathogen pressure is on the civ host, not the wider
trophic web.

Each ecosystem species' own `tolerance` envelope (not the host
species') gates the realised biomass loss. Net effect: an
asteroid strike sterilises narrow-envelope producers + consumers
at the impact cell while wide-envelope extremophiles ride it
out; a volcanic eruption starves the eruption cell's primary
producers; a solar flare wipes radiation-sensitive species
across the densest claimed cell.

T2 acceptance test
([`apply/asteroid.rs:134`](../sim/civ/src/catastrophe/apply/asteroid.rs#L134))
fires an asteroid at an aqueous-default eco fixture and asserts
every extant species takes a strict biomass drop. The companion
test
([`apply/solar_flare.rs:201`](../sim/civ/src/catastrophe/apply/solar_flare.rs#L201))
exercises the eco-side extremophile-vs-narrow-envelope
differential mirroring the civ-side P0.4 test.

## Magnetic-reversal amplification on SolarFlare

The solar-flare handler
([`apply/solar_flare.rs:60`](../sim/civ/src/catastrophe/apply/solar_flare.rs#L60))
reads `state.cosmic_ray_ground_flux()` (T8 magnetism state),
clamps to `[0.2, 5.0]`, and multiplies the flare's
`solar_flare_radiation_boost()` baseline by that scalar before
feeding the cell-conditions tuple:

```rust
let cosmic_amp = state
    .cosmic_ray_ground_flux()
    .max(Real::from_ratio(2, 10))
    .min(Real::from_int(5));
let rad_boost = solar_flare_radiation_boost() * cosmic_amp;
```

The raw flux is `1 / (dipole_strength + 0.1)`, spanning ~`[0,
10]` across the dipole envelope:

| Dipole state | Cosmic flux | Clamp | Flare amp |
|--------------|-------------|-------|-----------|
| Strong dipole (`= 10.0`) | ≈ 0.099 | 0.2 | 5× attenuation — shielded surface sees only 1/5 of nominal flare radiation |
| Earth-normal | ≈ 1.0 | 1.0 | nominal |
| Reversing (`= 0.1`) | ≈ 5.0 | 5.0 | 5× amplification — narrow-envelope species blown past `radiation_max` |

T8 acceptance test
([`apply/solar_flare.rs:316`](../sim/civ/src/catastrophe/apply/solar_flare.rs#L316))
runs two identical flare-firing planets, one with
`dipole_strength = 10.0` and one with `dipole_strength = 0.1`,
and asserts the strong-magnetosphere run takes strictly less
damage. The 0.2 floor preserves a minimum flare effect even on
heavily shielded worlds — the flare's particle spectrum isn't
entirely magnetic-deflectable.

## Spectral-type flare cadence

T18 — the solar-flare period scales with the host star's
spectral-class flare-rate multiplier
([`triggers.rs:97`](../sim/civ/src/catastrophe/triggers.rs#L97)):

| Spectral type | Rate | Effective period (× base 1567 years) |
|---------------|------|--------------------------------------|
| M dwarf | 100× | `base / 100` ≈ 16 years |
| K dwarf | 10× | `base / 10` ≈ 157 years |
| G dwarf | 1× | base ≈ 1567 years |
| F dwarf | 0.3× | `base × 10/3` ≈ 5223 years |
| A dwarf | 0.1× | `base × 10` ≈ 15670 years |

The per-flare cooldown (`SOLAR_FLARE_COOLDOWN_TICKS = 800 ×
MONTHS_PER_YEAR`) caps the realised frequency for high-rate
spectral classes, but the cadence *between* cooldown windows is
spectral-aware: a habitable-zone M-dwarf planet feels the
"100× flares" the M-dwarf calibration promises.

## Cosmology pushes

Each kind applies a deterministic push to the civ's cosmology
vector via `Civ::apply_cosmology_push`:

| Kind | Empirical | Communitarian | Reformist | Mystical | Hierarchical |
|------|-----------|---------------|-----------|----------|--------------|
| Volcanic | — | — | — | — | — (handler doesn't push) |
| Disease | 0 | +15% | -5% | +15% | +5% |
| Asteroid | -5% | +10% | +15% | +20% | -5% |
| SolarFlare | +15% | 0 | +10% | +5% | 0 |
| IceAge | 0 | +20% | -5% | +5% | +15% |

Disease pivots toward communitarian + mystical (plague-cosmology
pattern); asteroid pivots mystical + reformist (rebuild
pressure); solar flare pivots empirical + reformist (the
species observes the flare directly, driving observational
science); ice age pivots communitarian + hierarchical (huddle
together + centralized resource management).

## Substrate-relative severity

Catastrophe severity scales with substrate priors so different
planets feel different hazard profiles:

- A high-biosphere ammoniacal world has frequent low-severity
  disease.
- A low-biosphere silicate world has rare high-severity disease
  (every outbreak in a thin biosphere is more devastating).
- A volcanic crust raises both volcanic frequency and per-event
  severity.

This is the "different worlds, different hazards" counterpart
to the "different worlds, different sciences" theme.

## Catastrophe events

```
CatastropheFired { kind, civ_id, cell, severity, tick }
```

The `severity` scalar is `[0, 1]` — fraction of cell population
removed after the full damage formula (tools, tolerance,
dormancy) applies. Survivors take residual stress for several
ticks (lower birth rate, higher death rate).

The post-run rendering layer renders catastrophes as anchors in
the per-civ timeline; severe events bubble into the highlights
reel (see [report.md](report.md)).
