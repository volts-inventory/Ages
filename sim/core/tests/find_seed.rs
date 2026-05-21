//! Brute-force search for a working seed after Item 21's RNG shift.
//! Run with: `cargo test -p sim-core --test find_seed -- --nocapture --ignored find_working_seed`

use sim_core::{run, RunConfig};
use sim_events::JsonLinesEmitter;
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
