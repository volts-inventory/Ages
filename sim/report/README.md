# sim-report

Post-run markdown report generator (M6). Reads an NDJSON event log
produced by the `ages` binary and renders a structured markdown
report covering the species' full history.

## Pipeline

```
events.ndjson
    │
    ▼
parse::events_from_reader   →  Vec<protocol::Event>
    │
    ▼
digest::Digest::from_events →  per-civ chapters + cross-civ tables
    │
    ▼
render::markdown            →  String (the report)
```

`render_from_reader(reader) -> String` ties all three together for
the common case.

## Sections

1. **Run header** — seed, schema version, sim version, run length.
2. **Planet card** — composition, gravity, climate, atmosphere,
   biosphere, magnetosphere, stellar luminosity (from the
   `PlanetDerived` event).
3. **Species card** — traits, modalities, manipulation modes,
   native perceivable templates (from `SpeciesDerived`).
4. **Run summary** — counts: civs founded / collapsed, named figures,
   confirmed relations, refinements, catastrophes, tech unlocks,
   cosmology shifts, transmissions, diffusions, contacts, conflicts.
5. **Per-civ chapters** — founding event, named figures, tech ladder,
   key discoveries with fitted equations, refinements, catastrophes,
   cosmology drift, collapse cause.
6. **Inter-civ knowledge transmission** — table of relations
   comprehended across collapse boundaries.
7. **Inter-civ contact / conflict** — first-contact pairs and
   resolved conflicts.
8. **Concurrent-civ knowledge diffusion** — relation transfers
   between peaceful concurrent civs.
9. **Species population timeline** — sparse arc of population-
   affecting events (founding / collapse / catastrophe) with
   running active-civ count and recorded population. The sim
   doesn't emit per-tick population, so the column underestimates
   true peaks; it's a narrative arc rather than a tick-accurate
   plot.
10. **Highlight reel** — structural pins (founding, collapse,
    catastrophe, tech-tier crossings, first-of-kind discoveries,
    inter-civ transmissions) plus the top-scored long-tail events
    (refinements, cosmology shifts, conflicts, re-discoveries).

The biography also surfaces three species-level views above the
per-civ chapters:

- **Ages of the species** — emergent eras (Foundational / Empirical /
  Refinement / Tool / Concurrent / Successor) derived from event
  milestones, not authored thresholds. The project's namesake.
- **Memorable figures** — cross-civ rankings (most prolific scientist,
  deepest refiner, founders).
- **Species canon** — consolidated knowledge state across every civ
  at run-end, partitioned into non-trivial findings and a collapsed
  list of trivial `y \u{2248} 0` absences.

Years vs. ticks: `1 sim tick = 1 species-month` (one
`integrate_civ_step` call per tick); user-facing report measures
time in years (= tick / 12). Internal protocol still says "tick".

## Highlight scoring

Hybrid pin + scored-tail design:

- **Pins** are emitted unconditionally for the events listed above.
- **Scored long tail**: each candidate event scores
  `0.4·novelty + 0.3·magnitude + 0.2·figure-significance + 0.1·arc-coherence`.
  Top 5% by score (capped at 25 entries to keep medium-length runs
  legible) are surfaced. Weights are M6 starting points; tune as the
  feel of the rendered report develops.

## Binaries

`ages-report` (in the `ages` crate) is the user-facing CLI:

```
ages-report --in events.ndjson --out report.md
ages-report < events.ndjson > report.md
```

## Determinism

Every numeric value comes from the sim's `Q32.32` raw bits in the
event log; the report only ever lossy-converts to `f64` for display.
No values are fed back into the sim, so the precision-loss is safe
and explicit (see the module-level `clippy::cast_*` allow in
`lib.rs`).

The renderer is purely a pure function of the event log: same log
in → byte-identical markdown out.
