use super::*;
use protocol::{CivCollapsed, CivFounded, CivTerritoryChanged, Event, Phase, PlanetMap, TickEvent};
use sim_events::Emitter;

fn pm() -> PlanetMap {
    PlanetMap {
        grid_width: 4,
        grid_height: 3,
        elevation_q32: vec![0; 12],
        water_depth_q32: vec![0; 12],
    }
}

fn cfg(frame_every: u64) -> ViewportConfig {
    ViewportConfig {
        frame_every,
        use_alt_screen: false,
        use_color: false,
        show_planet_card: false,
        log_lines: 0,
        compact: false,
        temperature_unit: TempUnit::Fahrenheit,
    }
}

#[test]
fn no_render_for_tick_start() {
    let mut buf: Vec<u8> = Vec::new();
    let mut em = ViewportEmitter::new(&mut buf, cfg(10));
    em.emit(&Event::PlanetMap(pm())).unwrap();
    em.emit(&Event::CivFounded(CivFounded {
        tick: 0,
        civ_id: 1,
        parent_civ_id: None,
        name: String::new(),
        initial_population_q32: 0,
        founding_figure_count: 0,
        claimed_cells: vec![0, 1, 2],
    }))
    .unwrap();
    em.emit(&Event::Tick(TickEvent {
        tick: 10,
        phase: Phase::TickStart,
    }))
    .unwrap();
    drop(em);
    assert!(buf.is_empty());
}

#[test]
fn renders_at_cadence_only() {
    // Cadence in ticks (months). 120-tick cadence = 10
    // years. Frame caption derives year + month from tick.
    let mut buf: Vec<u8> = Vec::new();
    {
        let mut em = ViewportEmitter::new(&mut buf, cfg(120));
        em.emit(&Event::PlanetMap(pm())).unwrap();
        em.emit(&Event::CivFounded(CivFounded {
            tick: 0,
            civ_id: 1,
            parent_civ_id: None,
            name: String::new(),
            initial_population_q32: 0,
            founding_figure_count: 0,
            claimed_cells: vec![0, 1, 2],
        }))
        .unwrap();
        em.emit(&Event::Tick(TickEvent {
            tick: 120,
            phase: Phase::TickEnd,
        }))
        .unwrap();
        em.emit(&Event::Tick(TickEvent {
            tick: 180,
            phase: Phase::TickEnd,
        }))
        .unwrap();
        em.emit(&Event::Tick(TickEvent {
            tick: 240,
            phase: Phase::TickEnd,
        }))
        .unwrap();
    }
    let s = String::from_utf8(buf).unwrap();
    // Two frames (year 10, year 20); year 15 is between cadence
    // boundaries and skipped.
    // Caption uses compact "Y{year} M{month}" format.
    assert!(s.contains("Y10 M0"));
    assert!(s.contains("Y20 M0"));
    assert!(!s.contains("Y15"));
    assert!(s.contains("1 civ"));
}

#[test]
fn collapse_drops_civ_from_subsequent_frames() {
    // Cadence 60 ticks = 5 years; collapse between frames.
    let mut buf: Vec<u8> = Vec::new();
    let mut em = ViewportEmitter::new(&mut buf, cfg(60));
    em.emit(&Event::PlanetMap(pm())).unwrap();
    em.emit(&Event::CivFounded(CivFounded {
        tick: 0,
        civ_id: 1,
        parent_civ_id: None,
        name: String::new(),
        initial_population_q32: 0,
        founding_figure_count: 0,
        claimed_cells: vec![0, 1],
    }))
    .unwrap();
    em.emit(&Event::Tick(TickEvent {
        tick: 60,
        phase: Phase::TickEnd,
    }))
    .unwrap();
    em.emit(&Event::CivCollapsed(CivCollapsed {
        tick: 72,
        civ_id: 1,
        reason: "fixed_horizon".to_string(),
        final_population_q32: 0,
        final_figure_count: 0,
    }))
    .unwrap();
    em.emit(&Event::Tick(TickEvent {
        tick: 120,
        phase: Phase::TickEnd,
    }))
    .unwrap();
    let s = String::from_utf8(buf).unwrap();
    // First frame at year 5 has 1 active civ; second at year 10 has 0.
    // Compact caption format.
    assert!(s.contains("Y5 M0 · 1 civ"));
    assert!(s.contains("Y10 M0 · 0 civ"));
}

#[test]
fn territory_changed_updates_claim_set() {
    let mut buf: Vec<u8> = Vec::new();
    let mut em = ViewportEmitter::new(&mut buf, cfg(5));
    em.emit(&Event::PlanetMap(pm())).unwrap();
    em.emit(&Event::CivFounded(CivFounded {
        tick: 0,
        civ_id: 1,
        parent_civ_id: None,
        name: String::new(),
        initial_population_q32: 0,
        founding_figure_count: 0,
        claimed_cells: vec![0],
    }))
    .unwrap();
    em.emit(&Event::CivTerritoryChanged(CivTerritoryChanged {
        tick: 3,
        civ_id: 1,
        claimed_cells: vec![0, 1, 2, 3, 4, 5],
        population_q32: 0,
        cell_populations_q32: Vec::new(),
        cell_capacities_q32: Vec::new(),
    }))
    .unwrap();
    em.emit(&Event::Tick(TickEvent {
        tick: 5,
        phase: Phase::TickEnd,
    }))
    .unwrap();
    let s = String::from_utf8(buf).unwrap();
    // Body should contain digit '1' from cells 1..5 (centroid 0 stays 'A').
    assert!(s.contains('1'));
    assert!(s.contains('A'));
}

#[test]
fn civ_pop_line_clamps_negative_zero() {
    // Q32.32 cell populations can sum to a tiny negative f64 from
    // floating-point noise (or land on exactly -0.0). Naive
    // `{:.0}p` formatting then renders "-0p" in the civ sidebar.
    // Feed a single near-zero negative raw value and assert the
    // legend reads "0p", never "-0p".
    let mut buf: Vec<u8> = Vec::new();
    let mut em = ViewportEmitter::new(&mut buf, cfg(5));
    em.emit(&Event::PlanetMap(pm())).unwrap();
    em.emit(&Event::CivFounded(CivFounded {
        tick: 0,
        civ_id: 1,
        parent_civ_id: None,
        name: String::new(),
        initial_population_q32: 0,
        founding_figure_count: 0,
        claimed_cells: vec![0],
    }))
    .unwrap();
    em.emit(&Event::CivTerritoryChanged(CivTerritoryChanged {
        tick: 3,
        civ_id: 1,
        claimed_cells: vec![0],
        population_q32: -1,
        cell_populations_q32: vec![-1],
        cell_capacities_q32: vec![0],
    }))
    .unwrap();
    em.emit(&Event::Tick(TickEvent {
        tick: 5,
        phase: Phase::TickEnd,
    }))
    .unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(!s.contains("-0p"), "sidebar should never render -0p; got:\n{}", s);
    assert!(s.contains("0p"));
}

#[test]
fn freshly_founded_civ_sidebar_shows_initial_population() {
    // CivTerritoryChanged only fires when claimed_cells changes —
    // a freshly-founded civ may go many sim-years before its first
    // territory event. The sidebar reads cell_populations_q32, so
    // the founding handler must seed it from initial_population_q32
    // or the panel reports "0p" for an actively populated civ.
    // 4 cells × 25 q32-people each → initial_population_q32 = 100,
    // distributed evenly = 25 per cell.
    let mut buf: Vec<u8> = Vec::new();
    let mut em = ViewportEmitter::new(&mut buf, cfg(5));
    em.emit(&Event::PlanetMap(pm())).unwrap();
    let q32_one: i128 = 1_i128 << 32;
    em.emit(&Event::CivFounded(CivFounded {
        tick: 0,
        civ_id: 1,
        parent_civ_id: None,
        name: String::new(),
        initial_population_q32: 100 * q32_one,
        founding_figure_count: 1,
        claimed_cells: vec![0, 1, 2, 3],
    }))
    .unwrap();
    // No CivTerritoryChanged. Render at the cadence boundary; the
    // sidebar must reflect the founding population, not 0.
    em.emit(&Event::Tick(TickEvent {
        tick: 5,
        phase: Phase::TickEnd,
    }))
    .unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.contains("100p"), "sidebar should show founding pop; got:\n{}", s);
    assert!(!s.contains("· 0p"), "sidebar should not show 0p for live civ; got:\n{}", s);
}

#[test]
fn log_tail_captures_significant_events_only() {
    // Significant events (founded, collapsed, catastrophe,
    // tech, transmission, conflict) are rendered in the log
    // tail. Per-tick noise (Tick events) is filtered out.
    let mut buf: Vec<u8> = Vec::new();
    let mut em = ViewportEmitter::new(
        &mut buf,
        ViewportConfig {
            frame_every: 12,
            use_alt_screen: false,
            use_color: false,
            show_planet_card: false,
            log_lines: 3,
            compact: false,
            temperature_unit: TempUnit::Fahrenheit,
        },
    );
    em.emit(&Event::PlanetMap(pm())).unwrap();
    em.emit(&Event::CivFounded(CivFounded {
        tick: 0,
        civ_id: 1,
        parent_civ_id: None,
        name: String::new(),
        initial_population_q32: 0,
        founding_figure_count: 0,
        claimed_cells: vec![0, 1],
    }))
    .unwrap();
    em.emit(&Event::CatastropheFired(protocol::CatastropheFired {
        tick: 5,
        civ_id: 1,
        catastrophe_kind: "volcanic".to_string(),
        fraction_lost_q32: 0,
    }))
    .unwrap();
    em.emit(&Event::Tick(TickEvent {
        tick: 12,
        phase: Phase::TickEnd,
    }))
    .unwrap();
    let s = String::from_utf8(buf).unwrap();
    // The log section uses the heavy `------ log ------` form
    // (after experimenting with `-- log --` and a lighter
    // `· log ·` variant earlier). Substring
    // `-- log --` survives the heavy form too (it's contained
    // in `------ log ------`).
    assert!(s.contains("-- log --"));
    assert!(s.contains("civ 1 founded"));
    assert!(s.contains("hit by volcanic catastrophe"));
}

#[test]
fn log_tail_caps_at_log_lines() {
    // Pushing more events than `log_lines` keeps only the most
    // recent N in the deque.
    let mut buf: Vec<u8> = Vec::new();
    let mut em = ViewportEmitter::new(
        &mut buf,
        ViewportConfig {
            frame_every: 100,
            use_alt_screen: false,
            use_color: false,
            show_planet_card: false,
            log_lines: 2,
            compact: false,
            temperature_unit: TempUnit::Fahrenheit,
        },
    );
    em.emit(&Event::PlanetMap(pm())).unwrap();
    for civ_id in 1..=5 {
        em.emit(&Event::CivFounded(CivFounded {
            tick: 0,
            civ_id,
            parent_civ_id: None,
            name: String::new(),
            initial_population_q32: 0,
            founding_figure_count: 0,
            claimed_cells: vec![0],
        }))
        .unwrap();
    }
    em.emit(&Event::Tick(TickEvent {
        tick: 100,
        phase: Phase::TickEnd,
    }))
    .unwrap();
    let s = String::from_utf8(buf).unwrap();
    // log_lines=2 → only civs 4 and 5 should remain in the
    // deque (the older 1-3 dropped off the front).
    assert!(s.contains("civ 4 founded"));
    assert!(s.contains("civ 5 founded"));
    assert!(!s.contains("civ 1 founded"));
    assert!(!s.contains("civ 2 founded"));
}
