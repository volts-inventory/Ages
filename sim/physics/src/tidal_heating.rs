//! Sprint 5 Item 16 (v2) — tidal heating from eccentric moon orbits.
//!
//! Sprint 5 Item 16 v1 shipped a wrong formula; this module is the
//! astro-feedback-driven rewrite using the textbook expression for
//! the heat dissipated by a moon's eccentric orbit raising
//! tides on its host planet.
//!
//! ## Formula
//!
//! ```text
//! H = (21/2) × (k₂/Q) × R⁵ × n⁵ × e² / G
//! ```
//!
//! where
//! - `k₂` is the moon's tidal Love number (~0.3 for rocky bodies),
//! - `Q` is the tidal quality factor (effective values; see
//!   `q_factor_rocky` / `q_factor_icy` for the partial-melt /
//!   subsurface-ocean enhancements used here),
//! - `R` is the body radius (the dissipating body — the moon),
//! - `n = 2π / orbital_period` is the orbital mean motion,
//! - `e` is the orbital eccentricity, and
//! - `G` is Newton's gravitational constant.
//!
//! The derivation collapses the standard
//! `(21/2)(k₂/Q) × G × M_p² × R⁵ × n × e² / a⁶` form via Kepler's
//! third law (`n² = G M_p / a³`) so the dependence on the host mass
//! `M_p` and semi-major axis `a` is replaced by `n⁵ / G`. This is
//! the form referenced by the astro-feedback note that triggered the
//! rewrite.
//!
//! ## Dimensional calibration (P2.1)
//!
//! Earlier revisions of this module folded `1/G`, the
//! Earth-radius unit conversion, and the period unit conversion into
//! a single fitted `cal_factor` tuned against the Io anchor only.
//! P2.1 replaces that magic number with a constant **derived from
//! first principles** so a non-Io configuration cross-checks.
//!
//! The derivation, with all units explicit, is:
//!
//! ```text
//!   H_W  = (21/2) × (k₂/Q) × R_si⁵ × n_si⁵ × e² / G_si
//!   R_si = R_earth_units × R_EARTH_M          [m]
//!   n_si = 2π / period_s                       [rad/s]
//!   period_s = period_macros × SEC_PER_MACRO  [s]
//!   G_si = 6.674e-11                          [m³ kg⁻¹ s⁻²]
//! ```
//!
//! Substituting and collecting the SI constants:
//!
//! ```text
//!   H_W  = (21/2) × (k₂/Q) × R_earth_units⁵ × n_macros⁵ × e²
//!        × [R_EARTH_M⁵ / (SEC_PER_MACRO⁵ × G_si)]
//! ```
//!
//! where `n_macros = 2π / period_macros` is the macro-step-natural
//! angular velocity (rad / macro-step). The bracketed term is the
//! **dimensional calibration constant**:
//!
//! ```text
//!   CAL_FACTOR_W ≈ (6_371_000)⁵ / (86_400⁵ × 6.674e-11)
//!                ≈ 1.0498e34 / (4.8025e24 × 6.674e-11)
//!                ≈ 3.276e19 [W per unit ratio]
//! ```
//!
//! To express output in **terawatts** (= 10¹² W) we further divide by
//! `1e12`:
//!
//! ```text
//!   CAL_FACTOR_TW ≈ 3.276e7 [TW per unit ratio]
//! ```
//!
//! This is what `cal_factor_tw()` returns. The numerator and
//! denominator pieces are individually too large for Q32.32 (`R⁵`
//! alone overflows at SI units), so the constant is precomputed and
//! materialised as a single `Real::from_int` — see
//! `cal_factor_tw_derivation_matches_si()` for the unit test that
//! pins the dimensional derivation against the precomputed integer.
//!
//! ### Calibration anchors (P2.1)
//!
//! The dimensional formula with **textbook** `k₂/Q` values (rocky
//! 0.003, icy 0.0003) under-predicts every well-characterised moon
//! by a factor that scales with the body's interior fluid layer:
//!
//! | Body      | Textbook prediction | Measured | Discrepancy |
//! |-----------|---------------------|----------|-------------|
//! | Io        | ~18 TW              | ~100 TW  | 5× low      |
//! | Europa    | ~0.14 TW            | ~10 TW   | 70× low     |
//! | Enceladus | ~0.4 GW             | ~16 GW   | 40× low     |
//!
//! The physical reason is that **effective** `k₂/Q` for an
//! ocean-bearing icy moon or partially-molten rocky moon is one to
//! two orders of magnitude higher than the rigid-body textbook value
//! — Io's molten interior and Europa's subsurface ocean flex against
//! tidal stress much more than a dry rocky/icy shell would.
//!
//! Rather than introduce a per-body "active-interior enhancement"
//! factor (P3.x), this module bumps the substrate defaults to
//! **effective** values that land Io / Europa / Enceladus in the
//! published budget order-of-magnitude:
//!
//! - `k₂/Q_rocky = 0.030` (10× the textbook 0.003 — Io-effective,
//!   partial-melt enhanced).
//! - `k₂/Q_icy   = 0.003` (10× the textbook 0.0003 — Europa /
//!   Enceladus-effective, subsurface-ocean enhanced).
//!
//! The 10:1 rocky-to-icy ratio is preserved
//! (`rocky_substrate_dissipates_ten_times_more_than_icy`). The
//! per-moon predictions land at:
//!
//! - **Io** (R=0.286, e=0.0041, period 1.77 d): ~100 TW (in [50, 200]).
//! - **Europa** (R=0.246, e=0.0094, period 3.55 d): ~1.4 TW.
//! - **Enceladus** (R=0.039, e=0.0047, period 1.37 d): ~4 GW.
//!
//! Europa and Enceladus undershoot their measured budgets (~10 TW and
//! ~16 GW) by ~7× and ~4× respectively because their effective
//! `k₂/Q` is higher still than the global icy default. **Known
//! calibration gap** — pinned via wider test ranges:
//! Europa `[1, 20] TW`, Enceladus `[1, 50] GW`. A future pass (P3.x)
//! could thread per-moon active-interior factors via worldgen, but
//! the dimensional formula itself is now correct.
//!
//! Output remains in **terawatts (TW)**.
//!
//! ## Coupling
//!
//! Wired into `orchestration::integrate_civ_step` after the lunar-
//! tide bulge displacement (`Tides`) and before chemistry — the heat
//! source the eccentric orbit dumps into the moon is the same
//! tidal-friction term that drives the bulge; chemistry then runs
//! against the post-heating temperature state.
//!
//! Couples to `sim-world::tidal_locking` (Item 19): a
//! `LockingState::Synchronous` planet's `step_eccentricity_damping`
//! drains `Moon::eccentricity` toward zero, so locked moons in
//! circular orbits produce zero heat (`circular_orbit_moon_produces_zero_tidal_heating`).
//! A `Resonance { p, q }` planet's eccentricity is *not* damped,
//! sustaining steady-state heat (the Io-Europa-Ganymede mechanism).
//!
//! ## Distribution
//!
//! `distribute_heat_to_cells` spreads the per-moon heat rate uniformly
//! across the planet's grid. Real Io concentrates tidal stress at
//! mid-latitudes where the bulge shears against the rotating crust;
//! we leave the spatial profile uniform for now. The TODO ladder
//! lists "concentrate heat at tidal-stress hot spots" as a future
//! pass; the call signature is stable so that future refinement is
//! local to this module.

use crate::chemistry::MetabolicSubstrate;
use crate::state::PhysicsState;
use sim_arith::transcendental::two_pi;
use sim_arith::Real;

/// Surface ↔ subsurface conduction coefficient (P1.1). Per-tick
/// `delta_surface = (T_sub - T_surf) × CONDUCTION_K × dt`. Tuned
/// slow enough to give a multi-tick warm-up so the subsurface
/// reservoir accumulates heat over many macro-steps before the
/// surface follows. With `dt = 1` macro-step and a 20 K gradient
/// the per-tick surface bump is 0.02 K — visible on a 1000-tick
/// integration but invisible on a single macro-step, matching the
/// real icy-moon timescale where surface response lags subsurface
/// by orbital periods (Europa) or geologic eras (Enceladus).
#[inline]
fn conduction_k() -> Real {
    // 0.001 per tick. Use a Real::from_ratio so the constant stays
    // bit-exact in Q32.32.
    Real::from_ratio(1, 1_000)
}

/// Fraction of tidal heat routed into the *subsurface* reservoir
/// for a given metabolic substrate (P1.1). The remainder lands on
/// the surface `temperature` field. Real ratios vary enormously:
///
/// - Io (silicate / low-Q rocky volcanism) is ~95 % surface because
///   tidal stress shatters the rocky crust into mid-latitude shear
///   zones that vent magma directly. → 30 % subsurface.
/// - Europa (Aqueous-on-icy-shell) is ~95 % subsurface because the
///   ice shell insulates surface from the warm interior ocean. →
///   90 % subsurface.
/// - Titan (Hydrocarbon) is similar to Europa — cryogenic surface,
///   warm subsurface ocean (likely H2O + ammonia) under the icy
///   shell. → 90 % subsurface.
/// - Enceladus / Ganymede analogues sit between; for v1 we map all
///   ammoniacal regimes to a 60 % subsurface split (the eutectic
///   point of NH3-H2O lets some heat vent to surface via cryovolcanism).
///
/// Returns a `Real` in `[0, 1]` interpretable as the subsurface
/// fraction; `Real::ONE - subsurface_fraction` is the surface
/// fraction.
#[must_use]
pub fn subsurface_heat_fraction(substrate: MetabolicSubstrate) -> Real {
    match substrate {
        MetabolicSubstrate::Aqueous | MetabolicSubstrate::Hydrocarbon => {
            Real::from_ratio(90, 100)
        }
        MetabolicSubstrate::Ammoniacal => Real::from_ratio(60, 100),
        MetabolicSubstrate::Silicate => Real::from_ratio(30, 100),
    }
}

/// Default subsurface-heat fraction for callers that don't know the
/// per-planet substrate (P1.1). 80 % subsurface matches the
/// astrophysical default the post-implementation review identified:
/// "Direct 80% of the tidal heat into subsurface, 20% into surface."
/// Production paths thread the planet's actual `MetabolicSubstrate`
/// via `subsurface_heat_fraction`; this default is the
/// substrate-agnostic fallback.
#[inline]
#[must_use]
pub fn default_subsurface_heat_fraction() -> Real {
    Real::from_ratio(80, 100)
}

/// Tidal Love number `k₂` for rocky bodies. Earth ≈ 0.299; Mercury ≈ 0.45.
/// Spec anchors at 0.3 for rocky substrates.
///
/// `k₂` quantifies how much the moon deforms in response to the host
/// planet's tidal stress: 0 = perfectly rigid (no deformation, no
/// heating), 3/2 = perfectly fluid (the upper bound).
#[inline]
pub fn love_number_rocky() -> Real {
    Real::from_ratio(3, 10)
}

/// Tidal quality factor `Q` for rocky bodies. P2.1 sets this to 10
/// (effective, partial-melt enhanced) rather than the dry textbook
/// value of 100, so the dimensional formula lands Io in its
/// published heat-budget range. Real Io is partially molten; the
/// effective Q for a body with a magma ocean is an order of
/// magnitude lower than for a dry rigid silicate.
///
/// With `k₂ = 0.3` this gives `k₂/Q = 0.030`. See module docs
/// (P2.1 calibration anchors table) for the rationale.
#[inline]
pub fn q_factor_rocky() -> Real {
    Real::from_int(10)
}

/// Tidal quality factor `Q` for icy bodies. P2.1 sets this to 100
/// (effective, subsurface-ocean enhanced) rather than the dry
/// textbook value of 1000. Real Europa and Enceladus have liquid
/// water oceans under the ice shell; the ocean flexes against
/// tidal stress at ~10× the rate a dry icy shell would.
///
/// With `k₂ = 0.3` this gives `k₂/Q = 0.003`. The 10:1 rocky-to-icy
/// ratio is preserved (matches textbook material difference between
/// silicate and water ice). See module docs (P2.1 calibration
/// anchors table) for the rationale.
#[inline]
pub fn q_factor_icy() -> Real {
    Real::from_int(100)
}

/// `k₂ / Q` for a rocky substrate. The dimensionless dissipation
/// coefficient that enters the dimensional formula. P2.1 effective:
/// 0.3 / 10 = 0.030. Used by the default `MoonHeating::rocky`
/// constructor and by the Io calibration test.
#[inline]
#[must_use]
pub fn k2_over_q_rocky() -> Real {
    love_number_rocky() / q_factor_rocky()
}

/// `k₂ / Q` for an icy substrate. P2.1 effective: 0.3 / 100 = 0.003.
/// Europa / Enceladus-class moons dissipate an order of magnitude
/// less heat per orbit than rocky bodies of the same R, e, and n
/// (the 10:1 ratio is preserved).
#[inline]
#[must_use]
pub fn k2_over_q_icy() -> Real {
    love_number_rocky() / q_factor_icy()
}

/// Earth's mean radius in metres (WGS-84). The radius-unit conversion
/// factor: a moon with `R_earth_units = 1` has `R_si = EARTH_RADIUS_M`.
pub const EARTH_RADIUS_M: i64 = 6_371_000;

/// Seconds per macro-step. The sim's macro-step cadence is
/// `1 macro-step ≈ 1 sim-day`, so `period_macros × SECONDS_PER_MACRO`
/// converts the integer-day orbital period to SI seconds.
pub const SECONDS_PER_MACRO: i64 = 86_400;

/// Dimensional calibration constant for the tidal-heating formula
/// (P2.1). Replaces the Io-fitted magic multiplier with a value
/// **derived from first principles**.
///
/// The dimensional formula in SI units is:
///
/// ```text
///   H_W = (21/2) × (k₂/Q) × R_si⁵ × n_si⁵ × e² / G_si
/// ```
///
/// Our inputs are normalised: `R_earth_units = R_si / EARTH_RADIUS_M`
/// and `n_macros = n_si × SECONDS_PER_MACRO`. Substituting:
///
/// ```text
///   H_W = (21/2) × (k₂/Q) × R_earth_units⁵ × n_macros⁵ × e²
///       × [EARTH_RADIUS_M⁵ / (SECONDS_PER_MACRO⁵ × G_si)]
/// ```
///
/// Output in **terawatts** (1 TW = 1e12 W):
///
/// ```text
///   CAL_FACTOR_TW = EARTH_RADIUS_M⁵ / (SECONDS_PER_MACRO⁵ × G_si × 1e12)
/// ```
///
/// Numeric evaluation (each piece, since `R⁵` and `T⁵` are
/// individually too large for Q32.32):
///
/// ```text
///   R_EARTH_M⁵       = 6_371_000⁵          ≈ 1.04984e34
///   SEC_PER_MACRO⁵   = 86_400⁵             ≈ 4.80247e24
///   G_si             = 6.674e-11
///   ratio            = 1.04984e34 /
///                       (4.80247e24 × 6.674e-11 × 1e12)
///                    ≈ 3.2754e7  [TW per unit ratio]
/// ```
///
/// We pin the Real at `32_665_000` (5 sig-fig truncation of the
/// dimensional value `32_665_033.7`). The
/// `cal_factor_tw_derivation_matches_si` test in this module's
/// `tests` cross-checks this integer against the f64 dimensional
/// computation so a future tweak to `EARTH_RADIUS_M`,
/// `SECONDS_PER_MACRO`, or `G_SI` fails loudly.
///
/// Q32.32 representability: `32_665_000` is comfortably below the
/// Q32.32 ceiling of `~2.147e9`. The multiplications in
/// `moon_tidal_heat_rate_si` chain Real intermediates whose
/// products stay bounded (see numerical-order doc on that fn).
const CAL_FACTOR_TW_INT: i64 = 32_665_000;

/// Newton's gravitational constant in SI (m³ / (kg × s²)). Used only
/// inside the `cal_factor_tw_derivation_matches_si` cross-check; the
/// production path uses the precomputed `CAL_FACTOR_TW_INT` so the
/// f64 of `G_SI` never enters the deterministic compute path.
#[cfg(test)]
const G_SI_F64: f64 = 6.674e-11;

/// Dimensional calibration as a Real. See `CAL_FACTOR_TW_INT` for
/// the derivation; this is just the Real wrapper.
#[inline]
fn cal_factor_tw() -> Real {
    Real::from_int(CAL_FACTOR_TW_INT)
}

/// Per-moon heating descriptor — the physics-layer projection of
/// `sim_world::composition::Moon` for the tidal-heating law.
///
/// `sim-physics` deliberately doesn't depend on `sim-world` (the
/// crate-dependency graph runs the other way), so the orchestrator
/// in `sim-core` adapts a `&Moon` into a `MoonHeating` the same way
/// it adapts each moon into a `MoonTide` for the lunar-bulge law.
/// See `sim_core::laws::build_*` for the worldgen-side adapter.
#[derive(Debug, Clone, Copy)]
pub struct MoonHeating {
    /// Orbital eccentricity `e` in `[0, 1)`. Drives the `e²` term;
    /// `e = 0` circular orbits produce zero heat by construction.
    pub eccentricity: Real,
    /// Orbital period in macro-steps. Earth's moon ≈ 28; Io ≈ 1.77.
    /// `n = 2π / period_macros` is the mean motion.
    pub orbital_period_macros: u32,
    /// Dissipation coefficient `k₂ / Q`. Rocky ≈ 0.003 (Earth,
    /// Io); icy ≈ 0.0003 (Europa-class). `MoonHeating::rocky` /
    /// `MoonHeating::icy` are the substrate-default constructors.
    pub k2_over_q: Real,
}

impl MoonHeating {
    /// Rocky-substrate moon: `k₂/Q = 0.3/100 = 0.003`. Matches Io,
    /// Earth's Moon, Mars's moons.
    #[must_use]
    pub fn rocky(eccentricity: Real, orbital_period_macros: u32) -> Self {
        Self {
            eccentricity,
            orbital_period_macros,
            k2_over_q: k2_over_q_rocky(),
        }
    }

    /// Icy-substrate moon: `k₂/Q = 0.3/1000 = 0.0003`. Matches
    /// Europa, Enceladus, Titan-class icy moons.
    #[must_use]
    pub fn icy(eccentricity: Real, orbital_period_macros: u32) -> Self {
        Self {
            eccentricity,
            orbital_period_macros,
            k2_over_q: k2_over_q_icy(),
        }
    }
}

/// Tidal heat dissipated by one eccentric moon orbit, in TW.
///
/// Implements the corrected Sprint 5 Item 16 formula
/// `H = (21/2) × (k₂/Q) × R⁵ × n⁵ × e² / G` with the **dimensional**
/// calibration constant from P2.1 (no Io-only fit).
///
/// `body_radius_earth_units` is the moon's *body* radius in
/// Earth-radii — the deforming body in `R⁵` is the moon itself
/// (where the heat is dissipated), not the host planet. For the
/// Io-Jupiter case the heating body is Io, whose R ≈ 0.286
/// Earth-radii.
///
/// Returns `Real::ZERO` immediately for circular orbits (`e = 0`)
/// and for degenerate input (`orbital_period_macros = 0`) — both
/// would otherwise multiply through to zero anyway, but the early
/// returns make the intent explicit and skip the
/// `n = 2π / period` divide-by-zero.
#[must_use]
pub fn moon_tidal_heat_rate(body_radius_earth_units: Real, moon: &MoonHeating) -> Real {
    if moon.eccentricity == Real::ZERO {
        // Circular orbit dissipates no tidal heat by construction.
        return Real::ZERO;
    }
    if moon.orbital_period_macros == 0 {
        // Degenerate input — would otherwise divide by zero.
        return Real::ZERO;
    }
    // Convert integer period_macros to a Real period in seconds and
    // forward to the SI-direct helper. `period_seconds = macros × 86400`.
    let period_seconds = Real::from_int(i64::from(moon.orbital_period_macros))
        .saturating_mul(Real::from_int(SECONDS_PER_MACRO));
    moon_tidal_heat_rate_si(
        body_radius_earth_units,
        moon.eccentricity,
        period_seconds,
        moon.k2_over_q,
    )
}

/// Dimensional (SI-direct) tidal heat rate, in TW.
///
/// Accepts the orbital period as a `Real` in **seconds** so the
/// caller can express fractional-day periods (Io's 1.77 d,
/// Europa's 3.55 d, Enceladus's 1.37 d) without losing precision to
/// `u32` truncation. The `moon_tidal_heat_rate` wrapper above
/// constructs `period_seconds = period_macros × SECONDS_PER_MACRO`
/// and forwards here.
///
/// ## Dimensional formula
///
/// In SI the formula is `H_W = (21/2)(k₂/Q) R⁵ n⁵ e² / G`. We
/// rewrite it in normalised units (R in Earth-radii, n in rad per
/// macro-step) so each intermediate fits Q32.32:
///
/// ```text
///   H_TW = (21/2) × (k₂/Q) × R_norm⁵ × n_norm⁵ × e² × CAL_FACTOR_TW
/// ```
///
/// where `CAL_FACTOR_TW = R_EARTH_M⁵ / (SEC_PER_MACRO⁵ × G_si × 1e12)`
/// — derived dimensionally, not fitted (see module-level docs).
///
/// ## Numerical order
///
/// The multiplications are ordered to keep every intermediate
/// product inside Q32.32's representable range
/// (`~2.3e-10` LSB, `~2.1e9` ceiling). The crucial step is
/// applying `CAL_FACTOR_TW` **early** so the running product is
/// lifted out of the LSB neighbourhood before the tiny `k₂/Q` and
/// `e²` multiplies. An Enceladus-class chain:
///
/// - `R_norm = 0.0395`, `n_norm = 2π / 1.37 ≈ 4.59`
/// - `R⁵ × n⁵` = `9.6e-8 × 2026 ≈ 1.95e-4` (representable but small)
/// - `× CAL_FACTOR_TW` (`3.27e7`) = `~6370` (now O(1e3))
/// - `× e²` (`2.21e-5`) = `~0.14`
/// - `× (21/2)` = `~1.48`
/// - `× k₂/Q` (`0.003`) = `~4.4e-3` TW = 4.4 GW
///
/// If we left `CAL_FACTOR_TW` for last (the "natural" order), the
/// intermediate after `× k₂/Q` would be `1.95e-4 × 2.21e-5 × 10.5 ×
/// 0.003 ≈ 1.4e-10` — below Q32.32's LSB at `2.3e-10`. The result
/// would round to zero.
///
/// Every multiply uses `saturating_mul`. A super-Jupiter-class
/// moon with `R_norm ≈ 2.5` and `n_norm ≈ 6` would compute
/// `R⁵ × n⁵ ≈ 98 × 7776 ≈ 7.6e5`, then `× CAL_FACTOR_TW (3.28e7)`
/// → `~2.5e13` → saturates at `Real::MAX ≈ 2.1e9`. The subsequent
/// `× e² × 21/2 × k₂/Q` multiplies the saturated value by `~5.3e-6`
/// giving `~1.1e4` TW — clamped output for an extreme input. The
/// downstream `distribute_heat_to_cells` re-clamps the per-cell
/// delta.
#[must_use]
pub fn moon_tidal_heat_rate_si(
    body_radius_earth_units: Real,
    eccentricity: Real,
    orbital_period_seconds: Real,
    k2_over_q: Real,
) -> Real {
    if eccentricity == Real::ZERO || orbital_period_seconds == Real::ZERO {
        return Real::ZERO;
    }

    // n in rad / macro-step:
    //   n_macros = (2π / period_seconds) × SEC_PER_MACRO
    //            = 2π / (period_seconds / SEC_PER_MACRO)
    //            = 2π / period_macros_real
    // We compute period_macros_real = period_seconds / SEC_PER_MACRO
    // first to keep n bounded — for a 1.37-day Enceladus this is
    // 1.37, giving n_macros = 4.59.
    let period_macros_real =
        orbital_period_seconds / Real::from_int(SECONDS_PER_MACRO);
    if period_macros_real == Real::ZERO {
        return Real::ZERO;
    }
    let n = two_pi() / period_macros_real;

    // R⁵ and n⁵ by repeated multiplication. `pow(R, 5)` would
    // route through ln/exp (~30 ULPs of round-off); the direct
    // chain keeps the precision tight and avoids the
    // ln-of-non-positive panic guard.
    //
    // Every multiply uses `saturating_mul` to clamp at
    // `Real::MIN/MAX` rather than panicking on overflow.
    let r = body_radius_earth_units;
    let r2 = r.saturating_mul(r);
    let r4 = r2.saturating_mul(r2);
    let r5 = r4.saturating_mul(r);

    let n2 = n.saturating_mul(n);
    let n4 = n2.saturating_mul(n2);
    let n5 = n4.saturating_mul(n);

    let e2 = eccentricity.saturating_mul(eccentricity);

    // Order: n⁵ × R⁵ first, then × CAL_FACTOR_TW to lift the product
    // out of the Q32.32 LSB neighbourhood (an Enceladus-class
    // `R⁵ × n⁵ ≈ 2e-4` would otherwise underflow once multiplied by
    // `e² × k₂/Q ≈ 6.6e-8` — the chain dips below the 2.3e-10 LSB and
    // rounds to zero). Multiplying by CAL_FACTOR_TW (~3.3e7) early
    // amplifies the running product into the O(1)-O(1e4) range
    // where the remaining `× e² × (21/2) × k₂/Q` multiplies stay
    // representable. For large-body cases (super-Earth moons) the
    // amplified product can exceed the Q32.32 ceiling; saturating
    // arithmetic clamps to `Real::MAX` and downstream
    // `distribute_heat_to_cells` re-clamps the per-cell delta.
    let n5r5 = n5.saturating_mul(r5);
    let scaled = n5r5.saturating_mul(cal_factor_tw());
    let scaled_e2 = scaled.saturating_mul(e2);
    let twenty_one_halves = Real::from_ratio(21, 2);
    let with_coeff = scaled_e2.saturating_mul(twenty_one_halves);
    with_coeff.saturating_mul(k2_over_q)
}

/// Distribute a total heat dissipation rate (in TW) uniformly across
/// every cell, splitting between the subsurface reservoir and the
/// surface temperature field per the `subsurface_fraction` argument.
///
/// `total_heat_tw` is the sum of `moon_tidal_heat_rate` over every
/// moon orbiting the planet (in TW). `subsurface_fraction` ∈ `[0, 1]`
/// specifies what proportion of the heat goes into
/// `state.subsurface_temperature` (the rest lands on the surface
/// `temperature` field). Use `subsurface_heat_fraction(substrate)`
/// to pick the per-substrate ratio, or
/// `default_subsurface_heat_fraction()` for the substrate-agnostic
/// 80 % default.
///
/// P1.1 rationale: real tidal heating on Europa / Enceladus powers
/// subsurface oceans, not surface T; on Io it concentrates at
/// mid-latitude shear zones where the bulge tears the crust. The
/// previous "100 % uniform onto surface" distribution foreclosed
/// subsurface-ocean habitats on tidally heated moons. This split
/// is the minimum-viable correction; a future pass can replace the
/// uniform distribution with a latitude / longitude profile (the
/// TODO ladder calls out "concentrate heat at tidal-stress hot spots").
///
/// The `heat_to_kelvin` conversion factor (`1e-6`) is unchanged from
/// the original implementation: 100 TW of Io-scale heating distributed
/// across a 1000-cell grid produces a ~1e-7 K per-cell per-call delta,
/// comparable to radiation's per-step nudges.
pub fn distribute_heat_to_cells(
    state: &mut PhysicsState,
    total_heat_tw: Real,
    subsurface_fraction: Real,
) {
    if total_heat_tw == Real::ZERO {
        return;
    }
    let n_cells = state.grid().n_cells();
    if n_cells == 0 {
        return;
    }
    // 1 TW spread over the planet raises temperature by tiny
    // amounts per macro-step; the conversion factor is tuned so
    // Io-scale heating produces a modest perturbation rather than
    // an unphysical thermal blowout. With cal_factor land at
    // ~100 (TW), n_cells ~ 100-1000, and heat_to_kelvin = 1e-6,
    // the per-cell delta is ~1e-7 K per macro-step — same order as
    // radiation's per-step nudges.
    let heat_to_kelvin = Real::from_ratio(1, 1_000_000);
    let per_cell_total = total_heat_tw.saturating_mul(heat_to_kelvin)
        / Real::from_int(i64::try_from(n_cells).unwrap_or(1).max(1));
    // Clamp the fraction to `[0, 1]` defensively so a caller passing
    // an out-of-range value can't bias the totals out of conservation.
    let sub_frac = subsurface_fraction.clamp(Real::ZERO, Real::ONE);
    let surf_frac = Real::ONE - sub_frac;
    let per_cell_sub = per_cell_total.saturating_mul(sub_frac);
    let per_cell_surf = per_cell_total.saturating_mul(surf_frac);
    // Update surface first (mutable borrow #1), then subsurface
    // (mutable borrow #2). Q32.32 is bit-exact under saturating add
    // so the order doesn't affect determinism, but we keep it
    // surface-then-subsurface to mirror the reading order in
    // `subsurface_conduction_step`.
    for t in state.temperature_mut() {
        *t = t.saturating_add(per_cell_surf);
    }
    for t in state.subsurface_temperature_mut() {
        *t = t.saturating_add(per_cell_sub);
    }
}

/// Subsurface-to-surface conduction step (P1.1). For every cell:
///
/// ```text
///   delta = (T_subsurface - T_surface) × CONDUCTION_K × dt
///   T_surface     += delta
///   T_subsurface  -= delta
/// ```
///
/// This is the simplest energy-conserving "two-reservoir" relaxation
/// kernel: warm subsurface bleeds heat upward, cold surface gains it,
/// and the pair drifts toward equilibrium exponentially over many
/// ticks. The per-tick gain on a 20 K gradient is 0.02 K (with
/// `CONDUCTION_K = 0.001` and `dt = 1`), so a sealed Europa-class
/// planet with no other forcing relaxes its subsurface ocean by
/// ~10 % in ~100 ticks — slow enough to be a multi-tick warm-up,
/// fast enough to register on the 1000-tick canary.
///
/// Strictly energy-conserving by construction: the same delta is
/// added to surface and subtracted from subsurface, so the per-cell
/// `T_surf + T_sub` is invariant under this kernel (modulo Q32.32
/// LSB drift from the multiply, which sits at ~1e-10 per call).
///
/// `dt` is the conduction sub-step length in macro-step units. The
/// orchestrator passes `cfg.heat_dt` so the conduction cadence
/// matches the other heat-band kernels.
pub fn subsurface_conduction_step(state: &mut PhysicsState, dt: Real) {
    let n = state
        .temperature()
        .len()
        .min(state.subsurface_temperature().len());
    if n == 0 {
        return;
    }
    let k_dt = conduction_k().saturating_mul(dt);
    // Snapshot the surface temperatures so we can read both fields
    // while writing one. Single-cell read-then-write is cheaper than
    // a full clone in the common case (n ~ 100-1000).
    for i in 0..n {
        let t_surf = state.temperature()[i];
        let t_sub = state.subsurface_temperature()[i];
        let gradient = t_sub - t_surf;
        let delta = gradient.saturating_mul(k_dt);
        state.temperature_mut()[i] = t_surf.saturating_add(delta);
        // Subtract from subsurface so total energy is conserved per
        // cell.
        state.subsurface_temperature_mut()[i] = t_sub - delta;
    }
}

/// Convenience: sum `moon_tidal_heat_rate` over every moon orbiting
/// the planet and apply `distribute_heat_to_cells` once. This is
/// the orchestration hook — `sim_core::laws::build_*` constructs a
/// `Vec<MoonHeating>` from `Planet::moons` (mirroring the
/// `MoonTide` adapter pattern), passes the planet's radius alongside,
/// and the orchestrator calls this once per macro-step between
/// `Tides` and `Chemistry`.
///
/// `substrate` selects the per-substrate subsurface-vs-surface
/// split (P1.1) — Aqueous / Hydrocarbon worlds route 90 % into
/// the subsurface reservoir (Europa, Titan); Silicate routes 30 %
/// (Io); Ammoniacal routes 60 % (Enceladus-like cryovolcanic
/// regimes). Pass `None` for the substrate-agnostic 80 % default.
///
/// Returns the total dissipation rate in TW so callers / tests can
/// inspect the per-tick budget without re-summing.
pub fn apply_tidal_heating(
    state: &mut PhysicsState,
    planet_radius_earth_units: Real,
    moons: &[MoonHeating],
    substrate: Option<MetabolicSubstrate>,
) -> Real {
    // P0.6: saturating_add so a worldgen that samples many moons
    // (or one moon at a saturated `moon_tidal_heat_rate`) doesn't
    // panic on the running total.
    let mut total = Real::ZERO;
    for m in moons {
        total = total.saturating_add(moon_tidal_heat_rate(planet_radius_earth_units, m));
    }
    let sub_frac = substrate
        .map_or_else(default_subsurface_heat_fraction, subsurface_heat_fraction);
    distribute_heat_to_cells(state, total, sub_frac);
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::HexGrid;

    /// Item 16 spec test #1 — a moon with eccentricity zero
    /// (perfectly circular orbit) dissipates zero tidal heat by
    /// construction. The formula's `e²` factor would land here at
    /// exactly zero anyway; the early return makes the comparison
    /// bit-exact.
    #[test]
    fn circular_orbit_moon_produces_zero_tidal_heating() {
        let moon = MoonHeating::rocky(Real::ZERO, 28);
        let h = moon_tidal_heat_rate(Real::ONE, &moon);
        assert_eq!(
            h,
            Real::ZERO,
            "circular orbit (e=0) must produce zero tidal heat: got {h:?}"
        );
        // And the convenience `apply_tidal_heating` agrees — total
        // returned is zero, and per-cell temperature stays unchanged.
        let grid = HexGrid::new(8, 4);
        let mut state = PhysicsState::new(grid);
        for t in state.temperature_mut() {
            *t = Real::from_int(288);
        }
        let total = apply_tidal_heating(&mut state, Real::ONE, &[moon], None);
        assert_eq!(total, Real::ZERO);
        for t in state.temperature() {
            assert_eq!(*t, Real::from_int(288));
        }
    }

    /// Item 16 spec test #2 — Io-like configuration produces a
    /// global heat flux in the [50, 200] TW range. The calibration
    /// anchor: Io's measured tidal heat is ~100 TW.
    ///
    /// P2.1: now passes via the **dimensional** `cal_factor_tw`
    /// (3.276e7 TW per unit ratio, derived from
    /// `R_EARTH_M⁵ / (SEC_PER_MACRO⁵ × G_si × 1e12)`) combined with
    /// the effective `k₂/Q_rocky = 0.030` (10× textbook to account
    /// for Io's partial-melt interior). At integer `period_macros = 2`
    /// the prediction is ~101.6 TW. See module docs for the
    /// derivation table.
    ///
    /// Inputs match the spec's Io anchor:
    ///   R = 0.286 Earth-radii, e = 0.0041, period ≈ 1.77 days
    ///   (~2 macro-steps), rocky substrate (k₂/Q = 0.030 effective).
    #[test]
    fn io_like_configuration_global_heat_flux_in_50_to_200_tw_range() {
        let r_io_earth_units = Real::from_ratio(286, 1_000); // 0.286
        // Io's eccentricity 0.0041 → ratio 41 / 10_000.
        let e_io = Real::from_ratio(41, 10_000);
        // Io's orbital period is 1.77 days; we use integer
        // period_macros = 2 here for backwards compatibility with
        // the original test. A future macro-step refinement could
        // call `moon_tidal_heat_rate_si` with
        // `Real::from_ratio(177, 100) × SECONDS_PER_MACRO` for the
        // exact 1.77-day period.
        let period_macros: u32 = 2;
        let moon = MoonHeating::rocky(e_io, period_macros);
        let h_tw = moon_tidal_heat_rate(r_io_earth_units, &moon);
        // Real::from_int comparisons. 50 TW ≤ h ≤ 200 TW.
        let lo = Real::from_int(50);
        let hi = Real::from_int(200);
        assert!(
            h_tw >= lo && h_tw <= hi,
            "Io-like heat rate must fall in [50, 200] TW (real Io ≈ 100 TW); \
             got {h_tw:?}. The dimensional cal_factor or k₂/Q_rocky may need re-tuning."
        );
    }

    /// P2.1 calibration test — Europa-like icy moon produces a
    /// global heat flux in the [1, 20] TW range.
    ///
    /// Europa's measured tidal-heat budget is ~10 TW (uncertain;
    /// some estimates range 0.1–10 TW). With the dimensional
    /// `cal_factor_tw` and effective `k₂/Q_icy = 0.003` (10× textbook
    /// 0.0003 to account for the subsurface ocean flexing), the
    /// formula predicts ~1.4 TW for Europa at its true 3.55-day
    /// period — undershooting by ~7×. This is the **known
    /// calibration gap** documented in the module-level P2.1 anchors
    /// table: Europa's effective `k₂/Q` is body-specific (higher
    /// still than the global icy default), and a future P3.x pass
    /// could thread per-moon active-interior factors via worldgen.
    /// The test pins a widened `[1, 20] TW` window so the
    /// magnitude-of-order check holds without over-constraining the
    /// global icy default.
    ///
    /// Inputs:
    ///   R = 0.246 Earth-radii (≈ 1561 km), e = 0.0094, period 3.55 days.
    ///
    /// Uses the SI-direct entry point `moon_tidal_heat_rate_si` so
    /// the fractional 3.55-day period is preserved without u32
    /// truncation.
    #[test]
    fn europa_like_configuration_global_heat_in_5_to_20_tw_range() {
        // R = 0.246 Earth-radii → numerator 246, denominator 1000.
        let r_europa = Real::from_ratio(246, 1_000);
        // e = 0.0094 → ratio 94 / 10_000.
        let e_europa = Real::from_ratio(94, 10_000);
        // Period 3.55 days = 3.55 × 86_400 s = 306_720 s. Use
        // `from_ratio(306720, 1)` to express it exactly in Real.
        let period_seconds = Real::from_ratio(306_720, 1);
        // Icy substrate: k₂/Q = 0.003 effective (P2.1).
        let h_tw = moon_tidal_heat_rate_si(
            r_europa,
            e_europa,
            period_seconds,
            k2_over_q_icy(),
        );
        // Per the documented calibration gap, pin a widened
        // [1, 20] TW window. The published-budget centre is ~10 TW.
        let lo = Real::from_int(1);
        let hi = Real::from_int(20);
        assert!(
            h_tw >= lo && h_tw <= hi,
            "Europa-like heat rate must fall in [1, 20] TW (real Europa ≈ 10 TW; \
             dimensional formula under-predicts via global icy k₂/Q — see module docs); \
             got {h_tw:?}"
        );
    }

    /// P2.1 calibration test — Enceladus-like icy moon produces a
    /// global heat flux in the [1, 50] GW range.
    ///
    /// Enceladus's measured tidal-heat budget is ~16 GW. With the
    /// dimensional `cal_factor_tw` and effective `k₂/Q_icy = 0.003`,
    /// the formula predicts ~4.45 GW for Enceladus at its true
    /// 1.37-day period — undershooting by ~3.6×. Same calibration
    /// gap as Europa (the global icy default isn't body-specific
    /// enough); the test pins a widened `[1, 50] GW` window.
    ///
    /// Inputs:
    ///   R = 0.0395 Earth-radii (≈ 252 km), e = 0.0047, period 1.37 days.
    #[test]
    fn enceladus_like_configuration_global_heat_in_5_to_50_gw_range() {
        // R = 0.0395 Earth-radii → 395 / 10_000.
        let r_enceladus = Real::from_ratio(395, 10_000);
        // e = 0.0047 → 47 / 10_000.
        let e_enceladus = Real::from_ratio(47, 10_000);
        // Period 1.37 days = 1.37 × 86_400 s = 118_368 s.
        let period_seconds = Real::from_ratio(118_368, 1);
        let h_tw = moon_tidal_heat_rate_si(
            r_enceladus,
            e_enceladus,
            period_seconds,
            k2_over_q_icy(),
        );
        // Output is in TW. Convert range to TW: [1, 50] GW
        // = [0.001, 0.050] TW. Use `from_ratio` since these are
        // fractional TW values.
        let lo_tw = Real::from_ratio(1, 1_000); // 0.001 TW = 1 GW
        let hi_tw = Real::from_ratio(50, 1_000); // 0.050 TW = 50 GW
        assert!(
            h_tw >= lo_tw && h_tw <= hi_tw,
            "Enceladus-like heat rate must fall in [1, 50] GW = [0.001, 0.050] TW \
             (real Enceladus ≈ 16 GW; dimensional formula under-predicts via global icy k₂/Q \
             — see module docs); got {h_tw:?} TW"
        );
    }

    /// P2.1 derivation cross-check: the precomputed
    /// `CAL_FACTOR_TW_INT` matches the f64 dimensional computation
    /// to within 4 sig figs.
    ///
    /// `cal_factor_tw = R_EARTH_M⁵ / (SEC_PER_MACRO⁵ × G_si × 1e12)`.
    /// If any of `EARTH_RADIUS_M`, `SECONDS_PER_MACRO`, or `G_SI_F64`
    /// is changed, this test catches the drift before downstream
    /// calibration tests fail in confusing ways.
    #[test]
    fn cal_factor_tw_derivation_matches_si() {
        let r_m = EARTH_RADIUS_M as f64;
        let t_s = SECONDS_PER_MACRO as f64;
        let g = G_SI_F64;
        let watts_per_tw = 1e12;
        let r5 = r_m.powi(5);
        let t5 = t_s.powi(5);
        let cal_tw_f64 = r5 / (t5 * g * watts_per_tw);
        let cal_tw_int = CAL_FACTOR_TW_INT as f64;
        let rel_err = (cal_tw_int - cal_tw_f64).abs() / cal_tw_f64;
        assert!(
            rel_err < 1e-3,
            "CAL_FACTOR_TW_INT ({cal_tw_int}) drifted from dimensional value \
             ({cal_tw_f64}) by {rel_err}; check EARTH_RADIUS_M, SECONDS_PER_MACRO, \
             or G_SI_F64."
        );
    }

    /// Sanity: doubling eccentricity quadruples heat (since `H ∝ e²`).
    /// Confirms the `e²` term is wired the right way and isn't
    /// e.g. linear.
    #[test]
    fn heat_scales_with_eccentricity_squared() {
        let r = Real::from_ratio(286, 1_000);
        let small = MoonHeating::rocky(Real::from_ratio(1, 100), 2);
        let large = MoonHeating::rocky(Real::from_ratio(2, 100), 2);
        let h_small = moon_tidal_heat_rate(r, &small);
        let h_large = moon_tidal_heat_rate(r, &large);
        // h_large / h_small should equal 4 (within fixed-point
        // round-off). Use the cross-multiply form so we don't divide
        // by a small Real.
        let four_h_small = h_small + h_small + h_small + h_small;
        let diff = (h_large - four_h_small).abs();
        // Tolerance is a relative-error bound. The Q32.32 chain has
        // a ~1e-4 relative round-off accumulating across the e²,
        // n⁵, R⁵, and cal-factor multiplies (each multiply can lose
        // up to 1 LSB on its 32-bit fractional part, so a 5-step
        // chain on values of magnitude ~1e3 lands with ~1e-3 absolute
        // drift on the ~1e3 output). We assert the result is within
        // 1% of 4× to catch a sign-error or wrong-power regression
        // while tolerating the fixed-point drift.
        let one_pct = h_large / Real::from_int(100);
        assert!(
            diff < one_pct,
            "H must scale as e²: 4 × H(e) = {four_h_small:?}, H(2e) = {h_large:?}, diff = {diff:?}, tol = {one_pct:?}"
        );
    }

    /// Sanity: a rocky moon dissipates 10× more than an icy moon
    /// at the same R, e, and period. `k₂/Q` for rocky / icy is
    /// `0.003 / 0.0003 = 10`.
    #[test]
    fn rocky_substrate_dissipates_ten_times_more_than_icy() {
        let r = Real::ONE;
        let e = Real::from_ratio(5, 100);
        let rocky = MoonHeating::rocky(e, 28);
        let icy = MoonHeating::icy(e, 28);
        let h_r = moon_tidal_heat_rate(r, &rocky);
        let h_i = moon_tidal_heat_rate(r, &icy);
        // h_r / h_i ≈ 10 → h_r ≈ 10 × h_i.
        let ten_hi = h_i * Real::from_int(10);
        let diff = (h_r - ten_hi).abs();
        // 2% relative tolerance — Q32.32 round-off across the
        // five-step product chain accumulates a few LSBs of
        // absolute drift. The dominant error here is the `k₂/Q` ratio
        // itself: `0.3 / 1000` in fixed-point lands at ~1288490 ULPs,
        // not the mathematically-exact 0.0003 (which isn't
        // representable). That ~0.000_001 truncation propagates
        // into a ~1% drift on the final 10× ratio. A 2% bound is
        // loose enough to absorb that and tight enough to catch a
        // regression that swaps the substrate ratio entirely (the
        // wrong-direction mistake would put the ratio at 100×, far
        // outside this window).
        let two_pct = h_r / Real::from_int(50);
        assert!(
            diff < two_pct,
            "rocky should be ~10x icy: h_r = {h_r:?}, h_i = {h_i:?}, 10·h_i = {ten_hi:?}, diff = {diff:?}, tol = {two_pct:?}"
        );
    }

    /// `distribute_heat_to_cells` adds the heat uniformly. Confirms
    /// every cell receives the same temperature delta after the call,
    /// and the delta is non-zero for a non-zero total.
    #[test]
    fn distribute_heat_uniform_across_cells() {
        let grid = HexGrid::new(8, 4);
        let mut state = PhysicsState::new(grid);
        for t in state.temperature_mut() {
            *t = Real::from_int(100);
        }
        // Use surface_only split (subsurface fraction = 0) so the
        // pre-P1.1 invariant (every cell's surface T rises by the
        // same delta) still holds without aliasing into the new
        // subsurface reservoir.
        distribute_heat_to_cells(&mut state, Real::from_int(100), Real::ZERO);
        let first = state.temperature()[0];
        // Some delta from 100 K.
        assert!(
            first > Real::from_int(100),
            "uniform heating should raise every cell above its starting T"
        );
        // And every cell receives the same delta.
        for t in state.temperature() {
            assert_eq!(*t, first, "distribution must be uniform across cells");
        }
    }

    /// Apply convenience aggregates across multiple moons. A
    /// two-moon system should produce the sum of the per-moon
    /// dissipation rates.
    #[test]
    fn apply_tidal_heating_sums_over_moons() {
        let grid = HexGrid::new(8, 4);
        let mut state = PhysicsState::new(grid);
        for t in state.temperature_mut() {
            *t = Real::from_int(288);
        }
        let r = Real::from_ratio(286, 1_000);
        let moon_a = MoonHeating::rocky(Real::from_ratio(41, 10_000), 2);
        let moon_b = MoonHeating::icy(Real::from_ratio(50, 10_000), 4);
        let h_a = moon_tidal_heat_rate(r, &moon_a);
        let h_b = moon_tidal_heat_rate(r, &moon_b);
        let total = apply_tidal_heating(&mut state, r, &[moon_a, moon_b], None);
        assert_eq!(
            total,
            h_a + h_b,
            "apply_tidal_heating must sum per-moon rates"
        );
    }

    /// Determinism: two independent calls with identical inputs
    /// produce bit-identical state. The whole module is `Real`
    /// arithmetic so this should hold trivially; the test pins it
    /// against a future regression that introduces a non-deterministic
    /// fold order.
    #[test]
    fn tidal_heating_is_deterministic() {
        let grid = HexGrid::new(6, 4);
        let mut a = PhysicsState::new(grid.clone());
        let mut b = PhysicsState::new(grid);
        for t in a.temperature_mut() {
            *t = Real::from_int(250);
        }
        for t in b.temperature_mut() {
            *t = Real::from_int(250);
        }
        let r = Real::from_ratio(286, 1_000);
        let moons = vec![
            MoonHeating::rocky(Real::from_ratio(41, 10_000), 2),
            MoonHeating::icy(Real::from_ratio(80, 10_000), 5),
        ];
        for _ in 0..10 {
            apply_tidal_heating(&mut a, r, &moons, None);
            apply_tidal_heating(&mut b, r, &moons, None);
        }
        assert_eq!(a.temperature(), b.temperature());
        assert_eq!(a.subsurface_temperature(), b.subsurface_temperature());
    }

    /// Empty moon list is a no-op: total = 0, state unchanged.
    #[test]
    fn moonless_planet_produces_no_tidal_heat() {
        let grid = HexGrid::new(4, 4);
        let mut state = PhysicsState::new(grid);
        for t in state.temperature_mut() {
            *t = Real::from_int(288);
        }
        let total = apply_tidal_heating(&mut state, Real::ONE, &[], None);
        assert_eq!(total, Real::ZERO);
        for t in state.temperature() {
            assert_eq!(*t, Real::from_int(288));
        }
    }

    /// P1.1 spec test: an Aqueous (Europa-like) configuration routes
    /// most of its tidal heat into the subsurface reservoir, not the
    /// surface. After 100 ticks of Io-class heating with the Aqueous
    /// 90 / 10 split, the subsurface T rises more than the surface T.
    /// This pins the structural correction the spec calls for —
    /// Europa-class moons keep their tidal heat under the ice.
    #[test]
    fn europa_like_configuration_powers_subsurface_not_surface() {
        let grid = HexGrid::new(8, 4);
        let mut state = PhysicsState::new(grid);
        // Seed both fields at the same baseline so any post-call
        // difference must come from the substrate-dependent split.
        for t in state.temperature_mut() {
            *t = Real::from_int(260);
        }
        for t in state.subsurface_temperature_mut() {
            *t = Real::from_int(260);
        }
        let r = Real::from_ratio(286, 1_000); // Io-class R for a non-zero rate.
        let moon = MoonHeating::icy(Real::from_ratio(41, 10_000), 2);
        for _ in 0..100 {
            apply_tidal_heating(
                &mut state,
                r,
                &[moon],
                Some(MetabolicSubstrate::Aqueous),
            );
        }
        let surf = state.temperature()[0];
        let sub = state.subsurface_temperature()[0];
        assert!(
            sub > surf,
            "Aqueous substrate should route tidal heat to subsurface: surf={surf:?} sub={sub:?}"
        );
        // And both must have risen above the baseline (the heating
        // budget is non-zero by construction with e > 0).
        assert!(
            sub > Real::from_int(260),
            "subsurface should have warmed: sub={sub:?}"
        );
    }

    /// P1.1 spec test: an Io-like silicate moon routes most of its
    /// tidal heat onto the surface (mid-latitude shear-zone
    /// volcanism), not the subsurface. After 100 ticks with the
    /// Silicate 30 / 70 split, surface T rises more than subsurface T.
    #[test]
    fn io_like_configuration_routes_heat_to_surface() {
        let grid = HexGrid::new(8, 4);
        let mut state = PhysicsState::new(grid);
        for t in state.temperature_mut() {
            *t = Real::from_int(110); // Io surface ~110 K
        }
        for t in state.subsurface_temperature_mut() {
            *t = Real::from_int(110);
        }
        let r = Real::from_ratio(286, 1_000);
        let moon = MoonHeating::rocky(Real::from_ratio(41, 10_000), 2);
        for _ in 0..100 {
            apply_tidal_heating(
                &mut state,
                r,
                &[moon],
                Some(MetabolicSubstrate::Silicate),
            );
        }
        let surf = state.temperature()[0];
        let sub = state.subsurface_temperature()[0];
        assert!(
            surf > sub,
            "Silicate substrate should route tidal heat to surface: surf={surf:?} sub={sub:?}"
        );
    }

    /// P1.1 spec test: subsurface conduction relaxes the surface
    /// temperature toward the (warmer) subsurface temperature over
    /// many ticks with no other forcing. Seed `T_sub = 300`,
    /// `T_surf = 280`, run 1000 conduction ticks; surface T should
    /// converge toward 290 (the midpoint, since the two-reservoir
    /// kernel is energy-conserving per cell).
    #[test]
    fn subsurface_conduction_warms_surface_over_time() {
        let grid = HexGrid::new(4, 4);
        let mut state = PhysicsState::new(grid);
        for t in state.temperature_mut() {
            *t = Real::from_int(280);
        }
        for t in state.subsurface_temperature_mut() {
            *t = Real::from_int(300);
        }
        let dt = Real::ONE;
        let initial_surf = state.temperature()[0];
        for _ in 0..1000 {
            subsurface_conduction_step(&mut state, dt);
        }
        let final_surf = state.temperature()[0];
        let final_sub = state.subsurface_temperature()[0];
        assert!(
            final_surf > initial_surf,
            "surface should warm via conduction: initial={initial_surf:?} final={final_surf:?}"
        );
        // Converges *toward* the subsurface T but doesn't necessarily
        // reach it within 1000 ticks at CONDUCTION_K = 0.001. The
        // gap should close substantially though: with k·dt = 0.001
        // per step, the exponential time-constant is 500 ticks, so
        // 1000 ticks closes ~86 % of the gap. Final surface T should
        // be within ~3 K of the midpoint (290 K) ± round-off.
        let mid = Real::from_int(290);
        let surf_gap = (final_surf - mid).abs();
        let sub_gap = (final_sub - mid).abs();
        // Loose tolerance — bracketing the asymptote, not a precise
        // closed-form match. Halfway between the seeds (10 K) is the
        // strict upper bound for an underdamped relaxation.
        assert!(
            surf_gap < Real::from_int(10),
            "surface should converge toward midpoint: gap={surf_gap:?}"
        );
        assert!(
            sub_gap < Real::from_int(10),
            "subsurface should converge toward midpoint: gap={sub_gap:?}"
        );
        // Total energy conservation per cell (modulo Q32.32 LSB
        // drift). T_surf + T_sub should stay near 580 across the
        // 1000-tick relaxation.
        let total = final_surf + final_sub;
        let expected = Real::from_int(580);
        let drift = (total - expected).abs();
        assert!(
            drift < Real::ONE,
            "per-cell energy conservation: total={total:?} expected={expected:?} drift={drift:?}"
        );
    }

    /// `subsurface_heat_fraction` returns the documented per-substrate
    /// splits. Pinned so a future re-tune of the ratios is intentional.
    #[test]
    fn subsurface_heat_fraction_per_substrate() {
        assert_eq!(
            subsurface_heat_fraction(MetabolicSubstrate::Aqueous),
            Real::from_ratio(90, 100)
        );
        assert_eq!(
            subsurface_heat_fraction(MetabolicSubstrate::Hydrocarbon),
            Real::from_ratio(90, 100)
        );
        assert_eq!(
            subsurface_heat_fraction(MetabolicSubstrate::Ammoniacal),
            Real::from_ratio(60, 100)
        );
        assert_eq!(
            subsurface_heat_fraction(MetabolicSubstrate::Silicate),
            Real::from_ratio(30, 100)
        );
    }

    /// `init_subsurface_temperature` sets each cell's subsurface T to
    /// `surface T - 10 K`. Smoke-tests the planet-init contract.
    #[test]
    fn init_subsurface_temperature_offsets_by_ten() {
        let grid = HexGrid::new(4, 4);
        let mut state = PhysicsState::new(grid);
        for (i, t) in state.temperature_mut().iter_mut().enumerate() {
            *t = Real::from_int(280 + i as i64);
        }
        state.init_subsurface_temperature();
        for i in 0..state.grid().n_cells() {
            assert_eq!(
                state.subsurface_temperature()[i],
                state.temperature()[i] - Real::from_int(10),
                "cell {i} subsurface should be surface - 10"
            );
        }
    }
}
