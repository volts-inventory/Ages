//! `ToolKind::tool_prereqs` — earlier `ToolKind`s that must already
//! be unlocked. Extracted from `specs.rs` to keep the per-tool
//! dependency chain together in one place.

use super::super::ToolKind;

impl ToolKind {
    /// tool prereq: earlier `ToolKind`s that must already be
    /// in the civ's `unlocked_tools` for this one to unlock. Empty
    /// for the original 9 tools (they were authored as standalone
    /// unlocks); future capability tools (+) will use this to
    /// build longer chains.
    ///
    /// Invariant — checked by the `tool_prereqs_form_a_dag` test:
    /// every prereq must have a strictly lower `tier()` than the
    /// dependent tool, which keeps the dependency graph acyclic
    /// without runtime traversal.
    #[allow(clippy::match_same_arms)]
    pub fn tool_prereqs(self) -> &'static [ToolKind] {
        match self {
            ToolKind::DistanceImaging => &[],
            ToolKind::RemoteAcoustic => &[],
            ToolKind::FieldSensor => &[],
            ToolKind::ThermalSensor => &[],
            ToolKind::MagneticSensor => &[],
            ToolKind::BioelectricResonator => &[],
            ToolKind::FieldPropulsionEngine => &[],
            ToolKind::MetamaterialLattice => &[],
            ToolKind::AmphibiousConstruction => &[],
            // tier-1: all standalone — parallel discoveries
            // from observation alone. FoodProcessing notionally
            // builds on LocalisedCombustion, but both share the
            // same `confirmed(fire, temperature)` relation prereq,
            // so the substrate gate is enforced at the relation
            // layer rather than the tool layer (avoids same-tier
            // tool prereq, which would violate the strict
            // tier-monotonicity DAG invariant). Tier-2 tools that
            // genuinely depend on tier-1 outputs (e.g.
            // MaterialRefining → LocalisedCombustion + crust obs)
            // use `tool_prereqs` properly because the tier gap is
            // strictly positive there.
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

            // ─── tier-2 tool prereqs ───
            //
            // Tier-2 chains through tier-1 outputs. All prereqs
            // listed are tier 1, so the strict tier-monotonicity
            // DAG invariant holds. Substrate divergence enforced
            // through the relation_prereqs above + transitively
            // through the tier-1 tools' own gates.
            //
            // BulkCultivation: builds on FoodProcessing — cultivated
            // food has to be cookable to be the species' staple.
            ToolKind::BulkCultivation => &[ToolKind::FoodProcessing],
            // AnimalSymbiosis: builds on OrganizedHunting — you
            // learn the animal first by hunting it.
            ToolKind::AnimalSymbiosis => &[ToolKind::OrganizedHunting],
            // BulkStorage: builds on LocalisedCombustion — pottery
            // firing.
            ToolKind::BulkStorage => &[ToolKind::LocalisedCombustion],
            // MaterialRefining: builds on LocalisedCombustion
            // (smelting heat) AND StoneWorking (the prior craft
            // tradition the metallurgist refines).
            ToolKind::MaterialRefining => &[ToolKind::LocalisedCombustion, ToolKind::StoneWorking],
            // CulturalEncoding: builds on BasicTextiles — a writing
            // surface. (Could equally be on MaterialRefining for
            // clay tablets, but tying it to fibre keeps the
            // substrate-divergence path open: a no-fire civ can
            // still encode if it has plant fibres.)
            ToolKind::CulturalEncoding => &[ToolKind::BasicTextiles],
            // FluidControl: builds on FluidGathering — irrigation
            // beyond hand-carrying.
            ToolKind::FluidControl => &[ToolKind::FluidGathering],
            // WatercraftConstruction: builds on StoneWorking —
            // hull-shaping needs the carving / shaping tradition.
            ToolKind::WatercraftConstruction => &[ToolKind::StoneWorking],
            // PermanentMasonry: builds on StoneWorking + SimpleShelter
            // — the craft tradition + the building-pattern.
            ToolKind::PermanentMasonry => &[ToolKind::StoneWorking, ToolKind::SimpleShelter],
            // TradeNetworks: no physical-engineering tool prereq;
            // emerges from settled bands' surplus + barter
            // conventions.
            ToolKind::TradeNetworks => &[],
            // UrbanConstruction: builds on SimpleShelter (the
            // residential pattern). PermanentMasonry would also
            // make sense, but it's same-tier and same-tier
            // prereqs violate the strict-DAG invariant — so the
            // substrate gate uses tier-1 SimpleShelter, and the
            // relation pathway leaves UrbanConstruction reachable
            // for civs that haven't industrialised stone-working.
            ToolKind::UrbanConstruction => &[ToolKind::SimpleShelter],

            // ─── tier-3 tool prereqs ───
            //
            // ChemicalProjectile: combustion-derived branch —
            // requires both metallurgy AND sealed containers
            // (gunpowder needs a casing). MaterialRefining and
            // BulkStorage are both tier 2.
            ToolKind::ChemicalProjectile => &[ToolKind::MaterialRefining, ToolKind::BulkStorage],
            // PrecisionTimekeeping: needs CulturalEncoding to
            // record the periodic observations across years.
            ToolKind::PrecisionTimekeeping => &[ToolKind::CulturalEncoding],
            // MechanicalAdvantage: alternate path that does NOT
            // depend on combustion. Tier-1 StoneWorking is the
            // craft tradition. Per the agreed design (see commit
            // message): this opens Mechanisation to no-fire civs.
            ToolKind::MechanicalAdvantage => &[ToolKind::StoneWorking],
            // LongRangeNavigation: WatercraftConstruction (sea-
            // faring) + CulturalEncoding (charts).
            ToolKind::LongRangeNavigation => {
                &[ToolKind::WatercraftConstruction, ToolKind::CulturalEncoding]
            }
            // WrittenJurisprudence: CulturalEncoding (writing) +
            // TradeNetworks (the surplus that supports a legal
            // class).
            ToolKind::WrittenJurisprudence => {
                &[ToolKind::CulturalEncoding, ToolKind::TradeNetworks]
            }
            // AbstractMathematics: CulturalEncoding — formal
            // mathematics needs a recording substrate.
            ToolKind::AbstractMathematics => &[ToolKind::CulturalEncoding],
            // ArtisanalSpecialisation: TradeNetworks — the
            // exchange substrate that supports specialised
            // (non-self-sufficient) craft roles.
            ToolKind::ArtisanalSpecialisation => &[ToolKind::TradeNetworks],
            // DefensiveFortification: PermanentMasonry — the
            // construction tradition.
            ToolKind::DefensiveFortification => &[ToolKind::PermanentMasonry],
            // MotivePropulsion: WatercraftConstruction — the
            // hull substrate that propulsion mounts to.
            ToolKind::MotivePropulsion => &[ToolKind::WatercraftConstruction],

            // ─── tier-4 tool prereqs ───
            //
            // Mechanisation: ONLY MechanicalAdvantage (the alternate
            // path). Per the agreed substrate-divergence design,
            // a no-fire civ that reached MechanicalAdvantage at
            // tier 3 reaches Mechanisation at tier 4 too. The
            // fire-civ's MaterialRefining stacks separately as a
            // capacity multiplier, giving fire-civs an effectively
            // larger industrial age without locking out no-fire
            // civs entirely.
            ToolKind::Mechanisation => &[ToolKind::MechanicalAdvantage],
            // LongRangeCommunication: MaterialRefining (wire,
            // antennas, coils — metallurgy substrate).
            ToolKind::LongRangeCommunication => &[ToolKind::MaterialRefining],
            // ChemicalSynthesis: MaterialRefining (the chemistry
            // tradition the synthesist builds on).
            ToolKind::ChemicalSynthesis => &[ToolKind::MaterialRefining],
            // MedicalIntervention: BasicHealing (the herbal
            // tradition) + AbstractMathematics (formal physiology
            // and dosage models).
            ToolKind::MedicalIntervention => {
                &[ToolKind::BasicHealing, ToolKind::AbstractMathematics]
            }
            // AdvancedMaterials: MaterialRefining (alloys, ceramics,
            // and superconductors are all metallurgy descendents).
            ToolKind::AdvancedMaterials => &[ToolKind::MaterialRefining],
            // HeavyTransport: MotivePropulsion (engines) +
            // MechanicalAdvantage (roadbeds, levered loading).
            ToolKind::HeavyTransport => {
                &[ToolKind::MotivePropulsion, ToolKind::MechanicalAdvantage]
            }
            // PowerGeneration: MechanicalAdvantage (turbines are
            // mechanical; cells/reactors layer onto the same
            // engineering foundation).
            ToolKind::PowerGeneration => &[ToolKind::MechanicalAdvantage],
            // AnalyticalEngines: AbstractMathematics (formal logic)
            // + PrecisionTimekeeping (the clockwork tradition).
            ToolKind::AnalyticalEngines => &[
                ToolKind::AbstractMathematics,
                ToolKind::PrecisionTimekeeping,
            ],
            // MassLiteracy: WrittenJurisprudence (the formal-
            // legal substrate that motivates universal literacy).
            ToolKind::MassLiteracy => &[ToolKind::WrittenJurisprudence],
            // AerialTransport: MaterialRefining (lifting-gas
            // chemistry / structural alloys) + MotivePropulsion
            // (powered drive). This IS combustion-locked — a
            // no-fire civ doesn't reach AerialTransport. The
            // alternate-path argument doesn't extend to flight
            // because lighter-than-air craft genuinely need
            // chemistry the no-fire path doesn't reach.
            ToolKind::AerialTransport => &[ToolKind::MaterialRefining, ToolKind::MotivePropulsion],

            // ─── tier-5 tool prereqs ───
            //
            // DigitalComputation: AnalyticalEngines (the mechanical
            // computation tradition).
            ToolKind::DigitalComputation => &[ToolKind::AnalyticalEngines],
            // InformationNetworking: LongRangeCommunication.
            ToolKind::InformationNetworking => &[ToolKind::LongRangeCommunication],
            // GeneticManipulation: MedicalIntervention (the
            // physiology baseline) + ChemicalSynthesis (the
            // molecular toolset).
            ToolKind::GeneticManipulation => {
                &[ToolKind::MedicalIntervention, ToolKind::ChemicalSynthesis]
            }
            // OrbitalReach: Mechanisation + AerialTransport.
            // Inherits AerialTransport's combustion-lock through
            // MaterialRefining; no-fire civs don't reach orbit.
            ToolKind::OrbitalReach => &[ToolKind::Mechanisation, ToolKind::AerialTransport],
            // AdvancedMedicine: MedicalIntervention.
            ToolKind::AdvancedMedicine => &[ToolKind::MedicalIntervention],
            // MaterialFabrication: AdvancedMaterials.
            ToolKind::MaterialFabrication => &[ToolKind::AdvancedMaterials],
            // AutonomousSystems: Mechanisation + AnalyticalEngines.
            ToolKind::AutonomousSystems => &[ToolKind::Mechanisation, ToolKind::AnalyticalEngines],
            // EnergyStorage: PowerGeneration.
            ToolKind::EnergyStorage => &[ToolKind::PowerGeneration],
            // CryogenicEngineering: AdvancedMaterials.
            ToolKind::CryogenicEngineering => &[ToolKind::AdvancedMaterials],
            // OrganicSynthesis: ChemicalSynthesis. (GeneticManipulation
            // would be a natural same-tier reinforcement, but
            // same-tier prereqs violate strict-DAG.)
            ToolKind::OrganicSynthesis => &[ToolKind::ChemicalSynthesis],
            // no tool prereq. The apparatus is a primitive
            // intervention device — the civ doesn't need to have
            // built any other tool first. Substrate gate is
            // expressed at the relation-prereq layer (confirmed
            // fire) which already requires the civ's hypothesizer
            // to have done some real fitting work.
            ToolKind::ExperimentApparatus => &[],
            // HerbalMedicine: refines BasicHealing with extracts;
            // FluidGathering provides the water needed for tinctures
            // / decoctions. Both tier-1, strict tier monotonicity
            // satisfied.
            ToolKind::HerbalMedicine => &[ToolKind::BasicHealing, ToolKind::FluidGathering],
            // AcousticEngineering: the acoustic instrumentation
            // (RemoteAcoustic) supplies the math; the masonry
            // tradition (PermanentMasonry) supplies the resonant
            // chambers. Both tier-2.
            ToolKind::AcousticEngineering => {
                &[ToolKind::RemoteAcoustic, ToolKind::PermanentMasonry]
            }
        }
    }
}
