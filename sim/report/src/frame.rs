//! Shared world-frame rendering: a single ASCII snapshot of the
//! planet with all-civs-at-a-moment overlaid. Two consumers feed
//! the same `render_world_frame` so the post-run keyframe maps
//! and the live `--cli=viewport` mode produce visually identical
//! frames:
//!
//! - **Keyframe report** (post-run): `digest::Digest` replays the
//!   event stream, capturing `WorldFrame` snapshots every K ticks,
//!   and the report stacks them vertically with year captions so a
//!   reader sees the spatial story of civs rising and falling over
//!   thousands of years.
//! - **Live viewport** (CLI option): a streaming consumer mirrors
//!   the same per-civ claim state from `CivFounded` /
//!   `CivTerritoryChanged` / `CivCollapsed` events as they arrive,
//!   re-rendering periodically to stdout.
//!
//! Both paths produce the same `WorldFrame` and call
//! `render_world_frame` — there is no second renderer to drift.

use crate::q32::{pop_q32_to_f64, q32_to_f64};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

/// One civ's territorial state at a single moment.
#[derive(Debug, Clone)]
pub struct CivClaim {
    pub civ_id: u32,
    /// Cells the civ claims at this moment. Order doesn't matter
    /// for rendering; a `BTreeSet` keeps lookup O(log n) and stays
    /// deterministic across replay vs. live consumers.
    pub claimed_cells: BTreeSet<u32>,
    /// Cell index of the founder's attention focus (territory
    /// centroid). Marked specially in the rendered frame so the
    /// civ's "capital" location reads at a glance.
    pub centroid: u32,
    /// Per-cell population, as Q96.32 `Pop` raw bits (matches the
    /// `CivTerritoryChanged` event payload). Empty for civs
    /// reconstructed from older event logs that pre-date the
    /// field; the density renderer treats empty as "not
    /// available, skip density map".
    pub cell_populations_q32: BTreeMap<u32, i128>,
    /// Per-cell carrying capacity, as Q96.32 `Pop` raw bits. Mirrors
    /// the `CivTerritoryChanged` event field of the same name.
    /// The colored viewport's pop-digit scale reads each cell's
    /// pop as a fraction of its own cap so digit `9` = saturated,
    /// `0` = nearly empty. Empty for civs reconstructed from
    /// older event logs that pre-date the field; the renderer
    /// falls back to a frame-relative max in that case.
    pub cell_capacities_q32: BTreeMap<u32, i128>,
}

/// Snapshot of all active civs on the planet at a single tick.
/// Inactive (collapsed) civs are dropped from `civs` — the frame
/// shows the world as it stands at `tick`.
#[derive(Debug, Clone)]
pub struct WorldFrame {
    pub tick: u64,
    pub civs: Vec<CivClaim>,
    /// Cells with nomadic species presence (no civ claim,
    /// pop > `NOMAD_DISPLAY_FLOOR_POP`). Rendered as `0` glyphs
    /// when no civ claim and no centroid override.
    pub nomad_cells: Vec<u32>,
}

/// Style flags for `render_world_frame_styled`. All-false +
/// `SurfacePhase::Earthlike` is the monochrome non-compact digit-
/// glyph default (`render_world_frame`).
#[derive(Debug, Clone, Copy, Default)]
pub struct FrameStyle {
    /// Emit 256-color ANSI escapes around per-civ identity symbols.
    /// Live viewport only; markdown reports skip this.
    pub use_color: bool,
    /// One character per cell, no hex-row offset — halves grid
    /// width so 24-wide grids fit on portrait phone terminals.
    pub compact: bool,
    /// Replace per-civ claim digits with Unicode block glyphs
    /// (░ ▒ ▓ █) sized by per-cell pop fill-%. Centroid letters,
    /// disputed `#`, and nomad markers are unchanged.
    pub density: bool,
    /// Coarse surface-physics state. `Earthlike` is the historical
    /// glyph set; `Lava` remaps every non-peak cell to `*`; `IceCap`
    /// remaps water cells to `+`. Callers compute this once per
    /// planet via `render::surface_phase`.
    pub phase: crate::render::SurfacePhase,
}

/// Render a `WorldFrame` as an ASCII grid. The output is a single
/// fenced code block with a column header, hex-offset rows, and
/// per-cell symbols chosen by overlay precedence:
///
/// 1. **Centroid** of a civ → uppercase letter (`A` = civ 1's
///    capital, `B` = civ 2's, …). Founder attention focus.
/// 2. **Claimed by exactly one civ** → in monochrome, the civ's
///    digit (`1`–`9`, then `*` for civs ≥ 10); in colored mode, a
///    linear pop-saturation digit `1`–`9` (see `pop_digit`) — or
///    the underlying terrain glyph in the civ's colour for cells
///    below 10% of cap (ownership by colour, "barely settled" by
///    the landform showing through).
/// 3. **Claimed by multiple civs** → `#` (territory dispute).
/// 4. **Unclaimed** → terrain symbol from `terrain_symbol`.
///
/// `caption` (if non-empty) is written above the grid as a single
/// line of context — the keyframe report uses it for "Year 1000"
/// captions; the viewport uses it for "tick T / civs N / pop M"
/// status.
pub fn render_world_frame(
    pm: &protocol::PlanetMap,
    planet: Option<&protocol::PlanetDerived>,
    frame: &WorldFrame,
    caption: &str,
) -> String {
    render_world_frame_styled(pm, planet, frame, caption, FrameStyle::default())
}

/// Style-parameterised renderer. `FrameStyle::default()` matches
/// `render_world_frame` exactly. Colored output emits 256-color
/// ANSI escapes around per-civ identity symbols, with each civ id
/// mapped to a stable 24-step palette colour — use only on
/// terminal sinks. Density mode swaps the claim-digit ladder for
/// Unicode block glyphs (░ ▒ ▓ █) sized by per-cell pop / cap.
pub fn render_world_frame_styled(
    pm: &protocol::PlanetMap,
    planet: Option<&protocol::PlanetDerived>,
    frame: &WorldFrame,
    caption: &str,
    style: FrameStyle,
) -> String {
    let FrameStyle {
        use_color,
        compact,
        density: density_mode,
        phase,
    } = style;
    let mut s = String::new();
    if pm.grid_width == 0 || pm.grid_height == 0 {
        return s;
    }
    let terrain_peak = planet.map_or(0.0, |p| q32_to_f64(p.terrain_peak_q32));
    let w = pm.grid_width as usize;
    let h = pm.grid_height as usize;

    // Per-cell ownership tally: how many civs claim this cell, and
    // (if exactly one) which civ. Centroids tracked separately so
    // the founder marker overrides the digit on the centroid cell.
    // `civ_by_id` lets the colored render branch look up per-cell
    // population (via `cell_populations_q32`) when picking the
    // pop-scaled digit for a claimed cell.
    let mut owners: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    let mut centroids: BTreeMap<u32, u32> = BTreeMap::new();
    let mut civ_by_id: BTreeMap<u32, &CivClaim> = BTreeMap::new();
    // Frame-wide densest civ-claimed cell, used as a fallback
    // when per-cell capacity data is unavailable (older event
    // logs without `cell_capacities_q32`). The primary scale uses
    // each cell's own cap so digit 9 means "saturated".
    let mut frame_max_pop: f64 = 0.0;
    for civ in &frame.civs {
        civ_by_id.insert(civ.civ_id, civ);
        // `frame_max_pop` is the fallback cap when per-cell caps
        // are missing (older event logs); both colored pop-digit
        // mode and density mode lean on it, so accumulate when
        // either is active.
        if use_color || density_mode {
            for &raw in civ.cell_populations_q32.values() {
                let p = pop_q32_to_f64(raw);
                if p > frame_max_pop {
                    frame_max_pop = p;
                }
            }
        }
        // When two civs share a centroid cell (succession spawns
        // civ 2 with parent_civ_id=1, both compute their smallest
        // claimed cell as centroid → collision), prefer the *older*
        // civ's letter via `or_insert`. Older-wins keeps civ 1's
        // `A` visible at the contested capital; civ 2's territory
        // and centroid contention are still readable from the
        // surrounding `#` cells.
        centroids.entry(civ.centroid).or_insert(civ.civ_id);
        for &cell in &civ.claimed_cells {
            owners.entry(cell).or_default().push(civ.civ_id);
        }
    }
    // Nomad cells (species presence outside any civ).
    // Set lookup is O(log n) per cell, so a BTreeSet works well at
    // grid sizes up to a few thousand cells.
    let nomad_set: BTreeSet<u32> = frame.nomad_cells.iter().copied().collect();

    let _ = writeln!(s, "```text");
    if !caption.is_empty() {
        let _ = writeln!(s, "{caption}");
        let _ = writeln!(s);
    }
    // Compact mode uses 1-char-per-cell columns and no
    // hex-row offset (square-grid look) so larger sim grids
    // still fit on portrait phone terminals. Standard mode is
    // 2-char-per-cell with the hex offset for visual fidelity.
    let cell_width = if compact { 1 } else { 2 };
    let row_prefix_pad = if compact { 3 } else { 4 };
    // Compact mode column header.
    //
    // Just the ones digit is shown; at 24 cols a viewer can count
    // by 10s themselves, so a tens row would only be vertical
    // bloat. Columns 10..19 read as `0..9` again, columns 20..23
    // as `0..3`.
    //
    // Standard mode keeps its existing single-row dense header.
    if compact {
        let mut h_ones = " ".repeat(row_prefix_pad);
        for q in 0..w {
            h_ones.push(std::char::from_digit((q as u32) % 10, 10).unwrap_or(' '));
        }
        let _ = writeln!(s, "{h_ones}");
        // Horizontal axis separator between the column
        // header and the grid body. `+` corner sits at column
        // `row_prefix_pad - 1` (where the per-row `|` sits below),
        // then a dash per cell column, then a closing `+` on the
        // right — completing the top of the map's box.
        let mut axis = " ".repeat(row_prefix_pad.saturating_sub(1));
        axis.push('+');
        for _ in 0..w {
            axis.push('-');
        }
        axis.push('+');
        let _ = writeln!(s, "{axis}");
    } else {
        let mut header = " ".repeat(row_prefix_pad);
        for q in 0..w {
            let _ = write!(header, "{q:>cell_width$}");
        }
        let _ = writeln!(s, "{header}");
    }
    for r in 0..h {
        // Compact-only: row prefix carries `|` instead of
        // a trailing space, so each grid row reads as
        // `{row}|{cells}` and the | column lines up with the
        // horizontal axis's `+` corner emitted above the grid.
        let mut line = if compact {
            format!("{r:>2}|")
        } else {
            format!("{r:>2}  ")
        };
        if !compact && r % 2 == 1 {
            line.push(' ');
        }
        for q in 0..w {
            let cell = (r * w + q) as u32;
            // Centroid lookup runs *before* the dispute
            // check, so a contested capital still renders as a
            // letter (the older civ's, via the
            // `entry().or_insert()` ordering above). Surrounding
            // non-centroid contested cells still render as `#`,
            // so the dispute is visible at the borders without
            // erasing the capital.
            let owners_here = owners.get(&cell);
            // Filter out civ_id 0 entries — those leak through from
            // stateless-cohort or successor-handoff state in some
            // event sequences and would otherwise render as `?`
            // (claim_symbol's fallback). Treat them as unowned so
            // the terrain glyph shows through.
            let active_owners: Option<Vec<u32>> =
                owners_here.map(|c| c.iter().copied().filter(|&id| id > 0).collect());
            let active_centroid = centroids.get(&cell).copied().filter(|&id| id > 0);
            // Per-cell ANSI attribute prefix for civ-coloured cells.
            // Density mode encodes pop/cap in colour intensity (bold
            // ≥ 60%, normal ≥ 30%, dim < 30%) so the underlying
            // terrain glyph stays readable. Centroids and non-density
            // modes always render bold so the capital letter / digit
            // pops above the terrain.
            let mut civ_attr: &str = "1;";
            let (symbol, color_civ) = if let Some(civ_id) = active_centroid {
                (centroid_symbol(civ_id), Some(civ_id))
            } else if active_owners.as_ref().is_some_and(|c| c.len() > 1) {
                ('#', None)
            } else if let Some(claims) = active_owners.as_ref().filter(|c| !c.is_empty()) {
                let civ_id = claims[0];
                // Density mode shows the underlying terrain glyph in
                // the civ's colour with brightness scaled by
                // population so a reader sees both *what's there*
                // (land/coast/peak/plain) and *how settled it is*
                // (bold vs normal vs dim). Centroid letters still
                // mark capitals. Without density mode the
                // cell-rendering branches stay on their existing
                // per-mode logic — pop_digit on colored, claim_symbol
                // on mono.
                let symbol = if density_mode {
                    let claim = civ_by_id.get(&civ_id);
                    let pop = claim
                        .and_then(|c| c.cell_populations_q32.get(&cell).copied())
                        .map(pop_q32_to_f64)
                        .unwrap_or(0.0);
                    let cap = claim
                        .and_then(|c| c.cell_capacities_q32.get(&cell).copied())
                        .map(pop_q32_to_f64)
                        .filter(|c| *c > 0.0)
                        .unwrap_or(frame_max_pop);
                    let ratio = if cap > 0.0 { (pop / cap).clamp(0.0, 1.0) } else { 0.0 };
                    civ_attr = if ratio >= 0.60 {
                        "1;"
                    } else if ratio >= 0.30 {
                        ""
                    } else {
                        "2;"
                    };
                    crate::render::terrain_symbol(pm, r, q, terrain_peak, phase)
                } else if use_color {
                    // Colored mode: civ identity is conveyed by
                    // colour, so the digit is freed up to encode
                    // per-cell population on a linear scale
                    // (`pop_digit`). Cells under 10% of cap return
                    // `None` from `pop_digit`; fall back to the
                    // terrain glyph so a barely-settled claim reads
                    // as the civ's colour spreading across the
                    // landform (▒/·/△) rather than as a `0`.
                    let claim = civ_by_id.get(&civ_id);
                    let pop = claim
                        .and_then(|c| c.cell_populations_q32.get(&cell).copied())
                        .map(pop_q32_to_f64)
                        .unwrap_or(0.0);
                    let cap = claim
                        .and_then(|c| c.cell_capacities_q32.get(&cell).copied())
                        .map(pop_q32_to_f64)
                        .filter(|c| *c > 0.0)
                        // Fallback for older event logs that don't
                        // carry per-cell caps: use the frame's
                        // densest cell as the scale's `9`. Less
                        // semantically meaningful than per-cell
                        // saturation but keeps digits readable.
                        .unwrap_or(frame_max_pop);
                    pop_digit(pop, cap)
                        .unwrap_or_else(|| crate::render::terrain_symbol(pm, r, q, terrain_peak, phase))
                } else {
                    // Monochrome mode (markdown post-run report):
                    // keep the civ-id digit so civs are still
                    // distinguishable when ANSI colours aren't
                    // rendered.
                    claim_symbol(civ_id)
                };
                (symbol, Some(civ_id))
            } else if nomad_set.contains(&cell) {
                // Nomadic species presence (no civ claim). In
                // colored mode render the underlying terrain glyph
                // (the colour-emission branch tints it bright white
                // so nomads pop against terrain while still showing
                // *where* they're roaming). In mono / markdown mode
                // there's no colour to carry identity, so keep the
                // legacy `0` glyph as the nomad marker.
                let sym = if use_color {
                    crate::render::terrain_symbol(pm, r, q, terrain_peak, phase)
                } else {
                    '0'
                };
                (sym, None)
            } else {
                (crate::render::terrain_symbol(pm, r, q, terrain_peak, phase), None)
            };
            if use_color {
                if let Some(civ_id) = color_civ {
                    let _ = write!(
                        line,
                        "\x1b[{}38;5;{}m{}\x1b[0m",
                        civ_attr,
                        civ_color_code(civ_id),
                        symbol
                    );
                } else if nomad_set.contains(&cell) && active_centroid.is_none() {
                    // Nomadic species presence has no civ identity,
                    // so it doesn't get a civ palette colour, but it
                    // also shouldn't fade into the muted terrain
                    // palette. Render the terrain glyph in bold
                    // bright white (256-col index 15) so a nomad
                    // marker reads as "people here, no flag" while
                    // still showing what landscape they're on.
                    let _ = write!(line, "\x1b[1;38;5;15m{symbol}\x1b[0m");
                } else if let Some(tcolor) = terrain_color_code(symbol) {
                    // Terrain glyphs get their own
                    // 256-color codes when `use_color` is on,
                    // so water reads blue, mountains brown, land
                    // green, etc. The civ overlay (above) keeps
                    // its existing palette and uses bold so the
                    // markers pop above the terrain.
                    let _ = write!(line, "\x1b[38;5;{tcolor}m{symbol}\x1b[0m");
                } else {
                    line.push(symbol);
                }
            } else {
                line.push(symbol);
            }
            // Skip the trailing space in compact mode so each
            // cell is exactly 1 character.
            if !compact {
                line.push(' ');
            }
        }
        // Compact-only: right `|` border closes the box on
        // the right side of the grid. Combined with the top
        // `+----` and the bottom `+----` written below,
        // the map sits inside a fully-bounded ASCII rectangle.
        if compact {
            line.push('|');
        }
        let _ = writeln!(s, "{line}");
    }
    // Compact-only: bottom horizontal axis matching the top.
    // `+` corner at column `row_prefix_pad - 1`, then a dash per
    // cell column, then `+` at the right.
    if compact {
        let mut bottom = " ".repeat(row_prefix_pad.saturating_sub(1));
        bottom.push('+');
        for _ in 0..w {
            bottom.push('-');
        }
        bottom.push('+');
        let _ = writeln!(s, "{bottom}");
    }
    let _ = writeln!(s, "```");
    s
}

/// 256-color code for a terrain glyph in the compact map.
/// Picked to read as a plausible natural palette: blue for
/// water, green for land, brown for mountains, white for peaks,
/// muted yellow for gas-giant cloud bands, neutral gray for
/// featureless surface. Returns `None` for glyphs that should
/// stay terminal-default (claim digits, disputes, blanks). Civ
/// markers go through `civ_color_code` separately and are bolded.
pub(crate) fn terrain_color_code(c: char) -> Option<u8> {
    match c {
        '~' => Some(39),         // shallow water — sky blue
        '\u{2248}' => Some(27),  // ≈ deep water — deep blue
        '\u{25B2}' => Some(15),  // ▲ peak — bright white
        '\u{25B3}' => Some(94),  // △ low mountain — brown
        '\u{2592}' => Some(34),  // ▒ inland — forest green
        '\u{2591}' => Some(143), // ░ coastal — sand / khaki
        '\u{2261}' => Some(222), // ≡ gas band — light yellow
        '\u{00B7}' => Some(244), // · featureless — gray
        '*' => Some(208),        // magma — orange (silicate hot world)
        '+' => Some(159),        // ice sheet — light cyan (frozen aqueous)
        _ => None,
    }
}

/// Map a civ id to a stable 256-color palette index. 24 distinct
/// hues spread across the 6×6×6 colour cube so adjacent civs
/// pop visually. Civ ids cycle through the palette modulo 24.
pub(crate) fn civ_color_code(civ_id: u32) -> u8 {
    // 24 chosen colours from the xterm 256-colour palette,
    // weighted toward bright distinguishable hues.
    const PALETTE: [u8; 24] = [
        196, 202, 220, 154, 46, 47, 48, 51, 33, 27, 21, 57, 93, 129, 165, 201, 199, 198, 226, 190,
        118, 82, 39, 75,
    ];
    PALETTE[(civ_id.saturating_sub(1) as usize) % PALETTE.len()]
}

/// Capital marker for civ N: uppercase letter cycling A..Z.
/// Distinct from the territory digit so a reader can pick out
/// each civ's capital at a glance.
pub(crate) fn centroid_symbol(civ_id: u32) -> char {
    let idx = (civ_id.saturating_sub(1)) % 26;
    (b'A' + idx as u8) as char
}

/// Map a per-cell population to a single digit 1..9 on a linear
/// scale relative to the cell's own carrying capacity. Used by
/// the colored frame variants — each civ's identity is carried
/// by colour, so the digit is freed up to encode "how full is
/// this cell" at a glance:
///
/// - digit `9` ≈ ≥90% of cap (saturated)
/// - digit `5` ≈ 50–60% of cap
/// - digit `1` ≈ 10–20% of cap
/// - `None` ≈ < 10% of cap (caller renders the terrain glyph
///   in civ colour — ownership by colour, "barely settled" by
///   the landform showing through).
///
/// Each civ's cap depends on tech, terrain, season, and
/// biosphere — so as a civ unlocks better food tools the cap
/// rises and previously-saturated cells naturally fall to lower
/// digits, telling a "growth headroom recovered" story without
/// the renderer needing to know the cap formula directly.
///
/// `cap` ≤ 0 means cap data is missing (older event logs); the
/// caller should pass a frame-relative max-pop fallback, which
/// degrades the scale to "this cell vs. densest cell in the
/// frame" while keeping digits readable.
pub(crate) fn pop_digit(pop: f64, cap: f64) -> Option<char> {
    if pop <= 0.0 || cap <= 0.0 {
        return None;
    }
    // Linear saturation: each digit = 10% of cap. Above-cap pops
    // (rare overshoot of the logistic step) clamp to 9. Cells under
    // 10% of cap return `None` so the renderer falls back to the
    // terrain glyph (in civ colour) — frontier claims read as the
    // civ's colour spreading across the landscape rather than as
    // a sea of indistinguishable `0`s.
    let ratio = (pop / cap).clamp(0.0, 1.0);
    let bucket = (ratio * 10.0).floor() as i32;
    if bucket < 1 {
        return None;
    }
    Some((b'0' + bucket.min(9) as u8) as char)
}

/// Territory marker for civ N: digit 1..9 then `*` for civs ≥ 10.
/// Matches the existing per-civ territory map's convention.
/// Used in monochrome rendering (post-run markdown report); the
/// colored viewport path uses `pop_digit` instead since civ
/// identity is already carried by colour there.
pub(crate) fn claim_symbol(civ_id: u32) -> char {
    if civ_id == 0 {
        '?'
    } else if civ_id < 10 {
        (b'0' + civ_id as u8) as char
    } else {
        '*'
    }
}

/// Density-shading symbol from a normalised cell-pop ratio.
/// Five buckets matched to the Unicode block-character ladder so a
/// reader sees relative density at a glance: dense capital cells
/// pop, frontier cells fade into the terrain.
fn density_symbol(ratio: f64) -> char {
    if ratio >= 0.85 {
        '\u{2588}' // █
    } else if ratio >= 0.6 {
        '\u{2593}' // ▓
    } else if ratio >= 0.35 {
        '\u{2592}' // ▒
    } else if ratio > 0.0 {
        '\u{2591}' // ░
    } else {
        ' '
    }
}

/// Render a `WorldFrame` as a density map: each claimed cell
/// shaded by its cohort population relative to the densest cell
/// across all civs in the frame. Centroid letters preserve civ
/// identity at the capitals; the rest of the territory shows
/// density via Unicode block characters (` ░ ▒ ▓ █`). Unclaimed
/// cells stay as terrain symbols.
///
/// Returns an empty string if no civ has per-cell population data
/// (older event logs); callers should detect and skip rendering
/// the density map in that case.
pub fn render_density_frame(
    pm: &protocol::PlanetMap,
    planet: Option<&protocol::PlanetDerived>,
    frame: &WorldFrame,
    caption: &str,
    phase: crate::render::SurfacePhase,
) -> String {
    let mut s = String::new();
    if pm.grid_width == 0 || pm.grid_height == 0 {
        return s;
    }
    // Find max pop across all (civ, cell) pairs to normalise.
    let mut max_pop = 0.0_f64;
    let mut have_data = false;
    for civ in &frame.civs {
        for &raw in civ.cell_populations_q32.values() {
            have_data = true;
            let count = pop_q32_to_f64(raw);
            if count > max_pop {
                max_pop = count;
            }
        }
    }
    if !have_data || max_pop <= 0.0 {
        return s;
    }
    let terrain_peak = planet.map_or(0.0, |p| q32_to_f64(p.terrain_peak_q32));
    let w = pm.grid_width as usize;
    let h = pm.grid_height as usize;

    let mut centroids: BTreeMap<u32, u32> = BTreeMap::new();
    let mut cell_pop: BTreeMap<u32, f64> = BTreeMap::new();
    for civ in &frame.civs {
        centroids.insert(civ.centroid, civ.civ_id);
        for (&cell, &raw) in &civ.cell_populations_q32 {
            let entry = cell_pop.entry(cell).or_insert(0.0);
            *entry += pop_q32_to_f64(raw);
        }
    }

    let _ = writeln!(s, "```text");
    if !caption.is_empty() {
        let _ = writeln!(s, "{caption}");
        let _ = writeln!(s);
    }
    let mut header = String::from("    ");
    for q in 0..w {
        let _ = write!(header, "{q:>2}");
    }
    let _ = writeln!(s, "{header}");
    for r in 0..h {
        let mut line = format!("{r:>2}  ");
        if r % 2 == 1 {
            line.push(' ');
        }
        for q in 0..w {
            let cell = (r * w + q) as u32;
            let symbol = if let Some(&civ_id) = centroids.get(&cell) {
                centroid_symbol(civ_id)
            } else if let Some(&pop) = cell_pop.get(&cell) {
                density_symbol(pop / max_pop)
            } else {
                crate::render::terrain_symbol(pm, r, q, terrain_peak, phase)
            };
            line.push(symbol);
            line.push(' ');
        }
        let _ = writeln!(s, "{line}");
    }
    let _ = writeln!(s, "```");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pm(w: u32, h: u32) -> protocol::PlanetMap {
        let n = (w as usize) * (h as usize);
        protocol::PlanetMap {
            grid_width: w,
            grid_height: h,
            elevation_q32: vec![0; n],
            water_depth_q32: vec![0; n],
        }
    }

    /// Strip the column header + row prefixes so assertions can focus
    /// on cell content. Returns the joined body cells without
    /// row-axis numbers or alignment whitespace.
    fn body_cells(s: &str) -> String {
        let mut out = String::new();
        for line in s.lines().skip_while(|l| !l.starts_with("    ")).skip(1) {
            if line.starts_with("```") {
                break;
            }
            // Each row line starts with "{row:>2}  " (4 chars), then
            // an optional space for odd-row indent. Skip the prefix.
            if line.len() < 4 {
                continue;
            }
            out.push_str(&line[4..]);
        }
        out
    }

    #[test]
    fn empty_frame_renders_terrain_only() {
        let frame = WorldFrame {
            tick: 0,
            civs: vec![],
            nomad_cells: vec![],
        };
        let s = render_world_frame(&pm(4, 3), None, &frame, "");
        assert!(s.contains("```text"));
        let body = body_cells(&s);
        assert!(!body.contains('A'));
        assert!(!body.contains('1'));
    }

    #[test]
    fn single_civ_centroid_renders_as_letter_a() {
        let frame = WorldFrame {
            tick: 100,
            civs: vec![CivClaim {
                civ_id: 1,
                claimed_cells: BTreeSet::from([0, 1, 2]),
                centroid: 0,
                cell_populations_q32: BTreeMap::new(),
                cell_capacities_q32: BTreeMap::new(),
            }],
            nomad_cells: vec![],
        };
        let s = render_world_frame(&pm(4, 3), None, &frame, "Year 100");
        assert!(s.contains("Year 100"));
        let body = body_cells(&s);
        assert!(body.contains('A'));
        assert!(body.contains('1'));
    }

    #[test]
    fn overlapping_claims_render_as_hash() {
        let frame = WorldFrame {
            tick: 200,
            civs: vec![
                CivClaim {
                    civ_id: 1,
                    claimed_cells: BTreeSet::from([0, 5]),
                    centroid: 0,
                    cell_populations_q32: BTreeMap::new(),
                    cell_capacities_q32: BTreeMap::new(),
                },
                CivClaim {
                    civ_id: 2,
                    claimed_cells: BTreeSet::from([5, 6]),
                    centroid: 6,
                    cell_populations_q32: BTreeMap::new(),
                    cell_capacities_q32: BTreeMap::new(),
                },
            ],
            nomad_cells: vec![],
        };
        let s = render_world_frame(&pm(4, 3), None, &frame, "");
        let body = body_cells(&s);
        assert!(body.contains('#'));
        assert!(body.contains('A'));
        assert!(body.contains('B'));
    }

    #[test]
    fn pop_digit_saturation_buckets() {
        // pop / cap = 1.0 → 9 (saturated)
        assert_eq!(pop_digit(100.0, 100.0), Some('9'));
        // 10× over cap clamps at 9
        assert_eq!(pop_digit(1_000.0, 100.0), Some('9'));
        // 95% of cap → 9
        assert_eq!(pop_digit(95.0, 100.0), Some('9'));
        // 80% of cap → 8
        assert_eq!(pop_digit(80.0, 100.0), Some('8'));
        // 50% of cap → 5
        assert_eq!(pop_digit(50.0, 100.0), Some('5'));
        // 25% of cap → 2
        assert_eq!(pop_digit(25.0, 100.0), Some('2'));
        // 10% of cap → 1 (exactly on the 10% boundary)
        assert_eq!(pop_digit(10.0, 100.0), Some('1'));
        // Under 10% → None (caller renders the terrain glyph
        // in civ colour instead of a digit).
        assert_eq!(pop_digit(9.0, 100.0), None);
        assert_eq!(pop_digit(1.0, 100.0), None);
        assert_eq!(pop_digit(0.1, 100.0), None);
        // pop=0 → None regardless of cap
        assert_eq!(pop_digit(0.0, 1_000.0), None);
        // cap missing/zero → None
        assert_eq!(pop_digit(50.0, 0.0), None);
    }

    #[test]
    fn colored_render_uses_pop_digits_relative_to_cap() {
        // Civ 1 owns three cells. Cap is the same across cells;
        // pop varies. Colored mode shows each cell's saturation
        // digit (9 = at cap). Cell 0 is the centroid → letter `A`.
        // Cell 2 sits below the 10% floor, so its digit slot falls
        // back to the terrain glyph rendered in the civ's colour —
        // ownership by colour, "barely settled" by the landform.
        let cap = 1_000.0_f64;
        let scale = (1u64 << 32) as f64;
        let mut pops = BTreeMap::new();
        pops.insert(0u32, (cap * scale) as i128); // centroid; digit irrelevant
        pops.insert(1u32, (cap * scale) as i128); // saturated → '9'
        pops.insert(2u32, (1.0 * scale) as i128); // 0.1% of cap → terrain
        let mut caps = BTreeMap::new();
        caps.insert(0u32, (cap * scale) as i128);
        caps.insert(1u32, (cap * scale) as i128);
        caps.insert(2u32, (cap * scale) as i128);
        let frame = WorldFrame {
            tick: 100,
            civs: vec![CivClaim {
                civ_id: 1,
                claimed_cells: BTreeSet::from([0, 1, 2]),
                centroid: 0,
                cell_populations_q32: pops,
                cell_capacities_q32: caps,
            }],
            nomad_cells: vec![],
        };
        let s = render_world_frame_styled(
            &pm(4, 3),
            None,
            &frame,
            "",
            FrameStyle {
                use_color: true,
                ..Default::default()
            },
        );
        assert!(s.contains('A'));
        // Civ-color escape applied to the saturated `9`.
        let civ_color = civ_color_code(1);
        assert!(s.contains(&format!("\x1b[1;38;5;{civ_color}m9")));
        // Sparse cell renders as a civ-colored terrain glyph
        // (depth=0, elevation=0, terrain_peak=0 on the test
        // PlanetMap → `≡` gas-band from `terrain_symbol`).
        assert!(s.contains(&format!("\x1b[1;38;5;{civ_color}m\u{2261}")));
        // The under-10% bucket no longer emits a `0` digit for the
        // civ — only the terrain-glyph fallback.
        assert!(!s.contains(&format!("\x1b[1;38;5;{civ_color}m0")));
    }

    #[test]
    fn density_mode_keeps_terrain_glyph_and_varies_civ_color_intensity() {
        // In density mode the per-cell digit ladder (1-9) is
        // replaced by the underlying terrain glyph painted in the
        // civ's colour with brightness scaled by pop / cap (bold ≥
        // 60%, normal ≥ 30%, dim < 30%). The reader sees both
        // *what's there* (terrain) and *how settled it is*
        // (intensity). Centroid letters still mark capitals.
        let cap = 1_000.0_f64;
        let scale = (1u64 << 32) as f64;
        let mut pops = BTreeMap::new();
        pops.insert(0u32, (cap * scale) as i128); // centroid 100%
        pops.insert(1u32, (cap * scale) as i128); // 100% → bold civ colour
        pops.insert(2u32, (0.4 * cap * scale) as i128); // 40% → normal civ colour
        pops.insert(3u32, (0.1 * cap * scale) as i128); // 10% → dim civ colour
        let mut caps = BTreeMap::new();
        caps.insert(0u32, (cap * scale) as i128);
        caps.insert(1u32, (cap * scale) as i128);
        caps.insert(2u32, (cap * scale) as i128);
        caps.insert(3u32, (cap * scale) as i128);
        let frame = WorldFrame {
            tick: 100,
            civs: vec![CivClaim {
                civ_id: 1,
                claimed_cells: BTreeSet::from([0, 1, 2, 3]),
                centroid: 0,
                cell_populations_q32: pops,
                cell_capacities_q32: caps,
            }],
            nomad_cells: vec![],
        };
        let s = render_world_frame_styled(
            &pm(4, 3),
            None,
            &frame,
            "",
            FrameStyle {
                density: true,
                use_color: true,
                ..Default::default()
            },
        );
        let civ_color = civ_color_code(1);
        assert!(s.contains('A'), "centroid letter still present");
        // Test grid has elev=0, depth=0, no planet metadata; the
        // terrain symbol for these cells is `≡` (gas band).
        let terrain = '\u{2261}';
        // 100% cell → bold civ colour on terrain glyph
        assert!(
            s.contains(&format!("\x1b[1;38;5;{civ_color}m{terrain}")),
            "100% cell renders as bold civ-colour terrain glyph"
        );
        // 40% cell → normal civ colour on terrain glyph
        assert!(
            s.contains(&format!("\x1b[38;5;{civ_color}m{terrain}")),
            "40% cell renders as normal civ-colour terrain glyph"
        );
        // 10% cell → dim civ colour on terrain glyph
        assert!(
            s.contains(&format!("\x1b[2;38;5;{civ_color}m{terrain}")),
            "10% cell renders as dim civ-colour terrain glyph"
        );
        // No block-shade glyphs should appear from the per-cell
        // rendering path now that density is conveyed by intensity.
        assert!(!s.contains('\u{2588}'), "no █ from claimed cells");
        assert!(!s.contains('\u{2593}'), "no ▓ from claimed cells");
        // No pop-digits should leak through in density mode.
        assert!(!s.contains(" 9 "), "density mode shouldn't emit `9` digits");
        assert!(!s.contains(" 5 "), "density mode shouldn't emit `5` digits");
    }

    #[test]
    fn civ_id_above_nine_renders_as_star() {
        let frame = WorldFrame {
            tick: 0,
            civs: vec![CivClaim {
                civ_id: 12,
                claimed_cells: BTreeSet::from([1, 2]),
                centroid: 0,
                cell_populations_q32: BTreeMap::new(),
                cell_capacities_q32: BTreeMap::new(),
            }],
            nomad_cells: vec![],
        };
        let s = render_world_frame(&pm(4, 3), None, &frame, "");
        let body = body_cells(&s);
        assert!(body.contains('L'));
        assert!(body.contains('*'));
    }
}
