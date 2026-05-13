use super::*;
use protocol::{
    CivCollapsed, CivFounded, Event, FigureBorn, RelationConfirmed, RunHeader, SCHEMA_VERSION,
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
