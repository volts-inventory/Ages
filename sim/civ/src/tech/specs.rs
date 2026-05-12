//! Per-tool unlock prereqs: `observation_threshold`, `literacy_floor`,
//! `species_maturity_floor`, `obs_channel_filter`, `crust_prereqs`,
//! `relation_prereqs`, `tool_prereqs`. The big match arms that
//! describe what the species must observe, learn, and have built
//! before each tool becomes conceivable.

use super::ToolKind;
use sim_arith::Real;
use sim_physics::Substance;
use sim_recognition::ChannelKind;
use sim_species::ManipulationKind;
use sim_world::Crust;

// ─── resource_prereqs threshold tables ───────────────────────────
//
// `Real::from_int` is `const fn`, so each tool's prereq slice can be
// declared as a `const [(Substance, Real); N]` and returned as a
// `&'static` reference from `resource_prereqs`. `Real::from_ratio`
// is not `const`, so all thresholds round to whole units of summed
// claim-cell density (a 5-cell Lush civ has Fuel ≈ 5 summed; a
// 5-cell Hydrocarbon civ has Fossil ≈ 20 summed).

const LOC_COMBUSTION_RES: [(Substance, Real); 1] = [(Substance::Fuel, Real::from_int(1))];
const FOOD_PROC_RES: [(Substance, Real); 1] = [(Substance::Fuel, Real::from_int(1))];
const FLUID_GATHER_RES: [(Substance, Real); 1] = [(Substance::Water, Real::from_int(1))];
const BULK_STORAGE_RES: [(Substance, Real); 1] = [(Substance::Fuel, Real::from_int(2))];
const MATERIAL_REFINING_RES: [(Substance, Real); 1] = [(Substance::Fuel, Real::from_int(5))];
const FLUID_CONTROL_RES: [(Substance, Real); 1] = [(Substance::Water, Real::from_int(5))];
const WATERCRAFT_RES: [(Substance, Real); 1] = [(Substance::Water, Real::from_int(5))];
const CHEM_SYNTH_RES: [(Substance, Real); 1] = [(Substance::Fossil, Real::from_int(1))];
const MATERIAL_FAB_RES: [(Substance, Real); 1] = [(Substance::Fossil, Real::from_int(5))];
const ORGANIC_SYNTH_RES: [(Substance, Real); 1] = [(Substance::Fossil, Real::from_int(5))];

impl ToolKind {
    /// observation-pressure threshold (cumulative firings of
    /// `prereq_channels`-matching templates required for the tool
    /// to be conceivable). Per-tool placeholders under; sized
    /// so the progression spans a typical civ's ~500-tick window
    /// rather than firing trivially in the first few ticks (with a
    /// dev-grid 8×6 = 48 cells, water + ice + vapour templates
    /// alone can produce ~150 firings/tick, so thresholds need to
    /// be in the tens-of-thousands range to be a real gate).
    #[allow(clippy::match_same_arms)]
    pub fn observation_threshold(self) -> u64 {
        match self {
            ToolKind::ThermalSensor => 30_000,
            ToolKind::RemoteAcoustic => 30_000,
            ToolKind::FieldSensor => 75_000,
            ToolKind::DistanceImaging => 75_000,
            ToolKind::MagneticSensor => 140_000,
            // Tier-5 per-civ obs thresholds are intentionally
            // modest — the heavy lifting of "thousands of years"
            // happens at the species-cumulative maturity gate
            // (`species_maturity_floor`), not the per-civ obs
            // gate. The per-civ obs check just ensures the
            // unlocking civ itself has done some science.
            ToolKind::BioelectricResonator => 50_000,
            ToolKind::FieldPropulsionEngine => 8_000,
            ToolKind::MetamaterialLattice => 50_000,
            // AmphibiousConstruction: tier-3 obs threshold so
            // a civ has done meaningful science before it earns the
            // cross-domain expansion. Same scale as DistanceImaging
            // (also tier-3, no native-channel prereq).
            ToolKind::AmphibiousConstruction => 75_000,
            // tier-1 thresholds: low compared with sensorium
            // tools because tier-1 capabilities are pre-literate
            // technologies (campfire, club, knapped stone). The
            // `obs_channel_filter` for each tier-1 tool narrows
            // observations to the relevant template family, so a
            // civ with no fire-template firings can't unlock
            // LocalisedCombustion regardless of how much science
            // it does on water / fertile_land. Numbers calibrated
            // to fire over the first ~50-150 ticks on a typical
            // habitable seed.
            ToolKind::LocalisedCombustion => 5_000,
            ToolKind::ContactWeapon => 1_000,
            ToolKind::RangedMomentumWeapon => 1_500,
            ToolKind::SimpleShelter => 500,
            ToolKind::FoodProcessing => 2_000,
            ToolKind::FluidGathering => 1_000,
            ToolKind::BasicTextiles => 1_500,
            ToolKind::StoneWorking => 1_000,
            ToolKind::OrganizedHunting => 1_000,
            ToolKind::BasicHealing => 1_500,
            // tier-2 thresholds: same scale as the existing
            // sensorium tier-2 (ThermalSensor / RemoteAcoustic at
            // 30k) — settlements take meaningful science before
            // they coalesce. Calibrated to fire over the first
            // ~500-1500 ticks once a tier-1 tool that gates on
            // the same substrate has unlocked.
            ToolKind::BulkCultivation => 30_000,
            ToolKind::AnimalSymbiosis => 25_000,
            ToolKind::BulkStorage => 25_000,
            ToolKind::MaterialRefining => 30_000,
            ToolKind::CulturalEncoding => 20_000,
            ToolKind::FluidControl => 30_000,
            ToolKind::WatercraftConstruction => 25_000,
            ToolKind::PermanentMasonry => 20_000,
            ToolKind::TradeNetworks => 25_000,
            ToolKind::UrbanConstruction => 35_000,
            // tier-3: 50k-75k range, matching the existing
            // sensorium tier-3 (DistanceImaging / FieldSensor at
            // 75k). Pre-industrial science requires meaningful
            // accumulated observation pressure.
            ToolKind::ChemicalProjectile => 75_000,
            ToolKind::PrecisionTimekeeping => 60_000,
            ToolKind::MechanicalAdvantage => 60_000,
            ToolKind::LongRangeNavigation => 75_000,
            ToolKind::WrittenJurisprudence => 50_000,
            ToolKind::AbstractMathematics => 60_000,
            ToolKind::ArtisanalSpecialisation => 50_000,
            ToolKind::DefensiveFortification => 50_000,
            ToolKind::MotivePropulsion => 60_000,
            // tier-4: 80k-140k range, matching MagneticSensor's
            // existing tier-4 of 140k. Industrial-era tools demand
            // sustained scientific accumulation.
            ToolKind::Mechanisation => 140_000,
            ToolKind::LongRangeCommunication => 100_000,
            ToolKind::ChemicalSynthesis => 100_000,
            ToolKind::MedicalIntervention => 80_000,
            ToolKind::AdvancedMaterials => 100_000,
            ToolKind::HeavyTransport => 100_000,
            ToolKind::PowerGeneration => 120_000,
            ToolKind::AnalyticalEngines => 100_000,
            ToolKind::MassLiteracy => 80_000,
            ToolKind::AerialTransport => 130_000,
            // tier-5: information-age — sustained scientific
            // accumulation in the 80k-200k range. OrbitalReach is
            // the heavy hitter (200k) reflecting the difficulty
            // of escape velocity. Lower thresholds for biological
            // chemical tools that follow naturally from tier-4
            // chemistry.
            ToolKind::DigitalComputation => 80_000,
            ToolKind::InformationNetworking => 100_000,
            ToolKind::GeneticManipulation => 100_000,
            ToolKind::OrbitalReach => 200_000,
            ToolKind::AdvancedMedicine => 80_000,
            ToolKind::MaterialFabrication => 100_000,
            ToolKind::AutonomousSystems => 100_000,
            ToolKind::EnergyStorage => 100_000,
            ToolKind::CryogenicEngineering => 80_000,
            ToolKind::OrganicSynthesis => 80_000,
            // tier-2 capability — same observation-pressure
            // gate as the established sensorium tier-2 tools so a
            // civ that reaches ThermalSensor's window also reaches
            // the apparatus.
            ToolKind::ExperimentApparatus => 30_000,
        }
    }

    /// literacy floor: civ's `literacy_score` must
    /// reach this for the tool to unlock. Per-tool placeholders
    /// under.
    pub fn literacy_floor(self) -> Real {
        match self {
            ToolKind::ThermalSensor | ToolKind::RemoteAcoustic => Real::from_ratio(20, 100),
            ToolKind::FieldSensor
            | ToolKind::DistanceImaging
            | ToolKind::AmphibiousConstruction => Real::from_ratio(35, 100),
            // Tier-4 magnetic_sensor and tier-5 (transcendence-tier)
            // tools share a 0.55 floor on per-civ literacy. Tier-5
            // is gated separately by a species-cumulative maturity
            // check (`species_maturity_floor`) on top of this —
            // see `is_unlocked` callers.
            ToolKind::MagneticSensor
            | ToolKind::BioelectricResonator
            | ToolKind::FieldPropulsionEngine
            | ToolKind::MetamaterialLattice => Real::from_ratio(55, 100),
            // tier-1: pre-literate technologies. Floor of 0.0
            // — these unlock from observation pressure alone, with
            // no formal-literacy gate. A foraging band that has
            // never written anything down can still build
            // campfires, clubs, and shelter.
            // Tier-2 CulturalEncoding shares the zero floor —
            // it bootstraps literacy itself, so it must be
            // unlockable below the literacy gate it raises.
            ToolKind::LocalisedCombustion
            | ToolKind::ContactWeapon
            | ToolKind::RangedMomentumWeapon
            | ToolKind::SimpleShelter
            | ToolKind::FoodProcessing
            | ToolKind::FluidGathering
            | ToolKind::BasicTextiles
            | ToolKind::StoneWorking
            | ToolKind::OrganizedHunting
            | ToolKind::BasicHealing
            | ToolKind::CulturalEncoding => Real::ZERO,
            // tier-2: settlement-era tools demand a low
            // literacy floor (~0.15) — symbol-systems still nascent
            // but bands have begun keeping records.
            ToolKind::BulkCultivation
            | ToolKind::AnimalSymbiosis
            | ToolKind::BulkStorage
            | ToolKind::MaterialRefining
            | ToolKind::FluidControl
            | ToolKind::WatercraftConstruction
            | ToolKind::PermanentMasonry
            | ToolKind::TradeNetworks
            | ToolKind::UrbanConstruction => Real::from_ratio(15, 100),
            // tier-3: pre-industrial — formal recording is
            // expected. ~0.30 floor, around the existing tier-3
            // sensorium tools (DistanceImaging at 0.35).
            ToolKind::ChemicalProjectile
            | ToolKind::PrecisionTimekeeping
            | ToolKind::MechanicalAdvantage
            | ToolKind::LongRangeNavigation
            | ToolKind::WrittenJurisprudence
            | ToolKind::AbstractMathematics
            | ToolKind::ArtisanalSpecialisation
            | ToolKind::DefensiveFortification
            | ToolKind::MotivePropulsion => Real::from_ratio(30, 100),
            // tier-4: industrial-era — high literacy floor
            // (~0.50). Per the spec's "literacy ≥ 0.65" gate for
            // tier 5, tier 4 lives just below.
            ToolKind::Mechanisation
            | ToolKind::LongRangeCommunication
            | ToolKind::ChemicalSynthesis
            | ToolKind::MedicalIntervention
            | ToolKind::AdvancedMaterials
            | ToolKind::HeavyTransport
            | ToolKind::PowerGeneration
            | ToolKind::AnalyticalEngines
            | ToolKind::MassLiteracy
            | ToolKind::AerialTransport => Real::from_ratio(50, 100),
            // tier-5 (information-age): per spec, "literacy
            // ≥ 0.65". The pre-existing transcendence trio uses
            // 0.55 (handled in the earlier match arm above).
            ToolKind::DigitalComputation
            | ToolKind::InformationNetworking
            | ToolKind::GeneticManipulation
            | ToolKind::OrbitalReach
            | ToolKind::AdvancedMedicine
            | ToolKind::MaterialFabrication
            | ToolKind::AutonomousSystems
            | ToolKind::EnergyStorage
            | ToolKind::CryogenicEngineering
            | ToolKind::OrganicSynthesis => Real::from_ratio(65, 100),
            // tier-2 capability — 0.30 floor matches the
            // tier-3 group above (the civ has begun formal symbol-
            // keeping; sigmoid literacy at 0.30 ≈ ~50 confirmed
            // relations when combined with the obs threshold).
            // Joined with the tier-3 arm via the `|` pattern would
            // misclassify the tier; explicit arm with a documented
            // `match_same_arms` allow keeps the tier identity clear.
            #[allow(clippy::match_same_arms)]
            ToolKind::ExperimentApparatus => Real::from_ratio(30, 100),
        }
    }

    /// Species-cumulative maturity floor. Tier-5 tools demand the
    /// species has been doing science long enough that a transcendence-
    /// tier capability is conceivable. Returns the minimum
    /// `total_confirmed_relations` (cumulative across every civ
    /// in the run) that must hold before this tool is buildable.
    /// Tier-≤ 4 tools return `0` (no species-maturity gate).
    ///
    /// Set to 3000: a typical civ on a habitable planet confirms
    /// ~50-200 relations over its lifetime; 3000 across the species
    /// requires ~15-60 civ generations of accumulated science.
    /// At typical civ lifespans of 500-1500 ticks each, this
    /// pushes transcendence into the multi-thousand-year range
    /// (often year 5000-15000), emergent from the sim's own
    /// dynamics rather than from any authored progression rule.
    pub fn species_maturity_floor(self) -> u32 {
        match self {
            ToolKind::BioelectricResonator
            | ToolKind::FieldPropulsionEngine
            | ToolKind::MetamaterialLattice => 3_000,
            // falls through the wildcard at 0 — the apparatus
            // is mid-civ, not species-summit; per-civ gates
            // (observation + literacy + relation prereq) suffice.
            _ => 0,
        }
    }

    /// Channels used to compute the observation-pressure threshold.
    /// Defaults to `prereq_channels` (the buildability filter) but
    /// some tier-5 tools want to gate buildability on a sense type
    /// while measuring observation pressure across all channels —
    /// e.g. `BioelectricResonator` requires the species to natively
    /// possess `ElectricField` or `MagneticSense` (because the work
    /// is engineering on the species' own bioelectric body), but
    /// the observation-pressure measure is "the species has done
    /// enough science of any kind," not "enough field-firings."
    pub fn obs_channel_filter(self) -> &'static [ChannelKind] {
        match self {
            ToolKind::BioelectricResonator => &[],
            other => other.prereq_channels(),
        }
    }

    /// Tier-5 crust gate. Each late-game tool requires a specific
    /// crust profile — the physical substrate has to support the
    /// engineering. Returns `None` for tier-≤ 4 tools (no crust
    /// gate) or a slice of acceptable crusts.
    pub fn crust_prereqs(self) -> Option<&'static [Crust]> {
        match self {
            // Field propulsion: needs an exotic crust the
            // engineering can couple to (piezoelectric for
            // resonance, ferrous/rare-earth for high-Q magnets).
            ToolKind::FieldPropulsionEngine => {
                Some(&[Crust::Piezoelectric, Crust::Ferrous, Crust::RareEarth])
            }
            // Metamaterial lattice: needs piezo or rare-earth for
            // the structural building blocks.
            ToolKind::MetamaterialLattice => Some(&[Crust::Piezoelectric, Crust::RareEarth]),
            // BioelectricResonator and tier-≤ 4 tools: no crust
            // gate (the work is about the species' own fields, or
            // the tool predates crust-specific engineering).
            _ => None,
        }
    }

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
            // template under the per-substrate ignition mapping).
            ToolKind::ExperimentApparatus => &[(1, ChannelKind::InfraredThermal)],
        }
    }

    /// Material-resource prereq: minimum summed `Substance` density
    /// across the civ's `claimed_cells` required for the tool to
    /// unlock. Each `(Substance, threshold)` pair is checked
    /// independently and AND-combined — a tool with two prereqs
    /// requires both substances at or above their threshold.
    /// Empty slice means no resource gate.
    ///
    /// Thresholds are summed-density across all claimed cells, so a
    /// 5-cell territory on a Lush biosphere (Fuel ≈ 1.0/cell) totals
    /// ≈ 5.0; a Hydrocarbon-crust civ totals ≈ 4.0 × land-cells of
    /// `Substance::Fossil`. Calibration logic: tier-1 / cooking-tier
    /// tools demand a trace (≥ 0.1) so the civ has *some* substrate
    /// to work with; tier-2 production tools demand a small stock
    /// (1–3); tier-4 / tier-5 industrial tools demand a substantial
    /// stock (≥ 5).
    ///
    /// Hard gate: serendipity does not bypass material requirements.
    /// A civ literally needs the substrate present in territory.
    /// Non-extractive — checking density doesn't deplete it.
    #[allow(clippy::match_same_arms)]
    pub fn resource_prereqs(self) -> &'static [(Substance, Real)] {
        // Numerical thresholds via `Real::from_int`/`from_ratio`:
        // can't be const-evaluated to populate a `&'static` slice
        // directly, so each arm builds its own slice literal.
        match self {
            // ─── tier-1: subsistence-tier resource gates ───
            //
            // LocalisedCombustion: needs *any* biofuel in territory
            // to start a fire. Even a sparse-biosphere claim should
            // pass this; the gate just rules out all-water and
            // lifeless-substrate civs from spontaneously inventing
            // fire.
            ToolKind::LocalisedCombustion => &LOC_COMBUSTION_RES,
            // FoodProcessing: cooking-scale fuel demand.
            ToolKind::FoodProcessing => &FOOD_PROC_RES,
            // FluidGathering: the civ needs surface water to gather
            // from. Trace threshold — even arid claims with a single
            // wet cell pass.
            ToolKind::FluidGathering => &FLUID_GATHER_RES,
            // ─── tier-2: production-tier resource gates ───
            //
            // BulkStorage: kiln-firing pottery; modest fuel stock.
            ToolKind::BulkStorage => &BULK_STORAGE_RES,
            // MaterialRefining: charcoal smelting pulls a real
            // amount of biofuel through the furnace. Larger stock
            // demand than cooking.
            ToolKind::MaterialRefining => &MATERIAL_REFINING_RES,
            // FluidControl: irrigation requires meaningful surface
            // water in the claim.
            ToolKind::FluidControl => &FLUID_CONTROL_RES,
            // WatercraftConstruction: hulls + shipwrights' stock —
            // wood (biofuel proxy) + the water body to launch into.
            ToolKind::WatercraftConstruction => &WATERCRAFT_RES,
            // ─── tier-4 / tier-5: petrochemistry-tier resource
            //  gates. Pair with the relation
            //  prereqs on hydrocarbon_seep
            //  (template 21) so the civ both
            //  *understands* and *has access to*
            //  fossil deposits.
            //
            // ChemicalSynthesis: petrochemistry feedstock.
            ToolKind::ChemicalSynthesis => &CHEM_SYNTH_RES,
            // MaterialFabrication: polymer / advanced-materials
            // feedstock. Larger fossil draw than ChemicalSynthesis.
            ToolKind::MaterialFabrication => &MATERIAL_FAB_RES,
            // OrganicSynthesis: petrochemistry's tier-5 endpoint —
            // the bulk-feedstock organic-chemistry industry.
            ToolKind::OrganicSynthesis => &ORGANIC_SYNTH_RES,
            // Every other tool: no material gate. Sensorium tools
            // are body-physics; cultural / social / mechanical /
            // alternate-path tools don't depend on a specific
            // substance stockpile.
            _ => &[],
        }
    }

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
        }
    }

    /// Per-tool manipulation-mode prereq: the body-plan modes that
    /// suffice to fabricate this tool. The species must contain at
    /// least one of the listed kinds in its `manipulation_modes`
    /// for `is_buildable` to pass. Empty slice means "no
    /// manipulation gate" — pure social / cognitive tools that any
    /// sapient body plan can develop (e.g. `TradeNetworks`).
    ///
    /// Replaces the prior global `MANIPULATION_PREREQ = ToolExtension`
    /// gate. The old gate made every chemical-secretion / web /
    /// burrow / jet species permanently observation-only across the
    /// whole tree — collapsing "different sciences for different
    /// bodies" into "no science for most bodies." Per-tool prereqs
    /// preserve substrate divergence (tier-2+ instrument tools and
    /// `ExperimentApparatus` still demand `ToolExtension`) while
    /// letting any body plan reach tier-1 applied knowledge.
    ///
    /// Coverage guarantee — every `ManipulationKind` variant is
    /// accepted by at least one tier-1 tool, so no body-plan draw
    /// leaves a species frozen at zero tools. Verified by the
    /// `every_manipulation_kind_has_tier1_path` test.
    #[allow(clippy::match_same_arms)]
    #[allow(clippy::too_many_lines)]
    pub fn manipulation_prereqs(self) -> &'static [ManipulationKind] {
        match self {
            // ─── tier-1: applied-knowledge / animal-level tech ───
            //
            // LocalisedCombustion: handling fire — placing a brand,
            // building a pyre, transporting embers. Needs a grasp-
            // or shape-capable manipulator; ChemicalSecretion qualifies
            // via secreted-oxidiser ignition paths (a pyrophoric body
            // chemistry is a real-world template).
            ToolKind::LocalisedCombustion => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::TonguePrehensile,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
            ],
            // ContactWeapon: melee predation — biting, stinging,
            // clubbing, stunning. Every predator-mode manipulator
            // qualifies.
            ToolKind::ContactWeapon => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::ToolExtension,
                ManipulationKind::WebConstruct,
                ManipulationKind::Burrow,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::ElectricDischarge,
            ],
            // RangedMomentumWeapon: throwing / spitting / spraying /
            // net-flinging. Needs propulsion or grasp-and-release.
            ToolKind::RangedMomentumWeapon => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::ToolExtension,
                ManipulationKind::FluidJet,
                ManipulationKind::WebConstruct,
                ManipulationKind::ChemicalSecretion,
            ],
            // SimpleShelter: dwellings — burrows, web-nests,
            // secreted shells, leaned-stick lean-tos, packed mud.
            ToolKind::SimpleShelter => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::ToolExtension,
                ManipulationKind::WebConstruct,
                ManipulationKind::Burrow,
                ManipulationKind::ChemicalSecretion,
            ],
            // FoodProcessing: butchering / chewing / external
            // digestion / fire-cooking. Mouthparts, limbs, jetted
            // enzymes, secreted digestive fluids — most modes
            // qualify.
            ToolKind::FoodProcessing => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::TonguePrehensile,
                ManipulationKind::ToolExtension,
                ManipulationKind::FluidJet,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::Burrow,
            ],
            // FluidGathering: carrying / channelling water —
            // containers, jetted-collection, web-traps, secreted
            // vessels.
            ToolKind::FluidGathering => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::TonguePrehensile,
                ManipulationKind::ToolExtension,
                ManipulationKind::FluidJet,
                ManipulationKind::WebConstruct,
                ManipulationKind::ChemicalSecretion,
            ],
            // BasicTextiles: cordage / silk / weaving. WebConstruct
            // and ChemicalSecretion (silk-producing glands) are the
            // canonical body-plan paths; limbs / mandibles weave
            // plant fibre.
            ToolKind::BasicTextiles => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::Mandible,
                ManipulationKind::ToolExtension,
                ManipulationKind::WebConstruct,
                ManipulationKind::ChemicalSecretion,
            ],
            // StoneWorking: knapping / shaping stone. Needs
            // percussive striking + precision-grip. Excludes
            // fluid / web / chemical / electric paths (none of
            // them can knap obsidian).
            ToolKind::StoneWorking => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::ToolExtension,
            ],
            // OrganizedHunting: coordinated predation. The work is
            // social coordination + any predatory affordance — open
            // to every body plan.
            ToolKind::OrganizedHunting => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::TonguePrehensile,
                ManipulationKind::ToolExtension,
                ManipulationKind::FluidJet,
                ManipulationKind::WebConstruct,
                ManipulationKind::Burrow,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::ElectricDischarge,
            ],
            // BasicHealing: herbal / pharmaceutical first aid.
            // ChemicalSecretion is the natural strength (venom-
            // bearing species are already pharmacologists);
            // ElectricDischarge enables stun-cure traditions.
            ToolKind::BasicHealing => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::TonguePrehensile,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::ElectricDischarge,
            ],

            // ─── tier-2: settlement-tier tools ───
            //
            // BulkCultivation: agriculture at scale.
            ToolKind::BulkCultivation => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::Mandible,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
            ],
            // AnimalSymbiosis: herding — grip-based, pheromone-
            // controlled, or electric-herded.
            ToolKind::AnimalSymbiosis => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::ElectricDischarge,
            ],
            // BulkStorage: pottery / silos / chitin granaries /
            // woven baskets.
            ToolKind::BulkStorage => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::Mandible,
                ManipulationKind::ToolExtension,
                ManipulationKind::WebConstruct,
                ManipulationKind::ChemicalSecretion,
            ],
            // MaterialRefining: smelting / metallurgy. Demands
            // fire-handling + precision; secretion qualifies via
            // secreted flux / smelting agents.
            ToolKind::MaterialRefining => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
            ],
            // CulturalEncoding: writing / mark-making. Any mode
            // that leaves persistent signals — scratches, pheromone
            // trails, bioelectric impressions, woven knot-records.
            ToolKind::CulturalEncoding => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::TonguePrehensile,
                ManipulationKind::ToolExtension,
                ManipulationKind::WebConstruct,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::ElectricDischarge,
            ],
            // FluidControl: irrigation — ditches, dams, bored pipes,
            // jet-driven channels, secreted aqueducts.
            ToolKind::FluidControl => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::ToolExtension,
                ManipulationKind::FluidJet,
                ManipulationKind::Burrow,
                ManipulationKind::ChemicalSecretion,
            ],
            // WatercraftConstruction: hulls — shaped, woven, or
            // secreted.
            ToolKind::WatercraftConstruction => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::Mandible,
                ManipulationKind::ToolExtension,
                ManipulationKind::WebConstruct,
                ManipulationKind::ChemicalSecretion,
            ],
            // PermanentMasonry: stone construction. Same precision-
            // grip requirement as StoneWorking plus secreted
            // mortar / cement paths.
            ToolKind::PermanentMasonry => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
            ],
            // TradeNetworks: pure economic / social institution —
            // no manipulation gate.
            ToolKind::TradeNetworks => &[],
            // UrbanConstruction: city-scale building. Many paths.
            ToolKind::UrbanConstruction => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::ToolExtension,
                ManipulationKind::WebConstruct,
                ManipulationKind::Burrow,
                ManipulationKind::ChemicalSecretion,
            ],

            // ─── sensorium instruments (tier 2 / 3 / 4) ───
            //
            // Precision optical / acoustic / field instrumentation
            // — needs ToolExtension or a high-DoF flexible
            // manipulator. The work is sub-mm machining, not body
            // chemistry, so secretion / web / jet are excluded.
            ToolKind::ThermalSensor
            | ToolKind::RemoteAcoustic
            | ToolKind::DistanceImaging
            | ToolKind::FieldSensor
            | ToolKind::MagneticSensor => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::ToolExtension,
            ],
            // AmphibiousConstruction: cross-domain habitats —
            // diverse paths (limbed, tentacled, burrowing, web-built,
            // or secreted shells).
            ToolKind::AmphibiousConstruction => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::ToolExtension,
                ManipulationKind::WebConstruct,
                ManipulationKind::Burrow,
                ManipulationKind::ChemicalSecretion,
            ],
            // ExperimentApparatus: controlled-conditions intervention.
            // A clamp-and-measure rig is a *function* (hold one
            // channel at a known value, observe the response), not a
            // specific physical form — every manipulation mode can
            // build one with its own native affordance:
            // ChemicalSecretion runs controlled-concentration baths
            // (literal pharmacology), WebConstruct weaves a chamber
            // with calibrated mesh, FluidJet holds a stable jet as a
            // pressure clamp, ElectricDischarge clamps field strength
            // directly, Burrow excavates a controlled-volume cell. The
            // substrate gate (confirmed `fire`) plus per-channel
            // clamp-ladder math already encode "which experiments are
            // even meaningful here"; the manipulation gate just asks
            // "can the species deliberately hold a state."
            ToolKind::ExperimentApparatus => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::TonguePrehensile,
                ManipulationKind::ToolExtension,
                ManipulationKind::FluidJet,
                ManipulationKind::WebConstruct,
                ManipulationKind::Burrow,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::ElectricDischarge,
            ],

            // ─── tier-3: pre-industrial ───
            //
            // ChemicalProjectile: gunpowder weaponry — precision
            // metallurgy + chemistry.
            ToolKind::ChemicalProjectile => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
            ],
            // PrecisionTimekeeping: clocks — high-precision
            // mechanical fabrication.
            ToolKind::PrecisionTimekeeping => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::ToolExtension,
            ],
            // MechanicalAdvantage: levers / pulleys / wheels — broad
            // precision-shape work.
            ToolKind::MechanicalAdvantage => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::ToolExtension,
            ],
            // LongRangeNavigation: instruments + charts.
            ToolKind::LongRangeNavigation => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::ToolExtension,
            ],
            // WrittenJurisprudence + AbstractMathematics + MassLiteracy:
            // notation-bound — same broad palette as CulturalEncoding.
            ToolKind::WrittenJurisprudence | ToolKind::AbstractMathematics => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::TonguePrehensile,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::ElectricDischarge,
            ],
            // ArtisanalSpecialisation: crafts — most modes qualify.
            ToolKind::ArtisanalSpecialisation => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::ToolExtension,
                ManipulationKind::WebConstruct,
                ManipulationKind::ChemicalSecretion,
            ],
            // DefensiveFortification: large earthworks / walls.
            ToolKind::DefensiveFortification => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::ToolExtension,
                ManipulationKind::Burrow,
                ManipulationKind::ChemicalSecretion,
            ],
            // MotivePropulsion: sails / wheels / paddles —
            // mechanical shaping.
            ToolKind::MotivePropulsion => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::ToolExtension,
            ],

            // ─── tier-4: industrial ───
            //
            // Industrial tools narrow toward instrument-grade
            // precision. ToolExtension is universally accepted;
            // LimbGrasp / Tentacle qualify as high-DoF biological
            // substitutes; ElectricDischarge unlocks EM-native
            // branches; ChemicalSecretion unlocks the wet-chemistry
            // branches.
            ToolKind::Mechanisation => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::ToolExtension,
            ],
            ToolKind::LongRangeCommunication => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::ToolExtension,
                ManipulationKind::ElectricDischarge,
            ],
            ToolKind::ChemicalSynthesis => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
            ],
            ToolKind::MedicalIntervention => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
            ],
            ToolKind::AdvancedMaterials => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::ToolExtension,
            ],
            ToolKind::HeavyTransport => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::ToolExtension,
            ],
            ToolKind::PowerGeneration => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::ToolExtension,
                ManipulationKind::ElectricDischarge,
            ],
            ToolKind::AnalyticalEngines => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::ToolExtension,
            ],
            ToolKind::MassLiteracy => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::ElectricDischarge,
            ],
            ToolKind::AerialTransport => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::ToolExtension,
            ],

            // ─── tier-5: information-age + transcendence trio ───
            //
            // The tightest tier — engineered solids, atomic-precision
            // lattices, fabricated chips, rockets. ToolExtension is
            // mandatory for the narrative trio + the most demanding
            // engineering tools. EM-coupled tools (Bioelectric,
            // InfoNetworking, EnergyStorage) admit ElectricDischarge;
            // biochemistry tools (Genetic, AdvancedMedicine, Organic)
            // admit ChemicalSecretion.
            ToolKind::BioelectricResonator => &[
                ManipulationKind::ToolExtension,
                ManipulationKind::ElectricDischarge,
            ],
            ToolKind::FieldPropulsionEngine | ToolKind::MetamaterialLattice => {
                &[ManipulationKind::ToolExtension]
            }
            ToolKind::DigitalComputation
            | ToolKind::OrbitalReach
            | ToolKind::MaterialFabrication
            | ToolKind::AutonomousSystems
            | ToolKind::CryogenicEngineering => &[ManipulationKind::ToolExtension],
            ToolKind::InformationNetworking | ToolKind::EnergyStorage => &[
                ManipulationKind::ToolExtension,
                ManipulationKind::ElectricDischarge,
            ],
            ToolKind::GeneticManipulation
            | ToolKind::AdvancedMedicine
            | ToolKind::OrganicSynthesis => &[
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
            ],
        }
    }
}
