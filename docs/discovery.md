# Discovery

How civs derive named laws + SI coefficients from emergent physics.
The `Hypothesizer` is the engine of the project — every "they
discovered X" event in the post-run report came out of this pipeline.

For deeper detail per crate, see the discovery module
[`sim/civ/src/discovery/`](../sim/civ/src/discovery) and
[`sim/civ/README.md`](../sim/civ/README.md). For the templates that
fire and feed observations see [recognition.md](recognition.md). For
how confirmed relations gate tech unlocks see
[tech.md#per-tool-spec](tech.md#per-tool-spec).

## Hypothesizer pipeline

Each named figure inside a civ owns their own `Hypothesizer`
([`hypothesizer.rs:84`](../sim/civ/src/discovery/hypothesizer.rs)):

- **Two parallel candidate tracks** — firing relations (binary y =
  did the template fire?) and measurement relations (continuous y
  from a physics channel).
- **Rolling per-relation sample window** capped at `max_window =
  200`.
- **Per-relation `next_attempt` schedule** throttled to
  `attempt_period` ticks (default 20; species-derived; cut by the
  tech `discovery_rate_bonus` multiplier).
- **Confirmed registries** for both tracks
  (`confirmed: BTreeMap<u32, ConfirmedRelation>` /
  `confirmed_measurements: BTreeMap<u32, ConfirmedMeasurement>`).
- **Rivals pool** for competing-hypothesis storage.
- **Available form vocabulary** narrowed at construction from the
  perceivable-template structural tags via
  `derive_available_forms`.

### firings → samples → fits → confirmation

Per tick, the pipeline runs four phases inside the figure:

1. **Firings** — `RecognitionLibrary::scan_with_discovered`
   ([`sim/recognition/src/lib.rs:404`](../sim/recognition/src/lib.rs))
   produces `Firing { template_id, cell }` records for every cell
   × template signature match.
2. **Sample collection** — `Hypothesizer::observe_cells`
   ([`hypothesizer.rs:490`](../sim/civ/src/discovery/hypothesizer.rs))
   walks the figure's assigned cells, builds a per-template
   `fired_cells` index, and pushes one sample into every
   `(candidate, cell)` buffer:
   - Firing track: `y = 1.0` if the cell fired the template,
     `0.0` otherwise; `x = channel.read(state, cell)` in
     fit-space (`/ channel.scale()`).
   - Measurement track: `y = m.y_channel.read(state, prev_state,
     cell)`; `x = m.x_channel.read(...)`. `TemporalDelta` channels
     skip the sample when `prev_state` is unavailable.
   - For **residual candidates** (`m.residual_basis.is_some()`):
     subtract the basis's prediction from `y` before pushing — the
     fit operates on *what's left* after the base law explains its
     share.
3. **Fit attempts** — `step_with_cosmology_doubt_and_rate`
   ([`hypothesizer.rs:690`](../sim/civ/src/discovery/hypothesizer.rs))
   walks every candidate due (`tick >= next_attempt[rid]`),
   schedules the next attempt (`tick + next_period`, where
   `next_period = attempt_period / max(discovery_rate, 1.0)`,
   floored at 1 tick), and runs:
   - **Unconfirmed** → `best_confirmable_fit` tries every form in
     low-arity-first priority order; the first fit clearing
     `confidence ≥ exp(-1)` after cosmology + focus weighting
     becomes the confirmed primary. Emits `Confirmed` or
     `MeasurementConfirmed`.
   - **Confirmed without active refinement** → re-evaluate the
     active form's confidence on the latest samples; track the
     `low_confidence_streak` (`confidence ≤ exp(-2)`) and the
     `falsification_streak` (RMSE > 1.5× confirm-time residual).
     When either trigger fires, run the Occam-adjusted alternative
     search and emit `RefinementProposed` if the best alternative
     beats the active form by `switch_margin × doubt_scale`.
   - **In probation** → re-fit the candidate form; on confirm emit
     `RefinementConfirmed` and switch the active form; on
     `tick >= probation_deadline` emit `RefinementRejected`,
     revert, start a 100-tick cooldown.
4. **Residual cascade** — when a measurement relation confirms,
   auto-generate residual children at `child_depth = depth + 1`
   against `Elevation` (the most physically meaningful secondary
   channel), capped at `MAX_RESIDUAL_DEPTH = 3`. The basis is
   *frozen* at compose time (form + params snapshot) so refinement
   of the source doesn't retroactively invalidate child fits. See
   [Theory hierarchy](#theory-hierarchy).

The whole pass is per-figure: figures with different
`cell_assignment` see different sample distributions and may
confirm different relations — real per-figure attribution, not a
round-robin label.

## Fit basis

`fit` in [`sim/civ/src/fit.rs:85`](../sim/civ/src/fit.rs) is the
top-level entry point. The 12-form vocabulary lives in
[`sim/civ/src/forms.rs`](../sim/civ/src/forms.rs):

| Form | Shape | Param arity | Min samples floor `k` | Base tolerance |
|------|-------|------------:|----------------------:|---------------:|
| `Constant` | `y = a` | 1 | 2 | 5% |
| `Linear` | `y = a·x + b` | 2 | 4 | 10% |
| `Logarithmic` | `y = a·ln(x) + b` | 2 | 4 | 15% |
| `ExpDecay` | `y = a·exp(-b·x)` | 2 | 4 | 15% |
| `ExpGrowth` | `y = a·exp(b·x)` | 2 | 4 | 15% |
| `PowerLaw` | `y = a·xᵇ` | 2 | 4 | 20% |
| `InverseSquare` | `y = k / x²` | 1 | 4 | 20% |
| `ThresholdStep` | `[a, b, t]` — step at cut `t` | 3 | 6 | 15% |
| `Polynomial2` | `y = a·x² + b·x + c` | 3 | 6 | 15% |
| `Polynomial3` | `y = a·x³ + b·x² + c·x + d` | 4 | 8 | 20% |
| `Logistic` | `y = L / (1 + exp(-k·(x − x₀)))` | 3 | 10 | 25% |
| `PeriodicSine` | `y = a·sin(b·x + c) + d` | 4 | 12 | 30% |

### Saturating arithmetic

The fit module runs entirely in `Real` (Q32.32 fixed-point from
`sim_arith`). Saturating arithmetic shows up in two places:

- **`min_samples` ceiling** ([`fit.rs:66`](../sim/civ/src/fit.rs))
  computes `n_min = ceil(k_form / intelligence)` via raw Q-format
  bit-shifts so a tiny intelligence doesn't blow the cast; the
  result is floored at 1.
- **`evaluate` argument clamps** ([`forms.rs:200`](../sim/civ/src/forms.rs))
  clamp `exp` arguments to `[-20, +20]` so a fit whose `b · x`
  drifts past Q32.32's exp range doesn't panic; `PowerLaw` and
  `Logarithmic` short-circuit to `Real::ZERO` for `x ≤ 0`;
  `InverseSquare` guards against `x * x → 0` (Q32.32 underflow at
  `|x| ≲ 1.5e-5`).

The closed-form fits (`Constant`, `Linear`, `Polynomial2/3`,
`Logarithmic`, `InverseSquare`, `ExpDecay`/`Growth`/`PowerLaw` via
log-linearisation) use Gauss-Jordan elimination on
`Σϕ(x)ϕ(x)ᵀ`. `ThresholdStep` runs a search-based fit. `Logistic`
and `PeriodicSine` return `None` from `fit::fit` for now —
iterative-fit forms are a tuning follow-up. Tag-mapped vocabulary
sets still include them so the gate flips on without a re-author
once iterative fits land.

### Form vocabulary

The form vocabulary available to a civ is **not** the full 12 — it
is derived from the union of `FormTag`s carried by the civ's
perceivable templates via
`derive_available_forms` ([`forms.rs:265`](../sim/civ/src/forms.rs)):

| `FormTag` | Forms unlocked |
|-----------|----------------|
| (baseline) | `Constant`, `Linear` (always available) |
| `Threshold` | `ThresholdStep` |
| `Periodic` | `PeriodicSine` |
| `DistanceDecay` | `InverseSquare`, `PowerLaw` |
| `ExponentialChange` | `ExpDecay`, `ExpGrowth` |
| `Logistic` | `Logistic` |
| `Polynomial` | `Polynomial2`, `Polynomial3` |
| `PowerOrLog` | `PowerLaw`, `Logarithmic` |

A species whose perceivable templates never carry a `Periodic` tag
literally cannot propose `PeriodicSine` — different worlds get
structurally different scientific vocabularies, not just different
event timelines.

## Tolerance and minimum-sample formulas

Per the docstring at [`fit.rs:1`](../sim/civ/src/fit.rs):

```
residual   = sqrt(Σ(y_i − f(x_i))² / n)             (RMSE)
tolerance  = base_per_form × (1 / intelligence) / sqrt(n)
confidence = exp(−residual / tolerance)
n_min      = ceil(k_form / intelligence)            (per-form floor k_form)
```

A fit is **confirmable** when `confidence ≥ exp(-1) ≈ 0.368`
(equivalently `residual ≤ tolerance`). `is_confirmed()` at
[`fit.rs:57`](../sim/civ/src/fit.rs) is the gate.

`compute_tolerance` ([`fit.rs:464`](../sim/civ/src/fit.rs)) clamps
intelligence at a `Real::percent(1)` floor so the tolerance stays
positive and finite for degenerate species.

### Refinement thresholds

| Constant | Value | Source |
|----------|------:|--------|
| Refinement-trigger confidence | `exp(-2) ≈ 0.135` | [`hypothesizer.rs:25`](../sim/civ/src/discovery/hypothesizer.rs) |
| `SUSTAINED_TRIGGER_TICKS` | 50 | [`hypothesizer.rs:30`](../sim/civ/src/discovery/hypothesizer.rs) |
| `REFINEMENT_COOLDOWN_TICKS` | 100 | [`hypothesizer.rs:31`](../sim/civ/src/discovery/hypothesizer.rs) |
| `PROBATION_WINDOW_TICKS` | 200 | [`hypothesizer.rs:71`](../sim/civ/src/discovery/hypothesizer.rs) |
| `REVALIDATION_WINDOW_TICKS` | 50 (~4 sim-yr) | [`hypothesizer.rs:40`](../sim/civ/src/discovery/hypothesizer.rs) |
| `MAX_RESIDUAL_DEPTH` | 3 | [`hypothesizer.rs:48`](../sim/civ/src/discovery/hypothesizer.rs) |
| `DEFAULT_FALSIFICATION_TRIGGER_TICKS` | 30 | [`hypothesizer.rs:62`](../sim/civ/src/discovery/hypothesizer.rs) |
| `falsification_drift_ratio` | 1.5× | [`hypothesizer.rs:68`](../sim/civ/src/discovery/hypothesizer.rs) |
| `occam_lambda` | 2% | [`hypothesizer.rs:73`](../sim/civ/src/discovery/hypothesizer.rs) |
| `switch_margin` | 5% | [`hypothesizer.rs:77`](../sim/civ/src/discovery/hypothesizer.rs) |

The streak threshold for refinement triggering is
`SUSTAINED_TRIGGER_TICKS.div_ceil(attempt_period)`
([`hypothesizer.rs:1136`](../sim/civ/src/discovery/hypothesizer.rs))
— ticks-to-steps conversion so the wall-clock dynamics match the
spec across species with different cadences. Civ-aware callers
overwrite `falsification_trigger_ticks` via
`set_falsification_trigger_ticks` from
`streak_ticks_for_metabolism(120, metabolism)` so slow-substrate
worlds get correspondingly longer windows (silicate metabolism
≈ 0.2 → ~5× the aqueous baseline).

## Channels

`Channel` ([`channels.rs:13`](../sim/civ/src/discovery/channels.rs))
is the discovery-pipeline channel enum, distinct from recognition's
`ChannelKind` (which names *what a species senses*). `Channel`
names what the hypothesizer reads from physics state:

| Variant | Reads | `Channel::scale()` |
|---------|-------|-------------------:|
| `Temperature` | Cell temperature (K) | 100 |
| `WaterDepth` | Surface solvent column (m) | 100 |
| `ChargeMagnitude` | `|charge|` (the EM gradient electroreceptors read) | 10 |
| `Elevation` | Static terrain elevation (m) | 1000 |
| `Fuel` | `Substance::Fuel` density | 1 |
| `Oxidiser` | `Substance::Oxidiser` density | 1 |
| `Vapour` | `Substance::Vapour` density | 1 |
| `Ice` | `Substance::Ice` density | 1 |
| `Fossil` | `Substance::Fossil` density (geological hydrocarbon deposits) | 1 |
| `MagneticField` | `|B|` from the magnetism kernel (the dipole, not local charge) | 1 |
| `Resonance` | Per-cell resonance field `Ψ` (the field/resonance lever substrate; see [archetype.md](archetype.md)) | 1 |

Samples are stored in **fit-space** (raw value `/ Channel::scale()`)
so the `Σϕ(x)ϕ(x)ᵀ` accumulator stays inside Q32.32 range even with
hundreds of samples on wide-range channels. `params_in_real_units()`
on `ConfirmedRelation` / `ConfirmedMeasurement` rescales back to SI
for event emission and reporting.

### Channel ↔ modality bridge

`channels_for_modality` ([`channels.rs:107`](../sim/civ/src/discovery/channels.rs))
maps each `sim_species::ModalityKind` to the discovery `Channel`s
reachable through it:

| Modality | Channels |
|----------|----------|
| `VisualLight` / `VisualPolarization` | `Temperature`, `Elevation` |
| `InfraredThermal` | `Temperature` |
| `ChemicalTaste` / `ChemicalPheromone` | `Vapour`, `Oxidiser` |
| `AcousticAir` / `AcousticWater` / `Seismic` | `WaterDepth`, `Elevation`, `Temperature` |
| `ElectricField` | `ChargeMagnitude`, `Resonance` |
| `MagneticSense` / `RadioNative` | `MagneticField`, `Resonance` |
| `Tactile` | `Temperature`, `Elevation` |
| `Bioluminescent` / `Gestural` / `Postural` | (empty — pure-output modalities) |

`perceivable_channels` unions across the species' modality list;
the empty union falls back to a contact baseline of `Temperature`
+ `Elevation` (every creature with a body can read these). The
hypothesizer's candidate cross-product is then
`perceivable_template_ids × perceivable_channels` — both axes
restricted by the species sensorium so a magnetic-sense species
and a visual-light species draw structurally different
observational manifolds.

## Relation-id scheme

Three disjoint namespaces ([`channels.rs:69`](../sim/civ/src/discovery/channels.rs),
[`types.rs:81`](../sim/civ/src/discovery/types.rs)):

- **Firing** — `relation_id_for(template_id, channel) =
  template_id × 16 + (channel as u32)`. Caps under 16 channels;
  current 11 channels leave room for 5 more.
- **Measurement (direct)** — `measurement_relation_id(y, x) =
  1_000_000 + y.discriminant × 256 + x.discriminant`.
- **Measurement (residual)** — `2_000_000 + (mixed % 1_000_000)`
  where `mixed` folds the parent's `relation_id` and depth so the
  same `(y, x)` pair against different bases / depths gets distinct
  ids.

The split keeps the three catalogues coexisting in the same
`relation_id` namespace without renumbering when sensorium-extending
tools widen the perceivable templates.

## Hypothesis events

`HypothesisEvent` ([`events.rs`](../sim/civ/src/discovery/events.rs))
is the per-tick output of `Hypothesizer::step`. `sim/core` maps each
variant to a protocol event:

| `HypothesisEvent` | Protocol event | Fires when |
|-------------------|----------------|------------|
| `Confirmed(ConfirmedRelation)` | `RelationConfirmed` | First fit on a firing candidate clears `confidence ≥ exp(-1)`. |
| `MeasurementConfirmed(ConfirmedMeasurement)` | `MeasurementConfirmed` | First fit on a measurement candidate clears `exp(-1)`; carries `is_experimental` if apparatus contributed. |
| `RefinementProposed { ... }` | `RefinementProposed` | Active form's sustained-low-confidence streak or falsification streak triggers; an alternative form beats it by `switch_margin × doubt_scale`. Probation begins. |
| `RefinementConfirmed { ... }` | `RefinementConfirmed` | Probationary form re-fits at `confidence ≥ exp(-1)`. Active form swaps; old primary returns to the rivals pool. |
| `RefinementRejected { ... }` | `RefinementRejected` | `tick >= probation_deadline` without a confirm. Revert + 100-tick cooldown. |
| `Falsified { ... }` | `RelationFalsified` | `falsification_streak` hits `falsification_trigger_ticks`. Force-triggers refinement faster than the confidence-streak path. |
| `Revalidated { ... }` | `RelationRevalidated` | Inherited relation re-fits successfully on the successor's own samples after `REVALIDATION_WINDOW_TICKS`. Graduates to native confirmed status. |
| `Lapsed { ... }` | `RelationLapsed` | Inherited relation fails the re-fit. Dropped from `confirmed`. |

Two more discovery-domain events live outside `HypothesisEvent`
because they're emitted by `sim/core` rather than the hypothesizer:

- `RelationMythologized(...)` — emitted by inter-civ transmission
  when comprehension lands in `(MYTH_FLOOR, TRANSMIT_THRESHOLD]`
  (see [Cultural lock + mythologization](#cultural-lock--mythologization)).
- `RivalHypothesisProposed` / `PrimaryHypothesisDisplaced` — emitted
  by the `Hypothesizer::add_rival_hypothesis` /
  `displace_primary_with_best_rival` calls
  ([`hypothesizer.rs:594`](../sim/civ/src/discovery/hypothesizer.rs))
  for the multiple-coexisting-theories pattern.

## Cosmology + religion bias

Two epistemic layers shape confirmation:

- **Cosmology** ([`sim/civ/src/cosmology.rs`](../sim/civ/src/cosmology.rs))
  — 5 axes (`empirical`, `communitarian`, `reformist`, `mystical`,
  `hierarchical`), each `[-1, 1]`. Slow-drift deep worldview.
- **Religion** ([`sim/civ/src/religion.rs`](../sim/civ/src/religion.rs))
  — 3 axes (`theology`, `ritual`, `sacred_time`). Fast-divergent
  cultural layer; the `push_for_*` magnitudes are 3× cosmology's.

`best_confirmable_fit` ([`hypothesizer.rs:1254`](../sim/civ/src/discovery/hypothesizer.rs))
folds two cosmology hooks into the candidate-fit's confidence
before the `is_confirmed` check:

```
suppression = suppress_confidence_for(form, cosmology)
            = clamp(1 − dogmatism × form_distance(form, cosmology), 0, 1)
focus       = focus_weight_for(0, cosmology)
            = 1 + 0.25 × empirical + 0.25 × reformist
confidence  = (raw_confidence × suppression × focus).min(1.0)
```

A dogmatic civ finds heretical forms multiplicatively harder to
confirm; an empirical/reformist civ confirms faster.
`combined_form_distance` adds religion's axis preferences on top
(monist theology likes unifying forms; liturgical ritual prefers
step semantics; cyclical sacred-time embraces periodic).

Per-figure `doubt` scales the refinement-readiness `switch_margin`
([`hypothesizer.rs:1172`](../sim/civ/src/discovery/hypothesizer.rs))
via `doubt_scale = 1.5 − doubt`:

- doubt = 0.0 → 1.5× margin (conservative — stick with confirmed
  forms longer).
- doubt = 0.5 → 1.0× margin (neutral).
- doubt = 1.0 → 0.5× margin (aggressive — challenge confirmed
  relations sooner).

## Per-figure charisma weighting

Each `NamedFigure` carries four `[0, 1]` personality scalars
([`figures.rs:42`](../sim/civ/src/figures.rs)):

- `charisma` — scales cosmology + religion drift magnitudes.
- `curiosity` — reserved for fit-attempt cadence (forward-compat).
- `doubt` — feeds the refinement-readiness `switch_margin` scaler
  above.
- `communicativeness` — boosts comprehension on transmissions
  originating from this figure.

In the discovery-emission phase
([`sim/core/src/phases.rs:280`](../sim/core/src/phases.rs)), every
`HypothesisEvent` pushes both cosmology and religion in matching
directions. The push magnitude is **scaled by the originating
figure's charisma**:

```rust
let charisma = civ.figure_charisma(*figure_id);
civ.apply_cosmology_push(&cp, charisma);
civ.apply_religion_push(&rp, charisma);
```

A high-charisma figure's `Confirmed` event drives the civ's
cosmology toward empirical (and away from mystical) far more than
a low-charisma figure's would. Charismatic founders trigger the
"charismatic-founder" effect at `charisma >= 0.8`
([`figures.rs:57`](../sim/civ/src/figures.rs)). Per-figure charisma
is sampled deterministically from the founder's name-grammar seed
([`figures.rs:234`](../sim/civ/src/figures.rs)), so the same seed
produces the same charismatic-drift trajectory across replays.

## Cosmic-ray-flux-driven mutation

The biology layer is sensitive to a planet's cosmic-ray flux:
`PhysicsState::cosmic_ray_ground_flux`
([`sim/physics/src/state.rs:954`](../sim/physics/src/state.rs))
returns `1 / (dipole_strength + 0.1)`. A planet at `strength = 1`
sees a multiplier of ~0.91; a planet mid-magnetic-reversal at
`strength = 0.1` sees ~5.0 — a ~5× surface-flux amplification.

The multiplier feeds `step_speciation` and `step_hgt`
([`sim/core/src/run_tick.rs:261`](../sim/core/src/run_tick.rs))
through `cosmic_mult`, which scales speciation trial rates and
horizontal-gene-transfer outcomes per tick. A reversal-window
planet sees more daughter species in the same tick budget than a
stably-magnetised one.

The coupling into **discovery** is indirect but real: speciation
produces daughter species with potentially different
`ModalityKind` rosters and different `intelligence` /
`attempt_period` drift trajectories. The next founding civ on the
daughter species draws a different `perceivable_channels` set and a
different fit cadence — *different sciences* emerge structurally
from biological mutation accumulating under cosmic-ray pressure.

There is no direct "mutate this hypothesis" hook inside the
hypothesizer — mutation flows through species change → civ founding
→ hypothesizer construction, not through perturbations of confirmed
relations mid-run. The discovery pipeline is the deterministic part
of the stack; the cosmic-ray channel is one of the upstream sources
of structural variation it sees.

## Theory hierarchy (residual cascade)

Confirmed measurement relations auto-generate **residual children**
([`hypothesizer.rs:830`](../sim/civ/src/discovery/hypothesizer.rs)):
when a measurement confirms at `depth < MAX_RESIDUAL_DEPTH = 3`, a
child candidate is added with `y_channel` unchanged and `x_channel
= Direct(Elevation)`. The child's basis stores the parent's
form + params + x-channel as a *frozen* snapshot. At sample time
([`hypothesizer.rs:555`](../sim/civ/src/discovery/hypothesizer.rs))
the child subtracts the basis's prediction from `y` before
pushing, so the fit operates on the residual.

The frozen-basis choice means refinement of the source doesn't
retroactively invalidate the child — the hierarchy captures the
civ's *understanding at the moment of composition*, which is a
more faithful biography than live re-evaluation. Newton's gravity
→ Mercury's perihelion residual → ... — civs build layered
explanations across depths 0 → 1 → 2 → 3.

Residual ids live in the `2_000_000+` namespace
([`types.rs:81`](../sim/civ/src/discovery/types.rs)) so they never
collide with direct measurements or firing relations.

## Competing hypotheses (rivals)

Multiple forms can be confirmed on the same `(template, channel)`
simultaneously. `rivals: BTreeMap<u32, Vec<ConfirmedRelation>>`
([`hypothesizer.rs:114`](../sim/civ/src/discovery/hypothesizer.rs))
stores alternatives in addition to the primary in
`confirmed[relation_id]`:

- `add_rival_hypothesis(relation_id, rival)` accepts the rival if
  it doesn't duplicate the primary's form or another rival's.
- `displace_primary_with_best_rival(relation_id)` swaps the primary
  with the highest-confidence rival if the rival's confidence
  exceeds the primary's. The displaced primary returns to the
  rivals pool so future swaps can flip back.

Phlogiston vs oxygen, geocentric vs heliocentric, miasma vs germ
— multiple theories coexist before one displaces the other.
`sim/core` (or downstream callers) populates rivals via
`add_rival_hypothesis` and resolves via
`displace_primary_with_best_rival`. The hypothesizer ships the
storage and the API; auto-triggering logic is deferred so proposal
cadence can be tuned without re-shipping the data shape.

## Apparatus + experimental relations

`Hypothesizer::record_experimental_measurement`
([`hypothesizer.rs:317`](../sim/civ/src/discovery/hypothesizer.rs))
is the apparatus entry point. Each apparatus sample:

1. Pushes the `(x, y)` pair into the measurement buffer **twice**
   to express the information-density advantage of controlled
   samples.
2. Bumps `experimental_count_by_relation` by 2.
3. Lazily adds the candidate / buffer / next_attempt entries if the
   `(y, x)` pair isn't in the default catalogue — apparatus and
   observation samples on the same `(y, x)` pair co-fit.

At confirm time, `is_experimental = experimental_count > 0` —
any apparatus contribution flips the flag. The
`min_civ_experimental_relations` tech gate counts only
measurement relations flagged this way; a civ that never builds
`ExperimentApparatus` tops out at tier-2 by construction. See
[tech.md#experiment-apparatus](tech.md#experiment-apparatus) for
the apparatus mechanics.

## Inheritance + revalidation

When a civ collapses, surviving relations may transmit to
successor civs via the founding pipeline (and to peers via
diffusion). The successor doesn't trust the inherited fit
unconditionally — it tags inherited relations with
`inherited_from_tick` + `inherited_from_civ_id` and re-fits the
form against its own samples after `REVALIDATION_WINDOW_TICKS = 50`
([`hypothesizer.rs:926`](../sim/civ/src/discovery/hypothesizer.rs)):

- **Pass** → emit `Revalidated`. Clear inheritance tags; the
  relation graduates to native confirmed status with refreshed
  params and residual.
- **Fail** → emit `Lapsed`. Drop from `confirmed`. The successor
  has rejected the parent's theory on its own data.

The post-run report's per-civ chapter surfaces revalidated /
lapsed / falsified counts so the reader sees how each civ
*engaged* with its inheritance.

## Transmission and mythologization

Inter-civ transmission isn't binary. The comprehension scalar
(`comprehension = (1 − linguistic_dist) × age_decay × 0.7`,
[`transmission.rs:86`](../sim/civ/src/transmission.rs)) lands the
relation in one of three bands:

- **`comprehension > TRANSMIT_THRESHOLD = 0.15`** — full transfer;
  the receiving civ holds the relation as confirmed knowledge
  (with `inherited_from_*` tags pending revalidation).
- **`MYTH_FLOOR = 0.03 < comprehension ≤ TRANSMIT_THRESHOLD`** —
  the relation *content* doesn't transfer. Instead, a
  `RelationMythologized` event nudges the receiving civ's
  cosmology along an axis aligned with the relation's themes
  (`mythologization_axis` at [`transmission.rs:135`](../sim/civ/src/transmission.rs)
  picks the axis deterministically: 60% mystical bias, 40% spread
  across the other four). A society that lost the original physics
  of a phenomenon retains the taboo, ritual, or sacred reverence
  around it.
- **`comprehension ≤ MYTH_FLOOR`** — nothing transfers.

The mythologization band closes the "lost knowledge leaves a hole"
problem — real history often preserves the cultural shadow of
forgotten science.

## Cultural lock detection

`Civ::cultural_lock_streak` ([`sim/civ/src/lib.rs:261`](../sim/civ/src/lib.rs))
ticks up every tick that:

- `Cosmology::dogmatism()` ≥ `CULTURAL_LOCK_DOGMA = 0.85`
  ([`sim/civ/src/lib.rs:540`](../sim/civ/src/lib.rs)), AND
- `tick − last_refinement_tick` ≥ `cultural_lock_streak` (the
  metabolism-scaled window).

`last_refinement_tick` is bumped on every `RefinementConfirmed`
event; a civ that runs in dogmatic mode without successfully
refining any of its laws accumulates the streak.

When the streak reaches `CULTURAL_LOCK_STREAK_TICKS = 250 ×
MONTHS_PER_YEAR` ([`lib.rs:541`](../sim/civ/src/lib.rs)) — scaled
by substrate metabolism via `streak_ticks_for_metabolism` in the
production path ([`lifecycle.rs:136`](../sim/civ/src/lifecycle.rs))
— the civ collapses with `CollapseReason::CulturalLock`. Dogmatic
+ stagnant = dead.

The streak resets when either gate fails: a single
`RefinementConfirmed` resets `last_refinement_tick`, and a single
tick with `dogmatism < 0.85` resets the counter.

This is the discovery side of the same dial cosmology drift
controls: enough `Confirmed` events push empirical and pull
mystical, lowering dogmatism; enough `RefinementRejected` and
`Lapsed` events push the other way. The cultural-lock trigger is
what closes the loop on civs whose epistemology has stopped
*producing*.
