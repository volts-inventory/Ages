//! Lunar gravitational tides on the surface-water column.
//!
//! Previously the planet had no orbital companion and no tidal
//! forcing — `water_depth` only changed via gravity flow,
//! hydrology (evaporation/precipitation), and the seeded
//! initial state. Real planets with moons see the surface ocean
//! lift into a sub-lunar bulge and an antipodal bulge (the
//! gradient term in the lunar gravitational potential), with
//! the bulges sweeping the planet as the moon orbits — twice-
//! daily tides on Earth, much slower on slow rotators.
//!
//! This module adds a `Tides` law that:
//!
//! 1. Tracks the current sub-lunar longitude as
//!    `sub_lunar_q = (macro_step · width / lunar_period_macros) % width`.
//!    Pure integer arithmetic — no transcendentals, no cell-time
//!    drift.
//! 2. Computes a tidal potential per cell:
//!    `Φ[i] = mass_relative · cos(2θ) · cos²(latitude_phase)` where
//!    `θ = 2π · signed_q_diff / width` is the longitudinal angle from
//!    the sub-lunar point. Two peaks at sub-lunar and antipodal, two
//!    troughs at the quarter-circle low-tide longitudes. The `cos(2θ)`
//!    longitudinal shape pairs with the `cos²(latitude)` modulation
//!    described below. Both use `sim_arith::transcendental::cos`
//!    (the codebase has no `sin`, so the latitude squared-cosine is
//!    obtained via `cos²` rather than `sin`).
//! 3. For each pair (i, j) with i<j: redistributes `water_depth`
//!    from low-Φ cells to high-Φ cells using the standard pair-
//!    flux pattern:
//!    ```text
//!      flux = tide_k · dt · (Φ[i] - Φ[j])
//!      water_depth[i] += flux
//!      water_depth[j] -= flux
//!    ```
//!    Pair-flux preserves total water bit-exactly — the moon
//!    only *moves* surface water around, it doesn't create or
//!    destroy any.
//!
//! Latitudinal (r) modulation: each moon carries a
//! `declination_r` (sub-lunar latitude, derived from orbital
//! inclination at planet-build time). The per-cell potential
//! is multiplied by `cos²((axial.r - declination_r) · π / height)`
//! so cells at the moon's sub-lunar latitude see the full bulge,
//! cells a quarter-circle away (the planet's terminator latitude
//! relative to that moon) see zero forcing, and antipodal-latitude
//! cells see the full bulge again. `cos²` (rather than `|cos|`)
//! keeps the falloff smooth and the values non-negative without
//! needing a `sin` half-cycle trick. Multi-moon planets with
//! different inclinations get genuine latitudinal interference —
//! a polar moon and an equatorial moon force different latitudes
//! independently.
//!
//! Determinism: the only state-derived input is `state.macro_step()`,
//! which the orchestrator advances exactly once per macro-step.
//! Pair iteration is canonical-order. No interior mutability, no
//! per-tick allocation beyond a single potential vector. No state-
//! dependent branching beyond `pair (i, j) with j > i`.
//!
//! Mass conservation: pair-flux structure (each `flux` applied as
//! `+flux` to one cell and `-flux` to the other). Verified by
//! `tides_conserve_total_water`.

use crate::laws::Law;
use crate::state::PhysicsState;
use sim_arith::transcendental::{cos, pi, sqrt, two_pi};
use sim_arith::Real;

/// Earth-baseline tide-flux coefficient. The `for_gravity` constructor
/// scales this by `sqrt(gravity_g)` — tide amplitude on an Earth-analog
/// scales as `g^0.5` (tidal force is the gravity gradient over the
/// planet's radius; for a fixed-radius planet the gradient scales
/// linearly with `g` but the response of the column-integrated water
/// is the square-root since the restoring force also scales with `g`).
const TIDE_K_EARTH: (i64, i64) = (1, 1_000);

/// Per-moon tidal configuration. Earlier the `Tides`
/// law tracked a single `lunar_period_macros`; this struct promotes
/// that to a list so multi-moon planets get genuine
/// interference patterns. Mass scales each moon's contribution
/// to the per-cell potential. `declination_r` sets the sub-lunar
/// *latitude* (in axial-r cells) so the bulge tilts off-equator
/// for real moons / a real solar tide. With declination, cells
/// further from the moon's sub-lunar latitude see a reduced
/// bulge magnitude — the latitude-cosine falloff that real
/// planetary tides exhibit and that lets polar oceans flow
/// differently from equatorial ones.
#[derive(Debug, Clone, Copy)]
pub struct MoonTide {
    /// Tidal contribution weight. Earth's moon ≈ 1.0; the
    /// sun ≈ 0.46. For our model, `mass_relative` is just a
    /// scalar multiplier on the cos(2θ) potential.
    pub mass_relative: Real,
    /// Macro-steps for one full sub-lunar cycle. Earth's moon
    /// = 28 macro-steps at the standard cadence.
    pub period_macros: u32,
    /// Sub-lunar latitude offset in axial-r cells (signed; 0 = on
    /// the equator, positive = south by the `magnetism.rs:107`
    /// convention). For solar tides this drifts with the seasonal
    /// cycle; for moons it's fixed by the moon's orbital
    /// inclination. Defaults to 0 (equatorial) for the legacy
    /// constructor path; `for_planet` populates it from the moon's
    /// inclination.
    pub declination_r: i32,
}

#[derive(Debug, Clone)]
pub struct Tides {
    /// Pair-flux coefficient. Multiplies `(Φ[i] - Φ[j]) · dt` to
    /// give per-pair water transfer.
    pub tide_k: Real,
    /// Per-moon orbital configs. Empty for moonless
    /// planets (the `integrate` path returns early). Each moon
    /// contributes a cos(2θ) bulge at its own period; the
    /// per-cell potential is the mass-weighted sum.
    pub moons: Vec<MoonTide>,
}

impl Tides {
    /// Earth-like defaults: one Earth-Moon-equivalent moon with
    /// 28-macro-step cycle. Back-compat alias for
    /// `Tides::for_gravity(Real::ONE)` with the Earth-Moon + solar
    /// pair pre-populated.
    #[must_use]
    pub fn earth_like() -> Self {
        Self {
            tide_k: tide_k_for_gravity(Real::ONE),
            moons: vec![
                MoonTide {
                    mass_relative: Real::ONE,
                    period_macros: 28,
                    declination_r: 0,
                },
                // Solar tide. Earth's sun contributes ~46% of the
                // lunar tidal force (`mass_relative = 0.46`) and
                // orbits with a 1-year period. At the default
                // tick cadence (~12 macros/year) that's
                // 12 macro-steps for a full sweep. Without this
                // entry, spring/neap interference can only arise
                // from moon-moon beating; the standard Earth
                // spring tide (moon + sun aligned) is solar-
                // driven.
                MoonTide {
                    mass_relative: Real::from_ratio(46, 100),
                    period_macros: 12,
                    declination_r: 0,
                },
            ],
        }
    }

    /// Build an Earth-Moon + solar tide pair tuned for a planet
    /// whose surface gravity is `gravity_g` Earth-gravities. The
    /// `tide_k` coefficient scales with `sqrt(gravity_g)` so a 2g
    /// planet sees measurably stronger tides and a 0.4g (Mars-like)
    /// planet sees weaker ones. Tidal amplitude on an Earth-analog
    /// scales as `g^0.5` — the linear-in-`g` gradient force is
    /// balanced by the linear-in-`g` restoring weight, leaving the
    /// column response in the square root.
    #[must_use]
    pub fn for_gravity(gravity_g: Real) -> Self {
        let mut tides = Self::earth_like();
        tides.tide_k = tide_k_for_gravity(gravity_g);
        tides
    }

    /// Build from a list of per-moon tide configs. Empty
    /// list means moonless (integrate becomes a no-op). The Earth-
    /// baseline `tide_k` is preserved for back-compat; callers that
    /// know their planet's gravity should prefer
    /// `Tides::for_planet_with_gravity`.
    #[must_use]
    pub fn for_planet(moons: Vec<MoonTide>) -> Self {
        Self {
            tide_k: tide_k_for_gravity(Real::ONE),
            moons,
        }
    }

    /// Build from a list of per-moon tide configs with a per-planet
    /// gravity scaling. `gravity_g` is in Earth-gravities (so 1.0 for
    /// Earth, 0.38 for Mars, 2.0 for a 2g super-Earth). `tide_k`
    /// scales with `sqrt(gravity_g)` — see `for_gravity` for the
    /// rationale. Empty `moons` still makes `integrate` a no-op.
    #[must_use]
    pub fn for_planet_with_gravity(moons: Vec<MoonTide>, gravity_g: Real) -> Self {
        Self {
            tide_k: tide_k_for_gravity(gravity_g),
            moons,
        }
    }

    /// Sub-lunar longitude in grid-q for the moon
    /// at `moon_idx` and the given macro-step. Public so tests
    /// can pin it without driving an integration.
    //
    // (the helper used by `earth_like` / `for_gravity` /
    // `for_planet*` lives as a free function below.)
    #[must_use]
    pub fn sub_lunar_q(&self, moon_idx: usize, macro_step: u64, width: u32) -> i32 {
        let Some(moon) = self.moons.get(moon_idx) else {
            return 0;
        };
        let period = u64::from(moon.period_macros).max(1);
        let phase = macro_step.saturating_mul(u64::from(width)) / period;
        i32::try_from(phase % u64::from(width.max(1))).unwrap_or(0)
    }
}

/// Scale the Earth-baseline `tide_k` by `sqrt(gravity_g)`.
/// `gravity_g` is in Earth-gravities (1.0 for Earth, 0.38 for Mars,
/// 2.0 for a 2g super-Earth). A non-positive input falls back to the
/// Earth baseline rather than producing zero or negative coefficients.
fn tide_k_for_gravity(gravity_g: Real) -> Real {
    let base = Real::from_ratio(TIDE_K_EARTH.0, TIDE_K_EARTH.1);
    if gravity_g <= Real::ZERO {
        return base;
    }
    base * sqrt(gravity_g)
}

impl Law for Tides {
    fn integrate(&self, state: &mut PhysicsState, dt: Real) {
        if self.moons.is_empty() {
            return;
        }
        let grid = state.grid().clone();
        let width = i32::try_from(grid.width()).unwrap_or(1).max(1);

        // Per-cell tidal potential is the *sum* of each
        // moon's cos(2θ) contribution, weighted by moon mass.
        // Multi-moon planets get genuine interference patterns
        // (the spring/neap-cycle analogue). Each moon's
        // sub_lunar_q advances at its own period; their
        // collective bulge sweep is the geographic / temporal
        // beat pattern of the moon system.
        let n = grid.n_cells();
        let mut potential = vec![Real::ZERO; n];
        let two_pi_v = two_pi();
        let pi_v = pi();
        let height = i32::try_from(grid.height()).unwrap_or(1).max(1);
        for (m_idx, moon) in self.moons.iter().enumerate() {
            if moon.period_macros == 0 {
                continue;
            }
            let sub_lunar_q = self.sub_lunar_q(m_idx, state.macro_step(), grid.width());
            for (cid, axial) in grid.cells() {
                let i = cid.0 as usize;
                let raw_diff = (axial.q - sub_lunar_q).rem_euclid(width);
                let signed_q_diff = if raw_diff <= width / 2 {
                    raw_diff
                } else {
                    raw_diff - width
                };
                let theta = two_pi_v * Real::from_ratio(i64::from(signed_q_diff), i64::from(width));
                // Latitude falloff: cos² of the (cell-r minus moon's
                // declination) phase, scaled to a half-period over the
                // grid height. Cells at the sub-lunar latitude get
                // full forcing; cells a quarter-grid-height away get
                // zero forcing; antipodal-latitude cells get full
                // forcing again. `cos²` (square of `cos`) keeps the
                // result non-negative without a `sin` half-cycle.
                let lat_phase = pi_v
                    * Real::from_ratio(i64::from(axial.r - moon.declination_r), i64::from(height));
                let lat_cos = cos(lat_phase);
                let lat_falloff = lat_cos * lat_cos;
                potential[i] =
                    potential[i] + moon.mass_relative * cos(theta + theta) * lat_falloff;
            }
        }

        // Step 2: pair-flux water redistribution with donor-limited
        // flux. Bit-exact mass conservation by construction: every
        // flux value applied as `+flux` to one cell and `-flux` to
        // its pair.
        //
        // Earlier this loop computed `flux = tide_k · dt · ΔΦ`
        // unconditionally, then a post-pass clamp drove any
        // negative `next_w[i]` back to zero. Under earth-like
        // coefficients the post-pass never fired, but with
        // pathological coefficients (large `tide_k` relative to
        // ambient depth, or one cell already near zero from prior
        // pair-flux outflow within the same tick), the clamp would
        // silently *gain* mass — every clamped cell appeared with
        // its negative residue erased.
        //
        // Donor-limited fix: before applying the pair, clamp `flux`
        // so it never pulls more than the donor's currently-available
        // stock. Direction matters: `flux > 0` means cell j loses
        // (cap to `next_w[j]`); `flux < 0` means cell i loses
        // (cap to `next_w[i]`). The non-negative invariant then
        // follows from the cap rather than the post-pass erasure,
        // and total water is exactly conserved.
        //
        // Two-pass iteration (Sprint 1 Item 2): a single pass is
        // conservative but order-dependent — a cell visited early as
        // a *recipient* may later be asked to *donate* to another
        // neighbour, and the cap measured at request time can be
        // looser than what the cell could have actually moved had
        // it seen its incoming flux first. Conversely, a cell that
        // was a saturated donor in pass 1 may have received inflow
        // from a different neighbour and now has stock to export.
        //
        // Running the same donor-limited loop a second time, with
        // pass 2 reading the post-pass-1 state as the new donor
        // pool, drains that residual transient asymmetry. Two
        // passes are sufficient: pass 1 propagates one "hop" of
        // donor-cap slack, pass 2 propagates the next; with the
        // earth-like `tide_k = 0.001 · dt · ΔΦ` request size,
        // ΔΦ is bounded so a third pass moves only second-order
        // residue that the next macro-step's pass 1 will absorb
        // anyway. We don't iterate to fixed point because each
        // pass adds determinism-relevant work proportional to the
        // grid pair count, and the per-pair cap is order-dependent
        // — converging would require a global solve we explicitly
        // don't want at this layer.
        let prev_w = state.water_depth().to_vec();
        let mut next_w = prev_w.clone();
        donor_limited_pass(&grid, &potential, self.tide_k, dt, &mut next_w);
        donor_limited_pass(&grid, &potential, self.tide_k, dt, &mut next_w);
        // Bit-exact conservation: the pair-flux primitive applies
        // every `flux` as `+flux` to cell i and `-flux` to cell j,
        // so the integer-arithmetic sum is invariant across each
        // pass. This assert catches a future regression where
        // someone, e.g., adds a one-sided clamp or a stray cell-
        // local read before the assert can complain at run time.
        debug_assert_eq!(
            prev_w.iter().copied().fold(Real::ZERO, |a, b| a + b),
            next_w.iter().copied().fold(Real::ZERO, |a, b| a + b),
            "two-pass donor-limited tide flux must preserve total water bit-exactly"
        );
        state.water_depth_mut().copy_from_slice(&next_w);
    }
}

/// One pass of the donor-limited pair-flux redistribution. Reads
/// `potential` and per-cell stock from `next_w`, writes the
/// post-pass stock back into `next_w` in place.
///
/// Canonical pair order (j > i over `grid.cells()` × `grid.neighbours()`)
/// is preserved so determinism does not depend on pass count.
/// Used twice from `Tides::integrate` (see Sprint 1 Item 2 comment
/// for the two-pass rationale) and once from the test-only
/// `single_pass_for_comparison` helper used by
/// `two_pass_drains_more_than_single_pass`.
fn donor_limited_pass(
    grid: &crate::grid::HexGrid,
    potential: &[Real],
    tide_k: Real,
    dt: Real,
    next_w: &mut [Real],
) {
    for (cid, axial) in grid.cells() {
        let i = cid.0 as usize;
        for nb in grid.neighbours(axial) {
            let j = nb.0 as usize;
            if j > i {
                let dphi = potential[i] - potential[j];
                let mut flux = tide_k * dt * dphi;
                if flux > Real::ZERO {
                    // i gains, j loses — cap to j's stock.
                    let donor = next_w[j].max(Real::ZERO);
                    if flux > donor {
                        flux = donor;
                    }
                } else if flux < Real::ZERO {
                    // j gains, i loses — cap to i's stock.
                    let donor = next_w[i].max(Real::ZERO);
                    let neg_donor = Real::ZERO - donor;
                    if flux < neg_donor {
                        flux = neg_donor;
                    }
                }
                next_w[i] = next_w[i] + flux;
                next_w[j] = next_w[j] - flux;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::HexGrid;

    #[test]
    fn sub_lunar_advances_each_cycle() {
        let tides = Tides::earth_like();
        // 28-cycle, width 28: one cell per macro-step.
        assert_eq!(tides.sub_lunar_q(0, 0, 28), 0);
        assert_eq!(tides.sub_lunar_q(0, 1, 28), 1);
        assert_eq!(tides.sub_lunar_q(0, 14, 28), 14);
        assert_eq!(tides.sub_lunar_q(0, 28, 28), 0);
    }

    #[test]
    fn tides_conserve_total_water() {
        let grid = HexGrid::new(8, 4);
        let mut state = PhysicsState::new(grid);
        for w in state.water_depth_mut() {
            *w = Real::from_int(100);
        }
        let initial: Real = state
            .water_depth()
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        let tides = Tides::earth_like();
        for _ in 0..50 {
            tides.integrate(&mut state, Real::ONE);
            state.advance_macro_step();
        }
        let after: Real = state
            .water_depth()
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        assert_eq!(
            initial, after,
            "pair-flux tidal redistribution must conserve total water bit-exactly"
        );
    }

    #[test]
    fn tide_redistribution_donor_limited() {
        // Pathological-coefficient seed: tide_k=0.5 and varying
        // depths from 0 to 100. Previously the post-pass non-
        // negative clamp would have fired and silently *gained*
        // mass when a low-depth cell got pulled below zero.
        // With donor-limited flux, total water is bit-exact across
        // all macro-steps.
        let grid = HexGrid::new(8, 4);
        let mut state = PhysicsState::new(grid);
        for (i, w) in state.water_depth_mut().iter_mut().enumerate() {
            // Varying depths 0..100 — cells at index 0, 8, 16, 24
            // start at zero so the first outflow would underflow.
            *w = Real::from_int((i as i64) * 100 / 32);
        }
        let initial: Real = state
            .water_depth()
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        let tides = Tides {
            tide_k: Real::from_ratio(1, 2), // 0.5 — well above earth-like 0.001
            moons: vec![MoonTide {
                mass_relative: Real::ONE,
                period_macros: 28,
                declination_r: 0,
            }],
        };
        for _ in 0..50 {
            tides.integrate(&mut state, Real::ONE);
            state.advance_macro_step();
        }
        let after: Real = state
            .water_depth()
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        assert_eq!(
            initial, after,
            "donor-limited flux must conserve total water bit-exactly even under pathological tide_k"
        );
        // All cells must be non-negative — the donor cap enforces
        // this without a post-pass clamp.
        for w in state.water_depth() {
            assert!(
                *w >= Real::ZERO,
                "water depth must stay non-negative under donor-limited flux"
            );
        }
    }

    #[test]
    fn tides_conserve_water_under_pathological_coefficients() {
        // PR6 conservation invariant: even with `tide_k` an order
        // of magnitude above earth-like and per-cell depths
        // varying down to zero, the pair-flux + donor-limited
        // structure preserves total water bit-exactly. This is the
        // orchestrator-level `debug_assert!` made explicit at the
        // law level so a future regression in either the pair-flux
        // loop or the donor cap (PR1) trips here before it pollutes
        // an integrated run.
        let grid = HexGrid::new(8, 4);
        let mut state = PhysicsState::new(grid);
        for (i, w) in state.water_depth_mut().iter_mut().enumerate() {
            // Mix of full and empty cells so donor caps actually
            // engage during the pair pass.
            *w = if i % 3 == 0 {
                Real::ZERO
            } else {
                Real::from_int(((i as i64) * 50) % 200)
            };
        }
        let initial: Real = state
            .water_depth()
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        let tides = Tides {
            // 0.8 — 800× earth-like; well into the pathological
            // regime where a naive post-pass clamp would have
            // silently created mass.
            tide_k: Real::from_ratio(8, 10),
            moons: vec![MoonTide {
                mass_relative: Real::ONE,
                period_macros: 28,
                declination_r: 0,
            }],
        };
        // Assert conservation at every step, not just before/after,
        // so the failure surfaces at the exact tick where a future
        // regression starts to drift.
        for tick in 0..100 {
            let pre: Real = state
                .water_depth()
                .iter()
                .copied()
                .fold(Real::ZERO, |a, b| a + b);
            tides.integrate(&mut state, Real::ONE);
            let post: Real = state
                .water_depth()
                .iter()
                .copied()
                .fold(Real::ZERO, |a, b| a + b);
            assert_eq!(
                pre, post,
                "tide_k=0.8 leaked water at tick {tick}: \
                 pre={pre:?} post={post:?}"
            );
            state.advance_macro_step();
        }
        let after: Real = state
            .water_depth()
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        assert_eq!(initial, after);
        // Non-negativity invariant — the donor cap guarantees this.
        for w in state.water_depth() {
            assert!(
                *w >= Real::ZERO,
                "water depth must stay non-negative under pathological tide_k"
            );
        }
    }

    #[test]
    fn tide_declination_modulates_potential() {
        // Two cells at equal q-offset from sub-lunar (so identical
        // longitudinal forcing) but at different r-offsets from
        // the moon's declination latitude — the lat falloff must
        // give them different water-depth response. Earlier
        // `declination_r` was stored but never read, so this test
        // would have shown zero difference.
        //
        // Cell at r=0 sits on declination_r=0 (full forcing);
        // cell at r=height/2 sits at the quarter-grid latitude
        // (zero forcing under cos²). After many ticks, the equator
        // cell should have moved further from its starting depth
        // than the high-latitude cell.
        let grid = HexGrid::new(8, 4);
        let mut state = PhysicsState::new(grid);
        for w in state.water_depth_mut() {
            *w = Real::from_int(100);
        }
        let tides = Tides {
            tide_k: Real::percent(1),
            moons: vec![MoonTide {
                mass_relative: Real::ONE,
                period_macros: u32::MAX, // freeze sub-lunar at q=0
                declination_r: 0,
            }],
        };
        for _ in 0..30 {
            tides.integrate(&mut state, Real::ONE);
            state.advance_macro_step();
        }
        // grid is 8x4. cell_id = r * width + q. Equator (r=0, q=0)
        // is index 0; high-lat (r=2, q=0) — the cos² zero — is
        // index 16.
        let equator = state.water_depth()[0];
        let high_lat = state.water_depth()[16];
        let equator_drift = (equator - Real::from_int(100)).abs();
        let high_lat_drift = (high_lat - Real::from_int(100)).abs();
        assert!(
            equator_drift > high_lat_drift,
            "equator cell (on sub-lunar latitude) should drift more than \
             high-latitude cell (at cos² zero): \
             equator={equator:?} high_lat={high_lat:?}"
        );
    }

    #[test]
    fn tide_bulge_preserves_longitudinal_symmetry() {
        // Anti-aliasing test for the donor-limited tide flux.
        // Under pair-flux with order-dependent donor caps, the
        // resulting water distribution can lose the spatial
        // symmetry the potential implies (cos(2θ) is symmetric
        // around sub-lunar and antipodal, with two equal troughs
        // 90° away). We assert that under moderate tide_k the
        // post-tide depth shows the expected two-peak / two-trough
        // pattern: at the sub-lunar longitude, the antipodal
        // longitude, and the two trough longitudes 90° apart,
        // the cells should pair up with equal depths within a
        // tolerance.
        let grid = HexGrid::new(16, 4);
        let mut state = PhysicsState::new(grid);
        for w in state.water_depth_mut() {
            *w = Real::from_int(100);
        }
        let tides = Tides {
            tide_k: Real::percent(2),
            moons: vec![MoonTide {
                mass_relative: Real::ONE,
                period_macros: u32::MAX,
                declination_r: 0,
            }],
        };
        for _ in 0..50 {
            tides.integrate(&mut state, Real::ONE);
            state.advance_macro_step();
        }
        // grid is 16x4. Equator row (r=0). Sub-lunar q=0, antipodal
        // q=8, quarter q=4, quarter q=12.
        let sub_lunar = state.water_depth()[0];
        let antipodal = state.water_depth()[8];
        let quarter_a = state.water_depth()[4];
        let quarter_b = state.water_depth()[12];
        // Sub-lunar and antipodal are both bulge peaks — should be
        // close (cos(2 · 0) = cos(2 · π) = 1).
        let peak_asymmetry = (sub_lunar - antipodal).abs();
        // Two quarter-circle troughs (cos(2 · π/2) = cos(2 · 3π/2)
        // = -1) — should also be close to each other.
        let trough_asymmetry = (quarter_a - quarter_b).abs();
        // 2% tolerance — pair-flux conservation is bit-exact; this
        // catches gross order-dependent flux skew from the donor
        // limiter without overconstraining the discrete sampling.
        let tol = Real::from_int(2);
        assert!(
            peak_asymmetry < tol,
            "peak asymmetry too large: sub_lunar={sub_lunar:?} antipodal={antipodal:?}"
        );
        assert!(
            trough_asymmetry < tol,
            "trough asymmetry too large: quarter_a={quarter_a:?} quarter_b={quarter_b:?}"
        );
    }

    #[test]
    fn tides_lift_sub_lunar_cell_above_quarter_cell() {
        // With sub_lunar at q=0, the cells at q=0 should have
        // higher water depth than cells at q=width/4 after several
        // ticks of tidal redistribution.
        let grid = HexGrid::new(8, 1);
        let mut state = PhysicsState::new(grid);
        for w in state.water_depth_mut() {
            *w = Real::from_int(100);
        }
        let tides = Tides {
            tide_k: Real::percent(1),
            // freeze sub_lunar at 0 with a huge period
            moons: vec![MoonTide {
                mass_relative: Real::ONE,
                period_macros: u32::MAX,
                declination_r: 0,
            }],
        };
        for _ in 0..30 {
            tides.integrate(&mut state, Real::ONE);
            state.advance_macro_step();
        }
        let high = state.water_depth()[0];
        let low = state.water_depth()[2]; // q=2 is the quarter-circle for width=8
        assert!(
            high > low,
            "sub-lunar cell should be higher than quarter-circle cell: high={high:?} low={low:?}"
        );
    }

    #[test]
    fn tides_are_deterministic() {
        let grid = HexGrid::new(6, 4);
        let mut a = PhysicsState::new(grid.clone());
        let mut b = PhysicsState::new(grid);
        for w in a.water_depth_mut() {
            *w = Real::from_int(50);
        }
        for w in b.water_depth_mut() {
            *w = Real::from_int(50);
        }
        let tides = Tides::earth_like();
        for _ in 0..40 {
            tides.integrate(&mut a, Real::ONE);
            a.advance_macro_step();
            tides.integrate(&mut b, Real::ONE);
            b.advance_macro_step();
        }
        assert_eq!(a.water_depth(), b.water_depth());
        assert_eq!(a.macro_step(), b.macro_step());
    }

    /// Test-only: run exactly one donor-limited pass against a
    /// fresh `next_w` snapshot. Mirrors the single-pass behaviour
    /// the production code had before Sprint 1 Item 2, so the
    /// `two_pass_drains_more_than_single_pass` test can compare
    /// the per-cell outcomes head-to-head without re-implementing
    /// the pair-flux iteration in the test.
    fn single_pass_for_comparison(
        tides: &Tides,
        state: &mut PhysicsState,
        dt: Real,
    ) {
        if tides.moons.is_empty() {
            return;
        }
        let grid = state.grid().clone();
        let width = i32::try_from(grid.width()).unwrap_or(1).max(1);
        let n = grid.n_cells();
        let mut potential = vec![Real::ZERO; n];
        let two_pi_v = two_pi();
        let pi_v = pi();
        let height = i32::try_from(grid.height()).unwrap_or(1).max(1);
        for (m_idx, moon) in tides.moons.iter().enumerate() {
            if moon.period_macros == 0 {
                continue;
            }
            let sub_lunar_q = tides.sub_lunar_q(m_idx, state.macro_step(), grid.width());
            for (cid, axial) in grid.cells() {
                let i = cid.0 as usize;
                let raw_diff = (axial.q - sub_lunar_q).rem_euclid(width);
                let signed_q_diff = if raw_diff <= width / 2 {
                    raw_diff
                } else {
                    raw_diff - width
                };
                let theta =
                    two_pi_v * Real::from_ratio(i64::from(signed_q_diff), i64::from(width));
                let lat_phase = pi_v
                    * Real::from_ratio(
                        i64::from(axial.r - moon.declination_r),
                        i64::from(height),
                    );
                let lat_cos = cos(lat_phase);
                let lat_falloff = lat_cos * lat_cos;
                potential[i] =
                    potential[i] + moon.mass_relative * cos(theta + theta) * lat_falloff;
            }
        }
        let prev_w = state.water_depth().to_vec();
        let mut next_w = prev_w.clone();
        donor_limited_pass(&grid, &potential, tides.tide_k, dt, &mut next_w);
        // Conservation must hold for a single pass too — the
        // pair-flux primitive is what guarantees it.
        debug_assert_eq!(
            prev_w.iter().copied().fold(Real::ZERO, |a, b| a + b),
            next_w.iter().copied().fold(Real::ZERO, |a, b| a + b),
            "single-pass donor-limited tide flux must preserve total water bit-exactly"
        );
        state.water_depth_mut().copy_from_slice(&next_w);
    }

    #[test]
    fn two_pass_drains_more_than_single_pass() {
        // Construct a seed where pass 1's donor cap leaves residual
        // unsatisfied requests that pass 2 can satisfy. We need
        // cells whose donor stock is low at the start of pass 1
        // but becomes substantial after pass-1 inflow — then their
        // outflow request to a third cell can actually be honoured
        // in pass 2.
        //
        // Pathological tide_k (0.9) + half-empty / half-full
        // chequerboard depth makes pass-1 donor caps fire on
        // essentially every active pair, so pass 2 has plenty of
        // residual transient to drain.
        let grid = HexGrid::new(8, 4);
        let mut state_single = PhysicsState::new(grid.clone());
        let mut state_double = PhysicsState::new(grid.clone());
        for (i, w) in state_single.water_depth_mut().iter_mut().enumerate() {
            *w = if i % 2 == 0 {
                Real::ZERO
            } else {
                Real::from_int(50)
            };
        }
        for (i, w) in state_double.water_depth_mut().iter_mut().enumerate() {
            *w = if i % 2 == 0 {
                Real::ZERO
            } else {
                Real::from_int(50)
            };
        }
        let initial_total: Real = state_single
            .water_depth()
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        let tides = Tides {
            tide_k: Real::from_ratio(9, 10), // 0.9 — well into donor-cap-saturation regime
            moons: vec![MoonTide {
                mass_relative: Real::ONE,
                period_macros: u32::MAX, // freeze sub-lunar at q=0
                declination_r: 0,
            }],
        };

        // Single pass: production minus the second `donor_limited_pass`.
        single_pass_for_comparison(&tides, &mut state_single, Real::ONE);
        // Two pass: the production path.
        tides.integrate(&mut state_double, Real::ONE);

        // Conservation: both paths must preserve total water bit-exactly.
        let single_total: Real = state_single
            .water_depth()
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        let double_total: Real = state_double
            .water_depth()
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        assert_eq!(
            initial_total, single_total,
            "single-pass donor-limited flux must conserve total water"
        );
        assert_eq!(
            initial_total, double_total,
            "two-pass donor-limited flux must conserve total water"
        );

        // The two-pass result must differ from single-pass on at
        // least one cell — proof that pass 2 did real work, not a
        // no-op. We also compute the total L1 distance moved by
        // each variant from the initial state; the two-pass total
        // must be strictly greater (more mass redistributed) than
        // the single-pass total under this seed.
        let initial: Vec<Real> = (0..state_single.water_depth().len())
            .map(|i| {
                if i % 2 == 0 {
                    Real::ZERO
                } else {
                    Real::from_int(50)
                }
            })
            .collect();
        let l1_single: Real = state_single
            .water_depth()
            .iter()
            .zip(initial.iter())
            .map(|(a, b)| (*a - *b).abs())
            .fold(Real::ZERO, |a, b| a + b);
        let l1_double: Real = state_double
            .water_depth()
            .iter()
            .zip(initial.iter())
            .map(|(a, b)| (*a - *b).abs())
            .fold(Real::ZERO, |a, b| a + b);
        assert!(
            l1_double > l1_single,
            "two-pass must redistribute strictly more mass than single-pass: \
             single_l1={l1_single:?} double_l1={l1_double:?}"
        );
        // Strict per-cell difference: at least one cell must end up
        // at a different value under two passes than under one.
        assert!(
            state_single
                .water_depth()
                .iter()
                .zip(state_double.water_depth().iter())
                .any(|(a, b)| a != b),
            "two-pass must reach a different per-cell state than single-pass"
        );
    }

    #[test]
    fn no_moon_is_no_op() {
        let grid = HexGrid::new(4, 4);
        let mut state = PhysicsState::new(grid);
        for (i, w) in state.water_depth_mut().iter_mut().enumerate() {
            *w = Real::from_int(10 + i64::try_from(i).unwrap());
        }
        let initial: Vec<_> = state.water_depth().to_vec();
        let tides = Tides::for_planet(vec![]);
        for _ in 0..20 {
            tides.integrate(&mut state, Real::ONE);
            state.advance_macro_step();
        }
        assert_eq!(state.water_depth(), &initial[..]);
    }

    #[test]
    fn tides_scale_with_gravity() {
        // T3: a 2g planet's tide coefficient must be measurably
        // larger than a 0.5g planet's. The `sqrt(g)` coupling means
        // ratio = sqrt(2 / 0.5) = 2, so high-g `tide_k` is exactly
        // 2× low-g `tide_k`.
        let high_g = Tides::for_gravity(Real::from_int(2));
        let low_g = Tides::for_gravity(Real::from_ratio(1, 2));
        assert!(
            high_g.tide_k > low_g.tide_k,
            "2g planet must have larger tide_k than 0.5g planet: \
             high_g.tide_k={:?} low_g.tide_k={:?}",
            high_g.tide_k,
            low_g.tide_k
        );
        // Earth-equivalent reference: gravity = 1 must reproduce the
        // legacy `earth_like` `tide_k` bit-exactly, so the existing
        // sim-physics tests (and the Earth-analog reference run) are
        // unaffected by the new scaling path.
        let earth = Tides::for_gravity(Real::ONE);
        let legacy = Tides::earth_like();
        assert_eq!(
            earth.tide_k, legacy.tide_k,
            "Real::ONE gravity must reproduce earth_like tide_k exactly"
        );
    }
}
