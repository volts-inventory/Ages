//! ANSI escape sequences + line-formatting helpers used by the
//! live viewport frame writer. Width / centering helpers also live
//! here since they're shared between the divider routines and the
//! frame-content writers.

use std::io::Write;

/// ANSI: enter alternate-screen buffer. Saved+restored by the
/// terminal so the user's scrollback isn't polluted.
pub(super) const ANSI_ALT_SCREEN_ON: &str = "\x1b[?1049h";
/// ANSI: leave alternate-screen buffer. Restores previous scrollback.
pub(super) const ANSI_ALT_SCREEN_OFF: &str = "\x1b[?1049l";
/// Home cursor + full-screen clear.
///
/// An earlier diff-based rendering attempt (track previous
/// frame, emit cursor-positioned writes only for changed rows)
/// gave flicker-free behaviour on most terminals, but on
/// Termius the absolute `\x1b[N;1H` cursor-positioning combined
/// with the terminal's tendency to scroll past the visible-rows
/// boundary left stale rows from earlier frames visible alongside
/// new content (duplicate captions, leftover grid rows). The
/// current approach reverts to atomic clear-and-paint: each
/// frame begins with home + full-screen clear, packed into a
/// single atomic Vec<u8> write so the terminal sees the clear
/// followed by the body in a single chunk and paints it as one
/// pass. Less elegant than diff-render, but reliable across
/// every alt-screen-supporting terminal we've tested.
/// Per-row absolute cursor positioning. Each frame writes its
/// content via `\x1b[<row>;1H<line>\x1b[K` per row, never via
/// newlines, so the terminal never scrolls. After the last row
/// we emit `\x1b[J` (erase from cursor to end of screen) to
/// wipe any rows from a previously-taller frame.
pub(super) const ANSI_ERASE_TO_END: &str = "\x1b[J";
/// ANSI erase to end of line. Appended after every body
/// line write so old characters from the previous (longer) frame
/// don't bleed through where the new frame is shorter. Kept
/// alongside the full-screen clear as cheap safety against
/// terminals that don't fully honour `\x1b[2J`.
pub(super) const ANSI_ERASE_LINE: &str = "\x1b[K";
/// ANSI: hide cursor (avoids blink on the moving frame).
pub(super) const ANSI_HIDE_CURSOR: &str = "\x1b[?25l";
/// ANSI: show cursor (paired with the hide on shutdown).
pub(super) const ANSI_SHOW_CURSOR: &str = "\x1b[?25h";

/// Visual width of the map zone in the side-by-side layout. A
/// `w`-cell-wide compact grid renders as `{r:>2}|` (3-char row
/// prefix) + w cells + `|` right border = `w + 4` cols. With the
/// default grid at 36×30, `MAP_WIDTH = 40`. Used to pad map rows
/// so the vertical rule lines up at `MAP_WIDTH + 1`.
pub(super) const MAP_WIDTH: usize = 40;
/// Visual width of the right-hand sidebar zone. 30 cols fits the
/// legend lines (`#=war · ~sea · ≈deep` ≈ 19 chars) with margin
/// and per-civ panel headers (`─── {Civ name} ───` ≈ 22 chars
/// typical) without truncation. Combined with `MAP_WIDTH + GAP`
/// = 40 + 3 = 43 the total lands at 73; we round up to
/// `VIEWPORT_WIDTH = 74` for divider-math comfort.
pub(super) const SIDEBAR_WIDTH: usize = 30;
/// Total visual width of the viewport. The viewport is laid out
/// in a side-by-side layout — map zone left, vertical `|` rule,
/// sidebar right — so the total is `MAP_WIDTH + 3 (gap) +
/// SIDEBAR_WIDTH`. With the default 36×30 grid, `MAP_WIDTH` is 40
/// and the total is 73; rounded up to 74 so section divider
/// arithmetic (the centred label inside `--- label ---`) lands on
/// even-ish boundaries. Planet section + log section span the
/// full 74-col width; the middle section splits map (cols 0–39)
/// and sidebar (cols 43–72) with the `|` rule at col 41.
pub(super) const VIEWPORT_WIDTH: usize = 74;

/// Format a full-width section divider — `--- {label} ---` style
/// with the label centered and dashes filling out to
/// `VIEWPORT_WIDTH`. Used for the planet section header (which
/// spans the full ~70-col width). A lighter `· label ·` form for
/// non-map sections was tried briefly and reverted — the
/// consistent heavy divider reads better, and the map still stands
/// out via its full box + blank-line isolation + (in color mode)
/// bold-vs-dim brightness contrast.
/// Returns the formatted divider including a trailing newline.
pub(super) fn divider(label: &str) -> String {
    let labelled = format!(" {label} ");
    let dashes = VIEWPORT_WIDTH.saturating_sub(labelled.chars().count());
    let lhs = dashes / 2;
    let rhs = dashes - lhs;
    format!("{}{}{}\n", "-".repeat(lhs), labelled, "-".repeat(rhs))
}

/// Split section divider — two centred labels separated by
/// a `+` corner at `RULE_COL`. Used at the top and bottom of the
/// middle section so the vertical `|` rule connects through the
/// dashes cleanly. Layout per side: left half is `--- {left} ---`
/// padded to `MAP_WIDTH + 1` chars (the space-before-`|` slot
/// joins the dashes); right half is `--- {right} ---` padded to
/// `SIDEBAR_WIDTH + 1` chars (matching space-after-`|`). Either
/// label may be empty (`""`), which yields a pure-dash run on
/// that side — used for the `--- log ---+---` divider where the
/// right side has no separate label but the `+` corner still
/// closes the rule.
pub(super) fn split_divider(left: &str, right: &str) -> String {
    fn fmt_half(label: &str, width: usize) -> String {
        if label.is_empty() {
            return "-".repeat(width);
        }
        let labelled = format!(" {label} ");
        let dashes = width.saturating_sub(labelled.chars().count());
        let lhs = dashes / 2;
        let rhs = dashes - lhs;
        format!("{}{}{}", "-".repeat(lhs), labelled, "-".repeat(rhs))
    }
    let left_w = MAP_WIDTH + 1;
    let right_w = SIDEBAR_WIDTH + 1;
    format!("{}+{}\n", fmt_half(left, left_w), fmt_half(right, right_w))
}

/// Pad a visible-width string with trailing spaces to
/// exactly `width` columns. Used to align map rows and sidebar
/// rows under their respective zone widths so the `|` rule lands
/// at `RULE_COL` regardless of row content. ANSI escapes don't
/// count toward visible width (see `visible_width`).
pub(super) fn pad_to(line: &str, width: usize) -> String {
    let vis = visible_width(line);
    if vis >= width {
        return line.to_string();
    }
    let mut s = String::with_capacity(line.len() + (width - vis));
    s.push_str(line);
    for _ in 0..(width - vis) {
        s.push(' ');
    }
    s
}

/// Center `line` within `width` by splitting padding left + right.
/// Visible-width aware so ANSI-colored lines center on visible cols.
/// Used by the planet card so each line of stats sits in the middle
/// of the left zone instead of left-aligned against the divider.
pub(super) fn center_to(line: &str, width: usize) -> String {
    let vis = visible_width(line);
    if vis >= width {
        return line.to_string();
    }
    let total_pad = width - vis;
    let left = total_pad / 2;
    let right = total_pad - left;
    let mut s = String::with_capacity(line.len() + total_pad);
    for _ in 0..left {
        s.push(' ');
    }
    s.push_str(line);
    for _ in 0..right {
        s.push(' ');
    }
    s
}

/// Count the *visible* columns a line occupies, ignoring
/// ANSI escape sequences. The naive `chars().count()` counts
/// every Rust char including the bytes inside escape
/// sequences like `\x1b[1;38;5;196m`, which inflated the width
/// for any line carrying colored cells (the map-grid rows with
/// civ markers, terrain colors). Result: per-line centering
/// padded those rows less than terrain-only rows, so the grid
/// rows shifted left of the header / axis. The fix is to walk
/// the line and skip everything between `\x1b` and the
/// terminator letter when counting.
pub(super) fn visible_width(line: &str) -> usize {
    let mut count = 0usize;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // ANSI escape: skip CSI parameter chars + intermediate
            // bytes until a letter (the final byte) terminates it.
            for terminator in chars.by_ref() {
                if terminator.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            count += 1;
        }
    }
    count
}

/// Write a body line centered within `VIEWPORT_WIDTH`. Uses
/// `visible_width()` (rather than `chars().count()`) so lines
/// carrying ANSI color escapes (the grid rows in color mode)
/// center on visible columns, not on raw char count. Empty lines
/// pass through as a single (cleared) newline.
pub(super) fn write_centered_line<W: Write>(out: &mut W, line: &str) -> std::io::Result<()> {
    if line.is_empty() {
        // Even empty lines need to clear leftover content
        // from the previous (longer) frame at this row position.
        out.write_all(ANSI_ERASE_LINE.as_bytes())?;
        out.write_all(b"\n")?;
        return Ok(());
    }
    let line_width = visible_width(line);
    let pad_n = VIEWPORT_WIDTH.saturating_sub(line_width) / 2;
    for _ in 0..pad_n {
        out.write_all(b" ")?;
    }
    out.write_all(line.as_bytes())?;
    // Erase to end of line so any old chars from the
    // previous frame past this line's end get cleared.
    out.write_all(ANSI_ERASE_LINE.as_bytes())?;
    out.write_all(b"\n")?;
    Ok(())
}

