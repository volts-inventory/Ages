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
use crate::state::N_SUBSTANCES;
use sim_arith::transcendental::ln;
use sim_arith::Real;

/// Substrate identifier within `sim-physics`. Mirrors the
/// `sim_world::MetabolicSubstrate` enum so the chemistry layer can
/// expose substrate-coupled solvent semantics (solubility,
/// reaction kinetics) without taking a circular dependency on
/// `sim-world`. Callers in higher crates map their own
/// `MetabolicSubstrate` to this via the obvious 1-to-1 conversion
/// (or use the string-tag form via `*_for_tag`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetabolicSubstrate {
    Aqueous,
    Ammoniacal,
    Hydrocarbon,
    Silicate,
}

impl MetabolicSubstrate {
    /// String tag (`"aqueous"`, `"ammoniacal"`, `"hydrocarbon"`,
    /// `"silicate"`) — matches the existing `substrate_tag` form
    /// used by `substrate_properties` and `Chemistry::for_planet`.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            MetabolicSubstrate::Aqueous => "aqueous",
            MetabolicSubstrate::Ammoniacal => "ammoniacal",
            MetabolicSubstrate::Hydrocarbon => "hydrocarbon",
            MetabolicSubstrate::Silicate => "silicate",
        }
    }
}

/// Per-substrate dissolution propensity for every `Substance` in
/// the simulation. Indexed by `Substance::idx()`; each value lies
/// in `[0, 1]`. `0.0` = effectively insoluble; `1.0` = fully
/// dissolves. Water dissolves many salts (`Aqueous` baseline);
/// liquid ammonia is moderately polar; methane / ethane dissolves
/// almost nothing inorganic but is the natural solvent for
/// hydrocarbons; molten silicate dissolves silicate minerals.
///
/// Used by downstream chemistry / lifecycle / civ-extraction code
/// to gate "does this substance exist as a dissolved fraction in
/// the planet's solvent?" without hard-coding water-chauvinism.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SolubilityProfile {
    pub per_substance: [Real; N_SUBSTANCES],
}

/// Per-substrate dissolution table. Rough order-of-magnitude
/// values keyed to the existing `Substance` enum
/// (Water=0, Ice=1, Vapour=2, Fuel=3, Oxidiser=4, Ash=5,
/// Fossil=6, CO2=7). Values are dimensionless propensities, not
/// thermodynamic solubility constants — they parameterise how
/// readily each substance enters the planet's solvent phase for
/// the higher-level simulation, not Henry's-law equilibria.
#[must_use]
pub fn solvent_solubility(substrate: &MetabolicSubstrate) -> SolubilityProfile {
    // Helper: build the table from per-substance percent values
    // (0..=100) so the literals stay readable and fixed-point-exact.
    const fn pct_arr(arr: [i64; N_SUBSTANCES]) -> [i64; N_SUBSTANCES] {
        arr
    }
    let pcts: [i64; N_SUBSTANCES] = match substrate {
        // Water dissolves a wide spectrum: itself trivially (1.0),
        // ice not at all (already solid), vapour partially (0.5
        // represents the vapour-pressure-equilibrium leak), fuel
        // poorly (organics are mostly hydrophobic), oxidiser
        // appreciably (O2 dissolves), ash modestly, fossils very
        // little (asphaltenes / kerogens are nearly insoluble),
        // CO2 appreciably (carbonic acid forms).
        MetabolicSubstrate::Aqueous => pct_arr([100, 0, 50, 5, 30, 40, 1, 30]),
        // Liquid ammonia is moderately polar — dissolves water
        // (0.6, miscible with water in real chemistry) and a
        // moderate fraction of everything else (~0.3 baseline).
        MetabolicSubstrate::Ammoniacal => pct_arr([60, 30, 30, 30, 30, 30, 30, 30]),
        // Methane / ethane: nonpolar — dissolves fuel (organics)
        // very well (0.95), fossils well (0.8). Inorganic
        // substances barely dissolve (~0.05 baseline). Cryogenic
        // liquid hydrocarbon is a near-universal solvent for
        // organics and a near-universal non-solvent for
        // everything else — including CO2.
        MetabolicSubstrate::Hydrocarbon => pct_arr([5, 5, 5, 95, 5, 5, 80, 5]),
        // Molten silicate (magma): dissolves silicate minerals
        // (ash, ~0.4 — silicate analog) well; carries everything
        // else at ~0.1.
        MetabolicSubstrate::Silicate => pct_arr([10, 10, 10, 10, 10, 40, 10, 10]),
    };
    let mut per_substance = [Real::ZERO; N_SUBSTANCES];
    let mut i = 0;
    while i < N_SUBSTANCES {
        per_substance[i] = Real::from_ratio(pcts[i], 100);
        i += 1;
    }
    SolubilityProfile { per_substance }
}

/// Arrhenius-like per-substrate reaction-kinetics prefactor.
/// Multiplies reaction rates in the chemistry step so cold
/// substrates run slow chemistry and hot ones run fast — the
/// substrate's mean liquid temperature compresses or stretches
/// the effective activation-energy timescale.
///
/// Aqueous = 1.0 baseline. Ammoniacal (195-240 K) is ~0.4×
/// (colder, slower). Hydrocarbon (90-180 K) is ~0.05× (very cold,
/// very slow). Silicate (800-1500 K) is ~5× (very hot, very fast).
///
/// These are order-of-magnitude multipliers tuned to keep the
/// per-substrate `Chemistry` step physically plausible without
/// recomputing the full Arrhenius equation per cell.
#[must_use]
pub fn solvent_reaction_kinetics_prefactor(substrate: &MetabolicSubstrate) -> Real {
    match substrate {
        MetabolicSubstrate::Aqueous => Real::ONE,
        MetabolicSubstrate::Ammoniacal => Real::from_ratio(4, 10),
        MetabolicSubstrate::Hydrocarbon => Real::from_ratio(5, 100),
        MetabolicSubstrate::Silicate => Real::from_int(5),
    }
}

/// Map a substrate string tag to the local `MetabolicSubstrate`
/// enum. Unrecognised tags fall back to `Aqueous`, matching the
/// existing `substrate_properties` default behaviour.
#[must_use]
pub fn substrate_from_tag(substrate_tag: &str) -> MetabolicSubstrate {
    match substrate_tag {
        "ammoniacal" => MetabolicSubstrate::Ammoniacal,
        "hydrocarbon" => MetabolicSubstrate::Hydrocarbon,
        "silicate" => MetabolicSubstrate::Silicate,
        _ => MetabolicSubstrate::Aqueous,
    }
}

/// String-tag form of `solvent_reaction_kinetics_prefactor` for
/// callers that already carry a `substrate_tag: &str` (such as
/// `Chemistry::for_planet`).
#[must_use]
pub fn solvent_reaction_kinetics_prefactor_for_tag(substrate_tag: &str) -> Real {
    solvent_reaction_kinetics_prefactor(&substrate_from_tag(substrate_tag))
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chemistry::Substance;

    #[test]
    fn methane_substrate_solubility_excludes_most_substances() {
        // Cryogenic liquid hydrocarbon dissolves organics
        // (Fuel) almost completely but barely touches inorganic
        // substances. Water dissolution in methane is effectively
        // zero (ice forms on contact); fuel dissolution is near
        // unity.
        let p = solvent_solubility(&MetabolicSubstrate::Hydrocarbon);
        let water_solubility = p.per_substance[Substance::Water.idx()];
        let fuel_solubility = p.per_substance[Substance::Fuel.idx()];
        assert!(
            water_solubility < Real::percent(10),
            "methane should not dissolve water (got {water_solubility:?})"
        );
        assert!(
            fuel_solubility > Real::percent(50),
            "methane should dissolve fuel readily (got {fuel_solubility:?})"
        );
    }

    #[test]
    fn ammoniacal_solvent_reaction_kinetics_match_published() {
        // Cold solvents have lower Arrhenius prefactors than
        // warm ones. Spec: ammoniacal ≤ 0.5 and strictly
        // less than aqueous.
        let aqueous = solvent_reaction_kinetics_prefactor(&MetabolicSubstrate::Aqueous);
        let ammoniacal = solvent_reaction_kinetics_prefactor(&MetabolicSubstrate::Ammoniacal);
        assert!(
            ammoniacal < aqueous,
            "ammoniacal kinetics should be slower than aqueous \
             (got ammoniacal={ammoniacal:?}, aqueous={aqueous:?})"
        );
        assert!(
            ammoniacal <= Real::from_ratio(5, 10),
            "ammoniacal kinetics prefactor should be at most 0.5 \
             (got {ammoniacal:?})"
        );
    }

    #[test]
    fn silicate_kinetics_faster_than_hydrocarbon() {
        // Hot substrates should run chemistry quickly; cold
        // substrates slowly. Anchors the four-point ordering.
        let silicate = solvent_reaction_kinetics_prefactor(&MetabolicSubstrate::Silicate);
        let hydrocarbon = solvent_reaction_kinetics_prefactor(&MetabolicSubstrate::Hydrocarbon);
        assert!(
            silicate > hydrocarbon,
            "silicate (hot) should run faster than hydrocarbon (cold)"
        );
    }

    #[test]
    fn solubility_values_are_bounded() {
        // Every entry in every profile must lie in [0, 1].
        for substrate in [
            MetabolicSubstrate::Aqueous,
            MetabolicSubstrate::Ammoniacal,
            MetabolicSubstrate::Hydrocarbon,
            MetabolicSubstrate::Silicate,
        ] {
            let p = solvent_solubility(&substrate);
            for (i, v) in p.per_substance.iter().enumerate() {
                assert!(*v >= Real::ZERO, "substrate {substrate:?} entry {i} < 0");
                assert!(*v <= Real::ONE, "substrate {substrate:?} entry {i} > 1");
            }
        }
    }

    #[test]
    fn substrate_from_tag_round_trips_known_tags() {
        assert_eq!(
            substrate_from_tag("aqueous"),
            MetabolicSubstrate::Aqueous
        );
        assert_eq!(
            substrate_from_tag("ammoniacal"),
            MetabolicSubstrate::Ammoniacal
        );
        assert_eq!(
            substrate_from_tag("hydrocarbon"),
            MetabolicSubstrate::Hydrocarbon
        );
        assert_eq!(
            substrate_from_tag("silicate"),
            MetabolicSubstrate::Silicate
        );
        // Unknown falls back to aqueous (matching
        // substrate_properties).
        assert_eq!(
            substrate_from_tag("unknown_garbage"),
            MetabolicSubstrate::Aqueous
        );
    }
}
