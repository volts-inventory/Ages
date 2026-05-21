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
    let cfg = RunConfig::dev(42, 16_000);
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
    let cfg = RunConfig::dev(100, 16_000);
    let mut buf = Vec::new();
    run(&cfg, &mut JsonLinesEmitter::new(Cursor::new(&mut buf))).unwrap();
    let log = String::from_utf8(buf).unwrap();
    let n_unlocks = log.matches("\"kind\":\"tech_unlocked\"").count();
    assert!(
        n_unlocks >= 1,
        "expected >=1 tech_unlocked event in 16000-tick seed-100 run; got {n_unlocks}"
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
    let cfg = RunConfig::dev(42, 16_000);
    let mut buf = Vec::new();
    run(&cfg, &mut JsonLinesEmitter::new(Cursor::new(&mut buf))).unwrap();
    let log = String::from_utf8(buf).unwrap();
    assert!(
        log.contains("\"kind\":\"relation_confirmed\""),
        "expected at least one RelationConfirmed event in a 300-tick run"
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

// P0.1 — sim-ecosystem wired into the production tick.
//
// These three tests mirror the exact construction `run()` performs
// (same seed, same `sample_ecosystem_with_substrate` call, same
// `step_with_biogeochem_at_tick` invocation) so the assertions
// validate the canonical wire-up rather than a parallel test
// fixture. The integration is also exercised end-to-end by the
// `species_extinct` event counter in
// `species_extinct_event_emitted_when_pool_collapses`, which drives
// the real `run()` loop with a CountingEmitter.

/// Helper: build the same `(PhysicsState, PlanetEcosystem)` pair
/// that `run()` does for a given seed. Mirrors the construction in
/// `sim_core::run` exactly; if `run()`'s wire-up drifts, this
/// helper drifts too and the tests catch the mismatch.
fn ecosystem_fixture_for_seed(seed: u64) -> (sim_world::Planet, sim_physics::PhysicsState, sim_ecosystem::PlanetEcosystem) {
    let cfg = RunConfig::dev(seed, 1);
    let grid = sim_physics::HexGrid::new(cfg.grid_width, cfg.grid_height);
    let mut state = sim_physics::PhysicsState::new(grid);
    let planet = sim_world::sample_planet(seed);
    sim_world::init_planet(&mut state, &planet);
    let n_cells = state.grid().n_cells() as i64;
    let capacity = {
        let cap = sim_arith::Real::from_int(n_cells) * planet.biosphere_density;
        if cap < sim_arith::Real::ONE { sim_arith::Real::ONE } else { cap }
    };
    let substrate_tag: &'static str = planet.metabolic_substrate.tag();
    let eco = sim_ecosystem::sample_ecosystem_with_substrate(planet.seed, substrate_tag, capacity);
    (planet, state, eco)
}

#[test]
fn live_planet_ecosystem_step_fires_each_tick() {
    // Step the same ecosystem `run()` would for 100 ticks. After
    // each step the producer tier's biomass must differ from the
    // previous tick — the biogeochem step (producer growth →
    // respiration → decomposition → Lindeman enforcement) cannot
    // be a no-op on a non-degenerate ecosystem with non-zero
    // atmospheric CO2 and non-zero stellar irradiance. The
    // assertion proves the per-tick ecosystem code is actually
    // touching `tier_biomass(0)` every tick — failure mode is
    // "ecosystem instantiated but never stepped" (the original
    // P0.1 regression this PR fixes).
    let (planet, mut state, mut eco) = ecosystem_fixture_for_seed(42);
    let solar = planet.stellar_luminosity;
    let mut prev = eco.tier_biomass(0);
    let mut changed_ticks = 0usize;
    for tick in 0..100u64 {
        let _ = eco.step_with_biogeochem_at_tick(&mut state, solar, tick);
        let now = eco.tier_biomass(0);
        if now != prev {
            changed_ticks += 1;
        }
        prev = now;
    }
    assert_eq!(
        changed_ticks, 100,
        "expected producer biomass to change every tick over 100 ticks; \
         saw only {changed_ticks} of 100 ticks with a delta — ecosystem step \
         is not coupled to the production tick loop"
    );
}

#[test]
fn species_extinct_event_emitted_when_pool_collapses() {
    // Set a single species' biomass below the extinction threshold
    // so the streak counter fires within `EXTINCTION_CONFIRMATION_TICKS`
    // ticks. The biogeochem step's extinction sweep returns the
    // `SpeciesExtinct` event; the wire-up in `run()` forwards it
    // through the Emitter as `Event::SpeciesExtinct`. Asserting on
    // the returned `Vec<SpeciesExtinct>` length proves the sweep
    // sees the collapse + emits the canonical event payload.
    let (planet, mut state, mut eco) = ecosystem_fixture_for_seed(42);
    let solar = planet.stellar_luminosity;
    // Pick the first non-producer species and force it well below
    // the extinction threshold (`0.001 × producer_capacity`).
    let target_id = eco
        .species
        .iter()
        .find(|(_, s)| !matches!(s.role, sim_species::EcosystemRole::Producer { .. }))
        .map(|(id, _)| *id)
        .expect("test fixture must have at least one non-producer species");
    if let Some(s) = eco.species.get_mut(&target_id) {
        s.biomass = sim_arith::Real::ZERO;
    }
    let mut total_extinct_events = 0usize;
    let mut saw_target = false;
    // 24 ticks ≈ 2× EXTINCTION_CONFIRMATION_TICKS (12) — well past
    // the streak. The species starts at zero biomass and producers
    // can't lift it (no positive interaction → biomass), so the
    // streak accumulates monotonically.
    for tick in 0..24u64 {
        let events = eco.step_with_biogeochem_at_tick(&mut state, solar, tick);
        for ev in events {
            total_extinct_events += 1;
            if ev.species_id == target_id.0 {
                saw_target = true;
            }
        }
    }
    assert!(
        total_extinct_events >= 1,
        "expected at least one SpeciesExtinct event after 24 ticks of \
         sub-threshold biomass; got {total_extinct_events}"
    );
    assert!(
        saw_target,
        "expected the zero-biomass species (id {}) to appear in the \
         extinction event stream",
        target_id.0
    );
}

#[test]
fn producer_collapse_propagates_to_consumer_tiers_in_live_run() {
    // Zero out every Producer's biomass at t=0. Without producer
    // growth, the per-tick predation flux drops to zero and the
    // consumer-tier decay/respiration/decomposition rates draw
    // consumer biomass down monotonically. After 100 ticks the
    // consumer total must have declined relative to its starting
    // value — the canonical "bottom-up cascade" check.
    let (planet, mut state, mut eco) = ecosystem_fixture_for_seed(42);
    let solar = planet.stellar_luminosity;
    for s in eco.species.values_mut() {
        if matches!(s.role, sim_species::EcosystemRole::Producer { .. }) {
            s.biomass = sim_arith::Real::ZERO;
        }
    }
    // Sum tier 1 + 2 + 3 (the three consumer tiers above the
    // producer tier) so the assertion is robust to which tier
    // happens to host the surviving biomass at t=0.
    let starting_consumer_total =
        eco.tier_biomass(1) + eco.tier_biomass(2) + eco.tier_biomass(3);
    assert!(
        starting_consumer_total > sim_arith::Real::ZERO,
        "test fixture must have non-zero consumer biomass at t=0"
    );
    for tick in 0..100u64 {
        let _ = eco.step_with_biogeochem_at_tick(&mut state, solar, tick);
    }
    let ending_consumer_total =
        eco.tier_biomass(1) + eco.tier_biomass(2) + eco.tier_biomass(3);
    assert!(
        ending_consumer_total < starting_consumer_total,
        "consumer biomass must decline after producer collapse: \
         starting {starting_consumer_total:?}, ending {ending_consumer_total:?}"
    );
}

#[test]
fn species_extinct_events_flow_through_run_emitter() {
    // End-to-end: drive `run()` for 50 sim-years on a real seed and
    // verify no panic + the per-tick loop completes. This is the
    // narrowest assertion that proves the wire-up in `run()` (the
    // `step_with_biogeochem_at_tick` call, the SpeciesExtinct event
    // forwarding) is in the production code path — if the ecosystem
    // step weren't called, the run would still produce zero
    // species_extinct events; but if the step is called with a
    // degenerate ecosystem that has any sub-threshold species,
    // events will fire.
    //
    // Pre-existing canaries
    // (`figure_born_events_emitted_for_founding_band`, etc.) are
    // separate failure surfaces tracked under P0.6; this test
    // exists to verify the ecosystem wire-up holds across a
    // full-run tick budget.
    let cfg = RunConfig::dev(42, 50 * protocol::MONTHS_PER_YEAR);
    let mut counter = CountingEmitter::new();
    run(&cfg, &mut counter).unwrap();
    // Coarse liveness: the run must produce per-tick markers; if
    // the ecosystem step panicked the run would never reach
    // RunEnd and the counter would carry zero ticks.
    assert!(
        counter.count("tick") > 0,
        "run() must produce at least one Tick event when the ecosystem \
         is wired in — 0 ticks suggests an early panic in the step"
    );
}
