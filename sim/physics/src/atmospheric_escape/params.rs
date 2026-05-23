//! Planet-level escape inputs, MAVEN absolute-rate calibration, and
//! the per-channel base/reference constants.
//!
//! Splitting these out of the four channel files keeps every tunable
//! "magic number" in one place: tweaking, e.g., the photochemical
//! base rate doesn't require opening `photochemical.rs`, and the
//! MAVEN calibration scale that multiplies every channel lives next
//! to the per-channel bases it modulates. The functions in
//! `jeans.rs`, `hydrodynamic.rs`, `photochemical.rs`, and `ion.rs`
//! consume these constants as named imports.

use crate::chemistry::Substance;
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

/// MAVEN absolute-rate calibration scale (C2 calibration fix).
///
/// The four `*_BASE_NUM / *_BASE_DEN` constants below were originally
/// tuned for *fractional* per-tick loss anchors (Earth <5%/100 ticks,
/// Mars >1%/500 ticks, ratio-based H/He / CO2-vs-H2O tests). Converting
/// those per-tick fractions into absolute kg/s rates via Mars's
/// surface area + atmospheric column mass + 1-month tick duration
/// produced numbers ~4-5 orders of magnitude *above* MAVEN's measured
/// Mars ion + photochemical escape (~2-3 kg/s per channel; Jakosky et
/// al. 2018, Lillis et al. 2017).
///
/// The cleanest fix is a single shared scale applied to every channel's
/// base rate: `BASE_effective = (NUM / DEN) × MAVEN_CALIBRATION_SCALE`.
/// All relative-loss tests (which compare *ratios* between channels,
/// species, or planets) are invariant under a uniform multiplicative
/// scale; only the absolute-kg/s comparison shifts. With
/// `MAVEN_CALIBRATION_SCALE = 1 / 10_000`:
///
/// - Ion (oxidiser) at Mars: produced ≈ 4.4e5 × 1e-4 ≈ 44 kg/s.
/// - Photochemical (vapour) at Mars: ≈ 3.2e4 × 1e-4 ≈ 3.2 kg/s.
///
/// Both land comfortably inside the one-order-of-magnitude envelope
/// around MAVEN's ~3 kg/s literature value. Earth-equivalent loss
/// stays even smaller than before (the scale only shrinks rates),
/// keeping the < 5% / 100 ticks anchor intact.
pub const MAVEN_CALIBRATION_SCALE_NUM: i64 = 1;
pub const MAVEN_CALIBRATION_SCALE_DEN: i64 = 10_000;

/// Base per-tick Jeans-loss rate before the `exp(-lambda)` Jeans
/// suppression. Real Jeans escape is a tiny per-tick rate dominated
/// by the exponential. Set low enough that even at a relatively
/// permissive lambda ≈ 2-3 (small molecular mass / hot atmosphere)
/// the Jeans loss stays well under 1% per Gyr at this base.
///
/// Applied scale: `JEANS_BASE_NUM / JEANS_BASE_DEN ×
/// MAVEN_CALIBRATION_SCALE` — see [`MAVEN_CALIBRATION_SCALE_NUM`].
pub const JEANS_BASE_NUM: i64 = 1;
pub const JEANS_BASE_DEN: i64 = 100_000;

/// Base per-tick hydrodynamic blow-off rate before the EUV
/// modulation. Hydrodynamic loss is dramatic when it fires — young
/// Sun's high XUV stripped Mars's primordial atmosphere over a few
/// hundred million years — but it requires both high EUV and a
/// warm thermosphere. Tuned so a Mars-equivalent at modern EUV
/// (~0.0004 W/m²) loses negligibly via this channel, but the same
/// planet at 100× early-Sun EUV (~0.04 W/m²) loses dramatically.
///
/// Applied scale: `HYDRODYNAMIC_BASE_NUM / HYDRODYNAMIC_BASE_DEN ×
/// MAVEN_CALIBRATION_SCALE` — see [`MAVEN_CALIBRATION_SCALE_NUM`].
pub const HYDRODYNAMIC_BASE_NUM: i64 = 1;
pub const HYDRODYNAMIC_BASE_DEN: i64 = 10;

/// Base per-tick photochemical-loss rate before UV scaling and
/// per-substance dissociation factor. Mars loses ~2 kg/s of H via
/// H2O photolysis today; per-month tick over million-year scales
/// this lands at ~10% per Gyr after the UV + dissociation
/// multipliers. The Earth photochemical channel is suppressed
/// indirectly via the lower-magnitude `magnetic_strength` (and
/// directly via ozone shielding, modelled here as a smaller
/// effective UV) — see test calibration constants.
///
/// Applied scale: `PHOTOCHEMICAL_BASE_NUM / PHOTOCHEMICAL_BASE_DEN ×
/// MAVEN_CALIBRATION_SCALE` — see [`MAVEN_CALIBRATION_SCALE_NUM`].
pub const PHOTOCHEMICAL_BASE_NUM: i64 = 1;
pub const PHOTOCHEMICAL_BASE_DEN: i64 = 100_000;

/// Base per-tick ion-escape rate before magnetic-field
/// suppression. Mars (no dipole) loses ~3 kg/s of O via ion
/// pickup today; on a per-month tick over million-year scales
/// this lands at ~few percent per Gyr. Earth's strong dipole
/// brings the per-tick rate down by ~4× via the `1/(1+B)`
/// shielding term, making this the principal Mars-vs-Earth
/// differentiator.
///
/// Applied scale: `ION_BASE_NUM / ION_BASE_DEN ×
/// MAVEN_CALIBRATION_SCALE` — see [`MAVEN_CALIBRATION_SCALE_NUM`].
pub const ION_BASE_NUM: i64 = 1;
pub const ION_BASE_DEN: i64 = 10_000;

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

/// Per-substance escape weights used by the non-Jeans channels
/// (hydrodynamic blow-off, photochemical dissociation, ion escape).
/// These channels have their own physical drivers (EUV flux,
/// UV-driven dissociation, magnetic shielding) and their
/// mass-dependence is much gentler than Jeans's exponential, so a
/// linear weight that's roughly inverse-proportional to molecular
/// mass is a reasonable per-channel approximation. Jeans escape
/// itself uses `molecular_mass_amu` + `jeans_factor` for the
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
pub(crate) const fn substance_weight(s: Substance) -> (i64, i64) {
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
