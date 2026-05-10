# Post-run report

Two consumers read the canonical NDJSON event log:

- **`ages-report`** — Rust binary that emits a structured markdown
  report.
- **`narrate.py`** — pure stdlib Python 3 narrator that prints a
  templated prose story.

Both are pure consumers; no NDJSON, no report. Determinism flows
through: same seed → same NDJSON → same report → same narration.

For deeper detail per crate, see
[`sim/report/README.md`](../sim/report/README.md). For the live
viewport that shares the report's frame renderer see
[viewport.md](viewport.md).

## `ages-report` (Rust)

```sh
cargo run --release --bin ages-report -- --in events.ndjson \
  --out report.md
```

Walks the event log a single time, building a digest in memory,
then emits markdown. No snapshots needed; every section derives
from events.

### Sections

| Section | Source events |
|---------|---------------|
| Planet card | `RunHeader`, `Planet`, `PlanetMap` |
| Species card | `SpeciesDerived`, `SpeciesCosmologyBias` |
| Run summary | All structural pins; run-end reason from `RunHeader` |
| Spatial timeline (paired keyframes) | `Snapshot` deltas + cell-claim history reconstruction |
| Population timeline | `SpeciesNomadsChanged`, founding / collapse / catastrophe events |
| Per-civ chapters | All civ-scoped events for that civ id |
| Discovered templates | `TemplateDiscovered` |
| Invented tools | `TechUnlocked`, `ToolDiscovered` |
| Confirmed measurements | `MeasurementConfirmed` (with `is_experimental` markers) |
| Refinements | `RefinementProposed/Confirmed/Rejected` |
| Rivals | `RivalHypothesisProposed`, `PrimaryHypothesisDisplaced` |
| Inheritance | `RelationRevalidated`, `RelationLapsed` |
| Mythologization | `RelationMythologized` |
| Inter-civ knowledge transmission | `KnowledgeTransmitted` |
| Concurrent-civ diffusion | `KnowledgeDiffused` |
| Civ contact | `CivContact` |
| War campaigns | `WarDeclared`, `ConflictResolved` (aggregated), `PeaceConcluded` |
| Catastrophes | `CatastropheFired` |
| Highlight reel | hybrid score over structural pins |

### Spatial timeline (paired keyframes)

Every ~6 keyframes across the run, the report renders **both** an
ownership map and a density map of the same moment:

- **Ownership** — `A`/`B`/`C` capital letters mark each civ's
  centroid; cells claimed by exactly one civ render as that civ's
  digit (`1`–`9`, `*` for civs ≥10); disputed cells render as
  `#`; nomadic-only cells render as `0`; unclaimed cells show
  terrain.
- **Density** — same capital letters; claimed cells render as
  Unicode block-shading (` ░ ▒ ▓ █`) keyed to per-cell
  population relative to the densest cell in the frame. Capitals
  pop as `█`; frontiers fade to `░`.

Together: who owns what, and where the people actually live. The
ownership renderer is shared with the live viewport (see
[viewport.md](viewport.md)).

### War campaigns

The report aggregates consecutive `ConflictResolved` events
between the same pair into "war campaigns" with start/end years,
peak loss percentage, and final outcome. A single declared war
can produce 200+ per-cell skirmishes; the campaign view collapses
that into one readable narrative.

### Highlight reel

Hybrid score over structural pins — founding, collapse, first
confirmed relation per template, first tier-N tool, first contact,
first transmission, first conflict, transcendence threshold
crossings, catastrophe severity outliers. Top-N float to the
"Memorable moments" section near the head of the report.

### Per-civ scientific-lifecycle counters

Each per-civ chapter surfaces:

- Confirmed relations (count + key examples).
- Refinements proposed / confirmed / rejected.
- Inherited relations revalidated / lapsed.
- Own relations falsified.
- Rivals proposed / displaced.

So the reader sees how each civ *engaged* with its inheritance,
not just what it knew at peak.

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
