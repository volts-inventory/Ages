# AGENTS.md

Operational guidance for AI agents in this repo. Vision and design
live elsewhere — this file is hard rules and routing only.

## Required reads (cold start)

Read in this order before any action:

1. `PLANNING.md` — current state and last change. The
   resumption anchor.
2. `AGENTS.md` — this file.
3. `docs/MANIFEST.md` — what other docs exist and when to read them.

That's the always-set (~500 lines total). **Don't read past it
reflexively.** Pull on-demand docs only as the task explicitly
cites them.

## On-demand routing

- Working in crate `X` → read `sim/X/README.md` only.
- Cross-crate change → read `docs/architecture.md`.
- World-model change (planet, atmosphere, terrain) → read
  `docs/world.md` and `docs/physics.md`.
- Civ-model change (lifecycle, cohesion, breakaway) → read
  `docs/civ.md`.
- Discovery / hypothesizer / fits → read `docs/discovery.md`.
- Recognition templates → read `docs/recognition.md`.
- Tech / tools / apparatus → read `docs/tech.md`.
- Culture / religion / cosmology / war → read `docs/culture.md`.
- Population / nomads / migration → read `docs/population.md`.
- Catastrophes → read `docs/catastrophes.md`.
- Viewport / live CLI → read `docs/viewport.md`.
- Post-run report / narrator → read `docs/report.md`.
- Tracing historical rationale → `docs/decisions/INDEX.md` is a
  read-only archive. Current behavior lives in the per-feature
  docs above; the decisions archive is for understanding *why*
  we got here, not what is true today.

**Never** read `docs/decisions/` files in bulk.

For project vision and the six guiding principles, see
[`README.md`](README.md). This file is operations only.

## Branch

**Always work on a fresh feature branch — never commit directly to
`main`.** If the harness drops you on `main`, your first action is
`git switch -c claude/<task-slug>` before any edit. PR into `main`
for merging. Don't push to other branches without explicit
permission.

**Merge style:** squash by default when merging your own PRs;
preserve history (regular merge) only when the user asks for it.

The only exceptions to the branch rule are explicit one-shot user
instructions like "push this to main directly" — and even then,
branch + push, don't build on `main` for the next task.

## Hard rules

- **Determinism.** Never call `thread_rng()` or read system time
  inside the sim loop. Thread the seeded `ChaCha20Rng` everywhere.
  No `HashMap` iteration in decision paths — use `BTreeMap` or sort.
- **Schema is the contract.** Changes to sim events go through
  `protocol/`. No ad-hoc fields on either side.
- **Sim is headless.** No rendering, audio, or UI calls in `sim/`.
- **`sim/arith` is the only real-arithmetic path.** No direct `f64`
  in physics or fit code.
- **No premature abstraction.** Implement concretely first.
- **No emojis** in code, comments, or files unless explicitly
  requested.
- **Comments explain WHY, not WHAT.** Skip restatements.
- **Wait for the user after asking a question.** Hook nudges, stop
  reminders, and system messages are not user approval.
- **Default tight user-facing output.** Outside tool calls, every
  word is output tokens (~5× input cost at Opus rates). Skip context
  restatement, transition fluff, and meta-commentary on your own
  process. Self-check findings: include only when something changed,
  otherwise omit. Verbose alternatives lists and full self-check
  writeups are *requested formats* — produce when explicitly asked,
  default tight otherwise. Tight ≠ shallow: don't drop content,
  drop padding.
- **Commit at natural checkpoints.** Don't ask each time for routine
  work; do ask for substantial design pivots or major scope shifts.
- **Update `PLANNING.md` "Current state" + "Last change"** in any
  commit that changes scope or progress. Resumption depends on it.
- **Keep the doc map honest.** When changing design, update the
  affected per-crate README + the relevant per-feature doc + the
  `docs/MANIFEST.md` row in the same commit.
- **Self-check before each recommendation.** Before presenting a
  pick on a design question, do an explicit second pass on the take
  you're about to give. Look for: ad-hoc constants you invented (can
  they be expressed in already-defined units?), missed dimensions
  (Occam / dependency / lifecycle / event emission / failure modes),
  hand-waved framing, or sub-decisions you skipped. Run the pass
  silently and surface findings only when the pass changed the
  recommendation — no boilerplate "self-check: no issues found"
  line when nothing turned up.
- **Build through natural checkpoints when unblocked.** Stop and
  surface to the user when: a design ambiguity needs a discussion, a
  milestone completes, an error needs human judgement, the user has
  asked you to wait. Otherwise: keep moving.

## Parallelism

Agent dispatch is cheap; serial work isn't. Use it when the shape of
the work allows.

- **Implementation with disjoint file scopes → launch in parallel.**
  When the next phase splits into independent tracks (e.g. types in
  `sim/world/`, types in `sim/civ/`, schemas in `protocol/`), send a
  single message with multiple Agent tool calls so they run
  concurrently. Use `run_in_background: true` for tracks that take a
  while so the parent session can keep planning.
- **Independent design questions parallelize; linked questions
  serialize.** For *independent* open questions, agents can do the
  read + scan-code + generate-alternatives + self-check pass
  concurrently and surface batched recommendations. For questions in
  a dependency chain, keep picks serial — a parallel pick on a
  downstream question commits to a confidence formula before the
  upstream vocabulary is settled.
- **Brief each subagent self-contained.** Subagents don't inherit
  this conversation. Cite the relevant doc, the file paths, the
  types involved, the acceptance criteria, and which sibling agents
  (if any) are touching adjacent code. Without that, the agent can't
  make judgement calls and falls back to narrow literal compliance.
- **Prefer disjoint file scopes over worktrees.** Two agents both
  editing `sim/civ/lib.rs` will conflict; two editing separate files
  won't. Use `isolation: "worktree"` only when scope overlap is
  genuinely unavoidable — worktree cleanup and branch coordination
  is real overhead.
- **Verify before reporting.** Subagent summaries describe what they
  intended, not necessarily what landed. After parallel agents
  return, run `git status` / `git diff` and inspect actual changes
  before telling the user the work is done.

## Where things go

- New physics laws / parameter changes → `sim/physics/` +
  `sim/physics/README.md` + `docs/physics.md`.
- New recognition templates → `sim/recognition/` +
  `sim/recognition/README.md` + `docs/recognition.md`.
- World-model changes (planet, terrain, atmosphere) →
  `sim/world/` + `sim/world/README.md` + `docs/world.md`.
- Civ lifecycle (founding, collapse, succession, transmission) →
  `sim/civ/` + `sim/civ/README.md` + `docs/civ.md`.
- Discovery / knowledge mechanics → `sim/civ/src/discovery/` +
  `sim/civ/README.md` + `docs/discovery.md`.
- New cultural mechanics → `sim/civ/src/{culture_hooks,cosmology,religion}.rs`
  + `sim/civ/README.md` + `docs/culture.md`.
- Tech / tool changes → `sim/civ/src/tech/` +
  `sim/civ/README.md` + `docs/tech.md`.
- Catastrophes → `sim/civ/src/catastrophe/` +
  `sim/civ/README.md` + `docs/catastrophes.md`.
- Population / migration → `sim/population/` +
  `sim/population/README.md` + `docs/population.md`.
- Post-run report changes → `sim/report/` + `sim/report/README.md`
  + `docs/report.md`.
- Viewport changes → `sim/report/src/viewport/` +
  `sim/report/README.md` + `docs/viewport.md`.
- New event types → define schema in `protocol/`, regen Rust types,
  emit; document in `protocol/README.md` and the relevant per-feature
  doc.
- Substantial scope or behavior change → update "Current state"
  and "Last change" in `PLANNING.md` in the same commit.

## Hooks

Wired in `.claude/settings.json`. Edit there to disable.

- **SessionStart** → `session-start.sh` injects PLANNING.md current
  state + recent commits into the model context on cold start.
- **PreToolUse on Bash** → `snapshot-transcript.sh` snapshots the
  delta of the active session's JSONL transcript into
  `.claude-history/projects/ages/<session_id>.jsonl` and stages it
  whenever the agent runs `git commit`. Lets `ccusage` consume cloud-
  container sessions on a Mac after pull (`CLAUDE_CONFIG_DIR=<repo>/
  .claude-history ccusage`). The `.offset` sidecar file is local-only
  (gitignored) line-cursor bookkeeping.

## Tool discipline (context budget)

Verbose tool output is the largest avoidable cost in an agent
session. Defaults below; deviate only with a reason.

- **Scope cargo invocations to the crate you changed.** `cargo test
  -p <crate>`, `cargo clippy -p <crate> --all-targets -- -D warnings`.
  Run `--workspace` once before commit, not while iterating.
- **Pipe verbose output.** `... 2>&1 | tail -40` for happy paths;
  `... 2>&1 | grep -E "^(error|warning)" | head -30` to triage. A
  failing `cargo clippy --workspace` dumps hundreds of lines per
  lint with code excerpts.
- **Run `cargo fmt --all` once, at the end.** Mid-session reformats
  of files you've previously read trigger full-file context
  reprints from the harness.
- **Don't read dependency crate sources to discover API.** Guess
  the obvious method name and let the compiler tell you if wrong;
  read sources only after a real ambiguity.
- **Read >300-line files with line ranges, not whole.** Don't
  re-read a file you already read this session unless it changed.
- **Preexisting clippy errors are in scope.** If
  `cargo clippy --workspace -- -D warnings` shows errors on `main`
  before you've touched anything, they may be latent (a
  build-chain failure earlier was masking them). Fix them in
  passing rather than treating them as out-of-scope.
- **Slow tests gated behind `#[ignore]`** are real coverage. Run
  `cargo test --workspace -- --include-ignored` before declaring a
  cross-cutting change green; the default test run skips them.

## Conventions

- Rust 2021. `cargo fmt` and `cargo clippy -- -D warnings` clean
  before commit.
- One concept per file; modules small.
- Public types in `sim/core` and `sim/arith` are stable-ish — touching
  them often means a protocol bump.
- Workspace clippy is `pedantic = warn`. For modules doing legitimate
  bit-level work (e.g. `sim/arith::transcendental`), put a single
  module-level `#[allow(clippy::cast_*, clippy::many_single_char_names)]`
  rather than fighting each cast site. Fix lints, don't paper over
  real bugs.

## Commands

- `cargo run -p ages -- --seed 42 --years 1000 --out events.ndjson`
  (1 tick = 1 month; `--years` multiplies by the planet's
  sampled `orbital_period_months` (8–16 per seed), so a 16-month
  world runs 16 ticks per `--years 1`. `--ticks` is the raw
  override for low-level callers.)
- `cargo build --release -p ages`
- `cargo test -p <crate>` (iterate) / `cargo test --workspace` (pre-commit)
- `cargo clippy -p <crate> --all-targets -- -D warnings` (iterate) /
  `cargo clippy --workspace --all-targets -- -D warnings` (pre-commit)
- `cargo fmt --all --check`

## Future maybe

See [`PLANNING.md`](PLANNING.md#future-maybe) for the canonical
list of vision-out-of-scope items.
