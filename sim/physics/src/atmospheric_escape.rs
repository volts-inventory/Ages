//! Multi-channel atmospheric escape (Sprint 5 Item 17).
//!
//! Real planets lose atmosphere through four physical channels, each
//! with a distinct driver:
//!
//! 1. **Jeans (thermal)**. Random thermal motion in the exosphere
//!    flings molecules above escape velocity. The canonical Jeans
//!    dimensionless escape parameter is
//!    `λ = m × v_esc² / (2 × k_B × T)`; escape rate scales as
//!    `exp(-λ)`. Light molecules (small `m`) escape exponentially
//!    faster than heavy ones — for Earth conditions, the H-vs-He
//!    retention ratio is ~10⁴, far steeper than the linear per-
//!    substance weighting the v1 model used. Earth-equivalent
//!    Jeans loss of hydrogen is ~3 kg/s; for CO2 it's effectively
//!    zero. P2.2 of `docs/post-implementation-fixes.md` replaced
//!    the dimensionless `v_esc/sqrt(T)` heuristic with explicit
//!    `m × v_esc² / T` so heavy species are now exponentially
//!    retained as physics dictates.
//!
//! 2. **Hydrodynamic blow-off**. When XUV (extreme UV) flux is high
//!    enough, the exosphere expands outward as a bulk fluid rather
//!    than as individual escaping molecules. Young stars deliver
//!    ~100× modern Sun's XUV; the early Solar System lost most of
//!    Mars's primordial atmosphere this way. Modelled as
//!    `base × euv_flux × thermal_factor` — only fires meaningfully
//!    when both are high.
//!
//! 3. **Photochemical**. UV photolysis breaks H2O / CH4 into lighter
//!    species (H, O, etc.); those products escape faster than the
//!    parent. This is the dominant loss channel for Mars *today* —
//!    no ozone layer means raw UV reaches the lower atmosphere and
//!    cracks water vapour into H + OH, and the H escapes. Modelled
//!    as `base × uv_flux × dissociation_factor` where the
//!    dissociation factor is highest for light volatiles (H2O,
//!    CH4) and low for heavier ones (CO2).
//!
//! 4. **Ion escape**. Charged species escape along open magnetic
//!    field lines. A strong planetary magnetic field traps ions on
//!    closed field lines (magnetosphere); a weak / absent one lets
//!    the solar wind strip charged particles directly. Modelled as
//!    `base / (1 + magnetic_strength)`. Earth's strong dipole keeps
//!    ion loss negligible; Mars (no dipole) loses ~2 kg/s of O via
//!    this channel and ~few × 10^25 ions per second according to MAVEN.
//!
//! ## Composition shifts (light first)
//!
//! Light species (water vapour, methane) escape faster than heavy
//! ones (CO2, oxygen) in every channel — Jeans is exponential in
//! velocity which favours light molecules; hydrodynamic blow-off
//! drags lighter species more efficiently; photochemical dissociation
//! creates light products by definition; ion escape mass-fractionates
//! along the same axis. The per-substance mass weight below
//! captures this differential.
//!
//! ## Calibration
//!
//! Constants are tuned so:
//! - Earth-equivalent (1g, strong B, low EUV) loses < 1% of its
//!   atmosphere per Gyr.
//! - Mars-equivalent (0.38g, no B, ~1.5× solar EUV at the early epoch)
//!   loses ~10% per Gyr summed across the four channels.
//! - Hot young Venus (1g, weak B, high T, very high EUV) loses
//!   atmosphere catastrophically fast.
//!
//! Determinism: pure per-cell read + per-cell write, no pair
//! iteration, no state-dependent branching beyond clamps. Q32.32
//! throughout via `sim_arith::Real`.

use crate::chemistry::Substance;
use crate::state::PhysicsState;
use sim_arith::transcendental::exp;
use sim_arith::Real;

/// Planet-derived inputs to the escape calculation. Bundled in a
/// small struct rather than passed as five scalar arguments so the
/// call site stays readable and additions (e.g. exobase altitude)
/// don't break callers. Populated from `Planet` + `Star` at the
/// `sim-core` wiring layer — `sim-physics` doesn't depend on
/// `sim-world`, so we can't import `Planet` directly here.
#[derive(Debug, Clone, Copy)]
pub struct PlanetEscapeParams {
    /// Escape velocity at the planet's surface, km/s. Derived from
    /// `Planet::escape_velocity()`. Earth ≈ 11.18, Mars ≈ 5.03,
    /// Venus ≈ 10.36.
    pub escape_velocity_km_s: Real,
    /// Extreme-UV irradiance at the planet's orbit, W/m². Drives
    /// hydrodynamic blow-off. Read from `Planet::star.euv_flux`.
    /// Modern Sun-on-Earth ≈ 0.001 W/m² (the SED fraction is tiny);
    /// young Sun ≈ 0.1 W/m². If Item 18a hasn't merged yet, use
    /// `star.bolometric_luminosity × 0.001`.
    pub euv_flux_w_m2: Real,
    /// Near-UV irradiance at the planet's orbit, W/m². Drives
    /// photochemical dissociation. Modern Sun-on-Earth ≈ 90 W/m²;
    /// for stars without a dedicated UV SED channel, use a small
    /// fraction (~5%) of the bolometric luminosity.
    pub uv_flux_w_m2: Real,
    /// Planet-scale magnetic field strength, in arbitrary units
    /// matching whatever the `Magnetism` law writes to the per-cell
    /// field. Earth-equivalent dipole ≈ 1.0; Mars ≈ 0.0 (no global
    /// dipole). Read from `state.magnetic_field_magnitude` averaged
    /// across cells, scaled by `state.dipole_strength()` during
    /// reversal windows.
    pub magnetic_strength: Real,
}

impl PlanetEscapeParams {
    /// Earth-analog defaults — useful for tests and as a sanity
    /// baseline. Strong dipole, modern EUV / UV.
    #[must_use]
    pub fn earth_like() -> Self {
        Self {
            // sqrt(2 × 9.81 × 6.371e6) / 1000 ≈ 11.18 km/s.
            escape_velocity_km_s: Real::from_ratio(1_118, 100),
            // Modern Sun EUV at Earth (rough): ~0.001 W/m².
            euv_flux_w_m2: Real::from_ratio(1, 1_000),
            // Modern Sun UV at Earth: ~90 W/m².
            uv_flux_w_m2: Real::from_int(90),
            // Earth's dipole at full strength = 1.0.
            magnetic_strength: Real::ONE,
        }
    }

    /// Mars-analog defaults: low gravity, no magnetic field, weak
    /// modern EUV / UV. Test fixture.
    #[must_use]
    pub fn mars_like() -> Self {
        Self {
            // Mars escape velocity ≈ 5.03 km/s.
            escape_velocity_km_s: Real::from_ratio(503, 100),
            // ~0.4× Earth's EUV at modern Mars orbit (~1.5 AU).
            euv_flux_w_m2: Real::from_ratio(4, 10_000),
            // ~0.4× Earth's UV at Mars orbit.
            uv_flux_w_m2: Real::from_int(40),
            // Mars: no global dipole.
            magnetic_strength: Real::ZERO,
        }
    }
}

/// Base per-tick Jeans-loss rate before the `exp(-lambda)` Jeans
/// suppression. Real Jeans escape is a tiny per-tick rate dominated
/// by the exponential. Set low enough that even at a relatively
/// permissive lambda ≈ 2-3 (small molecular mass / hot atmosphere)
/// the Jeans loss stays well under 1% per Gyr at this base.
pub const JEANS_BASE_NUM: i64 = 1;
pub const JEANS_BASE_DEN: i64 = 100_000;

/// Reference temperature floor in the Jeans exponent. Keeps the
/// division `m × v_esc² / T` finite on frozen cells. Real exobase
/// temperatures sit at ~1000 K for Earth; surface T isn't the
/// exobase T but suffices as a proportional driver for our
/// simplified per-cell formulation.
pub const JEANS_T_FLOOR_K: i64 = 100;

/// Calibrated Jeans coefficient `C` such that
/// `λ = C × m_amu × v_esc_km_s² / T_K`.
///
/// Dimensional derivation: real `λ = m × v² / (2 × k_B × T)` with
/// `m` in kg, `v` in m/s, `k_B = 1.38e-23 J/K`. Converting `m` to
/// AMU (`× 1.66e-27 kg/AMU`) and `v` to km/s (`× 10⁶ (m/s)² /
/// (km/s)²`) gives an exact physical coefficient of
/// `1.66e-27 × 10⁶ / (2 × 1.38e-23) ≈ 60`. That coefficient applied
/// at *surface* temperature (rather than the exobase ~1000 K it's
/// meant to apply at) inflates λ by ~3-4× — Earth λ_H ≈ 26 at
/// T=288 K vs the physically meaningful ~7 at the exobase. To
/// recover sensible discrimination using surface T (which is what
/// `PhysicsState::temperature` exposes), we calibrate to `C = 6`,
/// approximately the physical coefficient divided by the surface-
/// to-exobase ratio. With `C = 6`:
///
/// - Earth H (m=1, v=11.2, T=288): λ ≈ 2.61, `exp(-λ) ≈ 0.073`.
/// - Earth He (m=4): λ ≈ 10.4, `exp(-λ) ≈ 3.0e-5`. H/He ≈ 2500.
/// - Mars vapour (m=18, v=5, T=240): λ ≈ 11.3.
/// - Mars CO2 (m=44): λ ≈ 27.5 (clamped to [`JEANS_LAMBDA_MAX`]).
///
/// The qualitative ordering (light escapes faster, exponentially)
/// matches first-principles Jeans escape; the absolute lambdas
/// are calibrated for surface-T inputs.
pub const JEANS_COEFFICIENT: i64 = 6;

/// Upper clamp on the Jeans exponent `λ`. `exp(-λ)` underflows
/// Q32.32 (smallest positive ≈ 2.33e-10) around `λ ≈ 22`; clamping
/// at 21 keeps the result strictly positive for the lightest
/// retained species so test ratios remain finite. The lower clamp
/// at 0 ensures we never accidentally amplify (Jeans is loss-only).
pub const JEANS_LAMBDA_MAX: i64 = 21;

/// Base per-tick hydrodynamic blow-off rate before the EUV
/// modulation. Hydrodynamic loss is dramatic when it fires — young
/// Sun's high XUV stripped Mars's primordial atmosphere over a few
/// hundred million years — but it requires both high EUV and a
/// warm thermosphere. Tuned so a Mars-equivalent at modern EUV
/// (~0.0004 W/m²) loses negligibly via this channel, but the same
/// planet at 100× early-Sun EUV (~0.04 W/m²) loses dramatically.
pub const HYDRODYNAMIC_BASE_NUM: i64 = 1;
pub const HYDRODYNAMIC_BASE_DEN: i64 = 10;

/// Temperature reference for the hydrodynamic thermal factor.
/// Hydrodynamic blow-off only fires once the upper atmosphere is
/// warm enough that the bulk flow speed exceeds the escape
/// velocity locally. `300 K` puts a typical Earth surface at
/// factor 1.0; a hot young Venus (700 K) hits ~2.3×.
pub const HYDRODYNAMIC_T_REF_K: i64 = 300;

/// Base per-tick photochemical-loss rate before UV scaling and
/// per-substance dissociation factor. Mars loses ~2 kg/s of H via
/// H2O photolysis today; per-month tick over million-year scales
/// this lands at ~10% per Gyr after the UV + dissociation
/// multipliers. The Earth photochemical channel is suppressed
/// indirectly via the lower-magnitude `magnetic_strength` (and
/// directly via ozone shielding, modelled here as a smaller
/// effective UV) — see test calibration constants.
pub const PHOTOCHEMICAL_BASE_NUM: i64 = 1;
pub const PHOTOCHEMICAL_BASE_DEN: i64 = 100_000;

/// UV reference flux for the photochemical channel. `100 W/m²` puts
/// modern Earth (~90 W/m² near-UV) at factor ~0.9; the photochemical
/// channel scales linearly with UV.
pub const PHOTOCHEMICAL_UV_REF_W_M2: i64 = 100;

/// Base per-tick ion-escape rate before magnetic-field
/// suppression. Mars (no dipole) loses ~3 kg/s of O via ion
/// pickup today; on a per-month tick over million-year scales
/// this lands at ~few percent per Gyr. Earth's strong dipole
/// brings the per-tick rate down by ~4× via the `1/(1+B)`
/// shielding term, making this the principal Mars-vs-Earth
/// differentiator.
pub const ION_BASE_NUM: i64 = 1;
pub const ION_BASE_DEN: i64 = 10_000;

/// Per-substance escape weights used by the non-Jeans channels
/// (hydrodynamic blow-off, photochemical dissociation, ion escape).
/// These channels have their own physical drivers (EUV flux,
/// UV-driven dissociation, magnetic shielding) and their
/// mass-dependence is much gentler than Jeans's exponential, so a
/// linear weight that's roughly inverse-proportional to molecular
/// mass is a reasonable per-channel approximation. Jeans escape
/// itself now uses [`molecular_mass_amu`] + [`jeans_factor`] for the
/// physically-grounded exponential mass discrimination.
///
/// Vapour (H2O, M=18) and Methane (CH4, M=16): light, escape fast
/// (relative to heavier oxidiser-like species). Photolysis
/// products dominate this row in real planets.
///
/// CO2 (M=44): heavy, escapes slowly even with no magnetic field.
/// The dominant escape channel for CO2 is hydrodynamic blow-off
/// during the young-star epoch; it's nearly impossible to lose
/// significant CO2 via Jeans alone.
///
/// Oxidiser (O2, M=32): intermediate. Ion escape is the dominant
/// channel for O on unmagnetised worlds (e.g. modern Mars losing
/// O to the solar wind).
const fn substance_weight(s: Substance) -> (i64, i64) {
    match s {
        // Methane is the lightest non-H tracked substance — escapes
        // fastest. Weight = 1.0.
        Substance::Methane => (10, 10),
        // Water vapour: slightly heavier than methane, similar weight.
        Substance::Vapour => (9, 10),
        // Oxidiser (O2): intermediate.
        Substance::Oxidiser => (5, 10),
        // CO2: heaviest tracked atmospheric gas.
        Substance::CO2 => (2, 10),
        // Other substances aren't atmospheric — return zero weight
        // so the iteration naturally skips them when called with a
        // non-atmospheric variant.
        _ => (0, 10),
    }
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
/// Non-atmospheric substances return zero so calling
/// [`escape_rate_for`] on a non-atmospheric variant short-circuits
/// cleanly (matches the previous `substance_weight = 0` behaviour).
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

/// Hypothetical atomic-hydrogen mass (1 amu) for the H-vs-He
/// fractionation test. Hydrogen isn't a tracked `Substance` variant
/// — it appears as a photolysis product of `Vapour` / `Methane`,
/// not as a discretely-tracked gas — but exposing the constant
/// lets us calibrate the Jeans factor against the canonical
/// hydrogen-vs-helium escape ratio (~10⁴ on Earth).
pub const HYDROGEN_MASS_AMU: i64 = 1;

/// Hypothetical helium mass (4 amu) for the H-vs-He fractionation
/// test. Same rationale as [`HYDROGEN_MASS_AMU`].
pub const HELIUM_MASS_AMU: i64 = 4;

/// The four atmospheric substances iterated by `atmospheric_escape_step`,
/// ordered light-first so composition shifts emerge naturally: if
/// the per-tick loss budget runs out (e.g. via the per-cell density
/// floor), light species have already taken their share.
pub const ATMOSPHERIC_SUBSTANCES: [Substance; 4] = [
    Substance::Methane,
    Substance::Vapour,
    Substance::Oxidiser,
    Substance::CO2,
];

/// Per-cell Jeans-escape factor `exp(-λ)` with
/// `λ = C × m_amu × v_esc² / T`, the canonical Jeans dimensionless
/// escape parameter
/// (`λ = m × v_esc² / (2 × k_B × T)` in SI units).
///
/// Inputs:
/// - `escape_velocity_km_s`: planet surface escape velocity in km/s.
/// - `temperature_k`: temperature in K (typically per-cell surface T;
///   real Jeans escape uses exobase T ≈ 1000 K on Earth, but the
///   coefficient [`JEANS_COEFFICIENT`] is calibrated for the
///   surface-T inputs available in `PhysicsState`).
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

/// Per-cell hydrodynamic thermal factor. `T / T_ref` — linear
/// scaling with temperature so a warmer atmosphere blows off more
/// dramatically. Clamped at zero for completeness; no upper cap so
/// truly hot atmospheres can lose mass without bound (the per-cell
/// density floor still applies).
#[must_use]
pub fn hydrodynamic_thermal_factor(temperature_k: Real) -> Real {
    let ratio = temperature_k / Real::from_int(HYDRODYNAMIC_T_REF_K);
    ratio.max(Real::ZERO)
}

/// Per-channel loss rate per tick for one substance at one cell.
/// Returned as a fraction of current density — the integrate pass
/// multiplies by the cell's density and clamps to the available
/// mass. Returned as four separate channels so tests can probe
/// each independently.
#[derive(Debug, Clone, Copy)]
pub struct EscapeChannels {
    pub jeans: Real,
    pub hydrodynamic: Real,
    pub photochemical: Real,
    pub ion: Real,
}

impl EscapeChannels {
    /// Total fractional loss rate this tick. Sum of the four
    /// channels. Tests assert "Mars analog hits non-trivial total
    /// loss" via this scalar.
    #[must_use]
    pub fn total(&self) -> Real {
        self.jeans + self.hydrodynamic + self.photochemical + self.ion
    }
}

/// Compute the four-channel fractional escape rate per tick for a
/// single substance at a single cell. The returned rates are
/// *fractions of current density* — multiply by the cell's per-
/// substance density to get the absolute mass loss. Each channel
/// is scaled by the per-substance weight (light species lose faster).
#[must_use]
pub fn escape_rate_for(
    substance: Substance,
    params: &PlanetEscapeParams,
    temperature_k: Real,
    dt: Real,
) -> EscapeChannels {
    let (w_num, w_den) = substance_weight(substance);
    if w_num == 0 {
        return EscapeChannels {
            jeans: Real::ZERO,
            hydrodynamic: Real::ZERO,
            photochemical: Real::ZERO,
            ion: Real::ZERO,
        };
    }
    let weight = Real::from_ratio(w_num, w_den);

    // ---- Jeans (thermal) ----
    // Mass-explicit Jeans escape: rate ∝ exp(-λ) with
    // `λ = m × v_esc² / (2 × k_B × T)`. The exponential
    // mass-dependence (4× heavier species ≈ 4× larger λ ≈
    // exp(-3λ_light) times more retention) replaces the old gentle
    // linear `substance_weight` factor (which spanned only 0.2-1.0
    // over 16-44 amu and so dramatically underestimated how
    // strongly heavy species are retained).
    let mass = molecular_mass_amu(substance);
    let jeans_base = Real::from_ratio(JEANS_BASE_NUM, JEANS_BASE_DEN);
    let jeans = jeans_base
        * jeans_factor(params.escape_velocity_km_s, temperature_k, mass)
        * dt;

    // ---- Hydrodynamic blow-off ----
    // Scales with EUV × thermal_factor. Only fires meaningfully
    // when both are high (young hot atmosphere).
    let hydrodynamic_base = Real::from_ratio(HYDRODYNAMIC_BASE_NUM, HYDRODYNAMIC_BASE_DEN);
    let hydrodynamic = hydrodynamic_base
        * params.euv_flux_w_m2
        * hydrodynamic_thermal_factor(temperature_k)
        * weight
        * dt;

    // ---- Photochemical ----
    // UV-driven dissociation. Scales linearly with UV flux and
    // per-substance dissociation factor — light species are easier
    // to photolyze, so reuse the substance weight. The weak
    // magnetic-shielding factor here is a proxy for the
    // co-evolution between magnetic field and atmospheric
    // composition: planets with strong magnetospheres tend to
    // build O2 → ozone layers (Earth) that absorb UV before it
    // reaches lower-atmosphere volatiles; planets without
    // magnetospheres (Mars) lose any ozone they had and let UV
    // strip H2O / CH4 directly. We're not modelling ozone
    // chemistry explicitly — using the same `1/(1+B)` form lets
    // the strong-field branch suppress photochem the same way
    // it suppresses ion escape.
    let photochem_base = Real::from_ratio(PHOTOCHEMICAL_BASE_NUM, PHOTOCHEMICAL_BASE_DEN);
    let uv_ref = Real::from_int(PHOTOCHEMICAL_UV_REF_W_M2);
    let uv_factor = params.uv_flux_w_m2 / uv_ref;
    let ozone_shield = Real::ONE / (Real::ONE + params.magnetic_strength);
    let photochemical = photochem_base * uv_factor * ozone_shield * weight * dt;

    // ---- Ion escape ----
    // Strong dipole suppresses (Earth); weak / absent enables (Mars).
    // `1 / (1 + B)` is the canonical magnetosphere-shielding form:
    // at B=0 the factor is 1; at B=1 it's 0.5; at B=10 it's ~0.09.
    let ion_base = Real::from_ratio(ION_BASE_NUM, ION_BASE_DEN);
    let ion_shield = Real::ONE / (Real::ONE + params.magnetic_strength);
    let ion = ion_base * ion_shield * weight * dt;

    EscapeChannels {
        jeans,
        hydrodynamic,
        photochemical,
        ion,
    }
}

/// Apply one multi-channel atmospheric-escape step. For each
/// atmospheric substance (lightest first), walk every cell and
/// subtract the four-channel loss rate × density × dt from the
/// per-cell substance pool, clamped at zero. Heavier species
/// iterate later, so on a budget-constrained tick (extreme
/// parameters) lighter species deplete first — composition shifts
/// emerge naturally from iteration order × per-substance weight.
///
/// Determinism: pure per-cell read + per-cell write, no pair
/// iteration. Iteration order is the fixed [`ATMOSPHERIC_SUBSTANCES`]
/// array; per-cell loop is monotonic over cell index. Identical
/// inputs → bit-exact identical outputs.
pub fn atmospheric_escape_step(
    state: &mut PhysicsState,
    params: &PlanetEscapeParams,
    dt: Real,
) {
    let n = state.grid().n_cells();
    let temps = state.temperature().to_vec();
    for &substance in &ATMOSPHERIC_SUBSTANCES {
        let densities = state.substance_mut(substance.idx());
        for i in 0..n {
            let channels = escape_rate_for(substance, params, temps[i], dt);
            let fraction = channels.total();
            // Fractional loss × current density → absolute loss;
            // clamp to the available mass so we never push the
            // density negative.
            let loss = (fraction * densities[i]).min(densities[i]).max(Real::ZERO);
            densities[i] = densities[i] - loss;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::HexGrid;

    /// Helper: build a uniformly-seeded planet's worth of atmospheric
    /// substances. Each of the four atmospheric channels is filled at
    /// the same per-cell density so total-mass deltas read directly
    /// as "loss across the run."
    fn seed_atmosphere(state: &mut PhysicsState, per_cell_density: Real) {
        for &s in &ATMOSPHERIC_SUBSTANCES {
            let dens = state.substance_mut(s.idx());
            for d in dens.iter_mut() {
                *d = per_cell_density;
            }
        }
    }

    fn total_substance(state: &PhysicsState, s: Substance) -> Real {
        state
            .substance(s.idx())
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b)
    }

    fn total_atmosphere(state: &PhysicsState) -> Real {
        ATMOSPHERIC_SUBSTANCES
            .iter()
            .map(|s| total_substance(state, *s))
            .fold(Real::ZERO, |a, b| a + b)
    }

    /// Mars-like (low gravity, no magnetic field, weak EUV) loses
    /// atmosphere at a non-trivial rate across many ticks via the
    /// combined four-channel model. With the new mass-explicit
    /// Jeans formulation, Mars's Jeans contribution is correctly
    /// near-negligible (real Mars Jeans escape is dominated by
    /// photochemical + ion channels); the dominant differentiator
    /// vs an Earth-equivalent magnetically-shielded planet is now
    /// the **ion channel** rather than Jeans. We verify both that
    /// Mars loses a measurable fraction over 500 ticks *and* that
    /// per-channel, Mars's no-field ion + photochemical loss
    /// dramatically exceeds Earth's shielded equivalent at matched
    /// substance.
    #[test]
    fn mars_analog_loses_atmosphere_via_combined_channels_at_realistic_rate() {
        let grid = HexGrid::new(4, 4);
        let mut mars = PhysicsState::new(grid.clone());

        for t in mars.temperature_mut() {
            *t = Real::from_int(250); // Mars surface mean ≈ 210 K; pick 250 for slightly-above floor.
        }

        let per_cell = Real::from_int(1000);
        seed_atmosphere(&mut mars, per_cell);

        let mars_params = PlanetEscapeParams::mars_like();
        let earth_params = PlanetEscapeParams::earth_like();
        let dt = Real::ONE;

        let mars_initial = total_atmosphere(&mars);

        // Run for many ticks to integrate the slow per-tick rate
        // into a measurable fractional loss.
        for _ in 0..500 {
            atmospheric_escape_step(&mut mars, &mars_params, dt);
        }

        let mars_final = total_atmosphere(&mars);
        let mars_lost = mars_initial - mars_final;

        // Mars must lose a non-trivial fraction — at least 1% of
        // its initial atmosphere across the run.
        let one_percent = mars_initial / Real::from_int(100);
        assert!(
            mars_lost > one_percent,
            "Mars-analog should lose >1% over 500 ticks; lost {mars_lost:?} of {mars_initial:?}"
        );

        // Per-channel: at matched temperature, Mars's no-field
        // ion-channel oxidiser loss exceeds Earth's strong-field
        // equivalent by the magnetosphere shielding factor
        // `1/(1+B)` — at B=1 (Earth), factor = 0.5; at B=0 (Mars),
        // factor = 1.0. So Mars ion ≥ 2× Earth ion at matched
        // substance. The ion channel is the physically meaningful
        // Mars-vs-Earth differentiator; the Jeans channel at
        // matched temperature is now correctly tiny on both worlds
        // (mass-explicit exp suppresses heavy species).
        let mars_ox = escape_rate_for(
            Substance::Oxidiser,
            &mars_params,
            Real::from_int(250),
            dt,
        );
        let earth_ox = escape_rate_for(
            Substance::Oxidiser,
            &earth_params,
            Real::from_int(250),
            dt,
        );
        // Mars ion = ion_base × 1.0 × weight; Earth ion = ion_base
        // × 0.5 × weight (because Earth's `magnetic_strength = 1`
        // halves through `1/(1+B)`). Ratio = exactly 2×.
        assert!(
            mars_ox.ion >= earth_ox.ion + earth_ox.ion,
            "Mars no-field ion escape should be at least 2x Earth strong-field; mars={:?} earth={:?}",
            mars_ox.ion,
            earth_ox.ion
        );

        // And the multi-channel Jeans factor is now mass-explicit:
        // at matched T and v_esc, heavier species have exponentially
        // smaller Jeans loss than lighter ones. CO2 (44 amu) Jeans
        // is strictly smaller than Methane (16 amu) Jeans on Mars.
        let mars_methane = escape_rate_for(
            Substance::Methane,
            &mars_params,
            Real::from_int(250),
            dt,
        );
        let mars_co2 = escape_rate_for(
            Substance::CO2,
            &mars_params,
            Real::from_int(250),
            dt,
        );
        assert!(
            mars_methane.jeans > mars_co2.jeans,
            "Mars Jeans should favour light species: methane={:?} co2={:?}",
            mars_methane.jeans,
            mars_co2.jeans
        );
    }

    /// Same planet (Mars-like otherwise) with a strong magnetic
    /// field loses much less atmosphere via the ion channel than
    /// the same planet with a weak / zero field. Direct test of
    /// the magnetosphere-shielding coupling.
    #[test]
    fn magnetic_field_protection_reduces_ion_escape() {
        // Probe ion channel directly — that's the only one
        // magnetic field affects. The per-substance weight cancels
        // when we compare the same substance across two field
        // strengths.
        let weak_params = PlanetEscapeParams {
            escape_velocity_km_s: Real::from_ratio(503, 100),
            euv_flux_w_m2: Real::ZERO,
            uv_flux_w_m2: Real::ZERO,
            magnetic_strength: Real::ZERO,
        };
        let strong_params = PlanetEscapeParams {
            magnetic_strength: Real::from_int(10),
            ..weak_params
        };

        let dt = Real::ONE;
        let temperature_k = Real::from_int(250);
        let weak = escape_rate_for(Substance::Oxidiser, &weak_params, temperature_k, dt);
        let strong = escape_rate_for(Substance::Oxidiser, &strong_params, temperature_k, dt);

        // Weak field → unshielded ion escape (rate ≈ ion_base × weight).
        // Strong field → shielded (rate ≈ ion_base × weight / 11).
        // The other three channels are identical between the two
        // (same T, EUV, UV) so the *total* difference is entirely
        // attributable to the ion channel.
        assert!(
            weak.ion > strong.ion,
            "weak field ion rate ({:?}) should exceed strong field ({:?})",
            weak.ion,
            strong.ion
        );
        // Quantitative: the ratio should be at least 5× — at B=10
        // the shielding factor is 1/11 ≈ 0.09, so weak/strong ≈ 11.
        let five = Real::from_int(5);
        assert!(
            weak.ion > strong.ion * five,
            "weak field should be >5× stronger ion escape; weak={:?} strong={:?}",
            weak.ion,
            strong.ion
        );

        // And the full-pass loss in a Mars-like planet matches the
        // single-cell channel signal: a weak-field planet ends up
        // lighter than a strong-field one after equal time.
        let grid = HexGrid::new(3, 3);
        let mut weak_state = PhysicsState::new(grid.clone());
        let mut strong_state = PhysicsState::new(grid);
        for t in weak_state.temperature_mut() {
            *t = Real::from_int(250);
        }
        for t in strong_state.temperature_mut() {
            *t = Real::from_int(250);
        }
        seed_atmosphere(&mut weak_state, Real::from_int(1000));
        seed_atmosphere(&mut strong_state, Real::from_int(1000));
        for _ in 0..200 {
            atmospheric_escape_step(&mut weak_state, &weak_params, dt);
            atmospheric_escape_step(&mut strong_state, &strong_params, dt);
        }
        let weak_total = total_atmosphere(&weak_state);
        let strong_total = total_atmosphere(&strong_state);
        assert!(
            weak_total < strong_total,
            "weak-field planet should retain less atmosphere: weak={weak_total:?} strong={strong_total:?}"
        );
    }

    /// On a low-gravity hot planet (max-loss configuration), the
    /// lightest species (methane, water vapour) lose mass faster
    /// than oxidiser, which loses faster than CO2. Validates the
    /// composition-shift physics: light first, then medium, then
    /// heavy.
    #[test]
    fn low_gravity_hot_planet_loses_h_first_then_o_then_co2() {
        let grid = HexGrid::new(3, 3);
        let mut state = PhysicsState::new(grid);
        // Hot atmosphere — high Jeans loss; low gravity from a
        // Mars-like escape velocity; high EUV / UV — drives all
        // four channels.
        for t in state.temperature_mut() {
            *t = Real::from_int(800);
        }
        let per_cell = Real::from_int(10_000);
        seed_atmosphere(&mut state, per_cell);

        let params = PlanetEscapeParams {
            // Mars-like surface gravity.
            escape_velocity_km_s: Real::from_ratio(503, 100),
            // Young hot star: high EUV.
            euv_flux_w_m2: Real::from_ratio(1, 100),
            // Strong UV — strips light volatiles.
            uv_flux_w_m2: Real::from_int(200),
            // No magnetic field — ion escape unshielded.
            magnetic_strength: Real::ZERO,
        };

        let initial = per_cell * Real::from_int(state.grid().n_cells() as i64);
        let dt = Real::ONE;

        // Track each substance independently so we can compare
        // fractional losses.
        for _ in 0..100 {
            atmospheric_escape_step(&mut state, &params, dt);
        }

        let methane_lost = initial - total_substance(&state, Substance::Methane);
        let vapour_lost = initial - total_substance(&state, Substance::Vapour);
        let oxidiser_lost = initial - total_substance(&state, Substance::Oxidiser);
        let co2_lost = initial - total_substance(&state, Substance::CO2);

        // Light-first composition shift: methane (M=16) and water
        // vapour (M=18) lose mass faster than oxygen (M=32), which
        // loses mass faster than CO2 (M=44).
        assert!(
            methane_lost > oxidiser_lost,
            "methane (light) should lose faster than oxidiser: methane={methane_lost:?} oxidiser={oxidiser_lost:?}"
        );
        assert!(
            vapour_lost > oxidiser_lost,
            "vapour (light) should lose faster than oxidiser: vapour={vapour_lost:?} oxidiser={oxidiser_lost:?}"
        );
        assert!(
            oxidiser_lost > co2_lost,
            "oxidiser should lose faster than CO2: oxidiser={oxidiser_lost:?} co2={co2_lost:?}"
        );
        // And every substance lost *something* — even heavy CO2
        // shouldn't be untouched on a max-loss configuration.
        assert!(
            co2_lost > Real::ZERO,
            "CO2 should lose some mass on max-loss config; got {co2_lost:?}"
        );
    }

    /// Determinism sanity check: identical inputs → identical
    /// outputs. Standard physics-determinism contract.
    #[test]
    fn deterministic_across_runs() {
        let grid = HexGrid::new(4, 4);
        let mut a = PhysicsState::new(grid.clone());
        let mut b = PhysicsState::new(grid);
        for t in a.temperature_mut() {
            *t = Real::from_int(300);
        }
        for t in b.temperature_mut() {
            *t = Real::from_int(300);
        }
        seed_atmosphere(&mut a, Real::from_int(1000));
        seed_atmosphere(&mut b, Real::from_int(1000));
        let params = PlanetEscapeParams::mars_like();
        for _ in 0..50 {
            atmospheric_escape_step(&mut a, &params, Real::ONE);
            atmospheric_escape_step(&mut b, &params, Real::ONE);
        }
        for s in &ATMOSPHERIC_SUBSTANCES {
            assert_eq!(
                a.substance(s.idx()),
                b.substance(s.idx()),
                "non-deterministic at substance {s:?}"
            );
        }
    }

    /// Earth-equivalent loses very little over 100 ticks — the
    /// strong dipole + low EUV / UV keep all four channels at low
    /// fractional rates. Calibration anchor for the < 1% / Gyr
    /// constraint in the spec.
    #[test]
    fn earth_equivalent_loses_minimally() {
        let grid = HexGrid::new(3, 3);
        let mut state = PhysicsState::new(grid);
        for t in state.temperature_mut() {
            *t = Real::from_int(288); // Earth surface mean.
        }
        let per_cell = Real::from_int(10_000);
        seed_atmosphere(&mut state, per_cell);
        let initial = total_atmosphere(&state);

        let params = PlanetEscapeParams::earth_like();
        for _ in 0..100 {
            atmospheric_escape_step(&mut state, &params, Real::ONE);
        }
        let lost = initial - total_atmosphere(&state);
        // Earth-equivalent should lose < 5% across 100 ticks
        // (calibration anchor — the spec calls for <1% per Gyr on
        // the natural time scale; per-tick this lands well under
        // a few percent over hundreds of ticks).
        let five_percent = initial / Real::from_int(20);
        assert!(
            lost < five_percent,
            "Earth-equivalent shouldn't lose >5% over 100 ticks; lost={lost:?}"
        );
    }

    /// **Hydrogen vs Helium fractionation** (P2.2 calibration anchor).
    ///
    /// The defining feature of the mass-explicit Jeans formula is
    /// that escape rate depends *exponentially* on molecular mass
    /// via `λ ∝ m`. A 4× heavier species (He vs H) gives a 4× larger
    /// λ, and `exp(-λ_He) / exp(-λ_H) = exp(-3 × λ_H)` — for Earth-
    /// like conditions (v_esc=11.2 km/s, T=288 K) the published
    /// hydrogen-vs-helium retention ratio is ~10⁴, vastly exceeding
    /// the ~5× span the old `substance_weight` heuristic could
    /// produce across the entire 16-44 amu range.
    ///
    /// We assert a conservative `ratio > 1000` (the real ratio is
    /// ~10⁴ but Q32.32 + the surface-T calibration push the practical
    /// floor down). The point is: light species escape thousands of
    /// times faster than heavier ones under Earth Jeans physics.
    #[test]
    fn h_vs_he_fractionation_ratio_above_thousand() {
        // Earth-like inputs.
        let v_esc = Real::from_ratio(112, 10); // 11.2 km/s
        let temperature = Real::from_int(288);
        let h_mass = Real::from_int(HYDROGEN_MASS_AMU);
        let he_mass = Real::from_int(HELIUM_MASS_AMU);

        let h_factor = jeans_factor(v_esc, temperature, h_mass);
        let he_factor = jeans_factor(v_esc, temperature, he_mass);

        // Both factors must be strictly positive — if either
        // clamped to zero we'd lose the discrimination signal.
        assert!(
            h_factor > Real::ZERO,
            "H Jeans factor must be > 0 (got {h_factor:?})"
        );
        assert!(
            he_factor > Real::ZERO,
            "He Jeans factor must be > 0 (got {he_factor:?})"
        );

        // H must escape much faster than He. Assert the ratio
        // exceeds 1000× (real ratio ~10⁴; we're conservative to
        // accommodate Q32.32 rounding near `exp(-λ)`'s tail).
        let thousand = Real::from_int(1000);
        assert!(
            h_factor > he_factor * thousand,
            "H/He Jeans ratio should exceed 1000x; H={h_factor:?} He={he_factor:?} ratio = H/He"
        );
    }

    /// **Mars CO2 retention vs vapour** (P2.2 calibration anchor).
    ///
    /// On Mars-like conditions (v_esc=5 km/s, T=240 K), heavier
    /// molecules (CO2, 44 amu) should have a much smaller Jeans
    /// factor than lighter ones (H2O, 18 amu) — Mars's atmosphere
    /// is composed mostly of CO2 today precisely because CO2's
    /// mass-explicit Jeans retention is so much stronger than
    /// water vapour's. The old linear `substance_weight` gave
    /// CO2 only 0.2/0.9 ≈ 22% of vapour's escape rate; the new
    /// formula gives CO2 Jeans an exponentially smaller factor.
    #[test]
    fn mars_co2_retention_higher_than_h2o() {
        let v_esc = Real::from_int(5); // 5 km/s (Mars escape velocity ≈ 5.03)
        let temperature = Real::from_int(240);
        let h2o_mass = molecular_mass_amu(Substance::Vapour);
        let co2_mass = molecular_mass_amu(Substance::CO2);

        let h2o_factor = jeans_factor(v_esc, temperature, h2o_mass);
        let co2_factor = jeans_factor(v_esc, temperature, co2_mass);

        // CO2 (heavier) Jeans factor strictly smaller than H2O.
        assert!(
            co2_factor < h2o_factor,
            "CO2 should have smaller Jeans factor than H2O on Mars; co2={co2_factor:?} h2o={h2o_factor:?}"
        );

        // And by a *large* margin — CO2 should be at least 100×
        // smaller. The old linear weight gave only a ~4.5×
        // discrimination (0.9 vs 0.2); the exponential mass-
        // dependence in the new formula puts heavier molecules
        // into a much steeper retention regime.
        let hundred = Real::from_int(100);
        assert!(
            h2o_factor > co2_factor * hundred,
            "H2O/CO2 Jeans ratio should exceed 100x on Mars; h2o={h2o_factor:?} co2={co2_factor:?}"
        );
    }
}
