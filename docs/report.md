# Post-run report

Two consumers read the canonical NDJSON event log emitted during a
simulation run:

- **`ages-report`** — Rust binary that walks the log, scores events,
  and emits a structured Markdown digest (one file per run).
- **`ages --replay-narration <log>`** — replays the event stream as
  human-readable narration. Same logic as the `--narration` live flag,
  decoupled from the simulator (see `docs/getting-started.md`).

This doc covers the Markdown digest; the live viewport is documented
separately at `docs/viewport.md`.

## Pipeline

```
ages → NDJSON event stream → ages-report → digest.md
                          ↘ narrator → stdout
```

Source: `sim/report/src/digest/`, `sim/report/src/highlights.rs`,
`sim/report/src/render/`, `sim/report/src/narration.rs`.

## Digest structure

The Markdown digest has four sections, top-down:

### 1. Planet header (`render/planet.rs`)

Captures the deterministic world-gen sample:

- Spectral type + age + bolometric luminosity (from `Star`)
- HZ inner/outer edges + planet orbital distance — flags
  in-HZ / hot / cold via `hz_factor`
- Mass, radius, derived gravity, escape velocity
- Locking state (Synchronous / Resonance / FreeRotator)
- Crust + base albedo
- Substrate, atmosphere class, surface pressure
- Mean temperature + equator-pole gradient
- Moon count + moon eccentricities

The ASCII map underneath is rendered with the planet's
[`SurfacePhase`](viewport.md#surface-phase) (Earthlike / Lava /
IceCap), derived via `render::surface_phase_for_digest` from the
captured `Digest::metadata` (substrate freeze/boil) + the planet
event's `substrate_perturbation`. The legend line above the map
adapts to the phase — `▒ inland · ░ coast · ~ shallow · ≈ deep`
for Earthlike, `* magma plain` for Lava, `+ ice sheet` for
IceCap.

### 2. Highlights (`highlights.rs`)

Scored event picks — the digest doesn't dump every event; it ranks
them by `score()` and emits the top entries with one-line narration.
Covered variants:

- `CivFounded`, `CivCollapsed`, `Refound`, `Breakaway`
- `FigureBorn` (with charisma/curiosity/doubt)
- `TechUnlocked` + tier
- `RelationConfirmed` / `Falsified` / `Revalidated` / `Lapsed` /
  `Mythologized`
- `RefinementProposed` / `Confirmed` / `Rejected`
- `CatastropheFired` + kind + civ + pop loss
- `SpeciesExtinct` + cause (PopulationCollapse / KeystoneCascade /
  Catastrophe)
- `SpeciationOccurred` + trigger + parent/daughter ids
- `HorizontalGeneTransfer` + donor/recipient + trait
- `CivResilienceTick` — only highlighted when resilience drops
  below 0.5 (degraded) or rises above 1.5 (thriving)
- `KnowledgeTransmitted`, `KnowledgeDiffused`, `Contact`, `War`,
  `Peace`, `AllianceFormed`, `AllianceDissolved`, `TradeRouteOpened`,
  `TradeRouteClosed`

Each scored event becomes one Markdown line via `scored_line()` arms.
Until F-wave the score table had several entries with no matching
arm; PR #129 (viewport rewrite) closed those gaps.

### 3. Ecosystem summary (`digest/build.rs::aggregate_ecosystem`)

Per-run aggregates surfaced post-F-wave:

- Total ecosystem species count (extant vs extinct)
- Total speciation events (by trigger kind)
- Total HGT events (by trait swapped)
- Catastrophe-kind histogram (Volcanic / Disease / Asteroid /
  SolarFlare / IceAge)
- Mean civ resilience over the run
- Whether any magnetic reversal fired
- (Placeholders, not yet emitted by protocol: Hadley cell count,
  mean jet velocity, total tidal heating budget, mean subsurface T)

### 4. Per-civ panels

For each civ that founded during the run:

- Founded tick + lifetime (in years)
- Peak population
- Peak claimed cells
- Final tech tier reached
- Catastrophes survived (per kind)
- Notable figures (with charisma + dominant trait)
- Final cosmology + religion drift

## Highlights scoring

`highlights::score()` returns `Option<i32>` per event. Higher scores
rank earlier. Some events are unconditional (every `CivFounded`
appears); others gated (`CivResilienceTick` only at extremes).
Tunable per-event pins live in `highlights.rs`.

## Determinism

Same input log produces bit-identical digest output. The digest walks
events in stream order and uses sorted `BTreeMap` iteration for any
per-species or per-civ aggregation — same determinism contract as
the simulator (`docs/architecture.md`).

## Narration replay

`ages --replay-narration <log>` reads the same NDJSON but emits per-
event prose instead of the digest. Source:
`sim/report/src/narration.rs`. `NarratorState` tracks names so later
lines say "Karnan" instead of `civ 1`. Used by:

- The simulator's `--narration` flag (NarratingEmitter wraps the
  in-process Emitter, narrates live).
- Post-run replay against a saved NDJSON log.

The narrator covers ~30 event variants (run start, planet, civ
lifecycle, figures, tech, catastrophes, relations, refinements,
knowledge, contact, war/peace, alliance, trade, cohesion,
resilience, life-expectancy, surplus, cosmology, religion, species
events, template/tool discovery, run end). Per-tick markers,
snapshots, and per-cell territory updates return `None` to keep
signal-to-noise high.

## Test coverage

- `sim/report/src/digest/tests` — per-aggregator increments + end-to-
  end Markdown
- `sim/report/src/highlights.rs` tests — every match arm produces
  non-empty output for a sample event
- `sim/report/src/narration.rs` tests — per-event-variant narration +
  `narration_replay_consumes_log_file` round-trip
