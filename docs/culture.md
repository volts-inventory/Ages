# Culture

Two-layer cultural model: a slow-drift species-anchored
**cosmology** and a fast-divergent civ-keyed **religion**. Plus
the transmission machinery that ferries knowledge across collapse
boundaries (where comprehension is gated by linguistic distance,
age decay, communication-channel modality speed, and tool
fidelity bonuses).

For deeper detail per crate, see
[`sim/civ/src/cosmology.rs`](../sim/civ/src/cosmology.rs),
[`sim/civ/src/religion.rs`](../sim/civ/src/religion.rs),
[`sim/civ/src/transmission.rs`](../sim/civ/src/transmission.rs),
and [`sim/civ/README.md`](../sim/civ/README.md). The kinship +
war machinery that reads these vectors lives in
[`sim/civ/src/conflict/`](../sim/civ/src/conflict/) — covered in
[civ.md#conflict-war--alliance--grudge--assessment](civ.md#conflict-war--alliance--grudge--assessment).

## Two layers, two timescales

| Layer | Scope | Drift speed | Drives |
|-------|-------|-------------|--------|
| **Cosmology** | Species-anchored (every civ inherits species' `initial_cosmology`) | Slow (×1 base) | Hypothesizer form preferences, cultural-lock collapse, alliance proximity, dogmatism |
| **Religion** | Civ-keyed jitter at founding | Fast (×3 base) | Kinship (dominant 0.60 weight), schism dynamics, hierarchical-strength bonus |

Both layers drift on the same hypothesis-engagement events but at
different magnitudes. Religion is the layer that actually drives
intra-species war via the kinship weighting; cosmology drives
the deeper worldview that constrains what counts as plausible
science.

## Cosmology — five slow-drift axes

The cosmology vector is the **deep worldview** layer. The
`Cosmology` struct lives at
[`sim/civ/src/cosmology.rs:11`](../sim/civ/src/cosmology.rs):

| Field | Low pole (−1) | High pole (+1) |
|-------|---------------|----------------|
| `empirical` | Mystical / revelation | Empirical / measurement |
| `communitarian` | Individualist | Communitarian |
| `reformist` | Dogmatic / canonical | Reformist / open to revision |
| `mystical` | Mechanistic | Mystical |
| `hierarchical` | Egalitarian | Hierarchical |

(`empirical` and `mystical` are paired but not strictly opposite
— a civ can be high on both, modelling religious empiricism /
sacred natural philosophy.)

### Magnitude, dogmatism, distance

```rust
magnitude() = sqrt(sum of squared axes)        // L2 norm
dogmatism() = (magnitude / sqrt(5)).clamp01()  // [0, 1]
distance_to(other) = L2 distance over 5 axes
```

`dogmatism` reaches 1.0 when every axis pins at ±1; it's the
input to the cultural-lock collapse trigger
(`CULTURAL_LOCK_DOGMA = 0.85`, see
[civ.md#cultural-lock](civ.md#cultural-lock)).

### Per-event push tables

`push_for_*` functions in
[`cosmology.rs:96-144`](../sim/civ/src/cosmology.rs) define how
each hypothesis-engagement event nudges the vector:

| Event | empirical | communitarian | reformist | mystical | hierarchical |
|-------|-----------|---------------|-----------|----------|--------------|
| `RelationConfirmed` | +0.025 | 0 | +0.010 | −0.020 | 0 |
| `RefinementProposed` | +0.010 | 0 | +0.020 | 0 | 0 |
| `RefinementConfirmed` | +0.020 | 0 | +0.030 | −0.010 | 0 |
| `RefinementRejected` | −0.010 | 0 | −0.020 | +0.020 | +0.010 |
| `CivCollapsed` | 0 | +0.050 | −0.050 | +0.075 | +0.025 |

Magnitudes were halved when religion took over the fast-cultural
role — cosmology is now the slow-drift deep-worldview layer.

### Emission gate

`COSMOLOGY_EMIT_THRESHOLD = 0.50` at
[`cosmology.rs:153`](../sim/civ/src/cosmology.rs). Re-emit
`CosmologyShifted` only when the L2 distance from the last
emitted snapshot ≥ 0.50 — keeps cosmology events near-millennium-
rare rather than firing every few centuries.

### Form distance (suppress_confidence input)

`form_distance(form, cosmology)`
([`cosmology.rs:218`](../sim/civ/src/cosmology.rs)) returns the
distance `[0, 1]` between a fit form and the civ's cosmology
preference. Higher = more heretical = stronger suppression of
that form's confidence during fitting. Per-form match-arm
preferences:

- `Linear` is universally easy (distance = 0).
- `ThresholdStep` rewards empirical pole.
- `PeriodicSine` rewards mystical pole.
- `Logistic` and `PowerLaw` reward reformist pole.
- `ExpDecay/ExpGrowth` reward both empirical and reformist
  (history-as-progress flavour).
- `Logarithmic`/`InverseSquare` reward empirical pole.
- `Polynomial2/3` rewards averaged reformist+empirical (case-
  specific empirical fits).

`combined_form_distance(form, cosmology, religion)` averages the
cosmology distance with the religion-derived distance below.

## Religion — three fast-divergent axes

The religion vector is the **fast-divergent cultural** layer that
real history actually shows over centuries (Catholic vs.
Orthodox, Sunni vs. Shia, Theravada vs. Mahayana). Civs founded
from a common ancestor can diverge religiously within
generations even while sharing cosmology.

The `Religion` struct lives at
[`sim/civ/src/religion.rs:29`](../sim/civ/src/religion.rs):

| Axis | Low pole (−1) | High pole (+1) |
|------|---------------|----------------|
| `theology` | Monist / one-truth | Pluralist / many-spirits |
| `ritual` | Pragmatic / ad-hoc | Liturgical / formal |
| `sacred_time` | Cyclical / eternal-return | Eschatological / arc-toward-end |

### Founding-time religion

`founding_religion(civ_id, charisma, doubt, curiosity)` at
[`religion.rs:113`](../sim/civ/src/religion.rs) is built from:

1. **Per-civ jitter** — deterministic ±0.20 splitmix-style hash
   on `civ_id`. Same `civ_id` always lands at the same jitter;
   different civs of the same species land on distinct vectors.
2. **Figure-trait offsets** — the founding figure's traits push
   one axis each:
   - `theology = −0.5·doubt + jitter` (high doubt → toward
     monism: there must be one truth)
   - `ritual = +0.5·charisma + jitter` (charismatic founder →
     formal liturgy)
   - `sacred_time = −0.3·curiosity + jitter` (curious founder →
     look to past for patterns, cyclical)

The jitter + figure offset push two civs of the same species
onto distinct religion vectors at birth — mimicking how real
religious traditions diverge from founding-figure personality +
early-event chance even within one species.

### Per-event push tables

Religion's `push_for_*` functions in
[`religion.rs:160-207`](../sim/civ/src/religion.rs) mirror
cosmology's but with 3× the magnitude (religion is *meant* to be
volatile):

| Event | theology | ritual | sacred_time |
|-------|----------|--------|-------------|
| `RelationConfirmed` | −0.15 | 0 | +0.10 |
| `RefinementProposed` | 0 | −0.08 | +0.06 |
| `RefinementConfirmed` | −0.08 | −0.12 | +0.10 |
| `RefinementRejected` | +0.10 | +0.15 | −0.08 |
| `CivCollapsed` | +0.25 | +0.30 | +0.40 |

Science accumulates → theology toward monism (one truth), sacred
time toward eschatological (history-as-progress). Rejection
defends orthodoxy → ritual toward liturgical, theology toward
pluralism (older spirits-and-omens view), sacred time toward
cyclical (return to tradition). Collapse → surviving founders
attribute it to gods/punishment, strong push toward pluralism +
liturgy + eschatological "end-times" framing.

### Emission gate

`RELIGION_EMIT_THRESHOLD = 0.20` at
[`religion.rs:214`](../sim/civ/src/religion.rs). Lower than
cosmology's 0.50 because religion is supposed to be faster-moving
and we want every meaningful schism on the wire.

### Religion in form-distance

`religion_form_distance(form, religion)` at
[`cosmology.rs:175`](../sim/civ/src/cosmology.rs) wires religion
into the same suppress-confidence pipeline cosmology uses:

- `theology`: monist (−1) likes unifying / single-law forms
  (`Constant`, `Linear`, `PowerLaw`); pluralist (+1) tolerates
  case-specific forms (`Polynomial`, `ThresholdStep`).
- `ritual`: liturgical (+1) prefers procedural / step semantics
  (`ThresholdStep`, `Logistic`); pragmatic (−1) prefers smooth
  continuous (`Linear`, `Polynomial`).
- `sacred_time`: cyclical (−1) embraces periodic (`PeriodicSine`);
  eschatological (+1) prefers one-way change (`ExpGrowth`,
  `ExpDecay`).

The `combined_form_distance` function averages cosmology and
religion contributions (each capped at 1.0; average is also in
`[0, 1]`) so both epistemic layers shape what a civ considers
plausible.

### Religion in war strength

`hierarchical_strength`
([`sim/civ/src/conflict/war.rs:70`](../sim/civ/src/conflict/war.rs))
reads `|theology| + |ritual|` magnitudes (sacred_time excluded —
it's a temporal-cycle axis, less tied to chain-of-command) as
the religion contribution to the 4-channel composite. High-
ritual, high-theology civs project authority through religious
institutions — the priest-king + state-religion archetype. See
[civ.md](civ.md#war-resolution) for the full composite formula.

## Cosmology + religion in collapse / breakaway

The collapse-driven push tables above mean both layers shift
toward more mystical / liturgical / eschatological framing after
a civ falls. A successor civ that takes over a stateless cohort
inherits the species' cosmology (the layer that drifted slowly
across the parent's lifetime) and gets a fresh founding-figure-
driven religion vector — so the cultural break across collapse
is sharp on the religion axis but continuous on cosmology.

The **dogmatic breakaway** path forks a civ when its cosmology
dogmatism is high enough that a heretical hypothesis force-
rejection triggers a fork centered on the heretical view (see
[civ.md#breakaway](civ.md#breakaway)).

## Transmission — knowledge across collapse

`transmission::transmit_from_parent` at
[`sim/civ/src/transmission.rs:252`](../sim/civ/src/transmission.rs)
runs the across-collapse path. When a successor civ founds, a
fraction of the predecessor's confirmed relations transmit into
the successor's knowledge — gated by a multiplicative
comprehension score.

### Comprehension score

```text
comprehension = linguistic_factor × age_decay × tier × cultural × comm_speed × ...
```

The base function (`comprehension`) in
[`transmission.rs:86`](../sim/civ/src/transmission.rs):

```rust
ling = (1 − linguistic_distance).max(0)
age  = exp(−age_ticks / decay_ticks)     // per-species decay constant
tier = TIER_FACTOR = 0.7                 // flat scalar
cult = 1.0                                // placeholder until cultural distance wires in
```

Linguistic distance is Jaccard distance over the two civs'
`NameGrammar` atom sets
([`transmission.rs:54`](../sim/civ/src/transmission.rs)) —
disjoint strategies (acoustic vs. chemical) read distance = 1
(zero overlap), same-strategy + same atoms reads 0.

`age_decay` is `exp(−age / decay)` where the decay constant is
per-species (`Species::transmission_decay_ticks`) so long-lived
social species preserve oral tradition longer. The default
`DECAY_CONSTANT_TICKS = 1000 × MONTHS_PER_YEAR` (e-fold over
1000 sim-years).

### Per-civ multipliers

`transmit_from_parent` adds three more multiplicative terms on
top of `comprehension` before clamping at 1.0:

1. **Communicativeness boost** — `1 + 0.3 × max(communicativeness)`
   across the parent's figures. A parent whose canon was carried
   by figures with strong narrative voices passes more across the
   boundary.
2. **Settlement persistence** — `parent.settlement_persistence_multiplier()`
   buckets the parent's `peak_claimed_cells` into a 4-tier
   ladder (capital/town/village/hamlet) ranging from 0.85 (1
   cell) up to 1.30 (16+ cells). A civ that grew large at peak
   left distributed archives across its territory.
3. **Tool transmission fidelity** — `1 + parent.tool_transmission_fidelity_bonus()`
   capped at ×1.5. Tools like `CulturalEncoding` (writing),
   `WrittenJurisprudence`, `AbstractMathematics`, `MassLiteracy`,
   `LongRangeCommunication`, `InformationNetworking`,
   `DigitalComputation` lift comprehension multiplicatively.

### Comm-channel modality transmission speed

`comm_speed` is the species' aggregate communication-channel
transmission-speed multiplier in `[0.1, 1.0]`, computed from
`CognitionTopology::transmission_speed_for_modality` at
[`sim/species/src/types.rs:763`](../sim/species/src/types.rs):

| Modality | Speed | Note |
|----------|-------|------|
| AcousticAir, AcousticWater, VisualLight, VisualPolarization, RadioNative | 1.0 | Fast-propagating long-range |
| Bioluminescent | 0.8 | Fast but line-of-sight |
| Seismic / VibrationalMechanical | 0.7 | Short-range mechanical |
| ChemicalPheromone, ChemicalTaste | 0.2 | Slow diffusion |
| Tactile | 0.1 | Short-range and slow |

The species' aggregate is the **max** modality speed over its
communication channels (see
`Species::communication_speed_multiplier` in
[`sim/species/src/species.rs:233`](../sim/species/src/species.rs)).
Folds into the comprehension score multiplicatively so a
chemical-pheromone species recovers less of its predecessor's
knowledge per unit-time than an acoustic species — a society
whose only comms are slow-diffusing pheromones loses canon at a
rate the parchment-and-ink seeds simply don't.

### Gating threshold + mythologization

`TRANSMIT_THRESHOLD = 0.15` at
[`transmission.rs:29`](../sim/civ/src/transmission.rs). Above
threshold, the relation transfers with confidence scaled by
comprehension; below threshold, two outcomes:

- **Mythologized** if `MYTH_FLOOR = 0.03 < score ≤ TRANSMIT_THRESHOLD`.
  The relation doesn't transfer as confirmed knowledge but
  perturbs the successor's cosmology along one of the five axes
  by `MYTH_PUSH_BASE × (1 − score) = 0.05 × (1 − score)`. The
  more lost the transmission, the more its residue distorts the
  receiving civ's worldview.
- **Lost** if `score ≤ MYTH_FLOOR`. No trace.

The mythologization-axis pick (`mythologization_axis` at
[`transmission.rs:135`](../sim/civ/src/transmission.rs)) is a
deterministic hash on `(template_id, relation_id, source_civ_id)`
with a 60% mystical bias (axis 3) and 40% spread across the
other four axes. Lost-meaning transmissions historically become
sacred / ineffable, so mystical is the modal default.

`MythologizationRecord` carries the source relation id, the
axis, the magnitude, and the comprehension score — feeding the
post-run report's narrative ("relation X comprehended at 0.08 —
mythologized rather than lost"). Real historical analogue: a
society that lost the original physics of a phenomenon may retain
the *meaning* (taboo, ritual, sacred reverence) without the form.

### Inheritance-window revalidation

Transmitted relations enter the successor's `confirmed` registry
tagged with:

- `inherited_from_tick = transmitted_at_tick`
- `inherited_from_civ_id = parent.id`
- `falsification_streak = 0` (drift state reset — predecessor's
  doesn't bias successor's tracking)
- `low_confidence_streak = 0`
- `confidence *= comprehension` (partial recovery)

After `REVALIDATION_WINDOW_TICKS = 50` (set in
[`sim/civ/src/discovery/hypothesizer.rs:40`](../sim/civ/src/discovery/hypothesizer.rs)),
the successor's hypothesizer re-evaluates the inherited fit on
its own samples. Passes graduate to native status; failures emit
`RelationLapsed` and drop the relation.

## Concurrent-civ peaceful diffusion

`transmission::diffuse_between`
([`transmission.rs:163`](../sim/civ/src/transmission.rs)) is the
cross-civ live-contact version. Two concurrent civs that pass
`conflict::is_peaceful_pair` (both hierarchical-axis below
`PEACEFUL_HIERARCHY_FLOOR = 0.40`) each tick exchange a fraction
of their confirmed relations:

- Same Jaccard-distance linguistic-factor + flat tier as
  cross-collapse comprehension.
- **No age decay** — live contact, not artifacts in transit.
- Source civ's `tool_transmission_fidelity_bonus` still
  applies, capped at ×1.5.
- `comm_speed` multiplier still folds in.

Skips relations the destination has already confirmed (direct
contact doesn't overwrite local knowledge). Same
`TRANSMIT_THRESHOLD = 0.15` gates the transfer.

## Kinship (read by belligerence)

Kinship between two civs is a weighted closeness across four
channels (see
[`sim/civ/src/conflict/assessment.rs:188`](../sim/civ/src/conflict/assessment.rs)):

| Weight | Channel |
|--------|---------|
| `KINSHIP_W_HIER = 0.10` | Hierarchical cosmology axis |
| `KINSHIP_W_COSMO = 0.15` | The four non-hierarchical cosmology axes (averaged) |
| `KINSHIP_W_TECH = 0.15` | Literacy |
| `KINSHIP_W_RELIGION = 0.60` | Three-axis religion vector |

Religion dominates (0.60) so the kinship lever survives in
single-species runs where cosmology stays clustered around the
species' anchor. The base score is attenuated by two M6-era
modifiers:

1. **Generation closeness** — `exp(−|gen_a − gen_b| / GENERATION_KIN_DECAY_GENERATIONS)`
   with `GENERATION_KIN_DECAY_GENERATIONS = 8`. Same-species
   cousins 8 lineages apart get ~0.37× the base kinship —
   restoring the Italian-republics dynamic (same religion, same
   lineage stem, but enough generational drift to make them fight).
2. **War-history grudge** — the per-pair grudge accumulator
   (asymmetric; loser holds longer) subtracted from kinship.
   Decays lazily; a pair that's been at war for decades reads as
   fundamentally hostile in kinship space even if religion has
   re-converged. See
   [civ.md#grudge](civ.md#grudge).

Kinship feeds belligerence as a multiplicative dampener: see
[civ.md#belligerence-assessment](civ.md#belligerence-assessment)
for the full pipeline.

## Cultural events

| Event | Trigger |
|-------|---------|
| `CosmologyShifted(axis, signed_magnitude)` | L2 drift past `COSMOLOGY_EMIT_THRESHOLD = 0.50` |
| `ReligionShifted(axis, signed_magnitude)` | L2 drift past `RELIGION_EMIT_THRESHOLD = 0.20` |
| `WarDeclared(civ_a, civ_b, belligerence)` | Pair crosses `WAR_DECLARE_THRESHOLD = 0.25` |
| `PeaceConcluded(civ_a, civ_b, ticks_elapsed)` | Pair drops below `WAR_END_THRESHOLD = 0.15` |
| `ConflictResolved(civ_a, civ_b, cell, ...)` | Per-cell skirmish outcome; aggregated into war campaigns by post-run report |
| `AllianceFormed(civ_a, civ_b)` | `propose_alliance` greenlights all five cumulative conditions |
| `AllianceDissolved(civ_a, civ_b, reason)` | Drift / trust / war misalignment |
| `RelationTransmitted(...)` | Transmission score cleared `TRANSMIT_THRESHOLD` |
| `RelationMythologized(...)` | Transmission score landed in `(MYTH_FLOOR, TRANSMIT_THRESHOLD]` |
| `RelationLapsed(...)` | Inherited relation failed revalidation |

## Multi-tick wars

Border conflicts resolve cell-by-cell across multiple skirmish
events. Each cell flips when the loser cohort's per-cell
population crosses `CELL_FLIP_FLOOR = CONFLICT_DEFEAT_FLOOR / 2 = 25`.
A per-skirmish `loss_frac` ceiling caps single-event population
loss at 60% (composed from
`CONFLICT_MIN_LOSS + hierarchy_bonus + tech_gap`, see
[civ.md#war-resolution](civ.md#war-resolution)).

The post-run report groups consecutive `ConflictResolved` events
between the same pair into "war campaigns" with start/end years,
peak loss percentage, and final outcome — see
[report.md](report.md). The viewport's per-tick log dedupes by
`(loser, winner)` pair so a 200-skirmish war surfaces once per
pair instead of flooding the log.

## Cosmology + religion in transmission

Inter-civ knowledge transmission has a mid-comprehension band
that doesn't transfer the relation content but instead nudges the
receiving civ's cosmology along an axis aligned with the
relation's themes (the mythologization path above). A society
that lost the original physics retains the cultural shadow.

See [discovery.md#transmission-and-mythologization](discovery.md#transmission-and-mythologization)
for the discovery-side narrative wrap.
