# Ages — Planning

## Current state

All seven planned milestones (M0 through M6) have shipped end-to-
end and merged into main. The species + multi-civ + post-run-
report pipeline is feature-complete against the AGENTS.md vision.
The project has moved out of milestone-build mode into tuning +
polish.

What runs end-to-end on `./run.sh`:

- Substrate-first planet sampling (Aqueous / Ammoniacal /
  Hydrocarbon / Silicate); every seed produces life of *some*
  chemistry.
- Hex-grid physics (heat, fluid, hydrology, magnetism, Lorentz,
  Coriolis, tides, radiation, vertical convection) with operator
  splitting; everything threads through Q32.32 fixed-point real
  arithmetic.
- 39 authored recognition templates plus emergent-template
  discovery from civ-confirmed thresholds; species canon adopts
  newly named phenomena across civ collapse boundaries.
- Per-civ hypothesizer with two parallel tracks (firing relations
  + continuous measurement relations), 12 form vocabulary,
  Occam-adjusted refinement on probation, rivals pool with
  displacement, theory hierarchy 3 levels deep.
- Predict-and-falsify: confirmed relations track a falsification
  streak; sustained mispredictions force refinement.
- Inheritance + revalidation: successors re-fit transmitted
  relations after a 50-tick window; failures emit
  `RelationLapsed` and drop.
- Mid-comprehension mythologization band: low-comprehension
  transmissions don't pass the relation but nudge the receiving
  civ's cosmology along an axis aligned with the relation's
  themes.
- Civ-built experiment apparatus: tier-2 capability tool for
  ToolExtension-bearing species; clamped-channel ladder
  experiments feed the hypothesizer's measurement track at 2×
  weighting and mark confirmed relations as experimental.
- 58-tool tech tree with strict prereqs + serendipitous unlocks;
  emergent tools propose from confirmed-relation clusters; both
  fold through the same effect aggregator across 8 effect
  categories.
- Per-cell heterogeneous populations with substrate-derived
  demographics, gradient-driven migration, nomadic species
  pool, habitat-priority diffusion, per-terrain habitability
  multipliers.
- Cohesion-driven civil war + breakaway path; multi-tick wars
  with marching front; belligerence-driven war declaration
  gated on contact + kinship-dampened drive score.
- Two-layer culture: slow-drift species-anchored cosmology
  (5 axes), fast-divergent civ-keyed religion (3 axes); kinship
  weighted dominantly by religion.
- Live ASCII viewport sharing the post-run report's frame
  renderer (default 36×30 grid, 74-column themed-box layout).
- Markdown post-run report (`ages-report`) with paired
  ownership/density spatial keyframes, war campaigns, per-civ
  scientific-lifecycle counters, highlight reel.
- Python prose narrator (`narrate.py`) consuming the same
  NDJSON via `RunMetadata` label tables.

A representative seed-42 / 5000-year run produces ~14 civs,
7000+ confirmed relations, 1800+ knowledge transmissions across
collapse boundaries, 4000+ conflict skirmishes grouped into war
campaigns, ~1000 lines of report.

## Last change

Documentation rewrite: de-Q-ified the whole repo, split
current-state behavior into per-feature docs in `docs/`, archived
`docs/decisions/` as historical-rationale only. Then split scope
across surfaces — added `docs/PROJECT.md` for vision-vs-state,
tightened README to entry-point shape, tightened this file to
resumption-anchor shape.

## How to use this file

This file's **Current state** + **Last change** sections (above)
are the resumption anchor for mid-task work. The file deliberately
does not restate the project's vision or goals — those live
elsewhere:

- [`README.md`](README.md) — public-facing entry: how to run it.
- [`docs/PROJECT.md`](docs/PROJECT.md) — vision principles
  tabulated against shipped features.
- [`docs/MANIFEST.md`](docs/MANIFEST.md) — full doc index.

When the change is substantial (new feature, scope shift), update
**Current state** and **Last change** above. When changing
design, also update the affected per-crate README + the relevant
per-feature doc + `docs/MANIFEST.md` in the same commit.

## Future maybe

Currently outside the project vision; could land later if the
direction shifts and a concrete consumer surfaces:

- **LLM in the loop or in post-processing** — hard rule. Can
  be added downstream by a user over the event log; not part
  of this project.
- **Native-GUI live UI** — terminal-only. (A live *ASCII*
  viewport ships as `--cli=viewport`.)
- **Networked multi-process sims** — single-process by design.
- **Save/load formats beyond snapshots** — periodic `Snapshot`
  digests cover the replay/checkpoint use case.
- **Mod hooks** — project framing.
- **Per-individual general population** — vision boundary.
- **Inter-planet civ contact** — single-planet by design.
- **Multi-species worlds** — single-species by design.
- **Quantum-scale physics** — vision boundary.
- **Turbulent fluid regimes** — computationally infeasible at
  planet scale.
- **Real meteorology fidelity** — vision boundary.
- **Audio** — project framing.
- **Web viewer** — could return if a downstream consumer asks.

## Historical

For the design-decision log (Q-record archive), see
[docs/decisions/INDEX.md](docs/decisions/INDEX.md). It is not
a source of truth for current behavior — the per-feature docs
in `docs/*.md` are.
