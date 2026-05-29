//! ratatui rendering for the interactive dashboard. Pure view layer:
//! every `draw_*` reads the mirrored [`ViewportEmitter`] state +
//! [`UiState`] and paints widgets — no state mutation here.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use super::{Tab, UiState};
use crate::frame::{
    centroid_symbol, civ_color_code, claim_symbol, pop_digit, terrain_color_code, CivClaim,
    WorldFrame,
};
use crate::q32::{fmt_pop, pop_q32_to_f64, q32_to_f64};
use crate::render::{terrain_symbol, SurfacePhase};
use crate::viewport::ViewportEmitter;
use protocol::{PlanetDerived, PlanetMap};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Tabs, Widget, Wrap};
use ratatui::Frame;
use sim_events::PaceControl;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Sink;

/// Model type alias: the TUI drives the viewport state-mirror with a
/// null sink (it never writes ANSI; it only accumulates state).
type Model = ViewportEmitter<Sink>;

pub(super) fn draw(f: &mut Frame, model: &Model, ui: &mut UiState, pace: &PaceControl) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // tab bar
            Constraint::Length(1), // status line
            Constraint::Min(0),    // body
            Constraint::Length(1), // controls footer
        ])
        .split(f.area());
    draw_tabs(f, chunks[0], ui);
    draw_status(f, chunks[1], model, ui, pace);
    match ui.tab {
        Tab::World => draw_world(f, chunks[2], model, ui),
        Tab::Civilizations => draw_civilizations(f, chunks[2], model, ui),
        Tab::Planet => draw_planet(f, chunks[2], model, ui),
    }
    draw_footer(f, chunks[3]);
}

fn draw_tabs(f: &mut Frame, area: Rect, ui: &UiState) {
    let titles: Vec<Line> = Tab::ALL
        .iter()
        .map(|t| Line::from(format!(" {} ", t.title())))
        .collect();
    let tabs = Tabs::new(titles)
        .select(ui.tab.index())
        .style(Style::default().fg(Color::Gray))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .divider("");
    f.render_widget(tabs, area);
}

fn draw_status(f: &mut Frame, area: Rect, model: &Model, ui: &UiState, pace: &PaceControl) {
    let period = model.orbital_period();
    let tick = model.current_tick();
    let year = protocol::year_of_tick_for_period(tick, period);
    let month = protocol::month_of_tick_for_period(tick, period);

    let state_span = if pace.is_paused() {
        Span::styled(
            " PAUSED ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            format!(" PLAY {} ", ui.speed_label()),
            Style::default().fg(Color::Black).bg(Color::Green),
        )
    };
    let temp = model.mean_temp_display().unwrap_or_else(|| "—".to_string());
    let info = format!(
        "  Y{year} M{month}  ·  {temp}  ·  {} civ (↑{} ↓{})  ·  pop {}  ·  nomad {}",
        model.active_civ_count(),
        model.founded_count(),
        model.collapsed_count(),
        fmt_pop(model.total_pop()),
        fmt_pop(model.nomad_pop()),
    );
    let mut spans = vec![state_span, Span::raw(info)];
    if ui.run_complete {
        spans.push(Span::styled(
            "  [run complete — press q]",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_footer(f: &mut Frame, area: Rect) {
    let controls = "q quit · space pause · s step · ←/→ speed · ↑/↓ civ · [ ] list · Tab view · d density · PgUp/Dn log/detail";
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            controls,
            Style::default().fg(Color::DarkGray),
        ))),
        area,
    );
}

fn draw_world(f: &mut Frame, area: Rect, model: &Model, ui: &mut UiState) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(5), Constraint::Length(8)])
        .split(area);
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(20), Constraint::Length(34)])
        .split(rows[0]);
    draw_map(f, cols[0], model, ui);
    // Right column: a compact species panel above the civ list, so the
    // World view shows who the protagonists are (and their habitat)
    // without switching to the Planet tab.
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(4)])
        .split(cols[1]);
    draw_species_panel(f, right[0], model);
    draw_civ_list(f, right[1], model, ui);
    draw_legend(f, rows[1], model);
    draw_log(f, rows[2], model, ui);
}

fn draw_species_panel(f: &mut Frame, area: Rect, model: &Model) {
    let text = model
        .species_card_text()
        .unwrap_or_else(|| "species pending".to_string());
    f.render_widget(
        Paragraph::new(text)
            .block(Block::default().borders(Borders::ALL).title(" Species "))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_civilizations(f: &mut Frame, area: Rect, model: &Model, ui: &mut UiState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(34), Constraint::Min(20)])
        .split(area);
    draw_civ_list(f, cols[0], model, ui);
    draw_civ_detail(f, cols[1], model, ui);
}

fn draw_planet(f: &mut Frame, area: Rect, model: &Model, ui: &UiState) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(8), Constraint::Length(5), Constraint::Min(4)])
        .split(area);
    let cards = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[0]);
    let planet_card = model
        .planet_card_text()
        .unwrap_or_else(|| "sampling planet…".to_string());
    f.render_widget(
        Paragraph::new(planet_card)
            .block(Block::default().borders(Borders::ALL).title(" Planet ")),
        cards[0],
    );
    let species_card = model
        .species_card_text()
        .unwrap_or_else(|| "awaiting first life…".to_string());
    f.render_widget(
        Paragraph::new(species_card)
            .block(Block::default().borders(Borders::ALL).title(" Species ")),
        cards[1],
    );
    draw_legend(f, rows[1], model);
    draw_log(f, rows[2], model, ui);
}

fn phase_label(phase: SurfacePhase) -> &'static str {
    match phase {
        SurfacePhase::Earthlike => "temperate",
        SurfacePhase::Lava => "molten",
        SurfacePhase::IceCap => "frozen",
        SurfacePhase::Scorched => "scorched",
    }
}

fn draw_map(f: &mut Frame, area: Rect, model: &Model, ui: &UiState) {
    let phase = model.phase();
    let title = format!(
        " {} · {} ",
        model.planet_name().unwrap_or("planet"),
        phase_label(phase),
    );
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if let Some(pm) = model.planet_map() {
        let frame = model.world_frame();
        let widget = MapWidget {
            pm,
            planet: model.planet(),
            phase,
            frame: &frame,
            density: ui.density,
            use_color: ui.use_color,
        };
        f.render_widget(widget, inner);
    } else {
        f.render_widget(Paragraph::new("sampling planet…"), inner);
    }
}

fn draw_civ_list(f: &mut Frame, area: Rect, model: &Model, ui: &mut UiState) {
    let panels = model.civ_panels();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Civs ({}) ", panels.len()));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let height = inner.height as usize;
    // Cache the row count so keyboard paging / selection-follow (in
    // `UiState`) can reason about a screenful.
    ui.civ_rows = height;
    if height == 0 {
        return;
    }
    // `civ_scroll` is the persistent top index, moved by `[`/`]` and
    // nudged by selection. Clamp it against the list length here.
    let max_start = panels.len().saturating_sub(height);
    if ui.civ_scroll > max_start {
        ui.civ_scroll = max_start;
    }
    let start = ui.civ_scroll;
    let items: Vec<ListItem> = panels
        .iter()
        .enumerate()
        .skip(start)
        .take(height)
        .map(|(idx, p)| {
            let selected = idx == ui.selected_civ;
            let swatch_color = if ui.use_color {
                Color::Indexed(p.color_idx)
            } else {
                Color::Reset
            };
            let war = if p.at_war { " ⚔" } else { "" };
            // The trailing population/tier detail is dimmed gray when
            // idle, but on the selected row's light background gray-on-
            // white is unreadable — switch it to the row's dark fg.
            let detail_fg = if selected { Color::Black } else { Color::Gray };
            let line = Line::from(vec![
                Span::styled("█ ", Style::default().fg(swatch_color)),
                Span::raw(format!("{} {}", p.centroid_letter, p.name)),
                Span::styled(
                    format!("  {}p t{}{}", fmt_pop(p.pop), p.tier, war),
                    Style::default().fg(detail_fg),
                ),
            ]);
            let style = if selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(line).style(style)
        })
        .collect();
    f.render_widget(List::new(items), inner);
}

fn draw_civ_detail(f: &mut Frame, area: Rect, model: &Model, ui: &mut UiState) {
    let panels = model.civ_panels();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Civilization ");
    let inner = block.inner(area);
    f.render_widget(block, area);
    let Some(p) = panels.get(ui.selected_civ) else {
        f.render_widget(Paragraph::new("no civilization selected"), inner);
        return;
    };
    let color = if ui.use_color {
        Color::Indexed(p.color_idx)
    } else {
        Color::Reset
    };
    let mut lines = vec![
        Line::from(Span::styled(
            format!("{} {}", p.centroid_letter, p.name),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("founded year {} · {} cells", p.founded_year, p.cells)),
        Line::from(format!("population {}", fmt_pop(p.pop))),
        Line::from(format!("tech tier {} · {} tools", p.tier, p.tool_count)),
    ];
    if let Some(tool) = &p.last_tool {
        lines.push(Line::from(format!("last unlock: {tool}")));
    }
    // Always show the meter; a civ with no cohesion event yet is
    // assumed fully cohesive (matches the legacy sidebar default).
    lines.push(cohesion_bar(p.cohesion_pct.unwrap_or(100)));
    if let Some(l) = p.life_years {
        lines.push(Line::from(format!("life expectancy {l:.0}y")));
    }
    let war = if p.at_war {
        format!("at war ⚔ — {}", p.rivals.join(", "))
    } else {
        "at peace".to_string()
    };
    lines.push(Line::from(war));
    if let Some((axis, v)) = p.cosmology_axis {
        lines.push(Line::from(format!("cosmology: {axis} {v:+.2}")));
    }
    if let Some((axis, v)) = p.religion_axis {
        lines.push(Line::from(format!("religion: {axis} {v:+.2}")));
    }
    let tools = model.civ_tools(p.id);
    if !tools.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("tools ({})", tools.len()),
            Style::default().add_modifier(Modifier::BOLD),
        )));
        // One wrapped paragraph line; `Wrap` flows the comma-joined
        // list across the pane width.
        lines.push(Line::from(Span::styled(
            tools.join(", "),
            Style::default().fg(Color::Gray),
        )));
    }
    // Scrollable for small terminals: clamp the offset against the
    // content that overflows the pane, then render with that scroll.
    let height = inner.height as usize;
    let max_scroll = lines.len().saturating_sub(height);
    if ui.detail_scroll > max_scroll {
        ui.detail_scroll = max_scroll;
    }
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .scroll((ui.detail_scroll as u16, 0)),
        inner,
    );
}

/// A 10-cell cohesion meter coloured by level (red below the
/// civil-war floor, amber in the breakaway band, green when stable).
fn cohesion_bar(pct: i64) -> Line<'static> {
    let pct = pct.clamp(0, 100);
    let filled = ((pct as f64 / 100.0) * 10.0).round() as usize;
    let bar = format!("{}{}", "█".repeat(filled), "░".repeat(10 - filled));
    let color = if pct < 20 {
        Color::Red
    } else if pct < 50 {
        Color::Yellow
    } else {
        Color::Green
    };
    Line::from(vec![
        Span::raw("cohesion "),
        Span::styled(bar, Style::default().fg(color)),
        Span::raw(format!(" {pct}%")),
    ])
}

fn draw_legend(f: &mut Frame, area: Rect, model: &Model) {
    let terrain = match model.phase() {
        SurfacePhase::Earthlike => "~ sea  ≈ deep  ▲ peak  △ hill  ▒ land  ░ coast  · plain",
        SurfacePhase::Lava => "* magma  ▲ peak  △ outcrop",
        SurfacePhase::IceCap => "+ ice  ▲ peak  △ hill  ▒ land  ░ coast  · plain",
        SurfacePhase::Scorched => "· dry basin  ▲ peak  △ hill  ▒ land  ░ coast",
    };
    let lines = vec![
        Line::from("A–Z = capital · 1–9 = % of cell capacity · # = disputed (multi-civ)"),
        Line::from("colour = owning civ (matches Civs panel) · bright/white = nomads"),
        Line::from(terrain),
    ];
    f.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title(" Legend ")),
        area,
    );
}

fn draw_log(f: &mut Frame, area: Rect, model: &Model, ui: &UiState) {
    let log = model.log_lines();
    let n = log.len();
    // Inner height is the bordered area minus the top + bottom rules.
    let height = (area.height.saturating_sub(2)) as usize;
    // `log_scroll` counts lines up from the newest; clamp so we never
    // scroll past the top.
    let max_scroll = n.saturating_sub(height);
    let scroll = ui.log_scroll.min(max_scroll);
    // Title doubles as a scroll affordance: how many older lines sit
    // above the current view.
    let title = if scroll > 0 {
        format!(" Events  ↑{scroll} more ")
    } else {
        " Events ".to_string()
    };
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let h = inner.height as usize;
    if h == 0 || n == 0 {
        return;
    }
    let end = n - scroll;
    let start = end.saturating_sub(h);
    let lines: Vec<Line> = log
        .iter()
        .skip(start)
        .take(end - start)
        .map(|s| Line::from(s.as_str()))
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

/// Live world map. Mirrors the cell-overlay precedence of
/// [`crate::frame::render_world_frame_styled`] (centroid letter →
/// dispute `#` → single-owner fill → nomad → terrain) but paints
/// ratatui cells with 256-colour indices + intensity modifiers
/// instead of emitting ANSI escapes. The glyph + colour helpers are
/// shared with that renderer so the two stay visually consistent.
struct MapWidget<'a> {
    pm: &'a PlanetMap,
    planet: Option<&'a PlanetDerived>,
    phase: SurfacePhase,
    frame: &'a WorldFrame,
    density: bool,
    use_color: bool,
}

impl Widget for MapWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let w = self.pm.grid_width as usize;
        let h = self.pm.grid_height as usize;
        if w == 0 || h == 0 || area.width == 0 || area.height == 0 {
            return;
        }
        let terrain_peak = self.planet.map_or(0.0, |p| q32_to_f64(p.terrain_peak_q32));

        // One pass to index ownership / centroids / nomads, matching
        // the ANSI renderer's precompute.
        let mut owners: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        let mut centroids: BTreeMap<u32, u32> = BTreeMap::new();
        let mut civ_by_id: BTreeMap<u32, &CivClaim> = BTreeMap::new();
        let mut frame_max_pop = 0.0_f64;
        for civ in &self.frame.civs {
            civ_by_id.insert(civ.civ_id, civ);
            for &raw in civ.cell_populations_q32.values() {
                let p = pop_q32_to_f64(raw);
                if p > frame_max_pop {
                    frame_max_pop = p;
                }
            }
            centroids.entry(civ.centroid).or_insert(civ.civ_id);
            for &cell in &civ.claimed_cells {
                owners.entry(cell).or_default().push(civ.civ_id);
            }
        }
        let nomad_set: BTreeSet<u32> = self.frame.nomad_cells.iter().copied().collect();

        // Terminal cells are ~twice as tall as wide, so 1 char/cell
        // renders a stretched, vertically-elongated map that also
        // leaves large horizontal voids on wide panes. Use 2 chars per
        // cell when they fit — roughly square aspect, fills the pane —
        // and fall back to 1 char on narrow terminals.
        let area_w = area.width as usize;
        let cell_w = if w * 2 <= area_w { 2 } else { 1 };
        let draw_w = w.min(area_w / cell_w.max(1));
        let draw_h = h.min(area.height as usize);
        let off_x = (area_w - draw_w * cell_w) / 2;
        let off_y = (area.height as usize - draw_h) / 2;

        for r in 0..draw_h {
            for q in 0..draw_w {
                let (ch, color, modifier) = classify_cell(
                    self.pm,
                    self.phase,
                    terrain_peak,
                    &owners,
                    &centroids,
                    &civ_by_id,
                    &nomad_set,
                    frame_max_pop,
                    r,
                    q,
                    w,
                    self.density,
                    self.use_color,
                );
                let style = Style::default().fg(color).add_modifier(modifier);
                let x = area.x + (off_x + q * cell_w) as u16;
                let y = area.y + (off_y + r) as u16;
                buf[(x, y)].set_char(ch).set_style(style);
                if cell_w == 2 {
                    // Solid fill: terrain/region glyphs double up so
                    // land/water/territory read as contiguous masses;
                    // single markers (capital letters, pop digits, the
                    // dispute `#`) get a trailing space so they stay
                    // legible rather than reading as "AA" / "99".
                    let second = if ch.is_ascii_alphanumeric() || ch == '#' {
                        ' '
                    } else {
                        ch
                    };
                    buf[(x + 1, y)].set_char(second).set_style(style);
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn classify_cell(
    pm: &PlanetMap,
    phase: SurfacePhase,
    terrain_peak: f64,
    owners: &BTreeMap<u32, Vec<u32>>,
    centroids: &BTreeMap<u32, u32>,
    civ_by_id: &BTreeMap<u32, &CivClaim>,
    nomad_set: &BTreeSet<u32>,
    frame_max_pop: f64,
    r: usize,
    q: usize,
    w: usize,
    density: bool,
    use_color: bool,
) -> (char, Color, Modifier) {
    let cell = (r * w + q) as u32;
    let terrain = || terrain_symbol(pm, r, q, terrain_peak, phase);
    let civ_color = |id: u32| {
        if use_color {
            Color::Indexed(civ_color_code(id))
        } else {
            Color::Reset
        }
    };

    let active_centroid = centroids.get(&cell).copied().filter(|&id| id > 0);
    if let Some(civ_id) = active_centroid {
        return (centroid_symbol(civ_id), civ_color(civ_id), Modifier::BOLD);
    }
    let active_owners: Option<Vec<u32>> =
        owners.get(&cell).map(|c| c.iter().copied().filter(|&id| id > 0).collect());
    if active_owners.as_ref().is_some_and(|c| c.len() > 1) {
        let color = if use_color { Color::White } else { Color::Reset };
        return ('#', color, Modifier::BOLD);
    }
    if let Some(claims) = active_owners.as_ref().filter(|c| !c.is_empty()) {
        let civ_id = claims[0];
        if !use_color {
            return (claim_symbol(civ_id), Color::Reset, Modifier::empty());
        }
        let claim = civ_by_id.get(&civ_id);
        let pop = claim
            .and_then(|c| c.cell_populations_q32.get(&cell).copied())
            .map_or(0.0, pop_q32_to_f64);
        let cap = claim
            .and_then(|c| c.cell_capacities_q32.get(&cell).copied())
            .map(pop_q32_to_f64)
            .filter(|c| *c > 0.0)
            .unwrap_or(frame_max_pop);
        let color = Color::Indexed(civ_color_code(civ_id));
        if density {
            let ratio = if cap > 0.0 { (pop / cap).clamp(0.0, 1.0) } else { 0.0 };
            let modifier = if ratio >= 0.60 {
                Modifier::BOLD
            } else if ratio >= 0.30 {
                Modifier::empty()
            } else {
                Modifier::DIM
            };
            return (terrain(), color, modifier);
        }
        return pop_digit(pop, cap)
            .map_or_else(|| (terrain(), color, Modifier::BOLD), |d| (d, color, Modifier::BOLD));
    }
    if nomad_set.contains(&cell) {
        return if use_color {
            (terrain(), Color::White, Modifier::BOLD)
        } else {
            ('0', Color::Reset, Modifier::empty())
        };
    }
    let sym = terrain();
    let color = if use_color {
        terrain_color_code(sym).map_or(Color::Reset, Color::Indexed)
    } else {
        Color::Reset
    };
    (sym, color, Modifier::empty())
}
