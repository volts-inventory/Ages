# Ages — Concept & Requirements

A conceptual description of what Ages is and what it should do. This is the
*idea*, not the design: no formulas, no data structures, no mechanics. It's
meant as the starting point for a fresh build, where the technical design gets
worked out from scratch.

---

## The idea

Ages is a program that invents an alien world from a seed and then writes the
**biography of a species that lives on it** — across thousands of years, through
the rise and fall of its civilizations.

You give it a number. It dreams up a planet, grows life suited to that planet,
and lets that life build societies that discover things, believe things, fight,
trade, collapse, and rebuild. Then it tells you the story of all of it.

There is no AI language model involved and nothing comes from the internet. The
same seed always produces the same world and the same history. It's for people
who like emergent worlds, "what if physics were different" toys, replayable
seeds, and reading a story a machine made up on its own.

---

## What makes it interesting

- **Every world is genuinely alien, and genuinely habitable.** Not just
  Earth-with-different-colors — worlds with different chemistries of life,
  different skies, different suns. Whatever the seed, *something* lives there.
- **The science is real, not scripted.** The inhabitants don't unlock a tech
  tree of pre-written facts. They actually observe their world and figure out
  how it works — and because their world is different from ours, the physics
  they discover is different from ours. A species that never sees anything
  cyclical can't even conceive of a wave.
- **Being wrong is part of the story.** Civilizations hold mistaken beliefs,
  argue about them, and sometimes overturn them. Knowledge gets lost when a
  civilization falls, and what little survives can be half-remembered into myth.
- **The species is the hero; civilizations are chapters.** Empires are
  episodes in a much longer life. The species itself slowly changes across the
  ages.
- **No two playthroughs converge on the same destiny.** There's no single
  "path to progress." A world's whole developmental arc — what its science is
  even *built on* — emerges from its nature.

---

## Design spirit

- **Grounded, not gamey.** Things should follow from physics and biology, not
  from arbitrary rules bolted on for flavor.
- **Emergent, not authored.** Interesting outcomes should arise from simple
  parts interacting, not from pre-written scripts.
- **Deep, not decorative.** Discoveries, beliefs, and conflicts should have real
  substance behind them, not just labels.
- **Reproducible.** Same seed, same story, always — that determinism is a
  promise the whole thing rests on.
- **Quiet and self-contained.** It runs on its own and emits a record. No
  network, no external services, no language model.

---

## The world

From a seed, Ages conjures a complete planet: its chemistry of life, its
atmosphere and oceans (or whatever stands in for oceans), its land, its weather
and seasons, its magnetic field, its star, its moons, its place in its solar
system. The planet should feel internally consistent — everything fits together
— and it should be able to be almost anything: frozen, molten, ocean, desert,
tide-locked, giant, tiny. The world isn't static scenery; it has a living
physics that runs the whole time.

There is one planet and one species per story.

---

## The life

A species grows to fit the world it's given. Its senses, its body, where it can
live, how long it lives, how it reproduces, how it thinks, how social it is —
all of that should follow from the planet. A creature of a sunless ocean world
won't have eyes; a creature of a crushing hot world is built for heat. The
species can take many forms — solitary minds, hive minds, even diffuse
colony-intelligences — and the kind of mind it has colors everything that
follows.

The world also has a wider web of life around the species — things it eats,
things that eat it, things it depends on — so the environment has ecological
weight. Life can evolve over the ages, and mass die-offs reshape who survives.

---

## The civilizations

Within the species, civilizations arise where life grows dense enough, then run
their course and fall, and others rise after them — sometimes side by side,
sometimes one after another, like Sumer to Babylon. Each is its own society with
its own territory, its own leaders, its own beliefs, its own knowledge and
tools.

Civilizations:

- **expand** across the world and **meet** their neighbors,
- **trade** in peace and **war** when pushed, with rivalry shaped by how alien
  the other side feels (kinship, shared beliefs, old grudges),
- **fracture** from within when they lose cohesion or split over doctrine,
- **collapse** for many reasons — famine, stagnation, zealotry, war, ruin —
- and are **succeeded** by others that inherit some of what came before.

When a civilization falls, most of what it knew is lost. The species carries on.

---

## The science

This is the heart of it. Civilizations look at their world and try to explain
it. They form hypotheses, test them against what they can actually observe,
confirm the ones that hold up, and keep refining them. They entertain competing
explanations and can overturn an old consensus with a better one. They can even
build experiments to probe the world deliberately rather than just watch it.

Crucially, what a civilization *can* discover is bounded by what it can perceive
and by the world it lives in. Different senses and different worlds lead to
genuinely different bodies of knowledge.

Knowledge passes from a fallen civilization to its successors imperfectly. Some
survives intact, some is misunderstood, and some degrades into myth and folklore
that shapes the next civilization's worldview without ever being understood.

---

## Belief and culture

Each society carries two layers of culture: a slow, deep **worldview** that
shapes what kinds of explanations even feel plausible to it, and a faster-moving
**religion** that gives it identity and divides it from others. Beliefs drift as
a civilization succeeds, fails, and collapses. A rigidly dogmatic culture
resists new ideas — and can ossify and die because of it. How close two
civilizations feel in belief and lineage governs whether contact turns to trade
or to war.

---

## Tools and the shape of progress

Technology grows out of what a civilization understands, what its body can
build, and what its world offers. There is deliberately **no single ladder of
progress**. A world that can't make fire takes an entirely different road than
one that can. Each world's development is best understood as resting on whatever
foundation its nature provides — combustion, or living chemistry, or fields and
resonance, or tides, or starlight, or others entirely — and the story should
recognize and name that character, including combinations nobody anticipated. At
the far end, different foundations lead to different ultimate fates, not one
shared singularity.

---

## Upheaval

The world throws disasters at its life — eruptions, plagues, impacts, stellar
storms, ice ages. Who survives depends on how prepared a civilization is and how
hardy the species is. Catastrophes don't just kill; they steer evolution, biasing
which traits carry forward into the species' future.

---

## What you get out of it

A run produces a faithful **record of everything that happened**, from which the
program can render:

- a **written history** — the planet, the species, a chapter for each
  civilization (its rise, its discoveries, its beliefs, its wars, its fall, its
  heirs), maps of who lived where, and a highlight reel of the moments that
  mattered;
- a **live view** you can watch as the world unfolds; and
- a **prose telling** of the story for anyone who'd rather read it as a tale.

The same story can always be replayed and retold from the record, exactly.

---

## Deliberately not in scope

- No AI language model anywhere in it.
- No graphical game; it lives in the terminal and in its written output.
- One planet, one species per story — no interplanetary or inter-species
  contact.
- It models notable figures and populations, not every individual.
- It stays at a human/planetary scale — no quantum physics, no faster-than-light,
  no claims about consciousness.
- No sound.

---

## Easy to get wrong (worth deciding early)

These are the places where the spirit of the project is easy to lose:

- **Don't privilege one path.** It's tempting to assume a fire-and-industry arc;
  the whole point is that the path emerges from the world.
- **Keep the full range reachable.** All the kinds of life, minds, habitats, and
  worlds the design promises should actually be able to appear — not quietly
  collapse to one default case.
- **Let evolution and culture actually move.** Drift, speciation, lost
  knowledge, and shifting belief should be live forces, not dormant ones.
- **Guard the determinism.** Reproducibility is foundational; it has to be
  designed in from the start, not patched on.
