# Post-run report

Two consumers read the canonical NDJSON event log:

- **`ages-report`** — Rust binary that emits a structured markdown
  document.
- **`narrate.py`** — pure stdlib Python 3 narrator that prints a
  templated prose story.

Both are pure functions of the NDJSON; no NDJSON, no rendered
output. Determinism flows through end-to-end: same seed → same
NDJSON → same rendered output → same narration.

For deeper detail per file, see
[`sim/report/README.md`](../sim/report/README.md). The live
viewport ([viewport.md](viewport.md)) shares the report's frame
renderer so live frames and post-run keyframes look identical.

## `ages-report` (Rust)

```sh
cargo run --release --bin ages-report -- --in events.ndjson \
  --out report.md
```

Walks the event log once into a `Digest`
([`sim/report/src/digest/types.rs:7`](../sim/report/src/digest/types.rs#L7)),
then emits markdown via `render::markdown`
([`sim/report/src/render/mod.rs:74`](../sim/report/src/render/mod.rs#L74)).
Every section derives from events — no snapshots required.

## Digest

The digest is the rendering layer's working memory. Built by
`Digest::from_events`
([`sim/report/src/digest/build.rs:114`](../sim/report/src/digest/build.rs#L114))
in three passes:

1. **Pass 1 — relation index.** Walk the events building
   `relation_names: relation_id → (template_name, channel)` from
   `RelationConfirmed` events plus a `template_names` map from
   both `RelationConfirmed` and `Recognition`. Downstream
   refinement / transfer events carry only the `relation_id`, so
   having names indexed up front lets the renderer label them
   without a second join.
2. **Pass 2 — main fold.** `absorb`
   ([`build.rs:241`](../sim/report/src/digest/build.rs#L241))
   routes each event into a `CivChapter`
   ([`digest/types.rs:204`](../sim/report/src/digest/types.rs#L204))
   keyed by `civ_id`, plus the cross-civ collections (`contacts`,
   `conflicts`, `wars`, `trade_routes`, `diffusions`,
   `transmissions`).
3. **Pass 3 — ecosystem aggregates.** `aggregate_ecosystem`
   ([`build.rs:185`](../sim/report/src/digest/build.rs#L185))
   folds `SpeciesExtinct`, `SpeciationOccurred`,
   `HorizontalGeneTransfer`, `CatastropheFired`, and
   `CivResilienceTick` into the `EcosystemSummary`
   ([`digest/types.rs:75`](../sim/report/src/digest/types.rs#L75)).
   Kept separate from `absorb` so the running mean / set unions
   don't pollute the per-civ chapter path.

The raw events stay in `Digest::events` so the highlights pass
(see below) can re-walk without re-parsing the NDJSON.

## Sections

| Section | Source events |
|---------|---------------|
| Run header | `RunStart`, `RunEnd` |
| Planet card | `Planet`, `PlanetMap` (rendered by [`render_planet`](../sim/report/src/render/planet.rs)) |
| Ecosystem & dynamics | Aggregated from `SpeciesExtinct`, `SpeciationOccurred`, `HorizontalGeneTransfer`, `CatastropheFired`, `CivResilienceTick` |
| ASCII map + climate strip | `PlanetMap` (terrain), `Planet` (per-row temperature) |
| Species card | `Species` |
| Ages of the species | Derived from foundings, first tier-N unlocks, first transmissions, … (`ages::derive_ages`) |
| Run summary | All structural pins |
| Memorable figures + species canon | `FigureBorn`, `TemplateDiscovered`, `ToolDiscovered` |
| Per-civ chapters | All civ-scoped events for that civ id |
| Inter-civ knowledge transmission | `KnowledgeTransmitted` (across collapse) |
| Concurrent diffusion | `KnowledgeDiffused` |
| Trade routes | `TradeRouteEstablished` / `TradeRouteClosed` |
| Inter-civ contact + conflict | `CivContact`, `WarDeclared`, `ConflictResolved`, `PeaceConcluded` |
| Population timeline | `SpeciesNomadsChanged` + founding / collapse / catastrophe |
| Migration patterns | Per-civ `territory_history` deltas |
| World keyframes | `digest.keyframes(every_ticks)` → `render_world_frame` |
| Highlight reel | Hybrid score over structural pins (see below) |

## Planet card

Built by `render_planet`
([`render/planet.rs:10`](../sim/report/src/render/planet.rs#L10)).
A markdown table with the planet's static properties. After
adding the stellar-class and HZ rows, the header now reports:

| Row | Source |
|-----|--------|
| Composition / gravity / mean temp / temperature gradient | `PlanetDerived` |
| Terrain peak / sea level / atmosphere / pressure / biosphere / magnetosphere / crust | `PlanetDerived` |
| Stellar flux + spectral class | `PlanetDerived::stellar_luminosity_q32`, mapped via `stellar_class_label` ([`planet.rs:344`](../sim/report/src/render/planet.rs#L344)) |
| HZ flux band | Constants `HZ_FLUX_INNER_SOLAR = 1.10`, `HZ_FLUX_OUTER_SOLAR = 0.36` ([`planet.rs:334`](../sim/report/src/render/planet.rs#L334)), normalised against `SOLAR_CONSTANT_W_M2 = 1361` |
| Moons / axial tilt / day length / orbital period / substrate | `PlanetDerived` |
| Rotation state | Heuristic on day-hours / orbit-hours via `rotation_state_label` ([`planet.rs:365`](../sim/report/src/render/planet.rs#L365)): `synchronous (tidally locked)` when the ratio is within ±5% of 1; `slow rotator` when day > orbit; `free rotation` otherwise |

### Stellar class

`stellar_class_label` buckets the bolometric flux (W/m² incident
at the planet) against the solar constant:

| Flux ratio | Class |
|------------|-------|
| ≥ 5.0 | F |
| ≥ 1.5 | G-warm |
| ≥ 0.7 | G |
| ≥ 0.3 | K |
| < 0.3 | M |

The mapping is coarse — the rendering layer's job is "give the
reader a hint about the host star," not stellar physics.

### HZ band

The conservative habitable-zone flux limits land roughly at 1.10
solar (inner edge — greenhouse runaway) and 0.36 solar (outer
edge — CO₂ ice-out / snowball). Rendered alongside the planet's
own flux ratio so the reader sees whether the planet sits inside
the HZ, near an edge, or well outside.

### Rotation state

`rotation_state_label` works from the wire schema's existing
fields — `day_length_hours_q32` and `orbital_period_months` —
because the protocol doesn't yet carry a dedicated locking-state
field. A 30 days/month × 24 h/day conversion gives the orbit in
hours; the day-hours / orbit-hours ratio decides the label.

The world layer carries a proper `LockingState` enum + the
`sub_stellar_point(planet, macro_step)` geometry
([`sim/world/src/tidal_locking.rs:129`](../sim/world/src/tidal_locking.rs#L129)).
Synchronous planets get a fixed `(0, 0)` sub-stellar point by
convention; this rendering-layer heuristic catches the common
synchronous case until the protocol surfaces the enum directly.

## Ecosystem & dynamics

Rendered immediately after the planet table by
`render_ecosystem_summary`
([`render/planet.rs:388`](../sim/report/src/render/planet.rs#L388))
so long-running planet-wide dynamics get an at-a-glance summary
alongside the static properties. Skipped (with a "no events
emitted" line) when the run produced no ecosystem traffic.

| Aggregate | Source |
|-----------|--------|
| Species count (extant / extinct) | `EcosystemSummary::known_species_ids` − `extinct_species_ids` ([`digest/types.rs:124`](../sim/report/src/digest/types.rs#L124)) |
| Speciation events | `SpeciationOccurred` count |
| Horizontal gene transfer events | `HorizontalGeneTransfer` count |
| Mean ecological resilience | Arithmetic mean of `CivResilienceTick::resilience_q32` over the run |
| Catastrophes by kind | Histogram from `CatastropheFired::catastrophe_kind` |
| Magnetic reversal events | Reserved (`magnetic_reversal_events`); the sim has a `MagneticReversal` law ([`sim/physics/src/magnetism.rs:182`](../sim/physics/src/magnetism.rs#L182)) but doesn't yet emit per-reversal events — surfaces as `_(not emitted)_` until it does |
| Hadley cell count / jet velocity | Reserved (`hadley_cell_count`, `mean_hadley_jet_q32`); internal to `sim_physics::hadley` for now |
| Total tidal-heating budget | Reserved (`total_tidal_heating_tw_q32`); per-moon tidal heating is internal |
| Mean subsurface ocean temp | Reserved (`mean_subsurface_temp_k_q32`); meaningful only for icy substrates with subsurface oceans |

The reserved fields render as `_(not emitted)_` placeholders so
the table shape stays stable across runs and future protocol
growth has a known landing spot.

### Mean civ resilience

Resilience is `producer_biomass / initial_producer_biomass`
clamped to `[0, 2]`. A run-end mean of ≈ 1.0 means the
ecosystem held up against the civs' draw across the run; values
below 1 indicate net biosphere degradation, above 1 indicate
ecological release (e.g. an early collapse freed up the
biosphere). The aggregator picks up `CivResilienceTick` events
in pass 3 of `Digest::from_events`; tests cover the mean
calculation at
[`digest/tests.rs`](../sim/report/src/digest/tests.rs).

### Catastrophe-kind histogram

Comma-separated `kind×count` summary (e.g.
`volcanic×3, disease×2, asteroid×1`). Built by walking
`Event::CatastropheFired` in pass 3 — the same events that feed
the per-civ chapter's `catastrophes` vector, just grouped by
kind across the planet instead of by civ id.

## Spatial timeline (paired keyframes)

Every ~6 keyframes across the run (proportional, snapped to
clean 500-year boundaries; scales correctly for runs > 5000
years), the rendered output includes **both** an ownership map
and a density map of the same moment via
`digest.keyframes(every_ticks)`
([`build.rs:24`](../sim/report/src/digest/build.rs#L24)):

- **Ownership** — `A`/`B`/`C` capital letters mark each civ's
  centroid; cells claimed by exactly one civ render as that
  civ's digit (`1`–`9`, `*` for civs ≥ 10) in monochrome; in
  colored output the digit encodes per-cell pop fill instead.
  Disputed cells render as `#`; nomadic-only cells render as
  `0`; unclaimed cells show terrain.
- **Density** — same capital letters; claimed cells render as
  Unicode block-shading (` ░ ▒ ▓ █`) keyed to per-cell
  population relative to the densest cell in the frame. Capitals
  pop as `█`; frontiers fade to `░`.

The keyframe data lives in `CivChapter::territory_history`
([`digest/types.rs:238`](../sim/report/src/digest/types.rs#L238))
— each `TerritorySnapshot` carries `claimed_cells`,
`population_q32`, plus per-cell `cell_populations_q32` and
`cell_capacities_q32` so the density renderer reads each cell
as `pop / cap` saturation.

Together: who owns what, and where the people actually live.
The ownership renderer is `render_world_frame_styled`
([`sim/report/src/frame.rs:115`](../sim/report/src/frame.rs#L115)),
shared with the live viewport (see [viewport.md](viewport.md)).

## War campaigns

Two paths, depending on what's in the event log:

1. **Q-war path** (current logs) — uses the explicit
   `WarDeclared` / `PeaceConcluded` brackets in `Digest::wars`
   ([`digest/types.rs:177`](../sim/report/src/digest/types.rs#L177)).
   Each war row reports the year span, the outcome (defeated /
   tensions eased / borders settled / ongoing), and the
   founding belligerence + drive + kinship Q32 values from
   `WarDeclared`.
2. **Legacy path** — for pre-Q-war logs without the brackets,
   `group_conflicts_into_campaigns`
   ([`render/mod.rs:546`](../sim/report/src/render/mod.rs#L546))
   groups consecutive `ConflictResolved` events between the
   same pair into "campaigns" with start/end year, skirmish
   count, peak loss %, and whether the campaign ended in
   defeat.

A single declared war can produce 200+ per-cell skirmish events
(`ConflictResolved` fires per cell-flip); both paths collapse
the spam into one readable row per war.

## Highlights

Hybrid score over structural pins, computed by
`highlights`
([`sim/report/src/highlights.rs:250`](../sim/report/src/highlights.rs#L250)).
Top-N float to the "Highlights" section near the head of the
rendered output.

### Structural pins

Always included, never filtered. Events that always pin (with
their dedup rules):

| Event | Pin rule |
|-------|----------|
| `CivFounded` | One pin per civ id |
| `CivCollapsed` | Per event |
| `CatastropheFired` | Per event |
| `TechUnlocked` | One pin per `(civ_id, tier)` — only the *first* tool at each tier per civ |
| `CivContact` | Per event |
| `KnowledgeTransmitted` | Coalesced by `(source_civ, dest_civ, tick)` — a civ that inherits 30 relations at founding gets one summary line |
| `RelationConfirmed` | First-of-kind per `relation_id`, with the `is_trivial_constant` filter ([`highlights.rs:565`](../sim/report/src/highlights.rs#L565)) discarding all-zero-coefficient fits (absences, not discoveries) |
| `SpeciesExtinct` | Per event (with snake-case cause label) |
| `SpeciationOccurred` | Per event (with trigger label) |
| `HorizontalGeneTransfer` | Per event (with trait name label) |
| `CivResilienceTick` | Only when resilience is below `0.5` (degraded) or above `1.5` (thriving) — mid-band drift surfaces in the per-civ chapter and the ecosystem-summary mean, not the highlight reel |

### Scored long tail

Events that don't pin but score against a hybrid formula:

```
score = 0.4·novelty + 0.3·magnitude + 0.2·figure-significance + 0.1·arc-coherence
```

Per `RelationConfirmed`
([`highlights.rs:52`](../sim/report/src/highlights.rs#L52)):

- **novelty** — `1.0` if this is the earliest tick the
  `relation_id` was confirmed; `0.2` for later re-discoveries by
  successor civs.
- **magnitude** — fit confidence (Q32 → `[0, 1]`).
- **figure-significance** — per-figure discovery count capped at
  1.0 (`count / 10`).
- **arc-coherence** — U-shape over the civ's lifespan: 1.0 at
  the ends, ~0.3 in the middle.

Other scored events use fixed-prior scores:

- `RefinementConfirmed` → 0.85 (always interesting).
- `CosmologyShifted` → `0.3 + 0.4 × dogmatism`.
- `ConflictResolved` → `0.5 + 0.3 × loss_fraction`.
- `WarDeclared` → `0.7 + 0.2 × belligerence`.
- `PeaceConcluded` → `0.65 + {defeated: 0.15, dropped: 0.05,
  territory_resolved: 0.0}`.
- `TemplateDiscovered` / `ToolDiscovered` → 0.95.

Defaults: top 5% of all scored events
(`DEFAULT_TOP_FRACTION = 0.05`), capped at 25
(`MAX_SCORED_HIGHLIGHTS`) so even very long runs stay readable.

Pinned events are excluded from the scored pool so the same
event can't appear twice.

### Pin markers

The highlights section renders pins as `**•**` and scored
entries as `·` ([`render/mod.rs:602`](../sim/report/src/render/mod.rs#L602))
so a reader can scan structural beats vs. scored long-tail at a
glance.

## Per-civ chapters

Each chapter
([`render/civ.rs`](../sim/report/src/render/civ.rs)) surfaces
the civ's full lifecycle from the data folded into `CivChapter`:

- Founding (parent civ if any, founding-figure count, initial
  pop).
- Per-civ territory map (final claim from
  `territory_history.last()`).
- Discoveries (`RelationConfirmed` for figures owned by the
  civ).
- Refinements (proposed / confirmed / rejected counts).
- Inheritance: revalidated / lapsed / falsified counts so the
  reader sees how each civ *engaged* with its inheritance, not
  just what it knew at peak.
- Tech ladder (`TechUnlocked` in unlock order; tier + granted
  channels + newly-perceivable templates).
- Discovered templates + invented tools (emergent recognition /
  emergent tools — the civ's biographical credit even though
  the species adopts the canon).
- Catastrophes (`CatastropheRecord` per kind + fraction lost).
- Cosmology + religion shift history (axis-vector snapshots
  emitted past the drift threshold).
- Life-expectancy timeline (`LifeExpectancySnapshot` from
  `CivLifeExpectancyChanged`).
- Surplus timeline (`SurplusSnapshot` from
  `CivSurplusChanged`).
- Memorable figures.
- Collapse record (tick, reason, final pop, final figures).

## CivResilienceTick rendering

`CivResilienceTick` events surface in three places:

1. **Highlights reel** — pinned when resilience is outside the
   `[0.5, 1.5]` band (degraded / thriving), suppressed
   otherwise. The narrative beat is always "X civ's ecosystem
   crashed/boomed," never the mid-band 0.05 drift step. See
   `highlights.rs:431` and the threshold constants
   `RESILIENCE_HIGHLIGHT_LOW = 0.5` /
   `RESILIENCE_HIGHLIGHT_HIGH = 1.5`
   ([`highlights.rs:22`](../sim/report/src/highlights.rs#L22)).
2. **Ecosystem-summary mean** — the planet header carries the
   arithmetic mean across every emitted tick (1.0 baseline
   means net-neutral biosphere; below 1 means civs are
   degrading the producer base on average).
3. **Per-civ chapter (future)** — the resilience trace is
   currently aggregated only in the planet header; the per-civ
   chapter could grow a per-civ resilience timeline in a
   follow-up. The events are present in `Digest::events` so
   nothing needs to be re-emitted.

## `narrate.py` (Python)

```sh
./narrate.py runs/2026-...-{seed}.ndjson
```

Pure stdlib Python 3, no LLM. Prints a templated prose story:
title, world description, inhabitants, major arcs grouped by
year (foundings / tech unlocks / catastrophes / collapses), and
ending stats.

```
════════════════════════════════════════════════════════════
                    The Story of Mira-a
               seed 7  ·  99 simulated years
════════════════════════════════════════════════════════════

THE WORLD
─────────
Mira-a is an ammoniacal world. Atmospheric mix is methane-rich;
mean surface temperature -85°F. The planet has a strong
magnetosphere and 2 moons. Days last 46 hours; the year runs
8 months at an 8° axial tilt. The biosphere is lush.

THE INHABITANTS
───────────────
The Quilites are solitary carbon-based life — centralized
cognition at the low tier. They sense their world through
touch, and manipulate it with secreted chemicals. Lifespans
average 61 years; their communication is precise.

MAJOR ARCS
──────────
⌚ Year 0
   M 0 — Civilization 1 took root, founded by 2 figures
          across 10 cells.
   M11 — Civ 1 unlocked thermal sensor.

⌚ Year 99
   M11 — Civilization 1 collapsed — civil war.
```

## Shared label vocabulary

Same vocabulary in both surfaces — substrate freeze/boil ranges
and label tables (`ocean world` / `scorching` / `solitary` /
`precise` / `carbon`) ride through the NDJSON via the
`RunMetadata` event so a label change in Rust propagates to the
narrator without code edits on the Python side.

`RunMetadata` carries:

- Substrate freeze / boil ranges (per-substrate).
- Label tables for cognition / sociality / communication-
  fidelity / biochemistry.
- Climate-band thresholds.
- Magnetosphere / atmosphere / biosphere / world-type label
  tables.

Both `ages-report` and `narrate.py` read these tables at the top
of the NDJSON and use them to render labels for every subsequent
event.

## Determinism

Both consumers are pure functions of the NDJSON. Diff the report
or the narration across two runs of the same seed — byte-for-byte
identical (modulo wall-clock timestamps if the run wrote any).

## Output channels (recap)

The simulation has five output channels per the project vision:

1. **NDJSON event log** (canonical history).
2. **Periodic `Snapshot` events** (state-digest checkpoints
   every 500 ticks, embedded in the NDJSON).
3. **Live CLI stream** (events tee'd to stdout during the run).
4. **Live ASCII viewport** (`--cli=viewport`).
5. **Markdown post-run report** (`ages-report`) + Python prose
   narrator (`narrate.py`).

A representative seed-42 / 5000-year run produces ~14 civs,
7000+ confirmed relations, 1800+ knowledge transmissions across
collapse boundaries, 4000+ conflict skirmishes grouped into war
campaigns, ~1000 lines of report.
