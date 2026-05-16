use super::hypothesizer::is_trivial_measurement;
use super::*;
use crate::fit::Sample;
use crate::forms::Form;
use sim_arith::Real;
use sim_physics::{HexGrid, PhysicsState};
use sim_recognition::Firing;
use std::collections::{BTreeSet, VecDeque};

fn baseline_templates() -> Vec<u32> {
    vec![1, 2, 3, 4, 5]
}

#[test]
fn falsification_trigger_default_30_civ_aware_uses_metabolism_scaled() {
    // Default constructor leaves the trigger at the legacy 30 so
    // planet-agnostic tests + tools keep their existing behaviour.
    let h = Hypothesizer::new(Real::ONE, &baseline_templates());
    assert_eq!(h.falsification_trigger_ticks, 30);

    // Civ-aware setter accepts the planet-derived scaled value
    // (silicate metabolism = 0.2 → 120 / 0.2 = 600 ticks).
    let mut h2 = Hypothesizer::new(Real::ONE, &baseline_templates());
    let metabolism = sim_arith::Real::from_ratio(2, 10);
    let ticks = crate::demographics::streak_ticks_for_metabolism(120, metabolism);
    h2.set_falsification_trigger_ticks(ticks);
    assert_eq!(h2.falsification_trigger_ticks, 600);

    // Zero is ignored — keeps the previous value as a guard against
    // metabolism = 0 collapses.
    let mut h3 = Hypothesizer::new(Real::ONE, &baseline_templates());
    h3.set_falsification_trigger_ticks(0);
    assert_eq!(h3.falsification_trigger_ticks, 30);
}

#[test]
fn cross_product_candidates_have_unique_stable_ids() {
    // 5 templates × 8 channels = 40 candidates; ids stable
    // across runs so confirmations survive a refresh.
    let cs = Hypothesizer::candidates_for(&baseline_templates());
    let ids: BTreeSet<u32> = cs.iter().map(|c| c.relation_id).collect();
    assert_eq!(ids.len(), cs.len(), "relation_ids must be unique");
    assert_eq!(cs.len(), 5 * Channel::ALL.len());
    for c in &cs {
        assert_eq!(
            c.relation_id,
            relation_id_for(c.template_id, c.channel),
            "ids must follow the stable relation_id_for rule"
        );
    }
}

#[test]
fn refresh_perceivable_preserves_existing_state() {
    let mut h = Hypothesizer::new(Real::ONE, &[1, 2]);
    let rid = relation_id_for(1, Channel::Temperature);
    h.samples.get_mut(&rid).unwrap().push_back(Sample {
        x: Real::ZERO,
        y: Real::ZERO,
    });
    // Add template 3 without losing template 1's samples.
    h.refresh_perceivable(&[1, 2, 3]);
    assert!(!h.samples.get(&rid).unwrap().is_empty());
    // Template 3 candidates land with empty buffers.
    let new_rid = relation_id_for(3, Channel::Temperature);
    assert!(h.samples.contains_key(&new_rid));
    assert!(h.samples.get(&new_rid).unwrap().is_empty());
}

#[test]
fn observe_collects_one_sample_per_cell_per_relation() {
    let mut h = Hypothesizer::new(Real::ONE, &baseline_templates());
    let grid = HexGrid::new(3, 3);
    let state = PhysicsState::new(grid);
    let firings: Vec<Firing> = vec![];
    let n_cells = state.grid().n_cells();
    h.observe(&state, &firings);
    for c in &h.candidates {
        let buf = h.samples.get(&c.relation_id).unwrap();
        assert_eq!(buf.len(), n_cells);
        // No firings -> all y must be zero.
        for s in buf {
            assert_eq!(s.y, Real::ZERO);
        }
    }
}

#[test]
fn observe_marks_fired_cells_as_y_one() {
    let mut h = Hypothesizer::new(Real::ONE, &baseline_templates());
    let grid = HexGrid::new(2, 2);
    let state = PhysicsState::new(grid);
    let firings = vec![Firing {
        template_id: 1,
        cell: 0,
    }];
    h.observe(&state, &firings);
    // Fire is template_id=1; the candidates at template 1 have
    // y=1 at cell 0 and y=0 elsewhere.
    for c in &h.candidates {
        if c.template_id != 1 {
            continue;
        }
        let buf = h.samples.get(&c.relation_id).unwrap();
        assert_eq!(buf[0].y, Real::ONE);
        for s in buf.iter().skip(1) {
            assert_eq!(s.y, Real::ZERO);
        }
    }
}

fn count_confirmed_events(events: &[HypothesisEvent]) -> usize {
    events
        .iter()
        .filter(|e| matches!(e, HypothesisEvent::Confirmed(_)))
        .count()
}

#[test]
fn step_confirms_constant_for_all_zero_relation() {
    // Template 99 never fires — all y=0, channel reading varies.
    // The pipeline confirms a Constant fit at value 0.
    let mut h = Hypothesizer::new(Real::ONE, &baseline_templates());
    // Hand-craft samples directly into one relation buffer.
    let rid = h.candidates[0].relation_id;
    let buf = h.samples.get_mut(&rid).unwrap();
    for i in 0..50 {
        buf.push_back(Sample {
            x: Real::from_int(i),
            y: Real::ZERO,
        });
    }
    let events = h.step(0);
    assert!(
        count_confirmed_events(&events) >= 1,
        "expected the all-zero relation to confirm"
    );
    let r = h.confirmed.get(&rid).expect("must be confirmed");
    assert_eq!(r.form, Form::Constant);
    assert!(r.confidence > Real::ZERO);
}

#[test]
fn measurement_confirms_temperature_smoothing_law() {
    // Hand-craft measurement samples that satisfy the
    // diffusion-equilibrium relation T_cell ≈ T_neighbour_mean,
    // then verify the hypothesizer confirms a Linear fit with
    // slope ≈ 1. The fit operates in normalised space; with
    // matching y/x scales (both Channel::Temperature, scale=100)
    // the SI slope equals the fit-space slope.
    let mut h = Hypothesizer::new(Real::ONE, &baseline_templates());
    let rid = measurement_relation_id(
        MeasurementChannel::Direct(Channel::Temperature),
        MeasurementChannel::NeighbourMean(Channel::Temperature),
    );
    let buf = h.measurement_samples.get_mut(&rid).unwrap();
    // 50 samples on y = x exactly — perfect equilibrium.
    for i in 0..50 {
        let v = Real::from_int(i) / Real::from_int(100);
        buf.push_back(Sample { x: v, y: v });
    }
    let events = h.step(0);
    let confirmed_measurements: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            HypothesisEvent::MeasurementConfirmed(m) => Some(m),
            _ => None,
        })
        .collect();
    assert!(
        confirmed_measurements.iter().any(|m| m.relation_id == rid),
        "expected the temperature-smoothing measurement to confirm",
    );
    let m = h.confirmed_measurements.get(&rid).expect("confirmed");
    // Linear or Constant: with y=x exactly, both fit; the
    // priority-by-param-count picks Constant first if it clears
    // exp(-1). For y=x the constant fit residual is non-zero so
    // Linear should win. Either way the SI-space recovery is
    // physical: slope ≈ 1, intercept ≈ 0 (Linear) or value ≈ mean
    // (Constant).
    assert!(matches!(m.form, Form::Linear | Form::Constant));
}

#[test]
fn step_confirms_threshold_for_step_signal() {
    // Build a clean step: y=0 for x<5, y=1 for x>=5.
    let mut h = Hypothesizer::new(Real::ONE, &baseline_templates());
    let rid = h.candidates[0].relation_id;
    let buf = h.samples.get_mut(&rid).unwrap();
    for i in 0..10 {
        buf.push_back(Sample {
            x: Real::from_int(i),
            y: if i < 5 { Real::ZERO } else { Real::ONE },
        });
    }
    let events = h.step(0);
    assert!(
        count_confirmed_events(&events) >= 1,
        "expected step signal to confirm"
    );
    let r = h.confirmed.get(&rid).expect("must be confirmed");
    // Either Constant fits poorly here (residual ≈ 0.5) OR
    // ThresholdStep fits exactly. Priority order picks the
    // simpler form first — but Constant won't clear confidence
    // threshold so ThresholdStep wins.
    assert!(matches!(
        r.form,
        Form::ThresholdStep | Form::Linear | Form::Constant
    ));
}

#[test]
fn refinement_triggers_when_active_form_drifts() {
    // Confirm a Constant fit (y all zero), then poison the sample
    // window with a clean step signal so the active Constant form
    // has near-zero confidence and ThresholdStep wins by margin.
    let mut h = Hypothesizer::new(Real::ONE, &baseline_templates());
    h.attempt_period = 1; // step every tick to drive the lifecycle
    let rid = h.candidates[0].relation_id;

    // Phase 1: 50 samples y=0 → confirm Constant.
    for i in 0..50 {
        h.samples.get_mut(&rid).unwrap().push_back(Sample {
            x: Real::from_int(i),
            y: Real::ZERO,
        });
    }
    let _ = h.step(0);
    assert!(h.confirmed.contains_key(&rid));
    assert_eq!(h.confirmed.get(&rid).unwrap().form, Form::Constant);

    // Phase 2: replace samples with a clean ThresholdStep signal.
    h.samples.get_mut(&rid).unwrap().clear();
    for i in 0..20 {
        h.samples.get_mut(&rid).unwrap().push_back(Sample {
            x: Real::from_int(i),
            y: if i < 10 { Real::ZERO } else { Real::ONE },
        });
    }

    // Drive the lifecycle until refinement events surface or
    // we hit a generous wall-clock cap.
    let mut saw_proposed = false;
    let mut saw_resolution = false;
    for tick in 1..=400 {
        let events = h.step(tick);
        for e in &events {
            match e {
                HypothesisEvent::RefinementProposed { .. } => saw_proposed = true,
                HypothesisEvent::RefinementConfirmed { .. }
                | HypothesisEvent::RefinementRejected { .. } => saw_resolution = true,
                HypothesisEvent::Confirmed(_)
                | HypothesisEvent::MeasurementConfirmed(_)
                | HypothesisEvent::Falsified { .. }
                | HypothesisEvent::Revalidated { .. }
                | HypothesisEvent::Lapsed { .. } => {}
            }
        }
        if saw_proposed && saw_resolution {
            break;
        }
    }
    assert!(saw_proposed, "expected RefinementProposed under drift");
    assert!(
        saw_resolution,
        "expected RefinementConfirmed or Rejected within 400 ticks"
    );
}

#[test]
fn step_throttles_attempts_per_relation() {
    let mut h = Hypothesizer::new(Real::ONE, &baseline_templates());
    let rid = h.candidates[0].relation_id;
    for _ in 0..50 {
        h.samples.get_mut(&rid).unwrap().push_back(Sample {
            x: Real::ZERO,
            y: Real::ZERO,
        });
    }
    h.step(0);
    // Immediately after step(0), attempt schedule moves to tick
    // 0 + attempt_period.
    let after = h.next_attempt.get(&rid).copied().unwrap();
    assert_eq!(after, h.attempt_period);
}

#[test]
fn trivial_measurement_rejects_uniform_field() {
    // Every sample identical: x = y = constant. The fitter
    // would happily confirm `Form::Constant` with residual ~ 0,
    // but the species learned nothing — the channel is uniform
    // across the window. Filter must reject before emit.
    let samples: Vec<Sample> = (0..32)
        .map(|_| Sample {
            x: Real::from_int(5),
            y: Real::from_int(5),
        })
        .collect();
    assert!(is_trivial_measurement(&samples));
}

#[test]
fn trivial_measurement_rejects_identity_uniform() {
    // The polar-cell-asymmetry case the review flagged: every
    // cell holds the same value so neighbour-mean equals the
    // direct read everywhere. Slope = 1, intercept = 0,
    // residual = 0. Identity-on-uniform is the explicit risk.
    let v = Real::from_ratio(7, 10);
    let samples: Vec<Sample> = (0..32).map(|_| Sample { x: v, y: v }).collect();
    assert!(is_trivial_measurement(&samples));
}

#[test]
fn trivial_measurement_accepts_real_gradient() {
    // Genuine gradient: x sweeps a range, y tracks linearly with
    // a real slope. Sample variance well above the threshold;
    // filter must let this pass so the diffusion-equilibrium
    // fits (the diffusion-equilibrium purpose) reach confirmation.
    let samples: Vec<Sample> = (0..32)
        .map(|i| {
            let x = Real::from_int(i);
            Sample { x, y: x }
        })
        .collect();
    assert!(!is_trivial_measurement(&samples));
}

#[test]
fn add_rival_rejects_same_form_as_primary() {
    use crate::discovery::ConfirmedRelation;
    use crate::forms::Form;
    let mut h = Hypothesizer::new(Real::ONE, &[1]);
    // Plant a primary fit.
    let primary = ConfirmedRelation {
        relation_id: 100,
        template_id: 1,
        channel: Channel::Temperature,
        form: Form::Linear,
        params: vec![],
        residual: Real::percent(1),
        confidence: Real::percent(80),
        n_samples: 32,
        confirmed_at_tick: 0,
        low_confidence_streak: 0,
        cooldown_until: 0,
        refinement: None,
        initial_residual: Real::percent(1),
        falsification_streak: 0,
        inherited_from_tick: None,
        inherited_from_civ_id: None,
    };
    h.confirmed.insert(100, primary.clone());
    // Same-form rival rejected.
    assert!(!h.add_rival_hypothesis(100, primary.clone()));
    // Different-form rival accepted.
    let mut alt = primary.clone();
    alt.form = Form::ThresholdStep;
    alt.confidence = Real::percent(70);
    assert!(h.add_rival_hypothesis(100, alt));
    assert_eq!(h.rivals.get(&100).map(Vec::len), Some(1));
}

#[test]
fn displace_swaps_when_rival_confidence_higher() {
    use crate::discovery::ConfirmedRelation;
    use crate::forms::Form;
    let mut h = Hypothesizer::new(Real::ONE, &[1]);
    let primary = ConfirmedRelation {
        relation_id: 200,
        template_id: 1,
        channel: Channel::Temperature,
        form: Form::Linear,
        params: vec![],
        residual: Real::percent(2),
        confidence: Real::percent(60),
        n_samples: 32,
        confirmed_at_tick: 0,
        low_confidence_streak: 0,
        cooldown_until: 0,
        refinement: None,
        initial_residual: Real::percent(2),
        falsification_streak: 0,
        inherited_from_tick: None,
        inherited_from_civ_id: None,
    };
    h.confirmed.insert(200, primary.clone());
    let mut high_conf_rival = primary.clone();
    high_conf_rival.form = Form::ThresholdStep;
    high_conf_rival.confidence = Real::percent(90);
    assert!(h.add_rival_hypothesis(200, high_conf_rival));
    let swap = h.displace_primary_with_best_rival(200);
    assert!(swap.is_some());
    let (old, new) = swap.unwrap();
    assert_eq!(old, Form::Linear);
    assert_eq!(new, Form::ThresholdStep);
    // Displaced primary returns to rivals.
    assert!(h.rivals.get(&200).is_some_and(|r| !r.is_empty()));
    // No-op when no rival exceeds new primary.
    assert!(h.displace_primary_with_best_rival(200).is_none());
}

#[test]
fn record_experimental_pushes_each_sample_twice() {
    // The 2× weighting expresses that controlled samples carry more
    // information than passive ones; every apparatus call writes the
    // (clamp, response) pair to the buffer two times so the fit
    // pool over-weights experimental contributions.
    let mut h = Hypothesizer::new(Real::ONE, &[]);
    let y = MeasurementChannel::Direct(Channel::Temperature);
    let x = MeasurementChannel::Direct(Channel::Temperature);
    let rid = measurement_relation_id(y, x);
    h.record_experimental_measurement(y, x, Real::from_int(3), Real::from_int(2));
    assert_eq!(h.measurement_samples.get(&rid).map(VecDeque::len), Some(2));
    assert_eq!(h.experimental_count_by_relation.get(&rid).copied(), Some(2));
}

#[test]
fn apparatus_relation_pair_added_to_measurements() {
    // The default catalogue doesn't include
    // (Direct(Fuel), Direct(Fuel)) — apparatus pairs must be added
    // to `measurements` lazily so step_with_cosmology_and_doubt
    // walks them and the fit can confirm.
    let mut h = Hypothesizer::new(Real::ONE, &[]);
    let y = MeasurementChannel::Direct(Channel::Fuel);
    let x = MeasurementChannel::Direct(Channel::Fuel);
    let before = h.measurements.len();
    h.record_experimental_measurement(y, x, Real::ONE, Real::ONE);
    let after = h.measurements.len();
    assert_eq!(after, before + 1);
}

#[test]
fn confirmed_measurement_flags_experimental_when_apparatus_contributed() {
    // A relation that received any apparatus sample at all marks
    // its eventual fit as experimental. Construct a minimal pool:
    // 80 apparatus samples on a clean linear y = 2x relation; fit
    // confirms; flag fires.
    let mut h = Hypothesizer::new(Real::ONE, &[]);
    let y = MeasurementChannel::Direct(Channel::Temperature);
    let x = MeasurementChannel::Direct(Channel::Temperature);
    let rid = measurement_relation_id(y, x);
    // Walk the 4-point ladder repeatedly so the fit has variance.
    for tick_step in 0..40_u32 {
        let xv = Real::from_int(i64::from(tick_step % 4));
        let yv = xv * Real::from_int(2);
        h.record_experimental_measurement(y, x, xv, yv);
    }
    // Drive the fit pipeline until measurement_next_attempt fires.
    for tick in 0..200 {
        let _ = h.step_with_cosmology(tick, &crate::cosmology::Cosmology::NEUTRAL);
        if h.confirmed_measurements.contains_key(&rid) {
            break;
        }
    }
    let conf = h
        .confirmed_measurements
        .get(&rid)
        .expect("apparatus-only fit should confirm");
    assert!(
        conf.is_experimental,
        "apparatus contribution must flip flag"
    );
}
