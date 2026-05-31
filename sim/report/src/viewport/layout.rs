//! Frame composition / region positioning for the viewport. The
//! `render` method orchestrates the three-region layout — top
//! (planet card + log) / middle (map + sidebar) / bottom (full-
//! width log fallback) — and handles the flicker-free per-row
//! alt-screen output strategy (absolute cursor positioning + tiny
//! inter-row sleep so multi-byte glyphs don't get split across
//! PTY chunks on mobile SSH terminals).

use super::ansi::{
    divider, pad_to, split_divider, visible_width, write_centered_line, ANSI_ERASE_LINE,
    ANSI_ERASE_TO_END, MAP_WIDTH,
};
use super::emitter::ViewportEmitter;
use crate::frame::WorldFrame;
use crate::q32::fmt_pop;
use std::io::Write;
use std::time::Duration;

impl<W: Write> ViewportEmitter<W> {
    pub(super) fn render(&mut self) -> std::io::Result<()> {
        let Some(pm) = &self.planet_map else {
            return Ok(());
        };
        let frame = WorldFrame {
            tick: self.current_tick,
            civs: self.civs.values().cloned().collect(),
            nomad_cells: self.nomad_cells.iter().copied().collect(),
            producer_index: self.producer_index.clone(),
        };
        let active = frame.civs.len();
        // Tick is in months; display year + month-in-year.
        // Compact format so the caption fits in ~30 cols
        // (portrait phone terminals). Year/month derives from
        // the planet's actual orbital period — a 16-month-year
        // planet shows months 0..=15, not wraps at 11 like Earth.
        let period = self
            .planet
            .as_ref()
            .map_or(protocol::BASELINE_MONTHS_PER_YEAR as u32, |p| {
                p.orbital_period_months
            });
        let year = protocol::year_of_tick_for_period(frame.tick, period);
        let month = protocol::month_of_tick_for_period(frame.tick, period);
        // Include total species population (civ pop + nomadic pop)
        // in the caption so the user can see the species' overall
        // mass at a glance. Civ pop sums the per-civ cell
        // populations across `self.civs`; nomadic pop is the
        // running total from the most recent
        // `SpeciesNomadsChanged` snapshot.
        let civ_pop: f64 = self
            .civs
            .values()
            .flat_map(|c| c.cell_populations_q32.values())
            .map(|p| crate::q32::pop_q32_to_f64(*p))
            .sum();
        let total_pop = (civ_pop + self.nomad_total_pop).max(0.0);
        let caption = format!(
            "Y{} M{} · {} civ · {}F/{}C · {}p",
            year,
            month,
            active,
            self.civ_founded_count,
            self.civ_collapsed_count,
            fmt_pop(total_pop),
        );
        // An empty caption is passed in — the caption
        // (`Y{n} M{n} · {civ} civ · {F}F/{C}C`) is rendered
        // separately, either in the planet section's tail or at
        // the top of the map section when the planet card is hidden.
        let body = crate::frame::render_world_frame_styled(
            pm,
            self.planet.as_ref(),
            &frame,
            "",
            crate::frame::FrameStyle {
                use_color: self.cfg.use_color,
                compact: self.cfg.compact,
                density: self.cfg.density_mode,
                phase: self.surface_phase(),
            },
        );
        // Build the entire frame into an in-memory Vec<u8>, then
        // prepend `\x1b[H\x1b[2J` (home + full-screen clear) and
        // write the whole thing in one pass. The terminal sees
        // clear and content as one chunk and paints both as a
        // single pass — flicker-free and stale-row-free.
        let mut buf: Vec<u8> = Vec::with_capacity(4096);
        // Box-section layout. ANSI dim escape wraps non-map
        // content when `use_color = true` so the map stays full
        // brightness and the surrounding sections recede.
        let (dim_on, dim_off) = if self.cfg.use_color {
            ("\x1b[2m", "\x1b[0m")
        } else {
            ("", "")
        };
        // ===== Top: planet section (left, left-aligned) + log (right) =====
        // When the planet card is shown, the planet card lines + caption
        // render left-aligned in the left zone (`MAP_WIDTH` cols), with
        // the recent-event log riding alongside in the right zone — same
        // `+`-cornered split-divider geometry as the map/key middle row.
        // The bottom log section drops out in that case (the log moves
        // up). Bare-map test configs (`show_planet_card = false`) keep
        // the original full-width bottom log section.
        let planet_section_shown = self.cfg.show_planet_card && self.planet.is_some();
        let log_rides_top = planet_section_shown && self.cfg.log_lines > 0;
        if planet_section_shown {
            if let Some(card) = self.planet_card() {
                let label = self.planet.as_ref().map_or("planet", |p| p.name.as_str());
                buf.write_all(dim_on.as_bytes())?;
                if log_rides_top {
                    buf.write_all(split_divider(label, "log").as_bytes())?;
                } else {
                    buf.write_all(divider(label).as_bytes())?;
                }
                let mut planet_lines: Vec<String> = card.lines().map(str::to_string).collect();
                planet_lines.push(caption.clone());
                // Block-center: every line gets the same left
                // padding computed from the widest line, so the
                // card reads as one centered block where every
                // entry starts at the same column. Independent
                // per-line centering put each line at its own
                // offset, which looked off-center even though each
                // line was technically balanced.
                let widest = planet_lines
                    .iter()
                    .map(|l| visible_width(l))
                    .max()
                    .unwrap_or(0);
                let block_left = MAP_WIDTH.saturating_sub(widest) / 2;
                // Compose each line as `<block_left spaces><line><right pad>`
                // so it pads out to MAP_WIDTH while sharing a single
                // left margin with every other line of the card.
                let render_line = |line: &str| -> String {
                    let mut s = String::with_capacity(MAP_WIDTH + 8);
                    for _ in 0..block_left {
                        s.push(' ');
                    }
                    s.push_str(line);
                    let used = block_left + visible_width(line);
                    for _ in used..MAP_WIDTH {
                        s.push(' ');
                    }
                    s
                };
                if log_rides_top {
                    let log_lines: Vec<&str> =
                        self.recent_events.iter().map(String::as_str).collect();
                    let row_count = planet_lines.len().max(log_lines.len());
                    for i in 0..row_count {
                        let p_row = planet_lines.get(i).map_or("", String::as_str);
                        let p_padded = render_line(p_row);
                        let l_row = log_lines.get(i).copied().unwrap_or("");
                        buf.write_all(p_padded.as_bytes())?;
                        buf.write_all(b" |")?;
                        if !l_row.is_empty() {
                            buf.write_all(b" ")?;
                            buf.write_all(l_row.as_bytes())?;
                        }
                        buf.write_all(ANSI_ERASE_LINE.as_bytes())?;
                        buf.write_all(b"\n")?;
                    }
                } else {
                    for line in &planet_lines {
                        buf.write_all(render_line(line).as_bytes())?;
                        buf.write_all(ANSI_ERASE_LINE.as_bytes())?;
                        buf.write_all(b"\n")?;
                    }
                }
                buf.write_all(dim_off.as_bytes())?;
            }
        }
        // Strip the markdown ```text...``` fences from the
        // body — post-run-report ornament that reads as ASCII
        // noise in the live viewport.
        let stripped = body.strip_prefix("```text\n").unwrap_or(&body);
        let stripped = stripped.strip_suffix("```\n").unwrap_or(stripped);
        // ===== Middle: side-by-side map + sidebar =====
        // Split divider — `--- map ---+--- key ---` — with
        // the `+` corner at `RULE_COL` so the vertical `|` rule
        // connects cleanly through the dashes.
        let middle_divider_shown = self.cfg.show_planet_card;
        if middle_divider_shown {
            buf.write_all(dim_on.as_bytes())?;
            buf.write_all(split_divider("map", "key").as_bytes())?;
            buf.write_all(dim_off.as_bytes())?;
        } else {
            buf.write_all(divider("map").as_bytes())?;
        }
        if !planet_section_shown {
            write_centered_line(&mut buf, &caption)?;
        }
        // Collect the map rows so we can pair them with sidebar
        // rows. Trim trailing blank lines that
        // `render_world_frame_inner` leaves after the bottom
        // `+----+` border.
        let map_lines: Vec<String> = stripped.lines().map(str::to_string).collect();
        // build sidebar lines (legend + species + per-civ).
        // Only rendered when the planet card is shown — the test
        // configs that disable it expect bare-map output.
        let sidebar_lines: Vec<String> = if self.cfg.show_planet_card {
            self.build_sidebar_lines()
        } else {
            Vec::new()
        };
        let row_count = map_lines.len().max(sidebar_lines.len());
        for i in 0..row_count {
            let map_row = map_lines.get(i).map_or("", String::as_str);
            let map_padded = pad_to(map_row, MAP_WIDTH);
            if self.cfg.show_planet_card {
                let side_row = sidebar_lines.get(i).map_or("", String::as_str);
                // each middle row reads as
                // `{map_padded} | {sidebar_padded}` — the `|` rule
                // sits at `RULE_COL` (== MAP_WIDTH + 1).
                buf.write_all(map_padded.as_bytes())?;
                buf.write_all(b" ")?;
                buf.write_all(dim_on.as_bytes())?;
                buf.write_all(b"|")?;
                buf.write_all(dim_off.as_bytes())?;
                buf.write_all(b" ")?;
                buf.write_all(dim_on.as_bytes())?;
                buf.write_all(side_row.as_bytes())?;
                buf.write_all(dim_off.as_bytes())?;
                buf.write_all(ANSI_ERASE_LINE.as_bytes())?;
                buf.write_all(b"\n")?;
            } else {
                // No sidebar — centre the map row within
                // `VIEWPORT_WIDTH` so the bare-map test path looks
                // sensible.
                write_centered_line(&mut buf, map_row)?;
            }
        }
        // ===== Bottom: log section (full-width, bare-map only) =====
        // The log moves up next to the planet card when the card is
        // shown (see the top section), so this bottom block only fires
        // for the bare-map path (`show_planet_card = false`) — the test
        // configs that exercise the log assertions.
        if self.cfg.log_lines > 0 && !log_rides_top {
            buf.write_all(dim_on.as_bytes())?;
            buf.write_all(divider("log").as_bytes())?;
            for line in &self.recent_events {
                write_centered_line(&mut buf, line)?;
            }
            for _ in self.recent_events.len()..self.cfg.log_lines {
                buf.write_all(b"\n")?;
            }
            buf.write_all(dim_off.as_bytes())?;
        }
        // Per-row absolute cursor positioning. The previous
        // variants either prepended `\x1b[H\x1b[2J` (visible
        // erase-then-paint flicker) or `\x1b[H` + body + `\x1b[J`
        // (still flickered on iOS Termius — content past the
        // terminal viewport's last row scrolls the alt-screen,
        // and the next frame's home + paint reads as flashing).
        // Both relied on `\n` line endings, which advance the
        // cursor and scroll once we hit the bottom row.
        //
        // Instead, position the cursor absolutely at row 1 col 1
        // before each line and append `\x1b[K` (erase to EOL).
        // The terminal never scrolls because we never use `\n`;
        // each frame paints in-place at fixed coordinates. Final
        // `\x1b[J` (erase to end of screen) cleans up rows below
        // the new frame's bottom.
        //
        // Per-row write + flush rather than one big buffered
        // write: a coloured frame is ~25 KiB, well past the
        // kernel's PTY chunk size (~4 KiB) and SSH/Termius
        // packetisation. When a multi-byte glyph (`▲` = 3 bytes,
        // `⚔` = 3 bytes, …) straddles a chunk boundary, the
        // terminal's UTF-8 parser sees a lone lead/continuation
        // byte and paints `?` until the next full frame redraws.
        // Per-row writes keep each row ≤ ~300 bytes (atomic on
        // any sane PTY and well within a single SSH segment), so
        // a multi-byte glyph is always delivered intact alongside
        // the ASCII that surrounds it.
        if self.cfg.use_alt_screen {
            // Convert the `\n`-separated body into per-row
            // absolute-positioning chunks. `\x1b[<row>;1H` sets
            // cursor to (row, col 1); rows are 1-indexed in ANSI.
            let body = std::str::from_utf8(&buf).unwrap_or("");
            let mut row_buf: Vec<u8> = Vec::with_capacity(512);
            for (row_idx, line) in body.split('\n').enumerate() {
                // Skip the synthetic empty trailing line that
                // `split('\n')` produces when the buffer ends in
                // `\n` — otherwise we'd emit a stray cursor-move
                // past the last real row.
                if row_idx > 0 && line.is_empty() && body.ends_with('\n') {
                    let next_split = body.split('\n').count() - 1;
                    if row_idx == next_split {
                        break;
                    }
                }
                let row_num = row_idx + 1; // ANSI rows are 1-indexed
                row_buf.clear();
                row_buf.extend_from_slice(format!("\x1b[{row_num};1H").as_bytes());
                row_buf.extend_from_slice(line.as_bytes());
                row_buf.extend_from_slice(ANSI_ERASE_LINE.as_bytes());
                self.out.write_all(&row_buf)?;
                self.out.flush()?;
                // Brief pause between row flushes. Some mobile
                // terminals (Termius / iOS over SSH is the
                // documented case) have a UTF-8 parser that drops
                // continuation bytes when rows arrive back-to-back
                // in tight succession — the symptom is multi-byte
                // glyphs flickering as `?` until the next full
                // frame repaints. A sub-millisecond pause is
                // enough for the parser to drain between rows;
                // ~1.5 ms × ~40 rows = ~60 ms per frame, well
                // under the configured tick cadence.
                std::thread::sleep(Duration::from_micros(1500));
            }
            self.out.write_all(ANSI_ERASE_TO_END.as_bytes())?;
        } else {
            self.out.write_all(&buf)?;
        }
        self.out.flush()?;
        Ok(())
    }
}
