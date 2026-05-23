//! Determinism + phase-order invariants. These tests pin the
//! byte-for-byte reproducibility of `run()` under a given seed
//! and the canonical 14-phase tick order.

use crate::*;

use protocol::Phase;
use sim_events::JsonLinesEmitter;
use std::io::Cursor;

#[test]
fn deterministic_event_log_for_same_seed() {
    let cfg = RunConfig::dev(42, 10);

    let mut buf_a = Vec::new();
    run(&cfg, &mut JsonLinesEmitter::new(Cursor::new(&mut buf_a))).unwrap();

    let mut buf_b = Vec::new();
    run(&cfg, &mut JsonLinesEmitter::new(Cursor::new(&mut buf_b))).unwrap();

    assert_eq!(
        buf_a, buf_b,
        "same seed must produce byte-for-byte identical event log"
    );
}

#[test]
fn different_seeds_diverge_in_header() {
    let cfg_a = RunConfig::dev(1, 1);
    let cfg_b = RunConfig::dev(2, 1);

    let mut buf_a = Vec::new();
    run(&cfg_a, &mut JsonLinesEmitter::new(Cursor::new(&mut buf_a))).unwrap();

    let mut buf_b = Vec::new();
    run(&cfg_b, &mut JsonLinesEmitter::new(Cursor::new(&mut buf_b))).unwrap();

    assert_ne!(
        buf_a, buf_b,
        "different seeds should yield different headers"
    );
}

#[test]
fn phase_order_is_compliant() {
    // The phase contract specifies fourteen phases in this exact order.
    assert_eq!(PHASE_ORDER.len(), 14);
    assert_eq!(PHASE_ORDER[0], Phase::TickStart);
    assert_eq!(PHASE_ORDER[13], Phase::TickEnd);
}
