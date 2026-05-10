//! Sensorium-extending tech. Tools grant access to physical
//! channels (`ChannelKind` from `sim_recognition`); the union of
//! species-native modalities and civ-unlocked tool channels drives
//! recognition-template perceivability.
//!
//! M3 ships a small registry of physically-grounded tool categories,
//! gated by present-physics + species capability prereqs. The
//! unlock trigger is wall-clock time-gated with prereq lockout
//! (cheap, demonstrable, naturally diverges species). M4
//! replaces the time gate with observation-count + population × literacy.
//!
//! Two further unlock gates turn the registry into a
//! genuine tech tree:
//!  * `relation_prereqs` — `(template_id, ChannelKind)` pairs the civ
//!    must have *confirmed* in its `Hypothesizer` (they understand
//!    the underlying physics, not just observed it). The
//!    `ChannelKind` documents which sensory modality the prereq
//!    belongs to; the lookup is satisfied when any confirmed
//!    relation matches the `template_id` (relations are stored
//!    keyed on a physics-channel rather than a sensory-channel,
//!    so the cleanest interpretation is template-level: "the
//!    civ has fit at least one law about this phenomenon").
//!  * `tool_prereqs` — earlier `ToolKind`s the civ must already
//!    have unlocked. Empty for the original 9 tools (they were
//!    standalone); future capability tools form longer chains.

use sim_species::ManipulationKind;

mod consumption;
mod effects;
mod gating;
mod identity;
mod specs;

#[cfg(test)]
mod tests;

pub use consumption::apply_tool_consumption;
pub use gating::{
    claim_substance_total, is_buildable, is_unlocked, resource_prereqs_missing_count,
    resource_prereqs_satisfied, serendipity_missing_prereqs, serendipity_roll, time_gate_open,
};

/// Stable tool identifier. Match arms by `ToolKind` are the
/// authoring surface; the integer `id` is for protocol events
/// and registry lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ToolKind {
    DistanceImaging,
    RemoteAcoustic,
    FieldSensor,
    ThermalSensor,
    MagneticSensor,
    /// Tier-5 late-game capability. Bioelectric instrumentation
    /// — the species' tools to study its own field signatures.
    /// Narrative milestone (story's "consciousness coupling
    /// engineered"); not a simulated capability.
    BioelectricResonator,
    /// Tier-5 late-game capability. Field-mediated propulsion —
    /// the species engineers coupling to local field structure.
    /// Narrative milestone (story's "field-mediated propulsion");
    /// the sim does not simulate non-Newtonian propulsion physics.
    FieldPropulsionEngine,
    /// Tier-5 late-game capability. Atomic-precision metamaterial
    /// lattice — engineered structured matter with designed
    /// electromagnetic / acoustic responses. Narrative milestone
    /// (story's "metamaterial maturity").
    MetamaterialLattice,
    /// tier-3 capability. Cross-domain habitats — submersible
    /// rigs for terrestrial civs, drylands hatcheries for aquatic
    /// civs. Lifts the `Species::habitat` claim restriction so a
    /// civ can grow territory across both water and land cells.
    /// Pre-unlock, an aquatic civ stays in water and a terrestrial
    /// civ stays on land (habitability still gates deep-ocean
    /// and gas-band uniformly). The unlock is the "your civ has
    /// engineered cross-domain survival" milestone — narratively a
    /// big deal, mechanically just opens the BFS gate to the other
    /// domain. No native-channel prereq (the tool is a synthetic
    /// substitute), tier-3 obs / literacy thresholds.
    AmphibiousConstruction,

    // ─── tier-1 capability tools (stone-age equivalent) ───
    //
    // Tier-1 tools express the earliest applied-knowledge layer —
    // controlled fire, melee/ranged weapons, dwellings, food
    // preparation, fluid containers, woven coverings, knapping,
    // coordinated hunting, basic medicine. Most are gated only on
    // observation pressure (these are pre-literate technologies
    // animals discover; a sapient species discovers them faster
    // but doesn't need formal mathematics for them). LocalisedCombustion
    // and FoodProcessing are the exceptions — combustion-based tools
    // require a confirmed `fire` law because cooking and metallurgy
    // depend on the species *understanding* heat well enough to
    // wield it deliberately.
    /// tier-1. Controlled exothermic reaction — campfires,
    /// torches, cooking heat. The combustion-derived branch's root
    /// node: most chemistry, metallurgy, and steam-era industry
    /// downstream of this. A civ that never observes fire (e.g. a
    /// deep-ocean species on a methane / ammonia world) is locked
    /// out of this branch and pushed toward the alternate
    /// mechanical-and-fluid path through `MechanicalAdvantage`.
    /// Effect: +15% carrying capacity (cooking + light + warmth).
    LocalisedCombustion,
    /// tier-1. Melee tool — club, blade, claw extension. The
    /// most universal weapon: any species with a manipulation
    /// modality and any spatial observations can shape one. No
    /// native-channel or relation prereq; observation threshold
    /// expresses "the species has enough environmental awareness
    /// to make purposeful tools." Effect: +0.10 war strength.
    ContactWeapon,
    /// tier-1. Projectile weapon — thrown rock, bow + arrow,
    /// fluid-jet. Trajectory work pulls in elementary gravity-and-
    /// momentum intuition; relation prereq is the confirmed
    /// `elevation × gravity` linear law. Effect: +0.10 war strength.
    RangedMomentumWeapon,
    /// tier-1. Dwelling — tent, hut, lean-to, dug-out, ice-
    /// burrow. Universal answer to seasonal cold and exposure.
    /// Observation threshold tied to cold-band templates so the
    /// species has actually experienced winter / polar seasons
    /// before it builds shelter for them. Effect: +0.10 seasonal
    /// floor (carrying-capacity floor in extreme months) and +0.05
    /// catastrophe resistance.
    SimpleShelter,
    /// tier-1. Cooking, grinding, fermenting — extracting
    /// more nutrient per fuel unit from a food source. Shares
    /// `LocalisedCombustion`'s `confirmed(fire, temperature)`
    /// relation prereq (no understanding of fire, no fire-cooking).
    /// Effect: +15% carrying capacity (multiplicative).
    FoodProcessing,
    /// tier-1. Containers + transport for solvent — gourds,
    /// skins, hollowed wood, ice cisterns. The species can carry
    /// water (or methane, or ammonia, depending on substrate)
    /// from where it pools to where it lives. Relation prereq is
    /// the confirmed `surface_water` law (knowing where solvent
    /// flows). Effect: +0.05 food-crisis resistance.
    FluidGathering,
    /// tier-1. Woven coverings — fibre + technique. Plant or
    /// animal-derived; observation threshold tied to `fertile_land`
    /// firings so the species knows what fibres to gather. Effect:
    /// +0.05 seasonal floor and +0.05 catastrophe resistance
    /// (clothing as exposure mitigation).
    BasicTextiles,
    /// tier-1. Knapping, chipping, polishing — shaped stone
    /// tools beyond simple impact-cores. Universal manipulation
    /// foundation; no relation prereq (animals knap, this is
    /// pre-symbolic). Effect: +0.05 war strength and +5%
    /// carrying capacity (better tools, more food extracted).
    StoneWorking,
    /// tier-1. Coordinated predation — drives, ambushes,
    /// communal traps. Distinct from `ContactWeapon`: this is the
    /// social-organisation half of hunting, not the weapon.
    /// Effect: +0.10 food-crisis resistance and +0.05 war
    /// strength (organised people fight better too).
    OrganizedHunting,
    /// tier-1. Wound care, herbal remedies, splinting.
    /// Observation threshold tied to `fertile_land` (the species
    /// has noticed which plants help). Effect: +0.10 catastrophe
    /// resistance (epidemics, post-conflict recovery).
    BasicHealing,

    // ─── tier-2 capability tools (settlement-era) ───
    //
    // Tier-2 lifts the species from foraging-bands into permanent
    // settlements: cultivation, domestication, durable storage,
    // metallurgy, symbolic notation, fluid control, watercraft,
    // permanent masonry, exchange networks, urban dwelling. Most
    // tier-2 tools chain through tier-1 prereqs (the strict tier
    // gap allows tool_prereqs without violating the DAG invariant).
    // Effects scale up — capacity ×1.5 to ×2.0, literacy bonuses
    // arrive, expansion-rate bonuses for watercraft and trade.
    /// tier-2. Farming / aquaculture / fungiculture — managed
    /// food production on selected biomes. Builds on `FoodProcessing`
    /// (cultivated food has to be cookable). Relation prereq:
    /// confirmed `fertile_land` law (the civ has fit *something*
    /// about which biomes feed it). Effect: capacity ×2.0.
    BulkCultivation,
    /// tier-2. Animal domestication / symbiosis. Builds on
    /// `OrganizedHunting` (you learn the animal first by hunting
    /// it). Relation prereq: confirmed `fertile_land` (knowing
    /// where animals concentrate). Effect: capacity ×1.5,
    /// food-crisis resistance +0.05.
    AnimalSymbiosis,
    /// tier-2. Pottery / silos / sealed vats. Builds on
    /// `LocalisedCombustion` (firing kilns) and on the confirmed
    /// fire law. Effect: food-crisis resistance +0.12 (representing
    /// the spec's -40% sensitivity reduction on `FOOD_CRISIS_THRESHOLD`).
    BulkStorage,
    /// tier-2. Metallurgy / ceramics — refining shaped raw
    /// materials. Builds on `LocalisedCombustion` (smelting heat)
    /// and `StoneWorking` (the prior craft tradition). Relation
    /// prereq: confirmed fire law. Effect: war strength +0.10
    /// (better weapons), capacity ×1.05.
    MaterialRefining,
    /// tier-2. Writing / symbolic notation — the species can
    /// record knowledge across generations. Builds on `BasicTextiles`
    /// (parchment / hide writing surfaces). No relation prereq —
    /// symbol-systems are pre-physics. Effect: literacy +0.10,
    /// transmission fidelity +0.10.
    CulturalEncoding,
    /// tier-2. Irrigation / drainage / aqueducts — managed
    /// solvent for crops + sanitation. Builds on `FluidGathering`.
    /// Relation prereq: confirmed `surface_water` law. Effect:
    /// capacity ×1.20.
    FluidControl,
    /// tier-2. Boats / rafts / submersibles — water-domain
    /// mobility. Builds on `StoneWorking` (carving / shaping).
    /// Relation prereq: confirmed `surface_water` law (the species
    /// understands wave / pressure mechanics enough to design
    /// hulls). Effect: expansion rate +0.10.
    WatercraftConstruction,
    /// tier-2. Permanent stone / brick construction — beyond
    /// the temporary `SimpleShelter`. Builds on both. Effect:
    /// catastrophe resistance +0.10, seasonal floor +0.05.
    PermanentMasonry,
    /// tier-2. Exchange systems / currency-equivalent. No
    /// physical-engineering prereqs; emerges from settled bands
    /// developing surplus + barter conventions. Effect: literacy
    /// +0.05, expansion rate +0.05.
    TradeNetworks,
    /// tier-2. Multi-dwelling settlements — first cities.
    /// Builds on `SimpleShelter` (the residential unit pattern).
    /// Effect: capacity ×1.10, seasonal floor +0.05.
    UrbanConstruction,

    // ─── tier-3 capability tools (pre-industrial) ───
    //
    // Tier-3 is the substrate-divergence fork point. The
    // combustion-derived branch (ChemicalProjectile from
    // MaterialRefining + BulkStorage + confirmed fire) genuinely
    // locks out no-fire seeds; MechanicalAdvantage's alternate
    // path (StoneWorking + confirmed tidal_extremum) keeps the
    // mechanical-only route to industry open. Per the agreed
    // design (see commit message): no-fire seeds reach a leaner
    // industrial age via wind / water / lever engineering.
    //
    // AmphibiousConstruction is already in place above (, id 9,
    // tier 3) — the new tier-3 variants are 9 in number.
    /// tier-3. Gunpowder-equivalent — exothermic + dense
    /// projectile. Lives at the convergence point of the
    /// combustion branch: requires `MaterialRefining` (tier-2,
    /// metallurgy) + `BulkStorage` (tier-2, sealed containers) +
    /// confirmed fire law. Substrate-locks: a no-fire seed
    /// genuinely cannot reach this. Effect: war strength +0.20.
    ChemicalProjectile,
    /// tier-3. Astronomy / clockwork — periodic
    /// phenomena formalism. Relation prereq: confirmed
    /// `tidal_extremum` law (the periodic substrate). Tool
    /// prereq: `CulturalEncoding` (recording observations).
    /// Effect: literacy +0.05, transmission fidelity +0.05.
    PrecisionTimekeeping,
    /// tier-3. Wheel / pulley / lever — the alternate path
    /// to industry that doesn't require combustion. Relation
    /// prereq: confirmed `tidal_extremum` (proxy for confirmed
    /// gravity-and-mechanics — tides are gravity-driven and
    /// reliably observable on most habitable seeds). Tool prereq:
    /// `StoneWorking` only (no combustion dependency). A no-fire
    /// civ reaches this and proceeds to `Mechanisation` via
    /// wind / water / lever engineering. Effect: capacity ×1.10,
    /// war strength +0.05.
    MechanicalAdvantage,
    /// tier-3. Compass / sextant / dead-reckoning. Tool
    /// prereqs: `WatercraftConstruction` + `CulturalEncoding`
    /// (sea-faring + map-keeping). Effect: expansion rate +0.15.
    LongRangeNavigation,
    /// tier-3. Formally codified law — written constitutional
    /// systems. Tool prereqs: `CulturalEncoding` + `TradeNetworks`
    /// (writing + the surplus that supports a legal class).
    /// Effect: literacy +0.15, transmission fidelity +0.10.
    WrittenJurisprudence,
    /// tier-3. Numerical / geometric formalism — the
    /// civilisation has a *symbolic* mathematics, not just
    /// fitted relations. Tool prereq: `CulturalEncoding`.
    /// Effect: literacy +0.10, transmission fidelity +0.05.
    AbstractMathematics,
    /// tier-3. Guild-craft / division of labour — specialists
    /// produce more per worker than generalists. Tool prereq:
    /// `TradeNetworks` (the exchange network that supports
    /// non-self-sufficient roles). Effect: capacity ×1.10.
    ArtisanalSpecialisation,
    /// tier-3. Walls / keeps / embankments. Tool prereq:
    /// `PermanentMasonry`. Effect: war strength +0.15,
    /// catastrophe resistance +0.10.
    DefensiveFortification,
    /// tier-3. Sailing / current / thermal-draft mobility —
    /// the species moves goods + people without animal traction.
    /// Tool prereq: `WatercraftConstruction`. Relation prereq:
    /// confirmed `surface_water` (water mechanics for sail / wind
    /// reading). Effect: expansion rate +0.10.
    MotivePropulsion,

    // ─── tier-4 capability tools (industrial-equivalent) ───
    //
    // Tier-4 is the industrial peak. Mechanisation is the
    // headline capacity ×3.0 — and per the agreed substrate-
    // divergence design, gates ONLY on MechanicalAdvantage (no
    // combustion required). A no-fire civ that reached
    // MechanicalAdvantage at tier 3 reaches Mechanisation at
    // tier 4 too, with a leaner industrial age (no metallurgy-
    // boosted weapons via ChemicalProjectile, no
    // ChemicalSynthesis branch). The fire-civ stacks
    // MaterialRefining + ChemicalSynthesis multipliers on top.
    //
    // AerialTransport is the one tier-4 tool that DOES require
    // combustion-derived MaterialRefining (lighter-than-air craft
    // need lifting gas; the alternate sub-orbital glider path
    // doesn't get a meaningful effect bonus).
    /// tier-4. Steam / pneumatic / electric mechanisation —
    /// the industrial revolution. Tool prereq: `MechanicalAdvantage`
    /// only (NOT `MaterialRefining` — alternate-path-friendly per
    /// the agreed design). Relation prereq: confirmed
    /// `tidal_extremum` (gravity-mechanics formalism). Effect:
    /// capacity ×3.0 (the spec's headline industrial multiplier),
    /// war strength +0.10.
    Mechanisation,
    /// tier-4. Radio / EM signalling — long-range comms.
    /// Tool prereq: `MaterialRefining` (wire / antenna materials).
    /// Relation prereq: confirmed `lightning_buildup` (EM
    /// physics). Effect: transmission fidelity +0.15,
    /// expansion rate +0.05.
    LongRangeCommunication,
    /// tier-4. Polymers / composites / fertilisers — applied
    /// chemistry beyond metallurgy. Tool prereq: `MaterialRefining`.
    /// Relation prereq: confirmed `fire` law. Effect: capacity
    /// ×1.20.
    ChemicalSynthesis,
    /// tier-4. Surgery / pharmacology / anaesthesia. Tool
    /// prereqs: `BasicHealing` (the tradition) + `AbstractMathematics`
    /// (formal physiology / dosage models). Effect: catastrophe
    /// resistance +0.15, capacity ×1.10.
    MedicalIntervention,
    /// tier-4. Alloys / superconductors / engineered ceramics.
    /// Tool prereq: `MaterialRefining`. Relation prereq: confirmed
    /// `fire` law. Effect: war strength +0.10, catastrophe
    /// resistance +0.05.
    AdvancedMaterials,
    /// tier-4. Rail / highway / canal-equivalent — bulk
    /// terrestrial transport. Tool prereqs: `MotivePropulsion` +
    /// `MechanicalAdvantage` (engines + roadbeds). Effect:
    /// expansion rate +0.20.
    HeavyTransport,
    /// tier-4. Turbines / cells / reactors — power generation
    /// at scale. Tool prereq: `MechanicalAdvantage`. Relation
    /// prereq: confirmed `lightning_buildup` (EM coupling for
    /// generators). Effect: capacity ×1.15.
    PowerGeneration,
    /// tier-4. Mechanical / electromechanical computation —
    /// pre-electronic computers (gears, relays, vacuum tubes).
    /// Tool prereqs: `AbstractMathematics` (formal logic) +
    /// `PrecisionTimekeeping` (the clockwork tradition). Effect:
    /// literacy +0.05, transmission fidelity +0.05.
    AnalyticalEngines,
    /// tier-4. Universal symbol-encoding access — printing
    /// presses, public schooling, mass-circulated text. Tool
    /// prereq: `WrittenJurisprudence` (the formalised legal
    /// substrate that motivates universal literacy). Effect:
    /// literacy +0.20 (the spec's headline literacy lifter),
    /// transmission fidelity +0.15.
    MassLiteracy,
    /// tier-4. Flight / buoyancy / levitation — aerial
    /// transport. Tool prereqs: `MaterialRefining` (lifting-gas
    /// chemistry / structural alloys) + `MotivePropulsion`
    /// (powered drive). Relation prereq: confirmed
    /// `tidal_extremum` (gravity formalism for aerodynamics).
    /// Effect: expansion rate +0.20.
    AerialTransport,

    // ─── tier-5 capability tools (information-age) ───
    //
    // Tier-5 (per spec: "literacy ≥ 0.65") is the species'
    // information age — programmable computation, networked
    // communication, genetic engineering, orbital reach, advanced
    // medicine, additive fabrication, autonomous systems, energy
    // storage, cryogenic preservation, biomolecule synthesis.
    // These coexist at tier 5 with the existing transcendence-tier
    // tools (BioelectricResonator, FieldPropulsionEngine,
    // MetamaterialLattice) — all tier 5, but the transcendence
    // trio additionally requires `species_maturity_floor` (3000
    // cumulative confirmed relations). Tier-5 capabilities chain
    // through tier-4 tool prereqs.
    /// tier-5. Programmable digital logic — silicon /
    /// photonic / quantum computation. Tool prereq:
    /// `AnalyticalEngines` (the mechanical computation tradition).
    /// Relation prereq: confirmed `lightning_buildup` (semiconductor
    /// physics rests on EM). Effect: literacy +0.10.
    DigitalComputation,
    /// tier-5. Distributed comms grid — internet equivalent.
    /// Tool prereq: `LongRangeCommunication`. Relation prereq:
    /// confirmed `lightning_buildup`. Effect: transmission
    /// fidelity +0.15, literacy +0.10.
    InformationNetworking,
    /// tier-5. Substrate-organism engineering — molecular
    /// biology applied. Tool prereqs: `MedicalIntervention` +
    /// `ChemicalSynthesis`. Effect: capacity ×1.20, catastrophe
    /// resistance +0.10.
    GeneticManipulation,
    /// tier-5. Escape-velocity-equivalent transport — orbital
    /// flight. Tool prereqs: `Mechanisation` (engines) +
    /// `AerialTransport` (the atmospheric flight tradition). NOTE:
    /// `AerialTransport` is combustion-locked (via `MaterialRefining`),
    /// so `OrbitalReach` inherits that lock. A no-fire civ that
    /// reached `Mechanisation` via the alternate path doesn't reach
    /// orbit — chemical rocketry / lifting gas requires combustion-
    /// derived chemistry. Relation prereq: confirmed
    /// `tidal_extremum` (orbital mechanics formalism). Effect:
    /// expansion rate +0.30.
    OrbitalReach,
    /// tier-5. Molecular / genetic medicine — beyond
    /// surgery + pharmacology. Tool prereq: `MedicalIntervention`.
    /// Effect: catastrophe resistance +0.15, capacity ×1.20.
    AdvancedMedicine,
    /// tier-5. Additive / molecular fabrication — print-on-
    /// demand structural matter. Tool prereq: `AdvancedMaterials`.
    /// Relation prereq: confirmed `fire` law (high-temperature
    /// processing). Effect: capacity ×1.10, war strength +0.05.
    MaterialFabrication,
    /// tier-5. Robotics + automation — labour-multiplying
    /// machines. Tool prereqs: `Mechanisation` + `AnalyticalEngines`
    /// (engines + control logic). Effect: capacity ×1.15.
    AutonomousSystems,
    /// tier-5. High-density power containment — batteries /
    /// flywheels / hydrogen / capacitor banks. Tool prereq:
    /// `PowerGeneration`. Relation prereq: confirmed
    /// `lightning_buildup` (electrochemistry). Effect: seasonal
    /// floor +0.10, capacity ×1.10.
    EnergyStorage,
    /// tier-5. Substrate-freeze preservation — extreme-cold
    /// engineering. Tool prereq: `AdvancedMaterials`. Relation
    /// prereq: confirmed `ice_present` law (cold-phase mastery).
    /// Effect: catastrophe resistance +0.05, capacity ×1.05.
    CryogenicEngineering,
    /// tier-5. Biomolecule fabrication — cultured proteins,
    /// designer biopolymers. Tool prereq: `ChemicalSynthesis`.
    /// Effect: capacity ×1.10. (On hydrocarbon-substrate worlds
    /// this is a particularly natural progression — confirmed
    /// `fertile_land` + confirmed `fire` chain through cleanly.)
    OrganicSynthesis,

    /// tier-2 capability. Civ-built experiment apparatus —
    /// the species' first move from passive observation to
    /// controlled-conditions intervention. Each unlocked apparatus
    /// clamps a physics channel at one of four ladder values pre-
    /// tick and reads the post-physics response in the apparatus
    /// cell, feeding the hypothesizer's measurement track with a
    /// (controlled x, response y) pair instead of whatever
    /// planetary heterogeneity passive observation provides.
    ///
    /// Tier 2; same gates as the established sensorium tier-2
    /// tools (observation 30k, literacy 0.30) plus the universal
    /// `MANIPULATION_PREREQ = ToolExtension` enforced in
    /// `is_buildable`. Substrate gate: confirmed `fire` law (the
    /// civ has fit *something* about controlled physical conditions
    /// before it builds a controlled-conditions device). No
    /// tool prereq, no granted channel — the effect is on the
    /// discovery layer (faster + cleaner law recovery), not on
    /// perception or capacity.
    ///
    /// Vision-aligned: a tactile-only ToolExtension-bearing species
    /// with a confirmed thermal law builds an apparatus and starts
    /// recovering its planet's `α` from clean diffusion experiments
    /// — that's the "Galileo, not Aristotle" upgrade the project
    /// has been missing. A no-tool species (chemical-modality
    /// floaters, etc.) stays observation-only forever, sustaining
    /// the "different sciences" goal.
    ExperimentApparatus,
}

/// **Retired :** previously the wall-clock unlock gate.
/// Kept as a constant for backwards readability of historical logs;
/// the production trigger now reads observation pressure +
/// literacy via `is_unlocked`.
/// scaled ×12 (year-meant: "tier × 200 years").
pub const TIER_UNLOCK_PERIOD_TICKS: u64 = 200 * protocol::MONTHS_PER_YEAR;
/// Manipulation mode prereq for every tool. Constructing a
/// sensorium-extending instrument needs a tool-using body plan;
/// species lacking `ToolExtension` are permanently locked out of
/// the entire registry. This is the central mechanism by which
/// non-tool-using species reach genuinely different sciences.
pub const MANIPULATION_PREREQ: ManipulationKind = ManipulationKind::ToolExtension;
