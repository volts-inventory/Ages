//! Per-kind firing predicates. Each `*_fires` returns whether the
//! corresponding catastrophe should trigger this tick — the
//! orchestrator in `mod.rs` then combines per-kind cooldowns +
//! triggers into a single `check_and_apply` decision.

use crate::Civ;
use sim_arith::{Pop, Real};
use sim_physics::PhysicsState;
use sim_world::{Magnetosphere, Planet};

use super::DISEASE_AGE_FLOOR_TICKS;

/// Volcanic trigger: any cell with `|charge| > 80` AND
/// `temperature > 600 K` flags an eruption. The combination of
/// extreme charge and extreme temperature is a tectonically-active
/// proxy under M3 physics (lightning-discharge zones near hot
/// spots).
pub(super) fn volcanic_fires(state: &PhysicsState) -> Option<usize> {
    let n = state.grid().n_cells();
    let charge_threshold = Real::from_int(80);
    let temp_threshold = Real::from_int(600);
    (0..n).find(|&i| {
        state.charge()[i].abs() > charge_threshold && state.temperature()[i] > temp_threshold
    })
}

/// Disease trigger: civ population > 80% of carrying capacity
/// AND civ has been continuously active for at least
/// `DISEASE_AGE_FLOOR_TICKS` (stretched by substrate metabolism so
/// a slow-substrate civ doesn't get plague-immune just because the
/// floor was calibrated against aqueous time).
pub(super) fn disease_fires(civ: &Civ, state: &PhysicsState, planet: &Planet, tick: u64) -> bool {
    let cap = civ.carrying_capacity(state);
    if cap <= Pop::ZERO {
        return false;
    }
    let crowding: Real = civ.cohort.total() / cap;
    if crowding < Real::from_ratio(8, 10) {
        return false;
    }
    let age_floor = crate::demographics::streak_ticks_for_metabolism(
        DISEASE_AGE_FLOOR_TICKS,
        planet.metabolic_substrate.metabolism(),
    );
    tick.saturating_sub(civ.founded_tick) >= age_floor
}

/// Probabilistic asteroid trigger. `tick` modulo a prime gives a
/// deterministic pseudo-random firing window roughly every
/// `ASTEROID_COOLDOWN_TICKS` ticks; combined with the cooldown gate
/// this lands an asteroid every ~5000-10000 ticks on a long-lived
/// civ. Cheap, deterministic, no per-tick RNG needed.
pub(super) fn asteroid_fires(tick: u64) -> bool {
    // Prime-number period gives a non-aliased firing pattern.
    // ×12 so the year-equivalent recurrence matches the old
    // yearly cadence under 1 tick = 1 month.
    tick > 0 && tick.is_multiple_of(4733 * protocol::MONTHS_PER_YEAR)
}

/// Solar flare trigger: planet has weak/none magnetosphere AND
/// stellar luminosity is high (above ~Earth's 1361 W/m²). Such
/// planets are EM-vulnerable; flare disrupts atmosphere + tools.
/// Probabilistic firing window keyed off tick (deterministic).
///
/// T18: the firing period is scaled by the host star's spectral-
/// class flare-rate multiplier (`Star::flare_rate_per_tick`).
/// M dwarfs (rate 100×) collapse the period to `base / 100`
/// (~188 ticks), K dwarfs (10×) to ~1880, G dwarfs (1×) keep the
/// base ~18804, F dwarfs (0.3×) stretch to ~62680, and A dwarfs
/// (0.1×) to ~188040. The per-flare cooldown
/// (`SOLAR_FLARE_COOLDOWN_TICKS = 9600`) caps the realised
/// frequency for the highly active classes, but the cadence
/// between cooldown windows is now spectral-aware: an M dwarf
/// hits every cooldown, a G dwarf hits roughly every other,
/// and an A dwarf rarely fires at all. This is the wiring
/// that lets a habitable-zone M-dwarf planet feel the "100×
/// flares" of Item 18 in the civ catastrophe stream.
pub(super) fn solar_flare_fires(planet: &Planet, tick: u64) -> bool {
    if !matches!(
        planet.magnetosphere,
        Magnetosphere::None | Magnetosphere::Weak
    ) {
        return false;
    }
    if planet.stellar_luminosity < Real::from_int(1500) {
        return false;
    }
    // Base period (G-dwarf calibration): 1567 years × MONTHS_PER_YEAR.
    let base_period = 1567 * protocol::MONTHS_PER_YEAR;
    // Per-spectral rate divides the period — higher rate ⇒ shorter
    // period ⇒ more frequent firings. The per-class rates
    // (`SpectralType::flare_rate_per_tick`) are rationals (100, 10,
    // 1, 0.3, 0.1); reading the class directly here keeps the
    // arithmetic in `u64` and avoids Q32.32 round-trips that would
    // truncate the sub-1× F/A dwarfs.
    use sim_world::SpectralType;
    let period = match planet.star.spectral_type {
        SpectralType::M => base_period / 100,
        SpectralType::K => base_period / 10,
        SpectralType::G => base_period,
        // F dwarf: 0.3× rate → 1/0.3 ≈ 3.33× the base period.
        SpectralType::F => (base_period * 10) / 3,
        // A dwarf: 0.1× rate → 10× the base period.
        SpectralType::A => base_period * 10,
    };
    tick > 0 && tick.is_multiple_of(period.max(1))
}

/// Ice age trigger: planet's mean temperature is below 260 K AND
/// the civ has been alive long enough for a multi-tick climate
/// excursion to bite. Probabilistic firing keyed off tick.
pub(super) fn ice_age_fires(planet: &Planet, civ: &Civ, tick: u64) -> bool {
    if planet.mean_temperature > Real::from_int(260) {
        return false;
    }
    if tick.saturating_sub(civ.founded_tick) < 1000 * protocol::MONTHS_PER_YEAR {
        return false;
    }
    tick > 0 && tick.is_multiple_of(2917 * protocol::MONTHS_PER_YEAR)
}
