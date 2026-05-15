use super::*;
use protocol::{
    CivCollapsed, CivFounded, CivSurplusChanged, Event, FigureBorn, RelationConfirmed,
    RunHeader, TradeRouteClosed, TradeRouteEstablished, SCHEMA_VERSION,
};

fn run_start() -> Event {
    Event::RunStart(RunHeader {
        schema_version: SCHEMA_VERSION,
        seed: 1,
        ages_version: "test".into(),
    })
}

#[test]
fn aggregates_civ_chapter_with_discovery_attribution() {
    let events = vec![
        run_start(),
        Event::FigureBorn(FigureBorn {
            tick: 0,
            civ_id: 1,
            figure_id: 10,
            name: "Mira".into(),
            charisma_q32: 0,
            curiosity_q32: 0,
            doubt_q32: 0,
            communicativeness_q32: 0,
            cell_assignment: 0,
        }),
        Event::CivFounded(CivFounded {
            tick: 0,
            civ_id: 1,
            parent_civ_id: None,
            name: String::new(),
            initial_population_q32: 0,
            founding_figure_count: 1,
            claimed_cells: vec![0, 1, 2, 3],
            cell_capacities_q32: Vec::new(),
        }),
        Event::RelationConfirmed(RelationConfirmed {
            tick: 42,
            relation_id: 100,
            figure_id: 10,
            template_id: 1,
            template_name: "fire".into(),
            channel: "temperature".into(),
            form: "linear".into(),
            params_q32: vec![0, 0],
            residual_q32: 0,
            confidence_q32: 0,
            n_samples: 4,
        }),
        Event::CivCollapsed(CivCollapsed {
            tick: 100,
            civ_id: 1,
            reason: "food_crisis".into(),
            final_population_q32: 0,
            final_figure_count: 1,
        }),
        Event::RunEnd {
            tick: 100,
            reason: "fixed_horizon".into(),
        },
    ];

    let d = Digest::from_events(&events);
    assert_eq!(d.civs.len(), 1);
    let civ = &d.civs[&1];
    assert_eq!(civ.figures.len(), 1);
    assert_eq!(civ.discoveries.len(), 1);
    assert_eq!(civ.discoveries[0].template_name, "fire");
    assert_eq!(civ.discoveries[0].channel, "temperature");
    assert!(civ.collapsed.is_some());
    assert_eq!(
        d.relation_names.get(&100).map(|l| l.template_name.as_str()),
        Some("fire")
    );
}

#[test]
fn aggregates_surplus_history_and_trade_routes() {
    let events = vec![
        run_start(),
        Event::CivFounded(CivFounded {
            tick: 0,
            civ_id: 1,
            parent_civ_id: None,
            name: "Aurelon".into(),
            initial_population_q32: 1 << 32,
            founding_figure_count: 1,
            claimed_cells: vec![0],
            cell_capacities_q32: Vec::new(),
        }),
        Event::CivFounded(CivFounded {
            tick: 0,
            civ_id: 2,
            parent_civ_id: None,
            name: "Brennhold".into(),
            initial_population_q32: 1 << 32,
            founding_figure_count: 1,
            claimed_cells: vec![5],
            cell_capacities_q32: Vec::new(),
        }),
        // Two surplus snapshots on civ 1 plus one on civ 2.
        Event::CivSurplusChanged(CivSurplusChanged {
            tick: 100,
            civ_id: 1,
            surplus_q32: 100 << 32,
            previous_q32: 0,
        }),
        Event::CivSurplusChanged(CivSurplusChanged {
            tick: 200,
            civ_id: 1,
            surplus_q32: 250 << 32,
            previous_q32: 100 << 32,
        }),
        Event::CivSurplusChanged(CivSurplusChanged {
            tick: 150,
            civ_id: 2,
            surplus_q32: 80 << 32,
            previous_q32: 0,
        }),
        // Trade route lifecycle: open at 50, close (war) at 300.
        Event::TradeRouteEstablished(TradeRouteEstablished {
            tick: 50,
            civ_a: 1,
            civ_b: 2,
        }),
        Event::TradeRouteClosed(TradeRouteClosed {
            tick: 300,
            civ_a: 1,
            civ_b: 2,
            reason: "war_declared".into(),
        }),
        // Second route opens and stays open.
        Event::TradeRouteEstablished(TradeRouteEstablished {
            tick: 500,
            civ_a: 1,
            civ_b: 2,
        }),
        Event::RunEnd {
            tick: 1000,
            reason: "fixed_horizon".into(),
        },
    ];
    let d = Digest::from_events(&events);
    let civ1 = &d.civs[&1];
    assert_eq!(civ1.surplus_history.len(), 2);
    assert_eq!(civ1.surplus_history[0].tick, 100);
    assert_eq!(civ1.surplus_history[1].surplus_q32, 250 << 32);
    let civ2 = &d.civs[&2];
    assert_eq!(civ2.surplus_history.len(), 1);
    assert_eq!(d.trade_routes.len(), 2);
    let first = &d.trade_routes[0];
    assert_eq!(first.start_tick, 50);
    assert_eq!(first.end_tick, Some(300));
    assert_eq!(first.close_reason.as_deref(), Some("war_declared"));
    let second = &d.trade_routes[1];
    assert_eq!(second.start_tick, 500);
    assert!(second.end_tick.is_none(), "second route is still open at run end");
    assert!(second.close_reason.is_none());
}
