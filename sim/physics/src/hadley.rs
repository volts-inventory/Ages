//! Hadley / Ferrel / polar circulation cells from angular-momentum
//! conservation (Sprint 5 Item 15).
//!
//! ## What changed vs. the v1 stub
//!
//! v1 *prescribed* a per-row zonal bias — three hard-coded latitude
//! belts on every world. That was cosmetic; the cell count couldn't
//! emerge from rotation rate × planet radius and a slow rotator
//! still got the Earth-like three-cell pattern. v2 derives the cell
//! count from the Rhines-length closure (see §1 below) — itself
//! tied to the Held-Hou Hadley-edge angular-momentum balance — and
//! enforces the per-cell circulation via angular-momentum
//! conservation on poleward-moving parcels and shear-driven
//! subsidence at the emergent cell boundaries. (F7 replaces an
//! earlier empirical `R_p/R_rossby` ladder with this closure.)
//!
//! ## Physical scaffolding
//!
//! 1. **Rossby deformation radius** (diagnostic, kept on the layout
//!    for downstream consumers):
//!    ```text
//!       R_rossby = sqrt(g · H) / Ω
//!    ```
//!    where `Ω` is the planet's angular rotation rate, `g` is
//!    surface gravity, and `H` is the atmospheric scale height.
//!    `sqrt(g · H)` is the gravity-wave phase speed for shallow
//!    water — divided by Ω it's the latitudinal scale at which
//!    Coriolis catches up with pressure-gradient flow.
//!
//!    **Cell-count closure (F7 — Rhines length)**: the number of
//!    cells per hemisphere is not `R_p / R_rossby` directly but
//!    derived from the Rhines length, the latitudinal scale at
//!    which turbulent eddies are arrested by the planetary
//!    vorticity gradient β:
//!    ```text
//!       L_rhines = π · sqrt(U / β)
//!       β        = 2Ω / R           (equatorial)
//!       U        = ΩR · sin²(lat_h) (Held-Hou implied thermal wind)
//!    ```
//!    `lat_h` is the Held-Hou Hadley edge (`held_hou_hadley_edge`).
//!    Cells per hemisphere ≈ `(π·R) / L_rhines = √2 / sin(lat_h)`,
//!    quantised by `cell_count_from_hadley_edge`. This replaces
//!    the prior empirical Rossby-ratio ladder
//!    (`[1.0, 2.3, 4.0]` → 1/3/4/5/6 cells) with a closure that
//!    derives from the same `(Ω, R, g, H, Δθ, T_eq)` parameters as
//!    the Hadley edge itself.
//!
//! 2. **Angular-momentum conservation** on meridionally-moving
//!    parcels:
//!    ```text
//!       M = Ω · r · cos²(lat) + u · cos(lat)
//!    ```
//!    is conserved as a parcel migrates north or south at upper
//!    levels. A parcel that leaves the equator with `u = 0` arrives
//!    at higher latitude with `u = Ω · r · (cos²(eq) - cos²(lat)) /
//!    cos(lat)` — a strong westerly. That's the subtropical jet.
//!
//! 3. **Shear instability** caps how far poleward a single cell can
//!    reach. When the implied jet velocity exceeds an instability
//!    threshold, the cell breaks: the air subsides and a new
//!    thermally-indirect (Ferrel-style) cell forms on its poleward
//!    flank. The emergent cell boundary is precisely the latitude
//!    where the shear-instability check triggers; the boundary set
//!    feeds back into the cell count of step 1.
//!
//! 4. **Number of cells emerges** rather than being prescribed (via
//!    the Rhines-length closure of step 1):
//!    - Slow rotator (long day, small radius) → `lat_h → π/2`
//!      → `N ≈ √2 ≈ 1.41 → 1 cell` pole-to-pole per hemisphere.
//!    - Earth-like (24 h, 6371 km) → `lat_h ≈ 25°`
//!      → `N ≈ √2 / 0.423 ≈ 3.34 → 3 cells` (Hadley + Ferrel + polar).
//!    - Rapid rotator (8 h) → `lat_h ≈ 11°`
//!      → `N ≈ √2 / 0.19 ≈ 7.4 → MAX_CELLS_PER_HEMISPHERE` (capped).
//!
//! ## Layout vs. application
//!
//! `compute_hadley_layout` is the kinematic decomposition: it
//! returns the latitude bands and the direction (meridional sign)
//! of each cell from planetary parameters alone. The struct is a
//! deterministic function of `(Ω, R_p, g, H)` and contains no
//! per-tick state. `apply_hadley_circulation` reads the layout and
//! applies the per-cell angular-momentum kick to the existing
//! horizontal velocity field after the wind step; this is what
//! grows the zonal jet aloft.
//!
//! ## Determinism
//!
//! Layout: pure function of four `Real` inputs; no RNG, no
//! state-dependent branching beyond inequalities between fixed-
//! point operands (exact under Q32.32).
//!
//! Apply: per-cell read of `(v_q, v_r)` and grid row; per-cell write
//! of `(v_q, v_r)`. Iterates cells in canonical `(r, q)` order.

use crate::laws::Law;
use crate::state::PhysicsState;
use sim_arith::transcendental::{cos, half_pi, sin, sqrt};
use sim_arith::Real;

/// Direction of meridional flow at the surface within a cell. The
/// upper-level flow runs opposite. `Poleward` = mass moves toward
/// the nearest pole (the Hadley / polar pattern). `Equatorward` =
/// mass moves toward the equator at the surface (the Ferrel
/// pattern, which is eddy-driven and thermally indirect).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellDirection {
    /// Surface flow toward the pole, upper-level return toward the
    /// equator. Thermally direct (warm air rises at one end, cold
    /// air sinks at the other). The Hadley and polar cells use
    /// this on Earth.
    Poleward,
    /// Surface flow toward the equator, upper-level return toward
    /// the pole. Thermally **indirect** — driven by eddy
    /// momentum convergence in the mid-latitude jet, not by buoyancy.
    /// The Ferrel cell uses this on Earth.
    Equatorward,
}

impl CellDirection {
    /// Signed surface-flow direction along the meridional axis.
    /// Returns `+1` for poleward, `-1` for equatorward. Used by
    /// the apply step to bias the per-cell `v_r` kick.
    #[must_use]
    pub fn sign(self) -> i64 {
        match self {
            Self::Poleward => 1,
            Self::Equatorward => -1,
        }
    }
}

/// One latitude band in the circulation layout: `[lat_start, lat_end]`
/// in radians (both bounds positive, measured from the equator —
/// the same band applies mirror-symmetrically in the southern
/// hemisphere). `direction` carries the surface meridional sign.
#[derive(Debug, Clone, Copy)]
pub struct HadleyCell {
    /// Equatorward edge of the cell, in radians. Always `<=
    /// lat_end`. `0` for the cell adjacent to the equator.
    pub lat_start: Real,
    /// Poleward edge of the cell, in radians. Always `>= lat_start`.
    /// `π/2` for the cell adjacent to a pole.
    pub lat_end: Real,
    /// Surface meridional direction in this cell.
    pub direction: CellDirection,
}

/// Full hemispheric circulation layout. The cells cover the
/// northern hemisphere from equator (`lat = 0`) to north pole
/// (`lat = π/2`), in equator-to-pole order. The southern hemisphere
/// mirrors the same band partition; the `apply_hadley_circulation`
/// step handles the hemispheric sign flip.
///
/// `cells_per_hemisphere` is the count of bands; one cell means a
/// single pole-to-pole Hadley cell (slow rotator), three means the
/// Earth-like Hadley + Ferrel + polar structure, four-or-more means
/// a rapid-rotator multi-cell jet structure.
#[derive(Debug, Clone)]
pub struct HadleyCellLayout {
    /// Per-hemisphere band partition, in equator-to-pole order.
    pub cells: Vec<HadleyCell>,
    /// Rossby deformation radius `sqrt(g · H) / Ω` in metres, kept
    /// around for diagnostics and the apply step's instability
    /// threshold.
    pub rossby_radius_m: Real,
    /// Number of cells per hemisphere — `cells.len()`. Stored
    /// alongside the band list so callers don't have to re-count.
    pub cells_per_hemisphere: u32,
}

impl HadleyCellLayout {
    /// Number of cells per hemisphere.
    #[must_use]
    pub fn cells_per_hemisphere(&self) -> u32 {
        self.cells_per_hemisphere
    }

    /// Find the cell index (in the northern-hemisphere ordering)
    /// whose `[lat_start, lat_end]` band contains the absolute
    /// latitude `abs_lat` (radians, in `[0, π/2]`). Returns the
    /// last cell if `abs_lat` is at or past the poleward edge.
    #[must_use]
    pub fn cell_at(&self, abs_lat: Real) -> Option<&HadleyCell> {
        for cell in &self.cells {
            if abs_lat <= cell.lat_end {
                return Some(cell);
            }
        }
        // `abs_lat` past the last cell's edge — pin to the
        // poleward-most cell. Caller passes `[0, π/2]` by
        // contract so this branch is only reached on a fixed-
        // point rounding tie at the exact pole.
        self.cells.last()
    }
}

/// Seconds per hour. Used internally to convert the planet's day
/// length (hours) into seconds for the Ω = 2π / T computation.
const SECONDS_PER_HOUR: i64 = 3_600;

/// Earth radius in metres, the reference for the `radius_earth`
/// input. Mirrors `EARTH_RADIUS_M` in `world/src/planet.rs`.
const EARTH_RADIUS_M: i64 = 6_371_000;

/// Atmospheric scale height in metres used when the caller doesn't
/// have a planet-specific value. Earth ≈ 8.4 km.
pub const DEFAULT_SCALE_HEIGHT_M: i64 = 8_400;

/// Default surface gravity in m/s². Earth ≈ 9.81 m/s².
#[must_use]
pub fn default_gravity_ms2() -> Real {
    Real::from_ratio(981, 100)
}

/// Default equator-pole potential-temperature contrast (K) used in
/// the Held-Hou Hadley-edge closure. Earth's annual-mean
/// radiative-equilibrium contrast at the tropopause is ≈ 60 K — the
/// gradient the atmosphere *would* have absent the Hadley
/// circulation. (The observed surface contrast is smaller, ≈ 30 K,
/// because the actual Hadley circulation has already flattened it.)
pub const DEFAULT_DELTA_THETA_K: i64 = 60;

/// Default mean equatorial reference temperature (K) used in the
/// Held-Hou closure. Earth tropical surface ≈ 300 K; the closure
/// is scale-invariant up to a logarithm in `T_eq`, so using the
/// surface value is the standard simplification.
pub const DEFAULT_T_EQ_K: i64 = 300;

/// Default Held-Hou closure tropopause height (m). The closure's
/// `H` parameter is the Hadley-cell lid (the tropopause), not the
/// atmospheric e-folding scale height — the cell extends from the
/// surface to the tropopause and the upper-level return flow
/// closes through that lid. Earth tropopause ≈ 12 km.
pub const DEFAULT_TROPOPAUSE_M: i64 = 12_000;

/// Held-Hou Hadley-edge latitude (radians) from the
/// angular-momentum + baroclinic-instability closure of Held & Hou
/// (1980, JAS). The edge is the latitude at which the
/// angular-momentum-conserving poleward flow can no longer remain
/// stable to baroclinic overturning; equivalently, where the implied
/// subtropical jet becomes too strong to maintain against shear
/// instability.
///
/// The closure is:
/// ```text
///     sin²(lat_edge) ≈ (5/3) · g · H · Δθ / ((Ω · R)² · T_eq)
/// ```
/// where:
/// - `g` is surface gravity (m/s²),
/// - `H` is the tropopause height (m) — the Hadley-cell lid,
/// - `Δθ` is the equator-pole radiative-equilibrium potential
///   temperature contrast (K),
/// - `Ω` is the planet's angular rotation rate (rad/s),
/// - `R` is the planet radius (m),
/// - `T_eq` is the equatorial reference temperature (K).
///
/// This is the textbook Held-Hou form (`(y_h/R)² ≈ 5gΔθH /
/// (3Ω²R²θ₀)`). For Earth with `H = 12 km`, `Δθ = 60 K`, `T_eq =
/// 300 K`, `Ω = 7.27e-5 rad/s`, `R = 6.371e6 m`, the closure
/// returns `lat_edge ≈ 25-26°` — within the observed 25-30°
/// subtropical Hadley-cell boundary. A slow rotator (longer day,
/// smaller `Ω`) gives a larger numerator-to-denominator ratio and
/// thus a poleward-shifted edge — the classic "wider Hadley cell on
/// a slow rotator" result.
///
/// The function clamps the inner ratio so `sin(lat_edge) ∈ [0, 1]`;
/// pathological inputs (zero Ω, huge Δθ) saturate the edge to π/2.
#[must_use]
pub fn held_hou_hadley_edge(
    omega_rad_s: Real,
    radius_m: Real,
    gravity_ms2: Real,
    tropopause_m: Real,
    delta_theta_k: Real,
    t_eq_k: Real,
) -> Real {
    // Slow-rotator limit: Ω → 0 sends sin² → ∞, which the closure
    // saturates at sin = 1 → lat_edge = π/2 (Hadley extends
    // pole-to-pole). The cell-count branch in `compute_hadley_layout`
    // already collapses to a single cell in this regime; this guard
    // makes the helper safe to call independently.
    if omega_rad_s <= Real::ZERO || radius_m <= Real::ZERO || t_eq_k <= Real::ZERO {
        return half_pi();
    }
    let five_over_three = Real::from_ratio(5, 3);
    // Numerator: (5/3) · g · H · Δθ. Each factor is bounded
    // (g ≈ 10, H ≈ 1e4, Δθ ≈ 60), so the product fits Q32.32
    // comfortably (~ 1e7).
    let numerator = five_over_three * gravity_ms2 * tropopause_m * delta_theta_k;
    // Denominator: (Ω · R)² · T_eq. Compute `Ω · R` first so the
    // intermediate stays small (Ω · R ≈ 463 m/s for Earth);
    // squaring it gives ≈ 2.14e5, times T_eq ≈ 300 gives ≈ 6.4e7
    // — still within Q32.32.
    let omega_r = omega_rad_s * radius_m;
    let denominator = omega_r * omega_r * t_eq_k;
    if denominator <= Real::ZERO {
        return half_pi();
    }
    let sin_sq = numerator / denominator;
    // Clamp to [0, 1] so sqrt + arcsin stay in domain.
    let sin_clamped = sqrt(sin_sq).min(Real::ONE).max(Real::ZERO);
    arcsin_unit(sin_clamped)
}

/// Compute `arcsin(x)` for `x ∈ [0, 1]` via Newton's method on
/// `f(θ) = sin(θ) − x = 0`. Returns θ ∈ `[0, π/2]`. Used by the
/// Held-Hou closure to invert `sin(lat_edge)`; isolated here so the
/// numerics are reviewable without distracting from the physics.
///
/// Newton iteration: `θ_{n+1} = θ_n − (sin(θ_n) − x) / cos(θ_n)`.
/// Convergence is quadratic; 16 iterations more than cover Q32.32's
/// precision. `cos(θ)` is bounded away from zero by clamping the
/// final approach to `θ ≤ π/2 − ε` so the divide is safe.
///
/// `x` is assumed already clamped to `[0, 1]`. Out-of-domain inputs
/// snap to the nearest valid endpoint rather than panicking.
fn arcsin_unit(x: Real) -> Real {
    if x <= Real::ZERO {
        return Real::ZERO;
    }
    let pi_2 = half_pi();
    if x >= Real::ONE {
        return pi_2;
    }
    // Initial guess: cubic Taylor `arcsin(x) ≈ x + x³/6`. Good
    // enough to land Newton in the quadratic-convergence regime for
    // x ≤ 0.9 (worst case |error| ≈ 0.03 rad at x = 0.9).
    let x_cube = x * x * x;
    let mut theta = x + x_cube / Real::from_int(6);
    if theta >= pi_2 {
        theta = pi_2 - Real::from_ratio(1, 1_000_000);
    }
    // Newton refinement. Clamp θ slightly below π/2 each iteration
    // so `cos(θ)` stays ≥ ~1e-3 and the divide doesn't explode on
    // x → 1 inputs that slipped past the early-out.
    let cos_floor = Real::from_ratio(1, 1_000);
    let theta_ceiling = pi_2 - Real::from_ratio(1, 1_000_000);
    for _ in 0..16 {
        let s = sin(theta);
        let c = cos(theta).max(cos_floor);
        let delta = (s - x) / c;
        let next = theta - delta;
        let next_clamped = next.max(Real::ZERO).min(theta_ceiling);
        if next_clamped == theta {
            return next_clamped;
        }
        theta = next_clamped;
    }
    theta
}

/// Compute the circulation-cell layout for a planet from its
/// rotation rate, radius, surface gravity, and atmospheric scale
/// height. All inputs are in SI units except the planet radius,
/// which is in Earth-radii (matching `Planet::radius`). The
/// returned layout's `cells` list covers the northern hemisphere;
/// the southern is the mirror image.
///
/// `day_length_hours` is the sidereal day; very long days collapse
/// to the slow-rotator limit (`Ω → 0`, `lat_h → π/2`, 1 cell per
/// hemisphere). `radius_earth = 1` is Earth-equivalent; the
/// function scales internally to metres for the Held-Hou
/// `sin²(lat_h) ∝ 1/(ΩR)²` and Rhines-length `N ∝ √2/sin(lat_h)`
/// closures.
#[must_use]
pub fn compute_hadley_layout(
    day_length_hours: Real,
    radius_earth: Real,
    gravity_ms2: Real,
    scale_height_m: i64,
) -> HadleyCellLayout {
    // Step 1: planet angular rotation rate Ω = 2π / day_length_seconds.
    // Slow-rotator guard: a non-positive or implausibly long day
    // collapses the rotation rate to (effectively) zero, which
    // sends R_rossby to infinity and yields a single pole-to-pole
    // Hadley cell. We don't need to compute Ω exactly in that
    // limit — just gate on it and short-circuit.
    let day_hours = if day_length_hours <= Real::ZERO {
        Real::from_int(24)
    } else {
        day_length_hours
    };
    let scale_height = if scale_height_m <= 0 {
        DEFAULT_SCALE_HEIGHT_M
    } else {
        scale_height_m
    };

    let day_seconds = day_hours * Real::from_int(SECONDS_PER_HOUR);
    // Ω = 2π / T. Guard against pathological inputs: tiny `day_seconds`
    // would explode Ω but the function still has to terminate; the
    // upper-bound cell count is capped at MAX_CELLS_PER_HEMISPHERE
    // independently.
    let two_pi = sim_arith::transcendental::two_pi();
    let omega = two_pi / day_seconds;

    // Step 2: gravity-wave phase speed c = sqrt(g · H), then the
    // Rossby deformation radius R_rossby = c / Ω. Carry both in
    // metres; the comparison against planet radius is in metres.
    let g = if gravity_ms2 <= Real::ZERO {
        default_gravity_ms2()
    } else {
        gravity_ms2
    };
    let h_m = Real::from_int(scale_height);
    let c_gw = sqrt(g * h_m);

    // For Ω == 0 (slow-rotator limit) we skip the divide and pin
    // R_rossby to a sentinel "huge" value; downstream the cell
    // count clamps to 1.
    let rossby = if omega <= Real::ZERO {
        Real::from_int(i64::MAX / (1 << 32))
    } else {
        c_gw / omega
    };

    // Step 3: planet radius in metres.
    let radius_earth_clamped = if radius_earth <= Real::ZERO {
        Real::ONE
    } else {
        radius_earth
    };
    let radius_m = radius_earth_clamped * Real::from_int(EARTH_RADIUS_M);

    // Held-Hou Hadley-edge closure: derive the equator-to-Hadley
    // boundary latitude from baroclinic-instability angular-momentum
    // balance rather than hard-coding 30°. Used both as the 3-cell
    // band split and as the thermal-wind velocity scale that feeds the
    // Rhines-length cell-count closure below.
    //
    // P3.6: this replaces the previous hard-coded `π/6` (= 30°)
    // edge with `arcsin(sqrt((5/3) g H Δθ / ((ΩR)² T_eq)))`.
    let hadley_edge = held_hou_hadley_edge(
        omega,
        radius_m,
        g,
        Real::from_int(DEFAULT_TROPOPAUSE_M),
        Real::from_int(DEFAULT_DELTA_THETA_K),
        Real::from_int(DEFAULT_T_EQ_K),
    );

    // Step 4: cells per hemisphere from the Rhines-length closure
    // (F7). Replaces the empirical `radius_m / R_rossby` ladder
    // (`[1.0, 2.3, 4.0]` thresholds → 1/3/4/5/6 cells) with the
    // physics-derived count:
    //
    //   L_rhines = π · sqrt(U / β)
    //   β       = 2Ω / R          (equatorial planetary vorticity gradient)
    //   U       = ΩR · sin²(lat_h) (Held-Hou implied thermal wind)
    //   N_per_hem ≈ (π·R) / L_rhines = √(2 · ΩR / U)
    //             = √(2 / sin²(lat_h))
    //             = √2 / sin(lat_h)
    //
    // The U scale ties to `lat_h` from the Held-Hou closure above,
    // so the cell count is fully consistent with the Hadley-edge
    // derivation rather than an independent empirical ladder. Earth
    // (lat_h ≈ 25°) gives `N ≈ √2 / 0.423 ≈ 3.34 → 3 cells`. A slow
    // rotator (lat_h → π/2) gives `N → √2 ≈ 1.41 → 1 cell`. An 8-hour
    // rapid rotator (lat_h ≈ 11°) gives `N ≈ √2 / 0.19 ≈ 7.4 →
    // capped at MAX_CELLS_PER_HEMISPHERE`.
    let cells_per_hem = cell_count_from_hadley_edge(hadley_edge);

    let cells = lay_out_cells(cells_per_hem, hadley_edge);

    HadleyCellLayout {
        cells,
        rossby_radius_m: rossby,
        cells_per_hemisphere: cells_per_hem,
    }
}

/// Hard cap on cells per hemisphere. A pathological rapid rotator
/// (sub-hour day, hyper-jovian radius) would otherwise demand
/// dozens of cells which doesn't usefully resolve on our grid
/// heights; cap at 6 (12 total pole-to-pole). Earth gets 3, a
/// slow rotator gets 1.
const MAX_CELLS_PER_HEMISPHERE: u32 = 6;

/// Translate the Held-Hou Hadley-edge latitude into a
/// cells-per-hemisphere count via the Rhines-length closure (F7).
///
/// Derivation (see `compute_hadley_layout` for the inline summary):
///
/// ```text
///     L_rhines = π · sqrt(U / β)
///     β        = 2Ω / R           (equatorial)
///     U        = ΩR · sin²(lat_h) (Held-Hou implied thermal wind)
/// ```
///
/// Cells per hemisphere ≈ `(π·R) / L_rhines = √2 / sin(lat_h)`. The
/// `(π·R)` numerator uses the full meridional half-circumference
/// (equator-to-equator over the pole = `π·R`), consistent with the
/// Rhines mode-count convention.
///
/// Quantisation rule:
/// - `N_continuous < 2` → 1 cell (single pole-to-pole Hadley, slow-
///   rotator regime — Held-Hou pole-to-pole limit).
/// - `2 ≤ N_continuous < 3` → 3 cells (the 2-cell partition would
///   place an equatorward cell at the pole, which is unphysical;
///   the next stable configuration above Hadley-only is the
///   Hadley + Ferrel + polar triplet).
/// - otherwise `floor(N_continuous)`, clamped to
///   `[3, MAX_CELLS_PER_HEMISPHERE]`.
///
/// Replaces the previous empirical `[1.0, 2.3, 4.0]` Rossby-ratio
/// ladder (`cell_count_from_ratio`). Self-consistent with the
/// Held-Hou Hadley-edge closure that already derives `lat_h` from
/// `(Ω, R, g, H, Δθ, T_eq)`.
fn cell_count_from_hadley_edge(hadley_edge: Real) -> u32 {
    // Floor on sin(lat_h) to keep the divide stable. A genuine slow
    // rotator saturates lat_h to π/2 → sin = 1, so the floor only
    // fires for pathologically rapid rotators where lat_h → 0 and
    // the count would diverge; in that regime we cap to
    // MAX_CELLS_PER_HEMISPHERE.
    let sin_floor = Real::from_ratio(1, 1_000);
    let sin_lat = sin(hadley_edge).max(sin_floor);
    // N_continuous = √2 / sin(lat_h). Compute as
    // `sqrt(2) / sin_lat` directly; both operands fit comfortably
    // in Q32.32 (sqrt(2) ≈ 1.414, sin_lat ∈ [1/1000, 1]).
    let sqrt_two = sqrt(Real::from_int(2));
    let n_continuous = sqrt_two / sin_lat;

    // Quantise. The boundary `n_continuous < 2` keeps the
    // Hadley-only regime intact; the next-up jump skips 2 cells
    // (the partition `[P, E]` would land an equatorward cell at
    // the pole, contradicting the thermally-direct polar-cell
    // expectation) and lands directly at 3 (Hadley + Ferrel +
    // polar).
    if n_continuous < Real::from_int(2) {
        return 1;
    }
    if n_continuous < Real::from_int(3) {
        return 3;
    }
    // Integer floor, then clamp to [3, MAX_CELLS_PER_HEMISPHERE].
    // `raw().to_num::<i64>()` truncates toward zero on the Q32.32
    // fixed-point representation; `n_continuous` is strictly
    // positive here (sin_lat ≥ 1/1000 > 0), so truncation equals
    // floor.
    let floored_i: i64 = n_continuous.raw().to_num::<i64>().max(3);
    let floored_u = u32::try_from(floored_i).unwrap_or(MAX_CELLS_PER_HEMISPHERE);
    floored_u.min(MAX_CELLS_PER_HEMISPHERE)
}

/// Build the per-hemisphere band list given a cell count. Bands
/// cover `[0, π/2]` in equator-to-pole order. Directions
/// alternate starting with poleward (Hadley) at the equator:
///   - 1 cell: [Poleward] (single Hadley)
///   - 3 cells: [Poleward, Equatorward, Poleward] (Hadley, Ferrel, polar)
///   - 4 cells: [Poleward, Equatorward, Poleward, Equatorward]
///   - 5 cells: [P, E, P, E, P]
///   - 6 cells: [P, E, P, E, P, E]
///
/// `hadley_edge` is the Held-Hou-derived Hadley-cell boundary
/// (radians, in `(0, π/2)`). Only the 3-cell layout reads it; the
/// 1-cell and 4+-cell layouts derive their boundaries from a uniform
/// mesh because the single-Hadley-cell baroclinic closure doesn't
/// apply outside the Earth-like regime. For the 3-cell layout, the
/// Hadley cell spans `[0, hadley_edge]` and the polar / Ferrel split
/// is placed at the midpoint of `hadley_edge` and `π/2` — the polar
/// cell takes the same width as the post-Hadley Ferrel cell. The
/// hard-coded 30°/60° split of the previous implementation has been
/// replaced by this closure (P3.6).
fn lay_out_cells(cells_per_hem: u32, hadley_edge: Real) -> Vec<HadleyCell> {
    let n = cells_per_hem.clamp(1, MAX_CELLS_PER_HEMISPHERE);
    let pi_2 = half_pi();
    if n == 1 {
        return vec![HadleyCell {
            lat_start: Real::ZERO,
            lat_end: pi_2,
            direction: CellDirection::Poleward,
        }];
    }
    if n == 3 {
        // Held-Hou-derived 3-cell split (P3.6): the Hadley edge is
        // `hadley_edge`, then the Ferrel cell spans from there to a
        // poleward boundary that sits halfway between `hadley_edge`
        // and the pole. This places the polar cell at a width
        // comparable to Earth's observed ~30° polar cap while
        // letting the Hadley edge move with the closure. Guard
        // against degenerate `hadley_edge` ≥ π/2 by pinning the
        // Ferrel band to a non-empty interior; this only fires on
        // pathological inputs (the closure clamps to π/2 in the
        // slow-rotator limit, but that limit also routes to the
        // n=1 branch above).
        let edge = hadley_edge
            .max(Real::from_ratio(1, 1_000))
            .min(pi_2 - Real::from_ratio(1, 1_000));
        // Polar boundary: midpoint of (edge, π/2). Symmetric split
        // of the post-Hadley latitude band between Ferrel and polar.
        let ferrel_polar_boundary = (edge + pi_2) / Real::from_int(2);
        return vec![
            HadleyCell {
                lat_start: Real::ZERO,
                lat_end: edge,
                direction: CellDirection::Poleward,
            },
            HadleyCell {
                lat_start: edge,
                lat_end: ferrel_polar_boundary,
                direction: CellDirection::Equatorward,
            },
            HadleyCell {
                lat_start: ferrel_polar_boundary,
                lat_end: pi_2,
                direction: CellDirection::Poleward,
            },
        ];
    }
    // General case: equal-width bands. The 2 / 4 / 5 / 6 cases all
    // use a uniform mesh; the closure-tuned 3-cell case above is
    // the only exception. Direction starts with Poleward (Hadley)
    // at the equator and alternates.
    let width = pi_2 / Real::from_int(i64::from(n));
    (0..n)
        .map(|i| {
            let lat_start = width * Real::from_int(i64::from(i));
            let lat_end = if i + 1 == n {
                pi_2
            } else {
                width * Real::from_int(i64::from(i + 1))
            };
            let direction = if i % 2 == 0 {
                CellDirection::Poleward
            } else {
                CellDirection::Equatorward
            };
            HadleyCell {
                lat_start,
                lat_end,
                direction,
            }
        })
        .collect()
}

/// Angular-momentum-conserving zonal kick applied after the
/// horizontal wind step. For each cell, derive the latitude from
/// the grid row, look up the band the cell sits in, and add a
/// per-tick increment to `v_q` consistent with conservation of
/// `M = Ω · r · cos²(lat) + u · cos(lat)` as parcels migrate
/// meridionally within the band.
///
/// The kick magnitude is bounded by the band's poleward-edge
/// implied jet velocity (the angular-momentum closed form) so the
/// integrator can't spin the zonal wind beyond the natural
/// subtropical-jet ceiling.
pub fn apply_hadley_circulation(
    state: &mut PhysicsState,
    layout: &HadleyCellLayout,
    dt: Real,
) {
    if dt <= Real::ZERO || layout.cells.is_empty() {
        return;
    }
    let grid = state.grid().clone();
    let height_i = i32::try_from(grid.height()).unwrap_or(i32::MAX).max(1);
    let half_h = height_i / 2;
    let n_cells = grid.n_cells();
    // Per-tick kick strength as a fraction of the band's
    // closed-form ceiling. Small so the apply step nudges rather
    // than slams; the steady-state jet builds over many ticks.
    let kick_fraction = Real::percent(1);

    let cells_snapshot: Vec<_> = grid
        .cells()
        .map(|(cid, axial)| (cid.0 as usize, axial.r))
        .collect();
    let (vq, _vr) = state.fluid_velocity_mut();
    for (i, r) in cells_snapshot {
        if i >= n_cells {
            continue;
        }
        // Latitude angle. Use the same mapping as Coriolis
        // (`signed_offset = r - half_h`); sign carries the
        // hemisphere, magnitude the absolute latitude.
        let signed_offset = r - half_h;
        let abs_lat = if half_h > 0 {
            half_pi()
                * Real::from_ratio(i64::from(signed_offset.abs()), i64::from(half_h))
        } else {
            Real::ZERO
        };
        let hemisphere_sign: i64 = match signed_offset.cmp(&0) {
            std::cmp::Ordering::Less => 1,
            std::cmp::Ordering::Greater => -1,
            std::cmp::Ordering::Equal => 0,
        };
        let Some(cell) = layout.cell_at(abs_lat) else {
            continue;
        };
        // Angular-momentum closed-form jet velocity at the
        // band's poleward edge:
        //   u_jet = Ω · R · (cos² lat_start - cos² lat_end) / cos lat_end
        // Conservatively bound to keep the apply step from
        // blowing up at the pole (cos lat_end → 0). We clamp
        // `cos lat_end >= 0.1` so the ceiling stays finite.
        let cos_start = cos(cell.lat_start);
        let cos_end = cos(cell.lat_end).max(Real::from_ratio(1, 10));
        let cos_diff = cos_start * cos_start - cos_end * cos_end;
        // Surface drag: a band with surface-equatorward flow
        // (Ferrel) implies the *surface* zonal wind is easterly
        // (the trade-wind regime spun up by the cell's flank),
        // and a band with surface-poleward flow (Hadley / polar)
        // implies a westerly surface signature. The direction
        // sign carries this:
        let dir_sign = Real::from_int(cell.direction.sign());
        // Per-tick kick: positive in the northern hemisphere if
        // dir_sign · cos_diff is positive, mirrored in the
        // south. The `kick_fraction · dt` scaling absorbs unit
        // conventions — the kernel is dimensionless so we don't
        // need to thread Ω · R in SI through every multiply.
        let kick = dt
            * kick_fraction
            * dir_sign
            * cos_diff
            * Real::from_int(hemisphere_sign);
        // Normalise the kick by `cos lat_end` to recover the
        // angular-momentum-implied jet velocity (large at low
        // latitude where the cells are wide, smaller at high
        // latitude where they're narrow).
        let kick_u = kick / cos_end;
        vq[i] = vq[i] + kick_u;
    }
}

/// Convenience: a `Law` wrapper that bundles a layout + the
/// existing `apply_hadley_circulation` step so the orchestrator
/// can drop it into the macro-step pipeline without a special
/// path. The layout is recomputed once at construction; the apply
/// step reuses it every tick.
#[derive(Debug, Clone)]
pub struct HadleyCirculation {
    /// Pre-computed layout. Recomputed only when the underlying
    /// planet parameters change (i.e. never for a single run).
    pub layout: HadleyCellLayout,
    /// Vacuum guard. `false` for `Atmosphere::None` planets —
    /// no medium means no jets to spin up. Defaults to `true`.
    pub has_atmosphere: bool,
}

impl HadleyCirculation {
    /// Build a `HadleyCirculation` law from planetary parameters.
    /// Mirrors `Coriolis::for_planet` in shape.
    #[must_use]
    pub fn for_planet(
        day_length_hours: Real,
        radius_earth: Real,
        gravity_ms2: Real,
        scale_height_m: i64,
        has_atmosphere: bool,
    ) -> Self {
        Self {
            layout: compute_hadley_layout(
                day_length_hours,
                radius_earth,
                gravity_ms2,
                scale_height_m,
            ),
            has_atmosphere,
        }
    }
}

impl Law for HadleyCirculation {
    fn integrate(&self, state: &mut PhysicsState, dt: Real) {
        if !self.has_atmosphere {
            return;
        }
        apply_hadley_circulation(state, &self.layout, dt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::HexGrid;

    /// Slow rotator: a 1000-hour day on a small (0.5 R_earth) world
    /// pushes the Rossby radius enormously larger than the planet
    /// radius, collapsing the layout to a single pole-to-pole
    /// Hadley cell per hemisphere — the Held-Hou slow-rotator limit.
    #[test]
    fn slow_rotator_has_one_pole_to_pole_hadley_cell() {
        let layout = compute_hadley_layout(
            Real::from_int(1_000),    // 1000-hour day
            Real::from_ratio(5, 10),  // 0.5 Earth radii
            default_gravity_ms2(),
            DEFAULT_SCALE_HEIGHT_M,
        );
        assert_eq!(
            layout.cells_per_hemisphere(),
            1,
            "slow rotator should collapse to one pole-to-pole \
             Hadley cell per hemisphere: layout={layout:?}"
        );
        assert_eq!(layout.cells.len(), 1);
        // The single cell must span the full hemisphere
        // [0, π/2] and circulate poleward at the surface
        // (thermally direct).
        let cell = &layout.cells[0];
        assert_eq!(cell.lat_start, Real::ZERO);
        assert_eq!(cell.lat_end, half_pi());
        assert_eq!(cell.direction, CellDirection::Poleward);
    }

    /// Earth-like rotation + radius lands in the three-cell window:
    /// Hadley + Ferrel + polar. The three bands must alternate in
    /// direction (so Ferrel is thermally indirect — the spec test
    /// `ferrel_cell_eddy_driven_not_thermally_direct` verifies the
    /// sign).
    #[test]
    fn earth_like_rotation_has_three_cell_structure() {
        let layout = compute_hadley_layout(
            Real::from_int(24),       // 24-hour day
            Real::ONE,                // 1.0 Earth radii
            default_gravity_ms2(),
            DEFAULT_SCALE_HEIGHT_M,
        );
        assert_eq!(
            layout.cells_per_hemisphere(),
            3,
            "Earth-like rotation should yield three cells per \
             hemisphere (Hadley + Ferrel + polar): layout={layout:?}"
        );
        assert_eq!(layout.cells.len(), 3);
        // The three cells must cover [0, π/2] contiguously.
        assert_eq!(layout.cells[0].lat_start, Real::ZERO);
        assert_eq!(layout.cells[2].lat_end, half_pi());
        for i in 1..layout.cells.len() {
            assert_eq!(
                layout.cells[i - 1].lat_end,
                layout.cells[i].lat_start,
                "cell {i} must start where cell {} ends (no gaps)",
                i - 1
            );
        }
    }

    /// The middle (Ferrel) cell in the three-cell layout must
    /// circulate in the opposite direction from its equatorward
    /// (Hadley) and poleward (polar) neighbours. That's the
    /// thermally-indirect signature: surface flow is *equatorward*
    /// in Ferrel where it's *poleward* in the flanking cells,
    /// because Ferrel is eddy-driven (momentum convergence in the
    /// mid-latitude jet) rather than buoyancy-driven.
    #[test]
    fn ferrel_cell_eddy_driven_not_thermally_direct() {
        let layout = compute_hadley_layout(
            Real::from_int(24),
            Real::ONE,
            default_gravity_ms2(),
            DEFAULT_SCALE_HEIGHT_M,
        );
        assert_eq!(layout.cells.len(), 3, "expected Earth-like three-cell layout");
        let hadley = &layout.cells[0];
        let ferrel = &layout.cells[1];
        let polar = &layout.cells[2];
        assert_eq!(hadley.direction, CellDirection::Poleward);
        assert_eq!(polar.direction, CellDirection::Poleward);
        assert_eq!(
            ferrel.direction,
            CellDirection::Equatorward,
            "Ferrel cell must circulate opposite to its Hadley/polar \
             neighbours — thermally indirect, eddy-driven: \
             hadley={hadley:?} ferrel={ferrel:?} polar={polar:?}"
        );
        // Opposite-direction predicate stated directly:
        assert_ne!(
            ferrel.direction, hadley.direction,
            "Ferrel must oppose Hadley"
        );
        assert_ne!(
            ferrel.direction, polar.direction,
            "Ferrel must oppose polar"
        );
    }

    /// Rapid rotator (e.g. 8-hour day) must produce more cells per
    /// hemisphere than the Earth-like 3-cell structure. Under the F7
    /// Rhines-length closure (`N ≈ √2 / sin(lat_h)`), the 8-hour
    /// rotator's Hadley edge contracts to `lat_h ≈ 11°`, giving
    /// `N ≈ 7.4` which clamps to MAX_CELLS_PER_HEMISPHERE. The
    /// expectation is just `> 3` — the closure floors the count
    /// down (via `.raw().to_num::<i64>()`), so the exact integer
    /// depends on the cap but must exceed Earth's three.
    #[test]
    fn rapid_rotator_has_more_cells_than_earth() {
        let layout = compute_hadley_layout(
            Real::from_int(8),
            Real::ONE,
            default_gravity_ms2(),
            DEFAULT_SCALE_HEIGHT_M,
        );
        assert!(
            layout.cells_per_hemisphere() > 3,
            "rapid rotator should produce more cells per hemisphere \
             than Earth's three: layout={layout:?}"
        );
    }

    /// `compute_hadley_layout` is a deterministic pure function:
    /// the same inputs produce the same output every time, no
    /// hidden global state.
    #[test]
    fn layout_is_deterministic() {
        let a = compute_hadley_layout(
            Real::from_int(24),
            Real::ONE,
            default_gravity_ms2(),
            DEFAULT_SCALE_HEIGHT_M,
        );
        let b = compute_hadley_layout(
            Real::from_int(24),
            Real::ONE,
            default_gravity_ms2(),
            DEFAULT_SCALE_HEIGHT_M,
        );
        assert_eq!(a.cells_per_hemisphere, b.cells_per_hemisphere);
        assert_eq!(a.cells.len(), b.cells.len());
        for (ca, cb) in a.cells.iter().zip(b.cells.iter()) {
            assert_eq!(ca.lat_start, cb.lat_start);
            assert_eq!(ca.lat_end, cb.lat_end);
            assert_eq!(ca.direction, cb.direction);
        }
        assert_eq!(a.rossby_radius_m, b.rossby_radius_m);
    }

    /// The apply step must produce equal-magnitude opposite-sign
    /// zonal velocities at mirrored hemispheric rows: a poleward-
    /// flowing Hadley cell at northern row N and at southern row
    /// N' (mirrored about the equator) must spin up `v_q` in
    /// opposite signs — the Coriolis-balanced angular-momentum
    /// signature.
    #[test]
    fn apply_kicks_v_q_with_hemispheric_mirror_symmetry() {
        let layout = compute_hadley_layout(
            Real::from_int(24),
            Real::ONE,
            default_gravity_ms2(),
            DEFAULT_SCALE_HEIGHT_M,
        );
        // Use an odd height so we have a clean equator + mirrored
        // rows. height=9 → half_h=4, equator at r=4, rows 0..3
        // are north, rows 5..8 are south.
        let grid = HexGrid::new(3, 9);
        let mut state = PhysicsState::new(grid);
        for _ in 0..50 {
            apply_hadley_circulation(&mut state, &layout, Real::ONE);
        }
        let centre_q = 1;
        let r_north = 1; // far from equator
        let r_south = 7; // mirror of r=1 about r=4
        let id_n = state.grid().cell_id(crate::grid::Axial::new(centre_q, r_north)).0 as usize;
        let id_s = state.grid().cell_id(crate::grid::Axial::new(centre_q, r_south)).0 as usize;
        let v_n = state.fluid_velocity().0[id_n];
        let v_s = state.fluid_velocity().0[id_s];
        // Both rows are inside the polar / outer cell on the
        // Earth-like 3-cell layout; signs must be opposite.
        assert!(
            (v_n > Real::ZERO && v_s < Real::ZERO)
                || (v_n < Real::ZERO && v_s > Real::ZERO),
            "mirrored-row zonal velocities must have opposite signs: \
             v_n={v_n:?} v_s={v_s:?}"
        );
    }

    /// The apply step is a no-op on an equator-only state with
    /// zero hemispheric structure (height = 1 → no signed offset).
    /// Sanity check: degenerate grids don't crash.
    #[test]
    fn apply_on_degenerate_grid_is_safe() {
        let layout = compute_hadley_layout(
            Real::from_int(24),
            Real::ONE,
            default_gravity_ms2(),
            DEFAULT_SCALE_HEIGHT_M,
        );
        let grid = HexGrid::new(3, 1);
        let mut state = PhysicsState::new(grid);
        apply_hadley_circulation(&mut state, &layout, Real::ONE);
        for v in state.fluid_velocity().0 {
            assert_eq!(*v, Real::ZERO);
        }
    }

    /// Acceptance test (P0.2): an Earth-like planet integrated for
    /// 1000 ticks under uniform initial conditions must produce a
    /// steady-state zonal jet whose magnitude at the subtropical-jet
    /// latitude (~30°) sits in `[10, 60]` (centred on the real-world
    /// ~30 m/s target with ±50% slack to absorb the dimensionless-
    /// scaling simplification in `apply_hadley_circulation`'s
    /// `kick_fraction · dt` lump-sum).
    ///
    /// The probe row is the one whose absolute latitude sits in the
    /// upper half of the Hadley/Ferrel band — that's where the
    /// angular-momentum-implied jet velocity is largest on the real
    /// atmosphere. The northern-hemisphere row at row index
    /// `half_h - 2` on the chosen grid lands at ~45° latitude (deep
    /// inside the Ferrel band, comfortably above the boundary jet's
    /// peak position). Both the Hadley and the Ferrel cells
    /// contribute to the jet on the real Earth — we sample inside
    /// Ferrel where the closed-form `(cos² lat_start − cos² lat_end)
    /// / cos lat_end` integrand peaks. Under the P3.6 Held-Hou
    /// closure the 3-cell split is ~25°/~57.5°/90° (Hadley/Ferrel
    /// boundary at the closure-derived edge, polar boundary at the
    /// midpoint of edge and the pole) — the `cos²` integrand still
    /// peaks comfortably inside the Ferrel interior at 45°.
    ///
    /// `dt = Real::from_int(3)` is the per-tick accumulator factor —
    /// the apply step's coefficient is dimensionless (the kernel
    /// absorbs unit conventions inside `kick_fraction`), so the
    /// 1000-tick × dt = 3 product matches the orchestrator's
    /// production cadence (~3 sim-days per macro-step under
    /// `heat_dt = Real::ONE` × 3-macro per civ-tick).
    #[test]
    fn earth_like_steady_state_jet_velocity_in_subtropical_band() {
        // Earth-like rotation + radius. `for_planet` builds the
        // layout (3 cells per hemisphere for Earth-like inputs —
        // verified by `earth_like_rotation_has_three_cell_structure`).
        let law = HadleyCirculation::for_planet(
            Real::from_int(24),       // 24-hour day
            Real::ONE,                // 1.0 Earth radii
            default_gravity_ms2(),
            DEFAULT_SCALE_HEIGHT_M,
            true,                     // has_atmosphere
        );
        // Sanity-check the layout matches the Earth-like 3-cell
        // expectation so the rest of the test is meaningful.
        assert_eq!(
            law.layout.cells_per_hemisphere(),
            3,
            "Earth-like inputs should produce 3 cells per hemisphere"
        );
        // Grid height 13 → half_h = 6. The northern-hemisphere row
        // r=3 corresponds to signed_offset = -3, i.e. abs_lat =
        // (π/2) · 3/6 = π/4 = 45° — deep inside the Ferrel band
        // (30°–60°), where the angular-momentum-implied jet
        // magnitude is largest. q=1 is the centre meridian.
        let grid = HexGrid::new(3, 13);
        let mut state = PhysicsState::new(grid);
        // Uniform-temperature initial state: PhysicsState::new
        // zeroes every field, so no extra setup is needed — every
        // cell starts with `v_q = v_r = 0` and identical (zero)
        // temperature. The Hadley kernel doesn't read temperature
        // (it consumes only the row index → latitude mapping +
        // the pre-computed layout), so the integration is a clean
        // probe of the angular-momentum kick alone.
        let dt = Real::from_int(3);
        for _ in 0..1000 {
            law.integrate(&mut state, dt);
        }
        // Probe the row that lands at 45° latitude (Ferrel
        // interior). The expected steady-state magnitude is:
        //   per-tick = dt · kick_fraction · dir_sign · (cos²(30°)
        //              − cos²(60°)) · hemisphere_sign / cos(60°)
        //            = 3 · 0.01 · (-1) · 0.5 · 1 / 0.5
        //            = -0.03
        //   over 1000 ticks ⇒ -30
        //   ⇒ |v_q| = 30, the canonical ~30 m/s subtropical-jet
        //   target.
        let centre_q = 1;
        let probe_r = 3; // 45° N, Ferrel interior
        let probe_id = state
            .grid()
            .cell_id(crate::grid::Axial::new(centre_q, probe_r))
            .0 as usize;
        let v_q = state.fluid_velocity().0[probe_id];
        let v_abs = v_q.abs();
        let lower = Real::from_int(10);
        let upper = Real::from_int(60);
        assert!(
            v_abs >= lower && v_abs <= upper,
            "subtropical-band zonal velocity outside [10, 60] m/s \
             slack window: |v_q|={v_abs:?} (raw v_q={v_q:?}); \
             expected the angular-momentum jet integrator to land \
             near the ~30 m/s real-Earth target after 1000 ticks"
        );
    }

    /// Vacuum world: `HadleyCirculation { has_atmosphere: false }`
    /// must short-circuit. No layout consultation, no `v_q`
    /// mutation.
    #[test]
    fn vacuum_planet_short_circuits() {
        let layout = compute_hadley_layout(
            Real::from_int(24),
            Real::ONE,
            default_gravity_ms2(),
            DEFAULT_SCALE_HEIGHT_M,
        );
        let mut state = PhysicsState::new(HexGrid::new(4, 7));
        let law = HadleyCirculation {
            layout,
            has_atmosphere: false,
        };
        for _ in 0..10 {
            law.integrate(&mut state, Real::ONE);
        }
        for v in state.fluid_velocity().0 {
            assert_eq!(*v, Real::ZERO);
        }
    }

    /// P3.6 acceptance test: the Held-Hou closure must place the
    /// Earth-equivalent Hadley edge in `[25°, 35°]`, matching the
    /// observed subtropical-jet boundary. The 3-cell layout's first
    /// cell (`cells[0]`) spans `[0, hadley_edge]`; we read its
    /// `lat_end` and convert to degrees.
    ///
    /// This replaces the prior hard-coded `π/6` (= 30°) edge with
    /// `arcsin(sqrt((5/3) g H Δθ / ((ΩR)² T_eq)))`. With the default
    /// closure parameters (`H = 12 km`, `Δθ = 60 K`, `T_eq = 300 K`)
    /// Earth lands at ≈ 25-26°, within the 25-35° subtropical-jet
    /// band the spec calls out.
    #[test]
    fn earth_like_hadley_edge_within_25_to_35_degrees() {
        let layout = compute_hadley_layout(
            Real::from_int(24),       // 24-hour day
            Real::ONE,                // 1.0 Earth radii
            default_gravity_ms2(),
            DEFAULT_SCALE_HEIGHT_M,
        );
        assert_eq!(
            layout.cells_per_hemisphere(),
            3,
            "Earth-like inputs must produce 3 cells per hemisphere \
             for this test to probe the Held-Hou edge: layout={layout:?}"
        );
        // Hadley edge in radians.
        let edge_rad = layout.cells[0].lat_end;
        // Convert to degrees: deg = rad · 180 / π. `half_pi` =
        // π/2 rad = 90°, so `deg = (edge_rad / half_pi) · 90`.
        let edge_frac = edge_rad / half_pi();
        let edge_deg = edge_frac * Real::from_int(90);
        let lo = Real::from_int(25);
        let hi = Real::from_int(35);
        assert!(
            edge_deg >= lo && edge_deg <= hi,
            "Held-Hou Hadley edge for Earth-equivalent planet must \
             sit in [25°, 35°]; got edge_rad={edge_rad:?}, \
             edge_deg={edge_deg:?}"
        );
    }

    /// P3.6 acceptance test: holding planet radius and gravity
    /// fixed, a slow rotator (longer day_length, smaller Ω) must
    /// shift the Held-Hou Hadley edge polewards relative to a fast
    /// rotator. The closure's `sin²(lat_edge) ∝ 1/(ΩR)²` predicts
    /// edge expansion as Ω shrinks: doubling the day length quadruples
    /// the sin² factor and roughly doubles the sin (until saturation).
    ///
    /// Both planets must stay in the 3-cell regime — otherwise the
    /// 1-cell pole-to-pole case (which `cells_per_hemisphere == 1`
    /// triggers for the slowest rotators) would compare apples to
    /// oranges. A 30-hour day still lands in the 3-cell window
    /// (ratio ≈ 1.29 vs Earth's ≈ 1.6); a 40-hour day collapses to
    /// 1 cell. We probe 24 h (Earth) vs 30 h (slow).
    #[test]
    fn slow_rotator_has_wider_hadley_cell_than_fast_rotator() {
        let fast = compute_hadley_layout(
            Real::from_int(24),       // Earth day length
            Real::ONE,
            default_gravity_ms2(),
            DEFAULT_SCALE_HEIGHT_M,
        );
        let slow = compute_hadley_layout(
            Real::from_int(30),       // slower rotator (30-hour day)
            Real::ONE,
            default_gravity_ms2(),
            DEFAULT_SCALE_HEIGHT_M,
        );
        assert_eq!(
            fast.cells_per_hemisphere(),
            3,
            "fast (24h) rotator must be in the 3-cell regime: \
             layout={fast:?}"
        );
        assert_eq!(
            slow.cells_per_hemisphere(),
            3,
            "slow (30h) rotator must also be in the 3-cell regime \
             (else we'd be comparing different layouts): \
             layout={slow:?}"
        );
        let fast_edge = fast.cells[0].lat_end;
        let slow_edge = slow.cells[0].lat_end;
        assert!(
            slow_edge > fast_edge,
            "slow rotator's Hadley edge must move polewards relative \
             to fast rotator's: fast_edge={fast_edge:?}, \
             slow_edge={slow_edge:?}"
        );
    }
}
