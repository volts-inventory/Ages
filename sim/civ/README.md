# sim/civ

The civ entity and its lifecycle. **Species is the run's persistent
unit**; civilizations are bounded collectivities within the species
that found, run their course, and often collapse. Multiple civs may
exist concurrently or sequentially; the species continues.

## Status

- **M2 shipped**: single-civ scaffold with per-civ cohort, simple
  birth/death dynamics, observation pool that folds recognition
  firings into per-template counts.
- **M3 in progress**: species lands in `sim/species`; the run loop
  now filters cohort observations through `Species::perceivable_firings`
  so a civ sees only firings whose channels intersect the species'
  modalities. Functional-form fitting and confidence scoring also
  land here:
  - `forms` — 12-form vocabulary with `param_count`, per-form
    minimum-sample floor, base tolerance, snake-case tag, and
    `evaluate(params, x)`. `available_forms()` is the
    architectural seam (returns all 12 at T0; sensorium / tech-tier
    gating is layered on top).
  - `fit` — RMSE metric, tolerance / confidence, closed-form
    least squares for the linear-in-parameters forms, log-
    linearised fits for `ExpDecay` / `ExpGrowth` / `PowerLaw`,
    threshold search for `ThresholdStep`. Iterative-fit forms
    (`Polynomial2/3`, `PeriodicSine`, `Logistic`) return `None`
    for now; iterative-fit pass is a follow-up.
  - `discovery::Hypothesizer` — civ-scoped sample collection for
    a fixed set of `(template, channel)` candidate relations,
    per-channel scaling so wide-range channels stay inside Q32.32,
    periodic fits in Occam priority order, confirmation when
    a fit clears `exp(-1)`. M3 attributes confirmations to
    `figure_id = 0`; named figures plug into the same lifecycle
    later.
  - Refinement lifecycle — per confirmed relation,
    `Hypothesizer::step` measures sustained-low-confidence streaks
    against the active form (`exp(-2)` trigger), runs Occam-adjusted
    candidate selection (`score = confidence − λ × param_count`,
    propose if `score_best > score_current + switch_margin`),
    runs a 200-tick probation window, enforces a 100-tick cooldown
    on rejection. Surfaces as `HypothesisEvent::RefinementProposed
    / RefinementConfirmed / RefinementRejected`; sim/core maps these
    to the protocol's three refinement events.

  - `figures::NamedFigure` + `figures::PhonemeGrammar` — named
    figures and the per-civ phoneme grammar. A founding civ ships
    with a 2–3 figure band; the grammar samples a 4–7 consonant +
    2–4 vowel inventory deterministically from `(civ_id,
    species_seed)`, and figure names are CV syllable strings drawn
    from it. Hypothesis events route through `figures::attribute`
    so each relation stably attributes to an active figure.
  - `tech::ToolKind` — sensorium-extending tools. 8 categories
    across tiers 2–5: `distance_imaging`, `remote_acoustic`,
    `field_sensor`, `thermal_sensor`, `magnetic_sensor` (tiers 2–4,
    sensorium extensions), plus three tier-5 transcendence-tier
    tools: `bioelectric_resonator` (engineering on the species'
    own field signatures), `field_propulsion_engine`,
    `metamaterial_lattice`. Each tool carries `(prereq_channels,
    manipulation_prereqs, granted_channels, tier, crust_prereqs,
    min_civ_confirmed_relations, min_civ_experimental_relations,
    literacy_floor, species_maturity_floor, relation_prereqs,
    tool_prereqs, resource_prereqs)`. Tools unlock when the
    species + planet prereqs hold AND the civ has fit at least
    `min_civ_confirmed_relations` confirmed relations of its own
    AND at least `min_civ_experimental_relations` of those came
    through `ExperimentApparatus` (intervention-supported
    epistemology, not passive observation) AND civ literacy clears
    its floor AND every `(template_id, _)` in `relation_prereqs`
    has a matching confirmed relation AND every tool in
    `tool_prereqs` is already unlocked AND (tier-5 only) the
    species has accumulated `species_maturity_floor` confirmed
    relations across all civs. `Civ::apply_tool_unlock` unions
    granted channels into `Civ::unlocked_channels`, marks newly-
    perceivable templates, and refreshes the hypothesizer's
    available form vocab. Tier-5 tools grant no perceptual
    channels — they are narrative milestones (the
    "consciousness-coupling" / "field-mediated propulsion" /
    "atomic-precision metamaterial" capabilities the project
    vision describes for late-game civilizations). Species
    without `ToolExtension` are permanently locked out — the
    central mechanism by which non-tool-using species reach
    genuinely different sciences.

  - **Experiment apparatus** — `apparatus::Apparatus { cell,
    clamp_channel, measure_channel }` records on `Civ::apparatus_cells`.
    Tier-2 `ToolKind::ExperimentApparatus` unlocks one apparatus
    per civ; `apparatus::write_apparatus_clamps` writes the
    `tick % 4` ladder value into physics state pre-integration;
    `apparatus::record_apparatus_samples` reads the apparatus
    cell's post-physics response and feeds `(clamp, response)`
    into the first active figure's hypothesizer via
    `Hypothesizer::record_experimental_measurement`. The 2× sample
    weighting + `experimental_count_by_relation` sidecar marks
    every relation that received apparatus contributions, surfacing
    on `ConfirmedMeasurement.is_experimental` and the protocol
    event flag of the same name. Tool gate is per-tool
    `manipulation_prereqs` (the apparatus accepts every
    `ManipulationKind` — a clamp-and-measure rig is a function,
    not a body-plan-specific form) + literacy 0.30 + obs pressure
    30k + confirmed `fire` so any species that has done enough
    science moves from observation-only to controlled-conditions
    intervention through its native manipulation affordance.
  - **Transcendence run-end** — fires when the species has
    sustained at-least-one-civ-with-all-tier-5-tools for
    `TRANSCENDENCE_SUSTAINED_TICKS` (currently 2000 ticks).
    Combined with the tier-5 species-maturity floor of 3000
    cumulative relations, transcendence naturally lands at year
    9000+ on substrate-aligned seeds — emergent from accumulated
    state, not authored progression.

  - **`catastrophe::CatastropheKind`** — five kinds:
    `Volcanic` (extreme charge × temperature signature),
    `Disease` (overcrowding + civ-age), `Asteroid` (rare
    deterministic-prime firing window, 40% pop loss, mystical
    push), `SolarFlare` (high stellar luminosity + weak/none
    magnetosphere, 10% loss, empirical push), `IceAge` (cold
    planet + civ-maturity gate, 20% loss, communitarian/
    hierarchical push). Each pivots cosmology on fire.

  - **Culture-axis behavioural wiring** — taboo, focus, and
    cosmology-suppression hooks all consume the civ's cosmology
    vector at the discovery pipeline. Cosmology suppression cuts
    fit confidence on heretical forms (dogmatic civs). Focus
    boosts fit confidence on empirical/reformist civs. Taboo
    attenuates `firings_by_template` accumulation on high-mystical
    civs via a deterministic per-(tick, civ, template, cell) hash
    bucket — no RNG threading. Together the cosmology vector
    drives *which* civs confirm *what*, not just whether
    confirmation gates open.

  - **Per-figure personality** — each `NamedFigure`
    carries `charisma`, `curiosity`, `doubt`, `communicativeness`
    scalars in `[0, 1]`. `doubt` scales the per-figure refinement
    `switch_margin` (high-doubt figures revise theories sooner);
    `communicativeness` scales the inter-civ transmission
    comprehension boost when this figure's civ transmits to a
    successor.
  - `forms::derive_available_forms` — form availability
    derivation. Returns `{Constant, Linear}` plus the union of
    `forms_for_tag` over the structural tags carried by every
    template the civ currently perceives. `Civ::refresh_available_forms`
    pushes the result into the hypothesizer.
  - `culture_hooks` — three M3-stub influences:
    `allow_observation` (taboo gating), `focus_weight`
    (priority redirection), `suppress_confidence`
    (cosmology-mismatched slowdown). All no-op pass-through
    in M3; M4 wires real numbers.

  Still pending in this crate: knowledge graphs per figure +
  population-driven figure-roster growth.
- **M4 pending**: full civ lifecycle (founding triggers, collapse
  triggers, succession), inter-civ knowledge transmission with
  comprehension decay, T1+ artifact persistence across civ
  boundaries.

## Founding (M4)

A new civ founds when a population pocket meets all of:

- **Critical mass** — settlement-scale population (~100 people) in
  contiguous regions.
- **Triggering condition** — at least one of: a charismatic candidate
  (named figure with high charisma), a surviving institutional
  remnant from a recently collapsed civ, an environmental reset
  (post-catastrophe colonisation), migration into uncolonised
  territory.
- **Cooldown** — typically a gap since the previous civ collapsed in
  this region (or immediate, if the trigger is a remnant or
  charismatic founder).

A founded civ inherits species traits, a phoneme grammar (sampled at
founding, biased toward accessible predecessor grammars in the
region), inherited cultural-memory tokens (oral tradition decays
~1 generation without persistence tech, plus comprehensible artefacts
and surviving named figures), and an empty knowledge graph except
where inheritance fills it.

## Collapse (M4)

A civ collapses when one or more triggers fire: population in
territory drops below a sustaining threshold, cultural-lock (focus
priorities zeroed and cosmology suppression high for many generations
→ no new discoveries, institutional failure), external conquest by
another civ (M5), knowledge plateau combined with migration drain.

On collapse:

- The civ-level knowledge graph dissolves.
- Named figures who outlive the collapse and remain in the territory
  may join successor civs; emigrants take their personal graphs to
  whichever concurrent civ they join (M5); the rest die out with
  their knowledge.
- All artifacts at persistence tier T1+ remain in their regions —
  physical objects in the world, owned by no civ, available for
  whatever civ next inhabits the region.
- Population in the civ's territory becomes `stateless` — still part
  of the species, no civ membership.

## Inter-civ knowledge transmission (M4)

Successor civs encounter inherited artifacts via direct encounter,
migration, or place-name and myth inheritance through stateless
populations. Comprehension is gated by:

- Linguistic distance between authoring and reading civ phoneme
  grammars.
- Cultural-cosmology distance.
- Time elapsed since artifact creation (older = more comprehension
  decay).
- Persistence tier of the artifact: T1 (rough scratches) is harder
  than T2 (codified system), which is harder than T3 (mass-distributed
  copies with multiple corroborating instances).

Comprehending an artifact reveals some fraction of the relations
encoded in it. Quipu unreadable to non-knot civs; ice-cuneiform
unreadable to civs without ice; songlines unreadable to civs that
don't walk the route. Recovery is partial and slow — a real
"dark age, then renaissance" arc.

## Succession dynamics

After collapse, the region's stateless population is a candidate pool
for new founding (per criteria above), a reservoir of slowly-decaying
oral cultural memory (T0 only), and a target for migration from
concurrent civs (M5). A region can host multiple successor civs over
time, gaps short (immediate succession from a remnant) or long
(centuries of stateless population before re-founding).

## Run-end is species-level, not civ-level

Civ collapse is **not** a run-end. The species persists, knowledge
artifacts persist, successor civs can emerge. Run-end conditions are
species-level (extinction, transcendence, user-stop, fixed horizon).

## Cited by

[docs/civ.md](../../docs/civ.md),
[docs/discovery.md](../../docs/discovery.md),
[docs/tech.md](../../docs/tech.md),
[docs/culture.md](../../docs/culture.md).
