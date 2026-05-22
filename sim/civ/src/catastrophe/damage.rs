//! Per-cell damage plumbing shared by every catastrophe kind:
//! the cell-conditions builder that feeds the tolerance gate
//! and the resistance + dormancy + seed-bank applicator that
//! turns a raw loss fraction into the realised population hit.

use crate::Civ;
use sim_arith::{Pop, Real};
use sim_physics::PhysicsState;
use sim_species::{apply_catastrophe_with_dormancy, Species};
use sim_world::Planet;

/// Per-catastrophe severity factor ∈ [0, 1] for the dormancy
/// damage-reduction formula. Sprint 2 Item 7b pins this at 1.0
/// (full-severity catastrophes) for all five kinds; a future
/// polish pass can expose a per-kind table if a follow-up wants
/// shallow events to bypass dormancy benefit. Centralised here so
/// the constant lives in one place.
const DORMANCY_SEVERITY_FACTOR: Real = Real::ONE;

/// Per-cell baseline radiation flux, Earth-surface units. Sits
/// well below the aqueous-default `radiation_max = 0.5` so a
/// quiet planet doesn't already saturate the radiation gate.
/// Catastrophe-specific deltas (solar flare, etc) are added on
/// top of this in the per-call-site cell-conditions builder.
pub(super) fn baseline_radiation_flux() -> Real {
    Real::from_ratio(1, 10)
}

/// Post-flare radiation magnitude added on top of the baseline
/// when a solar flare hits. Sits above the aqueous-default
/// `radiation_max = 0.5` so a narrow-envelope species takes the
/// full flare damage, while an extremophile with
/// `radiation_max ≥ 5` still has plenty of envelope headroom.
pub(super) fn solar_flare_radiation_boost() -> Real {
    Real::ONE
}

/// Post-impact radiation magnitude added on top of the baseline
/// for the ecosystem signature of an asteroid strike (T2). Set
/// well above the aqueous-default `radiation_max = 0.5` so a
/// narrow-envelope eco species takes the full hit while an
/// extremophile with high `radiation_max` retains envelope
/// headroom. Models the impactor's prompt gamma + activation
/// products at the strike site.
pub(super) fn asteroid_radiation_boost() -> Real {
    Real::from_int(5)
}

/// Drop in cell temperature applied when an ice age fires, in K.
/// Pushes the cell's read-out temperature below the aqueous
/// envelope's lower bound (273 K) for cold-baseline planets so
/// the temperature gate flags the catastrophe to a narrow-
/// envelope species.
pub(super) fn ice_age_temp_drop_k() -> Real {
    Real::from_int(50)
}

/// Pa per atm — conversion factor between `planet.surface_pressure`
/// (Pa) and the tolerance envelope's pressure range (atm).
fn pa_per_atm() -> Real {
    Real::from_int(101_325)
}

/// Build the `(temperature, pH, salinity, radiation, pressure)`
/// tuple a catastrophe-affected cell exposes to the tolerance
/// envelope. The hex grid only carries temperature + pressure
/// per cell; pH and salinity are derived from planet-level
/// substrate defaults (neutral pH, Earth-ocean-baseline salinity)
/// so the radiation/temperature axes drive the differential.
///
/// `temp_delta_k` adjusts the read-out temperature (negative for
/// ice age cold snap, zero otherwise). `extra_rad` adds to the
/// baseline radiation flux (positive for solar flare; pre-
/// multiplied by `cosmic_ray_ground_flux` at the call site,
/// clamped to `[0.2, 5.0]` in T8 so a magnetic-reversal window
/// amplifies post-flare ground flux while a strong stable dipole
/// attenuates it).
pub(super) fn catastrophe_cell_conditions(
    state: &PhysicsState,
    planet: &Planet,
    cell: usize,
    temp_delta_k: Real,
    extra_rad: Real,
) -> (Real, Real, Real, Real, Real) {
    let temp_slice = state.temperature();
    let pressure_slice = state.pressure();
    // Per-cell temperature with the catastrophe delta applied.
    // Fall back to the planet mean if the cell index is out of
    // range (defensive — callers always pass a valid index).
    let cell_t = temp_slice
        .get(cell)
        .copied()
        .unwrap_or(planet.mean_temperature);
    let t = (cell_t + temp_delta_k).max(Real::ZERO);
    // Neutral pH — no per-cell ocean-chemistry field yet. Pinned
    // to the centre of the aqueous envelope so the pH axis stays
    // a non-binding gate; substrate-specific pH biases land when
    // a richer ocean-chemistry field exists.
    let ph = Real::from_int(7);
    // Earth-ocean-baseline salinity (g/L). Sits inside every
    // substrate default's salinity range so this axis is non-
    // binding under default planets; a future per-cell salinity
    // field can plug in here.
    let sal = Real::from_int(20);
    // Radiation: baseline ground flux plus the event-specific
    // boost (already scaled by cosmic-ray amplification at the
    // call site for SolarFlare).
    let rad = baseline_radiation_flux() + extra_rad;
    // Pressure: prefer the per-cell state value if non-zero (Pa);
    // otherwise fall back to the planet's surface pressure (Pa).
    // Convert to atm for the tolerance envelope.
    let p_pa = pressure_slice
        .get(cell)
        .copied()
        .filter(|v| *v > Real::ZERO)
        .unwrap_or(planet.surface_pressure);
    let atm = pa_per_atm();
    let p = if atm > Real::ZERO {
        p_pa / atm
    } else {
        Real::ONE
    };
    (t, ph, sal, rad, p)
}

/// Wrap `apply_catastrophe_with_dormancy` with the per-civ
/// existing `apply_catastrophe_resistance` (tools soften the
/// blow), then the per-species dormancy reduction (tardigrade-
/// grade species shrug off catastrophes), then the per-species
/// `ToleranceEnvelope::match_score` so an extremophile shaped to
/// the affected cell's conditions rides out the catastrophe and
/// a narrow-envelope species takes the full hit. Tools first
/// preserves pre-existing behaviour for fixtures with
/// `dormancy = 0` and centre-of-envelope species.
///
/// Formula:
///   base_loss = raw_frac × (1 − civ_tool_resistance) × (1 − match_score)
///   after_dormancy = base_loss × (1 − dormancy × severity)
///
/// `match_score = 1.0` (perfect envelope fit) ⇒ zero damage;
/// `match_score = 0.0` (outside envelope) ⇒ full damage. The
/// returned fraction equals `after_dormancy`. Algebraically
/// identical to the pre-P1.3 form that applied `(1 − match_score)`
/// last; the rearrangement exposes `base_loss` (the loss fraction
/// the species would suffer without its dormancy trait) for the
/// seed-bank routing below.
///
/// **P1.3 — dormant-pool seeding:** in addition to returning the
/// realised loss fraction, this function deposits
/// `pop_before × base_loss × dormancy × severity` into
/// `civ.dormant_pool.population`. That's the fraction of the
/// would-be casualties — the people the catastrophe would have
/// killed without the species' dormancy trait — that enter
/// cryptobiosis instead of dying. `DormantPool::resurrect_step`
/// drains the reservoir back into the active cohort at 1%/tick
/// over the subsequent run. `pre_catastrophe_population` is also
/// bumped to track the largest active cohort ever observed so
/// the resurrection cap stays honest for civs that lose
/// population to multiple consecutive catastrophes.
pub(super) fn apply_resistance_and_dormancy(
    civ: &mut Civ,
    species: &Species,
    raw_frac: Real,
    cell: (Real, Real, Real, Real, Real),
    tick: u64,
) -> Real {
    let after_tools = civ.apply_catastrophe_resistance(raw_frac);
    let (t, ph, sal, rad, p) = cell;
    let survival_match = species.tolerance.match_score(t, ph, sal, rad, p);
    // `base_loss` is the loss fraction *before* the dormancy
    // reduction — i.e. the share of population that would die if
    // the species had no cryptobiosis trait. Tolerance and tool
    // resistance both fold into this; only the dormancy term is
    // separated out so we can route it into the seed bank.
    let base_loss = after_tools * (Real::ONE - survival_match);
    let after_dormancy = apply_catastrophe_with_dormancy(
        species.dormancy_capability,
        base_loss,
        DORMANCY_SEVERITY_FACTOR,
    );
    // Seed the dormant pool with the would-be casualties absorbed
    // by dormancy: `pop_before × base_loss × dormancy × severity`.
    // Equivalent to `pop_before × (base_loss − after_dormancy)` —
    // the headcount the dormancy multiplier just diverted out of
    // the death column.
    let pop_before = civ.cohort.total();
    // Track the high-water mark of the civ's active population so
    // `DormantPool::resurrect_step`'s cap reflects the largest
    // cohort the civ has ever held — not just the initial founder
    // population.
    if pop_before > civ.pre_catastrophe_population {
        civ.pre_catastrophe_population = pop_before;
    }
    let dormant_share = species.dormancy_capability * DORMANCY_SEVERITY_FACTOR;
    if dormant_share > Real::ZERO && pop_before > Pop::ZERO {
        let pop_before_real = pop_before.to_real_nonneg();
        let dormant_seeded = pop_before_real * base_loss * dormant_share;
        if dormant_seeded > Real::ZERO {
            civ.dormant_pool.population = civ.dormant_pool.population + dormant_seeded;
            civ.dormant_pool.entered_tick = tick;
        }
    }
    after_dormancy
}
