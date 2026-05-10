# Culture

Two-layer cultural model: a slow-drift species-anchored
**cosmology** and a fast-divergent civ-keyed **religion**. Plus
the kinship / belligerence / conflict machinery they feed.

For deeper detail per crate, see
[`sim/civ/src/cosmology.rs`](../sim/civ/src/cosmology.rs),
[`sim/civ/src/religion.rs`](../sim/civ/src/religion.rs),
[`sim/civ/src/conflict.rs`](../sim/civ/src/conflict.rs), and
[`sim/civ/README.md`](../sim/civ/README.md).

## Cosmology — five slow-drift axes

The cosmology vector is the **deep worldview** layer. It drifts
slowly under hypothesis-engagement events and biases the
hypothesizer's confirmation rates. It's anchored at the
species level: every civ inherits the species' `initial_cosmology`
at founding.

Cosmology is a five-axis `Real` vector
(`sim/civ/src/cosmology.rs`):

| Field | Low pole | High pole |
|-------|----------|-----------|
| `empirical` | Mystical / revelation | Empirical / measurement |
| `communitarian` | Individualist | Communitarian |
| `reformist` | Dogmatic / canonical | Reformist / open to revision |
| `mystical` | Mechanistic | Mystical |
| `hierarchical` | Egalitarian | Hierarchical |

(`empirical` and `mystical` are paired but not strictly
opposite — a civ can be high on both, modelling religious
empiricism / sacred natural philosophy.)

Each hypothesis-engagement event applies a small push along one
or more axes. Cosmology magnitudes were tuned down to "slow
drift" — the per-event push is small, and the emit threshold
(`COSMOLOGY_EMIT_THRESHOLD`) is correspondingly raised so
`CosmologyShifted` only fires when an axis really moves.

Cosmology biases the hypothesizer (see [discovery.md#cosmology-bias](discovery.md#cosmology-bias)):
high `reformist` accelerates confirmations on heretical forms;
low `reformist` (dogmatic) slows them. High `empirical` boosts
candidates with strong residual fits; high `mystical` is
permissive on threshold-firing patterns.

## Religion — three fast-divergent axes

The religion vector is the **fast-divergent cultural** layer that
real history actually shows over centuries (Catholic vs. Orthodox,
Sunni vs. Shia, Theravada vs. Mahayana). Civs founded from a
common ancestor can diverge religiously within generations even
while sharing cosmology.

| Axis | Description |
|------|-------------|
| `theology` | Polytheist ↔ monotheist ↔ non-theistic |
| `ritual` | Lay-led / spontaneous ↔ priest-mediated / formal |
| `sacred_time` | Cyclic festivals ↔ linear sacred history |

Religion is keyed by `(seed, civ_id)` jitter — each civ founds with
a unique vector. Every hypothesis-engagement event applies a push
to religion at **3× the cosmology magnitude**, so religion drifts
fast where cosmology drifts slow.

`ReligionShifted` events emit on L2 drift ≥ 0.20.

## Kinship

Kinship `∈ [0, 1]` between two civs is a weighted closeness
across four channels:

| Weight | Channel |
|--------|---------|
| `KINSHIP_W_HIER` = 0.10 | Hierarchical cosmology axis |
| `KINSHIP_W_COSMO` = 0.15 | The four non-hierarchical cosmology axes (averaged) |
| `KINSHIP_W_TECH` = 0.15 | Literacy |
| `KINSHIP_W_RELIGION` = 0.60 | Three-axis religion vector |

Religion dominates (0.60) so the kinship lever survives in
single-species runs where cosmology stays clustered around the
species' anchor.

## Belligerence + war

War is a stateful relationship between two civs, not a per-tick
overlap check. The pipeline:

1. **Contact** — `CivContact` must have emitted between the pair
   first (kinship-weighted belligerence cannot fire pre-contact).
2. **Belligerence score** — per pair:
   ```
   drive       = 0.45·pressure + 0.25·opportunity + 0.30·dominance
   belligerence = drive · (1 − KINSHIP_DAMPENER · kinship)
   ```
   `KINSHIP_DAMPENER = 0.20` — tuned down because single-species
   runs have kinship ≈ 1.0 throughout (every civ inherits the
   species' `initial_cosmology` bias).
3. **Hysteresis** — `WarDeclared` fires when belligerence ≥ 0.35;
   `PeaceConcluded` fires when it falls back below 0.20. Prevents
   flapping at threshold.
4. **War resolution** — once declared, the existing 75-tick
   `conflict::resolve()` machinery runs cell-by-cell skirmishes
   with marching front semantics.

## Multi-tick wars

Border conflicts resolve cell-by-cell across multiple skirmish
events. Each cell flips when the loser cohort's per-cell
population crosses `CELL_FLIP_FLOOR` (= `CONFLICT_DEFEAT_FLOOR / 2`).
A per-skirmish `loss_frac` ceiling caps single-event population
loss at 50%.

The post-run report groups consecutive `ConflictResolved` events
between the same pair into "war campaigns" with start/end years,
peak loss percentage, and final outcome — see
[report.md](report.md). The viewport's per-tick log dedupes by
`(loser, winner)` pair so a 200-skirmish war surfaces once per
pair instead of flooding the log.

## Cultural events

- `CosmologyShifted(axis, signed_magnitude)` — slow-drift
  cosmology shift past `COSMOLOGY_EMIT_THRESHOLD`.
- `ReligionShifted(axis, signed_magnitude)` — fast-divergent
  religion shift past `RELIGION_EMIT_THRESHOLD = 0.20`.
- `WarDeclared(civ_a, civ_b, belligerence)` — pair crosses the
  declare threshold.
- `PeaceConcluded(civ_a, civ_b, ticks_elapsed)` — pair drops
  below the end threshold.
- `ConflictResolved(civ_a, civ_b, cell, ...)` — per-cell
  skirmish outcome; aggregated into war campaigns by the
  post-run report.

## Cosmology + religion in transmission

Inter-civ knowledge transmission has a mid-comprehension band
that doesn't transfer the relation content but instead nudges the
receiving civ's cosmology along an axis aligned with the
relation's themes. See [discovery.md#transmission-and-mythologization](discovery.md#transmission-and-mythologization).
A society that lost the original physics retains the cultural
shadow.
