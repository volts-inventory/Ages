# Contributing to Ages

Thanks for your interest. This file is the contributor-facing
summary of what `AGENTS.md` covers in operational detail —
human-readable rather than agent-routing. If you're an AI agent,
start with `AGENTS.md` and `PLANNING.md` instead.

## Build

The project is a Cargo workspace. The toolchain is pinned in
`rust-toolchain.toml` (currently 1.94.0); `rustup` will install it
automatically the first time you build.

```sh
cargo build --workspace
```

For runtime experiments use release mode — the sim is
fixed-point-heavy and debug mode is several × slower:

```sh
cargo build --workspace --release
```

Two binaries land in `target/{debug,release}/`:

- `ages` — the simulation itself.
- `ages-report` — turns an NDJSON event log into a markdown report.

## Test

Default loop while iterating on a single crate:

```sh
cargo test -p <crate> --lib
```

Before committing:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --lib
```

The full integration / end-to-end matrix lives in `sim/core/tests/`
and is slower (~minutes); run it on substantial physics or
ecosystem changes:

```sh
cargo test --workspace
```

A handful of canary tests are pinned to specific seeds; if a
worldgen RNG shift invalidates a seed, the dev tool
`sim/core/tests/find_seed.rs` brute-forces a replacement:

```sh
cargo test -p sim-core --test find_seed -- --ignored --nocapture
```

## Project structure

```
ages/                ages + ages-report binaries
sim/arith            fixed-point types (Real, Pop, helpers)
sim/events           NDJSON event model + emitters
sim/physics          heat, fluids, hydrology, tides, tectonics, escape
sim/world            planet sampling, atmosphere, terrain, star, HZ
sim/recognition      phenomenon templates the hypothesizer fits against
sim/species          species derivation, tolerance, lifecycle, drift
sim/ecosystem        multi-species per-cell biogeochem + interactions
sim/population       cohorts, brackets, migration, nomads
sim/civ              civ lifecycle, conflict, culture, tech, discovery
sim/report           viewport, markdown report, prose narrator
sim/core             tick orchestrator + run loop
protocol             wire schema for NDJSON events
docs                 cross-crate and per-feature documentation
```

`docs/architecture.md` is the cross-crate process map (tick loop,
event flow, run-config plumbing). Read it before any change that
crosses crate boundaries. Per-feature docs are listed in
`docs/MANIFEST.md`.

Each crate under `sim/` has its own `README.md` with implementation
notes that aren't part of the public-facing docs.

## Determinism contract

The single hardest rule. The sim must produce byte-for-byte
identical NDJSON for the same `(seed, grid_width, grid_height)`
across runs, hosts, and CLI verbosity modes.

What this means in practice:

- **Never call `thread_rng()` or read system time inside the sim
  loop.** Every consumer of randomness threads the seeded
  `ChaCha20Rng` from `RunConfig`. If you need a fresh sub-stream,
  derive it deterministically from the parent (e.g. seeded by tick
  + entity id).
- **No `HashMap` iteration in decision paths.** `HashMap`'s
  iteration order is randomised per-process. Use `BTreeMap` or
  collect-then-sort. Inserting into a `HashMap` and reading it
  back later, where the result feeds an event, a step decision,
  or anything that affects future state, is a determinism bug.
  This applies transitively — `HashSet` and any `*::iter()` over
  a hashed structure carry the same hazard.
- **`sim/arith` is the only real-arithmetic path.** No raw `f64`
  in physics, fits, or civ logic. The default real type is
  Q32.32 (`Real`), backed by `fixed::types::I32F32`; the wider
  `Pop` is Q64.32 for aggregate populations that overflow Q32's
  ±2.1e9 range. Use the helpers in `sim_arith` — `saturating_add`,
  `from_f64_clamped`, etc. — rather than reaching for raw fixed
  arithmetic.
- **Conservation invariants are runtime-asserted.** Physics steps
  carry a cumulative-drift accumulator (Sprint 1 Item 5). If
  your change broadens a conservation tolerance, justify it in
  the commit message — silent drift is how reproducibility decays.
- **Adaptive dt is bounded by the acoustic CFL.** Don't bypass
  the sub-stepper to "smooth out" a transient; that's how you
  desynchronise.

A determinism regression is the most expensive kind of bug in
this codebase — once it lands, every downstream test that pinned
a seed has to be re-pinned. Sanity check by running the same
seed under two different `--cli` modes and diffing the NDJSON.

## Public API stability

The big code-organisation waves (CA / CB / CC) split oversized
modules into folders. The hard rule for those (and any future
analogue): **refactors that split a file must preserve every
existing re-export path.** Downstream code, doctests, and the
PLANNING.md "Where things go" entries all reference these paths,
and changing them creates merge friction for unrelated branches.

In practice: when moving `foo.rs` → `foo/{a.rs, b.rs, c.rs}`,
keep a `foo/mod.rs` that re-exports the same types and functions
the old `foo.rs` exposed. Inner modules can be `pub(crate)` if
nothing crosses the crate boundary; if downstream crates touched
the original symbols, leave the re-exports `pub`.

If you genuinely need to break an API surface, do it in a
dedicated commit that names every downstream call site you
updated. Don't smuggle API breaks into a refactor commit.

## Commit message conventions

Format used throughout the history:

```
<area>: <short imperative summary> (<item-or-task tag>) (#PR)
```

Common `<area>` values, drawn from the actual history:

- `physics` — anything in `sim/physics/`.
- `civ` — anything in `sim/civ/`.
- `ecosystem` — anything in `sim/ecosystem/`.
- `species` — anything in `sim/species/`.
- `world` — anything in `sim/world/`.
- `population` — anything in `sim/population/`.
- `recognition` — recognition templates.
- `core` — orchestration / tick / `sim/core/`.
- `report` — markdown report or viewport.
- `protocol` — schema-level changes.
- `ages+narration`, `civ+ecosystem`, `physics+world`,
  `species+civ` — compound when a change genuinely crosses crates.
- `docs` — anything under `docs/` or top-level docs.
- `chore` — workspace-level housekeeping, gitignore, deps.

Tags in parentheses: track item identifiers (`P1.1`, `T13`,
`F4`, `CA3`, `CB7`, `Sprint 5 Item 17`) so commits can be
correlated with the planning doc. Optional but encouraged.

PR number is appended by the merge tooling — don't hand-write it.

Body convention: 1–3 short paragraphs. The first paragraph
states *why*; the second covers any non-obvious mechanism; the
third lists testing done. Skip the body entirely for trivial
changes.

## PR workflow

1. Branch off `main` — `claude/<slug>` for AI-driven work,
   `<contributor>/<slug>` for human-driven. Never commit directly
   to `main`.
2. Implement the change. If it touches scope or design, update
   the relevant per-feature doc in the same commit. If it
   changes "Current state" or "Last change" semantics, update
   `PLANNING.md` in the same commit.
3. Run the local checks (`cargo fmt --all --check`, `cargo
   clippy --workspace --all-targets -- -D warnings`, `cargo test
   --workspace --lib`).
4. Push; open a PR with a 1–2 sentence summary and a "Test plan"
   bullet list.
5. Squash-merge by default. Regular merge only when the user
   asks for it.
6. Don't push to other branches without explicit permission.

For substantial design changes, open an issue first and let the
discussion settle before implementing. The expert-review waves
that drove most of v1.0 went through this pattern explicitly.

## Where to find things

When you need to tune a constant, the canonical reference is
`docs/internal/magic-constants.md` — the magic-constants ledger
introduced in F4. It lists every tunable in the sim with its
origin (literature citation, calibration target, or "TBD"), its
cross-planet calibration status, and where it lives in the code.
Add a row when you introduce a new constant; update the status
column when a calibration test starts gating it.

Other useful reference docs:

- `docs/decisions/INDEX.md` — read-only archive of design
  decisions. Useful for tracing *why* something is the way it is.
  Don't read it in bulk; jump to the specific `qNNN.md` that
  `INDEX.md` points to.
- `docs/architecture.md` — process layout and tick loop.
- `docs/physics.md` — physics laws and time-stepping.
- `docs/world.md` — worldgen, planet sampling, atmosphere.
- `docs/civ.md` — civ lifecycle.
- `docs/discovery.md` — hypothesizer mechanics.
- `docs/report.md` — post-run report and prose narrator.
- `docs/viewport.md` — live ASCII viewport.
- `AGENTS.md` — operational rules (the canonical source; this
  file is the human summary).
- `PLANNING.md` — current state and active work.

## Anti-patterns

A non-exhaustive list of things that have bitten us:

- **`HashMap` slipped into a step decision path.** See
  determinism contract. Always `BTreeMap` or sort.
- **Bare `f64` constants in physics.** Wrap in
  `Real::from_num(...)` at definition; the `arith` helpers will
  catch overflow you'd otherwise lose silently.
- **Splitting a module without preserving re-exports.** Breaks
  downstream call sites for no semantic gain. See public API
  stability above.
- **Skipping the doc update in the same commit.** A behavior
  change without a doc update splits truth between code and
  prose; the next reader doesn't know which to trust.
- **Reading whole long files (>300 lines) into context when a
  range would do.** Especially relevant for agents; see
  `AGENTS.md` tool-discipline section.
- **Amending a commit after a failed pre-commit hook.** The
  commit didn't happen, so `--amend` modifies the *previous*
  commit. Fix the issue, re-stage, create a new commit.

## License

By contributing, you agree your contribution is dual-licensed
under MIT OR Apache-2.0 (matching the project license).
