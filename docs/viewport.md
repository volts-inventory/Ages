# Viewport

Live ASCII viewport. Renders the planet in your terminal as the
simulation evolves, sharing the post-run report's `WorldFrame`
renderer so the live view and the post-run keyframes look
identical.

For deeper detail per crate, see
[`sim/report/src/viewport/`](../sim/report/src/viewport/) and
[`sim/report/src/frame.rs`](../sim/report/src/frame.rs).

## Invocation

```sh
cargo run --release -p ages -- --seed 42 --years 1000 \
  --cli viewport --tick-rate-ms 50 --frame-every-ticks 6
```

Or `./run.sh` for a fresh random seed at sensible defaults.

## Module layout

The viewport is split across seven small files so each concern
stays on its own page
([`sim/report/src/viewport/mod.rs:20-27`](../sim/report/src/viewport/mod.rs)):

| File | Responsibility |
|---|---|
| `emitter.rs` | `ViewportEmitter` struct + `Emitter` trait dispatch + alt-screen lifecycle. |
| `state.rs`  | `apply_state` — event → snapshot mirroring + the `should_render` frame-cadence gate. |
| `log.rs`    | `log_message` event classifier for the scrolling log, plus the cosmology / civ / relation label helpers. |
| `cards.rs`  | `planet_card()` and `species_card()` formatters. |
| `sidebar.rs`| `build_sidebar_lines()` — legend block + species recap + per-civ panels. |
| `layout.rs` | `render()` — three-region frame composition and per-row absolute-positioning paint. |
| `ansi.rs`   | ANSI escapes, divider helpers, visible-width math. |
| `config.rs` | `ViewportConfig` + `TempUnit`. |

The emitter mirrors a curated subset of events into snapshots
([`sim/report/src/viewport/emitter.rs:8-15`](../sim/report/src/viewport/emitter.rs)):
`Planet` / `PlanetMap`, `CivFounded`, `CivTerritoryChanged`,
`CivCollapsed`, `Tick { phase: TickEnd }` (the frame-cadence
trigger), and `RunEnd` (restore terminal + final frame). Every
other event is forwarded verbatim — the viewport is a pure
observer that happens to also write a refreshing frame to
stdout.

## Frame layout

74-column box layout with a vertical `|` rule splitting the
middle row into the **map zone** (left, 40 cols) and the
**sidebar** (right, 30 cols). Widths are pinned in
[`sim/report/src/viewport/ansi.rs:50-67`](../sim/report/src/viewport/ansi.rs):
`MAP_WIDTH = 40`, `SIDEBAR_WIDTH = 30`, `VIEWPORT_WIDTH = 74`.

```
---------------- Lyra-a ----------------
        ocean world · scorching
    methane-rich · 241F · strong mag
      129h · 11mo · 17° · 2 moons
         Y0 M0 · 1 civ · 1F/0C · 1834p

----------------- map ------------------+--------- key ----------
     01234567890123456789012345678901   | dim/bold=fill% · white=nomad · #=war
    +--------------------------------+  | ~sea · ≈deep · ▲peak · △hill
   0|A11≈≈≈≈≈≈░▒▒▒▒▒△△△△△△△△△△▲▲▲△△△1| | ▒land · ░coast · ·=plain
   1|11≈≈≈≈≈≈░▒▒▒▒▒△△△△△△△△△△▲▲▲▲▲△△1| |
   …                                    | ─── Cyranites ───
  19|111≈≈≈≈≈≈≈≈≈░▒▒▒▒▒△△△△△△000△△△△△|  | centralized medium cognition
    +--------------------------------+  | sense: tactile · manip: tentacle
                                        | 44y · solitary · noisy · carbon
                                        |
                                        | ─── Volthain ───
                                        | A=cap · ▒▒▒=pop · t2 · 4 tools
                                        | last: thermal sensor
                                        | y0 · 14 cells · 247p →
                                        | cohesion 100% · life 44y · peace
```

When `show_planet_card = true` (the default), the recent-events
log rides alongside the planet card at the top of the frame
instead of along the bottom — kept in
[`sim/report/src/viewport/layout.rs:94-169`](../sim/report/src/viewport/layout.rs).
Bare-map test configs (`show_planet_card = false`) keep the
classic centred map + bottom log layout.

### Sections

| Section | Source |
|---|---|
| Planet card | `planet_card()` — type · habitability badge, atmosphere · temperature · magnetism, atmospheric composition, day · year · tilt · moons. ([`cards.rs:23-109`](../sim/report/src/viewport/cards.rs)) |
| Caption | `Y{year} M{month} · {N} civ · {F}F/{C}C · {pop}p`. ([`layout.rs:56-64`](../sim/report/src/viewport/layout.rs)) |
| Map | `WorldFrame` shared with the post-run report (see [Frame renderer](#frame-renderer)). Default 36×30, compact 1-char-per-cell. |
| Sidebar legend | Glyph reference, 3 lines, mode-aware. ([`sidebar.rs:67-88`](../sim/report/src/viewport/sidebar.rs)) |
| Species card | Cognition phrase · sense/manip · lifespan / sociality / comm / biochem. ([`cards.rs:125-163`](../sim/report/src/viewport/cards.rs)) |
| Per-civ panels | One block per active civ, sorted by population desc. ([`sidebar.rs:114-376`](../sim/report/src/viewport/sidebar.rs)) |
| Log | Scrolling 3-line event log (default), most-recent-last. ([`log.rs`](../sim/report/src/viewport/log.rs)) |

## Map glyphs

Glyph picking lives in
[`frame.rs:render_world_frame_styled`](../sim/report/src/frame.rs)
with precedence: centroid → multi-owner `#` → single-owner cell
→ nomad → terrain. Terrain selection is in
[`render/planet.rs:terrain_symbol`](../sim/report/src/render/planet.rs).

### Terrain (biome) glyphs

| Glyph | Meaning | 256-color (`use_color`) |
|---|---|---|
| `≈` | Deep water (`water_depth > 100 m`) — Earthlike phase | 27 deep blue |
| `~` | Shallow water (`water_depth > 0`) — Earthlike phase | 39 sky blue |
| `░` | Coastal land (axial neighbour is water) | 143 sand |
| `▒` | Inland land (lower 55% of land range) | 34 forest green |
| `△` | Hill (next 30% of land range) | 94 brown |
| `▲` | Peak (top 15% of land range) | 15 bright white |
| `*` | Magma plain (Lava phase: silicate biology in molten range) | 208 orange |
| `+` | Ice sheet (IceCap phase: aqueous world below water freeze, `water_depth > 0`) | 159 light cyan |
| `≡` | Gaseous shell (`terrain_peak == 0`) | 222 light yellow |
| `·` | Featureless rocky / sub-surface ocean / oceanic basin | 244 gray |

The terrain colour table lives in
[`frame.rs:terrain_color_code`](../sim/report/src/frame.rs).

### Surface phase

The glyph picker is parameterised by a `SurfacePhase` enum
([`render/planet.rs:surface_phase`](../sim/report/src/render/planet.rs))
derived once per planet from substrate + mean temperature vs the
substrate's solvent freeze/boil:

| Phase | Trigger | Glyph remap |
|---|---|---|
| **Earthlike** | Anything not below — temperate aqueous, non-molten silicate, hydrocarbon/ammoniacal | Historical glyph set (`▲△▒░~≈`) |
| **Lava** | Silicate biology, `freeze_k ≤ T ≤ boil_k` (silicate-melt range) | Every cell → `*`; peaks/outcrops still `▲`/`△` |
| **IceCap** | Aqueous biology, `T < water_freeze` (Europa, Mars, snowball Earth) | `water_depth > 0` cells → `+`; land cells unchanged |

`SurfacePhase` is plumbed through `FrameStyle::phase` so the
viewport, the post-run report's ASCII map, the per-civ territory
map, and the density frame all see the same phase.

### Planet archetype label

The first line of the planet card (e.g. `ocean world · scorching`)
comes from [`labels::planet_archetype`](../sim/report/src/labels.rs).
Unlike the legacy substrate-only `planet_type`, it consults the
actual surface water coverage and thermal regime so the label
tracks geography:

| Substrate | Frozen (T < freeze) | Liquid + ocean ≥ 50% | Liquid + ocean 15–50% | Liquid + ocean 2–15% | Liquid + ocean < 2% | Vapor (T > boil) |
|---|---|---|---|---|---|---|
| aqueous | ice world | ocean world | continental world | arid world | desert world | hothouse world |
| hydrocarbon | frozen methane world | methane sea world | methane-lake world | frigid arid world | frigid desert | scorched hydrocarbon world |
| ammoniacal | frozen ammonia world | ammonia sea world | ammonia-lake world | cold arid world | cold desert | scorched ammonia world |
| silicate | rocky world | lava world | lava world | lava world | lava world | vaporised silicate world |

`terrain_peak == 0` short-circuits the whole table to `gas giant`.

### Civ / population glyphs

| Glyph | Meaning |
|---|---|
| `A`–`Z` | Civ centroid (civ 1 = `A`, civ 27 = `A` again — modulo 26) — [`frame.rs:centroid_symbol`](../sim/report/src/frame.rs). |
| `#` | Disputed cell (multiple civ owners). Centroid check runs before dispute check, so a contested capital still shows the older civ's letter. |
| **Colour mode, digit mode (default off):** `1`–`9` | Per-cell **population saturation** as `pop / cap` × 10, civ identity carried by colour. Digit `9` = ≥90% of carrying capacity, `1` ≈ 10%, < 10% falls back to the terrain glyph in civ colour ("ownership by colour, barely settled by landform"). See [`frame.rs:pop_digit`](../sim/report/src/frame.rs). |
| **Colour mode, density mode (default on):** terrain glyph in civ colour | Brightness encodes pop / cap — bold ≥ 60%, normal ≥ 30%, dim < 30%. Centroid letters still mark capitals. ([`frame.rs:262-330`](../sim/report/src/frame.rs)) |
| **Mono mode (markdown report):** `1`–`9` | Civ-id digit. `*` for civ ids ≥ 10. |
| `0` (mono) / terrain glyph in bright white (colour) | Nomadic species presence (no civ claim, pop > `NOMAD_DISPLAY_FLOOR_POP`). ([`frame.rs:331-365`](../sim/report/src/frame.rs)) |

Civ id → 256-color palette: 24 hand-picked hues, cycling
([`frame.rs:civ_color_code`](../sim/report/src/frame.rs)).

## Sidebar

Built by `build_sidebar_lines()`
([`sidebar.rs:41-377`](../sim/report/src/viewport/sidebar.rs)).
Three sub-blocks, each separated by a blank line.

### Legend

Three lines, mode-aware
([`sidebar.rs:67-114`](../sim/report/src/viewport/sidebar.rs)):

- **Colour mode, digit:** `1-9=fill% · white=nomad · #=war`
- **Colour mode, density:** `dim/bold=fill% · white=nomad · #=war`
- **Mono mode:** `1-9=civ-id · *=civ≥10` / `0=nomad · #=war · ~sea · ≈deep`

Lines 2 and 3 carry the terrain glyph key and **adapt to the
surface phase**:

- **Earthlike:** `~sea · ≈deep · ▲peak · △hill` / `▒land · ░coast · ·=plain`
- **Lava:** `* magma · ▲peak · △outcrop` / `(rocky peaks exposed above magma sea)`
- **IceCap:** `+ ice sheet · ▲peak · △hill` / `▒land · ░coast · ·=plain`

### Species panel

3-line body from `species_card()`, headed by `─── {species name} ───`:

```
─── Cyranites ───
centralized medium cognition
sense: tactile · manip: tentacle
44y · solitary · noisy · carbon
```

### Per-civ panels

One block per currently-active civ, sorted by total population
descending (ties broken by civ_id ascending —
[`sidebar.rs:126-131`](../sim/report/src/viewport/sidebar.rs)).
Each block:

```
─── Volthain ───            ← civ name painted in civ palette colour
A=cap · ▒▒▒=pop · t2 · 4 tools
last: thermal sensor
y127 · 14 cells · 247p ↑
cohesion 72% · life 44y · at war ⚔
empirical+0.3 · ritual-0.4
⚔ war: Karnan
```

Lines:

1. **Header** — `─── {Civ name} ───` or `─── civ {id} ───` if
   the name hasn't arrived yet. In colour mode the name renders
   in the civ's palette colour so the panel doubles as a colour
   swatch.
2. **Identity** — `{centroid_letter}=cap · {pop_swatch}=pop ·
   t{tier} · {N} tools`. In density mode `pop_swatch` is three
   `▒` glyphs in civ colour at dim / normal / bold brightness so
   the reader can match the swatch ladder to map cells. In digit
   mode it's `0-9` in civ colour. In mono mode it's the legacy
   `{letter}=cap · {digit}=civ`. ([`sidebar.rs:172-201`](../sim/report/src/viewport/sidebar.rs))
3. **Last unlock** (when present) — `last: {tool_name}`.
4. **Year / cells / pop** — `y{founded_year} · {N} cells ·
   {pop}p {trend}`. The trend arrow (`↑` / `↓` / `→`) compares
   against the previous frame's snapshot with a ±0.5% deadband
   ([`sidebar.rs:228-256`](../sim/report/src/viewport/sidebar.rs)).
5. **Cohesion / life / war** — `cohesion {pct}% · life {y}y · {at war ⚔ | peace}`.
6. **Belief axes** (optional) — dominant cosmology + religion
   axis with signed magnitude, e.g. `empirical+0.3 · ritual-0.4`.
   Threshold: |axis| ≥ 0.20 ([`sidebar.rs:301-342`](../sim/report/src/viewport/sidebar.rs)).
7. **War rivals** (optional) — `⚔ war: {Foo, Bar}` listing
   active war partners by name.

`civ_names`, `civ_founded_year`, cosmology, religion, tech tier,
tools, cohesion, life expectancy, and last unlocked tool all
live on the per-civ `CivState` struct
([`emitter.rs:134-145`](../sim/report/src/viewport/emitter.rs)),
populated incrementally from per-event handlers and cleared
together on `CivCollapsed`.

## Cards

### Planet card

Three lines, each ≤ 32 chars so the card fits on portrait phone
terminals ([`cards.rs:23-109`](../sim/report/src/viewport/cards.rs)):

1. `{planet_type} · {friendly_habitability_badge}` — e.g.
   `ocean world · scorching`.
2. `{atmosphere_descriptor} · {temperature}{unit} · {mag} mag` —
   reads the per-seed substrate freeze/boil perturbation from
   `RunMetadata` so the displayed value matches the physics
   wiring.
3. Atmospheric composition (top three channels by mass fraction)
   when non-vacuum.
4. `{day}h · {months}mo · {tilt}° · {N} moon(s)` — orbital
   mechanics.

Temperature unit defaults to Fahrenheit; flip via `TempUnit` in
the `ViewportConfig` ([`config.rs:42-86`](../sim/report/src/viewport/config.rs)).

### Species card

Three lines ([`cards.rs:125-163`](../sim/report/src/viewport/cards.rs)):

1. `{topology} {tier} cognition` — full-word topology
   (`centralized` / `distributed` / `collective` / `acentric`) +
   tier bucket from `cog_tier`.
2. `sense: {primary modality} · manip: {primary manipulation}`.
3. `{lifespan}y · {sociality} · {comm fidelity} · {biochem}`.

The species name is not repeated here — the sidebar's
`─── {species name} ───` divider carries it.

## ANSI / alt-screen lifecycle

`use_alt_screen = true` (the default) wraps the session in the
terminal's alternate-screen buffer + hides the cursor on first
frame ([`emitter.rs:176-194`](../sim/report/src/viewport/emitter.rs))
and restores both on `RunEnd`. The user's scrollback is
untouched. Escape constants live in
[`ansi.rs:8-43`](../sim/report/src/viewport/ansi.rs):

```
\x1b[?1049h   alt-screen on        ANSI_ALT_SCREEN_ON
\x1b[?1049l   alt-screen off       ANSI_ALT_SCREEN_OFF
\x1b[?25l     hide cursor          ANSI_HIDE_CURSOR
\x1b[?25h     show cursor          ANSI_SHOW_CURSOR
\x1b[K        erase to end of line ANSI_ERASE_LINE
\x1b[J        erase to end of screen ANSI_ERASE_TO_END
```

### Per-row paint strategy

The render loop builds the whole frame into an in-memory
`Vec<u8>` first
([`layout.rs:85-245`](../sim/report/src/viewport/layout.rs)). In
alt-screen mode each row is then emitted with an absolute
cursor-positioning escape (`\x1b[<row>;1H`) + `\x1b[K`, never
via `\n`, so the terminal never scrolls. After the last row a
trailing `\x1b[J` cleans up rows below the new frame's bottom
([`layout.rs:273-310`](../sim/report/src/viewport/layout.rs)).

A 1.5 ms inter-row pause keeps multi-byte glyphs (`▲`, `⚔`, `≈`)
from straddling PTY chunk boundaries on mobile SSH terminals
([`layout.rs:296-308`](../sim/report/src/viewport/layout.rs)) —
documented mitigation for iOS Termius's UTF-8 parser dropping
continuation bytes when rows arrive back-to-back.

When `use_alt_screen = false` the buffer is flushed in a single
write (no per-row positioning, no scroll-prevention) — the test
configs and pipe-to-file consumers take this path.

## Sub-stellar point on synchronous worlds

The planet-card layer flags rotation state via
`rotation_state_label` in
[`render/planet.rs:359-382`](../sim/report/src/render/planet.rs):
a planet whose day length lies within ±5% of one orbital period
reads as `synchronous (tidally locked)`, anything longer is a
`slow rotator`, anything shorter is `free rotation`. The
underlying tidal-locking model + sub-stellar longitude
advancement live in
[`sim/world/src/tidal_locking.rs`](../sim/world/src/tidal_locking.rs):
`sub_stellar_point(planet, macro_step)` returns the locked
planet's fixed sub-stellar `(lat, lon)` or, for free rotators,
the longitude that advances each macro-step.

The live viewport does **not** yet paint a sub-stellar point
highlight on the map grid — the rotation classification surfaces
only in the post-run markdown report's planet table. The
geometry is plumbed end-to-end through `sim_world`; a follow-up
can read the sub-stellar `(lat, lon)`, project it onto the hex
grid, and overlay a glyph on the frame renderer without
disturbing the determinism contract.

## Frame renderer

`render_world_frame_styled`
([`frame.rs:115-409`](../sim/report/src/frame.rs)) is the single
ASCII-grid renderer shared by:

- **Live viewport** (`--cli=viewport`) — `ViewportEmitter::render`
  passes `FrameStyle { use_color, compact, density }` populated
  from the user's `ViewportConfig`.
- **Post-run report keyframes** — `Digest::keyframes(every_ticks)`
  reconstructs `WorldFrame`s at fixed intervals and the markdown
  layer renders each one in monochrome compact density (see
  [report.md](report.md)).

Both paths build the same `WorldFrame { tick, civs, nomad_cells }`
struct ([`frame.rs:56-63`](../sim/report/src/frame.rs)) — there's
no second renderer to drift.

## Determinism

The viewport sits **outside** the simulation's compute path.
Toggling `--cli=viewport` doesn't change the canonical event log
— same seed, byte-for-byte identical NDJSON regardless of
`--cli` mode or `--tick-rate-ms`. The viewport is a pure
function of the event stream plus the small set of snapshot
structs on `ViewportEmitter`.

Terminal-resize behaviour is read-only — the viewport adapts to
the available width via `pad_to` / `visible_width` but never
feeds back into the simulation.

## Frame cadence

`--frame-every-ticks` controls how often a frame paints. Default
50 ticks. `--tick-rate-ms` adds a real-time delay between ticks
for watchability. Both knobs are decorative and don't affect the
NDJSON ([`state.rs:289-300`](../sim/report/src/viewport/state.rs):
`should_render` only returns `true` on `Tick { phase: TickEnd }`
where `tick % frame_every == 0`, plus the terminal frame on
`RunEnd`).

## Log dedup

The scrolling log has per-event dedup latches on
`ViewportEmitter` so high-volume per-tick chatter doesn't drown
the 3-line strip ([`emitter.rs:79-118`](../sim/report/src/viewport/emitter.rs)):

| Latch | Effect |
|---|---|
| `wars_logged: BTreeSet<(winner, loser)>` | One `defeated` line per war pair. Cleared on `CivCollapsed` so a re-emerged civ_id can re-trigger. |
| `templates_confirmed_logged: BTreeSet<template_id>` | One `species confirmed` line per template (recognition firings span thousands per run). |
| `relations_falsified_logged` / `relations_lapsed_logged: BTreeSet<relation_id>` | First-of-kind only per relation. |
| `transmissions_logged: BTreeSet<(source, dest)>` | One `sharing knowledge` line per pair for the lifetime of the run. |
| `wars_active: BTreeSet<(min, max)>` | Mirrors core's active-war set; drives the sidebar's `at war ⚔` tag. |
| `civ_last_emitted_pop_q32: BTreeMap<civ_id, i128>` | Previous-frame pop snapshot for the per-civ trend arrow (±0.5% deadband). |
| `relation_template_names: BTreeMap<relation_id, String>` | Captured on the first `RelationConfirmed`; lets downstream log lines read as `lost "water flows downhill"` instead of `lost r438`. |

`log_message` runs **before** the state-mutation match in
`apply_state`
([`state.rs:24-64`](../sim/report/src/viewport/state.rs)) so
events that drop identifying state (notably `CivCollapsed`
removing the civ's name) don't strip it before the log line
captures it. Same-tick `CivContact` events coalesce into one
line — `"Karnan, Goran met Yothan"` instead of two separate
`"X met Y"` lines.
