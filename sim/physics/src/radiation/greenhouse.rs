//! Per-cell dynamic greenhouse forcing: per-substance coefficients
//! (h2o, co2, ch4), cloud-deck contributions (cirrus / stratus),
//! the pressure-scaled saturation cap that bounds runaway, and the
//! per-tick photolysis decay applied to CH4.
//!
//! The atmosphere-class baseline `greenhouse_k` lives on
//! [`super::Radiation`] (folded into the per-row equilibrium table
//! by [`super::equilibrium::compute_t_eq_table`]); this module
//! covers the composition-dependent forcing layered on top each
//! tick.

use crate::clouds::{cirrus_greenhouse_strength, stratus_greenhouse_k, CloudType};
use sim_arith::transcendental::ln;
use sim_arith::Real;

/// Per-unit-density greenhouse coefficient for water vapour.
/// Calibrated against [`crate::hydrology::saturation_vapour_cap`],
/// which lets vapour density rise into the tens of thousands as
/// T approaches the cap reference. The feedback slope
/// `d(T_eq)/dT = K × d(cap)/dT = K × 4 × cap / T` crosses unity
/// (the K-I runaway threshold) when `K > T / (4 × cap(T))`. With
/// `K = 0.002` the threshold sits around T ~ 350-400 K —
/// temperate Earth-like cells stay below it (loop converges to a
/// modest equilibrium) and a hot seed pushes past it (loop
/// diverges into a Venus-style runaway, plateauing only when
/// [`greenhouse_cap_scaled`] binds).
pub(super) fn h2o_greenhouse_k() -> Real {
    Real::from_ratio(2, 1_000)
}

/// Per-unit-density greenhouse coefficient for CO2. Per-molecule
/// CO2 is a stronger IR absorber than H2O in its bands, but
/// planetary CO2 densities are much lower; the ~1500× scale vs.
/// H2O (in this unit system) lets a modest CO2 buildup deliver
/// the multi-K forcing real Earth sees from CO2 doubling without
/// requiring Venus-scale columns.
///
/// Calibrated for T13 snowball recovery (fix-C3): a snowball
/// planet with the stock `Volcanism::earth_like` source builds
/// ~9 units of CO2 per cell over ~10⁶ ticks (≈ 1000× the
/// depleted initial column). At `K = 5.0` the raw greenhouse
/// term lands near ~45 K — enough that, even with the
/// snowball-cell effective albedo still pinned high by sea-ice
/// (per-cell T_eq baseline ~233 K, ~36 K below the ice-free
/// baseline ~269 K), the total per-cell T_eq crosses the freeze
/// line and the positive ice-albedo feedback unlocks. The
/// previous coefficient (0.030 K per unit) topped out at ~0.3 K
/// of forcing from the same buildup, ~100× too small to lever
/// past the bifurcation gap.
///
/// Earth-baseline cells carry only fractions of a unit of CO2,
/// so the contribution stays a fraction of a degree — the modern
/// Earth equilibrium is dominated by the atmosphere-class
/// baseline `greenhouse_k`. A Venus-like dense-CO2 column
/// (~10⁶ units in the calibration test) overdrives the cap, so
/// [`greenhouse_cap_scaled`] binds and the plateau is set by the cap
/// rather than this coefficient.
pub(super) fn co2_greenhouse_k() -> Real {
    Real::from_int(5)
}

/// Per-unit-density greenhouse coefficient for methane. Similar
/// order to CO2 (per-molecule strong, density-low). Combined with
/// the photolysis decay below, this gives a transient warming
/// pulse from any CH4 injection that fades over hundreds of ticks.
pub(super) fn ch4_greenhouse_k() -> Real {
    Real::from_ratio(25, 1_000)
}

/// Per-tick exponential-decay factor on CH4 density, mimicking
/// UV photolysis (real-atmosphere lifetime ~10 years). Set to
/// `0.999` so a 1.0-density CH4 column halves in ~700 ticks —
/// short enough that a CH4 burst doesn't perpetually warm the
/// planet, long enough that CH4 still contributes meaningfully
/// to short-term warming events. Applied uniformly per cell;
/// future work can couple decay rate to local UV flux.
pub(super) fn ch4_decay_per_tick() -> Real {
    Real::from_ratio(999, 1_000)
}

/// Earth's nominal surface pressure (Pa). Anchors the pressure-
/// scaled greenhouse cap so the Earth-pressure case reduces to
/// the original 250 K saturation ceiling.
pub(super) fn earth_surface_pressure_pa() -> Real {
    Real::from_int(101_325)
}

/// `ln(10)` precomputed for converting natural log to log10.
/// `log10(x) = ln(x) / ln(10)`; pulled into a helper so the
/// pressure-cap formula avoids recomputing the constant per
/// integrate call.
fn ln_10() -> Real {
    // ln(10) ≈ 2.30258509…; rational approximation keeps the
    // Q32.32 representation deterministic across builds.
    Real::from_ratio(2_302_585, 1_000_000)
}

/// Per-cell greenhouse contribution ceiling, in K, scaled by
/// surface pressure. Physically motivated: real-atmosphere IR
/// absorption bands saturate at high optical depth, so doubling
/// the greenhouse gas column doesn't double the warming once the
/// bands are already opaque (Beer-Lambert with τ ≫ 1). However,
/// the *post-saturation* contribution from pressure-broadened
/// continuum absorption and overlapping-band wing forcing does
/// continue scaling with column density (i.e. with surface
/// pressure for a well-mixed gas). A constant cap therefore
/// underestimates the ceiling for thick atmospheres like Venus's
/// 90-bar CO2 column.
///
/// Formula: `cap = 250 + 100 × log10(P / P_earth)`, clamped to
/// `[50, 600]` K to keep the cap finite for arbitrarily thin /
/// thick atmospheres and to stay well clear of the Q32.32 range.
/// Anchor points:
/// - Earth (~101 325 Pa) → 250 K (preserves legacy calibration
///   and existing tests).
/// - Venus (~9.2×10⁶ Pa) → 250 + 100 × log10(90.8) ≈ 446 K,
///   which sits a Venus-equivalent runaway plateau in the
///   literature 700-770 K band (bare T_eq ~309 K + ~446 K cap
///   ≈ 755 K).
/// - Mars (~610 Pa) → clamped at the 50 K floor (raw value
///   would be 250 − 222 = 28 K).
///
/// Without this cap a Venus-style runaway in the simulation has
/// no upper bound — the H2O cycle keeps lifting
/// `saturation_vapour_cap(T)` quartically with T, which lifts
/// vapour, which lifts greenhouse forcing, with no physical stop
/// until the fixed-point arithmetic overflows
/// (`saturation_vapour_cap` hits `Real`'s ~2.1e9 ceiling around
/// T ≈ 5300 K).
pub fn greenhouse_cap_scaled(surface_pressure_pa: Real) -> Real {
    let earth_p = earth_surface_pressure_pa();
    let base = Real::from_int(250);
    // Guard against ln(0) / ln(negative): a zero / negative
    // surface pressure short-circuits to the 50 K floor (the
    // thin-atmosphere clamp). The constructor enforces a
    // positive default, but the explicit guard keeps the helper
    // robust if a caller threads through a degenerate value.
    if surface_pressure_pa <= Real::ZERO {
        return Real::from_int(50);
    }
    let ratio = surface_pressure_pa / earth_p;
    // `ln(ratio)` panics on a non-positive argument; the divide
    // above keeps `ratio > 0` because both numerator and
    // denominator are positive Reals.
    let log10_ratio = ln(ratio) / ln_10();
    let raw_cap = base + Real::from_int(100) * log10_ratio;
    // Clamp to `[50, 600]` K. The floor keeps thin-atmosphere
    // worlds (Mars) from collapsing the runaway-bounding cap to
    // zero; the ceiling keeps a hypothetical super-Earth with a
    // multi-hundred-bar atmosphere from overflowing the per-cell
    // feedback term.
    let floor = Real::from_int(50);
    let ceil = Real::from_int(600);
    raw_cap.max(floor).min(ceil)
}

/// Bundle of per-tick scalars used by the per-cell greenhouse
/// loop. Built once in [`super::Radiation::integrate`] so each
/// cell's call into [`greenhouse_cell`] avoids the function-call
/// overhead and re-derivation of the constants.
#[derive(Debug, Clone, Copy)]
pub(super) struct GreenhouseConstants {
    pub h2o_k: Real,
    pub co2_k: Real,
    pub ch4_k: Real,
    pub cap: Real,
    pub lapse_rate: Real,
    pub cirrus_altitude: Real,
    pub stratus_gh: Real,
}

impl GreenhouseConstants {
    pub(super) fn new(
        surface_pressure_pa: Real,
        lapse_rate: Real,
        cirrus_altitude: Real,
    ) -> Self {
        Self {
            h2o_k: h2o_greenhouse_k(),
            co2_k: co2_greenhouse_k(),
            ch4_k: ch4_greenhouse_k(),
            cap: greenhouse_cap_scaled(surface_pressure_pa),
            lapse_rate,
            cirrus_altitude,
            stratus_gh: stratus_greenhouse_k(),
        }
    }
}

/// Compute the per-cell dynamic greenhouse contribution: sum of
/// the vapour / CO2 / CH4 column-greenhouse terms plus the cloud-
/// deck forcing (cirrus / stratus), capped by
/// [`greenhouse_cap_scaled`] so a saturated runaway plateaus rather
/// than overflowing.
///
/// The atmosphere-class baseline (`greenhouse_k` passed to
/// [`super::Radiation::for_planet`]) is already folded into the
/// per-row equilibrium table; this returns only the per-cell
/// composition-dependent term.
///
/// The H2O term is *implicitly* exponential in T because vapour
/// density itself is bounded by [`crate::hydrology::saturation_vapour_cap`],
/// which rises quartically with T/T_ref — that's the Clausius-
/// Clapeyron-coupled positive feedback that drives runaway. The
/// pressure-scaled cap lets dense atmospheres reach the
/// literature-anchored Venus plateau (~735 K) rather than the
/// legacy Earth-pressure ceiling (~559 K with a 250 K cap).
pub(super) fn greenhouse_cell(
    consts: &GreenhouseConstants,
    vapour: Real,
    co2: Real,
    ch4: Real,
    cloud_fraction: Real,
    cloud_type_byte: u8,
    temp_prev: Real,
) -> Real {
    // Per-cell cloud greenhouse contribution. Cirrus cells
    // add `cirrus_gh × cloud_fraction`; stratus cells add
    // the smaller `stratus_gh × cloud_fraction`. Without
    // this term the cloud_fraction field affected only
    // albedo (shortwave shielding) — clouds in the real
    // climate also trap outgoing longwave.
    //
    // Cirrus magnitude is lapse-driven (any-planet backlog
    // T5): `cirrus_greenhouse_strength` evaluates
    // `(T_surface − T_cloud_top)^4 / ΔT_earth^4 × 15 K` per
    // cell, so a hotter cell at the same lapse + altitude
    // contributes more forcing — the Stefan-Boltzmann
    // surface-vs-cloud-top emission difference. Stratus
    // remains a constant: low-altitude clouds emit at
    // near-surface T and the per-planet variation is
    // dominated by composition rather than lapse.
    let cloud_gh_peak = match CloudType::from_byte(cloud_type_byte) {
        CloudType::Cirrus => {
            cirrus_greenhouse_strength(temp_prev, consts.lapse_rate, consts.cirrus_altitude)
        }
        CloudType::Stratus => consts.stratus_gh,
    };
    let cloud_gh = cloud_gh_peak * cloud_fraction.clamp01();
    // P0.6: saturating arithmetic so a hot seed whose
    // `vapour[i]` is at the Clausius-Clapeyron-driven cap
    // (`saturation_vapour_cap` peaks near ~4e7 at silicate-
    // world temperatures) doesn't panic on the
    // `vapour[i] * h2o_k` multiply or the four-way sum. The
    // subsequent `min(greenhouse_cap)` clamps the meaningful
    // contribution at the pressure-scaled cap (250 K at
    // Earth pressure, ~446 K at Venus pressure; see
    // `greenhouse_cap_scaled`).
    let v_term = vapour.saturating_mul(consts.h2o_k);
    let c_term = co2.saturating_mul(consts.co2_k);
    let m_term = ch4.saturating_mul(consts.ch4_k);
    let greenhouse_raw = v_term
        .saturating_add(c_term)
        .saturating_add(m_term)
        .saturating_add(cloud_gh);
    greenhouse_raw.min(consts.cap)
}
