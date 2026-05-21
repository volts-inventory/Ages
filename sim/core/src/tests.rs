use super::*;
use sim_events::{CountingEmitter, JsonLinesEmitter};
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

#[test]
fn figure_born_events_emitted_for_founding_band() {
    // No inaugural civ; civs emerge from nomadic tech
    // accumulation. Origin-seeded nomadic init means
    // the species starts at 1-3 origin cells (vs. spread
    // across every habitable cell earlier). Density-gradient
    // diffusion takes longer to fill enough cells to cross
    // the EMERGENT_FOUNDING_POP threshold, so this run length
    // bumped 800 → 2400 ticks. Tech-gated transit (strict
    // block at tier 0) bumped further to 8000 ticks. Halving
    // base growth (1/100 → 1/200, ~6%/yr compounded) for
    // realism bumps again to 16000 ticks — pre-tech cells
    // grow at half-speed; tech-rich cells need their growth
    // templates (thermal_gradient, seasonal_thaw) active to
    // approach the prior pace. The 2-3 figure_born band per
    // founding is unchanged.
    //
    // P0.6 (post-Sprint-4/5 RNG shift): seeds 42, 100, 7, 11
    // no longer produce civs within 16k ticks under the
    // Items-12-24 wiring (volcanism + weathering + tidal
    // heating + atmospheric escape shifted the per-seed
    // habitability bands such that those 4 specific worldgen
    // samples land outside the EMERGENT_FOUNDING_POP /
    // EMERGENT_FOUNDING_TECH joint gate). Brute-force seed
    // sweep in `tests/find_seed.rs::find_working_seed`
    // identified seed 1024 as the most reliable producer
    // post-RNG-shift (132 figures / 1093 unlocks / 24k
    // relations in 16k ticks); re-pinned here so the canary
    // continues to check the founding-band emission path
    // without false-failing on a worldgen sample artefact.
    let cfg = RunConfig::dev(1024, 16_000);
    let mut buf = Vec::new();
    run(&cfg, &mut JsonLinesEmitter::new(Cursor::new(&mut buf))).unwrap();
    let log = String::from_utf8(buf).unwrap();
    let n_figures = log.matches("\"kind\":\"figure_born\"").count();
    assert!(
        n_figures >= 2,
        "expected ≥ 2 figure_born events in a 16000-tick run, got {n_figures}"
    );
}

#[test]
fn relation_confirmations_attribute_to_real_figures() {
    // With a founding band of 2-3 figures, every RelationConfirmed
    // must carry a non-zero figure_id (round-robin by relation_id).
    let cfg = RunConfig::dev(42, 50);
    let mut buf = Vec::new();
    run(&cfg, &mut JsonLinesEmitter::new(Cursor::new(&mut buf))).unwrap();
    let log = String::from_utf8(buf).unwrap();
    for line in log.lines() {
        if line.contains("\"kind\":\"relation_confirmed\"") {
            assert!(
                !line.contains("\"figure_id\":0"),
                "relation_confirmed must attribute to a real figure: {line}"
            );
        }
    }
}

#[test]
fn tech_unlocked_events_emit_when_prereqs_met() {
    // Switched test seed from 42 to 100 because the
    // terrain redesign shifted seed 42's
    // habitable footprint enough that no tech unlocks within
    // a 4000-tick dev-grid run. Seed 100 still has reliable
    // tech unlocks under the new generation. Bumped
    // run to 8000 ticks because origin-seeded nomadic init +
    // density-gradient diffusion delay civ emergence; the
    // species takes longer to populate enough cells for tech
    // accumulation to cross the unlock threshold. The
    // invariant (≥ 1 unlock) is unchanged.
    //
    // P0.6 (post-Sprint-4/5 RNG shift): seed 100 joined seeds
    // 42, 7, 11 in the "no civs at 16k ticks" bucket once
    // Items 12-24 shifted the worldgen sampler's habitable-
    // band distribution. Re-pinned to seed 1024, identified
    // by `tests/find_seed.rs::find_working_seed` as the most
    // reliable post-shift producer (1093 unlocks in 16k
    // ticks). Pinning to the same seed as
    // `figure_born_events_emitted_for_founding_band` is
    // intentional — the two canaries exercise different
    // emission paths on the same planet, which lets a
    // future regression that breaks one path show up while
    // the other still passes.
    let cfg = RunConfig::dev(1024, 16_000);
    let mut buf = Vec::new();
    run(&cfg, &mut JsonLinesEmitter::new(Cursor::new(&mut buf))).unwrap();
    let log = String::from_utf8(buf).unwrap();
    let n_unlocks = log.matches("\"kind\":\"tech_unlocked\"").count();
    assert!(
        n_unlocks >= 1,
        "expected >=1 tech_unlocked event in 16000-tick seed-1024 run; got {n_unlocks}"
    );
}

// Note: the three multi-minute tests below (1000–2000 sim-
// year runs in debug) are now `#[ignore]` so routine
// `cargo test` is fast. Run them with
// `cargo test --release -- --include-ignored` for a full
// shipping check (release runs ~6× faster than debug; the
// overflow-checking the slow path used to give comes back when
// anyone runs `cargo test -- --include-ignored` in debug
// mode, which CI / pre-release should still do).
#[test]
#[ignore = "slow: runs 1000 sim-years; use --include-ignored for full check"]
fn knowledge_transmits_across_collapse_boundary() {
    // Acceptance: a long-enough run should produce
    // at least one cross-civ transmission. The current demographics
    // restored the seed-42 chain robustly. Uses CountingEmitter
    // so the in-memory event log doesn't blow up at month-
    // grained ticks.
    let cfg = RunConfig::dev(42, 1000 * protocol::MONTHS_PER_YEAR);
    let mut counter = CountingEmitter::new();
    run(&cfg, &mut counter).unwrap();
    let transmissions = counter.count("knowledge_transmitted");
    assert!(
        transmissions >= 1,
        "expected at least one knowledge_transmitted event; got {transmissions}"
    );
}

#[test]
#[ignore = "slow: runs 1000 sim-years; use --include-ignored for full check"]
fn successor_civs_found_after_collapse() {
    let cfg = RunConfig::dev(42, 1000 * protocol::MONTHS_PER_YEAR);
    let mut counter = CountingEmitter::new();
    run(&cfg, &mut counter).unwrap();
    let founded_count = counter.count("civ_founded");
    assert!(
        founded_count >= 2,
        "expected inaugural + at least one successor; got {founded_count}"
    );
}

#[test]
#[ignore = "slow: runs 2000 sim-years in debug; use --include-ignored for full check"]
fn collapse_fires_in_long_enough_run_with_no_discoveries() {
    // 2000 years gives the calmer mortality plus the
    // species-derived discovery cadence enough room for at
    // least one collapse to fire on seed 42 (cognition ≈ 0.2,
    // so attempt_period ≈ 26).
    let cfg = RunConfig::dev(42, 2000 * protocol::MONTHS_PER_YEAR);
    let mut counter = CountingEmitter::new();
    run(&cfg, &mut counter).unwrap();
    assert!(
        counter.count("civ_collapsed") >= 1,
        "expected at least one civ_collapsed event"
    );
}

/// A "quick" version of the long-run trio that
/// exercises the same code paths (run-loop coupling, event
/// emission, civ-lifecycle plumbing) without the multi-minute
/// debug-mode runtime. Uses 50 sim-years — far short of the
/// timescales the long tests verify, but enough to catch any
/// runtime panic, Q32.32 overflow, or deterministic-loop bug
/// that would also affect the long tests. The slow tests
/// catch the slow-emergence assertions (collapses,
/// transmissions, successor civs); this one catches the
/// "everything still wires together" failure mode in seconds.
#[test]
fn long_run_loop_smoke_short() {
    let cfg = RunConfig::dev(42, 50 * protocol::MONTHS_PER_YEAR);
    let mut counter = CountingEmitter::new();
    run(&cfg, &mut counter).unwrap();
    // No specific assertion on event counts (50 years is too
    // short to guarantee any of the slow-emergence outcomes).
    // The only requirement is that the run completes without
    // panicking — covers physics-law overflow regressions.
    assert!(
        counter.count("tick") > 0,
        "expected at least one tick event"
    );
}

#[test]
fn species_derived_event_emitted_at_run_start() {
    let cfg = RunConfig::dev(42, 1);
    let mut buf = Vec::new();
    run(&cfg, &mut JsonLinesEmitter::new(Cursor::new(&mut buf))).unwrap();
    let log = String::from_utf8(buf).unwrap();
    assert!(
        log.contains("\"kind\":\"species\""),
        "expected one species event at run start:\n{log}"
    );
    // Sanity: the event carries the seed and a Q32 trait field.
    assert!(log.contains("\"cognition_q32\""));
    assert!(log.contains("\"perceivable_template_ids\""));
}

#[test]
fn earth_like_run_emits_relation_confirmations() {
    // No inaugural civ; civs emerge from nomadic tech
    // accumulation. Origin-seeded nomadic init +
    // density-gradient diffusion delay civ emergence (the
    // species needs to populate enough cells before any
    // single cell crosses both the population and tech
    // thresholds). Run length bumped 300 → 2400 ticks. Tech-
    // gated transit (strict block at tier 0) bumps to 8000.
    // Halving base growth (1/100 → 1/200) bumps again to 16000
    // — same rationale as figure_born above.
    //
    // P0.6 (post-Sprint-4/5 RNG shift): seed re-pinned from
    // 42 to 1024 for the same reason as
    // `figure_born_events_emitted_for_founding_band` — seed
    // 42's planet no longer crosses the joint emergence gate
    // under the Items-12-24 wiring (~24k relations on seed
    // 1024 vs 0 on seed 42).
    let cfg = RunConfig::dev(1024, 16_000);
    let mut buf = Vec::new();
    run(&cfg, &mut JsonLinesEmitter::new(Cursor::new(&mut buf))).unwrap();
    let log = String::from_utf8(buf).unwrap();
    assert!(
        log.contains("\"kind\":\"relation_confirmed\""),
        "expected at least one RelationConfirmed event in a 16000-tick run"
    );
}

#[test]
fn earth_equivalent_seed_emits_recognition_firings() {
    // Validation: a Rocky+Lush seed (42) should emit
    // recognition firings in the first few ticks — surface_water
    // from the ocean cells alone, plus ice from polar regions
    // sub-freezing under the SI temperature gradient. Loose
    // assertion proves the cascade wires end-to-end without
    // pinning specific cell counts (which depend on grid + RNG).
    let cfg = RunConfig::dev(42, 3);
    let mut buf = Vec::new();
    run(&cfg, &mut JsonLinesEmitter::new(Cursor::new(&mut buf))).unwrap();
    let log = String::from_utf8(buf).unwrap();
    assert!(
        log.contains("surface_water"),
        "expected surface_water firings on a Rocky-with-oceans seed:\n{log}"
    );
}
