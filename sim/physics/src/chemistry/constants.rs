//! Substrate-relative chemistry constants: specific heats, latent
//! heats of fusion / vaporisation, specific gas constants,
//! reference pressure, the per-cell thermal mass, and the
//! Clausius-Clapeyron reference temperature for water.

/// Specific heat of water at constant pressure, J/(kg·K).
pub const C_P_WATER: i64 = 4_186;
/// Specific heat of liquid ammonia at constant pressure,
/// J/(kg·K). Higher than water — drives stronger temperature
/// swings per unit phase-change on ammoniacal worlds.
pub const C_P_AMMONIA: i64 = 4_700;
/// Specific heat of liquid methane at constant pressure,
/// J/(kg·K). Lower than water.
pub const C_P_METHANE: i64 = 3_500;
/// Specific heat of molten silicate at constant pressure,
/// J/(kg·K). Much lower than water — molten rock heats fast.
pub const C_P_SILICATE: i64 = 1_300;
/// Latent heat of fusion of water, J/kg (273.15 K, 1 atm).
pub const L_FUSION_WATER: i64 = 334_000;
/// Latent heat of vaporisation of water, J/kg (373.15 K, 1 atm).
pub const L_VAPORISATION_WATER: i64 = 2_257_000;
/// Latent heat of fusion of ammonia, J/kg (195.4 K).
pub const L_FUSION_AMMONIA: i64 = 339_000;
/// Latent heat of vaporisation of ammonia, J/kg (239.8 K, 1 atm).
pub const L_VAPORISATION_AMMONIA: i64 = 1_371_000;
/// Latent heat of fusion of methane, J/kg (90.7 K).
pub const L_FUSION_METHANE: i64 = 58_000;
/// Latent heat of vaporisation of methane, J/kg (111.7 K, 1 atm).
pub const L_VAPORISATION_METHANE: i64 = 510_000;
/// Latent heat of fusion of silicon, J/kg (1687 K).
pub const L_FUSION_SILICON: i64 = 1_790_000;
/// Latent heat of vaporisation of silicon, J/kg (3538 K, low-P
/// reference).
pub const L_VAPORISATION_SILICON: i64 = 12_500_000;
/// Approximate combustion enthalpy of dry wood, J/kg.
pub const COMBUSTION_ENTHALPY_WOOD: i64 = 16_000_000;
/// Approximate combustion enthalpy of fossil hydrocarbons, J/kg.
/// Sits between bituminous coal (~24 MJ/kg) and gasoline (~46 MJ/kg);
/// 42 MJ/kg is a reasonable mixed-fossil mean. Used by the fossil
/// combustion reaction so each unit of `Substance::Fossil` releases
/// about 2.6× the heat of an equivalent unit of biofuel.
pub const COMBUSTION_ENTHALPY_FOSSIL: i64 = 42_000_000;
/// Specific gas constant for water vapour, J/(kg·K). Used in
/// Clausius-Clapeyron: derived from the universal gas constant
/// 8.314 J/(mol·K) and water's molar mass 0.018015 kg/mol.
pub(crate) const R_SPECIFIC_WATER: i64 = 461;
/// Specific gas constant for ammonia, J/(kg·K). Universal R
/// (8.314) divided by NH3 molar mass 0.01703 kg/mol.
pub(crate) const R_SPECIFIC_AMMONIA: i64 = 488;
/// Specific gas constant for methane, J/(kg·K). Universal R
/// (8.314) divided by CH4 molar mass 0.01604 kg/mol.
pub(crate) const R_SPECIFIC_METHANE: i64 = 518;
/// Specific gas constant for silicon (vapour), J/(kg·K). Universal
/// R divided by Si molar mass 0.02809 kg/mol. Approximate — silicon
/// vapour at habitable-substrate temperatures is hypothetical.
pub(crate) const R_SPECIFIC_SILICON: i64 = 296;
/// Reference pressure for Clausius-Clapeyron: 1 atm in Pa.
pub const P_REF_ATM_PA: i64 = 101_325;
/// Effective thermal mass of one grid cell, in kg. Per-step
/// temperature change from a transition is
/// `m_transitioned * L / (CELL_THERMAL_MASS_KG * c_p)` —
/// dividing by the cell's effective thermal mass converts released
/// enthalpy into a sensible ΔT. Calibrated against the M1b rate
/// envelope so a full unit of vaporisation moves the cell by ≈ 1 K
/// per step. Refined when full-cell calorimetry lands.
pub const CELL_THERMAL_MASS_KG: i64 = 539;
