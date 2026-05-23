//! Fluvial erosion + crust-age / ridge-cooling depth modulator.
//!
//! Owns the slope × precipitation erosion pass (Sprint 4 Item 12) and
//! the half-space cooling depth update that ages ocean floor toward
//! a deeper bathymetry as it drifts away from the spreading ridge
//! (Sprint 4 Item 12b). Both passes feed into the same elevation
//! buffer the tectonic uplift / divergence step writes to; the
//! integrator wires them in order so the post-tectonic surface lands
//! consistent with the post-erosion + post-cooling state before
//! subduction and isostasy take their turns.

use super::plates::{CrustType, NEIGHBOUR_DIRECTIONS};
use super::Tectonics;
use sim_arith::transcendental::sqrt as sqrt_real;
use sim_arith::Real;

/// Sprint 4 Item 12b. Ridge-cooling depth coefficient — the
/// "350" prefactor in the empirical half-space cooling law
/// `depth ≈ 350 × sqrt(age_Ma)` (Parsons & Sclater 1977; Stein &
/// Stein 1992). Ocean floor near a mid-ocean ridge sits ~2.5 km
/// shallower than at 80 Ma age, and the relationship holds to
/// within a factor of two out to ~70-80 Ma where the half-space
/// model rolls over to a plate-cooling asymptote. Carried as a
/// `Real` so the depth math is fixed-point throughout.
pub const RIDGE_DEPTH_PREFACTOR: i64 = 350;

/// Sprint 4 Item 12b. Scale factor between simulation ticks and the
/// age units that feed the sqrt-cooling law. Earth's mid-ocean
/// ridge depths span 0-5 km across ~100 Myr; we treat `SCALE` ticks
/// as the equivalent of `1` age-unit in the prefactor formula so
/// the depth output lands in km-equivalent rather than spiking past
/// the elevation field's dynamic range on a per-tick cadence. The
/// value (10_000) is chosen so that ~10k macro-ticks of aging
/// produce a 350-unit depression, matching the Earth-like geologic
/// pacing of the surrounding tectonic step (per-month cadence × ~10k
/// months ≈ 800 yr → still well below realistic ridge timescales
/// but tuned to the run's accelerated clock).
pub const AGE_TICK_SCALE: i64 = 10_000;

/// Sprint 4 Item 12b. Final scaling between the raw "extra-depth"
/// metric (km-equivalent) and the per-cell elevation subtraction.
/// The prefactor-times-sqrt term yields km-equivalent depth; the
/// elevation field is in metre-equivalents, so without this scale
/// the depth would dwarf realistic seafloor topography. `1 / 100`
/// (0.01) damps the cumulative depth into the metre-scale band the
/// surrounding tectonic uplift / divergence kicks already operate
/// in, keeping the integrator's per-tick budget balanced.
pub const OCEAN_DEPTH_K_NUM: i64 = 1;
pub const OCEAN_DEPTH_K_DEN: i64 = 100;

/// Fluvial erosion pass. For every same-plate neighbour pair, transfer
/// elevation from the higher cell to the lower one at rate
/// `erosion_k × slope × precipitation × dt`, where `precipitation =
/// water + vapour` at the uphill cell (the humidity proxy
/// `Weathering` uses). Pair-flux symmetric: the transferred amount
/// is subtracted from one and added to the other, conserving `Σ
/// elevation` over the pair bit-exactly.
///
/// Cross-plate pairs are excluded so plate boundaries don't double-
/// count (the tectonic step already touched them). This also dodges
/// the awkward question of "which plate 'owns' the eroded mass at a
/// boundary" — punted to a future sub-item if it ever matters.
pub(super) fn run_fluvial(
    tect: &Tectonics,
    grid: &crate::grid::HexGrid,
    plate_ids: &[u32],
    elevation: &mut [Real],
    water: &[Real],
    vapour: &[Real],
    dt: Real,
) {
    for (cid, axial) in grid.cells() {
        let i = cid.0 as usize;
        let plate_i = plate_ids[i];
        for nb in grid.neighbours(axial).iter() {
            let j = nb.0 as usize;
            if j <= i {
                continue;
            }
            let plate_j = plate_ids[j];
            if plate_i != plate_j {
                continue;
            }
            let slope = elevation[i] - elevation[j];
            if slope == Real::ZERO {
                continue;
            }
            let (uphill, downhill, slope_mag) = if slope > Real::ZERO {
                (i, j, slope)
            } else {
                (j, i, -slope)
            };
            let precip = water[uphill] + vapour[uphill];
            if precip <= Real::ZERO {
                continue;
            }
            let raw = tect.erosion_k * slope_mag * precip * dt;
            // Don't erode more than half the slope in one pass;
            // otherwise an extreme rate would invert the slope.
            // The cap keeps the integrator stable even if a future
            // tuning bumps `erosion_k` aggressively.
            let cap = slope_mag / Real::from_int(2);
            let transfer = raw.min(cap);
            if transfer > Real::ZERO {
                elevation[uphill] = elevation[uphill] - transfer;
                elevation[downhill] = elevation[downhill] + transfer;
            }
        }
    }
}

/// Tectonic uplift / divergence at plate boundaries. Pair-iteration:
/// each unordered pair `(i, j)` is visited exactly once (we gate on
/// `j > i`). For each pair across different plates, compute the
/// relative velocity projected onto the unit hex-neighbour direction.
/// Convergent (projection negative — j moves toward i) → both cells
/// rise. Divergent → both cells sink. Symmetric in `i`, `j` so the
/// total `Σ elevation` change is `2 × per-cell-kick` per boundary
/// pair; not strictly mass-conserved (real tectonics sources / sinks
/// crust at boundaries) but the small kick magnitude makes the drift
/// bounded over geological time.
///
/// Writes to `at_ridge` to flag cells participating in a divergent
/// boundary this tick so the crust-age update below can reset them.
pub(super) fn run_uplift_divergence(
    tect: &Tectonics,
    grid: &crate::grid::HexGrid,
    plate_ids: &[u32],
    velocities: &[(Real, Real)],
    elevation: &mut [Real],
    at_ridge: &mut [bool],
    dt: Real,
) {
    for (cid, axial) in grid.cells() {
        let i = cid.0 as usize;
        let plate_i = plate_ids[i];
        for (k, nb) in grid.neighbours(axial).iter().enumerate() {
            let j = nb.0 as usize;
            if j <= i {
                continue;
            }
            let plate_j = plate_ids[j];
            if plate_i == plate_j {
                continue;
            }
            // Both plates must exist in the roster. Out-of-range ids
            // are skipped (defensive; would only fire if a future
            // sub-item shrinks the roster without remapping cells).
            if tect.plate_for(plate_i).is_none() || tect.plate_for(plate_j).is_none() {
                continue;
            }
            // Direction unit vector from i to j in axial coords.
            let (dir_q, dir_r) = NEIGHBOUR_DIRECTIONS[k];
            let dir_q_r = Real::from_int(dir_q);
            let dir_r_r = Real::from_int(dir_r);
            // Relative velocity: v_j - v_i. Project onto (dir_q,
            // dir_r). A *positive* projection means j is moving
            // away from i along the i→j direction → divergent.
            // A *negative* projection means j is moving toward i
            // along i→j → convergent.
            //
            // Reads velocity from `velocities` (Item 12e) — the
            // slab-pull-evolved values, not the immutable
            // `plates[i].velocity`. Closes the loop in a single
            // operator-split pass.
            let vel_i = velocities[plate_i as usize];
            let vel_j = velocities[plate_j as usize];
            let rel_q = vel_j.0 - vel_i.0;
            let rel_r = vel_j.1 - vel_i.1;
            let projection = rel_q * dir_q_r + rel_r * dir_r_r;
            let magnitude = projection.abs();
            if projection < Real::ZERO {
                // Convergent: uplift both sides.
                let kick = tect.convergence_rate * magnitude * dt;
                elevation[i] = elevation[i] + kick;
                elevation[j] = elevation[j] + kick;
            } else if projection > Real::ZERO {
                // Divergent: depression both sides. Don't let
                // elevation go negative — real ocean floor goes
                // *below* sea level (the sea floor IS negative
                // elevation in some coord systems), but per the
                // current `elevation` convention (metres above the
                // reference geoid, sea_level is the threshold)
                // sub-zero would confuse downstream slope reads.
                // Clamp at zero; future Item 12c isostasy lifts
                // this restriction once oceanic basins are
                // first-class.
                let kick = tect.divergence_rate * magnitude * dt;
                let new_i = elevation[i] - kick;
                let new_j = elevation[j] - kick;
                elevation[i] = new_i.max(Real::ZERO);
                elevation[j] = new_j.max(Real::ZERO);
                // Sprint 4 Item 12b. Mark both sides of this
                // divergent pair as "ridge cells" so the
                // crust-age update below resets them to zero
                // (fresh crust spawning at the spreading
                // centre). Interior cells of the same plate
                // age normally.
                at_ridge[i] = true;
                at_ridge[j] = true;
            }
            // Zero-projection (parallel boundary, no relative motion
            // along the boundary normal): transform-fault analogue;
            // no elevation change.
        }
    }
}

/// Crust age update + ocean-floor ridge-cooling depth (Sprint 4 Item
/// 12b). For each cell:
///   - If the cell was touched by a divergent boundary this tick,
///     reset its age to zero (fresh crust emplaced).
///   - Otherwise, increment the age by one. `saturating_add` so the
///     counter doesn't wrap on multi-million-tick runs.
///
/// Then, for oceanic cells, apply the half-space cooling depth
/// modulator:
///   extra_depth = 350 × sqrt(age / SCALE)
///   elevation -= extra_depth × OCEAN_DEPTH_K
///
/// The modulator runs only when the per-cell age field is sized;
/// otherwise we'd be indexing into an empty slice on states that
/// skipped `set_tectonics_fields`.
pub(super) fn run_age_and_cooling(
    tect: &Tectonics,
    state: &mut crate::state::PhysicsState,
    plate_ids: &[u32],
    elevation: &mut [Real],
    at_ridge: &[bool],
    dt: Real,
) {
    let n = state.grid().n_cells();
    if state.crust_age().len() != n {
        return;
    }
    // Snapshot the previous-tick ages so the writes below don't
    // tangle with the read-side depth calc. The age array is `u64` —
    // copy is cheap and keeps the mutation discipline matching the
    // elevation snapshot in the caller.
    let mut ages = state.crust_age().to_vec();
    for i in 0..n {
        if at_ridge[i] {
            ages[i] = 0;
        } else {
            ages[i] = ages[i].saturating_add(1);
        }
    }
    // Apply ridge-cooling depth to oceanic cells. Read crust_type
    // from the owning plate; non-oceanic cells are skipped.
    // Continental crust doesn't follow the half-space cooling law
    // (it's too thick and buoyant), and the spec only requires the
    // modulator on oceanic cells.
    let prefactor = Real::from_int(RIDGE_DEPTH_PREFACTOR);
    let scale = Real::from_int(AGE_TICK_SCALE);
    let ocean_k = Real::from_ratio(OCEAN_DEPTH_K_NUM, OCEAN_DEPTH_K_DEN);
    for i in 0..n {
        let plate_idx = plate_ids[i];
        let plate = match tect.plate_for(plate_idx) {
            Some(p) => p,
            None => continue,
        };
        if plate.crust_type != CrustType::Oceanic {
            continue;
        }
        if ages[i] == 0 {
            // sqrt(0) = 0 → no depth contribution at ridge.
            // Skipping is a perf detail; the math agrees.
            continue;
        }
        // age_in_ticks / SCALE — i64 ages capped at i64::MAX can
        // blow up `from_int`, but realistic runs stay far below
        // 2^63, so the cast is safe on simulation timescales. The
        // `min` guard keeps a runaway counter from panicking the
        // integrator.
        let age_capped = ages[i].min(i64::MAX as u64) as i64;
        let age_real = Real::from_int(age_capped);
        let ratio = age_real / scale;
        let s = sqrt_real(ratio);
        let extra_depth = prefactor * s;
        let drop = extra_depth * ocean_k * dt;
        // Subtract the depth, clamped at zero so the elevation field
        // stays non-negative — same convention as the divergence-step
        // clamp above.
        let new_elev = elevation[i] - drop;
        elevation[i] = new_elev.max(Real::ZERO);
    }
    state.crust_age_mut().copy_from_slice(&ages);
}
