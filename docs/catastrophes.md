# Catastrophes

Five cell-localized hazard kinds. Severity scales with the planet's
substrate so disease severity comes out of biosphere richness,
volcanic cooldown out of crust mineral content, ice-age severity
out of mean temperature.

For deeper detail per crate, see
[`sim/civ/src/catastrophe/`](../sim/civ/src/catastrophe/) and
[`sim/civ/README.md`](../sim/civ/README.md).

## Five kinds

| Kind | Trigger | Cell selection | Severity scales with |
|------|---------|----------------|----------------------|
| `Volcanic` | Crust mineral hot-zone + cooldown | Per-cell volcanic risk priors | Crust mineral richness |
| `Disease` | Endemic; biosphere reservoir | Densest claimed cell | Biosphere density |
| `Asteroid` | Stochastic (low base rate) | Deterministic `(seed, tick)`-keyed cell | Crust composition (impact substrate) |
| `SolarFlare` | Stellar luminosity priors | Whole-hemisphere; population effect proportional to tech-shielding | Luminosity |
| `IceAge` | Mean-temperature priors | Cells below seasonal-floor threshold | `(planet.mean_temperature - threshold)` magnitude |

Each catastrophe emits `CatastropheFired { kind, cell, severity,
tick }` plus follow-up `CivTerritoryChanged` if the catastrophe
unclaims cells.

## Cell-localized

Disease, asteroid, and volcanic always target specific cells.
Solar flare and ice age have hemispheric / multi-cell footprints
but still reduce to per-cell population effects so the
population-dynamics phase can apply them uniformly.

A civ may absorb a catastrophe without collapsing if its
population is concentrated outside the affected cell(s); a civ
whose densest cell takes a disease hit may collapse if the loss
exceeds its total-population fraction.

## Cooldowns

`Volcanic` cooldown: 200 baseline-years (`200 ×
MONTHS_PER_YEAR`). After firing, the cell can't fire volcanic
again until cooldown elapses.

`Disease` cooldown: 500 baseline-years per (civ, region) pair,
stretched by the substrate-metabolism factor — a silicate civ
sees plagues at a 5× longer absolute cadence than an aqueous one,
so per-generation hit rate stays constant across substrates. The
civ-age floor for disease (300 baseline-years post-founding) is
likewise stretched. The other four kinds are physics-driven
(stellar / orbital / geological) and keep raw tick cooldowns
regardless of substrate.

Asteroid, solar flare, ice age have their own per-cell cooldowns
sized to be rare events on the run timescale.

## Tech shielding

Some tier-2+ tools contribute to `catastrophe_resistance` (the
effect category in the tool spec). When a catastrophe fires on a
civ with shielding, severity multiplies by `(1 - resistance)`
clamped to a floor.

`solar_flare` is the canonical example: a civ with `electromagnetism`
or higher tier suffers a fraction of the loss an unshielded civ
takes. The tool is what makes Faraday-cage-equivalent shelter
possible — no shielding without the underlying physics being
known.

## Substrate-relative severity

Catastrophe severity scales with substrate priors so different
planets feel different hazard profiles:

- A high-biosphere ammoniacal world has frequent low-severity
  disease.
- A low-biosphere silicate world has rare high-severity disease
  (every outbreak in a thin biosphere is more devastating).
- A volcanic crust raises both volcanic frequency and per-event
  severity.

This is the "different worlds, different hazards" counterpart to
the "different worlds, different sciences" theme.

## Catastrophe events

```
CatastropheFired { kind, civ_id, cell, severity, tick }
```

The `severity` scalar is `[0, 1]` — fraction of cell population
removed before tech shielding applies. Survivors take residual
stress for several ticks (lower birth rate, higher death rate).

Post-run report renders catastrophes as anchors in the per-civ
timeline; severe events bubble into the highlights reel.
