# Ages — Requirements Specification

A high-level, implementation-independent specification of *intended*
functionality, written so the project can be rebuilt cleanly in a fresh
repository. It describes **what the system should do** — the behaviors,
entities, and relationships — not how the prior prototype calculated them.

The prototype's specific formulas, constants, thresholds, and coupling orders
are deliberately **excluded**: that logic is known to be flawed or half-wired,
and a rebuild should derive its own numbers from first principles. Where a
number appears below it is structural (e.g. "one species per planet", "one tick
= one month"), not a tuning constant to be copied.

Requirements use "shall" for hard requirements.

---

## 1. Vision

Ages is a deterministic, headless simulator that writes **the biography of an
alien species across thousands of years on a procedurally generated planet**.

Given a seed, it:

1. Samples a complete, internally consistent planet.
2. Steps real physics on a grid.
3. Evolves a single species fitted to that planet's niche.
4. Lets civilizations rise, expand, contact, trade, war, discover the physics
   of their world, collapse, and seed successors — all within that one species,
   across thousands of years.
5. Emits every structural transition as a structured event stream, then renders
   a written history.

**No LLM. No API keys. No network. The same seed always produces the same
history, exactly.** The audience is people who enjoy emergent worlds,
alternative-physics toys, replayable seeds, and reading a story a computer made
on its own.

---

## 2. Guiding principles

Every feature shall trace to at least one:

1. **Physical / biological grounding over game-mechanic abstraction.** Civs
   derive knowledge from simulated physics; species evolve to fit niches;
   derive rather than invent wherever possible.
2. **Emergence over authoring.** Phenomena emerge from physical conditions;
   paradigms from physics plus observation order; civs from population plus
   triggers; developmental paths from world plus species plus trajectory.
3. **The species is the protagonist; civs are episodes within its history.**
   Successive civilizations within one species are first-class.
4. **Quantitative depth, not tokens.** Discoveries are fitted functional forms
   with real parameters. Wrong hypotheses are first-class. Refinement is
   open-ended.
5. **No hand-holding output, no LLM.** Outputs are structured: an event log,
   periodic snapshots, a live view, a written report, and optional prose
   narration.
6. **Determinism as a contract.** The same inputs always produce the same
   history. All simulation arithmetic and randomness shall be reproducible
   across platforms and runs.

---

## 3. System architecture (high level)

### 3.1 Process model

A single headless simulator emits a structured event stream. There is **no
graphical UI and no LLM** anywhere in the pipeline. All consumers — the live
terminal view, the post-run report, the prose narrator — are pure functions of
the event stream and shall never feed back into the simulation.

### 3.2 Determinism contract

1. The same inputs (seed plus any world configuration and grid dimensions)
   shall reproduce an identical run.
2. Simulation arithmetic shall be reproducible across platforms — no reliance
   on platform-dependent floating-point behavior in the decision path.
3. All randomness shall be seeded and reproducible; nothing in the simulation
   loop shall read wall-clock time or an unseeded source.
4. Iteration over collections in decision paths shall be deterministically
   ordered.
5. Output mode, pacing, and rendering shall never alter the canonical event
   stream.
6. The build shall include automated determinism, performance-regression, and
   cross-seed divergence checks.

### 3.3 Time

- **One simulation tick = one month.** Physics integrates in finer sub-steps
  within a tick.
- Year length is **per-planet** (a planet's months-per-year is sampled in a
  bounded range), and drives seasonal cycles and the displayed calendar.
- The run length may be expressed in years (converted via the planet's year
  length) or directly in ticks.

### 3.4 Tick structure

Each tick advances, in a fixed and deterministic order: physics integration →
phenomenon recognition → observation and discovery → capability/tech evaluation
→ population dynamics → civilization lifecycle (founding, collapse, succession,
contact, trade, conflict, knowledge transfer, catastrophes) → cultural drift.
Event emission order shall be determined by this phase order and by sorted
entity identifiers within each phase.

### 3.5 Run-end conditions

A run ends for exactly one reason: species extinction; stagnation (a sustained
dark age with no active civilization); transcendence (a civilization sustains
the highest-tier capabilities long enough); reaching a fixed horizon; or a user
stop. Per-civilization collapse is **not** a run-end condition.

---

## 4. Command-line and output surface

### 4.1 Binaries

- A **simulator** binary that runs a world and writes the event log (and,
  depending on mode, a live stream).
- A **report** binary that renders a saved event log into a written report.

### 4.2 Controls

The simulator shall accept: a world seed; a run length (in years or ticks); an
output path for the event log; a verbosity/view mode selector (quiet, full
stream, highlights only, or a live view); optional grid dimensions; optional
real-time pacing for the live view; a prose-narration mode; an offline
narration-replay mode over a saved log; and an interactive world-builder mode.

### 4.3 Interactive world builder

An optional mode shall let the user hand-author planet-level attributes
(substrate, atmosphere, temperature, gravity, host star, axial tilt, day
length, year length, moon count, magnetosphere, crust mineralogy, biosphere
richness) while leaving map geography determined by the seed. Choosing an
attribute shall keep dependent attributes internally consistent; contradictory
choices shall warn but proceed.

### 4.4 Output channels

1. **Event log** — the canonical, append-only record; always written.
2. **Live stream / view** — the event stream filtered/formatted to the terminal
   as the run proceeds.
3. **Snapshots** — periodic state-digest checkpoints embedded in the event log.
4. **Written report** — produced from a saved event log.
5. **Prose narration** — human-readable per-event prose, live or replayed.

---

## 5. World

### 5.1 Purpose

Sample a complete, internally consistent planet from a seed and materialize a
grid map plus the planet-level state that feeds physics, the per-cell resource
inventory, and phenomenon recognition.

### 5.2 Requirements

1. Sampling shall be **substrate-first**: choose a metabolic chemistry, then
   constrain every other property so the planet supports life of *some* kind.
   **Every seed shall yield a habitable world.**
2. The world shall support a range of chemistries (e.g. water, ammonia,
   hydrocarbon, silicate), each with its own temperature window, solvent, and
   physical constants.
3. The planet shall be sampled, not categorized: properties are continuous
   vectors (e.g. atmospheric and crustal composition) feeding physics and
   recognition, never branched on a planet "type".
4. The system shall sample a host star (spectral class, spectral energy
   distribution, age, lifetime) and evolve its output over time, migrating the
   habitable zone as the star ages.
5. The system shall sample moons and a tidal-locking regime and evolve orbital
   state accordingly.
6. Physical scalars (gravity, escape velocity, density, irradiance) shall be
   **derived** from sampled mass/radius/orbit/star, not independently invented.
7. Terrain shall be generated so that land always exists; per-cell habitability
   shall be derived from terrain and the habitable-zone fit, and shall gate
   where civilizations can settle.
8. Climate bands shall be derived from each world's own temperature
   distribution, so band-relative phenomena fire on any world.
9. Seasonality shall follow from axial tilt and orbital position.
10. The system shall validate across a wide range of real-analog worlds
    (Earth-, Mars-, Venus-, Titan-, ice-moon-, super-Earth-, hot-Jupiter-,
    tidally-locked-, and lava-world-like cases).
11. **Exactly one species per planet**; multi-species and inter-planet contact
    are out of scope.

---

## 6. Physics

### 6.1 Purpose

Evolve deterministic, substrate-relative physical state on the grid, producing
the per-cell fields that phenomenon recognition reads and that conservation
invariants hold against.

### 6.2 Requirements

1. Physics shall integrate as a fixed, deterministic sequence of law families
   each tick. Inter-cell transfers shall conserve the quantity moved.
2. Constants shall be **substrate-relative** (phase boundaries, latent heats,
   thermal properties), not Earth defaults.
3. The model shall include, at minimum: heat transport; fluid flow; a
   hydrological cycle (the substrate's solvent in solid/liquid/gas phases);
   atmospheric circulation and rotation effects; radiation balance with a
   greenhouse response capable of runaway and snowball regimes; magnetism with
   geomagnetic reversals; tides and tidal heating; tectonics with a
   carbon-silicate weathering thermostat; cloud and ice-albedo feedbacks; and
   atmospheric escape.
4. Vacuum/atmosphere-less worlds shall gracefully disable atmosphere-dependent
   laws.
5. The model shall expose additional environmental fields needed for
   alternative-physics "lever" sciences (see §16), kept separate from the core
   laws.
6. Conservation invariants shall be checked in development builds; the model
   shall not silently leak or fabricate conserved quantities.
7. The grid is a sampling resolution over a sphere; physical magnitudes and
   event rates that scale with planet size shall scale accordingly.

---

## 7. Phenomenon recognition

### 7.1 Purpose

Translate emergent physical state into discrete, named phenomena that
civilizations can observe and reason about. There is no authored catalogue of
"what this world knows"; a default template set captures the shapes physics
reliably produces, and new phenomena can emerge.

### 7.2 Requirements

1. A library of phenomenon templates shall match patterns in physical state and
   emit per-cell "firings" each tick.
2. Each template shall declare which sensory channels can natively perceive it
   and a structural form-tag (e.g. threshold, periodic, distance-decay) that
   gates which functional forms a civ can later propose about it.
3. At run start, templates whose physical preconditions never occur on this
   world shall be dropped; survivors shall be split into **perceivable-now**
   (the species can sense them) versus **latent** (physics produces them but the
   species cannot yet sense them).
4. Latent phenomena shall become perceivable when the species/civ gains the
   relevant sense (e.g. via a sensorium-extending tool).
5. A civ that cannot perceive a phenomenon, or whose senses never expose a given
   structural form, shall be unable to discover laws of that shape — different
   sensoria yield structurally different sciences.
6. New phenomenon templates shall be able to **emerge** from confirmed
   civilizational discoveries and persist in the species' canon across
   civilization boundaries.
7. The set of channels a species can perceive, and the set of templates it can
   perceive, shall stay consistent — a single source of truth, with no template
   that is forever unperceivable by construction.

---

## 8. Species

### 8.1 Purpose

The species is the run's persistent protagonist, **derived from the planet**, so
its form reflects the niche the world provides. Its traits drive sensorium,
manipulation, demographics, cognition, ecological role, life cycle, tolerance,
and a baseline worldview.

### 8.2 Requirements

1. A species shall be derived deterministically from the planet; identical
   inputs yield an identical species.
2. Cognition shall be multi-dimensional and shall carry an organizational
   **topology** (e.g. individual/centralized, distributed-redundant, collective/
   eusocial, acentric/substrate-distributed) that affects learning cadence,
   knowledge retention, abstraction ceiling, and isolation behavior.
3. Sensory modalities and manipulation capabilities shall be gated by planet
   conditions (a sub-surface ocean has no vision, an airless world no acoustic
   air sense, etc.); a baseline sense shall always be present so a species can
   perceive *something*.
4. A material-culture manipulation capability shall gate access to higher-tier
   tools and to experimentation.
5. The species shall have a **habitat** that gates which cells it can natively
   settle until it develops the means to cross biomes. The full set of habitats
   (including subterranean and rock-dwelling) shall be reachable from
   appropriate worlds.
6. The species shall have an environmental **tolerance envelope** (temperature,
   acidity, salinity, radiation, pressure) used both as a hard occupancy gate
   and as a graded survival score under stress.
7. Demographic biology (reproductive strategy along an r/K axis, survival rates,
   age structure) shall be derived from traits and habitat.
8. The species shall have a **life-cycle type** (e.g. vertebrate-like,
   semelparous/iteroparous aquatic, insect-like, eusocial with castes,
   plant-like, microbial, modular/colonial). The chosen type shall be assigned
   from the species' biology — not fixed to one type — and shall drive
   population dynamics.
9. The species shall have an **ecological role** within the planet's food web,
   assigned from the full role distribution rather than fixed.
10. The species shall have a baseline **worldview bias** derived from its traits
    and environment, seeding cosmology.
11. Optional traits such as dormancy/cryptobiosis (rare, enabling
    mass-extinction survival) shall be supported.
12. The species persists across civilization collapses; it may slowly **drift**
    across successive civilizations and be biased by catastrophe selection
    pressure.

---

## 9. Population

### 9.1 Purpose

Evolve heterogeneous, spatially distributed, age-structured populations each
tick, with rates derived from the species' biology rather than human defaults.

### 9.2 Requirements

1. Each cell's population shall be an age-structured cohort; only the
   reproductive bracket reproduces and carries full economic/military weight.
2. Population dynamics (births, survival, aging, death) shall derive from the
   species' biology and life-cycle type, and shall respond to food security
   (demand versus a cell's carrying capacity).
3. Carrying capacity shall scale with habitability, season, technology, and
   ecological conditions.
4. Population shall migrate down pressure gradients toward cells with headroom,
   conserving people in transit.
5. Before civilizations exist, a **nomadic** population shall spread across
   habitable cells and seed civilization founding.
6. Catastrophes shall affect specific cells (not whole civilizations wholesale),
   preserving age structure; dormancy-capable survivors shall be able to recover
   over time.
7. Each life-cycle type shall have appropriate dynamics; a mismatch shall
   degrade gracefully rather than fail.

---

## 10. Ecosystem

### 10.1 Purpose

Model a planet-wide biota of multiple interacting, typed organisms within which
the protagonist species lives, so that environment, food supply, and extinction
have ecological grounding.

### 10.2 Requirements

1. The planet shall be populated with multiple typed organisms spanning a
   trophic web (producers through apex consumers, plus decomposers, mutualists,
   and parasites), respecting a sensible role distribution.
2. Organisms shall interact through typed pairwise relationships (predation,
   competition, mutualism, parasitism, habitat modification) with
   saturating/sigmoidal functional responses.
3. Energy transfer up trophic levels shall follow an ecological-efficiency
   pyramid appropriate to habitat, emerging from the interaction model rather
   than a corrective cap.
4. The ecosystem shall include a biogeochemical loop returning carbon to the
   atmosphere (respiration, decomposition) and coupling to producer growth.
5. Keystone organisms shall be identifiable from interaction structure.
6. Organisms shall go extinct when sustainably below viability; records shall be
   retained for history.
7. The ecosystem shall support evolutionary change: speciation (via multiple
   triggers) and, for microbial organisms, horizontal gene transfer — both
   producing deterministic, inheritable trait change.
8. Catastrophes shall propagate into the ecosystem, with survival graded by each
   organism's tolerance to local conditions.
9. Mutualist and parasite behavior shall be differentiated by sub-type where it
   meaningfully changes outcomes.

---

## 11. Civilizations

### 11.1 Purpose

A civilization is a bounded, transient society within the persistent species.
Civilizations found, run, collapse, and are succeeded by others that inherit
territory and partial knowledge. Multiple may exist concurrently or
sequentially.

### 11.2 Requirements

1. Civilizations shall found **emergently** when a populated region reaches
   sufficient density and the species has the requisite readiness — not from a
   scripted inaugural civ.
2. Each civilization shall hold territory (claimed cells), a founding cohort,
   named figures, a worldview, a religion, accumulated knowledge, technology,
   and an economy.
3. Each civilization shall carry **drift** off the species baseline; successors
   shall inherit and extend that drift, so a lineage of related civilizations
   forms while the species slowly changes.
4. Each civilization shall track **cohesion**, evolving with size, food
   security, dogmatism, and literacy.
5. Civilizations shall be able to **break away** into successor factions
   (driven by low cohesion or by doctrinal schism), with the faction inheriting
   parent state.
6. A civilization shall **collapse** on any of several conditions (e.g. food
   crisis, knowledge stagnation, doctrinal lock-in, loss of territory, civil
   war, depopulation). Collapse unclaims territory and ends its figures;
   knowledge survives only through the inheritance pathway.
7. After collapse, a successor shall be able to **re-found** from surviving
   population after a dark-age interval, inheriting partial knowledge.
8. Civilizations shall register **contact** when their territories meet; contact
   is a prerequisite for war and for peaceful exchange.
9. Civilizations shall maintain an **economy**: surplus that accumulates above
   subsistence, buffers food crises, strengthens war effort, and is drained by
   war and catastrophe. Peaceful contacted civilizations shall open **trade**
   that smooths surplus until war or collapse closes it.
10. Carrying capacity shall be modulated by an **ecological-resilience** factor
    reflecting the local biosphere's health.

### 11.3 Conflict

1. War shall be driven by a belligerence assessment (population pressure,
   opportunity, relative strength) dampened by kinship, and gated on prior
   contact, with hysteresis between declaring war and concluding peace.
2. War shall resolve incrementally over time, cell by cell, with casualties
   concentrated in the fighting-age bracket and territory flipping when a
   defender's local presence collapses.
3. Relative strength shall compose cultural, religious, social, and economic
   factors, and shall be affected by supply (surplus) and technology.
4. Civilizations shall form and dissolve **alliances** based on cultural and
   religious proximity and trust; mutual alliance shall suppress war.
5. A lazily decaying, asymmetric **grudge** shall let former combatants remain
   hostile, feeding back into kinship.

---

## 12. Culture

### 12.1 Purpose

A two-layer cultural model — a slow-drifting, species-anchored **cosmology**
that shapes which science feels plausible, and a fast-diverging,
civilization-specific **religion** that drives intra-species conflict via
kinship — plus the machinery that carries knowledge across collapse boundaries.

### 12.2 Requirements

1. Each civilization shall carry a multi-axis **cosmology** (worldview:
   empirical↔mystical, individualist↔communitarian, dogmatic↔reformist,
   mechanistic↔mystical, egalitarian↔hierarchical) inherited from the species
   baseline and drifting slowly with experience.
2. Each civilization shall carry a multi-axis **religion** (e.g. monist↔
   pluralist, pragmatic↔liturgical, cyclical↔eschatological) seeded by founding
   figures and diverging quickly.
3. Both layers shall drift in response to scientific successes, failures, and
   collapse — religion faster than cosmology.
4. Cosmology and religion shall **bias which functional forms a civ readily
   confirms** (e.g. mystical worldviews favor cyclical explanations), and a
   strongly dogmatic culture shall resist revision (feeding doctrinal-lock
   collapse).
5. **Kinship** between civilizations shall be computed primarily from religious
   proximity, plus worldview and lineage distance, attenuated by war grudges,
   and shall dampen belligerence.
6. **Knowledge transmission across collapse** shall be gated by a comprehension
   score (linguistic distance, age of knowledge, communication capability,
   settlement persistence, recording technology). High comprehension transfers
   the knowledge (subject to later revalidation); **mid comprehension** fails to
   transfer but leaves a **mythologized cultural shadow** that nudges the
   successor's cosmology; low comprehension is lost.
7. Concurrent peaceful civilizations shall **diffuse** knowledge to each other.

---

## 13. Discovery and science

### 13.1 Purpose

Civilizations derive genuine, quantitative knowledge about their world's physics
by fitting functional forms to what they can observe. Discoveries are fits
against simulated data, **not authored facts**. Wrong hypotheses are
first-class.

### 13.2 Requirements

1. Each named figure within a civilization shall observe perceivable phenomena
   and physical channels and propose hypotheses relating them.
2. The system shall support both **threshold/firing relations** (does a
   phenomenon occur under condition X?) and **continuous measurement relations**
   (how does quantity Y vary with quantity X?), the latter recovering real
   coefficients in physical units.
3. A library of **functional forms** (constant, linear, logarithmic,
   exponential, power-law, inverse-square, threshold-step, polynomial, logistic,
   periodic, …) shall be available, but each civ's usable vocabulary shall be
   gated by the structural forms its perceivable phenomena exhibit.
4. A hypothesis shall be **confirmed** when its fit quality crosses a confidence
   bar appropriate to sample size and the species' intelligence; worldview shall
   bias confirmation.
5. Confirmed relations shall be subject to **predict-and-falsify**: sustained
   misprediction shall force reconsideration.
6. **Refinement** shall be open-ended: a confirmed relation can be challenged by
   a better-fitting alternative on probation, with Occam-style preference for
   simpler forms; competing hypotheses (rivals) shall be retained and able to
   displace the incumbent, so paradigm shifts can emerge.
7. Theories shall be able to nest: a residual left by one relation can spawn a
   child relation, to a bounded depth.
8. Knowledge inherited across collapse shall be **revalidated** against the
   successor's own observations after a settling window; relations that fail to
   re-fit shall lapse and drop.
9. Civilizations with the means shall build **experimental apparatus** that
   clamps a variable and measures the response, feeding the measurement track
   with higher-quality data and marking the resulting relations as
   experimental.
10. The discovery pipeline shall be deterministic.

---

## 14. Technology

### 14.1 Purpose

A dependency graph of tools that unlock from confirmed science, bodily
capability, sensorium, literacy, and resources, plus runtime-emergent tools, all
feeding back into the simulation through a common set of effects.

### 14.2 Requirements

1. Technology shall be organized into **tiers** from stone-age through an
   information age and a narrative transcendence tier, as a strict dependency
   graph (no tool precedes its prerequisites).
2. Tool unlocking shall be **gated** by: confirmed (and, for higher tiers,
   experimental) relations; required sensory channels; bodily manipulation
   capability; literacy; accumulated species-wide scientific maturity; crust
   mineralogy; and local resource availability.
3. **Substrate divergence:** gating shall make sciences genuinely diverge by
   world — e.g. a world that cannot burn things shall be locked out of the
   combustion technology line, and shall reach an industrial age (if at all) by
   a different, non-combustion path. No single developmental path shall be
   privileged.
4. **Serendipitous unlocks** shall occasionally bypass a single missing
   prerequisite, deterministically.
5. Tools shall fold into a common set of **effects** (carrying capacity, food
   security, war strength, catastrophe resistance, literacy, knowledge
   transmission, discovery rate, expansion/migration, cohesion, demographic
   rates, etc.).
6. **Emergent tools** shall be minted at runtime when a civilization
   accumulates a cluster of confirmed relations on one channel, carry effects
   derived from that cluster, and persist in the species' canon.
7. Some tools shall **extend the sensorium** (granting new perceivable
   channels), promoting latent phenomena to perceivable.
8. Resource-consuming tools shall draw down planetary resources
   mass-conservatively, coupling technology back to the environment.
9. The highest tier shall represent narrative milestones (the basis for the
   transcendence run-end), **not** simulated exotic physics — no consciousness
   physics, no faster-than-light.

---

## 15. Catastrophes

### 15.1 Purpose

Localized environmental hazards that punctuate the history, with survival graded
by technology and biology.

### 15.2 Requirements

1. The system shall support several distinct catastrophe kinds (e.g. volcanism,
   disease, asteroid impact, solar flare, ice age), each with its own trigger
   conditions, cadence, scope, and severity.
2. Catastrophes shall strike specific cells/regions and may spread to neighbors.
3. Damage shall be attenuated, in order, by: technology-based resistance; species
   dormancy (diverting survivors into a recoverable dormant pool); and the
   species' tolerance fit to local conditions — so a well-adapted extremophile
   can shrug off what wipes a narrow-niche species.
4. Catastrophes (except purely internal ones such as disease) shall propagate
   into the ecosystem.
5. Catastrophes shall be able to bias the trait distribution of survivors,
   feeding species drift in successor civilizations.
6. Environmental coupling shall modulate hazards (e.g. a strong magnetosphere
   shields against radiation events; magnetic reversals amplify them).

---

## 16. Developmental archetypes

### 16.1 Purpose

A run's developmental path shall **emerge** rather than be authored. Each run is
characterized along an open space of peer "levers" (foundational energy/science
bases — combustion, field-resonance, biochemical, cryogenic, mechanical,
hydraulic, exotic-chemistry, plasma/EM, gravitational, photonic, nuclear, …),
with no privileged default.

### 16.2 Requirements

1. Each lever shall be scored from a world-plus-species **prior** and refined by
   the **realized** run trajectory (the channels a civ confirms relations on and
   the tool lines it unlocks).
2. The run shall be **classified** as a pure archetype (one dominant lever), a
   named hybrid (two co-dominant), or a signature-named emergent archetype (a
   novel mix) — so paths nobody anticipated are still detected and named, never
   collapsed onto the nearest familiar one.
3. An orthogonal **cognition overlay** (individual, collective, substrate-
   distributed) shall sit on any lever.
4. Classification shall be deterministic and stable across replays.
5. At transcendence, each archetype shall reach a **divergent endpoint** rather
   than one shared singularity; the cognition overlay shall color that endpoint.
6. Levers shall, where modeled, carry dedicated environmental fields,
   recognition templates, and discovery channels so an attuned species does
   genuine science on that basis. (The intent is for all levers to reach this
   depth over time.)
7. Archetype derivation (at run start) and the endpoint (at transcendence) shall
   surface as events in the log and in narration/report.

---

## 17. Event protocol and outputs

### 17.1 Event protocol

1. The event log shall be a versioned, append-only stream with one event per
   line, and a schema that is the single source of truth for the
   simulator-to-consumer contract — no ad-hoc fields on either side.
2. The header shall carry the seed, schema version, and simulator version so a
   consumer can refuse an incompatible log.
3. Events shall cover all structural transitions: run lifecycle; world and
   species derivation; civilization lifecycle (founding, territory change,
   collapse, succession, contact); figures; technology unlocks and discoveries;
   the discovery lifecycle (proposed, confirmed, falsified, refined,
   revalidated, lapsed, mythologized, rival/displacement); culture (cosmology,
   religion, cohesion); demographics (resilience, life expectancy, surplus);
   conflict (war, peace, alliance, trade); knowledge transfer; catastrophes;
   ecosystem (speciation, gene transfer, extinction); and developmental
   archetype.
4. Emission shall preserve deterministic order; no consumer mode shall reorder
   or alter the stream.
5. **Snapshots** shall be emitted periodically as coarse state digests for
   cross-checking and offline tooling; they are not full state checkpoints.

### 17.2 Live view

A live terminal view shall render the planet map, the species, the per-civ
state (population and trend, territory, technology, cohesion, beliefs,
war/peace), and a scrolling event log, updating as the run proceeds. It shall be
a pure observer of the event stream, shall never feed back into the simulation,
and shall offer at least a map-of-the-world view and a population-density view.
Pacing and framing controls are presentation-only.

### 17.3 Written report

A report generated from a saved event log shall be a pure, reproducible function
of that log, covering: a planet description; a species description; the species'
emergent "ages" and memorable figures and consolidated canon; a run summary;
per-civilization chapters (founding, growth, economy, conflicts, discoveries
with their fitted relationships, cultural drift, collapse, successors);
inter-civilization knowledge transfer; contact, conflict, and trade; spatial
keyframes of who lived where; a population timeline; and a curated highlight
reel of the most narratively significant events.

### 17.4 Prose narration

A narrator shall render the event stream as readable prose, live during a run or
replayed from a saved log, tracking names so later lines read naturally, and
suppressing low-signal per-tick noise. A standalone (non-simulator) narrator
over the same log format shall also be supported.

---

## 18. Scope boundaries (out of scope)

- An LLM anywhere in the pipeline (a downstream consumer may add one over the
  log).
- A graphical UI (terminal only).
- Networked or multi-process simulation.
- Save/load beyond periodic snapshots.
- Mod hooks.
- Modeling the general population as individual agents (the sim models named
  figures and cohorts).
- Inter-planet contact and multi-species worlds (single planet, single species).
- Quantum-scale physics, turbulent fluid regimes, full meteorological fidelity,
  consciousness physics, and faster-than-light travel.
- Audio.

---

## 19. Decisions to settle before building

The prior prototype left these load-bearing choices ambiguous or inconsistent;
a rebuild should settle each explicitly up front:

1. **Randomness model.** A single threaded seeded RNG versus deterministic
   hashing of stable identifiers — pick one and apply it uniformly. Whatever the
   choice, draw order is part of the determinism contract.
2. **Hemisphere/orientation convention.** Use one consistent north/south (and
   latitude) convention across climate, magnetism, rotation effects, radiation,
   and recognition.
3. **Evolution-vs-environment coupling.** Decide how strongly speciation and
   gene transfer depend on environmental drivers (e.g. radiation/magnetic
   state), so they are neither always-on nor effectively disabled.
4. **Calendar consistency.** Apply the per-planet year length uniformly across
   the simulator, report, and every narrator.
5. **Single source of truth for cross-cutting tables.** Keep paired data — the
   channels a phenomenon exposes versus the channels a species can perceive, and
   similar — defined once, so nothing is unreachable by construction.
6. **Completeness of derived variety.** Ensure the full intended variety is
   actually reachable at genesis: all life-cycle types, all ecological roles,
   and all habitats should be assignable from appropriate worlds, not narrowed
   to a default.

---

## Appendix — Representative run shape

A long run on a typical seed should produce a rich history: on the order of a
dozen-plus civilizations, thousands of confirmed scientific relations, knowledge
transmissions across collapse boundaries, grouped war campaigns, and a
multi-hundred-line report. These are qualitative expectations, not acceptance
thresholds.
