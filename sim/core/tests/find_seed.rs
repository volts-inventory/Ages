//! Brute-force search for a working seed after Item 21's RNG shift,
//! plus a fast panic-isolator and an overflow regression sweep.
//!
//! Run with: `cargo test -p sim-core --test find_seed -- --nocapture --ignored`.

use sim_core::{run, RunConfig};
use sim_events::{CountingEmitter, JsonLinesEmitter};
use std::io::Cursor;

#[test]
#[ignore]
fn find_working_seed() {
    for seed in &[42u64, 100, 7, 11, 23, 49, 73, 137, 271, 359, 503, 1024, 2048, 4096] {
        let cfg = RunConfig::dev(*seed, 16_000);
        let mut buf = Vec::new();
        if run(&cfg, &mut JsonLinesEmitter::new(Cursor::new(&mut buf))).is_err() {
            println!("seed {seed}: run errored");
            continue;
        }
        let log = String::from_utf8_lossy(&buf);
        let n_figures = log.matches("\"kind\":\"figure_born\"").count();
        let n_unlocks = log.matches("\"kind\":\"tech_unlocked\"").count();
        let n_rels = log.matches("\"kind\":\"relation_confirmed\"").count();
        println!("seed {seed}: figures={n_figures} unlocks={n_unlocks} relations={n_rels}");
    }
}

/// Fast panic-isolator: short tick budgets and a `CountingEmitter` so the
/// per-tick cost is event-counting rather than JSON serialisation. Lets us
/// triage which seeds panic (P0.6 fix #2) without the 20-minute run-length
/// of the original `find_working_seed` sweep.
#[test]
#[ignore]
fn overflow_sweep_no_panic() {
    // 20 seeds × 5000 ticks. The original P0.6 report identified seed 23
    // as the offender at fixed-1.31.0/arith.rs:573 (add-overflow). This
    // is the regression test for that.
    let seeds: Vec<u64> = (0u64..20).map(|i| i.wrapping_mul(13).wrapping_add(7)).collect();
    for seed in &seeds {
        let cfg = RunConfig::dev(*seed, 5_000);
        let mut counter = CountingEmitter::new();
        let result = run(&cfg, &mut counter);
        match result {
            Ok(()) => println!("seed {seed}: ok ({} ticks)", counter.count("tick")),
            Err(_) => panic!("seed {seed}: emitter errored (should never happen for CountingEmitter)"),
        }
    }
}

/// Per-seed quick triage: just runs one seed at a time so a panic
/// produces an immediate, attributable backtrace. The harness picks
/// the historically-problematic seed 23 by default. Re-target by
/// changing the `seed` constant.
#[test]
#[ignore]
fn seed_23_runs_without_panic() {
    let cfg = RunConfig::dev(23, 5_000);
    let mut counter = CountingEmitter::new();
    run(&cfg, &mut counter).expect("CountingEmitter never errors");
}
