//! Airy isostasy: crust thickness drives surface elevation
//! (Sprint 4 Item 12c).
//!
//! In real-world Airy isostasy a crust column floats on the denser
//! mantle, and a thicker column floats *higher* — the buoyant lift
//! scales with the density contrast between mantle and crust:
//!
//! ```text
//! h_surface = h_base + (ρ_mantle / ρ_crust - 1) × thickness
//! ```
//!
//! Using `ρ_mantle = 3300`, `ρ_crust_continental = 2700`,
//! `ρ_crust_oceanic = 3000` the buoyancy factor lands at
//! `~0.22` for continental crust and `~0.10` for oceanic. Continental
//! crust both starts thicker (`~35 km` vs `~7 km`) *and* gets a
//! larger lift coefficient, which is why continents stand kilometres
//! above abyssal plains rather than just metres.
//!
//! The factor is selected per cell from the current `crust_thickness`
//! via a midpoint threshold (`~20 km`). The threshold sits halfway
//! between the documented continental + oceanic defaults so a cell
//! at either default gets the right factor; future cells that drift
//! across the threshold (subduction-driven thinning, Item 12a) flip
//! their lift coefficient accordingly. A per-cell `crust_type` field
//! would be more direct but would duplicate information already
//! present in the plate roster; the threshold avoids that
//! redundancy at the cost of needing recalibration if `MIN_PLATES`
//! defaults shift more than a factor of 2 away from earth-like.
//!
//! ## Algorithm
//!
//! Two per-cell tracking vectors live on `PhysicsState`:
//!
//! - `h_base[i]` — the geological baseline. Captures all
//!   non-isostatic elevation changes (tectonic uplift, fluvial
//!   erosion) since the last `apply_isostasy` pass.
//! - `last_thickness[i]` — `crust_thickness[i]` at the last pass.
//!
//! Both vectors start empty; the first `apply_isostasy` call after
//! `set_tectonics_fields` lazily bakes them so callers can set
//! elevation + thickness in either order without having to plug a
//! separate "rebake" call into every setup path.
//!
//! Each pass:
//!
//! 1. Lazy bake on first call: `h_base[i] = elevation[i] - factor[i]
//!    × crust_thickness[i]`, `last_thickness[i] = crust_thickness[i]`.
//!    Returns without touching elevation — the bake is a no-op by
//!    construction.
//! 2. Subsequent calls absorb whatever external elevation changes
//!    happened since the last pass into `h_base`:
//!    `h_base[i] = elevation[i] - factor[i] × last_thickness[i]`.
//!    This shifts `h_base` by exactly the external delta because
//!    the previous pass left `elevation[i] = h_base_old[i] + factor[i]
//!    × last_thickness[i]`.
//! 3. Re-derive elevation: `elevation[i] = h_base[i] + factor[i] ×
//!    crust_thickness[i]`. Equivalently `elevation[i] +=
//!    factor[i] × (crust_thickness[i] - last_thickness[i])`.
//! 4. Refresh `last_thickness[i] = crust_thickness[i]`.
//!
//! Effect: thickness changes lift / drop the surface by `factor ×
//! Δthickness`; elevation changes from other laws pass through
//! unchanged. The combined picture is the rock-cycle behaviour the
//! sprint is after: convergent boundaries thicken crust and lift
//! mountains, erosion removes mass and the surface rebounds less
//! than the naive direct loss would suggest because some of the
//! erosion shows up as thickness reduction rather than direct
//! surface drop.

use crate::state::PhysicsState;
use sim_arith::Real;

/// Crust-thickness threshold (km-equiv) separating oceanic from
/// continental for the per-cell isostatic factor. Halfway between
/// the documented defaults (`OCEANIC_THICKNESS_KM = 7`,
/// `CONTINENTAL_THICKNESS_KM = 35`) so a cell at either default
/// classifies correctly. Anything below the threshold uses the
/// oceanic factor; the rest use continental.
pub const CRUST_TYPE_THICKNESS_THRESHOLD_KM: i64 = 20;

/// Isostatic-lift factor for continental crust:
/// `ρ_mantle / ρ_crust_continental - 1 = 3300/2700 - 1 = 2/9`.
/// Pre-computed as a ratio so the per-cell loop doesn't repeat the
/// division — `Real::from_ratio` is the canonical constructor for
/// fractional constants across the sim.
#[must_use]
pub fn continental_factor() -> Real {
    // 3300 / 2700 - 1 = (3300 - 2700) / 2700 = 600 / 2700 = 2 / 9.
    Real::from_ratio(2, 9)
}

/// Isostatic-lift factor for oceanic crust:
/// `ρ_mantle / ρ_crust_oceanic - 1 = 3300/3000 - 1 = 1/10`.
#[must_use]
pub fn oceanic_factor() -> Real {
    // 3300 / 3000 - 1 = (3300 - 3000) / 3000 = 300 / 3000 = 1 / 10.
    Real::from_ratio(1, 10)
}

/// Per-cell isostatic lift coefficient based on `crust_thickness`.
/// Thin crust gets the oceanic factor (~0.10), thick crust gets
/// the continental factor (~0.22). Splitting on a fixed threshold
/// rather than carrying a `crust_type` per cell keeps the state
/// surface area small; the cost is that a cell that thins across
/// the threshold (subduction) flips factor instantaneously rather
/// than blending. That's qualitatively right — once an oceanic
/// column has subducted enough mass to behave like continental crust
/// the local lift coefficient *should* jump.
#[must_use]
fn factor_for(thickness: Real) -> Real {
    if thickness < Real::from_int(CRUST_TYPE_THICKNESS_THRESHOLD_KM) {
        oceanic_factor()
    } else {
        continental_factor()
    }
}

/// Apply Airy isostasy in place: lift / drop the surface elevation
/// so it tracks the current `crust_thickness` per cell. See module
/// docs for the algorithm.
///
/// No-ops cleanly on bare states (no plates sampled → no
/// `crust_thickness` field, length-zero vector). The first call
/// after `set_tectonics_fields` lazily bakes the per-cell baseline
/// so callers don't need a separate seeding step.
pub fn apply_isostasy(state: &mut PhysicsState) {
    let n = state.grid().n_cells();
    // No tectonics fields installed → nothing to do. Same gate the
    // `Tectonics::integrate` law uses; keeps the bare-state default
    // path a no-op.
    if state.crust_thickness().len() != n {
        return;
    }
    // Snapshot the inputs we'll need so the borrow checker doesn't
    // get cross with simultaneous &-borrows of `state.elevation()`
    // and `&mut` writes to `state.h_base_mut()`. Same single-snapshot
    // pattern the rest of the law family uses (heat diffusion,
    // hydrology, etc.).
    let thickness = state.crust_thickness().to_vec();
    let elevation_in = state.elevation().to_vec();

    // Lazy-bake branch: empty h_base means this is the first call
    // since `set_tectonics_fields`. Snapshot the current elevation
    // as the baseline minus the isostatic lift implied by current
    // thickness, refresh `last_thickness`, and return without
    // touching elevation. The "first call is a no-op" contract
    // matches what `Tectonics::integrate` needs so init paths that
    // call `tect.integrate(...)` once with the freshly-baked plates
    // see the same elevation field they started with.
    if state.h_base().len() != n {
        let mut h_base = Vec::with_capacity(n);
        let mut last = Vec::with_capacity(n);
        for i in 0..n {
            let f = factor_for(thickness[i]);
            // h_base[i] = elevation[i] - factor × thickness[i] so
            // `elevation[i] = h_base[i] + factor × thickness[i]`
            // holds at the moment of baking. The next pass walks
            // from this baseline.
            h_base.push(elevation_in[i] - f * thickness[i]);
            last.push(thickness[i]);
        }
        *state.h_base_mut() = h_base;
        *state.last_thickness_mut() = last;
        return;
    }
    // Steady-state branch: absorb external elevation changes into
    // h_base, then re-derive elevation from h_base + new isostatic
    // lift.
    let last_thickness = state.last_thickness().to_vec();
    // Defensive: if a future code path leaves `last_thickness`
    // mis-sized, rebuild it (rather than indexing OOB) so the law
    // remains best-effort safe.
    if last_thickness.len() != n {
        // Treat as a fresh bake. Same logic as the lazy-bake branch
        // above; factored inline to avoid a recursive call (and the
        // risk of double-running the snapshot).
        let mut h_base = Vec::with_capacity(n);
        let mut last = Vec::with_capacity(n);
        for i in 0..n {
            let f = factor_for(thickness[i]);
            h_base.push(elevation_in[i] - f * thickness[i]);
            last.push(thickness[i]);
        }
        *state.h_base_mut() = h_base;
        *state.last_thickness_mut() = last;
        return;
    }

    // Update h_base: the previous pass set
    //     elevation[i] = h_base_prev[i] + factor_prev[i] × last_thickness[i]
    // and any external mutation since then shifts elevation by the
    // external delta `Δext`. We want
    //     h_base_new[i] = h_base_prev[i] + Δext
    //                   = elevation[i] - factor_prev[i] × last_thickness[i]
    // so the invariant after this pass is
    //     elevation_new[i] = h_base_new[i] + factor_curr[i] × thickness[i].
    //
    // factor_prev uses `last_thickness` to pick the oceanic /
    // continental coefficient — i.e. the same factor that was applied
    // last pass. factor_curr uses the *new* `thickness`. They
    // disagree only across the threshold flip which is rare; in the
    // overwhelming majority of cells they're identical and the
    // delta-form `elevation += factor × (thickness - last_thickness)`
    // applies cleanly.
    let mut new_elevation = elevation_in.clone();
    let mut new_h_base = Vec::with_capacity(n);
    let mut new_last = Vec::with_capacity(n);
    for i in 0..n {
        let factor_prev = factor_for(last_thickness[i]);
        let factor_curr = factor_for(thickness[i]);
        // Back out the previous isostatic lift.
        let h_base_new = elevation_in[i] - factor_prev * last_thickness[i];
        // Re-derive elevation under the current lift.
        new_elevation[i] = h_base_new + factor_curr * thickness[i];
        new_h_base.push(h_base_new);
        new_last.push(thickness[i]);
    }

    state.elevation_mut().copy_from_slice(&new_elevation);
    *state.h_base_mut() = new_h_base;
    *state.last_thickness_mut() = new_last;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::HexGrid;
    use crate::tectonics::CONTINENTAL_THICKNESS_KM;

    /// Set up a small single-plate continental state where the
    /// initial bake leaves elevation untouched. Returns the cell
    /// index used for assertions.
    fn small_continental_state(elevation: Real, thickness_km: i64) -> (PhysicsState, usize) {
        let grid = HexGrid::new(4, 3);
        let n = grid.n_cells();
        let mut state = PhysicsState::new(grid);
        let plate_id = vec![0u32; n];
        let crust_thickness = vec![Real::from_int(thickness_km); n];
        state.set_tectonics_fields(plate_id, crust_thickness);
        // Seed elevation after `set_tectonics_fields` so the lazy
        // bake on the first `apply_isostasy` pass captures the
        // values we actually want as the baseline.
        for e in state.elevation_mut() {
            *e = elevation;
        }
        // Prime the lazy bake so subsequent calls are steady-state
        // (and elevation is unchanged at this point).
        apply_isostasy(&mut state);
        (state, 5)
    }

    /// Increasing crust thickness at a cell lifts its surface
    /// elevation by approximately `factor × Δthickness`. Pure
    /// thickening, no other mutation — the cleanest signal for the
    /// isostasy step's core behaviour.
    #[test]
    fn crustal_thickening_lifts_surface_elevation() {
        let initial_elevation = Real::from_int(1000);
        let (mut state, i) = small_continental_state(
            initial_elevation,
            CONTINENTAL_THICKNESS_KM,
        );
        let elevation_before = state.elevation()[i];

        // Thicken the crust by 10 km. Continental factor is 2/9 so
        // the surface should rise by ~2.22 km.
        let thickening = Real::from_int(10);
        state.crust_thickness_mut()[i] = state.crust_thickness()[i] + thickening;
        apply_isostasy(&mut state);
        let elevation_after = state.elevation()[i];

        // Direction: thicker crust → higher surface.
        assert!(
            elevation_after > elevation_before,
            "thicker crust should lift elevation: before={elevation_before:?} after={elevation_after:?}"
        );
        // Magnitude: rise equals continental factor × thickening
        // exactly (no rounding loss — Real is Q32.32 and the
        // multiplication is closed for these magnitudes).
        let expected_rise = continental_factor() * thickening;
        let actual_rise = elevation_after - elevation_before;
        assert_eq!(
            actual_rise, expected_rise,
            "rise should equal continental factor × thickening: \
             actual={actual_rise:?} expected={expected_rise:?}"
        );

        // Sanity: non-thickened cells stay put. Picks an arbitrary
        // other cell to confirm the per-cell-ness of the lift.
        let other = if i == 0 { 1 } else { 0 };
        assert_eq!(
            state.elevation()[other], initial_elevation,
            "non-thickened cells must not move",
        );
    }

    /// Erosion that pulls both elevation and crust thickness down
    /// should produce *less* elevation loss than a naive model
    /// where both decreases stack directly on the surface — the
    /// isostatic-rebound signature.
    #[test]
    fn erosion_triggers_isostatic_rebound() {
        let initial_elevation = Real::from_int(2000);
        let (mut state, i) = small_continental_state(
            initial_elevation,
            CONTINENTAL_THICKNESS_KM,
        );
        let elevation_before = state.elevation()[i];
        let thickness_before = state.crust_thickness()[i];

        // Mimic an erosion event: surface mass is removed and the
        // crust thins. The two amounts are picked so the naive
        // bound (drop_elev + drop_thick) is unambiguously larger
        // than the isostatic outcome — keeps the assertion robust
        // against Q32.32 rounding.
        let drop_elev = Real::from_int(50);
        let drop_thick = Real::from_int(5);
        state.elevation_mut()[i] = elevation_before - drop_elev;
        state.crust_thickness_mut()[i] = thickness_before - drop_thick;

        apply_isostasy(&mut state);

        let elevation_after = state.elevation()[i];
        let actual_drop = elevation_before - elevation_after;

        // Direction: elevation should have dropped (erosion removes
        // mass, lift drops).
        assert!(
            elevation_after < elevation_before,
            "erosion should still net a drop: before={elevation_before:?} after={elevation_after:?}"
        );

        // Rebound: the actual drop is less than the naive sum
        // `drop_elev + drop_thick`. The naive sum is what you'd get
        // if both mutations stacked directly on the surface with no
        // isostatic compensation; with isostasy, the thickness drop
        // contributes only `factor × drop_thick` rather than the
        // full `drop_thick`.
        let naive_drop = drop_elev + drop_thick;
        assert!(
            actual_drop < naive_drop,
            "isostatic rebound should produce less drop than the naive sum: \
             actual_drop={actual_drop:?} naive_drop={naive_drop:?}"
        );

        // Quantitative check: actual drop should equal
        // `drop_elev + factor × drop_thick` (the manual elevation
        // mutation passes through, the thickness mutation is scaled
        // by the lift factor).
        let expected_drop = drop_elev + continental_factor() * drop_thick;
        assert_eq!(
            actual_drop, expected_drop,
            "drop should equal drop_elev + factor × drop_thick: \
             actual={actual_drop:?} expected={expected_drop:?}"
        );
    }
}
