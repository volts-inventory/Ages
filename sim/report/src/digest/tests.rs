use super::*;
use protocol::{
    CatastropheFired, CivCollapsed, CivFounded, CivResilienceTick, CivSurplusChanged, Event,
    ExtinctionCause, FigureBorn, HgtEvent, RelationConfirmed, RunHeader, SpeciationEvent,
    SpeciationTriggerKind, SpeciesExtinct, TraitName, TradeRouteClosed, TradeRouteEstablished,
    SCHEMA_VERSION,
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
    assert!(
        second.end_tick.is_none(),
        "second route is still open at run end"
    );
    assert!(second.close_reason.is_none());
}

/// Ecosystem aggregate fold counts speciation, HGT, catastrophes,
/// and extinctions; tracks the known / extinct species id sets;
/// and averages the resilience trace.
#[test]
fn aggregates_ecosystem_summary_across_events() {
    let half_q32 = (0.5_f64 * (1_u64 << 32) as f64) as i64;
    let one_q32 = 1_i64 << 32;
    let three_halves_q32 = (1.5_f64 * (1_u64 << 32) as f64) as i64;
    let events = vec![
        run_start(),
        // Two catastrophes — one volcano + one hurricane.
        Event::CatastropheFired(CatastropheFired {
            tick: 10,
            civ_id: 1,
            catastrophe_kind: "volcano".into(),
            fraction_lost_q32: 0,
        }),
        Event::CatastropheFired(CatastropheFired {
            tick: 20,
            civ_id: 1,
            catastrophe_kind: "hurricane".into(),
            fraction_lost_q32: 0,
        }),
        Event::CatastropheFired(CatastropheFired {
            tick: 30,
            civ_id: 1,
            catastrophe_kind: "volcano".into(),
            fraction_lost_q32: 0,
        }),
        // Two speciation events. Daughter 5 then 6, from parent 0.
        Event::SpeciationOccurred(SpeciationEvent {
            tick: 40,
            parent_id: 0,
            daughter_id: 5,
            trigger: SpeciationTriggerKind::Sympatric,
        }),
        Event::SpeciationOccurred(SpeciationEvent {
            tick: 50,
            parent_id: 5,
            daughter_id: 6,
            trigger: SpeciationTriggerKind::Polyploid,
        }),
        // One HGT — donor 5 → recipient 6.
        Event::HorizontalGeneTransfer(HgtEvent {
            tick: 60,
            donor_id: 5,
            recipient_id: 6,
            trait_swapped: TraitName::RadiationMax,
        }),
        // Species 5 goes extinct.
        Event::SpeciesExtinct(SpeciesExtinct {
            tick: 70,
            species_id: 5,
            cause: ExtinctionCause::PopulationCollapse,
        }),
        // Three resilience ticks — mean = (0.5 + 1.0 + 1.5) / 3 = 1.0.
        Event::CivResilienceTick(CivResilienceTick {
            tick: 80,
            civ_id: 1,
            resilience_q32: half_q32,
            producer_biomass_q32: 0,
            previous_q32: one_q32,
        }),
        Event::CivResilienceTick(CivResilienceTick {
            tick: 90,
            civ_id: 1,
            resilience_q32: one_q32,
            producer_biomass_q32: 0,
            previous_q32: half_q32,
        }),
        Event::CivResilienceTick(CivResilienceTick {
            tick: 100,
            civ_id: 1,
            resilience_q32: three_halves_q32,
            producer_biomass_q32: 0,
            previous_q32: one_q32,
        }),
        Event::RunEnd {
            tick: 200,
            reason: "fixed_horizon".into(),
        },
    ];
    let d = Digest::from_events(&events);
    let e = &d.ecosystem;
    assert_eq!(e.speciation_count, 2);
    assert_eq!(e.hgt_count, 1);
    assert_eq!(e.catastrophes_by_kind.get("volcano").copied(), Some(2));
    assert_eq!(e.catastrophes_by_kind.get("hurricane").copied(), Some(1));
    assert!(e.extinct_species_ids.contains(&5));
    assert_eq!(e.extinct_species_count(), 1);
    // Known: 0 (parent), 5, 6 — three species seen.
    assert_eq!(e.known_species_ids.len(), 3);
    // Mean resilience ≈ 1.0 — Q32.32 representation within rounding.
    let mean = e.mean_resilience_q32.expect("3 ticks → Some(mean)");
    let mean_f = mean as f64 / (1_u64 << 32) as f64;
    assert!(
        (mean_f - 1.0).abs() < 1e-6,
        "mean resilience should be ~1.0, got {mean_f}",
    );
    // Magnetic-reversal / Hadley / tidal-heating / subsurface
    // are not yet emitted as events; the digest should report 0 /
    // None placeholders so the wire schema can grow them later
    // without restructuring the field.
    assert_eq!(e.magnetic_reversal_events, 0);
    assert!(e.hadley_cell_count.is_none());
    assert!(e.mean_hadley_jet_q32.is_none());
    assert!(e.total_tidal_heating_tw_q32.is_none());
    assert!(e.mean_subsurface_temp_k_q32.is_none());
}

/// End-to-end smoke test: the digest's new ecosystem aggregates
/// surface in the rendered markdown's "Ecosystem & dynamics"
/// section. Locks the digest → renderer wiring so a refactor that
/// stops piping aggregates through to the planet card surfaces
/// loudly.
#[test]
fn rendered_markdown_includes_ecosystem_section() {
    let events = vec![
        run_start(),
        Event::SpeciationOccurred(SpeciationEvent {
            tick: 10,
            parent_id: 0,
            daughter_id: 1,
            trigger: SpeciationTriggerKind::Sympatric,
        }),
        Event::HorizontalGeneTransfer(HgtEvent {
            tick: 20,
            donor_id: 0,
            recipient_id: 1,
            trait_swapped: TraitName::RadiationMax,
        }),
        Event::CatastropheFired(CatastropheFired {
            tick: 30,
            civ_id: 1,
            catastrophe_kind: "volcano".into(),
            fraction_lost_q32: 0,
        }),
        Event::RunEnd {
            tick: 100,
            reason: "fixed_horizon".into(),
        },
    ];
    let d = Digest::from_events(&events);
    let md = crate::markdown(&d);
    assert!(
        md.contains("## Ecosystem & dynamics"),
        "rendered markdown should include the ecosystem section",
    );
    assert!(
        md.contains("Speciation events | 1"),
        "speciation count should surface as a table row; got:\n{md}",
    );
    assert!(
        md.contains("Horizontal gene transfer events | 1"),
        "HGT count should surface as a table row",
    );
    assert!(
        md.contains("volcano×1"),
        "catastrophe histogram should surface volcano count",
    );
}

/// Single-event smoke tests: each new aggregate increments
/// independently. Belt-and-suspenders alongside the combined test
/// above — locks the per-event semantics in case the combined test
/// drifts.
#[test]
fn ecosystem_aggregates_single_event_increments() {
    let only_speciation = Digest::from_events(&[
        run_start(),
        Event::SpeciationOccurred(SpeciationEvent {
            tick: 1,
            parent_id: 0,
            daughter_id: 1,
            trigger: SpeciationTriggerKind::FounderEffect,
        }),
    ]);
    assert_eq!(only_speciation.ecosystem.speciation_count, 1);
    assert_eq!(only_speciation.ecosystem.hgt_count, 0);

    let only_hgt = Digest::from_events(&[
        run_start(),
        Event::HorizontalGeneTransfer(HgtEvent {
            tick: 1,
            donor_id: 0,
            recipient_id: 1,
            trait_swapped: TraitName::TemperatureToleranceLow,
        }),
    ]);
    assert_eq!(only_hgt.ecosystem.hgt_count, 1);
    assert_eq!(only_hgt.ecosystem.speciation_count, 0);

    let only_extinct = Digest::from_events(&[
        run_start(),
        Event::SpeciesExtinct(SpeciesExtinct {
            tick: 1,
            species_id: 9,
            cause: ExtinctionCause::Catastrophe,
        }),
    ]);
    assert_eq!(only_extinct.ecosystem.extinct_species_count(), 1);
    assert!(only_extinct.ecosystem.extinct_species_ids.contains(&9));

    // No resilience ticks → mean is None, not Some(0).
    assert!(only_speciation.ecosystem.mean_resilience_q32.is_none());
}
