# Discovery

How civs derive law coefficients from emergent physics. The
hypothesizer is the engine of the project — every "they discovered
X" event in the post-run report came out of this pipeline.

For deeper detail per crate, see the discovery module
[`sim/civ/src/discovery/`](../sim/civ/src/discovery) and
[`sim/civ/README.md`](../sim/civ/README.md). For the templates
that fire and feed observations see [recognition.md](recognition.md).

## Two parallel sample tracks

Per civ, per active figure, the hypothesizer runs two tracks:

```
firing relations:        (x = channel reading, y = did the template
                          fire on this cell)
                         → recovers thresholds

measurement relations:   (x = channel reading, y = channel reading)
                         → both axes continuous; recovers SI
                           coefficients (e.g. heat-diffusion α)
```

Firing relations recover *whether* a phenomenon happens past a
threshold; measurement relations recover *how much* one channel
moves per unit of another. A confirmed ThresholdStep on the firing
track tells the civ "lightning fires when charge ≥ 40";
confirmed Linear on the measurement track tells them
"`ΔT ≈ α·∇²T` with α ≈ 0.18".

## Form vocabulary

Twelve fitted forms. A form is a parameterised functional shape +
a fit method:

| Form | Shape | Tag |
|------|-------|-----|
| `Constant` | `y = c` | (none — universal) |
| `Linear` | `y = a·x + b` | (none — universal) |
| `PeriodicSine` | `y = a·sin(ωx + φ) + b` | `Periodic` |
| `InverseSquare` | `y = k / x²` | `DistanceDecay` |
| `ExpDecay` | `y = a·exp(-b·x)` | `ExponentialChange` |
| `ExpGrowth` | `y = a·exp(b·x)` | `ExponentialChange` |
| `Logistic` | `y = L / (1 + exp(-k(x − x₀)))` | `Logistic` |
| `Polynomial2` | `y = a·x² + b·x + c` | `Polynomial` |
| `Polynomial3` | `y = a·x³ + b·x² + c·x + d` | `Polynomial` |
| `ThresholdStep` | `y = if x ≥ τ then high else low` | `Threshold` |
| `PowerLaw` | `y = a·xᵇ` | `PowerOrLog` |
| `Logarithmic` | `y = a·ln(x) + b` | `PowerOrLog` |

`FormTag` (`sim_recognition::FormTag`) is the structural class. The
form vocabulary available to a civ is gated by the union of tags
carried by its perceivable templates — civs that never observe a
`Periodic` template literally cannot propose `PeriodicSine`. This
is what makes "different worlds, different sciences" structural,
not narrative.

## Fit metric

RMSE on residuals + exponential confidence:
`confidence = exp(-residual / tolerance)`. A confirm threshold
fires when confidence ≥ `exp(-1) ≈ 0.368`. Tolerance is per-form
and tuned so realistic noise lands at confirm-band confidence.

Reduced-χ² migration is on the long-term roadmap (it requires
per-channel σ to be carried alongside measurements; deferred).

## Hypothesizer lifecycle

Per `(template, channel)` pair, the hypothesizer holds:

- A **primary** confirmed form (or none yet).
- A **rivals pool** of alternative forms also fitting the same
  data.
- A **probation** slot when a refinement is on trial.

Per-tick observations feed the accumulator. When a candidate's
confidence crosses the confirm threshold, it's promoted to primary
or to the rivals pool depending on whether a primary exists.

### Refinement (Occam-adjusted)

When a primary's residual drifts upward, the engine proposes an
Occam-adjusted alternative — typically a higher-degree polynomial
or a form with one extra parameter. The proposal runs on probation:
sampled in parallel with the primary for a probation window, then
either *supersedes* the primary (and the old primary returns to
the rivals pool) or *reverts*. Probation prevents flapping when
new data is just noisy.

### Cosmology bias

Cosmology axes bias confirmation. A dogmatic civ finds heretical
forms (those pushing against an axis dominated by canon)
multiplicatively harder to confirm; a reformist civ confirms
faster. See [culture.md](culture.md#cosmology) for the axes.

### Falsification streak

Confirmed relations track a `falsification_streak` counter:
sustained drift > 1.5× confirm-time residual. When the streak hits
30 ticks, `RelationFalsified` emits and force-triggers refinement
faster than the slow confidence-streak path. Wrong knowledge dies
under sustained mispredictions.

## Competing hypotheses (rivals)

Multiple forms can be confirmed on the same `(template, channel)`
simultaneously. The rivals pool stores them in confidence order.
When a rival's confidence overtakes the primary's, it displaces
the incumbent (who returns to the rivals pool):

- `RivalHypothesisProposed` — a new form crosses confirm.
- `PrimaryHypothesisDisplaced` — a rival promotes; old primary
  demotes.

Phlogiston-vs-oxygen, geocentric-vs-heliocentric, miasma-vs-germ:
multiple theories coexist before one wins out.

## Theory hierarchy

Confirmed measurement relations auto-generate **residual children**:
the residuals of the parent fit become the y-axis for a new
candidate against another channel. Up to three levels deep.

Newton's gravity → Mercury's perihelion → ... — civs derive layered
explanations. The basis snapshot is frozen at compose time so the
child's fit is reproducible even if the parent's coefficients
drift.

## Inheritance + revalidation

When a civ collapses, surviving relations may transmit to
successor civs (and to peers via diffusion). Comprehension
fidelity is gated by linguistic distance, cultural distance, time
elapsed, and the artefact's persistence tier.

After transmission, the inheriting civ doesn't trust the relation
unconditionally. It re-fits the inherited form against its own
observations after a 50-tick window:

- Pass → `RelationRevalidated` (relation persists at full
  confidence).
- Fail → `RelationLapsed` (relation is dropped).

The post-run report's per-civ chapter surfaces revalidated /
lapsed / falsified counts so the reader sees how each civ
*engaged* with its inheritance.

## Transmission and mythologization

Inter-civ transmission isn't binary. The comprehension scalar is
continuous and lands the relation in one of three bands:

- **High comprehension** — full transfer; receiving civ holds the
  relation as confirmed knowledge.
- **Mid comprehension (mythologization band)** — the relation
  *content* doesn't transfer. Instead, a `RelationMythologized`
  event nudges the receiving civ's cosmology along an axis aligned
  with the relation's themes. A society that lost the original
  physics of a phenomenon retains the taboo, ritual, or sacred
  reverence around it.
- **Low comprehension** — nothing transfers.

The mythologization band closes the "lost knowledge leaves a hole"
problem — real history often preserves the cultural shadow of
forgotten science.

## Controlled experiments

Civs that unlock the experiment-apparatus tool deterministically
clamp a physics channel on one of their cells through a four-value
ladder and sample the response. See
[tech.md#experiment-apparatus](tech.md#experiment-apparatus) for
unlock prereqs and ladder mechanics.

The experimental track:

- Pre-physics, `write_apparatus_clamps` overwrites the apparatus
  cell's clamp channel with one of four ladder values keyed by
  `tick % 4`.
- Post-physics, `record_apparatus_samples` reads the cell's
  measure-channel and feeds the `(clamp, response)` pair into the
  first active figure's hypothesizer measurement track via
  `Hypothesizer::record_experimental_measurement`.
- The pair is weighted 2× in the fit accumulator and increments
  the `experimental_count_by_relation` sidecar.
- When the relation later confirms, `MeasurementConfirmed.is_experimental`
  fires `true` if any apparatus contributions reached it.

A tactile-only ToolExtension-bearing species can derive its
planet's heat-diffusion `α` from a clean clamped-T-then-relax
experiment; a no-tool species stays observation-only forever.

## Events

Discovery emits a tight set of events into NDJSON
([`protocol/src/discovery_events.rs`](../protocol/src/discovery_events.rs)):

- `RelationConfirmed` / `RelationFalsified` / `RelationRevalidated`
  / `RelationLapsed` / `RelationMythologized`.
- `MeasurementConfirmed` (carries `is_experimental` flag).
- `RefinementProposed` / `RefinementConfirmed` / `RefinementRejected`.
- `RivalHypothesisProposed` / `PrimaryHypothesisDisplaced`.

The post-run report walks these to render per-civ knowledge
chapters with fitted equations, residuals, refinement chains, and
the rivals graveyard.
