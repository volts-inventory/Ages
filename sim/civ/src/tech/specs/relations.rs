//! `ToolKind::relation_prereqs` — the (template_id, ChannelKind)
//! pairs each tool depends on. Extracted from `specs.rs` because
//! it's the largest of the tier-relation gates (~275 lines).

use super::super::ToolKind;
use sim_recognition::ChannelKind;

impl ToolKind {
    /// relation prereq: `(template_id, ChannelKind)` pairs the
    /// civ must have *confirmed* in its `Hypothesizer` before the
    /// tool can unlock. The `template_id` identifies a recognition
    /// phenomenon (`fire`, `surface_water`, …); the `ChannelKind`
    /// documents which sensory modality narratively grounds the
    /// prereq (which sense the species is using to study the
    /// phenomenon). The lookup in `is_unlocked` is satisfied when
    /// any confirmed relation matches the `template_id` — relations
    /// are keyed on physics-channel (`Temperature`, `WaterDepth`, …)
    /// not sensory-channel, so template-level matching is the
    /// faithful interpretation of "the civ has fit a law about
    /// this phenomenon and is therefore ready to engineer beyond
    /// it." Templates picked here are reachable for any species
    /// with `VisualLight` / `Tactile` / `ChemicalTaste` (the
    /// commonest baseline modalities), so the prereqs add
    /// scientific maturity without locking out species that lack
    /// the rarer senses (`MagneticSense`, `InfraredThermal`,
    /// `RadioNative`).
    ///
    /// Match arms enumerated per tool — the choice for each tool
    /// is documented inline.
    #[allow(clippy::match_same_arms)]
    pub fn relation_prereqs(self) -> &'static [(u32, ChannelKind)] {
        match self {
            // ThermalSensor: confirmed `fire` (template 1). Heat-
            // bearing phenomena are the obvious prerequisite for
            // thermal instrumentation; the sensory tag is
            // `InfraredThermal` because the *tool's* output channel
            // is thermal even when the prereq science was done via
            // visual fire-watching.
            ToolKind::ThermalSensor => &[(1, ChannelKind::InfraredThermal)],
            // RemoteAcoustic: confirmed `surface_water` (template
            // 5). Wave-mechanics on standing water is the universal
            // sound-propagation analogue — fitting any law on water
            // bodies signals the civ has the mathematics for
            // longitudinal-pressure modelling. Tagged
            // `AcousticAir`/`Water` to mark the tool's output
            // domain.
            ToolKind::RemoteAcoustic => &[(5, ChannelKind::AcousticAir)],
            // DistanceImaging: confirmed `fire` (template 1) on a
            // visual channel. They've fit *something* about light-
            // emitting phenomena; ready to extend optical range.
            ToolKind::DistanceImaging => &[(1, ChannelKind::VisualLight)],
            // FieldSensor: confirmed `lightning_buildup` (template
            // 2). Even species without `ElectricField` can fit
            // lightning via tactile pre-discharge cues; once the
            // mathematics of charge build-up is known the
            // electrostatic instrumentation follows.
            ToolKind::FieldSensor => &[(2, ChannelKind::ElectricField)],
            // MagneticSensor: confirmed `surface_water` (template
            // 5). Planetary-scale fluid dynamics is the entry
            // point for understanding planetary-scale fields; the
            // tool tag is `MagneticSense` because that's the
            // tool's grant.
            ToolKind::MagneticSensor => &[(5, ChannelKind::MagneticSense)],
            // AmphibiousConstruction: confirmed `surface_water`
            // (template 5). They can't engineer cross-domain
            // habitats without modelling the water column they're
            // crossing into.
            ToolKind::AmphibiousConstruction => &[(5, ChannelKind::AcousticWater)],
            // BioelectricResonator: confirmed `fire` AND
            // `lightning_buildup` — heat + EM physics, both needed
            // for the bioelectric engineering programme.
            ToolKind::BioelectricResonator => &[
                (1, ChannelKind::InfraredThermal),
                (2, ChannelKind::ElectricField),
            ],
            // FieldPropulsionEngine: confirmed `fire`,
            // `lightning_buildup`, and `surface_water` — broad
            // mastery of thermal, EM, and fluid physics.
            ToolKind::FieldPropulsionEngine => &[
                (1, ChannelKind::InfraredThermal),
                (2, ChannelKind::ElectricField),
                (5, ChannelKind::AcousticWater),
            ],
            // MetamaterialLattice: confirmed `fire` and
            // `ice_present` — phase-of-matter mastery (hot + cold
            // extremes) is the materials-science substrate for
            // engineered lattices.
            ToolKind::MetamaterialLattice => &[
                (1, ChannelKind::InfraredThermal),
                (3, ChannelKind::VisualLight),
            ],

            // ─── tier-1 relation prereqs ───
            //
            // LocalisedCombustion: the only tier-1 tool that demands
            // a *confirmed* law. Per the substrate-divergence design
            // ( vision): a no-fire seed (deep-ocean, methane,
            // ammonia substrates that never observe ignition)
            // genuinely loses the combustion-derived branch and is
            // pushed toward MechanicalAdvantage / fluid-dynamics
            // alternates.
            ToolKind::LocalisedCombustion => &[(1, ChannelKind::InfraredThermal)],
            // FoodProcessing: cooking is fire-applied; same
            // relation prereq as combustion plus a tool_prereq on
            // LocalisedCombustion itself (no fire, no fire-cooking).
            ToolKind::FoodProcessing => &[(1, ChannelKind::InfraredThermal)],
            // The other tier-1 tools express animal-level
            // technology that doesn't require formal relation-
            // fitting: weapons, shelter, cordage, knapping,
            // organised hunting, herbal first aid. Pre-relation
            // applied knowledge.
            ToolKind::ContactWeapon
            | ToolKind::RangedMomentumWeapon
            | ToolKind::SimpleShelter
            | ToolKind::FluidGathering
            | ToolKind::BasicTextiles
            | ToolKind::StoneWorking
            | ToolKind::OrganizedHunting
            | ToolKind::BasicHealing => &[],

            // ─── tier-2 relation prereqs ───
            //
            // Tier-2 capabilities that build on combustion / fluids /
            // biome cultivation chain through their substrate's
            // confirmed law. The relation gate enforces "the civ
            // has fit *something* about the phenomenon" before the
            // engineering follows.
            //
            // BulkCultivation: confirmed `fertile_land` — the civ
            // has fit a law about which biomes feed it.
            ToolKind::BulkCultivation => &[(10, ChannelKind::ChemicalTaste)],
            // AnimalSymbiosis: confirmed `fertile_land` — same
            // rationale (the civ understands animal habitats
            // through biome science).
            ToolKind::AnimalSymbiosis => &[(10, ChannelKind::ChemicalTaste)],
            // BulkStorage: confirmed `fire` — pottery firing
            // requires understanding of heat. Substrate gate.
            ToolKind::BulkStorage => &[(1, ChannelKind::InfraredThermal)],
            // MaterialRefining: confirmed `fire` — smelting heat
            // physics. Same substrate gate.
            ToolKind::MaterialRefining => &[(1, ChannelKind::InfraredThermal)],
            // FluidControl: confirmed `surface_water` — irrigation
            // demands water-mechanics understanding.
            ToolKind::FluidControl => &[(5, ChannelKind::Tactile)],
            // WatercraftConstruction: confirmed `surface_water` —
            // hull design needs wave-mechanics understanding.
            ToolKind::WatercraftConstruction => &[(5, ChannelKind::Tactile)],
            // CulturalEncoding, PermanentMasonry, TradeNetworks,
            // UrbanConstruction: no relation prereq — these are
            // social / craft technologies that don't depend on
            // any single physics-channel law.
            ToolKind::CulturalEncoding
            | ToolKind::PermanentMasonry
            | ToolKind::TradeNetworks
            | ToolKind::UrbanConstruction => &[],

            // ─── tier-3 relation prereqs ───
            //
            // ChemicalProjectile: confirmed `fire` law — the
            // gunpowder branch's substrate gate. Tier-2 chain
            // (MaterialRefining + BulkStorage) re-asserts the same
            // gate via tool_prereqs, double-locking no-fire seeds.
            ToolKind::ChemicalProjectile => &[(1, ChannelKind::InfraredThermal)],
            // PrecisionTimekeeping: confirmed `tidal_extremum` —
            // the periodic phenomena that anchor calendar science.
            ToolKind::PrecisionTimekeeping => &[(14, ChannelKind::Tactile)],
            // MechanicalAdvantage: confirmed `tidal_extremum` —
            // tides are the universal gravity-driven mechanical
            // observable; fitting any law on tides demonstrates
            // the elementary mechanics any lever / pulley / wheel
            // engineering rests on. This is the alternate-path
            // gate that keeps no-fire seeds reachable to Mechanisation.
            ToolKind::MechanicalAdvantage => &[(14, ChannelKind::Tactile)],
            // LongRangeNavigation: confirmed `tidal_extremum`
            // (celestial periodicity for dead-reckoning).
            ToolKind::LongRangeNavigation => &[(14, ChannelKind::Tactile)],
            // MotivePropulsion: confirmed `surface_water` —
            // sail / wind physics on a fluid medium.
            ToolKind::MotivePropulsion => &[(5, ChannelKind::Tactile)],
            // WrittenJurisprudence, AbstractMathematics,
            // ArtisanalSpecialisation, DefensiveFortification:
            // social / formal-systems technologies, no
            // physics-channel relation prereq.
            ToolKind::WrittenJurisprudence
            | ToolKind::AbstractMathematics
            | ToolKind::ArtisanalSpecialisation
            | ToolKind::DefensiveFortification => &[],

            // ─── tier-4 relation prereqs ───
            //
            // Mechanisation: confirmed `tidal_extremum` (gravity-
            // mechanics for engine analysis). NO confirmed-fire
            // requirement — alternate-path-friendly.
            ToolKind::Mechanisation => &[(14, ChannelKind::Tactile)],
            // ChemicalSynthesis: petrochemistry. Confirmed fire
            // (high-temperature reaction control) AND confirmed
            // hydrocarbon_seep (the buried-fossil deposits the
            // synthetic-organic branch is built on). The fossil
            // gate makes ChemicalSynthesis a substrate-locked
            // branch: non-`Crust::Hydrocarbon` worlds reach
            // tier-4 chemistry through AdvancedMaterials (metals)
            // and tier-5 chemistry through alternate paths, but
            // the petrochemical lineage is closed to them. This
            // mirrors the FieldPropulsionEngine crust gate —
            // some late-game branches are geology-dependent.
            ToolKind::ChemicalSynthesis => &[
                (1, ChannelKind::InfraredThermal),
                (21, ChannelKind::ChemicalTaste),
            ],
            // AdvancedMaterials: confirmed fire — combustion-
            // derived metallurgy / ceramics. No fossil gate; the
            // metals branch stays open on every crust.
            ToolKind::AdvancedMaterials => &[(1, ChannelKind::InfraredThermal)],
            // LongRangeCommunication + PowerGeneration: confirmed
            // `lightning_buildup` (EM substrate gate).
            ToolKind::LongRangeCommunication | ToolKind::PowerGeneration => {
                &[(2, ChannelKind::ElectricField)]
            }
            // AerialTransport: confirmed `tidal_extremum` (gravity
            // formalism for aerodynamics + lift).
            ToolKind::AerialTransport => &[(14, ChannelKind::Tactile)],
            // MedicalIntervention, HeavyTransport, AnalyticalEngines,
            // MassLiteracy: no specific relation prereq — formal
            // and social technologies whose substrate gate is
            // expressed through tool_prereqs (which transitively
            // inherit the appropriate physics-relation gates).
            ToolKind::MedicalIntervention
            | ToolKind::HeavyTransport
            | ToolKind::AnalyticalEngines
            | ToolKind::MassLiteracy => &[],

            // ─── tier-5 relation prereqs ───
            //
            // Information-age tools that hinge on EM physics
            // (DigitalComputation, InformationNetworking,
            // EnergyStorage) gate on confirmed lightning_buildup.
            ToolKind::DigitalComputation
            | ToolKind::InformationNetworking
            | ToolKind::EnergyStorage => &[(2, ChannelKind::ElectricField)],
            // OrbitalReach: confirmed tidal_extremum (orbital
            // mechanics formalism — a civ that's never fit a
            // gravity-driven law isn't going to send things to
            // orbit, regardless of how many engines it has).
            ToolKind::OrbitalReach => &[(14, ChannelKind::Tactile)],
            // MaterialFabrication: confirmed fire (high-temp
            // additive processing) AND confirmed hydrocarbon_seep
            // — modern additive manufacturing rests on polymer
            // feedstocks that are themselves fossil-derived.
            // Substrate-locked along with ChemicalSynthesis;
            // non-Hydrocarbon worlds reach equivalent tier-5
            // production through AdvancedMedicine /
            // AutonomousSystems / GeneticManipulation paths.
            ToolKind::MaterialFabrication => &[
                (1, ChannelKind::InfraredThermal),
                (21, ChannelKind::ChemicalTaste),
            ],
            // CryogenicEngineering: confirmed ice_present (the
            // cold-phase substrate).
            ToolKind::CryogenicEngineering => &[(3, ChannelKind::VisualLight)],
            // OrganicSynthesis: petrochemistry's tier-5 endpoint.
            // Confirmed hydrocarbon_seep — the synthetic-organic
            // branch is fossil-substrate-locked. Non-Hydrocarbon
            // worlds skip this tool; their tier-5 organic
            // chemistry comes through AdvancedMedicine /
            // GeneticManipulation pathways instead.
            ToolKind::OrganicSynthesis => &[(21, ChannelKind::ChemicalTaste)],
            // GeneticManipulation, AdvancedMedicine,
            // AutonomousSystems: no specific physics-channel
            // relation prereq; substrate gate via chained
            // tool_prereqs.
            ToolKind::GeneticManipulation
            | ToolKind::AdvancedMedicine
            | ToolKind::AutonomousSystems => &[],
            // confirmed `fire` law — the substrate-spanning
            // signal that the civ has fit *something* about
            // controlled physical conditions. A no-fire seed (deep
            // ocean methane / ammonia substrate) doesn't hit this
            // unless / until it confirms a substrate-equivalent
            // (template id 1 fires for any thermal-signature
            // ExperimentApparatus: the universal "controlled
            // experiment" gateway. The prior `(1, InfraredThermal)`
            // gate was bug-shaped — template 1 is `fire`, whose
            // signature requires `Above(Oxidiser, 0)`, and so it
            // never triggers on a non-oxidising world (CO₂, methane,
            // ammonia, etc.). A civ on Lumen-h's 95% CO₂ atmosphere
            // could never confirm fire and was permanently locked
            // out of tier-3+ — the "stuck at 22 tools" plateau the
            // viewport surfaces. Swapped to `tidal_extremum`
            // (template 14, perceivable by Tactile which every
            // species has access to via baseline modalities) so the
            // gateway is reachable on every habitable world. Tidal
            // periodicity is also a more honest "you've fit a
            // quantitative-periodic law and so can engineer
            // measurement apparatus" gate than a thermal-only one.
            ToolKind::ExperimentApparatus => &[(14, ChannelKind::Tactile)],
            // HerbalMedicine: confirmed `surface_water` (template 5)
            // on the Tactile channel — universal access on any
            // ocean / lake-bearing world. Plant pharmacology
            // bootstraps from watching what grows where water
            // pools, even pre-formal-science.
            ToolKind::HerbalMedicine => &[(5, ChannelKind::Tactile)],
            // AcousticEngineering: confirmed `surface_water` (5)
            // tagged AcousticAir — same template as RemoteAcoustic's
            // prereq, so a civ that has built RemoteAcoustic
            // already satisfies this gate trivially. Keeps the
            // build path linear after the masonry/sensor pair are
            // both in.
            ToolKind::AcousticEngineering => &[(5, ChannelKind::AcousticAir)],
            // AnimalHusbandry: confirmed `surface_water` (5) on
            // Tactile — same universal water-presence gate. (A
            // species observing herd-watering behaviour fits a law
            // about animals + water.)
            ToolKind::AnimalHusbandry => &[(5, ChannelKind::Tactile)],
            // PreservedFood: confirmed `surface_water` (5). Brine,
            // ferment liquor, sun-drying all anchor on the same
            // universal water-cycle observations.
            ToolKind::PreservedFood => &[(5, ChannelKind::Tactile)],
            // BiomimeticDesign: confirmed `tidal_extremum` (14) —
            // the periodic phenomena that anchor formal-mathematics
            // abstraction. Same gate as AbstractMathematics' lineage.
            ToolKind::BiomimeticDesign => &[(14, ChannelKind::Tactile)],
            // HydraulicWorks: confirmed `surface_water` (5).
            // Engineering with water requires having fit *some*
            // law about how water moves.
            ToolKind::HydraulicWorks => &[(5, ChannelKind::Tactile)],
            // PrecisionInstruments: confirmed `tidal_extremum` (14).
            // The tradition of periodic-phenomenon measurement.
            ToolKind::PrecisionInstruments => &[(14, ChannelKind::Tactile)],
            // DistributedNetworks: confirmed `surface_water` (5)
            // as a stand-in for "long-distance phenomena
            // propagation". No specific physics gate beyond what
            // its tier-2/3 prereqs already enforce.
            ToolKind::DistributedNetworks => &[(5, ChannelKind::Tactile)],
        }
    }
}
