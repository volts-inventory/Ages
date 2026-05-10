//! Substrate-relative phase thresholds and properties.
//! Clausius-Clapeyron pressure-aware boiling-point computation
//! lives here (water + substrate-tag dispatcher); freeze and boil
//! per-substrate ranges and the `SubstrateProperties` record sit
//! alongside.

use super::constants::{
    CELL_THERMAL_MASS_KG, C_P_AMMONIA, C_P_METHANE, C_P_SILICATE, C_P_WATER, L_FUSION_AMMONIA,
    L_FUSION_METHANE, L_FUSION_SILICON, L_FUSION_WATER, L_VAPORISATION_AMMONIA,
    L_VAPORISATION_METHANE, L_VAPORISATION_SILICON, L_VAPORISATION_WATER, P_REF_ATM_PA,
    R_SPECIFIC_AMMONIA, R_SPECIFIC_METHANE, R_SPECIFIC_SILICON, R_SPECIFIC_WATER,
};
use sim_arith::transcendental::ln;
use sim_arith::Real;

/// Compute the boiling point of water in Kelvin at the given
/// pressure (Pa) via Clausius-Clapeyron:
///
/// ```text
///   1/T = 1/T_ref - (R/L) * ln(P / P_ref)
/// ```
///
/// At `P = 0` we return a sentinel (200 K) that's well below
/// freezing — vacuum has no defined boiling point in this model and
/// any liquid water would already be gone via Hertz-Knudsen-style
/// loss the chemistry doesn't yet simulate.
#[must_use]
pub fn water_boiling_point_k(pressure_pa: Real) -> Real {
    substrate_boiling_point_k("aqueous", pressure_pa)
}

/// Substrate-aware Clausius-Clapeyron boiling point. Given a
/// substrate tag and surface pressure, returns the boil temperature
/// in K via Clausius-Clapeyron with the substrate's `L_vap` and
/// `R_specific`. Generalises `water_boiling_point_k` so methane / NH3
/// / silicate worlds get pressure-varying boil thresholds, not just
/// the constant reference value the earlier code shipped.
///
/// For sub-vacuum pressure, returns a sub-freeze sentinel value so
/// chemistry treats the cell as vapour-only (the substance won't
/// stay liquid in vacuum).
#[must_use]
pub fn substrate_boiling_point_k(substrate_tag: &str, pressure_pa: Real) -> Real {
    let props = substrate_properties(substrate_tag);
    if pressure_pa <= Real::ZERO {
        // Sub-freeze sentinel: 50 K below the substrate's freeze
        // point, so cells under vacuum count as "above boil" and
        // any liquid evaporates.
        return props.freeze_point_k - Real::from_int(50);
    }
    let p_ref = Real::from_int(P_REF_ATM_PA);
    let t_ref = props.boil_ref_k;
    let l_over_r = Real::from_ratio(props.l_vaporisation, props.r_specific);
    let inv_t_ref = Real::ONE / t_ref;
    let p_ratio = pressure_pa / p_ref;
    let delta = ln(p_ratio) / l_over_r;
    let inv_t = inv_t_ref - delta;
    Real::ONE / inv_t
}

/// `water_boiling_point_k` for Aqueous and through a similar
/// Clausius-Clapeyron for the others (this module lifts that limit;
/// `substrate_boiling_point_k` now applies per-substrate).
#[must_use]
pub fn substrate_phase_thresholds(substrate_tag: &str) -> (Real, Real) {
    let p = substrate_properties(substrate_tag);
    (p.freeze_point_k, p.boil_ref_k)
}

/// Complete substrate phase / latent-heat properties. Used by
/// `substrate_boiling_point_k` for pressure-aware Clausius-Clapeyron
/// and by `Chemistry::for_planet` to select per-substrate latent
/// heats so the cell's temperature swing on phase transition matches
/// the substrate, not Earth-water.
#[derive(Debug, Clone, Copy)]
pub struct SubstrateProperties {
    pub freeze_point_k: Real,
    pub boil_ref_k: Real,
    pub l_fusion: i64,
    pub l_vaporisation: i64,
    pub r_specific: i64,
    /// Specific heat of the substrate at constant pressure,
    /// J/(kg·K). Earlier every latent-heat calculation used water's
    /// `C_P_WATER` regardless of substrate; per-substrate `c_p` lifts that hardcode
    /// so methane / ammonia / silicate worlds get correct
    /// temperature swings per unit phase change.
    pub c_p: i64,
    /// Cell thermal mass in kg of substrate per
    /// representative cell. Earlier a global
    /// `CELL_THERMAL_MASS_KG = 539` (water column) was used
    /// regardless of substrate; the per-substrate field derives it
    /// from typical-density × column-depth so each substrate's
    /// "mass that participates in temperature change" is
    /// physically correct.
    pub cell_thermal_mass_kg: i64,
}

#[must_use]
pub fn substrate_properties(substrate_tag: &str) -> SubstrateProperties {
    match substrate_tag {
        "ammoniacal" => SubstrateProperties {
            freeze_point_k: Real::from_ratio(19_540, 100),
            boil_ref_k: Real::from_ratio(23_980, 100),
            l_fusion: L_FUSION_AMMONIA,
            l_vaporisation: L_VAPORISATION_AMMONIA,
            r_specific: R_SPECIFIC_AMMONIA,
            c_p: C_P_AMMONIA,
            // Liquid ammonia density ≈ 682 kg/m³; column thermal
            // mass scales accordingly relative to water's 539 / 1000.
            cell_thermal_mass_kg: 370,
        },
        "hydrocarbon" => SubstrateProperties {
            freeze_point_k: Real::from_ratio(9_070, 100),
            boil_ref_k: Real::from_ratio(11_170, 100),
            l_fusion: L_FUSION_METHANE,
            l_vaporisation: L_VAPORISATION_METHANE,
            r_specific: R_SPECIFIC_METHANE,
            c_p: C_P_METHANE,
            // Liquid methane density ≈ 445 kg/m³.
            cell_thermal_mass_kg: 240,
        },
        "silicate" => SubstrateProperties {
            freeze_point_k: Real::from_int(1_687),
            boil_ref_k: Real::from_int(3_538),
            l_fusion: L_FUSION_SILICON,
            l_vaporisation: L_VAPORISATION_SILICON,
            r_specific: R_SPECIFIC_SILICON,
            c_p: C_P_SILICATE,
            // Molten silicate density ≈ 2700 kg/m³ — much heavier
            // than water; lots of mass to heat per K.
            cell_thermal_mass_kg: 1_450,
        },
        // Aqueous default: water at Earth-standard.
        _ => SubstrateProperties {
            freeze_point_k: Real::from_ratio(27_315, 100),
            boil_ref_k: Real::from_ratio(37_315, 100),
            l_fusion: L_FUSION_WATER,
            l_vaporisation: L_VAPORISATION_WATER,
            r_specific: R_SPECIFIC_WATER,
            c_p: C_P_WATER,
            cell_thermal_mass_kg: CELL_THERMAL_MASS_KG,
        },
    }
}
