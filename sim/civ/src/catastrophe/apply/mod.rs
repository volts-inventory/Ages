//! Per-tick catastrophe orchestrator: gates each of the five
//! kinds on its cooldown + firing predicate, then routes the
//! event through the shared resistance/dormancy/tolerance stack
//! and reports the realised population-loss fraction back to the
//! caller. CB2: each per-kind handler now lives in its own
//! sibling file (`volcanic`, `disease`, `asteroid`, `solar_flare`,
//! `ice_age`); this module hosts only the dispatcher.

use crate::Civ;
use sim_ecosystem::PlanetEcosystem;
use sim_physics::PhysicsState;
use sim_species::Species;
use sim_world::Planet;

use super::record::CatastropheRecord;

mod asteroid;
mod disease;
mod ice_age;
mod solar_flare;
mod volcanic;

#[cfg(test)]
mod test_helpers;

/// Per-tick catastrophe check. Mutates the civ (cohort + last_*
/// timestamps), the physics state (volcanic resets a cell), and
/// — F2 (xeno N2) — the planet ecosystem's per-cell biomass for
/// the species directly tied to the cell (producers on a volcanic
/// eruption cell, the densest-claimed cell for disease, etc.).
/// Per-cell coupling makes heterogeneous catastrophes possible:
/// a volcanic eruption on one cell starves only that cell's
/// producers, not the planet-wide aggregate.
/// Returns the record so the caller can emit `CatastropheFired`
/// and update `last_catastrophe_tick`.
///
/// `ecosystem` is `Option` so legacy callers (older fixtures, tests
/// that don't care about the ecosystem coupling) keep working. The
/// production callsite in `sim-core::run` passes `Some(&mut
/// ecosystem)` so the per-cell biomass tracks heterogeneous
/// catastrophe damage; tests that want to assert "the catastrophe
/// reduces eco biomass at cell N only" pass `Some` too.
///
/// Dispatch order: volcanic → disease → asteroid → solar flare →
/// ice age. Volcanic comes first because its physical signature
/// is the most explicit; disease is the demographic backstop. The
/// first kind whose cooldown + trigger both fire wins the tick —
/// subsequent kinds are skipped (`return Some(...)`).
pub fn check_and_apply(
    civ: &mut Civ,
    state: &mut PhysicsState,
    planet: &Planet,
    species: &Species,
    tick: u64,
    ecosystem: Option<&mut PlanetEcosystem>,
) -> Option<CatastropheRecord> {
    if !civ.is_active() {
        return None;
    }
    // F2 — hold ecosystem behind `Option<&mut>` so each branch can
    // reborrow as needed without moving out.
    let mut ecosystem = ecosystem;

    // Volcanic — check first since its physical signature is
    // explicit; disease is the demographic backstop.
    if let Some(rec) = volcanic::try_apply(civ, state, planet, species, tick, &mut ecosystem) {
        return Some(rec);
    }
    if let Some(rec) = disease::try_apply(civ, state, planet, species, tick, &mut ecosystem) {
        return Some(rec);
    }
    if let Some(rec) = asteroid::try_apply(civ, state, planet, species, tick, &mut ecosystem) {
        return Some(rec);
    }
    if let Some(rec) = solar_flare::try_apply(civ, state, planet, species, tick, &mut ecosystem) {
        return Some(rec);
    }
    if let Some(rec) = ice_age::try_apply(civ, state, planet, species, tick, &mut ecosystem) {
        return Some(rec);
    }
    None
}

#[cfg(test)]
mod tests {
    //! Dispatcher-level + generic `apply_resistance_and_dormancy`
    //! tests. Per-kind acceptance tests live alongside their
    //! handlers in the sibling files (`volcanic::tests`,
    //! `disease::tests`, etc.).

    use super::check_and_apply;
    use super::test_helpers::*;
    use super::super::damage::apply_resistance_and_dormancy;
    use crate::Civ;
    use sim_arith::{Pop, Real};

    #[test]
    fn no_catastrophe_on_quiet_state() {
        let mut civ = Civ::new(1, 0, Pop::from_int(50));
        let mut state = well_fed_state();
        let r = check_and_apply(
            &mut civ,
            &mut state,
            &earth_like_planet(),
            &test_species(),
            100,
            None,
        );
        assert!(r.is_none());
    }

    /// Synthetic test: a species whose envelope sits entirely
    /// outside the catastrophe cell (`match_score = 0`) takes the
    /// full `raw_frac` loss after the resistance + dormancy stack
    /// (both no-ops here ⇒ identity). Exercises the
    /// `apply_resistance_and_dormancy` formula directly to isolate
    /// the tolerance term from per-catastrophe trigger plumbing.
    #[test]
    fn tolerance_match_score_zero_means_full_damage() {
        let mut civ = Civ::new(1, 0, Pop::from_int(100));
        let mut species = test_species();
        // Envelope nowhere near the cell (temp 100-101 K, etc.).
        species.tolerance = sim_species::ToleranceEnvelope {
            temp_range: (Real::from_int(100), Real::from_int(101)),
            ph_range: (Real::from_int(1), Real::from_int(2)),
            salinity_range: (Real::from_int(900), Real::from_int(1_000)),
            radiation_max: Real::from_ratio(1, 1_000),
            pressure_range: (Real::from_int(50), Real::from_int(51)),
        };
        species.dormancy_capability = Real::ZERO;
        // Cell sits outside on every axis — match_score = 0.
        let cell = (
            Real::from_int(300), // T
            Real::from_int(7),   // pH
            Real::from_int(20),  // salinity
            Real::ONE,           // rad (above radiation_max)
            Real::ONE,           // pressure
        );
        let raw = Real::percent(40);
        let out = apply_resistance_and_dormancy(&mut civ, &species, raw, cell, 0);
        // No tools, no dormancy, match_score = 0 ⇒ out == raw exactly.
        assert_eq!(out, raw, "expected full raw_frac loss when match_score = 0");
    }

    /// Synthetic test: a species whose envelope perfectly contains
    /// the cell at its centre (`match_score = 1`) takes ~zero
    /// catastrophe damage. Mirrors the formula's "perfect fit ⇒
    /// no damage" guarantee.
    #[test]
    fn tolerance_match_score_one_means_no_damage() {
        let mut civ = Civ::new(1, 0, Pop::from_int(100));
        let mut species = test_species();
        // Cell at the exact centre of every axis.
        let t_centre = Real::from_int(300);
        let ph_centre = Real::from_int(7);
        let sal_centre = Real::from_int(20);
        let rad_zero = Real::ZERO;
        let p_centre = Real::ONE;
        let half = Real::ONE;
        species.tolerance = sim_species::ToleranceEnvelope {
            temp_range: (t_centre - half, t_centre + half),
            ph_range: (ph_centre - half, ph_centre + half),
            salinity_range: (sal_centre - half, sal_centre + half),
            // Any positive ceiling works — radiation_score returns
            // 1.0 when `rad <= 0`.
            radiation_max: Real::ONE,
            pressure_range: (p_centre - half, p_centre + half),
        };
        species.dormancy_capability = Real::ZERO;
        let cell = (t_centre, ph_centre, sal_centre, rad_zero, p_centre);
        let raw = Real::percent(40);
        let out = apply_resistance_and_dormancy(&mut civ, &species, raw, cell, 0);
        // Perfect centre on every axis ⇒ match_score = 1 ⇒ loss = 0.
        assert_eq!(
            out,
            Real::ZERO,
            "expected zero loss for centre-of-envelope species",
        );
    }

    /// P1.3 acceptance test #2: mass-extinction recovery via
    /// seed-bank resurrection. Apply a 100% catastrophe (everyone
    /// dies) to a `dormancy = 0.9` species, then run the
    /// per-tick resurrection step for 500 ticks. Active
    /// population must recover to ≥ 99% of pre-event. Drives the
    /// formula directly (no per-catastrophe firing machinery) so
    /// the test isolates the dormant-pool seeding + resurrection
    /// path from the catastrophe-trigger plumbing.
    #[test]
    fn mass_extinction_recovery_via_seed_bank_resurrection() {
        let pre_event = Pop::from_int(1_000);
        let mut civ = Civ::new(1, 0, pre_event);
        let mut species = test_species();
        species.dormancy_capability = Real::percent(90);
        // Cell outside the species' envelope on every axis →
        // match_score = 0 → full base damage. Combined with
        // raw = 1.0 the would-be loss is 100% of pop.
        species.tolerance = sim_species::ToleranceEnvelope {
            temp_range: (Real::from_int(100), Real::from_int(101)),
            ph_range: (Real::from_int(1), Real::from_int(2)),
            salinity_range: (Real::from_int(900), Real::from_int(1_000)),
            radiation_max: Real::from_ratio(1, 1_000),
            pressure_range: (Real::from_int(50), Real::from_int(51)),
        };
        let cell = (
            Real::from_int(300),
            Real::from_int(7),
            Real::from_int(20),
            Real::ONE,
            Real::ONE,
        );
        // raw = 1.0 — the catastrophe wants to kill everyone.
        let frac = apply_resistance_and_dormancy(&mut civ, &species, Real::ONE, cell, 100);
        // Realised loss ≈ 10% (1 − 0.9 × 1 = 0.1) so the active
        // cohort survives at ≈ 10%. Apply the shrink_to to match
        // what `check_and_apply` does at the call site.
        let target = (civ.cohort.total() * (Real::ONE - frac)).max(Pop::ZERO);
        civ.cohort.shrink_to(target);
        // Dormant pool now holds ≈ 90% × pre_event = 900.
        let expected_dormant = pre_event.to_real_nonneg() * Real::percent(90);
        let tol = Real::from_int(2);
        assert!(
            (civ.dormant_pool.population - expected_dormant).abs() <= tol,
            "expected ≈ 900 dormant, got {:?}",
            civ.dormant_pool.population,
        );
        // Run 500 ticks of resurrection at 1%/tick. Cap on the
        // pre-event population — never exceeded by construction.
        let target = pre_event.to_real_nonneg();
        for _ in 0..500 {
            let mut active = civ.cohort.total().to_real_nonneg();
            let revived = civ.dormant_pool.resurrect_step(&mut active, target);
            // Deposit the revived headcount back into the cohort
            // (fertile bracket; matches the "add to fertile"
            // distribution policy the spec mentions).
            civ.cohort.add_fertile(Pop::from_real(revived));
        }
        // Active population must be ≥ 99% of pre-event.
        let bound = pre_event.to_real_nonneg() * Real::percent(99);
        let active_now = civ.cohort.total().to_real_nonneg();
        assert!(
            active_now >= bound,
            "seed-bank recovery failed: active={active_now:?} bound={bound:?}",
        );
    }
}
