# Project — vision and current state

How the project's ambitions map to what's shipped today. For
mechanics depth on any single feature, see the per-feature docs
in [`docs/`](.). For how to run it, see [`README.md`](../README.md).
For active work tracking, see [`PLANNING.md`](../PLANNING.md).

## Vision in one paragraph

Ages produces **the biography of a species across its full
history**. A planet is sampled from a seed; a species evolves to
fit it; civilizations rise and fall within the species over
thousands of years, each deriving genuinely different physics
from ours because their world is different. Knowledge survives
collapses through inherited artifacts with comprehension-decayed
transmission. Discoveries are fits against real simulated data,
not authored truth tokens. Outputs are structured (NDJSON event
log + markdown report); no LLM in the loop.

## Six guiding principles

These are what the project is *for*. Every shipped feature should
trace back to at least one.

1. **Physical / biological grounding over game-mechanic
   abstraction.** Civs derive math from real simulated physics;
   species evolve to fit niches; no fudge constants where
   derivation is possible.
2. **Emergence over authoring.** Paradigms emerge from physics +
   observation order. Ages emerge from knowledge state. Phenomena
   emerge from law combinations on planet-specific conditions.
   Civs emerge from population + triggers.
3. **The species is the protagonist; civs are episodes within
   its history.** Sumer → Akkad → Babylon-style arcs are
   first-class.
4. **Quantitative depth, not tokens.** Discoveries are fitted
   functional forms with real parameters. Wrong hypotheses are
   first-class. Refinement is open-ended.
5. **No hand-holding output, no LLM.** Outputs are structured:
   NDJSON + snapshots + live CLI + markdown report.
6. **Determinism as a contract.** Same `(seed, grid)` pair =
   byte-for-byte identical NDJSON. Physics, fits, RNG all thread
   through Q32.32 fixed-point arithmetic.

## Vision pillar → shipped features

### 1. Physical / biological grounding

| Vision claim | Status | Where it lives |
|--------------|--------|----------------|
| Planet sampled from seed; physics state evolves deterministically | **Shipped** | [physics.md](physics.md), [world.md](world.md) |
| Substrate-first sampling — every seed produces life of *some* chemistry | **Shipped** (Aqueous / Ammoniacal / Hydrocarbon / Silicate) | [world.md](world.md) |
| Per-substrate constants (chemistry, latent heats, gas constants, cell thermal mass) | **Shipped** | [physics.md](physics.md) |
| Atmospheric composition + crustal composition as continuous vectors, not categorical labels | **Shipped** (9-channel + 7-channel) | [world.md](world.md) |
| Substrate-derived demographics (founding floor, capacity, migration, birth rate) | **Shipped** | [population.md](population.md) |
| Per-terrain habitability multipliers gating claim eligibility | **Shipped** | [world.md](world.md), [population.md](population.md) |

### 2. Emergence over authoring

| Vision claim | Status | Where it lives |
|--------------|--------|----------------|
| Phenomena emerge from physics, not authored catalogues | **Shipped** (template signature matching against physics state) | [recognition.md](recognition.md) |
| Recognition templates carry structural tags that gate which fitted forms a civ can propose | **Shipped** (form vocabulary follows perceivable templates) | [recognition.md](recognition.md), [discovery.md](discovery.md) |
| Civs emerge from nomadic density + tech-readiness, not scripted | **Shipped** | [civ.md](civ.md), [population.md](population.md) |
| Emergent recognition templates: confirmed thresholds become new named phenomena | **Shipped** | [recognition.md](recognition.md) |
| Emergent tools: confirmed-relation clusters become new tools in the species canon | **Shipped** | [tech.md](tech.md) |
| Two-layer culture: slow-drift cosmology + fast-divergent religion | **Shipped** (5-axis + 3-axis) | [culture.md](culture.md) |

### 3. Species as protagonist

| Vision claim | Status | Where it lives |
|--------------|--------|----------------|
| Species persists across civ rise/fall | **Shipped** | [species.md](species.md), [civ.md](civ.md) |
| Concurrent + sequential civs within one species | **Shipped** | [civ.md](civ.md) |
| Per-civ species drift (cognition / sociality / lifespan / communication deltas, accumulating across successors) | **Shipped** | [species.md](species.md), [civ.md](civ.md) |
| Knowledge inheritance across collapse boundaries | **Shipped** (with comprehension-decayed transmission + revalidation window) | [discovery.md](discovery.md) |
| Mid-comprehension mythologization band — "lost knowledge leaves a cultural shadow" | **Shipped** | [discovery.md](discovery.md), [culture.md](culture.md) |
| Cohesion-driven civil war + breakaway successors | **Shipped** | [civ.md](civ.md) |
| Belligerence-driven war (kinship-dampened, religion-weighted, contact-gated) | **Shipped** | [culture.md](culture.md) |

### 4. Quantitative depth

| Vision claim | Status | Where it lives |
|--------------|--------|----------------|
| 12-form vocabulary fitted against samples | **Shipped** | [discovery.md](discovery.md) |
| Both firing relations (thresholds) and continuous measurement relations (SI coefficients) | **Shipped** | [discovery.md](discovery.md) |
| Refinement on probation; rivals pool with displacement | **Shipped** | [discovery.md](discovery.md) |
| Theory hierarchy via residual children, 3 levels deep | **Shipped** | [discovery.md](discovery.md) |
| Predict-and-falsify: confirmed laws track misprediction streaks | **Shipped** | [discovery.md](discovery.md) |
| Inheritance revalidation: successors re-fit transmitted laws after a 50-tick window | **Shipped** | [discovery.md](discovery.md) |
| Civ-built experiment apparatus: clamped-channel intervention alongside passive observation | **Shipped** | [tech.md](tech.md), [discovery.md](discovery.md) |
| Cosmology biases hypothesizer confirmation rates | **Shipped** | [culture.md](culture.md), [discovery.md](discovery.md) |

### 5. Structured output, no LLM

| Vision claim | Status | Where it lives |
|--------------|--------|----------------|
| NDJSON canonical event log | **Shipped** | [architecture.md](architecture.md), [`protocol/README.md`](../protocol/README.md) |
| Periodic `Snapshot` digests embedded in NDJSON | **Shipped** | [architecture.md](architecture.md) |
| Live CLI event stream with verbosity levels | **Shipped** | [architecture.md](architecture.md) |
| Live ASCII viewport sharing the post-run frame renderer | **Shipped** | [viewport.md](viewport.md) |
| Markdown post-run report (planet card, species card, per-civ chapters, paired keyframes) | **Shipped** | [report.md](report.md) |
| Python prose narrator consuming the same NDJSON | **Shipped** | [report.md](report.md) |
| Shared label vocabulary via `RunMetadata` event | **Shipped** | [report.md](report.md) |

### 6. Determinism

| Vision claim | Status | Where it lives |
|--------------|--------|----------------|
| Single seeded `ChaCha20Rng` threaded through the sim | **Shipped** | [architecture.md](architecture.md) |
| Q32.32 fixed-point arithmetic; no `f64` outside `sim/arith` | **Shipped** | [physics.md](physics.md) |
| `BTreeMap` / sorted iteration in decision paths | **Shipped** | [architecture.md](architecture.md) |
| Same `(seed, grid)` pair = byte-for-byte identical NDJSON regardless of `--cli` mode or tick rate | **Shipped** | [architecture.md](architecture.md) |
| Determinism + performance + divergence tests in CI | **Shipped** | [architecture.md](architecture.md) |

## Known gaps / aspirations not fully realized

These trace to vision goals but haven't been served fully or have
known dilution:

- **Reduced-χ² fit metric.** The current confirm threshold is RMSE
  + exponential confidence at `exp(-1) ≈ 0.368`. A reduced-χ²
  migration was scoped but blocked on per-channel σ being carried
  alongside measurements. Still using the Gaussian-noise-ish
  approximation.
- **Multi-species worlds, inter-species contact.** One species per
  planet by design today. Contact between two species on the same
  planet (or two planets) is a vision-adjacent direction, not
  currently in scope.
- **Per-individual general population.** The sim models named
  figures + cohorts, not the general population as individuals.
  This is a deliberate scope boundary (computational tractability
  + project framing as biography rather than agent-based
  simulation).
- **Tier-5 capability simulation.** The transcendence trio
  (`BioelectricResonator`, `FieldPropulsionEngine`,
  `MetamaterialLattice`) gates the `transcendence` run-end but
  doesn't simulate consciousness physics or FTL — the project's
  vision boundary holds (no consciousness physics, no
  faster-than-light). They're narrative milestones, not modelled
  capabilities.
- **Real meteorology / turbulent fluid regimes.** Physics is
  classical-scale and deliberately simplified. Convection
  shipped (1.5D vertical stack), but no turbulence and no
  weather-system fidelity beyond the wind / hydrology / Coriolis
  / radiation laws.

These are not "bugs to fix" — they're intentional scope
boundaries documented here so contributors don't relitigate them
without explicit direction.

## Future maybe

For items currently outside the project vision but not foreclosed
forever, see [`PLANNING.md#future-maybe`](../PLANNING.md#future-maybe).
The threshold for moving anything from there to "shipped" is a
clear consumer plus a vision-direction shift discussion.

## Why this doc exists

The project has 14 per-feature docs and a separate `PLANNING.md`
status anchor. Neither answers "where does the project stand
against its own ambition?" in one place. This doc does. When the
vision evolves or a new feature lands, update the relevant
pillar's table here in the same commit that touches the
per-feature doc.
