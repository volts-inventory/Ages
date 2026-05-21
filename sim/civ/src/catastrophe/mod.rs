//! catastrophes — the 5-kind death-amplifier set. Each kind
//! has its own per-tick trigger predicate, its own cooldown, and
//! its own population-loss fraction; `check_and_apply` orchestrates
//! the per-civ check + apply step.

mod cells;
mod factors;
mod triggers;

pub use cells::{
    apply_to_cell_and_neighbors, densest_claimed_cell, deterministic_cell_pick, hex_neighbors,
};
pub use factors::{disease_severity_factor, ice_age_severity_factor, volcanic_cooldown_factor};

use crate::cosmology::Cosmology;
use crate::Civ;
use sim_arith::{Pop, Real};
use sim_physics::{PhysicsState, Substance};
use sim_species::{apply_catastrophe_with_dormancy, Species};
use sim_world::Planet;

use triggers::{asteroid_fires, disease_fires, ice_age_fires, solar_flare_fires, volcanic_fires};

/// catastrophe taxonomy. Five kinds — `Volcanic` and
/// `Disease` are the M4-min lithosphere/biosphere triggers,
/// plus three later additions for story diversity:
/// `Asteroid` (rare-event impact), `SolarFlare` (high stellar
/// luminosity + weak magnetosphere → EM disruption), and
/// `IceAge` (sustained planet-mean temperature drop).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatastropheKind {
    Volcanic,
    Disease,
    Asteroid,
    SolarFlare,
    IceAge,
}

impl CatastropheKind {
    pub fn tag(self) -> &'static str {
        match self {
            CatastropheKind::Volcanic => "volcanic",
            CatastropheKind::Disease => "disease",
            CatastropheKind::Asteroid => "asteroid",
            CatastropheKind::SolarFlare => "solar_flare",
            CatastropheKind::IceAge => "ice_age",
        }
    }
}

/// Per-kind cooldown (ticks). Placeholders under. : scaled
/// ×12 so the year-equivalent recurrence matches the old yearly
/// cadence under 1 tick = 1 month.
pub const VOLCANIC_COOLDOWN_TICKS: u64 = 200 * protocol::MONTHS_PER_YEAR;
pub const DISEASE_COOLDOWN_TICKS: u64 = 500 * protocol::MONTHS_PER_YEAR;
pub const ASTEROID_COOLDOWN_TICKS: u64 = 5_000 * protocol::MONTHS_PER_YEAR;
pub const SOLAR_FLARE_COOLDOWN_TICKS: u64 = 800 * protocol::MONTHS_PER_YEAR;
pub const ICE_AGE_COOLDOWN_TICKS: u64 = 4_000 * protocol::MONTHS_PER_YEAR;
pub const DISEASE_AGE_FLOOR_TICKS: u64 = 300 * protocol::MONTHS_PER_YEAR;

/// Population-fraction lost on each kind ( placeholders).
pub const VOLCANIC_POP_LOSS: (i64, i64) = (5, 100);
pub const DISEASE_POP_LOSS: (i64, i64) = (30, 100);
pub const ASTEROID_POP_LOSS: (i64, i64) = (40, 100);
pub const SOLAR_FLARE_POP_LOSS: (i64, i64) = (10, 100);
pub const ICE_AGE_POP_LOSS: (i64, i64) = (20, 100);

/// One catastrophe applied this tick — what kind, and the
/// fraction of population lost.
#[derive(Debug, Clone, Copy)]
pub struct CatastropheRecord {
    pub kind: CatastropheKind,
    pub fraction_lost: Real,
}

/// Per-catastrophe severity factor ∈ [0, 1] for the dormancy
/// damage-reduction formula. Sprint 2 Item 7b pins this at 1.0
/// (full-severity catastrophes) for all five kinds; a future
/// polish pass can expose a per-kind table if a follow-up wants
/// shallow events to bypass dormancy benefit. Centralised here so
/// the constant lives in one place.
const DORMANCY_SEVERITY_FACTOR: Real = Real::ONE;

/// Wrap `apply_catastrophe_with_dormancy` with the per-civ
/// existing `apply_catastrophe_resistance` (tools soften the
/// blow) then the per-species dormancy reduction (tardigrade-
/// grade species shrug off catastrophes). Tools-first preserves
/// the existing behaviour: the catastrophe-resistance test fixture
/// is unaffected by dormancy because the fixture's species has
/// `dormancy = 0`.
fn apply_resistance_and_dormancy(
    civ: &Civ,
    species: &Species,
    raw_frac: Real,
) -> Real {
    let after_tools = civ.apply_catastrophe_resistance(raw_frac);
    apply_catastrophe_with_dormancy(
        species.dormancy_capability,
        after_tools,
        DORMANCY_SEVERITY_FACTOR,
    )
}

/// Per-tick catastrophe check. Mutates the civ (cohort + last_*
/// timestamps) and the physics state (volcanic resets a cell).
/// Returns the record so the caller can emit `CatastropheFired`
/// and update `last_catastrophe_tick`.
#[allow(clippy::too_many_lines)]
pub fn check_and_apply(
    civ: &mut Civ,
    state: &mut PhysicsState,
    planet: &Planet,
    species: &Species,
    tick: u64,
) -> Option<CatastropheRecord> {
    if !civ.is_active() {
        return None;
    }
    // Volcanic — check first since its physical signature is
    // explicit; disease is the demographic backstop.
    // volcanic cooldown scales with crust — Basaltic
    // baseline, Hydrocarbon shorter (more frequent), older crusts
    // longer. Computed in Q32.32 then converted back to ticks.
    let volcanic_factor = volcanic_cooldown_factor(planet.crust);
    let scaled_cooldown_real =
        Real::from_int(i64::try_from(VOLCANIC_COOLDOWN_TICKS).unwrap_or(i64::MAX))
            * volcanic_factor;
    let scaled_volcanic_cooldown: u64 =
        u64::try_from(scaled_cooldown_real.raw().to_num::<i64>().max(1))
            .unwrap_or(VOLCANIC_COOLDOWN_TICKS);
    let volcanic_ready = civ
        .last_volcanic_tick
        .is_none_or(|t| tick.saturating_sub(t) >= scaled_volcanic_cooldown);
    if volcanic_ready {
        if let Some(cell) = volcanic_fires(state) {
            // Reset the cell: zero its fuel, drop temperature 50 K.
            state.substance_mut(Substance::Fuel.idx())[cell] = Real::ZERO;
            let cur = state.temperature()[cell];
            state.temperature_mut()[cell] = (cur - Real::from_int(50)).max(Real::ZERO);
            // region-targeted population loss: scales the
            // affected cell's region cohort by the volcanic
            // fraction. Aggregate cohort updates in sync.
            // PermanentMasonry / DefensiveFortification
            // soften the blow via apply_catastrophe_resistance.
            let raw_frac = Real::from(VOLCANIC_POP_LOSS);
            let frac = apply_resistance_and_dormancy(civ, species, raw_frac);
            let cell_u32 = u32::try_from(cell).unwrap_or(u32::MAX);
            let lost_in_region = civ.drop_cell_pop(cell_u32, frac);
            // For civs without claimed_cells (legacy / tests),
            // fall back to the aggregate-fraction loss so the
            // catastrophe still has an effect.
            if lost_in_region == Pop::ZERO {
                let target = (civ.cohort.total() * (Real::ONE - frac)).max(Pop::ZERO);
                civ.cohort.shrink_to(target);
            }
            civ.last_volcanic_tick = Some(tick);
            civ.last_catastrophe_tick = Some(tick);
            return Some(CatastropheRecord {
                kind: CatastropheKind::Volcanic,
                fraction_lost: frac,
            });
        }
    }
    // Disease — cell-targeted: starts at the densest cell
    // and spreads to adjacent claimed cells. Pre- it was a
    // uniform civ-wide pop drop, which read as artificial: a
    // plague hits cities first, not equally everywhere.
    //
    // Disease is biology-driven (crowding-disease dynamics tied to
    // generational time), so its cooldown stretches with substrate
    // metabolism — a silicate civ doesn't experience the same plague
    // cadence as an aqueous one in absolute ticks. The physics-
    // driven kinds (volcanic / asteroid / solar / ice age) keep raw
    // cooldowns: those are external to biology.
    let metabolism = planet.metabolic_substrate.metabolism();
    let disease_cooldown =
        crate::demographics::streak_ticks_for_metabolism(DISEASE_COOLDOWN_TICKS, metabolism);
    let disease_ready = civ
        .last_disease_tick
        .is_none_or(|t| tick.saturating_sub(t) >= disease_cooldown);
    if disease_ready && disease_fires(civ, state, planet, tick) {
        // severity scales with biosphere richness. :
        // BasicHealing / MedicalIntervention / AdvancedMedicine /
        // GeneticManipulation reduce the realised loss via
        // apply_catastrophe_resistance — the headline catastrophe-
        // resistance effect for healthcare-bearing civs.
        let base_frac = Real::from(DISEASE_POP_LOSS);
        let severity_frac = base_frac * disease_severity_factor(planet.biosphere);
        let frac = apply_resistance_and_dormancy(civ, species, severity_frac);
        let center_frac = frac * Real::from_int(2);
        let neighbor_frac = frac;
        let grid_w = state.grid().width();
        let grid_h = state.grid().height();
        let lost = if let Some(origin) = densest_claimed_cell(civ) {
            apply_to_cell_and_neighbors(
                civ,
                grid_w,
                grid_h,
                origin,
                center_frac,
                neighbor_frac,
                true,
            )
        } else {
            // Fallback for civs without per-cell cohorts (legacy /
            // tests): apply uniform fraction to aggregate.
            let before = civ.cohort.total();
            let target = (before * (Real::ONE - frac)).max(Pop::from_int(10));
            civ.cohort.shrink_to(target)
        };
        let _ = lost;
        civ.last_disease_tick = Some(tick);
        civ.last_catastrophe_tick = Some(tick);
        // pivot toward communitarian + mystical (plague-cosmology pattern).
        let push = Cosmology {
            empirical: Real::ZERO,
            communitarian: Real::percent(15),
            reformist: -Real::percent(5),
            mystical: Real::percent(15),
            hierarchical: Real::percent(5),
        };
        civ.apply_cosmology_push(&push, Real::ONE);
        return Some(CatastropheRecord {
            kind: CatastropheKind::Disease,
            fraction_lost: frac,
        });
    }

    // Asteroid impact — rare, dramatic, hits hard. Gated only by
    // tick-based deterministic firing window + cooldown.
    let asteroid_ready = civ
        .last_asteroid_tick
        .is_none_or(|t| tick.saturating_sub(t) >= ASTEROID_COOLDOWN_TICKS);
    if asteroid_ready && asteroid_fires(tick) {
        // cell-targeted: deterministic impact site per
        // (tick, civ_id). Impact cell takes 2× the global
        // fraction; adjacent claimed cells take 0.5× (debris,
        // fires). If the civ has no claim, fall back to uniform
        // pop drop so a brand-new civ still feels the global
        // aftermath. : catastrophe-resistance tools soften
        // the absolute loss (built shelter survives debris).
        let raw_frac = Real::from(ASTEROID_POP_LOSS);
        let frac = apply_resistance_and_dormancy(civ, species, raw_frac);
        let center_frac = frac * Real::from_int(2);
        let neighbor_frac = frac / Real::from_int(2);
        let grid_w = state.grid().width();
        let grid_h = state.grid().height();
        let lost = if let Some(impact) = deterministic_cell_pick(civ, tick) {
            apply_to_cell_and_neighbors(
                civ,
                grid_w,
                grid_h,
                impact,
                center_frac,
                neighbor_frac,
                true,
            )
        } else {
            let before = civ.cohort.total();
            let target = (before * (Real::ONE - frac)).max(Pop::from_int(10));
            civ.cohort.shrink_to(target)
        };
        let _ = lost;
        civ.last_asteroid_tick = Some(tick);
        civ.last_catastrophe_tick = Some(tick);
        // Asteroid pushes mystical strongly + reformist (rebuild
        // pressure) — civilization-shaking event.
        let push = Cosmology {
            empirical: -Real::percent(5),
            communitarian: Real::percent(10),
            reformist: Real::percent(15),
            mystical: Real::percent(20),
            hierarchical: -Real::percent(5),
        };
        civ.apply_cosmology_push(&push, Real::ONE);
        return Some(CatastropheRecord {
            kind: CatastropheKind::Asteroid,
            fraction_lost: frac,
        });
    }

    // Solar flare — gated on planet's stellar luminosity +
    // magnetosphere weakness. Hits modestly; pushes empirical
    // (the species observes the flare directly).
    let flare_ready = civ
        .last_solar_flare_tick
        .is_none_or(|t| tick.saturating_sub(t) >= SOLAR_FLARE_COOLDOWN_TICKS);
    if flare_ready && solar_flare_fires(planet, tick) {
        // catastrophe resistance softens the flare's hit
        // (advanced shielding / underground habitats / radiation
        // medicine).
        let raw_frac = Real::from(SOLAR_FLARE_POP_LOSS);
        let frac = apply_resistance_and_dormancy(civ, species, raw_frac);
        let before = civ.cohort.total();
        let target = (before * (Real::ONE - frac)).max(Pop::from_int(10));
        let _lost = civ.cohort.shrink_to(target);
        civ.last_solar_flare_tick = Some(tick);
        civ.last_catastrophe_tick = Some(tick);
        // Empirical + reformist (the species sees the sky's
        // role in their fate — drives observational science).
        let push = Cosmology {
            empirical: Real::percent(15),
            communitarian: Real::ZERO,
            reformist: Real::percent(10),
            mystical: Real::percent(5),
            hierarchical: Real::ZERO,
        };
        civ.apply_cosmology_push(&push, Real::ONE);
        return Some(CatastropheRecord {
            kind: CatastropheKind::SolarFlare,
            fraction_lost: frac,
        });
    }

    // Ice age — gated on cold-planet baseline + civ maturity.
    // Pushes communitarian (huddle-together) + hierarchical
    // (centralized resource management).
    let ice_ready = civ
        .last_ice_age_tick
        .is_none_or(|t| tick.saturating_sub(t) >= ICE_AGE_COOLDOWN_TICKS);
    if ice_ready && ice_age_fires(planet, civ, tick) {
        // severity scales with planet's mean temperature —
        // colder planets suffer worse ice ages. : catastrophe
        // resistance + cryogenic-engineering tools soften the loss.
        let base_frac = Real::from(ICE_AGE_POP_LOSS);
        let severity_frac =
            (base_frac * ice_age_severity_factor(planet.mean_temperature)).min(Real::percent(60));
        let frac = apply_resistance_and_dormancy(civ, species, severity_frac);
        let before = civ.cohort.total();
        let target = (before * (Real::ONE - frac)).max(Pop::from_int(10));
        let _lost = civ.cohort.shrink_to(target);
        civ.last_ice_age_tick = Some(tick);
        civ.last_catastrophe_tick = Some(tick);
        let push = Cosmology {
            empirical: Real::ZERO,
            communitarian: Real::percent(20),
            reformist: -Real::percent(5),
            mystical: Real::percent(5),
            hierarchical: Real::percent(15),
        };
        civ.apply_cosmology_push(&push, Real::ONE);
        return Some(CatastropheRecord {
            kind: CatastropheKind::IceAge,
            fraction_lost: frac,
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_physics::HexGrid;
    use sim_recognition::RecognitionLibrary;
    use sim_world::{sample_planet, Magnetosphere};

    /// Default test species — `dormancy_capability = 0` so all
    /// existing pre-Sprint-2-Item-7b catastrophe assertions still
    /// hold (no damage-reduction multiplier). Dormancy-specific
    /// tests below construct their own species with explicit
    /// `dormancy_capability`.
    fn test_species() -> Species {
        let planet = sample_planet(1);
        let lib = RecognitionLibrary::earth_like_default();
        let mut s = sim_species::derive(&planet, &lib);
        s.dormancy_capability = Real::ZERO;
        s
    }

    fn species_with_dormancy(dormancy: Real) -> Species {
        let mut s = test_species();
        s.dormancy_capability = dormancy;
        s
    }

    fn empty_state() -> PhysicsState {
        PhysicsState::new(HexGrid::new(4, 4))
    }

    fn well_fed_state() -> PhysicsState {
        let mut s = PhysicsState::new(HexGrid::new(4, 4));
        for v in s.substance_mut(Substance::Fuel.idx()) {
            *v = Real::from_int(10);
        }
        s
    }

    /// Test fixture: a benign Earth-like planet that doesn't
    /// trigger any of the new gated catastrophes. Lets the
    /// existing volcanic/disease tests run unaffected.
    fn earth_like_planet() -> Planet {
        Planet {
            seed: 0,
            name: "TestPlanet".to_string(),
            gravity: Real::from_int(10),
            composition: sim_world::Composition::Rocky,
            mean_temperature: Real::from_int(288),
            temperature_gradient: Real::from_int(20),
            terrain_peak: Real::from_int(8000),
            terrain_centre_q: 0,
            terrain_centre_r: 0,
            sea_level: Real::from_int(2000),
            atmosphere: sim_world::Atmosphere::Oxidising,
            atmospheric_composition: sim_world::AtmosphericComposition::vacuum(),
            biosphere_density: Real::from_ratio(3, 10),
            crustal_composition: sim_world::CrustalComposition::empty(),
            surface_pressure: Real::from_int(101_325),
            biosphere: sim_world::BiosphereClass::Lush,
            magnetosphere: Magnetosphere::Strong,
            crust: sim_world::Crust::Basaltic,
            stellar_luminosity: Real::from_int(1361),
            moon_count: 1,
            moons: vec![sim_world::Moon {
                mass_relative_x100: 100,
                orbital_period_macros: 28,
                inclination_deg_x10: 51,
            }],
            orbital_eccentricity_x100: 2,
            axial_tilt_deg: Real::from_int(23),
            day_length_hours: Real::from_int(24),
            orbital_period_months: 12,
            metabolic_substrate: sim_world::MetabolicSubstrate::Aqueous,
            substrate_perturbation: Real::ZERO,
        }
    }

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
        );
        assert!(r.is_none());
    }

    #[test]
    fn volcanic_fires_on_extreme_signature() {
        let mut civ = Civ::new(1, 0, Pop::from_int(100));
        let mut state = well_fed_state();
        state.charge_mut()[0] = Real::from_int(120);
        state.temperature_mut()[0] = Real::from_int(700);
        let r = check_and_apply(
            &mut civ,
            &mut state,
            &earth_like_planet(),
            &test_species(),
            50,
        );
        let rec = r.expect("volcanic should fire");
        assert_eq!(rec.kind, CatastropheKind::Volcanic);
        // Cell 0 fuel reset, temperature dropped, civ pop dropped.
        assert_eq!(state.substance(Substance::Fuel.idx())[0], Real::ZERO);
        assert!(state.temperature()[0] < Real::from_int(700));
        assert!(civ.cohort.total() < Pop::from_int(100));
        assert_eq!(civ.last_volcanic_tick, Some(50));
        assert_eq!(civ.last_catastrophe_tick, Some(50));
    }

    #[test]
    fn volcanic_respects_cooldown() {
        // cooldown lengths derive from VOLCANIC_COOLDOWN_TICKS
        // so the test stays correct as the constant scales.
        let mut civ = Civ::new(1, 0, Pop::from_int(100));
        let mut state = well_fed_state();
        state.charge_mut()[0] = Real::from_int(120);
        state.temperature_mut()[0] = Real::from_int(700);
        let sp = test_species();
        check_and_apply(&mut civ, &mut state, &earth_like_planet(), &sp, 0);
        // Re-set the trigger (in case the apply zeroed something).
        state.charge_mut()[0] = Real::from_int(120);
        state.temperature_mut()[0] = Real::from_int(700);
        // Halfway through cooldown — still inside.
        let r = check_and_apply(
            &mut civ,
            &mut state,
            &earth_like_planet(),
            &sp,
            VOLCANIC_COOLDOWN_TICKS / 2,
        );
        assert!(r.is_none());
        // Past cooldown.
        state.charge_mut()[0] = Real::from_int(120);
        state.temperature_mut()[0] = Real::from_int(700);
        let r = check_and_apply(
            &mut civ,
            &mut state,
            &earth_like_planet(),
            &sp,
            VOLCANIC_COOLDOWN_TICKS + 50,
        );
        assert!(r.is_some());
    }

    #[test]
    fn disease_fires_under_crowding_after_age_floor() {
        let mut civ = Civ::new(1, 0, Pop::from_int(50));
        let mut state = empty_state();
        // Tiny capacity (no fuel) → infinite crowding... actually
        // capacity=0 means food_security=0 → other path. Use a
        // small fuel value tuned for the 50_000/fuel-unit baseline:
        // 0.001 × 50_000 = 50, matching the civ's pop so crowding = 1.0.
        state.substance_mut(Substance::Fuel.idx())[0] = Real::from_ratio(1, 1000);
        let r = check_and_apply(
            &mut civ,
            &mut state,
            &earth_like_planet(),
            &test_species(),
            DISEASE_AGE_FLOOR_TICKS,
        );
        let rec = r.expect("disease should fire");
        assert_eq!(rec.kind, CatastropheKind::Disease);
        assert!(civ.cohort.total() < Pop::from_int(50));
        // Cosmology pivoted.
        assert!(civ.cosmology.mystical > Real::ZERO);
    }

    #[test]
    fn disease_blocked_before_age_floor() {
        let mut civ = Civ::new(1, 0, Pop::from_int(50));
        let mut state = empty_state();
        state.substance_mut(Substance::Fuel.idx())[0] = Real::from_ratio(1, 1000);
        let r = check_and_apply(
            &mut civ,
            &mut state,
            &earth_like_planet(),
            &test_species(),
            DISEASE_AGE_FLOOR_TICKS - 1,
        );
        assert!(r.is_none());
    }

    /// Sprint 2 Item 7b spec test #1.
    ///
    /// Species with `dormancy = 0.9` takes ~10× less damage than
    /// `dormancy = 0` from the same catastrophe. We exercise the
    /// disease pathway because its fixture is the most stable:
    /// known-firing trigger, no per-civ-shelter to confound the
    /// loss math. The two civs are identical apart from the
    /// species' dormancy trait.
    #[test]
    fn dormant_species_survives_catastrophe_at_reduced_rate() {
        let baseline_pop = Pop::from_int(50);
        let dormancy_high = Real::percent(90);

        // Baseline run — dormancy = 0.
        let mut civ_low = Civ::new(1, 0, baseline_pop);
        let mut state_low = empty_state();
        state_low.substance_mut(Substance::Fuel.idx())[0] = Real::from_ratio(1, 1000);
        let rec_low = check_and_apply(
            &mut civ_low,
            &mut state_low,
            &earth_like_planet(),
            &species_with_dormancy(Real::ZERO),
            DISEASE_AGE_FLOOR_TICKS,
        )
        .expect("baseline disease should fire");

        // Dormant run — dormancy = 0.9, otherwise identical.
        let mut civ_high = Civ::new(1, 0, baseline_pop);
        let mut state_high = empty_state();
        state_high.substance_mut(Substance::Fuel.idx())[0] = Real::from_ratio(1, 1000);
        let rec_high = check_and_apply(
            &mut civ_high,
            &mut state_high,
            &earth_like_planet(),
            &species_with_dormancy(dormancy_high),
            DISEASE_AGE_FLOOR_TICKS,
        )
        .expect("dormant disease should fire");

        // Both should be the same `kind` (disease) — the dormancy
        // multiplier only shrinks fraction_lost, not the trigger.
        assert_eq!(rec_low.kind, CatastropheKind::Disease);
        assert_eq!(rec_high.kind, CatastropheKind::Disease);

        // Effective fraction should be ~10× smaller. Allow a small
        // tolerance because both also pass through
        // `apply_catastrophe_resistance` (which is 1.0 at zero
        // tools) — the ratio is exactly
        // `(1 - 0.9 × 1.0) / (1 - 0 × 1.0) = 0.10`.
        let ratio = rec_high.fraction_lost / rec_low.fraction_lost;
        // ~0.10 ± 1% — Q32.32 is exact for these magnitudes; the
        // tolerance only protects against incidental future
        // resistance bumps that future code paths might apply
        // uniformly to both.
        assert!(
            ratio <= Real::percent(11) && ratio >= Real::percent(9),
            "expected ~0.10× damage with dormancy=0.9, got ratio={ratio:?}",
        );
    }
}
