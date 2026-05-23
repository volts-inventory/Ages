# Viewport

Live ASCII viewport — the planet rendered in your terminal as it
evolves, repainted every N ticks while the sim runs. Shares the
post-run report's frame renderer
([`sim/report/src/frame.rs`](../sim/report/src/frame.rs)) so the
live view and the post-run keyframes are visually identical.

The viewport is a pure observer wrapped around the event stream —
it consumes the canonical NDJSON, mirrors a small per-civ state
snapshot, and never feeds back into the simulation. Toggling
`--cli=viewport` produces byte-for-byte identical NDJSON for the
same seed.

For deeper detail per file, see
[`sim/report/src/viewport/`](../sim/report/src/viewport/) and
[`sim/report/README.md`](../sim/report/README.md). After CC2 the
crate is split across one file per responsibility — `emitter`,
`state`, `log`, `cards`, `sidebar`, `layout`, `ansi`, `config`,
`mod` — wired together by `mod.rs`.

## Invocation

```sh
cargo run --release -p ages -- --seed 42 --years 1000 \
  --cli viewport --tick-rate-ms 50 --frame-every-ticks 6
```

Or just `./run.sh` for a fresh random seed at sensible defaults.

## Module layout

| File | Responsibility |
|------|----------------|
| `mod.rs` | Public surface (`ViewportConfig`, `TempUnit`, `ViewportEmitter`) and module wiring. |
| `emitter.rs` | The `ViewportEmitter<W>` struct + the `Emitter` trait `emit` dispatch + alt-screen lifecycle (`ensure_initialised` / `shutdown`). |
| `state.rs` | `apply_state` — event → state mirroring (planet/species/civ snapshots, dedup latches, cosmology cache, frame-cadence `should_render` gate). |
| `cards.rs` | `planet_card` and `species_card` — top-of-frame info blocks. |
| `sidebar.rs` | `build_sidebar_lines` — legend + species recap + per-civ panels with the colour swatch + cohesion / war / belief axes. |
| `log.rs` | `log_message` — event → scrolling-log classifier, plus `civ_label` / `relation_label` formatters and the `COSMOLOGY_AXIS_NAMES` table. |
| `layout.rs` | `render` — three-region frame composition (top, middle, bottom), the per-row absolute-positioning output strategy, and the dim-vs-bold ANSI wrap. |
| `ansi.rs` | ANSI escape constants + `divider` / `split_divider` / `pad_to` / `visible_width` / `write_centered_line` helpers + the `MAP_WIDTH` / `SIDEBAR_WIDTH` / `VIEWPORT_WIDTH` constants. |
| `config.rs` | `ViewportConfig` user-facing knobs + `TempUnit` enum. |

## Per-tick rendering

The viewport mirrors a tiny set of events into its state, then
re-renders the whole frame whenever the cadence gate fires:

- `Planet` / `PlanetMap` — terrain backdrop + orbital metadata.
- `Species` / `SpeciesCosmologyBias` / `RunMetadata` — species
  recap + label tables.
- `CivFounded` / `CivTerritoryChanged` / `CivCollapsed` — per-civ
  claim sets, names, founding years; the `CivState` map at
  `viewport/emitter.rs:135` carries the per-civ sidebar fields.
- `Tick { phase: TickEnd }` — cadence trigger (`should_render` at
  `viewport/state.rs:289`).
- `RunEnd` — restore terminal + final paint + `shutdown`
  (`viewport/emitter.rs:188`).
- Many other events feed only the scrolling log via
  `log_message` (`viewport/log.rs:73`).

Every other event is forwarded verbatim. The viewport never
mutates events.

## Layout

A 74-column themed box laid out as map zone (left, 40 cols) +
vertical `|` rule + sidebar (right, 30 cols). Defined in
`viewport/ansi.rs:50` (`MAP_WIDTH`, `SIDEBAR_WIDTH`,
`VIEWPORT_WIDTH`).

```
---------------- Lyra-a ----------------+------- log -------
        ocean world · scorching         | y3 civ 1 founded
    methane-rich · 241F · strong mag    | y14 unlocked sail
      129h · 11mo · 17° · 2 moons       | y57 met Karnan
         Y0 M0 · 1 civ · 1F/0C · 1834p  |
----------------- map ------------------+------- key -------
     01234567890123456789012345678901   | 1-9=fill% · …
    +--------------------------------+  | ~sea · ≈deep · …
   0|A11≈≈≈≈≈≈░▒▒▒▒▒△△△△△△△△△△▲▲▲△△△1| | ▒land · ░coast · ·
   1|11≈≈≈≈≈≈░▒▒▒▒▒△△△△△△△△△△▲▲▲▲▲△△1| |
   …                                    | ─── Cyranites ───
  19|111≈≈≈≈≈≈≈≈≈░▒▒▒▒▒△△△△△△000△△△△△ | centralized medium …
    +--------------------------------+  | sense: tactile · …
                                        | ─── Volthain ───
                                        | A=cap · 0-9=pop · t3
                                        | y127 · 14 cells · 247p
                                        | cohesion 82% · peace
```

`layout.rs` orchestrates three regions:

1. **Top** — planet section (left zone) + scrolling log (right
   zone), split by a `--- Name ---+--- log ---` divider with a
   `+` corner where the `|` rule joins the dashes (see
   `split_divider` at `viewport/ansi.rs:97`). When
   `show_planet_card = false` the log moves to a full-width
   bottom block instead.
2. **Middle** — the map grid (left zone, the body of
   `render_world_frame_styled`) zipped row-by-row with the
   sidebar (right zone, from `build_sidebar_lines`).
3. **Bottom** — full-width log fallback for bare-map test
   configs.

The planet card is block-centered: every line gets the same left
padding (computed off the widest line), so the card reads as one
balanced block rather than per-line independent centering
(`layout.rs:122`).

## Glyph table

Terrain glyphs come from
[`render::terrain_symbol`](../sim/report/src/render/planet.rs)
shared with the post-run report. Civ markers come from
`frame.rs:centroid_symbol` / `claim_symbol` / `pop_digit`.

| Glyph | Meaning |
|-------|---------|
| `≈` | Deep ocean (`water_depth > 100m`) |
| `~` | Shallow water |
| `≡` | Gas-giant cloud band (`terrain_peak == 0`) |
| `░` | Coastal land (axial neighbour is water) |
| `▒` | Inland land |
| `△` | Hill (top ~30% of land range) |
| `▲` | Peak (top ~15%) |
| `·` | Featureless rocky / sub-surface ocean ice |
| `0` (mono) | Nomadic species presence on an unclaimed cell |
| `1`–`9` (mono) | Cell claimed by exactly that civ id |
| `*` (mono) | Civ id ≥ 10 |
| `1`–`9` (colored) | Per-cell pop saturation (`pop_digit` in [`frame.rs:476`](../sim/report/src/frame.rs#L476)) |
| terrain glyph in civ colour | Cell claimed but pop < 10% of cap — "barely settled" frontier |
| `A`–`Z` | Civ centroid (capital letter). Older civ wins on collision via `entry().or_insert()` |
| `#` | Disputed cell (multi-owner) |
| `▒` in 3 brightness levels | Density-mode pop fill: bold ≥ 60% / normal ≥ 30% / dim < 30% |

Terrain glyphs colour-code in capable terminals via
`terrain_color_code` ([`frame.rs:418`](../sim/report/src/frame.rs#L418)):
blue water, brown mountains, green land, white peaks, light
yellow gas bands, gray featureless. ANSI-aware `visible_width`
([`ansi.rs:141`](../sim/report/src/viewport/ansi.rs#L141))
strips escape sequences before counting columns so coloured rows
align with the rest of the layout.

### Civ palette

Each civ id maps deterministically to a 256-colour palette index
via `civ_color_code` ([`frame.rs:435`](../sim/report/src/frame.rs#L435)):
a 24-hue ring spread across the 6×6×6 cube so adjacent civs
remain visually distinct. Civ ids cycle through the palette
modulo 24.

### Disputed-cell rendering

Multi-owner cells render as `#`. The render pipeline runs the
multi-owner check **after** the centroid lookup, so a contested
capital still shows the older civ's letter (`entry().or_insert()`
ordering in `frame.rs:170`); the surrounding non-centroid cells
still render `#`, leaving the border-conflict visible without
erasing the capital.

### Civ-id-zero filter

Civ-id `0` entries — leaked through from successor-handoff state
in some event sequences — would render as the `?` fallback of
`claim_symbol`. The render path filters them out so terrain shows
through (`frame.rs:252`).

## Sidebar

Built by `build_sidebar_lines`
([`sidebar.rs:41`](../sim/report/src/viewport/sidebar.rs#L41)).
Three sub-blocks separated by blank lines:

### Legend

Three lines of glyph reference. In colored mode the first line
swaps to `1-9=fill% · white=nomad · #=war` (or
`dim/bold=fill% · white=nomad · #=war` in density mode); in
monochrome the legacy `1-9=civ-id · *=civ≥10` line surfaces
since civ identity is then carried by the digit, not the colour.

### Species recap

Reuses `species_card` so the cognition / sense / biology summary
stays visible alongside the map even as it scrolls past. Header
reads `─── {SpeciesName} ───`.

### Per-civ panels

One block per active civ, ordered by **total population
descending** (sums Q96.32 cell pops via `i128` saturating add for
exact ranking; ties broken by civ id ascending —
`sidebar.rs:114`). Each panel layout:

```
─── Volthain ───
A=cap · 0-9=pop · t3 · 14 tools
last: writing
y127 · 14 cells · 247p ↑
cohesion 82% · life 67y · peace
empirical+0.3 · ritual-0.4
⚔ war: Karnan, Goran
```

- **Header** `─── name ───` — name painted in the civ's palette
  colour (256-colour bold) in colored mode, plain text in mono.
- **Identity line** — `cap=A · 0-9=pop · t{tier} · {N} tools`,
  with the cap letter + the `0-9` swatch both rendered in the
  civ's colour. In density mode the `0-9` swatch becomes three
  `▒` glyphs at dim / normal / bold brightness so the legend
  matches the on-map density encoding.
- **Last unlock** `last: {tool}` — only when the civ has unlocked
  anything; omitted to keep brand-new founders visually compact.
- **Founding + cell count + pop + trend** `y{year} · N cells · Mp {↑↓→}`
  — pop trend computed against the previous render's Q32 snapshot
  (`civ_last_emitted_pop_q32`), with a ±0.5% deadband so monthly
  food-cycle noise doesn't oscillate the arrow
  (`sidebar.rs:233`).
- **Cohesion / life / war tag** — cohesion as a 0-100% percentage
  (so the reader can compare against the civil-war floor ~10 and
  breakaway band 10-35), life expectancy in *this planet's* years
  (months ÷ `orbital_period_months`), war/peace status.
- **Belief axis** — strongest-magnitude cosmology + religion axis
  with signed magnitude (`empirical+0.30 · ritual-0.20`); hidden
  when both vectors are still neutral. Cosmology axis names from
  `COSMOLOGY_AXIS_NAMES` ([`log.rs:23`](../sim/report/src/viewport/log.rs#L23)):
  `empirical`, `communitarian`, `reformist`, `mystical`,
  `hierarchical`. Religion axes: `theology`, `ritual`,
  `afterlife`.
- **Active wars** `⚔ war: {names}` — mirrored from
  `WarDeclared` / `PeaceConcluded`; pairs stored as normalised
  `(min, max)` so the same war isn't listed twice.

The `civ_names` and `civ_founded_year` come from `CivFounded`
and clear on `CivCollapsed`. Civ names come from
`civ_name_from_seed(seed, civ_id)` — 64 stems × 6 endings = 384
deterministic kingdom-feeling names per `(seed, civ_id)` pair.

## Planet card

Built by `planet_card`
([`cards.rs:23`](../sim/report/src/viewport/cards.rs#L23)).
Compact format — every line ≤ 32 chars so the card fits portrait
phone terminals (iPhone Termius narrowest column ≈ 30 + 2 char
margin):

```
ocean world · scorching
methane-rich · 241F · strong mag
78%N₂ 21%O₂ 1%Ar
129h · 11mo · 17° · 2 moons
Y0 M0 · 1 civ · 1F/0C · 1834p
```

Lines, in order:

1. **Type + badge** — `{ptype} · {friendly_badge}` (e.g. `ocean
   world · scorching`).
2. **Climate** — `{atmosphere_descriptor} · {temp}{unit} · {mag}
   mag`. The substrate freeze/boil ranges drive the
   `host_species_status` badge and come from the `RunMetadata`
   event (perturbed by `p.substrate_perturbation_q32` so the
   per-seed values match the chemistry actually wired into the
   sim — see `cards.rs:43`).
3. **Atmospheric composition** — top three channels by mass
   fraction. Skipped on vacuum (sum ≈ 0).
4. **Orbital** — `{day_hours}h · {months}mo · {tilt}° · {N} moon(s)`.
5. **Caption** — `Y{n} M{n} · {civs} civ · {F}F/{C}C · {pop}p`
   written by `render` itself
   ([`layout.rs:56`](../sim/report/src/viewport/layout.rs#L56)).
   Year + month derive from the planet's actual orbital period —
   a 16-month-year planet shows months 0..=15.

`Planet` is emitted once at run start, so the card stays static
for the run.

## Species card

Built by `species_card`
([`cards.rs:125`](../sim/report/src/viewport/cards.rs#L125)).
Returns `None` until both `Planet` and `Species` events have
arrived (the biochem axis needs the planet's substrate). Three
lines:

```
centralized medium cognition
sense: tactile · manip: tentacle
44y · solitary · noisy · carbon
```

1. **Cognition** — `{topology} {tier} cognition` as a noun
   phrase. Topology mapped to the long form (`distributed` from
   `distributed-redundant`, `acentric` from `acentric`, etc.);
   tier word from `labels::cog_tier`.
2. **Senses + manipulation** — primary modality + primary
   manipulation mode prefixed with `sense:` / `manip:`.
3. **Biology** — `{lifespan}y · {sociality} · {comm} · {biochem}`.

The species name lives in the sidebar's `─── name ───` divider
header, not in the card body, to avoid duplication.

## ANSI alt-screen for clean rendering

In default `use_alt_screen = true` mode (the user-facing
interactive path) the viewport wraps the session in alternate-
screen buffer + cursor-hide on first frame, restores both on
`RunEnd`. ANSI constants in
[`viewport/ansi.rs`](../sim/report/src/viewport/ansi.rs):

| Const | Sequence | Purpose |
|-------|----------|---------|
| `ANSI_ALT_SCREEN_ON` | `\x1b[?1049h` | Enter alt buffer (preserves scrollback). |
| `ANSI_ALT_SCREEN_OFF` | `\x1b[?1049l` | Restore previous scrollback on shutdown. |
| `ANSI_HIDE_CURSOR` | `\x1b[?25l` | Hide cursor — avoids blink on the moving frame. |
| `ANSI_SHOW_CURSOR` | `\x1b[?25h` | Paired with hide on shutdown. |
| `ANSI_ERASE_LINE` | `\x1b[K` | Erase to EOL — appended after every body line so old chars from a longer prior frame don't bleed through. |
| `ANSI_ERASE_TO_END` | `\x1b[J` | Erase to end of screen — written after the last row so rows from a taller previous frame get wiped. |

### Per-row absolute-positioning paint

The render path
([`layout.rs:273`](../sim/report/src/viewport/layout.rs#L273))
walks the composed frame line-by-line and emits each row as
`\x1b[<row>;1H<line>\x1b[K`, never via `\n`. The terminal cursor
never advances past the last row and the alt-screen never
scrolls. After the last row a single `\x1b[J` cleans up rows
left over from a taller prior frame.

The implementation pauses ~1.5 ms between rows
(`std::thread::sleep(Duration::from_micros(1500))`) to drain the
PTY/terminal UTF-8 parser between row-sized writes. Some mobile
terminals (Termius / iOS over SSH) drop continuation bytes when
multi-byte glyphs like `▲` or `⚔` straddle PTY chunk boundaries
in tight back-to-back writes; the sub-ms pause prevents the
"glyph flickers as `?`" artifact. Total ≈ 60 ms per frame across
~40 rows — well under the default tick cadence.

In `use_alt_screen = false` mode (tests, piping to a file) the
whole frame writes in a single buffered `write_all` instead.

`run.sh` does *not* auto-shrink the grid based on terminal
height; the user can keep the full grid even if it scrolls the
planet section off the top.

## Scrolling log

3 lines by default (`ViewportConfig::log_lines`, fits portrait
phone terminals at ~25 visible rows without scrolling the planet
section off the top — `config.rs:107`). Built from
`log_message` ([`log.rs:73`](../sim/report/src/viewport/log.rs#L73))
which classifies each event into a one-line message or `None`.
Curated subset surfaces:

- Foundings + collapses (`CivFounded`, `CivCollapsed`).
- Catastrophes (`CatastropheFired`).
- First per-pair `KnowledgeTransmitted` (dedup via
  `transmissions_logged`).
- Per-pair war defeats (dedup via `wars_logged`; only when
  `loser_defeated = true`).
- `WarDeclared`, `PeaceConcluded` (with reason label).
- `AllianceFormed`, `AllianceDissolved` (with reason).
- `TechUnlocked` (verb varies by `serendipitous` flag).
- `CivContact` — same-tick contacts coalesce into one line
  (`"Karnan, Goran met Yothan"`) at `state.rs:38`.
- `CosmologyShifted` — names the dominant axis with signed
  magnitude (`drifts empirical+0.30`).
- `TemplateDiscovered`, `ToolDiscovered` (rare emergence beats).
- `SpeciesCosmologyBias` — tick-0 one-shot, dominant axis only.
- `RivalHypothesisProposed`, `PrimaryHypothesisDisplaced` (with
  template name).
- `RelationMythologized` (with axis name).
- `CohesionShifted` (rising/falling).
- `SpeciesDrift` — largest-magnitude channel.
- First per-relation `RelationConfirmed` (template-level dedup),
  first per-relation `RelationFalsified` / `RelationLapsed`.
- `FigureBorn` filtered to charisma ≥ 0.7 (top ~30%).

Per-pair / per-kind dedup latches live on `ViewportEmitter`
(`wars_logged`, `transmissions_logged`,
`templates_confirmed_logged`, `relations_falsified_logged`,
`relations_lapsed_logged`) and clear on `CivCollapsed` so a
re-emerged civ_id can re-trigger fresh lines.

`log_message` runs **before** the state-mutation match in
`apply_state` ([`state.rs:18`](../sim/report/src/viewport/state.rs#L18))
so events that drop identifying state (notably `CivCollapsed`
removing the civ name) log under the still-present name rather
than the `civ {id}` fallback.

## Sub-stellar point highlight on synchronous worlds

The world layer carries `sub_stellar_point(planet, macro_step)`
([`sim/world/src/tidal_locking.rs:129`](../sim/world/src/tidal_locking.rs#L129)).
For `LockingState::Synchronous` planets the point is fixed at
`(0, 0)` by convention — the locked face perpetually shows the
same hemisphere to the star, so the sub-stellar point doesn't
move. For free-rotator / resonance planets the longitude advances
with `macro_step` at one full revolution per
`day_length_hours / 24` macro-steps.

The current viewport surfaces the rotation state indirectly
through the planet card's day / year line and through the
post-run report's `Rotation state | synchronous (tidally
locked)` row
([`render/planet.rs:89`](../sim/report/src/render/planet.rs#L89);
the `rotation_state_label` heuristic compares
`day_length_hours / (orbital_period_months × 30 × 24)`). A
dedicated `*` overlay on the locked hemisphere's central cell
remains a follow-up: the geometry is wired (latitude/longitude
in fractional turns at `tidal_locking.rs:120`) but the renderer
doesn't yet project it onto the hex grid.

## Determinism

The viewport sits **outside** the simulation's compute path.
Toggling `--cli=viewport` doesn't change the canonical event log
— same seed produces byte-for-byte identical NDJSON regardless
of `--cli` mode or `--tick-rate-ms`. The viewport is a pure
function of the event stream + the small frame-state struct
described above.

`tput lines` and terminal-resize behaviour are read-only — the
viewport adapts to the terminal but doesn't feed back into the
simulation.

## Frame cadence

`--frame-every-ticks` (default 50) controls how often a frame
paints. `--tick-rate-ms` adds a real-time delay between ticks
for watchability. Both knobs are decorative — they don't affect
the NDJSON.

`should_render` ([`state.rs:289`](../sim/report/src/viewport/state.rs#L289))
gates on `Tick { phase: TickEnd }` events whose `tick %
frame_every == 0`, plus `RunEnd` for a guaranteed final frame.
