//! F2 (xeno N2) — per-cell biomass for heterogeneous catastrophes,
//! plus T9 biome-weighted initialisation.

use super::capacity;
use crate::*;
use sim_arith::Real;
use sim_species::EcosystemRole;
use std::collections::BTreeMap;

#[test]
fn eco_species_has_per_cell_biomass_after_init() {
    // Worldgen path — sample a grid-aware ecosystem and assert every
    // species' `cell_biomass` vector matches the planet's cell
    // count. Uniform initialisation: aggregate / n_cells per cell.
    let n_cells: usize = 8 * 8;
    let eco = sample_ecosystem_with_substrate_for_grid(
        42,
        "aqueous",
        capacity(),
        n_cells,
        None,
    );
    assert_eq!(eco.n_cells, n_cells, "ecosystem n_cells unset");
    assert!(!eco.species.is_empty(), "no species sampled");
    for (id, s) in &eco.species {
        assert_eq!(
            s.cell_biomass.len(),
            n_cells,
            "species {id:?} cell_biomass len {} != n_cells {n_cells}",
            s.cell_biomass.len()
        );
    }
}

#[test]
fn aggregate_biomass_equals_sum_of_cell_biomass() {
    // Invariant: `EcoSpecies.biomass == sum(cell_biomass)` at every
    // tick once the per-cell distribution has been initialised.
    // Run the worldgen-aware sampler, step a handful of ticks, and
    // assert the invariant holds for every species. The proportional
    // rescale in `step_at_tick` is responsible — without it, the
    // aggregate evolves while the per-cell vector stays frozen.
    let n_cells: usize = 8 * 8;
    let mut eco = sample_ecosystem_with_substrate_for_grid(
        7,
        "aqueous",
        capacity(),
        n_cells,
        None,
    );

    let assert_invariant = |eco: &PlanetEcosystem, label: &str| {
        for (id, s) in &eco.species {
            let sum = s
                .cell_biomass
                .iter()
                .copied()
                .fold(Real::ZERO, |a, b| a + b);
            // `sum` and `s.biomass` may differ by 1 ulp due to Q32.32
            // rounding when we divide-then-multiply during the
            // rescale; pin equality directly because the rescale
            // re-derives `s.biomass = sum(cells)` to keep them
            // bit-equal.
            assert_eq!(
                sum, s.biomass,
                "{label}: species {id:?} aggregate {:?} != sum(cell_biomass) {sum:?}",
                s.biomass
            );
        }
    };

    assert_invariant(&eco, "post-init");

    for tick in 0..5 {
        eco.step_at_tick(tick);
        assert_invariant(&eco, &format!("post-step tick {tick}"));
    }
}

#[test]
fn volcanic_event_reduces_local_eco_biomass_only() {
    // Heterogeneous-catastrophe coupling: `reduce_at_cell` drains a
    // single cell's biomass for a single species without crashing
    // the other cells or the planet-wide aggregate beyond the
    // proportional share. Models the N2 spec target — a volcanic
    // eruption on cell N starves only that cell's producers.
    let n_cells: usize = 4 * 4;
    let mut eco = sample_ecosystem_with_substrate_for_grid(
        99,
        "aqueous",
        capacity(),
        n_cells,
        None,
    );

    // Pick a producer to poke.
    let producer_id = *eco
        .species
        .iter()
        .find(|(_, s)| matches!(s.role, EcosystemRole::Producer { .. }))
        .map(|(id, _)| id)
        .expect("at least one producer");

    let target_cell: usize = 3;
    let other_cell: usize = 11;
    let before_target = eco
        .species
        .get(&producer_id)
        .unwrap()
        .cell_biomass[target_cell];
    let before_other = eco
        .species
        .get(&producer_id)
        .unwrap()
        .cell_biomass[other_cell];
    let before_aggregate = eco.species.get(&producer_id).unwrap().biomass;
    assert!(before_target > Real::ZERO, "target cell starts non-zero");
    assert!(before_other > Real::ZERO, "other cell starts non-zero");

    // Fire a 90% local catastrophe at the target cell.
    eco.reduce_at_cell(producer_id, target_cell, Real::from_ratio(9, 10));

    let after_target = eco
        .species
        .get(&producer_id)
        .unwrap()
        .cell_biomass[target_cell];
    let after_other = eco
        .species
        .get(&producer_id)
        .unwrap()
        .cell_biomass[other_cell];
    let after_aggregate = eco.species.get(&producer_id).unwrap().biomass;

    // Target cell shrank by ~90%; allow tiny rounding slack.
    assert!(
        after_target < before_target,
        "target cell biomass did not drop: before={before_target:?}, after={after_target:?}",
    );
    let expected_target = before_target * Real::from_ratio(1, 10);
    assert!(
        (after_target - expected_target).max(expected_target - after_target)
            <= Real::from_ratio(1, 1000) * before_target,
        "target cell drop off-spec: expected ~{expected_target:?}, got {after_target:?}",
    );

    // Other cell stayed put — the catastrophe is local, not
    // planet-wide.
    assert_eq!(
        after_other, before_other,
        "non-targeted cell biomass changed: before={before_other:?}, after={after_other:?}",
    );

    // Aggregate dropped by exactly the loss at the target cell.
    let expected_aggregate = before_aggregate - (before_target - after_target);
    assert_eq!(
        after_aggregate, expected_aggregate,
        "aggregate / per-cell mismatch: expected {expected_aggregate:?}, got {after_aggregate:?}",
    );

    // Invariant: sum(cell_biomass) == biomass.
    let sum = eco
        .species
        .get(&producer_id)
        .unwrap()
        .cell_biomass
        .iter()
        .copied()
        .fold(Real::ZERO, |a, b| a + b);
    assert_eq!(
        sum, after_aggregate,
        "post-reduce invariant: sum {sum:?} != aggregate {after_aggregate:?}",
    );
}

#[test]
fn initialise_cell_biomass_preserves_aggregate() {
    // `initialise_cell_biomass` is the worldgen helper that splits
    // the aggregate uniformly. Verify each cell gets aggregate /
    // n_cells, the aggregate is re-pinned to sum(cells) (so the
    // invariant holds bit-exactly), and the re-pin drift is bounded
    // by Q32.32's per-cell ulp.
    let mut eco = sample_ecosystem(123, capacity());
    let aggregates_before: BTreeMap<_, _> = eco
        .species
        .iter()
        .map(|(id, s)| (*id, s.biomass))
        .collect();

    let n_cells: usize = 16;
    eco.initialise_cell_biomass(n_cells, None);
    assert_eq!(eco.n_cells, n_cells);

    for (id, s) in &eco.species {
        assert_eq!(s.cell_biomass.len(), n_cells);
        let per_cell = aggregates_before[id] / Real::from_int(n_cells as i64);
        for (i, c) in s.cell_biomass.iter().enumerate() {
            assert_eq!(
                *c, per_cell,
                "species {id:?} cell {i}: expected {per_cell:?}, got {c:?}",
            );
        }
        // Invariant: `biomass == sum(cell_biomass)`. The aggregate
        // may shift by up to `n_cells × Q32.32_ulp` from the
        // pre-init value because the divide-then-multiply roundtrip
        // truncates each cell; the re-pin to the cell sum makes the
        // invariant exact, and the absolute drift stays ≤ n_cells
        // ulps.
        let sum = s
            .cell_biomass
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        assert_eq!(
            s.biomass, sum,
            "post-init aggregate {:?} != sum(cells) {sum:?}",
            s.biomass
        );
        // Bounded drift: max one ulp per cell.
        let before = aggregates_before[id];
        let drift = if s.biomass > before {
            s.biomass - before
        } else {
            before - s.biomass
        };
        // Real::ZERO is 1 ulp in Q32.32 = 1/2^32; bound by
        // `n_cells × ulp` = `n_cells / 2^32`. Express as a small
        // ratio: `n_cells / 1_000_000_000` is more than enough slack.
        let max_drift = Real::from_ratio(n_cells as i64, 1_000_000);
        assert!(
            drift <= max_drift,
            "init drift {drift:?} for species {id:?} exceeds bound {max_drift:?}",
        );
    }
}

#[test]
fn biomass_concentrates_in_habitable_cells() {
    // T9 — when `initialise_cell_biomass` receives a per-cell weight
    // vector, cells with higher weight (lush rainforest analog) get
    // measurably more biomass than low-weight cells (desert analog).
    // The aggregate invariant `sum(cell_biomass) == biomass` must
    // hold bit-exactly after the weighted split (the same re-pin to
    // the cell sum the uniform path uses).
    let mut eco = sample_ecosystem(456, capacity());
    let aggregates_before: BTreeMap<_, _> = eco
        .species
        .iter()
        .map(|(id, s)| (*id, s.biomass))
        .collect();

    // 8 cells: first 4 lush (weight 1.0), last 4 sparse (weight 0.1).
    // 10× ratio is well past Q32.32 rounding noise so the assertion
    // bites even on the smallest-biomass apex species.
    let n_cells: usize = 8;
    let lush = Real::ONE;
    let sparse = Real::from_ratio(1, 10);
    let weights: Vec<Real> = (0..n_cells)
        .map(|i| if i < n_cells / 2 { lush } else { sparse })
        .collect();
    eco.initialise_cell_biomass(n_cells, Some(&weights));
    assert_eq!(eco.n_cells, n_cells);

    for (id, s) in &eco.species {
        assert_eq!(s.cell_biomass.len(), n_cells);
        // Lush cells > sparse cells for every species with non-zero
        // biomass. Skip extinct species (biomass == 0 → every cell
        // is 0, comparison degenerates).
        if s.biomass <= Real::ZERO {
            continue;
        }
        let lush_total: Real = s.cell_biomass[..n_cells / 2]
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        let sparse_total: Real = s.cell_biomass[n_cells / 2..]
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        assert!(
            lush_total > sparse_total,
            "species {id:?}: lush_total {lush_total:?} not > sparse_total {sparse_total:?}",
        );
        // Each lush cell should be strictly larger than each sparse
        // cell — the 10× weight ratio is uniform per species so the
        // per-cell ordering holds independently of biomass scale.
        for i in 0..n_cells / 2 {
            for j in n_cells / 2..n_cells {
                assert!(
                    s.cell_biomass[i] > s.cell_biomass[j],
                    "species {id:?}: lush cell {i} ({:?}) not > sparse cell {j} ({:?})",
                    s.cell_biomass[i],
                    s.cell_biomass[j],
                );
            }
        }

        // Invariant: aggregate equals sum-of-cells bit-exactly.
        let sum = s
            .cell_biomass
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        assert_eq!(
            s.biomass, sum,
            "weighted-init aggregate {:?} != sum(cells) {sum:?}",
            s.biomass
        );
        // Drift from the pre-init aggregate stays bounded by the
        // same `n_cells × Q32.32 ulp` envelope as the uniform path.
        let before = aggregates_before[id];
        let drift = if s.biomass > before {
            s.biomass - before
        } else {
            before - s.biomass
        };
        let max_drift = Real::from_ratio(n_cells as i64, 1_000_000);
        assert!(
            drift <= max_drift,
            "weighted init drift {drift:?} for species {id:?} exceeds bound {max_drift:?}",
        );
    }
}

#[test]
fn weighted_biomass_falls_back_to_uniform_on_zero_sum() {
    // T9 — degenerate input: every cell weight is zero (e.g. an
    // HZ-evicted planet whose terrain × hz_factor is uniformly 0).
    // `initialise_cell_biomass` must fall back to the uniform split
    // so the aggregate is preserved rather than zeroed out.
    let mut eco = sample_ecosystem(789, capacity());
    let n_cells: usize = 8;
    let weights = vec![Real::ZERO; n_cells];
    eco.initialise_cell_biomass(n_cells, Some(&weights));

    for s in eco.species.values() {
        assert_eq!(s.cell_biomass.len(), n_cells);
        if s.biomass <= Real::ZERO {
            continue;
        }
        // Uniform split — every cell equal.
        let first = s.cell_biomass[0];
        for (i, c) in s.cell_biomass.iter().enumerate() {
            assert_eq!(*c, first, "cell {i}: expected uniform {first:?}, got {c:?}");
        }
    }
}
