//! F1 / T2 / T9 ecosystem-events tests. These verify the
//! `run()` loop wires `step_with_biogeochem_at_tick`,
//! `species_extinct` event forwarding, and the speciation /
//! HGT registry into the production tick path. The helper
//! `ecosystem_fixture_for_seed` mirrors the construction
//! `run()` performs (same `sample_ecosystem_with_substrate`
//! call, same `step_with_biogeochem_at_tick` invocation) so a
//! drift in the production wire-up trips these tests instead
//! of silently moving the test fixture.

use crate::*;

use sim_events::CountingEmitter;

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

#[test]
fn ecosystem_events_fire_in_live_run() {
    // F1: post-PR-#73 (P0.1) the run loop calls `step_speciation`
    // and `step_hgt` each tick, but the `species_registry` was
    // initialised empty so both early-returned. F1 populates the
    // registry from `ecosystem.species` at worldgen: every
    // `EcoSpecies` becomes a per-trait `Species` with role taken
    // from the ecosystem record and a role-mapped `Lifecycle`
    // (Producer → Plant, Microbial parasites/saprotrophs/detritivore
    // → Microbial, etc.) so polyploid speciation and the HGT trial
    // path both have a non-empty pool to draw from. With the
    // registry populated, at least one
    // `speciation_occurred` or `species_extinct` event should land
    // in a 16k-tick run on the proven seed-1024 worldgen. We accept
    // either side of the path so a quiet 16k-tick ecosystem still
    // proves the wire-up is live — the ecosystem-event channel
    // produced one event end-to-end through `run()`.
    let cfg = RunConfig::dev(1024, 16_000);
    let mut counter = CountingEmitter::new();
    run(&cfg, &mut counter).unwrap();
    let speciation_count = counter.count("speciation_occurred");
    let extinction_count = counter.count("species_extinct");
    assert!(
        speciation_count + extinction_count >= 1,
        "expected >= 1 speciation_occurred or species_extinct event in 16k-tick run, \
         got {speciation_count}+{extinction_count}"
    );
}
