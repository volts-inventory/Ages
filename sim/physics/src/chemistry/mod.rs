//! Substrate-aware chemistry: phase transitions, combustion, and
//! Clausius-Clapeyron pressure-aware boil. The `Chemistry` law
//! aggregates a substrate-specific reaction set; per-tick
//! integration applies each reaction in order.

mod constants;
mod reactions;
mod substance;
mod substrate;

pub use constants::*;
pub use reactions::{BiofuelRegrowth, CombustionReaction, PhaseTransition};
pub use substance::Substance;
pub use substrate::{
    substrate_boiling_point_k, substrate_phase_thresholds, substrate_properties,
    water_boiling_point_k, SubstrateProperties,
};

use crate::laws::Law;
use crate::state::PhysicsState;
use sim_arith::Real;

/// The chemistry law family. Holds an authored list of phase
/// transitions and combustion reactions; applies them in fixed
/// order each call. Order matters for fixed-point determinism but
/// not for physical correctness — any fixed order works.
#[derive(Debug, Clone)]
pub struct Chemistry {
    pub transitions: Vec<PhaseTransition>,
    pub combustions: Vec<CombustionReaction>,
    /// Photosynthesis-equivalent biofuel regrowth (Ash → Fuel +
    /// Oxidiser). Empty on lifeless worlds; one entry on any planet
    /// with a non-zero biosphere class.
    pub regrowths: Vec<BiofuelRegrowth>,
}

impl Chemistry {
    /// Build chemistry with planet-derived SI thresholds.
    ///
    /// - `surface_pressure_pa` drives the *Aqueous* boiling point
    ///   via Clausius-Clapeyron (`water_boiling_point_k`); other
    ///   substrates use their reference-pressure boil constant
    ///   (follow-up: substrate-specific Clausius-Clapeyron).
    /// - `ignition_threshold_k` is the autoignition temperature for
    ///   the fuel + oxidiser pair on this planet (oxidising rich
    ///   atmospheres ignite cooler; reducing or thin atmospheres
    ///   need to be hotter).
    /// - `substrate_tag` selects the freeze/boil thresholds.
    ///   The substrate's solvent semantics replace Earth-water
    ///   chauvinism — methane worlds freeze at 91 K, silicate at
    ///   1687 K, ammonia at 195 K. `Substance::{Water, Ice, Vapour}`
    ///   semantically represent "solvent liquid / solid / gas" for
    ///   the run's substrate.
    ///
    /// Latent-heat coefficients are J/kg divided by water's
    /// specific heat — the per-step temperature update is
    /// `m * latent_heat / c_p_water`. Future refinement: per-
    /// substrate latent heats. Earth-water coefficients are a
    /// reasonable order-of-magnitude proxy for now.
    pub fn for_planet(
        surface_pressure_pa: Real,
        ignition_threshold_k: Real,
        substrate_tag: &str,
    ) -> Self {
        Self::for_planet_with_perturbation(
            surface_pressure_pa,
            ignition_threshold_k,
            substrate_tag,
            Real::ZERO,
        )
    }

    /// Per-seed substrate variation. Same as `for_planet` but
    /// applies a relative perturbation in `[-0.05, +0.05]` to the
    /// substrate's nominal freeze + boil points. Aqueous water on
    /// seed 42 might freeze at 273.5 K, on seed 100 at 270.7 K —
    /// substrate variant stays the same; the exact phase-transition
    /// temperature varies within the substrate's tolerance window.
    pub fn for_planet_with_perturbation(
        surface_pressure_pa: Real,
        ignition_threshold_k: Real,
        substrate_tag: &str,
        substrate_perturbation: Real,
    ) -> Self {
        let props = substrate_properties(substrate_tag);
        // Apply (1 + perturbation) to freeze + boil baselines.
        // The perturbation is bounded ±0.05 (so each seed's chemistry
        // shifts by ≤5%), preserving the substrate's tolerance
        // window — Aqueous still freezes near 273 K, just not
        // exactly at 273.15 K.
        let perturb = Real::ONE + substrate_perturbation;
        let freeze_point = props.freeze_point_k * perturb;
        // Substrate-aware Clausius-Clapeyron for *every*
        // substrate, not just Aqueous. Methane / NH3 / silicate
        // worlds now also see boil point shift with pressure.
        let boil_point = substrate_boiling_point_k(substrate_tag, surface_pressure_pa) * perturb;
        // Per-substrate `c_p` and `cell_thermal_mass_kg` so the
        // K-per-kg conversion matches the substrate, not just
        // its latent-heat magnitude. Earlier code used `C_P_WATER ×
        // CELL_THERMAL_MASS_KG = 4186 × 539` for every substrate;
        // now we read `props.c_p × props.cell_thermal_mass_kg` so
        // methane / ammonia / silicate worlds get correctly-scaled
        // temperature swings on phase change.
        let denom = Real::from_int(props.c_p * props.cell_thermal_mass_kg);
        let lh_freeze = Real::from_int(props.l_fusion) / denom;
        let lh_melt = -lh_freeze;
        let lh_condense = Real::from_int(props.l_vaporisation) / denom;
        let lh_evaporate = -lh_condense;
        let lh_combustion = Real::from_int(COMBUSTION_ENTHALPY_WOOD) / denom;
        let lh_fossil_combustion = Real::from_int(COMBUSTION_ENTHALPY_FOSSIL) / denom;
        // Photosynthesis is endothermic (sun-driven); per-unit
        // regrowth absorbs the same energy biofuel combustion
        // released, so the cycle is roughly energy-neutral on long
        // timescales. Sign is negated: positive enthalpy → negative
        // latent_heat → cell cools as biofuel regrows.
        let lh_regrowth = -lh_combustion;
        // Habitable temperature band for regrowth: [freeze + 5,
        // boil - 5]. Stays inside the substrate's liquid-solvent
        // window so cells in the equator-pole gradient that locally
        // freeze or boil don't regrow biofuel.
        let regrow_min = freeze_point + Real::from_int(5);
        let regrow_max = boil_point - Real::from_int(5);
        Self {
            transitions: vec![
                PhaseTransition {
                    from: Substance::Water,
                    to: Substance::Ice,
                    threshold: freeze_point,
                    rate: Real::percent(1),
                    forward_when_hot: false,
                    latent_heat: lh_freeze,
                },
                PhaseTransition {
                    from: Substance::Ice,
                    to: Substance::Water,
                    threshold: freeze_point,
                    rate: Real::percent(1),
                    forward_when_hot: true,
                    latent_heat: lh_melt,
                },
                PhaseTransition {
                    from: Substance::Water,
                    to: Substance::Vapour,
                    threshold: boil_point,
                    rate: Real::from_ratio(1, 1000),
                    forward_when_hot: true,
                    latent_heat: lh_evaporate,
                },
                PhaseTransition {
                    from: Substance::Vapour,
                    to: Substance::Water,
                    threshold: boil_point,
                    rate: Real::from_ratio(1, 1000),
                    forward_when_hot: false,
                    latent_heat: lh_condense,
                },
            ],
            combustions: vec![
                CombustionReaction {
                    fuel: Substance::Fuel,
                    oxidiser: Substance::Oxidiser,
                    product: Substance::Ash,
                    ignition_threshold: ignition_threshold_k,
                    rate: Real::percent(1),
                    latent_heat: lh_combustion,
                },
                // Fossil combustion: same stoichiometry, higher
                // ignition threshold (+200 K above the planet's
                // biofuel ignition point so spontaneous wildfires
                // can't easily light buried hydrocarbons — civ-
                // engineered combustion eventually will), and ~2.6×
                // the energy density of biofuel.
                CombustionReaction {
                    fuel: Substance::Fossil,
                    oxidiser: Substance::Oxidiser,
                    product: Substance::Ash,
                    ignition_threshold: ignition_threshold_k + Real::from_int(200),
                    rate: Real::percent(1),
                    latent_heat: lh_fossil_combustion,
                },
            ],
            regrowths: vec![BiofuelRegrowth {
                fuel: Substance::Fuel,
                oxidiser: Substance::Oxidiser,
                ash: Substance::Ash,
                min_temp: regrow_min,
                max_temp: regrow_max,
                // Need at least a trace of liquid solvent to act as
                // a cofactor — desert cells without surface water
                // don't regrow.
                water_cofactor_min: Real::percent(1),
                // 1/1000 per tick: an ash-saturated cell at full
                // deficit closes ~10% of the gap in 100 ticks,
                // ~63% in 1000 ticks. Slow enough that combustion-
                // depleted regions remain demographically pinched
                // for many generations before recovering.
                rate: Real::from_ratio(1, 1000),
                latent_heat: lh_regrowth,
            }],
        }
    }

    /// Earth-equivalent default chemistry. Tests and orchestration
    /// fallback when no Planet is in scope. Equivalent to
    /// `for_planet(101_325 Pa, 600 K, "aqueous")`.
    pub fn earth_like_water() -> Self {
        Self::for_planet(Real::from_int(P_REF_ATM_PA), Real::from_int(600), "aqueous")
    }
}

impl Law for Chemistry {
    fn integrate(&self, state: &mut PhysicsState, dt: Real) {
        for transition in &self.transitions {
            transition.apply(state, dt);
        }
        for combustion in &self.combustions {
            combustion.apply(state, dt);
        }
        // Regrowth runs after combustion in the same tick: ash
        // produced this tick is already eligible to feed regrowth.
        // Order is fixed for determinism.
        for regrowth in &self.regrowths {
            regrowth.apply(state, dt);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::HexGrid;
    use crate::state::N_SUBSTANCES;

    fn fresh_state(width: u32, height: u32) -> PhysicsState {
        PhysicsState::new(HexGrid::new(width, height))
    }

    #[test]
    fn cold_water_freezes() {
        // 263 K = -10 °C, below water's 273.15 K freezing point.
        let mut state = fresh_state(3, 3);
        for t in state.temperature_mut() {
            *t = Real::from_int(263);
        }
        for w in state.substance_mut(Substance::Water.idx()) {
            *w = Real::from_int(10);
        }
        let chem = Chemistry::earth_like_water();
        for _ in 0..50 {
            chem.integrate(&mut state, Real::ONE);
        }
        // Some water has converted to ice.
        let total_ice: Real = state
            .substance(Substance::Ice.idx())
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        assert!(total_ice > Real::ZERO);
    }

    #[test]
    fn hot_water_evaporates() {
        // 423 K = 150 °C, above water's 373.15 K boiling point at
        // 1 atm.
        let mut state = fresh_state(3, 3);
        for t in state.temperature_mut() {
            *t = Real::from_int(423);
        }
        for w in state.substance_mut(Substance::Water.idx()) {
            *w = Real::from_int(10);
        }
        let chem = Chemistry::earth_like_water();
        for _ in 0..200 {
            chem.integrate(&mut state, Real::ONE);
        }
        let total_vapour: Real = state
            .substance(Substance::Vapour.idx())
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        assert!(total_vapour > Real::ZERO);
    }

    #[test]
    fn temperate_water_stays_water() {
        // 293 K = 20 °C, between freezing and boiling.
        let mut state = fresh_state(3, 3);
        for t in state.temperature_mut() {
            *t = Real::from_int(293);
        }
        for w in state.substance_mut(Substance::Water.idx()) {
            *w = Real::from_int(10);
        }
        let chem = Chemistry::earth_like_water();
        let initial_water: Real = state
            .substance(Substance::Water.idx())
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        for _ in 0..50 {
            chem.integrate(&mut state, Real::ONE);
        }
        let final_water: Real = state
            .substance(Substance::Water.idx())
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
        // Mid-range temperature → no transition triggered → water
        // total unchanged.
        assert_eq!(initial_water, final_water);
    }

    #[test]
    fn mass_is_conserved() {
        // 268 K = -5 °C: cold enough to drive freezing.
        let mut state = fresh_state(4, 4);
        for t in state.temperature_mut() {
            *t = Real::from_int(268);
        }
        for w in state.substance_mut(Substance::Water.idx()) {
            *w = Real::from_int(7);
        }
        for v in state.substance_mut(Substance::Vapour.idx()) {
            *v = Real::from_int(3);
        }
        let total_initial = total_mass(&state);
        let chem = Chemistry::earth_like_water();
        for _ in 0..100 {
            chem.integrate(&mut state, Real::ONE);
        }
        let total_final = total_mass(&state);
        assert_eq!(total_initial, total_final);
    }

    #[test]
    fn chemistry_is_deterministic() {
        let mut a = fresh_state(5, 5);
        let mut b = fresh_state(5, 5);
        for t in a.temperature_mut() {
            *t = Real::from_int(423);
        }
        for t in b.temperature_mut() {
            *t = Real::from_int(423);
        }
        for w in a.substance_mut(Substance::Water.idx()) {
            *w = Real::from_int(10);
        }
        for w in b.substance_mut(Substance::Water.idx()) {
            *w = Real::from_int(10);
        }
        let chem = Chemistry::earth_like_water();
        for _ in 0..30 {
            chem.integrate(&mut a, Real::ONE);
            chem.integrate(&mut b, Real::ONE);
        }
        assert_eq!(
            a.substance(Substance::Water.idx()),
            b.substance(Substance::Water.idx())
        );
        assert_eq!(
            a.substance(Substance::Ice.idx()),
            b.substance(Substance::Ice.idx())
        );
        assert_eq!(
            a.substance(Substance::Vapour.idx()),
            b.substance(Substance::Vapour.idx())
        );
    }

    fn total_mass(state: &PhysicsState) -> Real {
        let mut total = Real::ZERO;
        for sub in 0..N_SUBSTANCES {
            for v in state.substance(sub) {
                total = total + *v;
            }
        }
        total
    }

    #[test]
    fn freezing_releases_latent_heat() {
        // 268 K: cold water that freezes should warm the cell
        // (exothermic).
        let mut state = fresh_state(3, 3);
        for t in state.temperature_mut() {
            *t = Real::from_int(268);
        }
        for w in state.substance_mut(Substance::Water.idx()) {
            *w = Real::from_int(10);
        }
        let initial_temp = state.temperature()[0];

        let chem = Chemistry::earth_like_water();
        for _ in 0..20 {
            chem.integrate(&mut state, Real::ONE);
        }

        // Some water has frozen → cell warmed up.
        let final_temp = state.temperature()[0];
        assert!(
            final_temp > initial_temp,
            "freezing should release latent heat (exothermic)"
        );
    }

    #[test]
    fn evaporation_absorbs_latent_heat() {
        // 423 K: hot water that evaporates should cool the cell
        // (endothermic).
        let mut state = fresh_state(3, 3);
        for t in state.temperature_mut() {
            *t = Real::from_int(423);
        }
        for w in state.substance_mut(Substance::Water.idx()) {
            *w = Real::from_int(10);
        }
        let initial_temp = state.temperature()[0];

        let chem = Chemistry::earth_like_water();
        for _ in 0..50 {
            chem.integrate(&mut state, Real::ONE);
        }

        let final_temp = state.temperature()[0];
        assert!(
            final_temp < initial_temp,
            "evaporation should absorb latent heat (endothermic cooling)"
        );
    }

    #[test]
    fn combustion_consumes_fuel_and_oxidiser() {
        // 700 K is above the 600 K Earth-like default ignition.
        let mut state = fresh_state(3, 3);
        for t in state.temperature_mut() {
            *t = Real::from_int(700);
        }
        for f in state.substance_mut(Substance::Fuel.idx()) {
            *f = Real::from_int(10);
        }
        for o in state.substance_mut(Substance::Oxidiser.idx()) {
            *o = Real::from_int(10);
        }
        let initial_fuel = state.substance(Substance::Fuel.idx())[0];
        let initial_temp = state.temperature()[0];

        let chem = Chemistry::earth_like_water();
        for _ in 0..5 {
            chem.integrate(&mut state, Real::ONE);
        }

        assert!(
            state.substance(Substance::Fuel.idx())[0] < initial_fuel,
            "combustion should consume fuel"
        );
        assert!(
            state.substance(Substance::Ash.idx())[0] > Real::ZERO,
            "combustion should produce ash"
        );
        assert!(
            state.temperature()[0] > initial_temp,
            "combustion should release heat"
        );
    }

    #[test]
    fn combustion_does_not_fire_below_ignition() {
        // 323 K = 50 °C: well below the 600 K ignition threshold.
        let mut state = fresh_state(3, 3);
        for t in state.temperature_mut() {
            *t = Real::from_int(323);
        }
        for f in state.substance_mut(Substance::Fuel.idx()) {
            *f = Real::from_int(10);
        }
        for o in state.substance_mut(Substance::Oxidiser.idx()) {
            *o = Real::from_int(10);
        }
        let initial_fuel = state.substance(Substance::Fuel.idx())[0];

        let chem = Chemistry::earth_like_water();
        for _ in 0..20 {
            chem.integrate(&mut state, Real::ONE);
        }

        assert_eq!(state.substance(Substance::Fuel.idx())[0], initial_fuel);
        assert_eq!(state.substance(Substance::Ash.idx())[0], Real::ZERO);
    }

    #[test]
    fn combustion_conserves_mass() {
        let mut state = fresh_state(3, 3);
        for t in state.temperature_mut() {
            *t = Real::from_int(700);
        }
        for f in state.substance_mut(Substance::Fuel.idx()) {
            *f = Real::from_int(10);
        }
        for o in state.substance_mut(Substance::Oxidiser.idx()) {
            *o = Real::from_int(10);
        }
        let initial = total_mass(&state);

        let chem = Chemistry::earth_like_water();
        for _ in 0..20 {
            chem.integrate(&mut state, Real::ONE);
        }

        let final_mass = total_mass(&state);
        assert_eq!(initial, final_mass);
    }

    #[test]
    fn boil_point_drops_with_pressure() {
        // Clausius-Clapeyron: half-atm pressure → boiling well below
        // 373.15 K. Real measured value at 50 kPa is ~354 K.
        let p_low = Real::from_int(50_000);
        let t_low = water_boiling_point_k(p_low).to_f64_for_display();
        assert!(
            (t_low - 354.0).abs() < 5.0,
            "Clausius-Clapeyron at 50 kPa: expected ~354 K, got {t_low}"
        );
        // 1 atm should round-trip to almost exactly 373.15 K.
        let p_atm = Real::from_int(P_REF_ATM_PA);
        let t_atm = water_boiling_point_k(p_atm).to_f64_for_display();
        assert!(
            (t_atm - 373.15).abs() < 0.5,
            "boil point at 1 atm should be 373.15 K, got {t_atm}"
        );
        // 2 atm should sit ~394 K.
        let p_high = Real::from_int(2 * P_REF_ATM_PA);
        let t_high = water_boiling_point_k(p_high).to_f64_for_display();
        assert!(
            (t_high - 394.0).abs() < 5.0,
            "boil point at 2 atm: expected ~394 K, got {t_high}"
        );
    }
}
