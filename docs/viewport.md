# Viewport

Live ASCII viewport. Renders the planet in your terminal as it
evolves, sharing the post-run report's frame renderer so the live
view and the post-run keyframes look identical.

For deeper detail per crate, see
[`sim/report/src/viewport/`](../sim/report/src/viewport/) and
[`sim/report/README.md`](../sim/report/README.md).

## Invocation

```sh
cargo run --release -p ages -- --seed 42 --years 1000 \
  --cli viewport --tick-rate-ms 50 --frame-every-ticks 6
```

Or just `./run.sh` for a fresh random seed at sensible defaults.

## Layout

74-column themed-box layout with a vertical rule separating the
map zone (left) from the sidebar (right):

```
---------------- Lyra-a ----------------
        ocean world · scorching
    methane-rich · 241F · strong mag
      129h · 11mo · 17° · 2 moons
         Y0 M0 · 1 civ · 1F/0C · 1834p

----------------- map ------------------
     01234567890123456789012345678901
    +--------------------------------+
   0|A11≈≈≈≈≈≈░▒▒▒▒▒△△△△△△△△△△▲▲▲△△△1|
   1|11≈≈≈≈≈≈░▒▒▒▒▒△△△△△△△△△△▲▲▲▲▲△△1|
   …
  19|111≈≈≈≈≈≈≈≈≈░▒▒▒▒▒△△△△△△000△△△△△|
    +--------------------------------+

----------------- key ------------------
  0-9=pop · 0=nomad · #=war
  ~sea · ≈deep · ▲peak · △hill
  ▒land · ░coast · ·=plain
-------------- Cyranites ---------------
      centralized medium cognition
    sense: tactile · manip: tentacle
    44y · solitary · noisy · carbon
----------------- log ------------------
      y0 civ 1 founded (10 cells)
```

Sections:

| Section | Content |
|---------|---------|
| Planet | Name, world type, climate, atmosphere, mean temp, magnetism, day length, year length, axial tilt, moons. Bottom row carries the calendar (Y/M), civ counts, figure counts, total population. |
| Map | The hex grid rendered as ASCII, framed by `+--+` corners. Default 36×30. |
| Key | Glyph legend — terrain bands, claim symbols, war markers, nomadic glyphs. |
| Species | Cognition topology, sensorium, manipulation, lifespan, sociality, communication fidelity, biochemistry. |
| Log | Scrolling 3-line event log. Recent events first. |
| Per-civ panels | One 4-line block per active civ: header `─── {Civ name} ───` (name colored in the civ's palette), identity line — `{letter}=cap · 0-9=pop` in colored mode (both glyphs serve as a colour swatch), or `{letter}=cap · {digit}=civ` in monochrome — then stats line `y{founded_year} · {N} cells · {pop}p` and religion line. |

## Map glyphs

| Glyph | Meaning |
|-------|---------|
| `≈` | Deep ocean |
| `~` | Shallow water |
| `≡` | Gas-giant cloud band |
| `░` | Coast |
| `·` | Low-relief / sub-surface ocean / oceanic basin floor |
| `▒` | Land |
| `△` | Hill |
| `▲` | Peak |
| `0` | Nomadic (unclaimed) population concentration; rendered in default white |
| `0`–`9` (colored) | Cell claimed by a civ. Digit is a log-scaled saturation reading: `9` = pop at the cell's carrying capacity, lower digits = farther below cap (each step ≈ ³√10× pop). Civ identity is carried by the colour, not the digit. |
| `1`–`9` (mono) | Cell claimed by exactly that civ. Used in the markdown post-run report where colours don't render. |
| `*` (mono) | Cell claimed by a civ with id ≥ 10 |
| `A`–`Z` | Civ centroid (capital letter); colored to match the civ's claim digit |
| `#` | Disputed cell (multi-owner) |

Terrain glyphs colour-code in capable terminals: blue water, brown
mountains, green land, white peaks. ANSI-aware visible-width
calculation handles colour-escape sequences without misaligning
the row gutter.

## Civ panels

Each currently-active civ surfaces a 4-line block sorted by
population descending (so the biggest civ shows first):

```
─── Volthain ───
  A=cap · 0-9=pop
  y127 · 14 cells · 247p
  reformist · solitary
```

In colored mode the civ name and identity glyphs render in the
civ's palette colour so the panel doubles as a colour swatch — a
reader can match each panel to the matching cells on the map at a
glance. In monochrome mode the identity line falls back to
`{letter}=cap · {digit}=civ` and the digit names the civ id (so
civs are still distinguishable in the markdown post-run report).

`civ_names` and `civ_founded_year` maps populate on `CivFounded`
and clear on `CivCollapsed`. Civ names come from
`civ_name_from_seed(seed, civ_id)` — 64 stems × 6 endings = 384
deterministic kingdom-feeling names per `(seed, civ_id)` pair.

## Disputed-cell rendering

Multi-owner cells render as `#`. The render pipeline runs the
multi-owner check **before** the centroid letter so overlapping
centroids surface as `#` instead of one civ's letter silently
masking the other. Older civ's `A` shows at the contested capital
(via `entry().or_insert()` ordering) so the same letter doesn't
flicker between civs.

## Atomic frame paint

Each frame writes to a `Vec<u8>` buffer first, then flushes in one
write — no incremental-paint flicker, no scroll, no half-painted
rows. The buffer starts with `\x1b[H\x1b[2J` (cursor home + clear)
so the prior frame is cleared as part of the same atomic write.

`run.sh` does *not* auto-shrink the grid based on terminal height;
the user can stick with the full 36×30 grid even if it scrolls
the planet section off the top.

## Conflict log dedup

Multi-tick wars produce per-cell `ConflictResolved` events. A
single declared war can produce 200+ skirmishes. The viewport
tracks `wars_logged: BTreeSet<(u32, u32)>` to dedupe so each war
surfaces **once** in the log per pair, cleared on `CivCollapsed`
so re-emerged civ_ids re-trigger.

`log_message` is a `&mut self` method on the viewport state to
mutate the dedup set deterministically.

## Determinism

The viewport sits **outside** the simulation's compute path.
Toggling `--cli=viewport` doesn't change the canonical event log
— same seed, byte-for-byte identical NDJSON regardless of
`--cli` mode or `--tick-rate-ms`. The viewport is a pure
function of the event stream + a small frame-state struct.

`tput lines` and terminal-resize behaviour are read-only — the
viewport adapts to the terminal but doesn't feed back into the
simulation.

## Frame cadence

`--frame-every-ticks` controls how often a frame paints. When the
flag is omitted, the default auto-scales with the total tick count
so a long run doesn't drown the viewport: the formula targets
~1200 frames across the run, with a floor of 50 ticks/frame. A
5000-year monthly-tick run keeps the snappy 50-tick cadence; a
50,000-year run paints every ~500 ticks. Pass an explicit value to
override.

`--tick-rate-ms` adds a real-time delay between ticks for
watchability — both knobs are decorative and don't affect the
NDJSON.
