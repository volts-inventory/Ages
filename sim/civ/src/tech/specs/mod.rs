//! Per-tool unlock prereqs: `min_civ_confirmed_relations`,
//! `min_civ_experimental_relations`, `literacy_floor`,
//! `species_maturity_floor`, `crust_prereqs`, `relation_prereqs`,
//! `tool_prereqs`. The big match arms that describe what the civ
//! and species must have done before each tool becomes conceivable.
//!
//! The unlock pipeline is built on *what the civ has confirmed*
//! (relations the hypothesizer has fit and verified), not *what
//! the cells have seen* (raw template firings). Tools come from
//! experimentation and time, not from inherited environmental
//! pressure.

use super::ToolKind;
use sim_arith::Real;
use sim_physics::Substance;
use sim_world::Crust;

// Submodules split out from the original `specs.rs` so the per-tool
// gate definitions stay readable. Each submodule contributes one
// `impl ToolKind` block; Rust merges them at link time.
mod manipulation;
mod relations;
mod tools;

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
    /// Minimum count of confirmed relations the *civ itself* must
    /// have fit before this tool unlocks. Sum of confirmed firing-
    /// relations plus confirmed measurement-relations across the
    /// civ's active figures.
    ///
    /// This is the "civ experimentation" half of the gate: tech
    /// emerges from the civ's own scientific work, not from
    /// inherited environmental pressure. Successors do *not*
    /// inherit confirmed-relation counts — each civ has to do its
    /// own science.
    ///
    /// Tier ladder:
    /// - tier-1: 0 — pre-literate, foraging, just `relation_prereqs`
    ///   for the specific phenomenon (fire, water, prey).
    /// - tier-2: 5 — the civ has fit a handful of basic laws and is
    ///   ready for settlement-era tools. `ExperimentApparatus` sits
    ///   here too — it's the gateway to "real" science.
    /// - tier-3: 25 — pre-industrial. Sustained scientific
    ///   accumulation across ~250 ticks of figure-hypothesizer work.
    /// - tier-4: 75 — industrial. Multi-generation canon required.
    /// - tier-5: 200 — information-age. Sustained scientific
    ///   tradition across many figure generations. Combined with
    ///   `min_civ_experimental_relations`, demands intervention-
    ///   supported epistemology, not just passive observation.
    #[allow(clippy::match_same_arms)]
    pub fn min_civ_confirmed_relations(self) -> u32 {
        match self {
            // tier-1: no civ-maturity gate. `relation_prereqs` for
            // the relevant phenomenon (fire, water, prey, …) is the
            // only "science" prereq.
            ToolKind::LocalisedCombustion
            | ToolKind::ContactWeapon
            | ToolKind::RangedMomentumWeapon
            | ToolKind::SimpleShelter
            | ToolKind::FoodProcessing
            | ToolKind::FluidGathering
            | ToolKind::BasicTextiles
            | ToolKind::StoneWorking
            | ToolKind::OrganizedHunting
            | ToolKind::BasicHealing => 0,
            // tier-2: settlement-era. A handful of confirmed
            // relations — the civ has begun structured science.
            ToolKind::BulkCultivation
            | ToolKind::AnimalSymbiosis
            | ToolKind::BulkStorage
            | ToolKind::MaterialRefining
            | ToolKind::CulturalEncoding
            | ToolKind::FluidControl
            | ToolKind::WatercraftConstruction
            | ToolKind::PermanentMasonry
            | ToolKind::TradeNetworks
            | ToolKind::UrbanConstruction
            // tier-2 sensorium + apparatus
            | ToolKind::ThermalSensor
            | ToolKind::RemoteAcoustic
            | ToolKind::ExperimentApparatus
            | ToolKind::HerbalMedicine
            | ToolKind::AnimalHusbandry
            | ToolKind::PreservedFood => 5,
            // tier-3: pre-industrial. Sustained hypothesizer
            // activity plus the experimental gate below ensures the
            // apparatus is being used. Lowered 25 → 15 because the
            // viewport-observed plateau showed civs stuck at tier-2
            // for centuries — even active hypothesizers were
            // reaching 25 confirmed only after several civ
            // generations on slow substrates.
            ToolKind::ChemicalProjectile
            | ToolKind::PrecisionTimekeeping
            | ToolKind::MechanicalAdvantage
            | ToolKind::LongRangeNavigation
            | ToolKind::WrittenJurisprudence
            | ToolKind::AbstractMathematics
            | ToolKind::ArtisanalSpecialisation
            | ToolKind::DefensiveFortification
            | ToolKind::MotivePropulsion
            | ToolKind::AmphibiousConstruction
            // tier-3 sensorium
            | ToolKind::FieldSensor
            | ToolKind::DistanceImaging
            | ToolKind::AcousticEngineering
            | ToolKind::HydraulicWorks => 15,
            // tier-4: industrial. Multi-generation canon. Pairs
            // with the experimental floor below. Lowered 75 → 50
            // so a long-lived civ with active apparatus work can
            // reach the industrial age within ~1-2 species
            // lifetimes instead of needing 3-5.
            ToolKind::Mechanisation
            | ToolKind::LongRangeCommunication
            | ToolKind::ChemicalSynthesis
            | ToolKind::MedicalIntervention
            | ToolKind::AdvancedMaterials
            | ToolKind::HeavyTransport
            | ToolKind::PowerGeneration
            | ToolKind::AnalyticalEngines
            | ToolKind::MassLiteracy
            | ToolKind::AerialTransport
            // tier-4 sensorium
            | ToolKind::MagneticSensor
            | ToolKind::PrecisionInstruments
            | ToolKind::DistributedNetworks
            | ToolKind::BiomimeticDesign => 50,
            // tier-5: information-age + transcendence. Combined
            // with the 80-experimental floor and the 3000 species-
            // maturity floor, demands a long-lived civ standing on
            // a deeply-matured species.
            ToolKind::DigitalComputation
            | ToolKind::InformationNetworking
            | ToolKind::GeneticManipulation
            | ToolKind::OrbitalReach
            | ToolKind::AdvancedMedicine
            | ToolKind::MaterialFabrication
            | ToolKind::AutonomousSystems
            | ToolKind::EnergyStorage
            | ToolKind::CryogenicEngineering
            | ToolKind::OrganicSynthesis
            // tier-5 transcendence trio
            | ToolKind::BioelectricResonator
            | ToolKind::FieldPropulsionEngine
            | ToolKind::MetamaterialLattice => 200,
        }
    }

    /// Minimum count of *experimentally-confirmed* relations the
    /// civ itself must have fit. A measurement relation counts here
    /// only if at least one apparatus sample contributed to its fit
    /// pool (`ConfirmedMeasurement.is_experimental == true`).
    ///
    /// This is the "experimentation" half of the gate: passive
    /// observation gets a civ to tier-2 (just enough to build the
    /// apparatus). Past tier-2, real science requires intervention
    /// — clamp x, measure y. A civ that never builds
    /// `ExperimentApparatus` tops out at tier-2 by construction.
    ///
    /// Tier ladder:
    /// - tier-1 / tier-2: 0 — the apparatus isn't yet unlocked, so
    ///   experimental confirmations aren't possible.
    /// - tier-3: 5 — a handful of intervention-supported laws.
    /// - tier-4: 20 — industrial-era requires sustained
    ///   experimental work.
    /// - tier-5: 80 — information-age epistemology.
    #[allow(clippy::match_same_arms)]
    pub fn min_civ_experimental_relations(self) -> u32 {
        match self {
            // tier-1 + tier-2: no experimental gate (apparatus
            // unlocks at tier-2).
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
            | ToolKind::BulkCultivation
            | ToolKind::AnimalSymbiosis
            | ToolKind::BulkStorage
            | ToolKind::MaterialRefining
            | ToolKind::CulturalEncoding
            | ToolKind::FluidControl
            | ToolKind::WatercraftConstruction
            | ToolKind::PermanentMasonry
            | ToolKind::TradeNetworks
            | ToolKind::UrbanConstruction
            | ToolKind::ThermalSensor
            | ToolKind::RemoteAcoustic
            | ToolKind::ExperimentApparatus
            | ToolKind::HerbalMedicine
            | ToolKind::AnimalHusbandry
            | ToolKind::PreservedFood => 0,
            // tier-3: a few apparatus-supported confirmations.
            ToolKind::ChemicalProjectile
            | ToolKind::PrecisionTimekeeping
            | ToolKind::MechanicalAdvantage
            | ToolKind::LongRangeNavigation
            | ToolKind::WrittenJurisprudence
            | ToolKind::AbstractMathematics
            | ToolKind::ArtisanalSpecialisation
            | ToolKind::DefensiveFortification
            | ToolKind::MotivePropulsion
            | ToolKind::AmphibiousConstruction
            | ToolKind::FieldSensor
            | ToolKind::DistanceImaging
            | ToolKind::AcousticEngineering
            | ToolKind::HydraulicWorks => 3,
            // tier-4: sustained experimental tradition. Lowered
            // 20 → 12 alongside the confirmed-relation drop so the
            // experimental-effort budget is proportional and the
            // tier-3 → tier-4 ladder stays climbable.
            ToolKind::Mechanisation
            | ToolKind::LongRangeCommunication
            | ToolKind::ChemicalSynthesis
            | ToolKind::MedicalIntervention
            | ToolKind::AdvancedMaterials
            | ToolKind::HeavyTransport
            | ToolKind::PowerGeneration
            | ToolKind::AnalyticalEngines
            | ToolKind::MassLiteracy
            | ToolKind::AerialTransport
            | ToolKind::MagneticSensor
            | ToolKind::PrecisionInstruments
            | ToolKind::DistributedNetworks
            | ToolKind::BiomimeticDesign => 12,
            // tier-5: information-age + transcendence — the
            // civ has built a mature experimental epistemology.
            ToolKind::DigitalComputation
            | ToolKind::InformationNetworking
            | ToolKind::GeneticManipulation
            | ToolKind::OrbitalReach
            | ToolKind::AdvancedMedicine
            | ToolKind::MaterialFabrication
            | ToolKind::AutonomousSystems
            | ToolKind::EnergyStorage
            | ToolKind::CryogenicEngineering
            | ToolKind::OrganicSynthesis
            | ToolKind::BioelectricResonator
            | ToolKind::FieldPropulsionEngine
            | ToolKind::MetamaterialLattice => 80,
        }
    }

    /// literacy floor: civ's `literacy_score` must
    /// reach this for the tool to unlock. Per-tool placeholders
    /// under.
    pub fn literacy_floor(self) -> Real {
        match self {
            ToolKind::ThermalSensor
            | ToolKind::RemoteAcoustic
            | ToolKind::HerbalMedicine
            | ToolKind::AnimalHusbandry
            | ToolKind::PreservedFood => Real::percent(20),
            ToolKind::FieldSensor
            | ToolKind::DistanceImaging
            | ToolKind::AmphibiousConstruction
            | ToolKind::AcousticEngineering
            | ToolKind::HydraulicWorks => Real::percent(35),
            // Tier-4 alt-path: PrecisionInstruments,
            // DistributedNetworks, BiomimeticDesign — same 0.50
            // floor as the rest of tier-4.
            ToolKind::PrecisionInstruments
            | ToolKind::DistributedNetworks
            | ToolKind::BiomimeticDesign => Real::percent(50),
            // Tier-4 magnetic_sensor and tier-5 (transcendence-tier)
            // tools share a 0.55 floor on per-civ literacy. Tier-5
            // is gated separately by a species-cumulative maturity
            // check (`species_maturity_floor`) on top of this —
            // see `is_unlocked` callers.
            ToolKind::MagneticSensor
            | ToolKind::BioelectricResonator
            | ToolKind::FieldPropulsionEngine
            | ToolKind::MetamaterialLattice => Real::percent(55),
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
            | ToolKind::UrbanConstruction => Real::percent(15),
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
            | ToolKind::MotivePropulsion => Real::percent(30),
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
            | ToolKind::AerialTransport => Real::percent(50),
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
            | ToolKind::OrganicSynthesis => Real::percent(65),
            // tier-2 capability — 0.30 floor matches the
            // tier-3 group above (the civ has begun formal symbol-
            // keeping; sigmoid literacy at 0.30 ≈ ~50 confirmed
            // relations when combined with the obs threshold).
            // Joined with the tier-3 arm via the `|` pattern would
            // misclassify the tier; explicit arm with a documented
            // `match_same_arms` allow keeps the tier identity clear.
            #[allow(clippy::match_same_arms)]
            ToolKind::ExperimentApparatus => Real::percent(30),
        }
    }

    /// Species-cumulative maturity floor. Tier-5 tools demand the
    /// species has been doing science long enough that an
    /// information-age / transcendence capability is conceivable.
    /// Returns the minimum `total_confirmed_relations` (cumulative
    /// across every civ in the run) that must hold before this
    /// tool is buildable. Tier-≤ 4 tools return `0` (no species-
    /// maturity gate; per-civ confirmed + experimental floors
    /// suffice).
    ///
    /// 3000 across the species ≈ 15–60 civ generations of
    /// accumulated science (typical civ confirms ~50–200 relations
    /// over its 500–1500-tick lifespan). On a 9-month planet that
    /// puts tier-5 in the y3,000–y15,000 range, emergent from the
    /// sim's own dynamics rather than from any authored
    /// progression rule.
    ///
    /// Earlier this gated only the three transcendence-tier tools
    /// (BioelectricResonator / FieldPropulsionEngine /
    /// MetamaterialLattice); the 10 information-age tier-5 tools
    /// fell through at 0, letting fresh civs speedrun tier-5 once
    /// their per-civ observation thresholds cleared. Extended to
    /// all 13 tier-5 tools so the species-wide "thousands of years"
    /// anchor applies uniformly across the tier.
    #[allow(clippy::match_same_arms)]
    pub fn species_maturity_floor(self) -> u32 {
        match self {
            // tier-5 transcendence trio
            ToolKind::BioelectricResonator
            | ToolKind::FieldPropulsionEngine
            | ToolKind::MetamaterialLattice => 3_000,
            // tier-5 information-age
            ToolKind::DigitalComputation
            | ToolKind::InformationNetworking
            | ToolKind::GeneticManipulation
            | ToolKind::OrbitalReach
            | ToolKind::AdvancedMedicine
            | ToolKind::MaterialFabrication
            | ToolKind::AutonomousSystems
            | ToolKind::EnergyStorage
            | ToolKind::CryogenicEngineering
            | ToolKind::OrganicSynthesis => 3_000,
            // tier-≤ 4: per-civ gates carry the work.
            _ => 0,
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
}
