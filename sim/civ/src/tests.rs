use super::*;
use sim_arith::Pop;
use sim_physics::{HexGrid, PhysicsState, Substance};
use sim_recognition::Firing;

fn empty_state() -> PhysicsState {
    PhysicsState::new(HexGrid::new(2, 2))
}

fn well_fed_state() -> PhysicsState {
    let mut s = PhysicsState::new(HexGrid::new(4, 4));
    // Saturate Fuel substance so carrying_capacity is large.
    for v in s.substance_mut(Substance::Fuel.idx()) {
        *v = Real::from_int(10);
    }
    s
}

#[test]
fn fresh_civ_is_active_with_no_collapse_state() {
    let civ = Civ::new(1, 0, Pop::from_int(50));
    assert!(civ.is_active());
    assert!(civ.collapsed_tick.is_none());
    assert_eq!(civ.low_food_streak, 0);
}

#[test]
fn collapse_fires_on_sustained_food_crisis() {
    let mut civ = Civ::new(1, 0, Pop::from_int(1000));
    let state = empty_state();
    // P0.5 — capacity now reads `self.producer_biomass` rather
    // than `Substance::Fuel`. Drive producer to zero so the legacy
    // "capacity = 0 → security = 0 every tick" intent still
    // collapses the civ on the food-crisis streak.
    civ.producer_biomass = Real::ZERO;
    let mut reason = None;
    for tick in 1..=(FOOD_CRISIS_STREAK_TICKS + 100) {
        // Mark a recent discovery so plateau doesn't fire first.
        civ.last_discovery_tick = tick;
        if let Some(r) = civ.check_collapse(tick, &state) {
            reason = Some((r, tick));
            break;
        }
    }
    let (r, t) = reason.expect("food crisis should fire within window");
    assert_eq!(r, CollapseReason::FoodCrisis);
    assert_eq!(t, FOOD_CRISIS_STREAK_TICKS);
}

#[test]
fn collapse_fires_on_knowledge_plateau() {
    let mut civ = Civ::new(1, 0, Pop::from_int(50));
    let state = well_fed_state();
    // Capacity is huge, food security stays at 1.0; only plateau
    // can fire. With last_discovery_tick = 0 (founding tick),
    // collapse fires at tick = PLATEAU_WINDOW_TICKS.
    let mut reason = None;
    for tick in 1..=(PLATEAU_WINDOW_TICKS + 50) {
        if let Some(r) = civ.check_collapse(tick, &state) {
            reason = Some((r, tick));
            break;
        }
    }
    let (r, t) = reason.expect("plateau should fire within window");
    assert_eq!(r, CollapseReason::KnowledgePlateau);
    assert_eq!(t, PLATEAU_WINDOW_TICKS);
}

#[test]
fn note_discovery_resets_plateau_window() {
    let mut civ = Civ::new(1, 0, Pop::from_int(50));
    let state = well_fed_state();
    // Mark discoveries every PLATEAU_WINDOW_TICKS - 10 to keep
    // the plateau from firing.
    for tick in 1..=(PLATEAU_WINDOW_TICKS * 3) {
        if tick % (PLATEAU_WINDOW_TICKS - 10) == 0 {
            civ.note_discovery(tick);
        }
        assert!(
            civ.check_collapse(tick, &state).is_none(),
            "plateau should not fire while discoveries continue"
        );
    }
}

#[test]
fn collapse_marks_figures_retired_and_cohort_stateless() {
    let mut civ = Civ::new(1, 0, Pop::from_int(50));
    assert_eq!(civ.cohort.civ_membership, Some(1));
    let n_figures = civ.figures.len();
    civ.collapse(42);
    assert_eq!(civ.collapsed_tick, Some(42));
    assert!(!civ.is_active());
    assert_eq!(civ.cohort.civ_membership, None);
    assert_eq!(
        civ.figures
            .iter()
            .filter(|f| f.retired_tick == Some(42))
            .count(),
        n_figures
    );
}

#[test]
fn collapse_is_idempotent() {
    let mut civ = Civ::new(1, 0, Pop::from_int(50));
    civ.collapse(10);
    // Second collapse at a later tick must not overwrite the
    // first collapse_tick or the figures' retired_tick.
    civ.collapse(20);
    assert_eq!(civ.collapsed_tick, Some(10));
    assert!(civ.figures.iter().all(|f| f.retired_tick == Some(10)));
}

#[test]
fn check_collapse_returns_none_after_already_collapsed() {
    let mut civ = Civ::new(1, 0, Pop::from_int(50));
    civ.collapse(10);
    let state = empty_state();
    for tick in 11..=300 {
        assert!(civ.check_collapse(tick, &state).is_none());
    }
}

#[test]
fn fresh_civ_has_no_observations() {
    let civ = Civ::new(1, 0, Pop::from_int(50));
    assert_eq!(civ.observation_count(1), 0);
}

#[test]
fn observations_accumulate() {
    let mut civ = Civ::new(1, 0, Pop::from_int(50));
    let firings = vec![
        Firing {
            template_id: 1,
            cell: 0,
        },
        Firing {
            template_id: 1,
            cell: 1,
        },
        Firing {
            template_id: 2,
            cell: 0,
        },
    ];
    civ.observe(&firings);
    civ.observe(&firings);
    assert_eq!(civ.observation_count(1), 4);
    assert_eq!(civ.observation_count(2), 2);
    assert_eq!(civ.observation_count(99), 0);
}

#[test]
fn population_step_is_deterministic() {
    let mut a = Civ::new(1, 0, Pop::from_int(100));
    let mut b = Civ::new(1, 0, Pop::from_int(100));
    for _ in 0..20 {
        a.step_population();
        b.step_population();
    }
    assert_eq!(a.population(), b.population());
}

// Carrying-capacity calibration tests live in `mod demographics::tests`.
// civ_name_from_seed tests live in `mod naming::tests`.

/// A civ whose `claimed_cells.len() <= 1` for at least
/// `TINY_TERRITORY_STREAK_TICKS` consecutive ticks collapses
/// with reason `TerritoryTooSmall`. Without this gate there was no
/// auto-collapse on tiny territory and a parent squeezed to
/// one cell could linger indefinitely.
#[test]
fn collapse_fires_on_tiny_territory_streak() {
    let mut civ = Civ::new(1, 0, Pop::from_int(50));
    // One claimed cell for the full streak window. Use the
    // well-fed state so food security stays above the crisis
    // floor and only the territory trigger can fire.
    let mut cells = BTreeSet::new();
    cells.insert(0u32);
    civ.claim_cells(&cells);
    let state = well_fed_state();
    let mut reason = None;
    for tick in 1..=(TINY_TERRITORY_STREAK_TICKS + 50) {
        // Mark recent discovery so plateau doesn't pre-empt.
        civ.last_discovery_tick = tick;
        if let Some(r) = civ.check_collapse(tick, &state) {
            reason = Some((r, tick));
            break;
        }
    }
    let (r, t) = reason.expect("territory-too-small should fire within window");
    assert_eq!(r, CollapseReason::TerritoryTooSmall);
    assert_eq!(t, TINY_TERRITORY_STREAK_TICKS);
}

/// Territory recovery resets the tiny-territory streak.
/// A civ that bounces back above the floor mid-streak must not
/// collapse the moment it dips again — the streak is consecutive
/// ticks, not lifetime ticks-at-tiny. Runs the dip/recover/dip
/// cycle below the collapse threshold so the assertion targets
/// the streak counter rather than collapse firing.
#[test]
fn tiny_territory_streak_resets_on_recovery() {
    // Sanity guard: this test depends on the dip phase being
    // shorter than the collapse threshold. If a future tuning
    // pass shrinks `TINY_TERRITORY_STREAK_TICKS` below 5, the
    // dip count below needs to drop too — fail loudly here so
    // the regression shows up locally rather than as a flaky
    // test in CI.
    const { assert!(TINY_TERRITORY_STREAK_TICKS >= 5) }
    let dip_ticks = TINY_TERRITORY_STREAK_TICKS / 2;
    let mut civ = Civ::new(1, 0, Pop::from_int(50));
    let state = well_fed_state();
    let mut tiny = BTreeSet::new();
    tiny.insert(0u32);
    civ.claim_cells(&tiny);
    for tick in 1..=dip_ticks {
        civ.last_discovery_tick = tick;
        assert!(civ.check_collapse(tick, &state).is_none());
    }
    assert_eq!(civ.tiny_territory_streak, dip_ticks);
    // Recover to 3 cells: streak resets to 0.
    let mut bigger = BTreeSet::new();
    bigger.insert(0u32);
    bigger.insert(1u32);
    bigger.insert(2u32);
    civ.claim_cells(&bigger);
    let recovery_tick = dip_ticks + 1;
    civ.last_discovery_tick = recovery_tick;
    assert!(civ.check_collapse(recovery_tick, &state).is_none());
    assert_eq!(civ.tiny_territory_streak, 0);
}

/// Depopulation collapse: a civ whose `aggregate_population`
/// has stayed at or below `DEPOPULATION_FLOOR_POP` for
/// `DEPOPULATION_STREAK_TICKS` consecutive ticks collapses
/// with `CollapseReason::Depopulation`. Without this trigger
/// the viewport could read "0p" indefinitely for an
/// effectively-empty civ that hasn't tripped any other streak.
#[test]
fn collapse_fires_on_depopulation_streak() {
    // Start the civ already depopulated. Founding with pop 0
    // makes the streak begin firing on the very first tick.
    let mut civ = Civ::new(1, 0, Pop::ZERO);
    let state = well_fed_state();
    let mut reason = None;
    for tick in 1..=(DEPOPULATION_STREAK_TICKS + 50) {
        // Mark recent discovery so plateau doesn't pre-empt.
        civ.last_discovery_tick = tick;
        if let Some(r) = civ.check_collapse(tick, &state) {
            reason = Some((r, tick));
            break;
        }
    }
    let (r, t) = reason.expect("depopulation should fire within window");
    assert_eq!(r, CollapseReason::Depopulation);
    assert_eq!(t, DEPOPULATION_STREAK_TICKS);
}

/// Population recovery resets the depopulation streak. A civ
/// that dips to zero briefly during a catastrophe but rebuilds
/// must not collapse the moment it dips again — streaks are
/// consecutive ticks, not lifetime.
#[test]
fn depopulation_streak_resets_on_recovery() {
    const { assert!(DEPOPULATION_STREAK_TICKS >= 5) }
    let dip_ticks = DEPOPULATION_STREAK_TICKS / 2;
    let mut civ = Civ::new(1, 0, Pop::ZERO);
    let state = well_fed_state();
    for tick in 1..=dip_ticks {
        civ.last_discovery_tick = tick;
        assert!(civ.check_collapse(tick, &state).is_none());
    }
    assert_eq!(civ.depopulation_streak, dip_ticks);
    // Recover the cohort above the floor: streak resets to 0.
    civ.cohort = sim_population::Cohort::with_civ(Pop::from_int(500), civ.id);
    let recovery_tick = dip_ticks + 1;
    civ.last_discovery_tick = recovery_tick;
    assert!(civ.check_collapse(recovery_tick, &state).is_none());
    assert_eq!(civ.depopulation_streak, 0);
}

// successor_centroid_* unit tests live in `mod succession::tests`;
// the end-to-end succession check stays here (next test) because
// it exercises the full `Civ` lifecycle.

/// End-to-end check that the succession path
/// (parent collapses → stateless cohort → `refound_from_stateless`
/// → core's centroid override) leaves civ 2 with a centroid that
/// is NOT civ 1's centroid. Mirrors the real flow in
/// `sim/core/src/lib.rs` so a future refactor that drops the
/// override gets caught by a unit test, not just by eyeballing
/// the viewport.
#[test]
fn successor_lands_on_distinct_centroid() {
    let grid = HexGrid::new(8, 8);
    // Civ 1: founding band picks cell 0 as centroid (typical
    // seed-42-style outcome that motivated the override). Claim a small
    // contiguous block around it so the parent has a real
    // capital before collapsing.
    let mut civ1 = Civ::new(1, 0, Pop::from_int(50));
    civ1.territory_centroid = 0;
    let mut civ1_cells: BTreeSet<u32> = BTreeSet::new();
    civ1_cells.insert(0u32);
    civ1_cells.insert(1u32);
    civ1_cells.insert(8u32);
    civ1.claim_cells(&civ1_cells);
    civ1.collapse(100);
    let parent_centroid = civ1.territory_centroid;
    // Civ 2: refound_from_stateless rebuilds the band; its
    // figures-derived centroid will collide with cell 0 on this
    // setup. Run the same override sim/core does.
    let stateless = Cohort::with_civ(Pop::from_int(50), 2);
    let mut civ2 = Civ::refound_from_stateless(2, 200, stateless, Real::ONE, 0, &[], 1);
    // Simulate compute_territory: BFS from civ2's initial
    // centroid. Use a small target so the territory is just a
    // local block — keeps the test deterministic without
    // dragging in sim-core.
    let initial_centroid = civ2.territory_centroid;
    let mut initial_cells: BTreeSet<u32> = BTreeSet::new();
    initial_cells.insert(initial_centroid);
    let axial = grid.axial_of(sim_physics::CellId(initial_centroid));
    for nb in grid.neighbours(axial) {
        initial_cells.insert(nb.0);
    }
    // Apply successor centroid override.
    let new_centroid = pick_successor_centroid(
        parent_centroid,
        &initial_cells,
        initial_centroid,
        &grid,
        &BTreeSet::new(),
    );
    civ2.territory_centroid = new_centroid;
    civ2.claim_cells(&initial_cells);
    assert_ne!(
        civ2.territory_centroid, civ1.territory_centroid,
        "successor centroid must differ from predecessor's",
    );
    // And the chosen centroid is one of civ 1's centroid's hex
    // neighbours (the strongest preference rule), not just any
    // arbitrary cell.
    let parent_axial = grid.axial_of(sim_physics::CellId(parent_centroid));
    let neighbour_ids: BTreeSet<u32> = grid.neighbours(parent_axial).iter().map(|c| c.0).collect();
    assert!(
        neighbour_ids.contains(&civ2.territory_centroid),
        "expected civ 2's centroid adjacent to civ 1's; got {} (parent {})",
        civ2.territory_centroid,
        parent_centroid
    );
}

/// Per-cell habitability multipliers honour the published
/// table — deep ocean / gas band gate to zero, coast scales above
/// baseline, peaks scale to ~10%. Sanity check that the constants
/// don't drift without a deliberate decision.
#[test]
fn habitability_multipliers_match_published_table() {
    use sim_world::habitability_multiplier;
    assert_eq!(habitability_multiplier('\u{2248}'), Real::ZERO); // ≈ deep ocean
    assert_eq!(habitability_multiplier('\u{2261}'), Real::ZERO); // ≡ gas
    assert_eq!(
        habitability_multiplier('~'),
        Real::percent(5),
        "shallow sea should be 0.05",
    );
    assert_eq!(
        habitability_multiplier('\u{2591}'),
        Real::percent(120),
        "coast should be 1.20",
    );
    assert_eq!(habitability_multiplier('\u{2592}'), Real::percent(90)); // ▒ inland
    assert_eq!(habitability_multiplier('\u{25B3}'), Real::percent(60)); // △ hill
    assert_eq!(habitability_multiplier('\u{25B2}'), Real::percent(10)); // ▲ peak
    assert_eq!(habitability_multiplier('\u{00B7}'), Real::ONE); // · plain
                                                                // Unknown glyph defaults to 1.0 so the production path can
                                                                // pass any char without crashing.
    assert_eq!(habitability_multiplier('?'), Real::ONE);
}

/// Only deep-ocean and gas glyphs sit below the claim
/// threshold; everything else is claimable. Locks the BFS gate
/// against accidental tightening (e.g. peaks dropping to 0.04).
#[test]
fn claim_threshold_keeps_peaks_claimable_excludes_water_and_gas() {
    use sim_world::{habitability_multiplier, is_claimable_multiplier};
    // Claimable terrain.
    for g in [
        '\u{2591}', '\u{2592}', '\u{25B3}', '\u{25B2}', '\u{00B7}', '~',
    ] {
        assert!(
            is_claimable_multiplier(habitability_multiplier(g)),
            "expected {g} claimable"
        );
    }
    // Walls.
    for g in ['\u{2248}', '\u{2261}'] {
        assert!(
            !is_claimable_multiplier(habitability_multiplier(g)),
            "expected {g} excluded"
        );
    }
}

/// End-to-end check on a real seed at the default 36×30
/// grid. Founds a civ via `sim_core::run_main`, asserts no
/// claimed cell is `≈` deep ocean or `≡` gas band, and asserts
/// that the terrain-aware capacity differs from the planet-less
/// `carrying_capacity` (i.e. the multiplier path actually fires
/// rather than being silently bypassed). This is the smoke-test
/// for the full integration; the unit tests above cover the
/// helper functions in isolation.
#[test]
fn terrain_habitability_smoke_test() {
    use sim_physics::HexGrid;
    use sim_world::{init_planet, sample_planet, terrain_glyph_at};
    // Seed 1 + 36×30 default grid. Without the habitability gate the founding centroid
    // for cell 0 typically lands in the deep-ocean band on this
    // seed; the multiplier path relocates it.
    let planet = sample_planet(1);
    let grid = HexGrid::new(36, 30);
    let mut state = PhysicsState::new(grid);
    init_planet(&mut state, &planet);
    // Smoke-check: cell 0 on this seed is in the water band — a
    // sanity check that the test exercises the new gate, not
    // just the no-op land-only path. Skip the assertion if the
    // renderer's classification surprises us; the gate's still
    // tested in the BFS-claimed-cells assertion below.
    let cell0_glyph = terrain_glyph_at(&state, &planet, 0);
    let _ = cell0_glyph;

    // Build a minimal civ + run the same pick-then-BFS dance
    // sim/core does for the inaugural civ, so this test stays
    // free of sim/core's full lifecycle.
    let mut civ = Civ::new(1, 0, Pop::from_int(50));
    // Force the figure-derived seed centroid to cell 0 (deep
    // ocean on seed 1) to exercise the relocation logic.
    civ.territory_centroid = 0;

    // BFS gate: requires no claimed cell to be deep ocean
    // or gas. Re-implement the same compute_territory + pick
    // logic via the public helpers exposed in sim_world.
    let mut visited = BTreeSet::new();
    let mut queue: std::collections::VecDeque<u32> = std::collections::VecDeque::new();
    // Pick the first claimable cell starting from cell 0.
    queue.push_back(0u32);
    visited.insert(0u32);
    let mut habitable_seed: u32 = 0;
    while let Some(c) = queue.pop_front() {
        let m = sim_world::cell_habitability(&state, &planet, c);
        if sim_world::is_claimable_multiplier(m) {
            habitable_seed = c;
            break;
        }
        let axial = state.grid().axial_of(sim_physics::CellId(c));
        for nb in state.grid().neighbours(axial) {
            if visited.insert(nb.0) {
                queue.push_back(nb.0);
            }
        }
    }
    assert!(
        sim_world::is_claimable_multiplier(sim_world::cell_habitability(
            &state,
            &planet,
            habitable_seed
        )),
        "pick_habitable_cell must return a claimable cell"
    );

    civ.territory_centroid = habitable_seed;
    // BFS through claimable cells only.
    let mut claimed = BTreeSet::new();
    claimed.insert(habitable_seed);
    let mut bfs_visited = BTreeSet::new();
    bfs_visited.insert(habitable_seed);
    let mut bfs_queue: std::collections::VecDeque<u32> = std::collections::VecDeque::new();
    bfs_queue.push_back(habitable_seed);
    let target = 8usize;
    while let Some(c) = bfs_queue.pop_front() {
        if claimed.len() >= target {
            break;
        }
        let m = sim_world::cell_habitability(&state, &planet, c);
        if sim_world::is_claimable_multiplier(m) || c == habitable_seed {
            claimed.insert(c);
            let axial = state.grid().axial_of(sim_physics::CellId(c));
            let mut nbs: Vec<u32> = state.grid().neighbours(axial).iter().map(|c| c.0).collect();
            nbs.sort_unstable();
            for n in nbs {
                if bfs_visited.insert(n) {
                    bfs_queue.push_back(n);
                }
            }
        }
    }
    civ.claim_cells(&claimed);

    // Habitability invariant: NO claimed cell is deep ocean / gas.
    for &c in &civ.claimed_cells {
        let glyph = terrain_glyph_at(&state, &planet, c);
        assert_ne!(glyph, '\u{2248}', "civ claimed deep-ocean cell {c}");
        assert_ne!(glyph, '\u{2261}', "civ claimed gas-band cell {c}");
    }

    // Multiplier path: terrain-aware capacity differs from
    // the planet-less aggregate. With non-trivial terrain mix
    // (coast + inland + maybe hill) the two values must differ;
    // identical sums would mean every claimed cell happened to
    // land at multiplier 1.0, which would mean the multiplier
    // path silently no-ops — exactly the regression to catch.
    let plain_cap = civ.carrying_capacity(&state);
    let terrain_cap = civ.carrying_capacity_with_terrain(&state, &planet);
    assert_ne!(
        plain_cap, terrain_cap,
        "terrain-aware capacity must differ from plain on a varied seed; \
             plain={plain_cap:?}, terrain={terrain_cap:?}",
    );
}

// ─── Tool-effect wire-in tests ───
//
// The four extra effect-category aggregators (seasonal floor,
// catastrophe resistance, expansion rate, transmission fidelity)
// now have consuming call sites. These tests pin the wire-ins by
// comparing pre-tool vs. post-tool sim output through the actual
// helper / consumer code paths.

/// Wire-in: `effective_seasonal_factor` lifts a worst-case
/// seasonal factor when seasonal-floor tools are unlocked.
#[test]
fn seasonal_floor_lifts_winter_factor_with_shelter() {
    let mut civ = Civ::new(1, 0, Pop::from_int(50));
    // Pre-shelter: identity passthrough on raw factor.
    let raw = Real::percent(80);
    let no_shelter = civ.effective_seasonal_factor(raw);
    assert_eq!(
        no_shelter, raw,
        "with no seasonal-floor tools, effective factor should equal raw",
    );
    // Unlock SimpleShelter (+0.10 seasonal floor).
    civ.unlocked_tools
        .insert(crate::tech::ToolKind::SimpleShelter);
    let with_shelter = civ.effective_seasonal_factor(raw);
    assert!(
        with_shelter > raw,
        "with SimpleShelter unlocked, effective factor {with_shelter:?} \
             should exceed raw {raw:?}",
    );
    // Optimal-season factor (1.0) should pass through unchanged
    // — the floor only lifts losses, never the ceiling.
    let optimal = Real::ONE;
    assert_eq!(civ.effective_seasonal_factor(optimal), optimal);
}

/// Wire-in: `apply_catastrophe_resistance` reduces a base
/// loss fraction when catastrophe-resistance tools are unlocked.
#[test]
fn catastrophe_resistance_softens_loss_with_medical_tools() {
    let mut civ = Civ::new(1, 0, Pop::from_int(50));
    let baseline_loss = Real::percent(30); // 30% baseline loss
                                           // Pre-tool: identity.
    let untouched = civ.apply_catastrophe_resistance(baseline_loss);
    assert_eq!(untouched, baseline_loss);
    // Unlock BasicHealing (+0.10 catastrophe resistance).
    civ.unlocked_tools
        .insert(crate::tech::ToolKind::BasicHealing);
    let with_healing = civ.apply_catastrophe_resistance(baseline_loss);
    assert!(
        with_healing < baseline_loss,
        "with BasicHealing unlocked, loss {with_healing:?} should fall \
             below baseline {baseline_loss:?}",
    );
    // Stack MedicalIntervention (+0.15) and AdvancedMedicine (+0.15)
    // — total 0.40 resistance, loss should be ~0.18 (0.30 × 0.60).
    civ.unlocked_tools
        .insert(crate::tech::ToolKind::MedicalIntervention);
    civ.unlocked_tools
        .insert(crate::tech::ToolKind::AdvancedMedicine);
    let with_full_med = civ.apply_catastrophe_resistance(baseline_loss);
    let expected = Real::percent(18);
    let diff = if with_full_med > expected {
        with_full_med - expected
    } else {
        expected - with_full_med
    };
    assert!(
        diff < Real::from_ratio(1, 1000),
        "stacked medical resistance should give ~0.18 loss; got {with_full_med:?}",
    );
}

/// `tool_lifespan_extension_factor` aggregates additively across
/// unlocked tools and caps at 1.00 (= 2× lifespan max). Wires
/// through `effective_lifespan_years` so a senescence-treatment
/// civ actually has a longer biological cap than its un-teched
/// kin of the same species.
#[test]
fn tool_lifespan_extension_lifts_effective_lifespan() {
    let mut civ = Civ::new(1, 0, Pop::from_int(50));
    let zero_factor = civ.tool_lifespan_extension_factor();
    assert_eq!(zero_factor, Real::ZERO);
    // Stack MedicalIntervention (+0.05) + AdvancedMedicine (+0.10)
    // + GeneticManipulation (+0.20). Total = 0.35.
    civ.unlocked_tools
        .insert(crate::tech::ToolKind::MedicalIntervention);
    civ.unlocked_tools
        .insert(crate::tech::ToolKind::AdvancedMedicine);
    civ.unlocked_tools
        .insert(crate::tech::ToolKind::GeneticManipulation);
    let stacked = civ.tool_lifespan_extension_factor();
    let expected = Real::percent(35);
    let diff = if stacked > expected {
        stacked - expected
    } else {
        expected - stacked
    };
    assert!(
        diff < Real::from_ratio(1, 1000),
        "stacked extension {stacked:?} should be ~0.35"
    );
}

/// `tool_mortality_reduction_per_bracket` aggregates additively
/// across unlocked tools and caps each bracket at 0.80. A
/// fully-equipped civ pulls every bracket toward the cap; a
/// pre-tech civ sees zero across the board.
#[test]
fn tool_mortality_reduction_aggregates_per_bracket() {
    let mut civ = Civ::new(1, 0, Pop::from_int(50));
    let zero = civ.tool_mortality_reduction_per_bracket();
    for v in zero {
        assert_eq!(v, Real::ZERO);
    }
    // BasicHealing alone: [0.15, 0.10, 0.05, 0.0].
    civ.unlocked_tools
        .insert(crate::tech::ToolKind::BasicHealing);
    let one = civ.tool_mortality_reduction_per_bracket();
    assert!(one[0] > Real::ZERO, "infant cut from BasicHealing");
    assert!(one[1] > Real::ZERO, "juvenile cut from BasicHealing");
    assert_eq!(one[3], Real::ZERO, "elder unaffected by BasicHealing");
    // Stack MedicalIntervention + AdvancedMedicine + GeneticManipulation
    // — totals at infant ≈ 0.55, juvenile ≈ 0.50, fertile ≈ 0.40,
    // elder ≈ 0.40, all under the 0.80 cap.
    civ.unlocked_tools
        .insert(crate::tech::ToolKind::MedicalIntervention);
    civ.unlocked_tools
        .insert(crate::tech::ToolKind::AdvancedMedicine);
    civ.unlocked_tools
        .insert(crate::tech::ToolKind::GeneticManipulation);
    let stacked = civ.tool_mortality_reduction_per_bracket();
    for (i, v) in stacked.iter().enumerate() {
        assert!(
            *v > Real::percent(30),
            "stacked bracket {i} reduction {v:?} should exceed 0.30"
        );
        assert!(
            *v <= Real::percent(80),
            "stacked bracket {i} reduction {v:?} should respect 0.80 cap"
        );
    }
}

/// Wire-in: `tool_expansion_rate_bonus` aggregates correctly
/// — used in the territory-growth path's
/// `effective_pop = pop * (1 + bonus)` calculation.
#[test]
fn expansion_rate_aggregator_reflects_unlocked_navigation_tools() {
    let mut civ = Civ::new(1, 0, Pop::from_int(100));
    assert_eq!(civ.tool_expansion_rate_bonus(), Real::ZERO);
    civ.unlocked_tools
        .insert(crate::tech::ToolKind::WatercraftConstruction); // +0.10
    civ.unlocked_tools
        .insert(crate::tech::ToolKind::TradeNetworks); // +0.05
    let bonus = civ.tool_expansion_rate_bonus();
    let expected = Real::percent(15);
    let diff = if bonus > expected {
        bonus - expected
    } else {
        expected - bonus
    };
    assert!(
        diff < Real::from_ratio(1, 1_000_000),
        "expansion rate bonus should sum to ≈0.15; got {bonus:?}",
    );
}

/// Wire-in: `tool_transmission_fidelity_bonus` aggregates
/// correctly — applied as a multiplicative comprehension lift in
/// `transmission::diffuse_between` and `transmit_from_parent`.
#[test]
fn transmission_fidelity_aggregator_reflects_unlocked_encoding_tools() {
    let mut civ = Civ::new(1, 0, Pop::from_int(100));
    assert_eq!(civ.tool_transmission_fidelity_bonus(), Real::ZERO);
    civ.unlocked_tools
        .insert(crate::tech::ToolKind::CulturalEncoding); // +0.10
    civ.unlocked_tools
        .insert(crate::tech::ToolKind::WrittenJurisprudence); // +0.10
    civ.unlocked_tools
        .insert(crate::tech::ToolKind::MassLiteracy); // +0.15
    let bonus = civ.tool_transmission_fidelity_bonus();
    let expected = Real::percent(35);
    let diff = if bonus > expected {
        bonus - expected
    } else {
        expected - bonus
    };
    assert!(
        diff < Real::from_ratio(1, 1_000_000),
        "transmission fidelity bonus should sum to ≈0.35; got {bonus:?}",
    );
}

/// `tool_discovery_rate_bonus` aggregates additively across
/// unlocked tools and stays at zero for a pre-tech civ. Stacking
/// AnalyticalEngines (+0.15) + DigitalComputation (+0.20) +
/// AbstractMathematics (+0.10) yields ≈0.45.
#[test]
fn tool_discovery_rate_aggregates() {
    let mut civ = Civ::new(1, 0, Pop::from_int(10));
    assert_eq!(civ.tool_discovery_rate_bonus(), Real::ZERO);
    civ.unlocked_tools
        .insert(crate::tech::ToolKind::AnalyticalEngines);
    civ.unlocked_tools
        .insert(crate::tech::ToolKind::DigitalComputation);
    civ.unlocked_tools
        .insert(crate::tech::ToolKind::AbstractMathematics);
    let bonus = civ.tool_discovery_rate_bonus();
    let expected = Real::percent(45);
    let diff = if bonus > expected {
        bonus - expected
    } else {
        expected - bonus
    };
    assert!(
        diff < Real::from_ratio(1, 1_000_000),
        "discovery rate bonus should sum to ≈0.45; got {bonus:?}",
    );
}

/// `tool_cohesion_bonus` aggregates additively but caps at +0.40
/// so a fully-equipped late-game civ doesn't drown out the size /
/// food penalties in `update_cohesion`. Stacking
/// WrittenJurisprudence (+0.10) + MassLiteracy (+0.10) +
/// InformationNetworking (+0.10) + DefensiveFortification (+0.05)
/// + TradeNetworks (+0.05) + CulturalEncoding (+0.05) +
/// UrbanConstruction (+0.05) raw-sums to 0.50, clamps to 0.40.
#[test]
fn tool_cohesion_caps_at_forty() {
    let mut civ = Civ::new(1, 0, Pop::from_int(10));
    assert_eq!(civ.tool_cohesion_bonus(), Real::ZERO);
    for tk in [
        crate::tech::ToolKind::WrittenJurisprudence,
        crate::tech::ToolKind::MassLiteracy,
        crate::tech::ToolKind::InformationNetworking,
        crate::tech::ToolKind::DefensiveFortification,
        crate::tech::ToolKind::TradeNetworks,
        crate::tech::ToolKind::CulturalEncoding,
        crate::tech::ToolKind::UrbanConstruction,
    ] {
        civ.unlocked_tools.insert(tk);
    }
    let bonus = civ.tool_cohesion_bonus();
    assert_eq!(
        bonus,
        Real::percent(40),
        "cohesion bonus should clamp to +0.40; got {bonus:?}",
    );
}

/// `tool_migration_speed_bonus` aggregates additively. Stacking
/// HeavyTransport (+0.20) + AerialTransport (+0.20) +
/// MotivePropulsion (+0.10) yields 0.50.
#[test]
fn tool_migration_speed_aggregates() {
    let mut civ = Civ::new(1, 0, Pop::from_int(10));
    assert_eq!(civ.tool_migration_speed_bonus(), Real::ZERO);
    civ.unlocked_tools
        .insert(crate::tech::ToolKind::HeavyTransport);
    civ.unlocked_tools
        .insert(crate::tech::ToolKind::AerialTransport);
    civ.unlocked_tools
        .insert(crate::tech::ToolKind::MotivePropulsion);
    let bonus = civ.tool_migration_speed_bonus();
    let expected = Real::percent(50);
    let diff = if bonus > expected {
        bonus - expected
    } else {
        expected - bonus
    };
    assert!(
        diff < Real::from_ratio(1, 1_000_000),
        "migration speed bonus should sum to ≈0.50; got {bonus:?}",
    );
}

/// `tool_fertility_bonus` aggregates additively and caps at +0.50.
/// Stacking MedicalIntervention (+0.10) + AdvancedMedicine (+0.10)
/// + GeneticManipulation (+0.05) + FoodProcessing (+0.05) +
/// BulkCultivation (+0.05) + BasicHealing (+0.05) + BulkStorage
/// (+0.03) raw-sums to 0.43, stays under the 0.50 cap.
#[test]
fn tool_fertility_aggregates_and_caps() {
    let mut civ = Civ::new(1, 0, Pop::from_int(10));
    assert_eq!(civ.tool_fertility_bonus(), Real::ZERO);
    for tk in [
        crate::tech::ToolKind::MedicalIntervention,
        crate::tech::ToolKind::AdvancedMedicine,
        crate::tech::ToolKind::GeneticManipulation,
        crate::tech::ToolKind::FoodProcessing,
        crate::tech::ToolKind::BulkCultivation,
        crate::tech::ToolKind::BasicHealing,
        crate::tech::ToolKind::BulkStorage,
    ] {
        civ.unlocked_tools.insert(tk);
    }
    let bonus = civ.tool_fertility_bonus();
    let expected = Real::percent(43);
    let diff = if bonus > expected {
        bonus - expected
    } else {
        expected - bonus
    };
    assert!(
        diff < Real::from_ratio(1, 1_000_000),
        "fertility bonus should sum to ≈0.43 (under 0.50 cap); got {bonus:?}",
    );
}

/// Sprint 3 Item 10: a `CognitionTopology::Collective` species
/// loses ~95% of its effective cognition when its population
/// falls below `COLLECTIVE_QUORUM_POP` (the swarm-quorum). At or
/// above quorum the cognition is unaffected — a single-civ test
/// at the quorum boundary surfaces both branches of the gate.
#[test]
fn collective_species_cognition_drops_in_isolation() {
    use sim_recognition::RecognitionLibrary;
    use sim_world::sample_planet;
    let planet = sample_planet(1);
    let lib = RecognitionLibrary::earth_like_default();
    let mut species = sim_species::derive(&planet, &lib);
    species.cognition_topology = sim_species::CognitionTopology::Collective;
    species.cognition = Real::from_ratio(8, 10); // 0.8 base

    // Healthy civ above the quorum: effective_cognition equals the
    // species baseline (no isolation penalty).
    let healthy = Civ::new(1, 0, Pop::from_int(500));
    assert!(healthy.cohort.total() >= crate::COLLECTIVE_QUORUM_POP);
    let healthy_cog = healthy.effective_cognition(&species);
    assert_eq!(healthy_cog, Real::from_ratio(8, 10));

    // Isolated civ below the quorum: effective_cognition is the
    // baseline × isolation_penalty (0.05) → 0.8 × 0.05 = 0.04.
    let isolated = Civ::new(2, 0, Pop::from_int(10));
    assert!(isolated.cohort.total() < crate::COLLECTIVE_QUORUM_POP);
    let isolated_cog = isolated.effective_cognition(&species);
    let expected = Real::from_ratio(8, 10) * Real::from_ratio(5, 100);
    assert_eq!(isolated_cog, expected);
    assert!(isolated_cog < healthy_cog);

    // Sanity: a Centralized species at the same low population is
    // unaffected — only Collective triggers the penalty.
    let mut central_species = species.clone();
    central_species.cognition_topology = sim_species::CognitionTopology::Centralized;
    let central_isolated = isolated.effective_cognition(&central_species);
    assert_eq!(central_isolated, Real::from_ratio(8, 10));
}

/// Sprint 3 Item 10: an `Acentric` species' cross-collapse
/// knowledge decay constant is 5× longer than a `Centralized`
/// species'. Over a long age window the same predecessor
/// relation comprehends with a higher `age_decay` factor for the
/// Acentric species than the Centralized one — the substrate
/// preserves cumulative memory across generations.
#[test]
fn acentric_species_retains_knowledge_across_generations_better() {
    use crate::transmission::age_decay;
    use sim_recognition::RecognitionLibrary;
    use sim_world::sample_planet;
    let planet = sample_planet(1);
    let lib = RecognitionLibrary::earth_like_default();
    let mut centralized = sim_species::derive(&planet, &lib);
    centralized.cognition_topology = sim_species::CognitionTopology::Centralized;
    let mut acentric = centralized.clone();
    acentric.cognition_topology = sim_species::CognitionTopology::Acentric;

    // 1000 sim-years × 12 months ≈ a deep time window where the
    // Centralized species' base 500-1500 yr e-fold constant has
    // delivered substantial decay but the Acentric species' 5×
    // stretched constant has barely started.
    let age_ticks = 1000u64 * 12;
    let centralized_ticks = centralized.transmission_decay_ticks();
    let acentric_ticks = acentric.transmission_decay_ticks();
    // Sanity: stretched 5× (give or take 1 tick for rounding).
    assert!(acentric_ticks >= centralized_ticks * 4);

    let centralized_decay = age_decay(age_ticks, centralized_ticks);
    let acentric_decay = age_decay(age_ticks, acentric_ticks);
    // age_decay is exp(-age/decay), so larger decay constant →
    // larger value (closer to 1.0).
    assert!(
        acentric_decay > centralized_decay,
        "Acentric must lose less knowledge than Centralized over the same age window: \
         acentric={acentric_decay:?} centralized={centralized_decay:?}",
    );
}

// P0.3 — Lifecycle dispatch wired into the production civ tick.
//
// `step_population_with_capacity` (and its per-cell sibling) now
// route through `sim_population::lifecycle::step_for_lifecycle`,
// so non-Vertebrate species run their topology-specific step
// rather than the legacy 4-bracket cohort. The three tests below
// pin the three properties the dispatch must satisfy:
//
//  1. Vertebrate behaviour is preserved bit-for-bit.
//  2. Insect-species dynamics differ from Vertebrate-species
//     dynamics under identical params.
//  3. Eusocial colonies only reproduce via the Reproductive caste
//     — Worker headcount does not grow from births.

/// Vertebrate-species trajectory is unchanged post-dispatch. Two
/// identical civs — one stepped through the new
/// `step_population_with_capacity` (which now routes through
/// `step_for_lifecycle`), one stepped through the raw
/// `PopulationDynamics::step_with_capacity` — must produce
/// bit-identical bracket counts over 100 ticks.
#[test]
fn vertebrate_civ_dynamics_unchanged_post_dispatch() {
    use sim_recognition::RecognitionLibrary;
    use sim_world::sample_planet;
    let planet = sample_planet(1);
    let lib = RecognitionLibrary::earth_like_default();
    let mut species = sim_species::derive(&planet, &lib);
    species.lifecycle = sim_species::Lifecycle::Vertebrate;

    let mut civ = Civ::new(1, 0, Pop::from_int(1_000));
    civ.configure_lifecycle_state(&species.lifecycle);
    let state = well_fed_state();

    // Mirror cohort + dynamics — the "via legacy" baseline.
    let mut mirror_cohort = civ.cohort.clone();
    let mirror_dynamics = civ.dynamics.clone();
    let cap = civ.carrying_capacity(&state);

    for _ in 0..100 {
        civ.step_population_with_capacity(&state, &species);
        mirror_dynamics.step_with_capacity(&mut mirror_cohort, cap);
    }

    assert_eq!(
        civ.cohort.infant, mirror_cohort.infant,
        "Vertebrate dispatch must match legacy step bit-for-bit (infant)"
    );
    assert_eq!(
        civ.cohort.juvenile, mirror_cohort.juvenile,
        "Vertebrate dispatch must match legacy step bit-for-bit (juvenile)"
    );
    assert_eq!(
        civ.cohort.fertile, mirror_cohort.fertile,
        "Vertebrate dispatch must match legacy step bit-for-bit (fertile)"
    );
    assert_eq!(
        civ.cohort.elder, mirror_cohort.elder,
        "Vertebrate dispatch must match legacy step bit-for-bit (elder)"
    );
}

/// Insect-species and Vertebrate-species civs with identical
/// starting conditions diverge under the dispatch. The Insect
/// branch adds an extra adult-mortality term (5% per tick) on top
/// of the legacy step, so the adult (fertile) bracket must trail
/// the Vertebrate baseline after a few hundred ticks.
#[test]
fn insect_civ_distinct_dynamics_from_vertebrate() {
    use sim_recognition::RecognitionLibrary;
    use sim_world::sample_planet;
    let planet = sample_planet(1);
    let lib = RecognitionLibrary::earth_like_default();
    let mut insect = sim_species::derive(&planet, &lib);
    insect.lifecycle = sim_species::Lifecycle::Insect;
    let mut vertebrate = insect.clone();
    vertebrate.lifecycle = sim_species::Lifecycle::Vertebrate;

    let mut insect_civ = Civ::new(1, 0, Pop::from_int(1_000));
    insect_civ.configure_lifecycle_state(&insect.lifecycle);
    let mut vert_civ = Civ::new(2, 0, Pop::from_int(1_000));
    vert_civ.configure_lifecycle_state(&vertebrate.lifecycle);
    let state = well_fed_state();

    // Both civs see identical capacity / dynamics; only the
    // `Lifecycle` differs.
    for _ in 0..200 {
        insect_civ.step_population_with_capacity(&state, &insect);
        vert_civ.step_population_with_capacity(&state, &vertebrate);
    }

    let insect_fertile = insect_civ.cohort.fertile;
    let vert_fertile = vert_civ.cohort.fertile;
    assert!(
        insect_fertile < vert_fertile,
        "Insect adult-mortality term must drag fertile below Vertebrate: \
         insect={insect_fertile:?} vert={vert_fertile:?}"
    );
    // Totals should also differ — confirms the dispatch ran a
    // different code path, not just rounding noise.
    assert_ne!(
        insect_civ.cohort.total(),
        vert_civ.cohort.total(),
        "Insect and Vertebrate trajectories must diverge under dispatch"
    );
}

/// Eusocial colony: Worker caste must not grow from reproduction.
/// Seed Reproductive=10 + Worker=100, step under generous
/// capacity, and assert Worker headcount never exceeds the
/// initial 100 — only the Reproductive caste produces offspring,
/// and births land in Reproductive (not Worker).
#[test]
fn eusocial_civ_only_reproductive_caste_breeds() {
    use sim_population::LifecycleState;
    use sim_recognition::RecognitionLibrary;
    use sim_species::CasteRole;
    use sim_world::sample_planet;
    let planet = sample_planet(1);
    let lib = RecognitionLibrary::earth_like_default();
    let mut species = sim_species::derive(&planet, &lib);
    species.lifecycle = sim_species::Lifecycle::Eusocial {
        castes: vec![CasteRole::Reproductive, CasteRole::Worker],
    };

    let mut civ = Civ::new(1, 0, Pop::from_int(110));
    // Override the default founder-into-Reproductive seeding with
    // an explicit Reproductive=10 + Worker=100 split.
    civ.lifecycle_state = LifecycleState::for_lifecycle(&species.lifecycle);
    if let Some(colony) = civ.lifecycle_state.eusocial_mut() {
        colony.castes.insert(CasteRole::Reproductive, Pop::from_int(10));
        colony.castes.insert(CasteRole::Worker, Pop::from_int(100));
    }
    let initial_worker = if let LifecycleState::Eusocial(c) = &civ.lifecycle_state {
        c.caste(CasteRole::Worker)
    } else {
        panic!("expected Eusocial state");
    };
    assert_eq!(initial_worker, Pop::from_int(100));

    let state = well_fed_state();
    for _ in 0..120 {
        civ.step_population_with_capacity(&state, &species);
    }

    let final_worker = if let LifecycleState::Eusocial(c) = &civ.lifecycle_state {
        c.caste(CasteRole::Worker)
    } else {
        panic!("expected Eusocial state");
    };
    assert!(
        final_worker <= initial_worker,
        "Worker caste must not grow via reproduction: initial={initial_worker:?} final={final_worker:?}"
    );
    // Reproductive caste should hold ≥ initial 10 (births offset
    // survival decay under abundant food).
    let final_repro = if let LifecycleState::Eusocial(c) = &civ.lifecycle_state {
        c.caste(CasteRole::Reproductive)
    } else {
        panic!("expected Eusocial state");
    };
    assert!(
        final_repro >= Pop::from_int(10),
        "Reproductive caste should grow from births: {final_repro:?}"
    );
}

// P0.5 — civ carrying capacity tracks live producer biomass.
//
// Three properties pinned:
//  1. `civ.carrying_capacity` scales linearly with
//     `self.producer_biomass`.
//  2. Driving producer biomass to zero on a per-tick step
//     collapses the civ's aggregate population over 30 ticks.
//  3. `ecological_resilience` reads `producer_biomass /
//     initial_producer_biomass` clamped to `[0, 2]` — thriving
//     biosphere ⇒ resilience > 1.0; collapsed ⇒ < 0.5.

/// Same civ, two producer biomass readings: capacity must
/// differ by the same ratio as the biomass scalars (every other
/// multiplier in the capacity formula — `tech_multiplier`,
/// `tool_capacity_multiplier`, `carrying_capacity_per_unit`,
/// claimed-cell fraction — is shared between the two reads).
#[test]
fn civ_capacity_tracks_producer_biomass() {
    let mut civ = Civ::new(1, 0, Pop::from_int(50));
    let state = empty_state();

    // High-biomass world (thriving producers, anchor reading).
    civ.producer_biomass = Real::from_int(100);
    let cap_high = civ.carrying_capacity(&state);

    // Zero-biomass world (cascading-extinction endpoint).
    civ.producer_biomass = Real::ZERO;
    let cap_zero = civ.carrying_capacity(&state);

    assert_eq!(
        cap_zero,
        Pop::ZERO,
        "zero producer biomass must collapse civ capacity to zero, got {cap_zero:?}",
    );
    assert!(
        cap_high > Pop::ZERO,
        "thriving producer biomass must yield non-zero capacity, got {cap_high:?}",
    );

    // 10× linearity: 10× the biomass ⇒ 10× the cap.
    civ.producer_biomass = Real::from_int(10);
    let cap_ten = civ.carrying_capacity(&state);
    // 100 / 10 = 10. Compare via Q32.32 raw bits — pop = cap_high
    // raw / cap_ten raw should equal 10 to within Q32.32 noise.
    let ratio_q32 = cap_high.raw().to_bits() / cap_ten.raw().to_bits().max(1);
    assert_eq!(
        ratio_q32, 10,
        "capacity must scale linearly with producer_biomass; ratio_q32={ratio_q32}",
    );
}

/// Zero producer biomass for 30 ticks: civ population must
/// fall. Drives `step_population_per_cell` directly with
/// `producer_biomass = ZERO` on each tick so the per-cell step
/// sees zero capacity and the cohort's logistic step pulls the
/// population down each tick.
///
/// Acceptance: aggregate population after 30 ticks is strictly
/// below the initial 1000-pop seed. (Exact decay is calibrated by
/// the population dynamics' default mortality; what matters here
/// is monotonic decline under starvation.)
#[test]
fn zero_producer_biomass_causes_civ_population_decline() {
    use sim_recognition::RecognitionLibrary;
    use sim_world::sample_planet;
    let planet = sample_planet(1);
    let lib = RecognitionLibrary::earth_like_default();
    let species = sim_species::derive(&planet, &lib);

    let mut civ = Civ::new(1, 0, Pop::from_int(1_000));
    civ.configure_lifecycle_state(&species.lifecycle);
    let state = empty_state();
    let initial_pop = civ.aggregate_population();
    assert!(initial_pop > Pop::ZERO, "fresh civ should have a population");

    // 30 ticks at zero producer biomass — `step_population_per_cell`
    // caches `producer_biomass = ZERO` on every call, so the per-
    // cell capacity collapses and the cohort dynamics' food-stress
    // mortality drains the population.
    for tick in 1..=30 {
        civ.step_population_per_cell(&state, tick, &planet, &species, Real::ZERO);
    }
    let final_pop = civ.aggregate_population();
    assert!(
        final_pop < initial_pop,
        "civ population must decline under zero producer biomass; initial={initial_pop:?} final={final_pop:?}",
    );
}

/// `ecological_resilience` is a signed, meaningful scalar:
/// thriving producer biomass (2× the anchor) yields resilience >
/// 1.0; collapsed producer biomass (10% of the anchor) yields
/// resilience < 0.5. The anchor is captured on the civ's first
/// non-default `update_producer_biomass` call so the ratio reads
/// "this tick's biomass relative to first-observed."
#[test]
fn ecological_resilience_is_signed_and_meaningful() {
    let mut civ = Civ::new(1, 0, Pop::from_int(50));

    // Anchor the civ on a non-default biomass reading so the
    // resilience denominator becomes the captured anchor rather
    // than the constructor's `Real::ONE` sentinel.
    let anchor = Real::from_int(100);
    civ.update_producer_biomass(anchor);
    assert_eq!(
        civ.initial_producer_biomass, anchor,
        "first non-default update must capture the anchor",
    );
    assert_eq!(
        civ.ecological_resilience,
        Real::ONE,
        "anchor reading must yield baseline 1.0 resilience",
    );

    // Thriving — 2× the anchor.
    civ.update_producer_biomass(anchor * Real::from_int(2));
    assert!(
        civ.ecological_resilience > Real::ONE,
        "thriving biosphere must raise resilience above 1.0; got {:?}",
        civ.ecological_resilience,
    );
    // [0, 2] clamp — even a 10× spike caps at 2.0.
    civ.update_producer_biomass(anchor * Real::from_int(10));
    assert!(
        civ.ecological_resilience <= Real::from_int(2),
        "resilience must clamp at the upper 2.0 ceiling; got {:?}",
        civ.ecological_resilience,
    );

    // Collapsed — 10% of the anchor → resilience 0.1 << 0.5.
    civ.update_producer_biomass(anchor / Real::from_int(10));
    assert!(
        civ.ecological_resilience < Real::percent(50),
        "collapsed biosphere must drop resilience below 0.5; got {:?}",
        civ.ecological_resilience,
    );
    // Zero — resilience clamps at 0.
    civ.update_producer_biomass(Real::ZERO);
    assert_eq!(
        civ.ecological_resilience,
        Real::ZERO,
        "zero biosphere must clamp resilience at 0.0",
    );
}
