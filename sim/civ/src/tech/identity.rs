//! `ToolKind` identity: the `ALL` and `TIER_FIVE` arrays plus the
//! `id`, `name`, `tier`, `prereq_channels`, `granted_channels`
//! property accessors. These are the enum's intrinsic shape — what
//! each tool *is*, not how it's gated or what it does to the civ.

use super::ToolKind;
use sim_recognition::ChannelKind;

impl ToolKind {
    pub const ALL: [ToolKind; 59] = [
        ToolKind::DistanceImaging,
        ToolKind::RemoteAcoustic,
        ToolKind::FieldSensor,
        ToolKind::ThermalSensor,
        ToolKind::MagneticSensor,
        ToolKind::BioelectricResonator,
        ToolKind::FieldPropulsionEngine,
        ToolKind::MetamaterialLattice,
        ToolKind::AmphibiousConstruction,
        // tier-1
        ToolKind::LocalisedCombustion,
        ToolKind::ContactWeapon,
        ToolKind::RangedMomentumWeapon,
        ToolKind::SimpleShelter,
        ToolKind::FoodProcessing,
        ToolKind::FluidGathering,
        ToolKind::BasicTextiles,
        ToolKind::StoneWorking,
        ToolKind::OrganizedHunting,
        ToolKind::BasicHealing,
        // tier-2
        ToolKind::BulkCultivation,
        ToolKind::AnimalSymbiosis,
        ToolKind::BulkStorage,
        ToolKind::MaterialRefining,
        ToolKind::CulturalEncoding,
        ToolKind::FluidControl,
        ToolKind::WatercraftConstruction,
        ToolKind::PermanentMasonry,
        ToolKind::TradeNetworks,
        ToolKind::UrbanConstruction,
        // tier-3 (AmphibiousConstruction already at id 9, listed above)
        ToolKind::ChemicalProjectile,
        ToolKind::PrecisionTimekeeping,
        ToolKind::MechanicalAdvantage,
        ToolKind::LongRangeNavigation,
        ToolKind::WrittenJurisprudence,
        ToolKind::AbstractMathematics,
        ToolKind::ArtisanalSpecialisation,
        ToolKind::DefensiveFortification,
        ToolKind::MotivePropulsion,
        // tier-4
        ToolKind::Mechanisation,
        ToolKind::LongRangeCommunication,
        ToolKind::ChemicalSynthesis,
        ToolKind::MedicalIntervention,
        ToolKind::AdvancedMaterials,
        ToolKind::HeavyTransport,
        ToolKind::PowerGeneration,
        ToolKind::AnalyticalEngines,
        ToolKind::MassLiteracy,
        ToolKind::AerialTransport,
        // tier-5 (information-age — coexists with the
        // pre-existing tier-5 transcendence trio at ids 6-8)
        ToolKind::DigitalComputation,
        ToolKind::InformationNetworking,
        ToolKind::GeneticManipulation,
        ToolKind::OrbitalReach,
        ToolKind::AdvancedMedicine,
        ToolKind::MaterialFabrication,
        ToolKind::AutonomousSystems,
        ToolKind::EnergyStorage,
        ToolKind::CryogenicEngineering,
        ToolKind::OrganicSynthesis,
        // tier-2 (capability): controlled-conditions apparatus.
        ToolKind::ExperimentApparatus,
    ];

    /// Tier-5 tools — the late-game capabilities. Used by 's
    /// `transcendence` run-end check (a civ that unlocks all three
    /// after a sustained existence has reached the tech-tree
    /// summit).
    pub const TIER_FIVE: [ToolKind; 3] = [
        ToolKind::BioelectricResonator,
        ToolKind::FieldPropulsionEngine,
        ToolKind::MetamaterialLattice,
    ];

    pub fn id(self) -> u32 {
        match self {
            ToolKind::DistanceImaging => 1,
            ToolKind::RemoteAcoustic => 2,
            ToolKind::FieldSensor => 3,
            ToolKind::ThermalSensor => 4,
            ToolKind::MagneticSensor => 5,
            ToolKind::BioelectricResonator => 6,
            ToolKind::FieldPropulsionEngine => 7,
            ToolKind::MetamaterialLattice => 8,
            ToolKind::AmphibiousConstruction => 9,
            // tier-1: ids 10-19
            ToolKind::LocalisedCombustion => 10,
            ToolKind::ContactWeapon => 11,
            ToolKind::RangedMomentumWeapon => 12,
            ToolKind::SimpleShelter => 13,
            ToolKind::FoodProcessing => 14,
            ToolKind::FluidGathering => 15,
            ToolKind::BasicTextiles => 16,
            ToolKind::StoneWorking => 17,
            ToolKind::OrganizedHunting => 18,
            ToolKind::BasicHealing => 19,
            // tier-2: ids 20-29
            ToolKind::BulkCultivation => 20,
            ToolKind::AnimalSymbiosis => 21,
            ToolKind::BulkStorage => 22,
            ToolKind::MaterialRefining => 23,
            ToolKind::CulturalEncoding => 24,
            ToolKind::FluidControl => 25,
            ToolKind::WatercraftConstruction => 26,
            ToolKind::PermanentMasonry => 27,
            ToolKind::TradeNetworks => 28,
            ToolKind::UrbanConstruction => 29,
            // tier-3: ids 30-38 (AmphibiousConstruction at 9
            // is the 10th tier-3 tool but pre-).
            ToolKind::ChemicalProjectile => 30,
            ToolKind::PrecisionTimekeeping => 31,
            ToolKind::MechanicalAdvantage => 32,
            ToolKind::LongRangeNavigation => 33,
            ToolKind::WrittenJurisprudence => 34,
            ToolKind::AbstractMathematics => 35,
            ToolKind::ArtisanalSpecialisation => 36,
            ToolKind::DefensiveFortification => 37,
            ToolKind::MotivePropulsion => 38,
            // tier-4: ids 39-48
            ToolKind::Mechanisation => 39,
            ToolKind::LongRangeCommunication => 40,
            ToolKind::ChemicalSynthesis => 41,
            ToolKind::MedicalIntervention => 42,
            ToolKind::AdvancedMaterials => 43,
            ToolKind::HeavyTransport => 44,
            ToolKind::PowerGeneration => 45,
            ToolKind::AnalyticalEngines => 46,
            ToolKind::MassLiteracy => 47,
            ToolKind::AerialTransport => 48,
            // tier-5: ids 49-58 (information-age). The
            // pre-existing transcendence trio occupies ids 6-8.
            ToolKind::DigitalComputation => 49,
            ToolKind::InformationNetworking => 50,
            ToolKind::GeneticManipulation => 51,
            ToolKind::OrbitalReach => 52,
            ToolKind::AdvancedMedicine => 53,
            ToolKind::MaterialFabrication => 54,
            ToolKind::AutonomousSystems => 55,
            ToolKind::EnergyStorage => 56,
            ToolKind::CryogenicEngineering => 57,
            ToolKind::OrganicSynthesis => 58,
            // id 59 — first id past the tier-5 block.
            ToolKind::ExperimentApparatus => 59,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ToolKind::DistanceImaging => "distance_imaging",
            ToolKind::RemoteAcoustic => "remote_acoustic",
            ToolKind::FieldSensor => "field_sensor",
            ToolKind::ThermalSensor => "thermal_sensor",
            ToolKind::MagneticSensor => "magnetic_sensor",
            ToolKind::BioelectricResonator => "bioelectric_resonator",
            ToolKind::FieldPropulsionEngine => "field_propulsion_engine",
            ToolKind::MetamaterialLattice => "metamaterial_lattice",
            ToolKind::AmphibiousConstruction => "amphibious_construction",
            // tier-1
            ToolKind::LocalisedCombustion => "localised_combustion",
            ToolKind::ContactWeapon => "contact_weapon",
            ToolKind::RangedMomentumWeapon => "ranged_momentum_weapon",
            ToolKind::SimpleShelter => "simple_shelter",
            ToolKind::FoodProcessing => "food_processing",
            ToolKind::FluidGathering => "fluid_gathering",
            ToolKind::BasicTextiles => "basic_textiles",
            ToolKind::StoneWorking => "stone_working",
            ToolKind::OrganizedHunting => "organized_hunting",
            ToolKind::BasicHealing => "basic_healing",
            // tier-2
            ToolKind::BulkCultivation => "bulk_cultivation",
            ToolKind::AnimalSymbiosis => "animal_symbiosis",
            ToolKind::BulkStorage => "bulk_storage",
            ToolKind::MaterialRefining => "material_refining",
            ToolKind::CulturalEncoding => "cultural_encoding",
            ToolKind::FluidControl => "fluid_control",
            ToolKind::WatercraftConstruction => "watercraft_construction",
            ToolKind::PermanentMasonry => "permanent_masonry",
            ToolKind::TradeNetworks => "trade_networks",
            ToolKind::UrbanConstruction => "urban_construction",
            // tier-3
            ToolKind::ChemicalProjectile => "chemical_projectile",
            ToolKind::PrecisionTimekeeping => "precision_timekeeping",
            ToolKind::MechanicalAdvantage => "mechanical_advantage",
            ToolKind::LongRangeNavigation => "long_range_navigation",
            ToolKind::WrittenJurisprudence => "written_jurisprudence",
            ToolKind::AbstractMathematics => "abstract_mathematics",
            ToolKind::ArtisanalSpecialisation => "artisanal_specialisation",
            ToolKind::DefensiveFortification => "defensive_fortification",
            ToolKind::MotivePropulsion => "motive_propulsion",
            // tier-4
            ToolKind::Mechanisation => "mechanisation",
            ToolKind::LongRangeCommunication => "long_range_communication",
            ToolKind::ChemicalSynthesis => "chemical_synthesis",
            ToolKind::MedicalIntervention => "medical_intervention",
            ToolKind::AdvancedMaterials => "advanced_materials",
            ToolKind::HeavyTransport => "heavy_transport",
            ToolKind::PowerGeneration => "power_generation",
            ToolKind::AnalyticalEngines => "analytical_engines",
            ToolKind::MassLiteracy => "mass_literacy",
            ToolKind::AerialTransport => "aerial_transport",
            // tier-5
            ToolKind::DigitalComputation => "digital_computation",
            ToolKind::InformationNetworking => "information_networking",
            ToolKind::GeneticManipulation => "genetic_manipulation",
            ToolKind::OrbitalReach => "orbital_reach",
            ToolKind::AdvancedMedicine => "advanced_medicine",
            ToolKind::MaterialFabrication => "material_fabrication",
            ToolKind::AutonomousSystems => "autonomous_systems",
            ToolKind::EnergyStorage => "energy_storage",
            ToolKind::CryogenicEngineering => "cryogenic_engineering",
            ToolKind::OrganicSynthesis => "organic_synthesis",
            //
            ToolKind::ExperimentApparatus => "experiment_apparatus",
        }
    }

    /// tier. Same axis as persistence tiers. Match arms
    /// enumerated per tool for readability even where adjacent
    /// arms produce the same value.
    #[allow(clippy::match_same_arms)]
    pub fn tier(self) -> u8 {
        match self {
            ToolKind::ThermalSensor => 2,
            ToolKind::RemoteAcoustic => 2,
            ToolKind::DistanceImaging => 3,
            ToolKind::FieldSensor => 3,
            ToolKind::MagneticSensor => 4,
            ToolKind::BioelectricResonator => 5,
            ToolKind::FieldPropulsionEngine => 5,
            ToolKind::MetamaterialLattice => 5,
            ToolKind::AmphibiousConstruction => 3,
            // tier-1
            ToolKind::LocalisedCombustion => 1,
            ToolKind::ContactWeapon => 1,
            ToolKind::RangedMomentumWeapon => 1,
            ToolKind::SimpleShelter => 1,
            ToolKind::FoodProcessing => 1,
            ToolKind::FluidGathering => 1,
            ToolKind::BasicTextiles => 1,
            ToolKind::StoneWorking => 1,
            ToolKind::OrganizedHunting => 1,
            ToolKind::BasicHealing => 1,
            // tier-2
            ToolKind::BulkCultivation => 2,
            ToolKind::AnimalSymbiosis => 2,
            ToolKind::BulkStorage => 2,
            ToolKind::MaterialRefining => 2,
            ToolKind::CulturalEncoding => 2,
            ToolKind::FluidControl => 2,
            ToolKind::WatercraftConstruction => 2,
            ToolKind::PermanentMasonry => 2,
            ToolKind::TradeNetworks => 2,
            ToolKind::UrbanConstruction => 2,
            // tier-3
            ToolKind::ChemicalProjectile => 3,
            ToolKind::PrecisionTimekeeping => 3,
            ToolKind::MechanicalAdvantage => 3,
            ToolKind::LongRangeNavigation => 3,
            ToolKind::WrittenJurisprudence => 3,
            ToolKind::AbstractMathematics => 3,
            ToolKind::ArtisanalSpecialisation => 3,
            ToolKind::DefensiveFortification => 3,
            ToolKind::MotivePropulsion => 3,
            // tier-4
            ToolKind::Mechanisation => 4,
            ToolKind::LongRangeCommunication => 4,
            ToolKind::ChemicalSynthesis => 4,
            ToolKind::MedicalIntervention => 4,
            ToolKind::AdvancedMaterials => 4,
            ToolKind::HeavyTransport => 4,
            ToolKind::PowerGeneration => 4,
            ToolKind::AnalyticalEngines => 4,
            ToolKind::MassLiteracy => 4,
            ToolKind::AerialTransport => 4,
            // tier-5: information-age. Same tier as the
            // pre-existing transcendence trio (Bioelectric /
            // FieldPropulsion / Metamaterial); the differentiator
            // is `species_maturity_floor` — only the trio gates on
            // species-cumulative confirmed-relations.
            ToolKind::DigitalComputation => 5,
            ToolKind::InformationNetworking => 5,
            ToolKind::GeneticManipulation => 5,
            ToolKind::OrbitalReach => 5,
            ToolKind::AdvancedMedicine => 5,
            ToolKind::MaterialFabrication => 5,
            ToolKind::AutonomousSystems => 5,
            ToolKind::EnergyStorage => 5,
            ToolKind::CryogenicEngineering => 5,
            ToolKind::OrganicSynthesis => 5,
            // tier-2 capability — same scale as ThermalSensor /
            // RemoteAcoustic. Buildable mid-civ once observation
            // pressure has accumulated.
            ToolKind::ExperimentApparatus => 2,
        }
    }

    /// Modality channel(s) a species needs to natively possess to
    /// be capable of building this tool. An empty list means no
    /// native-channel prereq (the tool is a synthetic substitute
    /// for a sense the species lacks). Match arms enumerated for
    /// readability.
    #[allow(clippy::match_same_arms)]
    pub fn prereq_channels(self) -> &'static [ChannelKind] {
        match self {
            // Distance-imaging extends an existing visual sense; if
            // the species has no light perception at all, the tool
            // can't be built.
            ToolKind::DistanceImaging => &[ChannelKind::VisualLight],
            // Acoustic-extension needs an existing acoustic sense.
            ToolKind::RemoteAcoustic => &[ChannelKind::AcousticAir, ChannelKind::AcousticWater],
            // Synthetic substitutes — no native-channel prereq.
            ToolKind::FieldSensor => &[],
            ToolKind::ThermalSensor => &[],
            // Magnetic sensor still requires the planet to have a
            // magnetic field the tool can register; that's
            // enforced via the planet-prereq check at unlock time
            // rather than the species-channel prereq.
            ToolKind::MagneticSensor => &[],
            // Bioelectric resonator — the species' tools to read
            // its own field signatures. Needs a sense for fields
            // already (electric or magnetic) so the engineering
            // tradition has an observational basis.
            ToolKind::BioelectricResonator => {
                &[ChannelKind::ElectricField, ChannelKind::MagneticSense]
            }
            // Field propulsion + metamaterial lattice — narrative
            // capabilities; no native-channel prereq beyond the
            // crust + magnetosphere checks at unlock time.
            ToolKind::FieldPropulsionEngine => &[],
            ToolKind::MetamaterialLattice => &[],
            // AmphibiousConstruction: synthetic substitute for the
            // habitat the species lacks; no native-channel prereq.
            ToolKind::AmphibiousConstruction => &[],
            // tier-1: applied-knowledge tools, no native-
            // channel prereq. The species' baseline manipulation
            // suffices; observation pressure expresses readiness.
            ToolKind::LocalisedCombustion
            | ToolKind::ContactWeapon
            | ToolKind::RangedMomentumWeapon
            | ToolKind::SimpleShelter
            | ToolKind::FoodProcessing
            | ToolKind::FluidGathering
            | ToolKind::BasicTextiles
            | ToolKind::StoneWorking
            | ToolKind::OrganizedHunting
            | ToolKind::BasicHealing => &[],
            // tier-2: same — capability tools don't require
            // a particular sense; substrate divergence is enforced
            // by relation_prereqs and (transitively) by tier-1
            // tool_prereqs that gate on substrate-specific
            // observations.
            ToolKind::BulkCultivation
            | ToolKind::AnimalSymbiosis
            | ToolKind::BulkStorage
            | ToolKind::MaterialRefining
            | ToolKind::CulturalEncoding
            | ToolKind::FluidControl
            | ToolKind::WatercraftConstruction
            | ToolKind::PermanentMasonry
            | ToolKind::TradeNetworks
            | ToolKind::UrbanConstruction => &[],
            // tier-3: same — substrate divergence enforced
            // by relation_prereqs + chained tool_prereqs.
            ToolKind::ChemicalProjectile
            | ToolKind::PrecisionTimekeeping
            | ToolKind::MechanicalAdvantage
            | ToolKind::LongRangeNavigation
            | ToolKind::WrittenJurisprudence
            | ToolKind::AbstractMathematics
            | ToolKind::ArtisanalSpecialisation
            | ToolKind::DefensiveFortification
            | ToolKind::MotivePropulsion => &[],
            // tier-4: same.
            ToolKind::Mechanisation
            | ToolKind::LongRangeCommunication
            | ToolKind::ChemicalSynthesis
            | ToolKind::MedicalIntervention
            | ToolKind::AdvancedMaterials
            | ToolKind::HeavyTransport
            | ToolKind::PowerGeneration
            | ToolKind::AnalyticalEngines
            | ToolKind::MassLiteracy
            | ToolKind::AerialTransport => &[],
            // tier-5 (information-age): same.
            ToolKind::DigitalComputation
            | ToolKind::InformationNetworking
            | ToolKind::GeneticManipulation
            | ToolKind::OrbitalReach
            | ToolKind::AdvancedMedicine
            | ToolKind::MaterialFabrication
            | ToolKind::AutonomousSystems
            | ToolKind::EnergyStorage
            | ToolKind::CryogenicEngineering
            | ToolKind::OrganicSynthesis => &[],
            // experiment apparatus is a *generic* intervention
            // device — no native-channel prereq. Its
            // `manipulation_prereqs` (broad — every kind) does the
            // body-plan gating in `is_buildable`.
            ToolKind::ExperimentApparatus => &[],
        }
    }

    /// Channels granted on unlock. The civ's perceivable-template
    /// set unions these with species-native modalities; templates
    /// whose own channel list now intersects fire for the civ.
    /// Match arms enumerated per tool for clarity.
    #[allow(clippy::match_same_arms)]
    pub fn granted_channels(self) -> &'static [ChannelKind] {
        match self {
            // Distance-imaging doesn't grant a new channel — it
            // raises the *range* on existing visual perception.
            // M3 has no range-gated templates; the tool registers
            // as unlocked but adds no new channels until the
            // template catalog grows.
            ToolKind::DistanceImaging => &[],
            ToolKind::RemoteAcoustic => &[],
            ToolKind::FieldSensor => &[ChannelKind::ElectricField],
            ToolKind::ThermalSensor => &[ChannelKind::InfraredThermal],
            ToolKind::MagneticSensor => &[ChannelKind::MagneticSense],
            // Tier-5 tools are narrative milestones; they don't
            // grant new perceptual channels. Their effect on the
            // sim is the `TechUnlocked` event itself plus their
            // contribution to the transcendence trigger.
            ToolKind::BioelectricResonator => &[],
            ToolKind::FieldPropulsionEngine => &[],
            ToolKind::MetamaterialLattice => &[],
            // AmphibiousConstruction: mechanical effect is
            // habitat-gate lift in `compute_territory`; no
            // perceptual channel granted.
            ToolKind::AmphibiousConstruction => &[],
            // tier-1: capability tools, not sensorium tools —
            // they apply observed knowledge to civ-level outcomes
            // rather than extending perception. No perceptual
            // channel granted.
            ToolKind::LocalisedCombustion
            | ToolKind::ContactWeapon
            | ToolKind::RangedMomentumWeapon
            | ToolKind::SimpleShelter
            | ToolKind::FoodProcessing
            | ToolKind::FluidGathering
            | ToolKind::BasicTextiles
            | ToolKind::StoneWorking
            | ToolKind::OrganizedHunting
            | ToolKind::BasicHealing => &[],
            // tier-2: same — no perceptual channels.
            ToolKind::BulkCultivation
            | ToolKind::AnimalSymbiosis
            | ToolKind::BulkStorage
            | ToolKind::MaterialRefining
            | ToolKind::CulturalEncoding
            | ToolKind::FluidControl
            | ToolKind::WatercraftConstruction
            | ToolKind::PermanentMasonry
            | ToolKind::TradeNetworks
            | ToolKind::UrbanConstruction => &[],
            // tier-3: same — no perceptual channels.
            ToolKind::ChemicalProjectile
            | ToolKind::PrecisionTimekeeping
            | ToolKind::MechanicalAdvantage
            | ToolKind::LongRangeNavigation
            | ToolKind::WrittenJurisprudence
            | ToolKind::AbstractMathematics
            | ToolKind::ArtisanalSpecialisation
            | ToolKind::DefensiveFortification
            | ToolKind::MotivePropulsion => &[],
            // tier-4: same.
            ToolKind::Mechanisation
            | ToolKind::LongRangeCommunication
            | ToolKind::ChemicalSynthesis
            | ToolKind::MedicalIntervention
            | ToolKind::AdvancedMaterials
            | ToolKind::HeavyTransport
            | ToolKind::PowerGeneration
            | ToolKind::AnalyticalEngines
            | ToolKind::MassLiteracy
            | ToolKind::AerialTransport => &[],
            // tier-5 (information-age): same.
            ToolKind::DigitalComputation
            | ToolKind::InformationNetworking
            | ToolKind::GeneticManipulation
            | ToolKind::OrbitalReach
            | ToolKind::AdvancedMedicine
            | ToolKind::MaterialFabrication
            | ToolKind::AutonomousSystems
            | ToolKind::EnergyStorage
            | ToolKind::CryogenicEngineering
            | ToolKind::OrganicSynthesis => &[],
            // experiment apparatus grants no perceptual channel
            // — the effect is on the discovery layer (experimental
            // samples flow into the existing measurement track), not
            // on perception.
            ToolKind::ExperimentApparatus => &[],
        }
    }
}
