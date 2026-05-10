# Documentation manifest

What exists, how big it is, and when to read it. Goal: small
session intake — read the **always** set, plus only the on-demand
docs your task explicitly cites.

## Always (session start)

| Path | Purpose |
|------|---------|
| `PLANNING.md` | Current state + last change. The resumption anchor for mid-task work. |
| `AGENTS.md` | Hard rules + routing. |
| `docs/MANIFEST.md` | The doc index you're reading now. |

## On-demand: project orientation

| Path | When to read |
|------|--------------|
| `README.md` | Public-facing entry: how to run it + brief framing. Read once for orientation. |
| `docs/PROJECT.md` | Vision principles tabulated against shipped features. Read when you need the "where does the project stand vs its ambition?" view, or when adding a feature whose vision-tracing isn't obvious. |
| `CLAUDE_HISTORY.md` | Chronological log of substantive PRs landed via Claude. Read when reconstructing why a given mechanic exists. |

## On-demand: cross-crate architecture

Read when working across multiple crates or on architectural
seams.

| Path | When to read |
|------|--------------|
| `docs/architecture.md` | Process layout, crate map, event/snapshot model, phase order, determinism contract, run-end taxonomy. |

## On-demand: per-feature docs

Read when changing the named feature. Each doc is a self-contained
description of *current behavior* — not history. For historical
rationale see `docs/decisions/INDEX.md` (archive only).

| Path | Topic |
|------|-------|
| `docs/world.md` | Planet sampling, terrain, atmosphere, climate, habitability. |
| `docs/physics.md` | Heat / fluid / hydrology / magnetism / Lorentz / Coriolis / tides / radiation / vertical convection. |
| `docs/recognition.md` | Templates, signatures, fields, perceivable vs latent, emergent templates. |
| `docs/discovery.md` | Form vocabulary, fits, hypothesizer lifecycle, theory hierarchy, rivals, mythologization, experiments. |
| `docs/species.md` | Species derivation, sensorium, per-civ drift, cosmology baseline. |
| `docs/civ.md` | Civ lifecycle: founding, cohesion, breakaway, collapse, succession, contact. |
| `docs/culture.md` | Cosmology + religion axes, kinship, belligerence, war, conflict events. |
| `docs/tech.md` | 58-tool tree, prereqs, effects, serendipitous unlocks, sensorium tools, experiment apparatus, emergent tools. |
| `docs/population.md` | Cohorts, substrate-derived demographics, nomads, habitat-priority diffusion, migration. |
| `docs/catastrophes.md` | Five catastrophe kinds, severity scaling, tech shielding. |
| `docs/viewport.md` | Live ASCII viewport layout, glyphs, civ panels, frame paint, dedup. |
| `docs/report.md` | Post-run markdown report sections; `narrate.py` prose narrator; shared label vocabulary. |

## On-demand: per-crate READMEs

Read only the crate(s) you're working in.

| Path | Topic |
|------|-------|
| `sim/arith/README.md` | Q32.32 fixed-point + transcendentals. |
| `sim/core/README.md` | Tick loop, RNG, phase walking, `build_laws`. |
| `sim/physics/README.md` | Laws, grid, time-stepping. |
| `sim/world/README.md` | Planet sampling (substrate-first), habitability, regions. |
| `sim/recognition/README.md` | Template-driven recognition. |
| `sim/species/README.md` | Species derivation, sensorium gating, cognition topology. |
| `sim/civ/README.md` | Civ lifecycle: founding, collapse, succession, tech tiers 1-5, culture wiring, catastrophes, hypothesizer, apparatus, religion. |
| `sim/population/README.md` | Cohorts, dynamics, carrying capacity. |
| `sim/events/README.md` | NDJSON emitter contract. |
| `sim/report/README.md` | Post-run markdown report + live ASCII viewport. |
| `protocol/README.md` | Schema-as-contract; versioning; event domains. |

## Decisions archive

| Path | When to read |
|------|--------------|
| `docs/decisions/INDEX.md` | Read-only historical archive. The Q-record of how each design choice landed. **Current behavior lives in the per-feature docs above** — read those first when in doubt. |
| `docs/decisions/q##.md` | Read a specific Q only when reconstructing the *rationale* for a past decision. Don't treat them as a source of truth for current code; they may have drifted. |

## Reading patterns

**Cold session start (any task):**
Always set (`PLANNING.md` + `AGENTS.md` + `MANIFEST.md`).

**Touching civ founding/lifecycle:**
Always set + `docs/civ.md` + `sim/civ/README.md`.

**Adding a recognition template:**
Always set + `docs/recognition.md` + `sim/recognition/README.md`.

**Cross-cutting refactor (e.g. event schema bump):**
Always set + `docs/architecture.md` + `protocol/README.md` + `sim/events/README.md`.

**Tuning a physics law:**
Always set + `docs/physics.md` + `sim/physics/README.md`.

**Tracing why something is the way it is (rare):**
Above + `docs/decisions/INDEX.md` → specific `q##.md`.

## Maintenance

- A new doc → add a row here in the same commit.
- A doc gets retired → remove the row.
- A per-feature doc and the relevant per-crate README must stay
  consistent. When changing behavior, update both in the same
  commit.
- `docs/decisions/` is now a **frozen archive** — don't add new
  Q files. Capture new design decisions inline in the relevant
  per-feature doc with a short "decided 2026-XX" annotation if
  needed.
