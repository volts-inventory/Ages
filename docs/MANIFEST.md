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

## Customer-facing (read first if you're new)

| Path | When to read |
|------|--------------|
| `README.md` | Public-facing entry: what the project is + how to run it. Read once for orientation. |
| `docs/getting-started.md` | Ten-minute walkthrough from a fresh clone to a finished run with a written history. Where to go if "I just want to see it work" is the first ask. Includes `--config` interactive planet builder. |
| `CHANGELOG.md` | Reverse-chronological user-visible changes. Versions are retroactive; first release is `1.0.0`. |
| `CONTRIBUTING.md` | Human-readable contributor summary: build, test, lint, doc rules, commit conventions. (`AGENTS.md` is the operational equivalent for agents.) |

## On-demand: project orientation

| Path | When to read |
|------|--------------|
| `docs/PROJECT.md` | Vision principles tabulated against shipped features. Read when you need the "where does the project stand vs its ambition?" view, or when adding a feature whose vision-tracing isn't obvious. |

## On-demand: cross-crate architecture

Read when working across multiple crates or on architectural
seams.

| Path | When to read |
|------|--------------|
| `docs/architecture.md` | Workspace crate map, dependency hierarchy, post-cleanup module layout, operator-splitting orchestration, Q32.32 determinism contract, event-emitter model (`Emitter` trait, `NarratingEmitter`, `replay_narration`), phase order, run-end taxonomy. |

## On-demand: per-feature docs

Read when changing the named feature. Each doc is a self-contained
description of *current behavior* — not history. For historical
rationale see `docs/decisions/INDEX.md` (archive only).

| Path | Topic |
|------|-------|
| `docs/world.md` | Planet sampling, terrain, atmosphere, climate, habitability, `--config` user overrides (`PlanetOverrides` + `sample_planet_with_overrides`). |
| `docs/physics.md` | Heat / fluid / hydrology / magnetism / Lorentz / Coriolis / tides / radiation / vertical convection / tectonics / tidal heating / atmospheric escape. |
| `docs/recognition.md` | Templates, signatures, fields, perceivable vs latent, emergent templates. |
| `docs/discovery.md` | Form vocabulary, fits, hypothesizer lifecycle, theory hierarchy, rivals, mythologization, experiments. |
| `docs/species.md` | Species derivation, sensorium, per-civ drift, cosmology baseline. |
| `docs/civ.md` | Civ lifecycle: founding, cohesion, breakaway, collapse, succession, contact. |
| `docs/culture.md` | Cosmology + religion axes, kinship, belligerence, war, conflict events. |
| `docs/tech.md` | 58-tool tree, prereqs, effects, serendipitous unlocks, sensorium tools, experiment apparatus, emergent tools. |
| `docs/population.md` | Cohorts, substrate-derived demographics, nomads, habitat-priority diffusion, migration. |
| `docs/catastrophes.md` | Five catastrophe kinds, severity scaling, tech shielding. |
| `docs/viewport.md` | Live ASCII viewport layout, glyphs (Earthlike + Lava + IceCap surface phases, magma `*` + ice `+`), surface-aware planet archetype labels, civ panels, frame paint, dedup. |
| `docs/report.md` | Post-run markdown report sections; `narrate.py` prose narrator; in-binary `--narration` flag; shared label vocabulary. |

## On-demand: per-crate READMEs

Read only the crate(s) you're working in.

| Path | Topic |
|------|-------|
| `sim/arith/README.md` | Q32.32 fixed-point + transcendentals; `Real`, `Pop`, `Real::percent`. |
| `sim/core/README.md` | Tick loop, RNG, phase walking, `build_laws`, `tick_steps/`, nomads. |
| `sim/physics/README.md` | Laws, grid, operator-splitting orchestration, tectonics, tidal heating, atmospheric escape, radiation. |
| `sim/world/README.md` | Planet sampling (substrate-first), habitability, regions. |
| `sim/recognition/README.md` | Template-driven recognition. |
| `sim/species/README.md` | Species derivation, sensorium gating, cognition topology. |
| `sim/ecosystem/README.md` | Functional-group ecosystem (planet/step + biogeochem + extinction + centrality), HGT, speciation. |
| `sim/civ/README.md` | Civ lifecycle: founding, collapse, succession, tech tiers 1-5, culture wiring, catastrophes, hypothesizer, apparatus, religion, conflict folder. |
| `sim/population/README.md` | Cohorts, dynamics, lifecycle, carrying capacity. |
| `sim/events/README.md` | NDJSON `Emitter` trait + adapters (`JsonLinesEmitter`, `TeeEmitter`, `FilterEmitter`, `ThrottledEmitter`). |
| `sim/report/README.md` | Post-run markdown report; live ASCII viewport (`viewport/`); narration (`NarratingEmitter`, `replay_narration`); render/digest split. |
| `protocol/README.md` | Schema-as-contract; versioning; event domains. |

## Internal reference

Not user-facing. Read when you specifically need calibration
provenance or per-machine housekeeping.

| Path | When to read |
|------|--------------|
| `docs/internal/README.md` | One-line index to the two files below. |
| `docs/internal/magic-constants.md` | The fitted / heuristic / dimensional-constant ledger. Origin + cross-planet extrapolation status for every numerical coefficient. Look here before tuning any constant. |
| `docs/internal/maintenance.md` | Per-machine housekeeping for the agent-worktree dev environment (e.g. pruning stale worktrees). Not required for normal development. |

## Decisions archive

| Path | When to read |
|------|--------------|
| `docs/decisions/INDEX.md` | Read-only historical archive. The Q-record of how each design choice landed. **Current behavior lives in the per-feature docs above** — read those first when in doubt. |
| `docs/decisions/q##.md` | Read a specific Q only when reconstructing the *rationale* for a past decision. Don't treat them as a source of truth for current code; they may have drifted. |

## Reading patterns

**Cold session start (any task):**
Always set (`PLANNING.md` + `AGENTS.md` + `MANIFEST.md`).

**Brand-new contributor (human):**
`README.md` + `docs/getting-started.md` + `CONTRIBUTING.md`.

**Touching civ founding/lifecycle:**
Always set + `docs/civ.md` + `sim/civ/README.md`.

**Adding a recognition template:**
Always set + `docs/recognition.md` + `sim/recognition/README.md`.

**Cross-cutting refactor (e.g. event schema bump):**
Always set + `docs/architecture.md` + `protocol/README.md` + `sim/events/README.md`.

**Tuning a physics law or a magic constant:**
Always set + `docs/physics.md` + `sim/physics/README.md` +
`docs/internal/magic-constants.md`.

**Tracing why something is the way it is (rare):**
Above + `docs/decisions/INDEX.md` → specific `q##.md`.

## Maintenance

- A new doc → add a row here in the same commit.
- A doc gets retired → remove the row.
- A per-feature doc and the relevant per-crate README must stay
  consistent. When changing behavior, update both in the same
  commit.
- `docs/decisions/` is a **frozen archive** — don't add new
  Q files. Capture new design decisions inline in the relevant
  per-feature doc with a short "decided 2026-XX" annotation if
  needed.
- User-visible changes go in `CHANGELOG.md` under `## Unreleased`.
