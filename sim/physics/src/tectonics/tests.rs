//! Tectonics integration tests.
//!
//! Exercises the full `Law::integrate` orchestrator end-to-end:
//! convergent / divergent uplift, fluvial erosion, determinism,
//! subduction (oceanic-continental, basin consumption, continental
//! non-subduction), ridge-age + half-space cooling depth, worldgen
//! crust split, and slab-pull velocity dynamics + cap. Split out of
//! `mod.rs` (CB1) so the `Tectonics` data model, the phase
//! orchestrator, and these tests each live in their own file.

use super::*;
use crate::chemistry::Substance;
use crate::grid::{Axial, HexGrid};
use crate::laws::Law;
use crate::state::PhysicsState;
use sim_arith::Real;

/// Build a minimal two-plate state where the plates move toward
/// each other across a vertical boundary. After N ticks the
/// elevation at the boundary should rise from the initial value.
#[test]
fn convergent_boundary_uplifts_elevation() {
    let grid = HexGrid::new(6, 3);
    let n = grid.n_cells();
    let mut state = PhysicsState::new(grid.clone());

    // Two plates: left half (cols 0..3) is plate 0, right half
    // (cols 3..6) is plate 1. Velocities point toward each other
    // (plate 0 moves east, plate 1 moves west).
    let mut plate_id = vec![0u32; n];
    for (cid, axial) in grid.cells() {
        plate_id[cid.0 as usize] = if axial.q < 3 { 0 } else { 1 };
    }
    let plates = vec![
        Plate {
            id: 0,
            crust_type: CrustType::Continental,
            velocity: (Real::from_int(1), Real::ZERO),
            thickness: Real::from_int(CONTINENTAL_THICKNESS_KM),
        },
        Plate {
            id: 1,
            crust_type: CrustType::Continental,
            velocity: (Real::from_int(-1), Real::ZERO),
            thickness: Real::from_int(CONTINENTAL_THICKNESS_KM),
        },
    ];
    let crust_thickness = vec![Real::from_int(CONTINENTAL_THICKNESS_KM); n];
    state.set_tectonics_fields(plate_id.clone(), crust_thickness);

    // Pick a boundary cell on each side. Cell (2, 1) is on the
    // left plate's east edge; cell (3, 1) is on the right plate's
    // west edge. Their elevations should rise after the tectonic
    // step runs.
    let east_edge = state.grid().cell_id(Axial::new(2, 1)).0 as usize;
    let west_edge = state.grid().cell_id(Axial::new(3, 1)).0 as usize;
    let east_before = state.elevation()[east_edge];
    let west_before = state.elevation()[west_edge];

    let tect = Tectonics {
        plates,
        // Bigger rates than earth_like so the effect is visible
        // in a handful of ticks — the test asserts direction not
        // magnitude, but a small rate drowns the signal in
        // round-to-zero on Q32.32 fixed-point.
        convergence_rate: Real::percent(10),
        divergence_rate: Real::percent(10),
        erosion_k: Real::ZERO,
        ..Tectonics::earth_like()
    };
    for _ in 0..20 {
        tect.integrate(&mut state, Real::ONE);
    }
    let east_after = state.elevation()[east_edge];
    let west_after = state.elevation()[west_edge];
    assert!(
        east_after > east_before,
        "east boundary should rise under convergence: \
         before={east_before:?} after={east_after:?}"
    );
    assert!(
        west_after > west_before,
        "west boundary should rise under convergence: \
         before={west_before:?} after={west_after:?}"
    );
}

/// Two plates moving apart should lower the boundary elevation.
/// Seed the boundary with non-zero elevation so the divergence
/// step has somewhere to move from (with the zero-floor clamp).
#[test]
fn divergent_boundary_lowers_elevation() {
    let grid = HexGrid::new(6, 3);
    let n = grid.n_cells();
    let mut state = PhysicsState::new(grid.clone());

    let mut plate_id = vec![0u32; n];
    for (cid, axial) in grid.cells() {
        plate_id[cid.0 as usize] = if axial.q < 3 { 0 } else { 1 };
    }
    // Plates move apart: plate 0 west, plate 1 east.
    let plates = vec![
        Plate {
            id: 0,
            crust_type: CrustType::Oceanic,
            velocity: (Real::from_int(-1), Real::ZERO),
            thickness: Real::from_int(OCEANIC_THICKNESS_KM),
        },
        Plate {
            id: 1,
            crust_type: CrustType::Oceanic,
            velocity: (Real::from_int(1), Real::ZERO),
            thickness: Real::from_int(OCEANIC_THICKNESS_KM),
        },
    ];
    let crust_thickness = vec![Real::from_int(OCEANIC_THICKNESS_KM); n];
    state.set_tectonics_fields(plate_id, crust_thickness);

    // Seed elevation high enough that the zero-floor clamp doesn't
    // dominate.
    for e in state.elevation_mut() {
        *e = Real::from_int(1000);
    }

    let east_edge = state.grid().cell_id(Axial::new(2, 1)).0 as usize;
    let west_edge = state.grid().cell_id(Axial::new(3, 1)).0 as usize;
    let east_before = state.elevation()[east_edge];
    let west_before = state.elevation()[west_edge];

    let tect = Tectonics {
        plates,
        convergence_rate: Real::percent(10),
        divergence_rate: Real::percent(10),
        erosion_k: Real::ZERO,
        ..Tectonics::earth_like()
    };
    for _ in 0..20 {
        tect.integrate(&mut state, Real::ONE);
    }
    let east_after = state.elevation()[east_edge];
    let west_after = state.elevation()[west_edge];
    assert!(
        east_after < east_before,
        "east boundary should fall under divergence: \
         before={east_before:?} after={east_after:?}"
    );
    assert!(
        west_after < west_before,
        "west boundary should fall under divergence: \
         before={west_before:?} after={west_after:?}"
    );
}

/// Wet, steep cell loses elevation; flat dry cell doesn't.
#[test]
fn erosion_lowers_elevation_in_wet_steep_cells() {
    let grid = HexGrid::new(4, 3);
    let n = grid.n_cells();
    let mut state = PhysicsState::new(grid.clone());

    // Single plate so no tectonic step interferes; pure erosion.
    let plate_id = vec![0u32; n];
    let plates = vec![Plate {
        id: 0,
        crust_type: CrustType::Continental,
        velocity: (Real::ZERO, Real::ZERO),
        thickness: Real::from_int(CONTINENTAL_THICKNESS_KM),
    }];
    let crust_thickness = vec![Real::from_int(CONTINENTAL_THICKNESS_KM); n];
    state.set_tectonics_fields(plate_id, crust_thickness);

    // Steep, wet cell at (1, 1) — surrounded by lower-elevation
    // neighbours and seeded with lots of water + vapour.
    let steep_wet = state.grid().cell_id(Axial::new(1, 1)).0 as usize;
    state.elevation_mut()[steep_wet] = Real::from_int(2000);
    state.water_depth_mut()[steep_wet] = Real::from_int(500);
    state.substance_mut(Substance::Vapour.idx())[steep_wet] = Real::from_int(500);

    // Flat, dry cell at (3, 0) — same elevation as its
    // neighbours, no water. Erosion driven by slope × precip
    // gives zero on both factors.
    let flat_dry = state.grid().cell_id(Axial::new(3, 0)).0 as usize;
    // Leave elevation at zero (matches neighbours), water at
    // zero (set by default).

    let steep_before = state.elevation()[steep_wet];
    let flat_before = state.elevation()[flat_dry];

    let tect = Tectonics {
        plates,
        convergence_rate: Real::ZERO,
        divergence_rate: Real::ZERO,
        // Large erosion_k so the signal lands in a few ticks.
        erosion_k: Real::from_ratio(1, 1_000),
        ..Tectonics::earth_like()
    };
    for _ in 0..10 {
        tect.integrate(&mut state, Real::ONE);
    }
    let steep_after = state.elevation()[steep_wet];
    let flat_after = state.elevation()[flat_dry];

    assert!(
        steep_after < steep_before,
        "wet steep cell should lose elevation: \
         before={steep_before:?} after={steep_after:?}"
    );
    assert_eq!(
        flat_after, flat_before,
        "flat dry cell should not change: \
         before={flat_before:?} after={flat_after:?}"
    );
}

/// Same seed + grid → same plate layout + same per-tick
/// evolution. Exercises the SplitMix64-based sampler.
#[test]
fn tectonics_is_deterministic() {
    let grid = HexGrid::new(10, 8);
    let seed = 0xDEAD_BEEF_CAFE_BABE_u64;
    let (tect_a, plate_a, crust_a) = Tectonics::sample_plates_for_seed(seed, &grid);
    let (tect_b, plate_b, crust_b) = Tectonics::sample_plates_for_seed(seed, &grid);

    assert_eq!(plate_a, plate_b);
    assert_eq!(crust_a, crust_b);
    assert_eq!(tect_a.plates.len(), tect_b.plates.len());
    for (pa, pb) in tect_a.plates.iter().zip(tect_b.plates.iter()) {
        assert_eq!(pa.id, pb.id);
        assert_eq!(pa.crust_type, pb.crust_type);
        assert_eq!(pa.velocity, pb.velocity);
        assert_eq!(pa.thickness, pb.thickness);
    }

    // Now run the integrator on two independent states and
    // assert bit-equality of the elevation field afterwards.
    let mut state_a = PhysicsState::new(grid.clone());
    let mut state_b = PhysicsState::new(grid);
    state_a.set_tectonics_fields(plate_a, crust_a);
    state_b.set_tectonics_fields(plate_b, crust_b);
    for e in state_a.elevation_mut() {
        *e = Real::from_int(500);
    }
    for e in state_b.elevation_mut() {
        *e = Real::from_int(500);
    }
    for w in state_a.water_depth_mut() {
        *w = Real::from_int(100);
    }
    for w in state_b.water_depth_mut() {
        *w = Real::from_int(100);
    }
    for _ in 0..50 {
        tect_a.integrate(&mut state_a, Real::ONE);
        tect_b.integrate(&mut state_b, Real::ONE);
    }
    assert_eq!(state_a.elevation(), state_b.elevation());
}

/// Bonus: confirm the worldgen sampler stays within the documented
/// [MIN_PLATES, MAX_PLATES] window for an arbitrary earth-like seed.
#[test]
fn plate_count_within_range_for_earth_like_seed() {
    let grid = HexGrid::new(36, 30);
    for seed in [
        0x0000_0000_0000_0001_u64,
        0xDEAD_BEEF_CAFE_BABE,
        0x0123_4567_89AB_CDEF,
        0xFEED_FACE_BAAD_F00D,
    ] {
        let (tect, _, _) = Tectonics::sample_plates_for_seed(seed, &grid);
        let count = tect.plates.len() as u32;
        assert!(
            (MIN_PLATES..=MAX_PLATES).contains(&count),
            "plate count {count} outside [{MIN_PLATES}, {MAX_PLATES}] for seed {seed:#x}"
        );
    }
}

/// Convergent oceanic-continental boundary should consume the
/// oceanic crust over `SUBDUCTION_DT_TICKS`-ish ticks: the
/// oceanic-side boundary cells lose thickness, flip to the
/// continental plate's id once below the floor, and the
/// aggregate `subducted_mass` becomes positive (Sprint 4 Item
/// 12a).
#[test]
fn oceanic_continental_convergence_consumes_oceanic_crust() {
    let grid = HexGrid::new(6, 3);
    let n = grid.n_cells();
    let mut state = PhysicsState::new(grid.clone());

    // Plate 0 = continental on left (q < 3), plate 1 = oceanic
    // on right (q >= 3). Velocities point toward each other so
    // every cell pair across the q=3 boundary is convergent.
    let mut plate_id = vec![0u32; n];
    let mut crust_thickness = vec![Real::ZERO; n];
    for (cid, axial) in grid.cells() {
        let i = cid.0 as usize;
        if axial.q < 3 {
            plate_id[i] = 0;
            crust_thickness[i] = Real::from_int(CONTINENTAL_THICKNESS_KM);
        } else {
            plate_id[i] = 1;
            crust_thickness[i] = Real::from_int(OCEANIC_THICKNESS_KM);
        }
    }
    let plates = vec![
        Plate {
            id: 0,
            crust_type: CrustType::Continental,
            velocity: (Real::from_int(1), Real::ZERO),
            thickness: Real::from_int(CONTINENTAL_THICKNESS_KM),
        },
        Plate {
            id: 1,
            crust_type: CrustType::Oceanic,
            velocity: (Real::from_int(-1), Real::ZERO),
            thickness: Real::from_int(OCEANIC_THICKNESS_KM),
        },
    ];
    state.set_tectonics_fields(plate_id.clone(), crust_thickness);

    // Identify the oceanic-side boundary column (q == 3) — the
    // cells we expect to subduct over the run.
    let oceanic_boundary_cells: Vec<usize> = grid
        .cells()
        .filter(|(_, axial)| axial.q == 3)
        .map(|(cid, _)| cid.0 as usize)
        .collect();
    assert!(!oceanic_boundary_cells.is_empty());

    let tect = Tectonics {
        plates,
        convergence_rate: Real::ZERO,
        divergence_rate: Real::ZERO,
        erosion_k: Real::ZERO,
        ..Tectonics::earth_like()
    };
    // Run well past SUBDUCTION_DT_TICKS so even with the
    // per-tick decrement scaled by OCEANIC_THICKNESS_KM /
    // SUBDUCTION_DT_TICKS the boundary cells have time to drop
    // below the reassignment floor.
    for _ in 0..(SUBDUCTION_DT_TICKS + 50) {
        tect.integrate(&mut state, Real::ONE);
    }

    // Every oceanic-boundary cell should now belong to plate 0
    // (the overriding continental plate). Equivalently, the
    // oceanic plate has lost its frontline column.
    for &c in &oceanic_boundary_cells {
        assert_eq!(
            state.plate_id()[c],
            0,
            "oceanic boundary cell {c} should have flipped to \
             continental plate after subduction"
        );
    }
    assert!(
        state.subducted_mass() > Real::ZERO,
        "subducted_mass should be positive after a convergent \
         oceanic-continental run, got {:?}",
        state.subducted_mass()
    );
    // Cross-check via the Tectonics-side accessor that wires
    // through to PhysicsState — this is what Item 12d volcanism
    // will call.
    assert_eq!(
        Tectonics::subducted_mass(&state),
        state.subducted_mass()
    );
}

/// A small oceanic plate surrounded on every side by convergent
/// continental plates should be wholly consumed given enough
/// time. Test runs an explicitly geological number of ticks
/// (1000 ≫ SUBDUCTION_DT_TICKS) and asserts no cell still
/// belongs to the oceanic plate at the end (Sprint 4 Item 12a).
#[test]
fn ocean_basin_can_be_completely_consumed_over_geological_time() {
    let grid = HexGrid::new(5, 5);
    let n = grid.n_cells();
    let mut state = PhysicsState::new(grid.clone());

    // Centre cell is the oceanic basin. Surround with
    // continental cells. The continental plate pushes inward,
    // so every cell-pair across the basin perimeter is
    // convergent.
    const OCEANIC_PLATE_ID: u32 = 0;
    let centre = state.grid().cell_id(Axial::new(2, 2)).0 as usize;
    let mut plate_id = vec![1u32; n];
    let mut crust_thickness = vec![Real::from_int(CONTINENTAL_THICKNESS_KM); n];
    plate_id[centre] = OCEANIC_PLATE_ID;
    crust_thickness[centre] = Real::from_int(OCEANIC_THICKNESS_KM);

    // Plate roster: oceanic basin = plate 0 stationary; the
    // surrounding continental plate = plate 1 moving inward
    // (toward the centre). With a stationary oceanic core,
    // convergence at each of the centre's six neighbour pairs
    // depends purely on the sign of `v_continental · dir`. The
    // velocity (-1, -1) produces negative projection on the
    // east / northeast / southeast half of the hex rosette —
    // enough convergent neighbour pairs every tick to keep
    // wearing the centre cell down.
    let plates = vec![
        Plate {
            id: 0,
            crust_type: CrustType::Oceanic,
            velocity: (Real::ZERO, Real::ZERO),
            thickness: Real::from_int(OCEANIC_THICKNESS_KM),
        },
        Plate {
            id: 1,
            crust_type: CrustType::Continental,
            velocity: (Real::from_int(-1), Real::from_int(-1)),
            thickness: Real::from_int(CONTINENTAL_THICKNESS_KM),
        },
    ];
    state.set_tectonics_fields(plate_id, crust_thickness);

    let tect = Tectonics {
        plates,
        convergence_rate: Real::ZERO,
        divergence_rate: Real::ZERO,
        erosion_k: Real::ZERO,
        ..Tectonics::earth_like()
    };

    // Roll the simulation forward over geological time. 1000
    // ticks is ≫ SUBDUCTION_DT_TICKS so the centre cell — the
    // only oceanic-plate cell — has time to wear down past the
    // reassignment floor.
    for _ in 0..1000 {
        tect.integrate(&mut state, Real::ONE);
    }

    // No cell should still belong to the oceanic plate. The
    // basin has been fully consumed; everything is now part of
    // the surrounding continental plate.
    for i in 0..n {
        assert_ne!(
            state.plate_id()[i],
            OCEANIC_PLATE_ID,
            "cell {i} still belongs to oceanic plate after \
             1000 ticks; basin should have been fully consumed"
        );
    }
    // And the subducted-mass pool should reflect the consumed
    // crust.
    assert!(
        state.subducted_mass() > Real::ZERO,
        "subducted_mass should be positive after total basin \
         consumption, got {:?}",
        state.subducted_mass()
    );
}

/// Continental-continental convergent boundaries must NOT
/// subduct — Item 12a explicitly preserves the existing
/// Himalayan-uplift behaviour for those boundaries. Set up two
/// continental plates with convergent velocities and confirm
/// `subducted_mass` stays at zero (and plate ids don't migrate).
#[test]
fn continental_continental_convergence_does_not_subduct() {
    let grid = HexGrid::new(6, 3);
    let n = grid.n_cells();
    let mut state = PhysicsState::new(grid.clone());

    let mut plate_id = vec![0u32; n];
    for (cid, axial) in grid.cells() {
        plate_id[cid.0 as usize] = if axial.q < 3 { 0 } else { 1 };
    }
    let plates = vec![
        Plate {
            id: 0,
            crust_type: CrustType::Continental,
            velocity: (Real::from_int(1), Real::ZERO),
            thickness: Real::from_int(CONTINENTAL_THICKNESS_KM),
        },
        Plate {
            id: 1,
            crust_type: CrustType::Continental,
            velocity: (Real::from_int(-1), Real::ZERO),
            thickness: Real::from_int(CONTINENTAL_THICKNESS_KM),
        },
    ];
    let crust_thickness = vec![Real::from_int(CONTINENTAL_THICKNESS_KM); n];
    let plate_id_before = plate_id.clone();
    state.set_tectonics_fields(plate_id, crust_thickness);

    let tect = Tectonics {
        plates,
        convergence_rate: Real::percent(10),
        divergence_rate: Real::percent(10),
        erosion_k: Real::ZERO,
        ..Tectonics::earth_like()
    };
    for _ in 0..200 {
        tect.integrate(&mut state, Real::ONE);
    }

    // No mass should have been pumped into the subduction pool;
    // continental-continental collisions thicken rather than
    // consume crust.
    assert_eq!(
        state.subducted_mass(),
        Real::ZERO,
        "continental-continental convergence should not produce \
         subducted mass; got {:?}",
        state.subducted_mass()
    );
    // Plate ids must be stable across the run — no cell
    // reassignment for non-subducting boundaries.
    assert_eq!(state.plate_id(), plate_id_before.as_slice());
}

/// Sprint 4 Item 12b: ridge cells should stay at age 0 each tick
/// while interior cells (same plate, not touching a divergent
/// boundary) accumulate age normally. Two oceanic plates moving
/// apart create a ridge along the q=3 / q=2 boundary; cells
/// inside the plates (away from the boundary) age, cells at the
/// boundary stay at zero.
#[test]
fn ridge_crust_starts_age_zero() {
    let grid = HexGrid::new(8, 3);
    let n = grid.n_cells();
    let mut state = PhysicsState::new(grid.clone());

    // Two plates moving apart: plate 0 (q < 4) goes west,
    // plate 1 (q >= 4) goes east. The boundary sits between
    // q=3 (plate 0) and q=4 (plate 1).
    let mut plate_id = vec![0u32; n];
    for (cid, axial) in grid.cells() {
        plate_id[cid.0 as usize] = if axial.q < 4 { 0 } else { 1 };
    }
    let plates = vec![
        Plate {
            id: 0,
            crust_type: CrustType::Oceanic,
            velocity: (Real::from_int(-1), Real::ZERO),
            thickness: Real::from_int(OCEANIC_THICKNESS_KM),
        },
        Plate {
            id: 1,
            crust_type: CrustType::Oceanic,
            velocity: (Real::from_int(1), Real::ZERO),
            thickness: Real::from_int(OCEANIC_THICKNESS_KM),
        },
    ];
    let crust_thickness = vec![Real::from_int(OCEANIC_THICKNESS_KM); n];
    state.set_tectonics_fields(plate_id, crust_thickness);

    // Seed elevation high enough that the divergence + depth
    // clamps don't keep zeroing the field — the test only
    // reads ages here.
    for e in state.elevation_mut() {
        *e = Real::from_int(5000);
    }

    // Ridge cells: q=3 (plate 0's east edge) and q=4 (plate 1's
    // west edge), both at r=1 so they're true 6-neighbours.
    let ridge_left = state.grid().cell_id(Axial::new(3, 1)).0 as usize;
    let ridge_right = state.grid().cell_id(Axial::new(4, 1)).0 as usize;
    // Interior cell: q=0, r=1 (plate 0, far from the boundary so
    // none of its hex neighbours cross to plate 1).
    let interior = state.grid().cell_id(Axial::new(0, 1)).0 as usize;

    let tect = Tectonics {
        plates,
        convergence_rate: Real::percent(10),
        divergence_rate: Real::percent(10),
        erosion_k: Real::ZERO,
        ..Tectonics::earth_like()
    };

    let n_ticks = 5u64;
    for _ in 0..n_ticks {
        tect.integrate(&mut state, Real::ONE);
    }

    let ages = state.crust_age();
    assert_eq!(
        ages[ridge_left], 0,
        "ridge cell (q=3,r=1) should stay at age 0 across {n_ticks} ticks: got {}",
        ages[ridge_left]
    );
    assert_eq!(
        ages[ridge_right], 0,
        "ridge cell (q=4,r=1) should stay at age 0 across {n_ticks} ticks: got {}",
        ages[ridge_right]
    );
    assert_eq!(
        ages[interior], n_ticks,
        "interior cell (q=0,r=1) should accumulate {n_ticks} ticks of age: got {}",
        ages[interior]
    );
}

/// Sprint 4 Item 12b: an oceanic cell with a larger crust age
/// should sit at a lower elevation than an otherwise-identical
/// cell with a smaller crust age. Mirrors the real-world
/// observation that older sea floor is deeper than newer sea
/// floor near a mid-ocean ridge (half-space cooling).
#[test]
fn ocean_depth_increases_with_crustal_age() {
    let grid = HexGrid::new(6, 3);
    let n = grid.n_cells();
    let mut state = PhysicsState::new(grid.clone());

    // Single oceanic plate, zero velocity → no tectonic kicks
    // interfere; the only elevation change should come from the
    // ridge-cooling depth modulator driven by the manually-set
    // crust_age field. Single-plate also means `at_ridge` stays
    // all-false (no divergent pairs), so the manual ages don't
    // get reset.
    let plate_id = vec![0u32; n];
    let plates = vec![Plate {
        id: 0,
        crust_type: CrustType::Oceanic,
        velocity: (Real::ZERO, Real::ZERO),
        thickness: Real::from_int(OCEANIC_THICKNESS_KM),
    }];
    let crust_thickness = vec![Real::from_int(OCEANIC_THICKNESS_KM); n];
    state.set_tectonics_fields(plate_id, crust_thickness);

    // Seed both cells with the same high elevation so we can
    // observe the depth modulator deepen each independently.
    for e in state.elevation_mut() {
        *e = Real::from_int(10_000);
    }

    // Young (ridge-fresh) cell: age starts at 0.
    let young = state.grid().cell_id(Axial::new(1, 1)).0 as usize;
    // Old cell: pre-aged so the sqrt(age / SCALE) term is
    // substantially non-zero on the first integrate call.
    let old = state.grid().cell_id(Axial::new(4, 1)).0 as usize;
    // Pre-load the old cell with a large age (1e6 ticks). Even
    // with `AGE_TICK_SCALE = 10_000` this gives `sqrt(100) = 10`,
    // multiplied by the 350 prefactor and the 0.01 ocean-depth
    // scaler that's 35 km of depth per tick — well above the
    // signal floor for a fixed-point comparison.
    state.crust_age_mut()[old] = 1_000_000;
    state.crust_age_mut()[young] = 0;

    let tect = Tectonics {
        plates,
        convergence_rate: Real::ZERO,
        divergence_rate: Real::ZERO,
        erosion_k: Real::ZERO,
        ..Tectonics::earth_like()
    };
    tect.integrate(&mut state, Real::ONE);

    let young_elev = state.elevation()[young];
    let old_elev = state.elevation()[old];
    assert!(
        old_elev < young_elev,
        "older oceanic cell should be deeper (lower elevation) \
         than younger oceanic cell: old={old_elev:?} young={young_elev:?}"
    );
}

/// Worldgen sampler should produce a roughly 60/40 oceanic /
/// continental split. With 8-15 plates per seed the individual
/// counts vary, but aggregated across many seeds the ratio
/// should land near the documented 60 %.
#[test]
fn worldgen_crust_split_is_roughly_60_oceanic() {
    let grid = HexGrid::new(20, 16);
    let mut oceanic = 0u32;
    let mut continental = 0u32;
    for seed in 0u64..200 {
        let (tect, _, _) = Tectonics::sample_plates_for_seed(seed, &grid);
        for p in &tect.plates {
            match p.crust_type {
                CrustType::Oceanic => oceanic += 1,
                CrustType::Continental => continental += 1,
            }
        }
    }
    let total = oceanic + continental;
    let pct = (oceanic * 100) / total;
    // 60 % ± 10 % tolerance — 200 seeds is enough sample to
    // catch a calibration bug without false-positive rejecting
    // legitimate sampling variation.
    assert!(
        (50..=70).contains(&pct),
        "oceanic share {pct}% outside [50, 70] across 200 seeds: \
         oceanic={oceanic} continental={continental}"
    );
}

/// Sprint 4 Item 12e: an oceanic plate seated next to a
/// continental plate accelerates *toward* the continental plate
/// under slab-pull.
///
/// Grid is a torus, so any two-plate split has a *pair* of
/// boundaries (east + wraparound-west) whose slab-pull vectors
/// cancel by symmetry. To get a measurable net force we wire a
/// three-plate strip:
///
/// - plate 0 (oceanic): single column at q=0.
/// - plate 1 (continental): middle columns q=1..3.
/// - plate 2 (oceanic): right columns q=3..6.
///
/// Plate 0 sees plate 1 (continental → subducting boundary on
/// the east) and plate 2 (oceanic → no slab-pull on the west
/// wrap, since both sides are oceanic with no age field yet).
/// Net pull is positive q. Run 10 macro-ticks. Plate 0's
/// q-velocity should have evolved positive (east → toward the
/// continent); plate 1 stays at rest because the overriding
/// plate doesn't accumulate slab-pull. Plate 2 mirrors plate 0
/// in reverse.
#[test]
fn plate_velocities_evolve_via_slab_pull() {
    let grid = HexGrid::new(6, 3);
    let n = grid.n_cells();
    let mut state = PhysicsState::new(grid.clone());

    let mut plate_id = vec![0u32; n];
    for (cid, axial) in grid.cells() {
        plate_id[cid.0 as usize] = match axial.q {
            0 => 0,         // oceanic strip on the west
            1 | 2 => 1,     // continental middle
            _ => 2,         // oceanic east (q = 3..5)
        };
    }
    let plates = vec![
        Plate {
            id: 0,
            crust_type: CrustType::Oceanic,
            velocity: (Real::ZERO, Real::ZERO),
            thickness: Real::from_int(OCEANIC_THICKNESS_KM),
        },
        Plate {
            id: 1,
            crust_type: CrustType::Continental,
            velocity: (Real::ZERO, Real::ZERO),
            thickness: Real::from_int(CONTINENTAL_THICKNESS_KM),
        },
        Plate {
            id: 2,
            crust_type: CrustType::Oceanic,
            velocity: (Real::ZERO, Real::ZERO),
            thickness: Real::from_int(OCEANIC_THICKNESS_KM),
        },
    ];
    let crust_thickness = vec![Real::from_int(OCEANIC_THICKNESS_KM); n];
    state.set_tectonics_fields(plate_id, crust_thickness);

    let tect = Tectonics {
        plates,
        ..Tectonics::earth_like()
    };
    for _ in 0..10 {
        tect.integrate(&mut state, Real::ONE);
    }

    let vels = tect.current_velocity.borrow();
    let oceanic_west_v = vels[0];
    let continental_v = vels[1];
    let oceanic_east_v = vels[2];

    // Plate 0 (oceanic, west strip) accumulates positive-q pull
    // from its eastern boundary with the continental middle.
    // The western boundary with plate 2 is oceanic-oceanic and
    // contributes nothing.
    assert!(
        oceanic_west_v.0 > Real::ZERO,
        "oceanic plate 0 q-velocity should evolve positive \
         (toward continental neighbour to the east); \
         got {oceanic_west_v:?}"
    );
    // Plate 2 (oceanic, east block) accumulates *negative*-q pull
    // from its western boundary with the continental middle —
    // mirror image of plate 0.
    assert!(
        oceanic_east_v.0 < Real::ZERO,
        "oceanic plate 2 q-velocity should evolve negative \
         (toward continental neighbour to the west); \
         got {oceanic_east_v:?}"
    );
    // Plate 1 (continental, overriding) does not accumulate
    // slab-pull. It should remain at rest.
    assert_eq!(
        continental_v,
        (Real::ZERO, Real::ZERO),
        "continental overriding plate velocity should not evolve \
         under slab-pull; got {continental_v:?}"
    );

    // The slab-pull force buffer should also be populated this
    // tick — confirms the recompute path actually ran.
    let forces = tect.slab_pull_force.borrow();
    assert!(
        forces[0].0 > Real::ZERO,
        "plate 0 slab-pull force q should be positive (east); \
         got {force:?}",
        force = forces[0]
    );
    assert_eq!(
        forces[1],
        (Real::ZERO, Real::ZERO),
        "continental plate accumulates no slab-pull; got {force:?}",
        force = forces[1]
    );
    assert!(
        forces[2].0 < Real::ZERO,
        "plate 2 slab-pull force q should be negative (west); \
         got {force:?}",
        force = forces[2]
    );
}

/// Sprint 4 Item 12e: plates initialised with *parallel*
/// velocities (no convergence at the shared boundary) should
/// nevertheless see the oceanic side accelerate toward the
/// continental side as soon as slab-pull engages. This is the
/// "subduction zone initiation" path — the moment the geometry
/// becomes an oceanic-continental edge, regardless of the
/// kinematic relative velocity, the dynamics begin to converge.
///
/// Same three-plate strip as `plate_velocities_evolve_via_slab_pull`
/// (see that test for why two plates on a torus cancel by
/// symmetry). The novelty here is that *all three* plates start
/// with the same eastward parallel velocity — the existing
/// tectonic-uplift pass would emit no kick at the boundaries —
/// yet plate 0's (oceanic west) velocity along q must drift
/// above the shared parallel baseline, confirming slab-pull's
/// independence from initial relative motion.
#[test]
fn subduction_zone_initiation_changes_plate_velocity() {
    let grid = HexGrid::new(6, 3);
    let n = grid.n_cells();
    let mut state = PhysicsState::new(grid.clone());

    let mut plate_id = vec![0u32; n];
    for (cid, axial) in grid.cells() {
        plate_id[cid.0 as usize] = match axial.q {
            0 => 0,         // oceanic west strip
            1 | 2 => 1,     // continental middle block
            _ => 2,         // oceanic east block
        };
    }
    // All three plates start with the *same* eastward velocity —
    // no convergence at any boundary in the kinematic sense.
    let parallel_v = (Real::ONE, Real::ZERO);
    let plates = vec![
        Plate {
            id: 0,
            crust_type: CrustType::Oceanic,
            velocity: parallel_v,
            thickness: Real::from_int(OCEANIC_THICKNESS_KM),
        },
        Plate {
            id: 1,
            crust_type: CrustType::Continental,
            velocity: parallel_v,
            thickness: Real::from_int(CONTINENTAL_THICKNESS_KM),
        },
        Plate {
            id: 2,
            crust_type: CrustType::Oceanic,
            velocity: parallel_v,
            thickness: Real::from_int(OCEANIC_THICKNESS_KM),
        },
    ];
    let crust_thickness = vec![Real::from_int(OCEANIC_THICKNESS_KM); n];
    state.set_tectonics_fields(plate_id, crust_thickness);

    let tect = Tectonics {
        plates,
        ..Tectonics::earth_like()
    };

    // A few ticks of slab-pull should make plate 0 (oceanic west)
    // faster eastward than its initial parallel velocity, while
    // plate 1 (continental) holds steady.
    for _ in 0..5 {
        tect.integrate(&mut state, Real::ONE);
    }

    let vels = tect.current_velocity.borrow();
    let oceanic_west_v = vels[0];
    let continental_v = vels[1];
    let oceanic_east_v = vels[2];

    // Plate 0 accelerated *past* its initial parallel velocity
    // along q — confirms slab-pull engages on initiation even
    // when there's no kinematic convergence at the start.
    assert!(
        oceanic_west_v.0 > parallel_v.0,
        "oceanic plate 0 should accelerate beyond initial parallel \
         velocity along q under slab-pull; \
         initial={parallel_v:?} after={oceanic_west_v:?}"
    );
    // Plate 2 *decelerated* along q (slab-pull yanks it west,
    // into the trench under plate 1) — moves below the parallel
    // baseline.
    assert!(
        oceanic_east_v.0 < parallel_v.0,
        "oceanic plate 2 should decelerate below initial parallel \
         velocity along q under slab-pull; \
         initial={parallel_v:?} after={oceanic_east_v:?}"
    );
    // Continental plate held its parallel velocity (no slab-pull
    // on overriding plates in this implementation).
    assert_eq!(
        continental_v, parallel_v,
        "continental overriding plate should retain parallel \
         velocity; initial={parallel_v:?} after={continental_v:?}"
    );
}

/// Sprint 4 Item 12e: the per-axis velocity cap
/// (`MAX_PLATE_VELOCITY` = 5) prevents runaway acceleration. Run
/// the three-plate oceanic-continental-oceanic strip for many
/// ticks with an inflated `dt` to push past the cap quickly; the
/// resulting velocity must not exceed the documented bound on
/// either axis, and the oceanic plate's q must saturate exactly
/// at the cap (confirms the cap is what's binding, not just a
/// vanishing pull).
#[test]
fn slab_pull_velocity_cap_prevents_runaway() {
    let grid = HexGrid::new(6, 3);
    let n = grid.n_cells();
    let mut state = PhysicsState::new(grid.clone());

    let mut plate_id = vec![0u32; n];
    for (cid, axial) in grid.cells() {
        plate_id[cid.0 as usize] = match axial.q {
            0 => 0,
            1 | 2 => 1,
            _ => 2,
        };
    }
    let plates = vec![
        Plate {
            id: 0,
            crust_type: CrustType::Oceanic,
            velocity: (Real::ZERO, Real::ZERO),
            thickness: Real::from_int(OCEANIC_THICKNESS_KM),
        },
        Plate {
            id: 1,
            crust_type: CrustType::Continental,
            velocity: (Real::ZERO, Real::ZERO),
            thickness: Real::from_int(CONTINENTAL_THICKNESS_KM),
        },
        Plate {
            id: 2,
            crust_type: CrustType::Oceanic,
            velocity: (Real::ZERO, Real::ZERO),
            thickness: Real::from_int(OCEANIC_THICKNESS_KM),
        },
    ];
    let crust_thickness = vec![Real::from_int(OCEANIC_THICKNESS_KM); n];
    state.set_tectonics_fields(plate_id, crust_thickness);

    let tect = Tectonics {
        plates,
        ..Tectonics::earth_like()
    };

    // Hammer with an inflated dt to drive the velocity past the
    // cap if it were unbounded. The cap clamp must still hold.
    let big_dt = Real::from_int(100_000);
    for _ in 0..50 {
        tect.integrate(&mut state, big_dt);
    }

    let vels = tect.current_velocity.borrow();
    let cap = max_plate_velocity();
    for (idx, v) in vels.iter().enumerate() {
        assert!(
            v.0.abs() <= cap,
            "plate {idx} q-velocity {v:?} exceeds cap {cap:?}"
        );
        assert!(
            v.1.abs() <= cap,
            "plate {idx} r-velocity {v:?} exceeds cap {cap:?}"
        );
    }
    // Plate 0 (oceanic west) saturates *at* the positive cap on
    // q; plate 2 (oceanic east) saturates at the negative cap on
    // q. Confirms the cap is what's binding (not just a vanishing
    // pull).
    assert_eq!(
        vels[0].0, cap,
        "oceanic plate 0 q-velocity should saturate at +MAX_PLATE_VELOCITY"
    );
    assert_eq!(
        vels[2].0,
        Real::ZERO - cap,
        "oceanic plate 2 q-velocity should saturate at -MAX_PLATE_VELOCITY"
    );
}
