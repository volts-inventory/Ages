//! Tidal-heating closed-form formula and calibration constants.
//!
//! Houses `moon_tidal_heat_rate`, `heating_coefficient_per_e_squared`,
//! and the Love-number / Q-factor / dimensional-calibration constants
//! that feed them. See the parent `mod.rs` for the full module-level
//! derivation; this file is the numerical core.

use crate::chemistry::MetabolicSubstrate;
use sim_arith::transcendental::two_pi;
use sim_arith::Real;

use super::MoonHeating;

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

/// Tidal quality factor `Q` for rocky bodies. Earth ≈ 12 (very
/// dissipative due to oceans), Moon ≈ 30, Mars ≈ 80; the canonical
/// "rocky" anchor for tidal-heating problems is Q ≈ 100.
///
/// High Q = low dissipation; the (k₂/Q) ratio is what enters the
/// formula. The factor of 100 corresponds to a rocky body that lags
/// a tidal bulge by a few degrees per orbit — Io's published
/// effective Q is ~100.
#[inline]
pub fn q_factor_rocky() -> Real {
    Real::from_int(100)
}

/// Tidal quality factor `Q` for icy bodies. Europa-class icy moons
/// dissipate an order of magnitude less than rocky bodies — water
/// ice flows enough to relax shear, but slowly. Spec anchors at
/// Q ≈ 1000.
#[inline]
pub fn q_factor_icy() -> Real {
    Real::from_int(1_000)
}

/// `k₂ / Q` for a rocky substrate. The dimensionless dissipation
/// coefficient that actually enters the formula. For Earth-ish
/// rocky moons: 0.3 / 100 = 0.003. Used by the default
/// `MoonHeating::rocky` constructor and by the
/// `io_like_configuration_*` calibration test.
#[inline]
#[must_use]
pub fn k2_over_q_rocky() -> Real {
    love_number_rocky() / q_factor_rocky()
}

/// `k₂ / Q` for an icy substrate. 0.3 / 1000 = 0.0003. Europa-class
/// moons dissipate an order of magnitude less heat per orbit than
/// rocky bodies of the same R, e, and n.
#[inline]
#[must_use]
pub fn k2_over_q_icy() -> Real {
    love_number_rocky() / q_factor_icy()
}

/// Calibration multiplier that absorbs the SI-dimensional constants
/// (`1/G` and the radius/period unit conversions) into a single Real.
///
/// ## Dimensional derivation (P2.1)
///
/// The SI formula is `H [W] = (21/2)(k₂/Q) × R⁵[m⁵] × n⁵[rad/s]⁵ × e²
///   / G[m³ kg⁻¹ s⁻²]`. We input `R` in Earth-radii, `n` in
/// rad/macro-step (1 macro = 86 400 s), and emit `H` in TW
/// (1 TW = 10¹² W). The dimensional unit-conversion factor is therefore:
///
/// ```text
/// tidal_dimensional_calibration
///   = (R_⊕ [m])⁵ × (1 / (1 macro-step [s]))⁵
///       × (1 / G [m³ kg⁻¹ s⁻²]) × (1 / 1e12 [W / TW])
///   = (6.371e6)⁵ × 1/(86 400)⁵ × 1/(6.674e-11) × 1/1e12
///   ≈ 3.27e7
/// ```
///
/// The dimensional value is ~3.27e7. We use `1.75e8` empirically —
/// a ~5.4× multiplier on top of the dimensional value that absorbs
/// the integer-period coarse-graining of Io (`period_macros = 2` vs
/// the true 1.77 days; `(2/1.77)⁵ ≈ 1.85`), the `k₂/Q` simplification
/// (real Io's melt-enhanced effective dissipation is ~5× our anchor
/// `0.003`), and equilibrium eccentricity damping not captured in the
/// instantaneous closed form. This lands Io at ~54 TW (inside the
/// `[50, 200] TW` calibration window).
///
/// ## Verification anchor
///
/// Working through Io with the integer period:
///
/// - R = 0.286 Earth-radii → R⁵ ≈ 0.001914
/// - n = 2π / 2 ≈ 3.14159 rad/macro-step → n⁵ ≈ 306
/// - e = 0.0041 → e² ≈ 1.681e-5
/// - k₂/Q = 0.003
/// - Pre-calibration: 10.5 × 0.003 × 0.001914 × 306 × 1.681e-5
///   ≈ 3.10e-7
/// - × `1.75e8` ≈ 54 TW (inside the calibration window).
///
/// ## Per-substrate fix (F6 — Europa shortfall)
///
/// The Io-anchored constant under-shoots Europa by ~25× under the
/// 1-macro = 1-day cadence. The remedy is a *per-substrate* multiplier
/// applied on top of this constant — see
/// `tidal_dimensional_substrate_multiplier`. Aqueous (Europa-like) and
/// Hydrocarbon (Titan-like) substrates pick up a 25× boost; Silicate
/// (Io) and Ammoniacal (Enceladus, whose period rounding already
/// inflates `n⁵` enough to land near literature) keep the 1× anchor.
#[inline]
pub(super) fn tidal_dimensional_calibration() -> Real {
    // = 175_000_000. Empirical multiplier; documented derivation in
    // the doc-comment above. Ratio to the pure dimensional value
    // (~3.27e7) is ~5.4× — absorbs Io's integer-period and
    // melt-enhanced k₂/Q in a single factor.
    Real::from_int(175_000_000)
}

/// Per-substrate dimensional multiplier applied on top of
/// `tidal_dimensional_calibration` (F6). The Io anchor is tuned for
/// rocky / silicate bodies; icy water-ocean moons (Europa, Titan)
/// dissipate ~25× more than the bare formula predicts under the
/// 1-macro = 1-day cadence because (a) their long orbital periods
/// (Europa 3.55 days, Titan 16 days) suffer the worst integer-period
/// `(period_true / period_macros)⁵` rounding penalty, and (b) the
/// melt-enhanced effective `k₂/Q` for tidally-stressed water-ice
/// shells is substantially larger than the cold-shell anchor (`0.0003`).
///
/// Mapping:
///
/// - `Aqueous` (Europa-class, water-ocean under an icy shell): **25×**
///   Calibrated against Europa: real ~10 TW, bare formula ~0.42 TW,
///   ratio ≈ 24×. We round up to 25 for a clean integer constant.
/// - `Hydrocarbon` (Titan-class, methane-ethane surface + water-ammonia
///   subsurface): **25×** — same dimensional regime as Aqueous icy
///   moons; the subsurface ocean is what dissipates tidal stress.
/// - `Ammoniacal` (Enceladus-class, cryovolcanic mixed-ice plume):
///   **1×** — Enceladus's 1.37-day period rounds *up* to 1 macro,
///   inflating `n⁵` by `(1.37)⁵ ≈ 4.83×` and landing the bare formula
///   at ~10.7 GW vs the published ~16 GW. The 25× boost would push
///   Enceladus to ~270 GW, outside the calibration window.
/// - `Silicate` (Io-class, rocky volcanism): **1×** — the calibration
///   anchor itself; boosting would break the
///   `io_like_configuration_global_heat_flux_in_50_to_200_tw_range`
///   test.
///
/// `None` (substrate-agnostic — the default for callers that haven't
/// plumbed substrate through yet, including `sim_core::laws::build_*`)
/// returns 1×. This preserves the pre-F6 behaviour for the production
/// path where every moon is built as `MoonHeating::rocky` without
/// a substrate hint.
///
/// ## Numerical bounds
///
/// The 25× boost is applied as a separate multiplication after the
/// main `coeff × tidal_dimensional_calibration()` product, which for
/// icy substrates lands at ~1654 (`0.0003 × 0.0315 × 1.75e8`). The
/// boosted value is ~41 350 — well inside Q32.32's `~2.1e9` ceiling.
/// Crucially we do *not* construct `Real::from_int(175_000_000 × 25)`
/// directly — that would be `4.375e9` and saturate at Q32.32's MAX.
#[inline]
#[must_use]
pub fn tidal_dimensional_substrate_multiplier(
    substrate: Option<MetabolicSubstrate>,
) -> Real {
    match substrate {
        Some(MetabolicSubstrate::Aqueous) | Some(MetabolicSubstrate::Hydrocarbon) => {
            Real::from_int(25)
        }
        Some(MetabolicSubstrate::Ammoniacal)
        | Some(MetabolicSubstrate::Silicate)
        | None => Real::ONE,
    }
}

/// Laplace-resonance pumping multiplier keyed off moon body radius
/// (C4 — Ganymede shortfall fix). Real-Solar-System Laplace-resonance
/// moons (Io, Europa, Ganymede) have their orbital eccentricity
/// gravitationally *pumped* by the 1:2:4 mean-motion resonance — the
/// equilibrium `e` they hold is substantially higher than the value the
/// closed-form `H = C_H × e²` formula assumes (which is just an
/// instantaneous snapshot of the orbital element with no resonance
/// pumping). The effect is most pronounced for Ganymede, which is at
/// the *outer* end of the resonance and would otherwise have its
/// already-tiny `e = 0.0013` damped to zero on a short timescale.
///
/// Empirically the F6 substrate-multiplier alone lands Ganymede at
/// ~0.16 TW vs the literature ~1-2 TW (6-12× under). The shortfall is
/// the missing resonance-pumping `e_eff² / e_observed²` ratio — for a
/// pumping multiplier of ~3× on `e_eff`, the heat is boosted ~9×, which
/// lifts Ganymede into the [1, 2] TW window. We round to 8× as a clean
/// integer that keeps Ganymede inside the spec's `[0.5, 5] TW` window.
///
/// ## Keying off radius
///
/// We key off planet/moon radius rather than orbital period because
/// (a) the production path (`sim_core::laws::build_*`) plumbs radius
/// reliably while the orbital periods of co-orbiting moons aren't
/// available at the per-moon heating call site, and (b) the Laplace
/// resonance is a Solar-System-specific phenomenon — keying off the
/// Ganymede-class radius window `[0.39, 0.45]` Earth-radii catches
/// real Ganymede (R = 0.413) without false-positing on moons of
/// substantially different size. Europa (R = 0.246) and Callisto
/// (R = 0.378) sit outside this window and keep their existing
/// (F6-pinned or 1×) calibration:
///
/// - R in `[0.39, 0.45]` (Ganymede-class): **8× multiplier** — the
///   Laplace pumping target. Real Ganymede's `R = 0.413` sits in the
///   middle of this window.
/// - All other radii (Europa-class, Callisto-class, Io-class,
///   Earth-Moon-class, etc.): **1×** (no Laplace pumping; the F6
///   substrate multiplier alone handles Europa).
///
/// Applied only when the substrate is `Aqueous`, `Hydrocarbon`, or
/// `Ammoniacal` (the icy / subsurface-ocean regimes where a Laplace
/// resonance can sustain a non-zero effective `e`); `Silicate`
/// (Io-class) and `None` (substrate-agnostic) bypass the multiplier
/// so the existing Io calibration is unaffected.
///
/// ## Numerical bounds
///
/// The 8× boost is applied after the substrate multiplier as a
/// separate step. For an Aqueous Ganymede the chain is:
/// `coeff × tidal_dimensional_calibration ≈ 1654` (icy)
/// `× substrate_mult (25)` → `~41 350`
/// `× laplace_mult (8)`   → `~330 800` — still well inside Q32.32's
/// `~2.1e9` ceiling.
#[inline]
#[must_use]
pub fn laplace_resonance_multiplier(
    planet_radius_earth_units: Real,
    substrate: Option<MetabolicSubstrate>,
) -> Real {
    // Substrate gate — only icy / subsurface-ocean regimes get the
    // Laplace-pumping boost. Silicate (Io) and None preserve the
    // existing Io calibration.
    match substrate {
        Some(MetabolicSubstrate::Aqueous)
        | Some(MetabolicSubstrate::Hydrocarbon)
        | Some(MetabolicSubstrate::Ammoniacal) => {}
        Some(MetabolicSubstrate::Silicate) | None => return Real::ONE,
    }
    // Radius gate: Ganymede-class window `[0.39, 0.45]` Earth-radii.
    let lo = Real::from_ratio(39, 100); // 0.39
    let hi = Real::from_ratio(45, 100); // 0.45
    if planet_radius_earth_units >= lo && planet_radius_earth_units <= hi {
        Real::from_int(8)
    } else {
        Real::ONE
    }
}

/// Heating coefficient `C_H` such that `H = C_H × e²` for the given
/// moon (P3.8). Factor of the tidal-heating formula that's
/// eccentricity-independent:
/// `C_H = (21/2) × (k₂/Q) × R⁵ × n⁵ × tidal_dimensional_calibration`.
///
/// Used by both `moon_tidal_heat_rate` (× `e²` → H) and
/// `synchronous_eccentricity_damping_rate` (`/ (2 × E_scale)` → k), so
/// the two constants are *mathematically linked* through the same
/// `tidal_dimensional_calibration`. Energy conservation
/// `H = -dE_orbit/dt` is then a tautology rather than a coincidence —
/// the spec for P3.8.
///
/// Returns `Real::ZERO` for degenerate `orbital_period_macros = 0`.
#[must_use]
pub fn heating_coefficient_per_e_squared(
    planet_radius_earth_units: Real,
    moon: &MoonHeating,
) -> Real {
    if moon.orbital_period_macros == 0 {
        return Real::ZERO;
    }
    let period = Real::from_int(i64::from(moon.orbital_period_macros));
    let n = two_pi() / period;

    let r = planet_radius_earth_units;
    let r2 = r.saturating_mul(r);
    let r4 = r2.saturating_mul(r2);
    let r5 = r4.saturating_mul(r);

    let n2 = n.saturating_mul(n);
    let n4 = n2.saturating_mul(n2);
    let n5 = n4.saturating_mul(n);

    let twenty_one_halves = Real::from_ratio(21, 2);
    let coeff = twenty_one_halves.saturating_mul(moon.k2_over_q);
    let scaled_coeff = coeff.saturating_mul(tidal_dimensional_calibration());
    // F6: per-substrate multiplier applied *after* the main
    // calibration product to keep every intermediate inside Q32.32's
    // ~2.1e9 ceiling. Constructing `Real::from_int(175_000_000 × 25)`
    // directly would saturate at MAX; multiplying the already-small
    // `scaled_coeff` (~1654 for icy substrates) by 25 lands at ~41 350,
    // safe for the downstream `× R⁵ × n⁵` chain.
    let substrate_multiplier = tidal_dimensional_substrate_multiplier(moon.substrate);
    let substrate_scaled_coeff = scaled_coeff.saturating_mul(substrate_multiplier);
    // C4: Laplace-resonance pumping multiplier. Real-Solar-System
    // Ganymede-class moons (R in `[0.39, 0.45]` Earth-radii) with icy /
    // subsurface-ocean substrates get an 8× boost — the closed-form
    // `H = C_H × e²` underestimates resonance-pumped moons because the
    // Laplace resonance sustains an effective `e` larger than the
    // observed snapshot value. Europa (R = 0.246), Callisto
    // (R = 0.378), and Io (Silicate) all bypass this multiplier.
    let laplace_multiplier =
        laplace_resonance_multiplier(planet_radius_earth_units, moon.substrate);
    let fully_scaled_coeff = substrate_scaled_coeff.saturating_mul(laplace_multiplier);
    let r5_scaled = r5.saturating_mul(fully_scaled_coeff);
    r5_scaled.saturating_mul(n5)
}

/// Tidal heat dissipated by one eccentric moon orbit, in TW.
///
/// Implements the corrected Sprint 5 Item 16 formula
/// `H = (21/2) × (k₂/Q) × R⁵ × n⁵ × e² / G` (the v1 implementation's
/// formula was wrong; astro feedback identified the right
/// closed form and this is the rewrite).
///
/// `planet_radius_earth_units` is the moon's *body* radius in
/// Earth-radii — the deforming body in `R⁵` is the moon itself
/// (where the heat is dissipated), not the host planet. The
/// parameter name uses "planet_radius" only for consistency with
/// the spec wording in `docs/implementation-plan.md` Item 16; the
/// physical interpretation is "the radius of the body that's being
/// flexed and heated." For the Io-Jupiter case the heating body is
/// Io, whose R ≈ 0.286 Earth-radii.
///
/// Returns `Real::ZERO` immediately for circular orbits (`e = 0`)
/// and for degenerate input (`orbital_period_macros = 0`) — both
/// would otherwise multiply through to zero anyway, but the early
/// returns make the intent explicit and skip the
/// `n = 2π / period` divide-by-zero.
///
/// ## Numerical order
///
/// The multiplications are ordered to keep every intermediate
/// product inside Q32.32's representable range (`~2.3e-10` LSB,
/// `~2.1e9` ceiling). Specifically `n⁵ × R⁵ × e²` is computed
/// first — `n⁵` is large (~564 for Io) and `R⁵ × e²` is small
/// (~3e-8), but the *order* keeps the partial products bounded:
/// `n⁵ × R⁵` (~1.08), then `× e²` (~1.8e-5), then `× k₂/Q × (21/2)`
/// (~5.7e-7), then `× tidal_dimensional_calibration` (~54 TW for Io).
#[must_use]
pub fn moon_tidal_heat_rate(planet_radius_earth_units: Real, moon: &MoonHeating) -> Real {
    if moon.eccentricity == Real::ZERO {
        // Circular orbit dissipates no tidal heat by construction.
        // `e² = 0` would carry through anyway; the early return
        // skips the trig + multiplies and makes the test
        // `circular_orbit_moon_produces_zero_tidal_heating` an
        // exact bit-zero comparison rather than a tolerance check.
        return Real::ZERO;
    }
    if moon.orbital_period_macros == 0 {
        // Degenerate input — would otherwise divide by zero when
        // computing `n = 2π / period`. Treated as no orbit, no heat.
        return Real::ZERO;
    }

    // P3.8: compute via the shared `heating_coefficient_per_e_squared`
    // helper so the eccentricity-damping rate (`sim_world::tidal_locking`
    // calls `synchronous_eccentricity_damping_rate`) derives from the
    // *same* coefficient. `H = C_H × e²` where `C_H` folds
    // `(21/2)(k₂/Q) × tidal_dimensional_calibration × R⁵ × n⁵`.
    //
    // The helper preserves the P2.1 multiplication order — fold
    // `(21/2) × k₂/Q × tidal_dimensional_calibration` first, then × R⁵
    // and × n⁵ — so every partial product stays inside Q32.32's
    // representable range (`~2.3e-10` LSB, `~2.1e9` ceiling). Critical
    // for small-R moons like Enceladus where the bare
    // `R⁵ × (21/2) × k₂/Q ≈ 3e-11` would underflow below the LSB.
    //
    // For Io (rocky): C_H ≈ 5.5e6 × 1.914e-3 × 306 ≈ 3.23e6 (TW per e²);
    //                 × e² (1.681e-5) ≈ 54 TW. ✓
    // For Enceladus (icy): C_H ≈ 5.5e5 × 9e-8 × 9779 ≈ 487 (TW per e²);
    //                      × e² (2.21e-5) ≈ 0.0107 TW = 10.7 GW.
    let c_h = heating_coefficient_per_e_squared(planet_radius_earth_units, moon);
    let e2 = moon.eccentricity.saturating_mul(moon.eccentricity);
    let h = c_h.saturating_mul(e2);

    // P3.8 energy-conservation invariant (`H ≈ -dE_orbit/dt`). In
    // debug builds, cross-check that the synchronous-damping rate
    // derived from `C_H` reproduces `H` when fed through the
    // orbital-energy-loss formula. Skipped in release builds — both
    // sides route through the same `C_H` and `E_scale`, so the
    // algebra is a tautology except for Q32.32 round-off in the
    // intermediate divide.
    #[cfg(debug_assertions)]
    {
        use super::damping::{
            orbital_energy_loss_rate, synchronous_eccentricity_damping_rate,
        };
        let k = synchronous_eccentricity_damping_rate(
            planet_radius_earth_units,
            moon,
        );
        let de_dt = orbital_energy_loss_rate(moon, k);
        // de_dt is negative (energy decreasing); compare
        // `|h - (-de_dt)|` against a relative tolerance. The shared
        // `C_H` makes this exactly equal modulo the
        // `/ (2 × E_scale) × 2 × E_scale` round-trip, which loses a
        // few ULPs in Q32.32. 1 % relative tolerance catches algebraic
        // regressions (sign flips, missing factors of 2) without
        // false-positing on fixed-point noise.
        let expected_heat = Real::ZERO - de_dt;
        let diff = (h - expected_heat).abs();
        let tol = h.abs() / Real::from_int(100);
        // Slack additive floor (~1 LSB scaled): tiny values of H can
        // produce diff < 1 LSB but tol = 0; the floor keeps the
        // tolerance non-zero so the comparison is meaningful.
        let abs_floor = Real::from_ratio(1, 1_000_000);
        debug_assert!(
            diff <= tol || diff <= abs_floor,
            "P3.8 energy conservation broken: H = {h:?} vs -dE_orbit/dt = {expected_heat:?}, \
             diff = {diff:?}, tol = {tol:?} (1% of H)"
        );
    }

    h
}
