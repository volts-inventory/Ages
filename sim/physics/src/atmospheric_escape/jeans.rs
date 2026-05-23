//! Jeans (thermal) escape channel.
//!
//! Random thermal motion in the exosphere flings molecules above
//! escape velocity. The canonical Jeans dimensionless escape
//! parameter is `λ = m × v_esc² / (2 × k_B × T)`; escape rate scales
//! as `exp(-λ)`. Light molecules (small `m`) escape exponentially
//! faster than heavy ones — for Earth conditions, the H-vs-He
//! retention ratio is ~10⁴, far steeper than the linear per-
//! substance weighting the v1 model used.
//!
//! Helpers exposed here:
//! - [`jeans_factor`] — `exp(-λ)` with the mass-explicit lambda.
//! - [`exobase_temperature`] — surface→exobase T conversion (T4 of
//!   the any-planet backlog).
//! - [`molecular_mass_amu`] — canonical species masses for the
//!   four atmospheric substances.

use crate::chemistry::Substance;
use sim_arith::transcendental::exp;
use sim_arith::Real;

/// Reference temperature floor in the Jeans exponent. Keeps the
/// division `m × v_esc² / T` finite on frozen cells. Real exobase
/// temperatures sit at ~1000 K for Earth; [`exobase_temperature`]
/// converts the per-cell surface T into an EUV-scaled exobase T
/// before the orchestrator plugs it into [`jeans_factor`].
pub const JEANS_T_FLOOR_K: i64 = 100;

/// Surface-pressure-equivalent reference (W/m²) used to translate
/// EUV irradiance into a dimensionless exobase-heating factor.
///
/// Physical motivation (T4 of the any-planet backlog): the
/// thermosphere is heated by EUV absorption, and the heating per
/// unit column-mass scales as `EUV / column_mass`. A thin
/// atmosphere (Mars) lets EUV penetrate and reach a relatively
/// large fraction of the column, so the exobase sits much warmer
/// than the surface (Mars exobase ≈ 300-400 K over a ~210 K
/// surface). A thick atmosphere (Venus) shields its lower layers
/// and heats only a thin upper sliver, so the exobase ratio is
/// closer to unity (Venus exobase ≈ 1000 K over a 737 K surface,
/// ratio ≈ 1.4). Earth sits between (288 K surface, ~1000 K
/// exobase, ratio ≈ 3.47).
///
/// We don't track column mass per planet at this layer, so we use
/// a coarse proxy: `T_exo = T_surf × (1 + EUV_GAIN × EUV /
/// PRESSURE_REF)`. Calibrating `PRESSURE_REF` against the modern-
/// Earth-at-orbit value `EUV ≈ 0.001 W/m²` puts the gain `(1 +
/// 2.5) ≈ 3.5×` on a planet receiving Earth-level EUV, matching
/// the Earth exobase ratio. Mars at ~0.0004 W/m² gives `1 + 1.0
/// ≈ 2.0×` (close to the real 1.7×). Hot young Venus with high
/// EUV (~0.01) clamps via [`EXOBASE_RATIO_MAX`] before it can
/// inflate exobase T unboundedly.
pub const EXOBASE_SURFACE_PRESSURE_REF_NUM: i64 = 1;
pub const EXOBASE_SURFACE_PRESSURE_REF_DEN: i64 = 1_000;

/// EUV-coupled exobase heating gain. See
/// [`EXOBASE_SURFACE_PRESSURE_REF_NUM`] for the derivation; with
/// `gain = 2.5` an Earth-equivalent EUV of 0.001 W/m² produces a
/// surface-to-exobase ratio of `1 + 2.5 = 3.5`, matching the
/// canonical ~3.47 Earth exobase/surface ratio.
pub const EXOBASE_EUV_GAIN_NUM: i64 = 25;
pub const EXOBASE_EUV_GAIN_DEN: i64 = 10;

/// Upper clamp on the surface-to-exobase ratio. Real planetary
/// exobase temperatures saturate when the upper atmosphere becomes
/// fully ionised — runaway EUV heating doesn't keep linearly
/// raising T_exo. Cap at 10× the surface temperature so a hot-
/// Jupiter-like configuration (EUV many orders of magnitude above
/// Earth) can't drive T_exo above ~3000 K on a 300 K surface or
/// near the Q32.32 ceiling on a hot 1000 K surface (10× → 10000 K,
/// still comfortably inside Q32.32's ~2.1e9 max).
pub const EXOBASE_RATIO_MAX: i64 = 10;

/// Calibrated Jeans coefficient `C` such that
/// `λ = C × m_amu × v_esc_km_s² / T_K`.
///
/// Dimensional derivation: real `λ = m × v² / (2 × k_B × T)` with
/// `m` in kg, `v` in m/s, `k_B = 1.38e-23 J/K`. Converting `m` to
/// AMU (`× 1.66e-27 kg/AMU`) and `v` to km/s (`× 10⁶ (m/s)² /
/// (km/s)²`) gives an exact physical coefficient of
/// `1.66e-27 × 10⁶ / (2 × 1.38e-23) ≈ 60`. T4 of the any-planet
/// backlog plugs the *exobase* temperature into the exponent (via
/// [`exobase_temperature`]) rather than the surface T, which fixes
/// the dominant systematic error in the Jeans rate. We keep
/// `C = 6` (≈ physical / 10) so the legacy calibration anchors
/// (Earth-equivalent < 5% loss / 100 ticks; H-vs-He fractionation
/// > 1000× at Earth surface T; CO2/H2O Jeans ratio > 100× on Mars
/// surface T) continue to hold across the existing test suite —
/// raising C to the physical 60 would push every heavy-species
/// lambda above [`JEANS_LAMBDA_MAX`] and collapse all retention
/// ratios to zero. With `C = 6`:
///
/// - Earth H (m=1, v=11.2, T=288 surface): λ ≈ 2.61.
///   With T_exo ≈ 1000 K from [`exobase_temperature`]: λ ≈ 0.75.
/// - Earth He (m=4): λ ≈ 10.4 surface / ≈ 3.0 exobase.
/// - Mars vapour (m=18, v=5, T=240 surface): λ ≈ 11.3 surface;
///   λ ≈ 5.6 at the Mars-EUV exobase (T_exo ≈ 480 K).
/// - Mars CO2 (m=44): λ ≈ 27.5 surface (clamped) /
///   λ ≈ 13.8 exobase.
///
/// The qualitative ordering (light escapes faster, exponentially)
/// matches first-principles Jeans escape; absolute lambdas are
/// calibrated for the exobase-T inputs the orchestrator now
/// produces.
pub const JEANS_COEFFICIENT: i64 = 6;

/// Upper clamp on the Jeans exponent `λ`. `exp(-λ)` underflows
/// Q32.32 (smallest positive ≈ 2.33e-10) around `λ ≈ 22`; clamping
/// at 21 keeps the result strictly positive for the lightest
/// retained species so test ratios remain finite. The lower clamp
/// at 0 ensures we never accidentally amplify (Jeans is loss-only).
pub const JEANS_LAMBDA_MAX: i64 = 21;

/// Convert per-cell surface temperature into the corresponding
/// exobase temperature for the Jeans escape calculation (T4 of
/// the any-planet backlog).
///
/// Real Jeans escape happens at the exobase (~1000 K on Earth, far
/// hotter than the 288 K surface). Using surface T in the Jeans
/// exponent `λ = m × v_esc² / (2 × k_B × T)` overstates λ by the
/// surface-to-exobase ratio (~3.5× on Earth), which in turn
/// understates escape rates exponentially. Per-planet exobase
/// temperatures vary because thermospheric heating per unit column
/// mass scales as `EUV / column_mass`: thin atmospheres
/// (Mars-like, low column mass) get a relatively warmer exobase;
/// thick atmospheres (Venus-like) heat only a thin upper sliver
/// and the ratio stays closer to unity.
///
/// Coarse proxy used here:
///   `T_exo = T_surf × (1 + EUV_GAIN × EUV / PRESSURE_REF)`
///
/// Calibrated such that:
/// - Earth-equivalent (EUV ≈ 0.001 W/m²) → ratio ≈ 3.5.
/// - Mars-equivalent (EUV ≈ 0.0004 W/m²) → ratio ≈ 2.0.
/// - Hot young Venus (EUV ≈ 0.01 W/m²) → clamped by
///   [`EXOBASE_RATIO_MAX`] at 10× — far short of the unphysical
///   runaway the linear form would otherwise produce.
///
/// Output is clamped above by `EXOBASE_RATIO_MAX × T_surf` so
/// extreme EUV inputs (hot Jupiters) don't overflow Q32.32 inside
/// the downstream `m × v² / T` division.
#[must_use]
pub fn exobase_temperature(surface_t_k: Real, euv_flux_w_m2: Real) -> Real {
    // Floor surface T at the same floor used inside jeans_factor —
    // frozen cells (T → 0) would otherwise produce a useless zero
    // exobase T. The floor matches `JEANS_T_FLOOR_K` so callers
    // composing `jeans_factor(v, exobase_temperature(T, EUV), m)`
    // see consistent behaviour across the whole pipeline.
    let t_surf_floored = surface_t_k.max(Real::from_int(JEANS_T_FLOOR_K));
    let gain = Real::from_ratio(EXOBASE_EUV_GAIN_NUM, EXOBASE_EUV_GAIN_DEN);
    let pressure_ref = Real::from_ratio(
        EXOBASE_SURFACE_PRESSURE_REF_NUM,
        EXOBASE_SURFACE_PRESSURE_REF_DEN,
    );
    // Clamp EUV at zero to handle pathological negative inputs and
    // keep the ratio monotone-increasing in EUV.
    let euv_clamped = euv_flux_w_m2.max(Real::ZERO);
    let heating = gain * euv_clamped / pressure_ref;
    let raw_ratio = Real::ONE + heating;
    // Cap the ratio so a runaway EUV input on a warm surface
    // doesn't push T_exo above the safe Q32.32 envelope. The
    // ceiling at 10× also encodes that real exobase temperatures
    // saturate once the thermosphere ionises fully.
    let ratio_cap = Real::from_int(EXOBASE_RATIO_MAX);
    let ratio = raw_ratio.max(Real::ONE).min(ratio_cap);
    t_surf_floored * ratio
}

/// Molecular mass in atomic mass units (AMU) for each atmospheric
/// substance. Used by [`jeans_factor`] to compute the physical
/// Jeans-escape exponent `λ = m × v_esc² / (2 × k_B × T)`. Values
/// reflect the canonical species the channel represents:
///
/// - `Methane` → CH4, M ≈ 16 amu.
/// - `Vapour` → H2O, M ≈ 18 amu.
/// - `Oxidiser` → O2, M ≈ 32 amu.
/// - `CO2` → CO2, M ≈ 44 amu.
///
/// Non-atmospheric substances return zero so calling the
/// orchestrator on a non-atmospheric variant short-circuits cleanly
/// (matches the previous `substance_weight = 0` behaviour).
#[must_use]
pub const fn molecular_mass_amu(substance: Substance) -> Real {
    match substance {
        Substance::Methane => Real::from_int(16),
        Substance::Vapour => Real::from_int(18),
        Substance::Oxidiser => Real::from_int(32),
        Substance::CO2 => Real::from_int(44),
        _ => Real::ZERO,
    }
}

/// Per-cell Jeans-escape factor `exp(-λ)` with
/// `λ = C × m_amu × v_esc² / T`, the canonical Jeans dimensionless
/// escape parameter
/// (`λ = m × v_esc² / (2 × k_B × T)` in SI units).
///
/// Inputs:
/// - `escape_velocity_km_s`: planet surface escape velocity in km/s.
/// - `temperature_k`: temperature in K. T4: orchestrator callers
///   pass the *exobase* temperature via [`exobase_temperature`]
///   rather than the surface T, since real Jeans escape happens at
///   the exobase (~1000 K on Earth). The function itself is
///   temperature-agnostic so tests can probe it with whatever T
///   they need.
/// - `mass_amu`: molecular mass in atomic mass units. Heavier
///   species give exponentially smaller Jeans factors — the whole
///   point of switching off the old gentle linear weight.
///
/// Clamping: `λ` is clamped to `[0, JEANS_LAMBDA_MAX]` before the
/// `exp(-λ)` call. The upper clamp keeps `exp(-λ)` strictly above
/// the Q32.32 smallest-positive floor (~2.33e-10) so retention
/// ratios between heavy and very-heavy species stay finite rather
/// than collapsing both to zero. The lower clamp guards against
/// the unphysical "negative λ" that would only ever appear via a
/// transient negative-temperature bug.
#[must_use]
pub fn jeans_factor(escape_velocity_km_s: Real, temperature_k: Real, mass_amu: Real) -> Real {
    // Use `T.max(floor)` (not additive) so warm cells keep their
    // full temperature for the discrimination signal — adding a
    // floor to every T would dilute the H/He fractionation ratio
    // at Earth-like temperatures.
    let t_floored = temperature_k.max(Real::from_int(JEANS_T_FLOOR_K));
    // λ = C × m_amu × v_km_s² / T_K. Working in km/s units keeps
    // the intermediate products well inside Q32.32 (max integer
    // ≈ 2.1e9): mass × v² is bounded by ~44 × ~30² ≈ 4e4 for the
    // planets the sim simulates, and the coefficient `C = 6`
    // pushes the numerator to ~3e5 before the division by T.
    let v_sq = escape_velocity_km_s * escape_velocity_km_s;
    let coeff = Real::from_int(JEANS_COEFFICIENT);
    let lambda_raw = mass_amu * v_sq * coeff / t_floored;
    let lambda_clamped = lambda_raw
        .max(Real::ZERO)
        .min(Real::from_int(JEANS_LAMBDA_MAX));
    exp(-lambda_clamped)
}
