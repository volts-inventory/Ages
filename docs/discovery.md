# Discovery

How civs derive law coefficients from emergent physics. The
hypothesizer is the engine of the project — every "they discovered
X" event in the post-run report came out of this pipeline.

For deeper detail per crate, see the discovery module
[`sim/civ/src/discovery/`](../sim/civ/src/discovery) and
[`sim/civ/README.md`](../sim/civ/README.md). For the templates
that fire and feed observations see [recognition.md](recognition.md).

## Hypothesizer pipeline

A civ holds one `Hypothesizer` per named figure
([`hypothesizer.rs:85`](../sim/civ/src/discovery/hypothesizer.rs)).
Each figure runs the same four-stage pipeline per tick:

```
firings → samples → fits → confirmation
   │         │        │          │
   │         │        │          └─ exp(-residual/tolerance) ≥ exp(-1)
   │         │        │             promotes to ConfirmedRelation /
   │         │        │             ConfirmedMeasurement.
   │         │        └─ best_confirmable_fit walks available_forms,
   │         │           lowest-arity first, returns the first hit.
   │         └─ observe_cells accumulates per-relation rolling windows
   │            of (x, y) samples, capped at max_window = 200.
   └─ RecognitionLibrary::scan emits Firing { template_id, cell };
      Civ::perceivable_firings filters to the species + extra-perceivable
      sensorium set.
```

### Two parallel sample tracks

Per civ, per active figure, the hypothesizer runs two tracks:

| Track | x | y | Recovers |
|-------|---|---|----------|
| **Firing relations** | channel reading | `1.0` if template fired on cell else `0.0` | Thresholds (does the phenomenon happen past this value?). Confirmed `ThresholdStep` on lightning_buildup tells the civ "lightning fires when `\|Q\| ≥ 40`". |
| **Measurement relations** | channel reading | another channel reading (or temporal/spatial derivative) | SI coefficients (how much one channel moves per unit of another). Confirmed `Linear` on `TemporalDelta(T)` vs `Laplacian(T)` recovers `ΔT ≈ α · ∇²T` with `α ≈ 0.18`. |

The cross-product of perceivable templates and perceivable channels
yields the firing candidate set
(`Hypothesizer::candidates_for_with_channels`,
[`hypothesizer.rs:185`](../sim/civ/src/discovery/hypothesizer.rs)).
The default measurement set is built in
`default_measurements` ([`hypothesizer.rs:373`](../sim/civ/src/discovery/hypothesizer.rs))
and includes spatial-equilibrium pairings
(`Temperature` ↔ `NeighbourMean(Temperature)`,
`WaterDepth` ↔ `NeighbourMean(WaterDepth)`,
`ChargeMagnitude` ↔ `NeighbourMean(ChargeMagnitude)`,
`Temperature` ↔ `Elevation` as a thermal lapse-rate proxy) plus
temporal-derivative pairings
(`TemporalDelta(T)` ↔ `Laplacian(T)`, equivalents for
`ChargeMagnitude` and `WaterDepth`). The temporal pairings recover
the actual physical-law coefficients (heat-conduction `α`, EM
conductivity, gravity-flow `k`).

## Channels

The hypothesizer's `Channel` enum
([`channels.rs:13`](../sim/civ/src/discovery/channels.rs))
enumerates **10 physics channels** the fit pool may use as the
independent variable: `Temperature`, `WaterDepth`,
`ChargeMagnitude`, `Elevation`, `Fuel`, `Oxidiser`, `Vapour`,
`Ice`, `Fossil`, `MagneticField`.

Per-channel normalisation scales (`Channel::scale`, line 187) keep
fit accumulators inside Q32.32 range. `Temperature` is divided by
100 (so 200–400 K maps to 2.0–4.0), `Elevation` by 1000, and so
on. Discovered parameters are stored in this fit-space and
rescaled to real units on event emit via
`ConfirmedRelation::params_in_real_units` /
`ConfirmedMeasurement::params_in_real_units`.

`channels_for_modality` ([`channels.rs:107`](../sim/civ/src/discovery/channels.rs))
maps each species `ModalityKind` to its observable physics
channels. A visual species reads `Temperature` + `Elevation`; an
infrared species reads `Temperature` only; an electric-field
species reads `ChargeMagnitude`; a magnetic-sense / radio-native
species reads `MagneticField`. Output-only modalities
(`Bioluminescent`, `Gestural`, `Postural`) contribute no channels;
species with only those modalities fall back to a minimum-viable
`Temperature + Elevation` contact set.

`MeasurementChannel` ([`channels.rs:244`](../sim/civ/src/discovery/channels.rs))
wraps each `Channel` in one of four spatial / temporal modes:
`Direct`, `NeighbourMean`, `Laplacian`
(`Σ(neighbour - cell)` over the 6 axial hex neighbours),
`TemporalDelta` (`current - previous`). `Laplacian` is the
discrete diffusion operator; fits against `TemporalDelta` recover
diffusion coefficients.

## Fit basis

`Form` ([`forms.rs:17`](../sim/civ/src/forms.rs)) is the
parameterised functional shape vocabulary. Twelve forms ship at
boot:

| Form | Shape | Tag | Param count |
|------|-------|-----|------------:|
| `Constant` | `y = a` | — | 1 |
| `Linear` | `y = a·x + b` | — | 2 |
| `Polynomial2` | `y = a·x² + b·x + c` | `Polynomial` | 3 |
| `Polynomial3` | `y = a·x³ + b·x² + c·x + d` | `Polynomial` | 4 |
| `PowerLaw` | `y = a·xᵇ` | `PowerOrLog` | 2 |
| `Logarithmic` | `y = a·ln(x) + b` | `PowerOrLog` | 2 |
| `ExpDecay` | `y = a·exp(-b·x)` | `ExponentialChange` | 2 |
| `ExpGrowth` | `y = a·exp(b·x)` | `ExponentialChange` | 2 |
| `InverseSquare` | `y = a / x²` | `DistanceDecay` | 1 |
| `Logistic` | `y = L / (1 + exp(-k(x − x₀)))` | `Logistic` | 3 |
| `PeriodicSine` | `y = a·sin(ωx + φ) + b` | `Periodic` | 4 |
| `ThresholdStep` | `y = a if x < t else b`, params `[a, b, t]` | `Threshold` | 3 |

`Constant` and `Linear` are always available. The other ten are
gated by the union of `FormTag`s carried by the civ's
perceivable templates. A civ that never observes a `Periodic`
template literally cannot propose `PeriodicSine`; this is what
makes "different worlds, different sciences" structural, not
narrative.

`PeriodicSine` and `Logistic` are nonlinear in their parameters
and currently stubbed in `fit_params`
([`fit.rs:138`](../sim/civ/src/fit.rs)). The linear-in-params
forms use Gauss-Jordan least-squares with a basis expansion.

### Saturating arithmetic guards

The Q32.32 fixed-point representation tops out at ±~2.1e9. With
hundreds of samples and `Polynomial3`'s `x³ × x³ = x⁶` cross-terms,
the `Σϕ(x)ϕ(x)ᵀ` accumulator can saturate. Every product and
running sum in `fit_linear_in_basis` and the Gauss-Jordan elimination
uses `saturating_mul` / `saturating_add` / `saturating_sub` so
overflow clamps to the type limit rather than wrapping
([`fit.rs:178`, `262`](../sim/civ/src/fit.rs)). The near-singular
determinant check at the matrix-solve step then rejects the
saturated case rather than producing a junk fit.

This matters because **fits run on potentially noisy real-physics
samples**, not authored test data — the guards prevent rare
adversarial sample windows (a 6th-power blow-up under a tight
elevation distribution) from poisoning the hypothesizer's
confirmed registry.

## Tolerance + minimum-sample formulas

`compute_tolerance` ([`fit.rs:464`](../sim/civ/src/fit.rs)):

```
tolerance = base_per_form / max(intelligence, 0.01) / sqrt(n_samples)
```

A smart species needs less tolerance per fit; more samples narrow
the tolerance further. `base_per_form` is per-form (tighter for
`Constant`/`Linear`, looser for `PeriodicSine`).

`min_samples` ([`fit.rs:66`](../sim/civ/src/fit.rs)):

```
n_min = ceil(k_form / max(intelligence, 0))
```

`k_form` per form ([`forms.rs:80`](../sim/civ/src/forms.rs)) is 2
for `Constant`, 4 for `Linear` / exponentials / power-law, 6 for
`Polynomial2` / `ThresholdStep`, 8 for `Polynomial3`, 10 for
`Logistic`, 12 for `PeriodicSine`. The `intelligence` divisor
means a species at `cognition = 2.0` (above-baseline) needs half
the data a baseline species does to identify a form.

### Intelligence keyed off species cognition

The civ's `intelligence` is the species' `cognition` scalar
threaded through at founding. `attempt_period_for_cognition`
([`demographics.rs:196`](../sim/civ/src/demographics.rs)) sets the
hypothesizer's per-candidate fit cadence at
`max(5, round((1.5 - cognition) × 20))` — a baseline-cognition
species (0.5) attempts a fit every 20 ticks per candidate; a
high-cognition (1.0) species every 10 ticks; a low-cognition
(0.2) species every ~26 ticks. `CognitionTopology` adds a
multiplier: `DistributedRedundant` shrinks by 0.7, `Acentric`
stretches by 5.0. `scale_attempt_period_for_metabolism` finally
applies the planet's metabolism — silicate worlds at
`metabolism = 0.2` get 5× longer periods so "society-scale"
cadence is consistent across substrates.

## Fit metric

RMSE on residuals plus exponential confidence:

```
confidence = exp(-residual / tolerance)
```

A fit confirms when `confidence ≥ exp(-1) ≈ 0.368` (i.e.
`residual ≤ tolerance`). The same exponential is reused for
refinement triggers (`exp(-2) ≈ 0.135` is the
sustained-low-confidence threshold) so the lifecycle constants
land on one scale.

## Hypothesis events

Per-tick output is a `Vec<HypothesisEvent>`
([`events.rs:11`](../sim/civ/src/discovery/events.rs)):

| Event | Fires when |
|-------|------------|
| `Confirmed(ConfirmedRelation)` | A firing-track candidate cleared `exp(-1)`. |
| `MeasurementConfirmed(ConfirmedMeasurement)` | A measurement-track candidate cleared `exp(-1)`. Carries `is_experimental: bool`. |
| `RefinementProposed { … }` | A higher-Occam-score alternative crossed `switch_margin` on a confirmed relation in low-confidence streak. Goes on probation. |
| `RefinementConfirmed { … }` | Probation candidate re-fit and cleared `exp(-1)` within the 200-tick window. Old form returns to rivals; new form is the primary. |
| `RefinementRejected { … reason }` | Probation deadline expired without confirmation. Old form stays; cooldown starts. |
| `Falsified { … streak_ticks }` | A confirmed relation's RMSE drifted above `1.5×` its confirm-time residual for `falsification_trigger_ticks` consecutive ticks. Force-triggers refinement faster than the slow confidence-streak path. |
| `Revalidated { from_civ_id, … }` | An inherited relation re-fit successfully on the successor's own observations after the 50-tick window. Native-confirmed status. |
| `Lapsed { from_civ_id, attempted_form }` | An inherited relation failed to re-fit. Dropped from `confirmed`. |

The post-run report walks these to render per-civ knowledge
chapters with fitted equations, residuals, refinement chains, the
rivals graveyard, and which laws were inherited / revalidated /
lapsed.

`RivalHypothesisProposed` and `PrimaryHypothesisDisplaced` round
out the rival lifecycle (see [#competing-hypotheses](#competing-hypotheses-rivals)
below); `RelationMythologized` lives on the transmission layer
(see [#transmission-and-mythologization](#transmission-and-mythologization)).

## Hypothesizer lifecycle

Per `(template, channel)` pair the hypothesizer holds:

- A **primary** confirmed form (or none yet).
- A **rivals pool** of alternative forms also fitting the same
  data.
- A **probation slot** when a refinement is on trial.
- A rolling sample window capped at 200 entries per relation.

### Confirmation path

`step_unconfirmed` ([`hypothesizer.rs:869`](../sim/civ/src/discovery/hypothesizer.rs))
runs when no primary exists. `best_confirmable_fit`
([`hypothesizer.rs:1254`](../sim/civ/src/discovery/hypothesizer.rs))
walks the available form vocabulary sorted by `param_count` then
`form_priority_tiebreak`, returns the first fit that clears
`exp(-1)` after cosmology suppression + focus weighting are
applied.

### Refinement (Occam-adjusted)

`step_confirmed` re-evaluates the active form's confidence every
tick. When confidence drops to `≤ exp(-2)` for
`SUSTAINED_TRIGGER_TICKS / attempt_period` cycles, the engine
proposes the best Occam-adjusted alternative:

```
score = confidence - occam_lambda × param_count
proposal fires if score_best > score_active + switch_margin × doubt_scale
```

where `occam_lambda = 0.02`, `switch_margin = 0.05`, and
`doubt_scale = 1.5 - figure.doubt` (high-doubt figures push
faster — `RefinementProposed` lands sooner). The new form goes on
probation for 200 ticks; if it confirms in window, it supersedes
the primary and the old primary returns to the rivals pool.
Otherwise the engine emits `RefinementRejected` and enters a
100-tick cooldown.

### Prediction-drift falsification

Confirmed relations track a `falsification_streak`: the count of
consecutive ticks where active RMSE exceeds `1.5×` the
confirm-time `initial_residual`. When the streak hits
`falsification_trigger_ticks` (default 30; substrate-rescaled by
`streak_ticks_for_metabolism`), `RelationFalsified` emits and the
engine force-triggers refinement by lifting
`low_confidence_streak` to its trigger value
([`hypothesizer.rs:1032`](../sim/civ/src/discovery/hypothesizer.rs)).
Wrong knowledge dies under sustained mispredictions instead of
waiting for the slow confidence-streak path.

### Cosmology bias

`step_with_cosmology` ([`hypothesizer.rs:647`](../sim/civ/src/discovery/hypothesizer.rs))
threads the civ's `Cosmology` vector through the fit pipeline.
`culture_hooks::suppress_confidence_for` makes heretical forms
(those pushing against an axis dominated by canon) multiplicatively
harder to confirm for a dogmatic civ; reformist civs confirm
faster. See [culture.md#cosmology](culture.md#cosmology) for the
five axes.

## Competing hypotheses (rivals)

Multiple forms can be confirmed on the same `(template, channel)`
simultaneously. The rivals pool
([`hypothesizer.rs:114`](../sim/civ/src/discovery/hypothesizer.rs))
stores them ordered. `add_rival_hypothesis` accepts an alternative
fit (rejected if the same form is already primary or in the pool);
`displace_primary_with_best_rival` swaps when a rival's confidence
overtakes the primary's:

- `RivalHypothesisProposed` — a new form crossed confirm.
- `PrimaryHypothesisDisplaced` — a rival promoted; old primary
  demoted (re-enters the rivals pool so future swaps can flip back).

Phlogiston-vs-oxygen, geocentric-vs-heliocentric, miasma-vs-germ:
multiple theories coexist before one wins out.

## Theory hierarchy

Confirmed measurement relations auto-generate **residual children**
([`hypothesizer.rs:830`](../sim/civ/src/discovery/hypothesizer.rs)):
the residuals of the parent fit become the y-axis for a new
candidate against another channel. Up to `MAX_RESIDUAL_DEPTH = 3`
levels deep.

The basis (form + params + source channel) is frozen at compose
time in a `ResidualBasis` ([`types.rs:55`](../sim/civ/src/discovery/types.rs)).
Subsequent refinement of the source doesn't retroactively
invalidate the child — the hierarchy captures the civ's
*understanding at the moment of composition*, which is a more
faithful biography than live re-evaluation.

Newton's gravity → Mercury's perihelion → … — civs derive layered
explanations.

## Per-figure charisma weighting

Named figures
([`figures.rs:57`](../sim/civ/src/figures.rs)) carry `charisma`,
`curiosity`, `doubt` scalars in `[0, 1]`. Each affects the
discovery loop differently:

- **`doubt`** threads directly into the hypothesizer:
  `step_with_cosmology_doubt_and_rate`
  ([`hypothesizer.rs:690`](../sim/civ/src/discovery/hypothesizer.rs))
  scales the refinement `switch_margin` by `(1.5 - doubt)`.
  Doubt 1.0 = aggressive (0.5× margin, refinements land sooner);
  doubt 0.0 = conservative (1.5× margin, sticks with confirmed
  forms longer); 0.5 = neutral.
- **`charisma`** weights how strongly the figure's hypothesis
  events nudge the civ's cosmology / religion vectors. When a
  hypothesis event fires, sim/core picks up the figure that
  produced it and calls
  `civ.apply_cosmology_push(&push_for_relation_confirmed(), charisma)`
  ([`sim/core/src/phases.rs:385`](../sim/core/src/phases.rs)).
  High-charisma figures shift the civ's worldview far more per
  event than low-charisma figures — a Newton-equivalent reshapes
  cosmology on a single discovery; an obscure figure barely
  registers. `Civ::figure_charisma` ([`state.rs:269`](../sim/civ/src/state.rs))
  defaults missing figures to 0.5.
- **`curiosity`** modulates which candidate relations a figure
  prioritises sampling on (cell-assignment + observation pickup
  weighting).

## Inheritance + revalidation

When a civ collapses, surviving relations may transmit to
successor civs (and to peers via diffusion). Comprehension
fidelity is gated by linguistic distance, cultural distance, time
elapsed, and the artefact's persistence tier
([`transmission.rs`](../sim/civ/src/transmission.rs)).

After transmission, the inheriting civ doesn't trust the relation
unconditionally. It re-fits the inherited form against its own
observations after a `REVALIDATION_WINDOW_TICKS = 50` window
([`hypothesizer.rs:920`](../sim/civ/src/discovery/hypothesizer.rs)):

- **Pass** → `Revalidated`; `inherited_from_tick` and
  `inherited_from_civ_id` are cleared; the relation is now
  natively confirmed.
- **Fail** → `Lapsed`; relation is dropped from `confirmed`.

The post-run report's per-civ chapter surfaces revalidated /
lapsed / falsified counts so the reader sees how each civ
*engaged* with its inheritance.

## Transmission and mythologization

Inter-civ transmission isn't binary. The comprehension scalar is
continuous and lands the relation in one of three bands:

- **High comprehension** — full transfer; receiving civ holds the
  relation as inherited-confirmed knowledge (subject to
  revalidation).
- **Mid comprehension (mythologization band)** — the relation
  *content* doesn't transfer. Instead, a `RelationMythologized`
  event nudges the receiving civ's cosmology along an axis aligned
  with the relation's themes
  ([`transmission.rs:42`](../sim/civ/src/transmission.rs)). A
  society that lost the original physics of a phenomenon retains
  the taboo, ritual, or sacred reverence around it.
- **Low comprehension** — nothing transfers.

The mythologization band closes the "lost knowledge leaves a hole"
problem — real history often preserves the cultural shadow of
forgotten science.

## Controlled experiments

Civs that unlock `ExperimentApparatus` deterministically clamp a
physics channel on one of their cells through a four-value ladder
and sample the response. See
[tech.md#experiment-apparatus](tech.md#experiment-apparatus) for
unlock prereqs and ladder mechanics.

The experimental track:

- Pre-physics, `write_apparatus_clamps` overwrites the apparatus
  cell's clamp channel with one of four ladder values keyed by
  `tick % 4`.
- Post-physics, `record_apparatus_samples` reads the cell's
  measure-channel and feeds the `(clamp, response)` pair into the
  first active figure's hypothesizer measurement track via
  `Hypothesizer::record_experimental_measurement`
  ([`hypothesizer.rs:317`](../sim/civ/src/discovery/hypothesizer.rs)).
- The pair is pushed **twice** into the fit buffer (information-
  density bonus) and increments
  `experimental_count_by_relation`.
- When the relation later confirms,
  `ConfirmedMeasurement.is_experimental` fires `true` if any
  apparatus contributions reached it.

A tactile-only `ToolExtension`-bearing species derives its
planet's heat-diffusion `α` from a clean clamped-T-then-relax
experiment; a no-tool species stays observation-only forever and
can never satisfy any tier-3+ tool's
`min_civ_experimental_relations` floor.

## Cosmic-ray-flux coupled mutation / speciation

Discovery isn't sealed off from the planet's space-weather
budget. The ecosystem layer
([`sim/ecosystem/src/speciation.rs`](../sim/ecosystem/src/speciation.rs))
couples the per-tick speciation pulse to
`PhysicsState::cosmic_ray_ground_flux()`, which scales as
`1 / (dipole_strength + 0.1)`. A strong, stable magnetosphere
clamps the multiplier to 0 — Earth-like worlds see no continuous
mutation pump. During a **magnetic reversal window**
(`dipole_strength → 0`), flux peaks at ~10 and every triggered
speciation event (Allopatric, Sympatric, Polyploid, FounderEffect,
PostExtinctionRadiation) spawns up to **10 daughters per fire**
instead of one
([`clamp_cosmic_ray_multiplier`, line 674](../sim/ecosystem/src/speciation.rs)).

Why this matters for discovery: every new daughter species starts
with a fresh `Hypothesizer` whose candidate set, channel set, and
available form vocabulary derive from its (possibly drifted)
sensorium. A radiation burst window produces a wide-spectrum
forking of scientific traditions: many sibling species, each
exploring the same physics through subtly different perceivable-
template manifolds. The cosmology / religion drift across the
daughters is then independent. The post-run report shows runs
with stable magnetospheres producing tight monocultural canons,
and runs through deep reversals producing branching, divergent
canons.

## Cultural lock detection

A civ collapses with `CollapseReason::CulturalLock` when **high
dogmatism persists without refinement** for a sustained streak.
The check lives in `Civ::check_collapse_with_terrain`
([`lifecycle.rs:170`](../sim/civ/src/lifecycle.rs)):

```rust
if cosmology.dogmatism() >= CULTURAL_LOCK_DOGMA (0.85)
   && (tick - last_refinement_tick) >= CULTURAL_LOCK_STREAK_TICKS (250 yr)
{
    cultural_lock_streak += 1;
}
if cultural_lock_streak >= cultural_lock_streak_threshold {
    return Some(CollapseReason::CulturalLock);
}
```

`dogmatism` is the L2-norm of the cosmology vector divided by
`sqrt(5)` ([`cosmology.rs:42`](../sim/civ/src/cosmology.rs)); a
fully-aligned cosmology (every axis pinned at ±1) reads 1.0. The
`last_refinement_tick` advances when any `RefinementConfirmed`
event lands, so a civ that keeps revising its laws never
accumulates the streak even at high dogmatism.

Substrate-aware: `streak_ticks_for_metabolism` rescales the
threshold so silicate civs at `metabolism = 0.2` get 5× longer
windows ([`lifecycle.rs:137`](../sim/civ/src/lifecycle.rs)).

Pairs with the cosmology-suppression hook
(`culture_hooks::suppress_confidence_for`) — a dogmatic civ finds
heretical refinements multiplicatively harder to confirm, so
`last_refinement_tick` stagnates, the streak climbs, and the civ
collapses under its own orthodoxy. The mechanism encodes the
real-history pattern: civilisations that calcify their canon
stop generating new science and eventually break under
unaccommodated change.
