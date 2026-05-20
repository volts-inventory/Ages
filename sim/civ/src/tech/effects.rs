//! per-tool effect contributions: capacity multiplier + 7
//! additive bonuses (food crisis, war strength, seasonal floor,
//! catastrophe resistance, literacy, expansion rate, transmission
//! fidelity). Civ-level aggregators in `crate::tools` fold each
//! tool's contribution across `unlocked_tools`.

use super::ToolKind;
use sim_arith::Real;

impl ToolKind {
    // ─── effect categories ───
    //
    // Tools "do something" for the civ — extend carrying capacity,
    // shore up food security, harden against catastrophe, lift
    // literacy floor, multiply war strength, raise the seasonal
    // population floor, speed expansion, harden knowledge
    // transmission. Each method below returns this tool's
    // contribution; `Civ`-level aggregators (in `lib.rs`) fold the
    // contributions across `unlocked_tools`.
    //
    // Sign / type convention:
    //  * `*_multiplier` returns a multiplicative factor (Real::ONE
    //  = no effect; > Real::ONE means "this tool helps").
    //  * `*_bonus` returns an additive shift (Real::ZERO = no
    //  effect; positive = "this tool helps").
    // Civ aggregators apply the right combinator (product for
    // multipliers, sum for bonuses) and pass into the consuming
    // call sites (`carrying_capacity_with_terrain`,
    // `check_collapse_with_terrain`, `conflict::strength`,
    // `literacy_score`).

    /// Tool's multiplicative contribution to carrying capacity.
    /// Tier-1 `LocalisedCombustion` + `FoodProcessing` each give
    /// ×1.15 (cooking + warmth + better food extraction);
    /// `StoneWorking` gives ×1.05; everything else neutral.
    #[allow(clippy::match_same_arms)]
    pub fn capacity_multiplier(self) -> Real {
        match self {
            // Tier-1
            ToolKind::LocalisedCombustion => Real::percent(115),
            ToolKind::FoodProcessing => Real::percent(115),
            ToolKind::StoneWorking => Real::percent(105),
            // Tier-2: agriculture is the headline demographic-
            // transition jump — settled farming was *the* biggest
            // density bump in real history (paleolithic 0.1/km² →
            // neolithic 5/km²). `BulkCultivation` ×5.0 carries that
            // weight; `AnimalSymbiosis` ×2.5 layers plough animals +
            // livestock on top. `FluidControl` (irrigation),
            // `UrbanConstruction` (settlement density), and
            // `MaterialRefining` (craft surplus) add the smaller
            // amplifications around it.
            ToolKind::BulkCultivation => Real::from_int(5),
            ToolKind::AnimalSymbiosis => Real::percent(250),
            ToolKind::FluidControl => Real::percent(120),
            ToolKind::UrbanConstruction => Real::percent(110),
            ToolKind::MaterialRefining => Real::percent(105),
            // Tier-3: `ArtisanalSpecialisation` + `MechanicalAdvantage`
            // give craft-amplification and labour-amplification
            // respectively. Both ×1.10 — the major capacity jumps
            // live at tier-2 (cultivation / domestication) and tier-4
            // (Mechanisation ×10.0). `AmphibiousConstruction` ×1.05 —
            // the habitat-lift in `can_claim_glyph` is the headline
            // mechanical effect, but living-on-water construction
            // (stilts, floating platforms, sea walls) also lifts cell
            // capacity directly.
            ToolKind::ArtisanalSpecialisation => Real::percent(110),
            ToolKind::MechanicalAdvantage => Real::percent(110),
            ToolKind::AmphibiousConstruction => Real::percent(105),
            // Tier-4: the industrial-revolution headline. `Mechanisation`
            // ×10.0 — mechanised agriculture + rail logistics drove
            // the 19th-century population explosion. `ChemicalSynthesis`
            // ×3.0 — Haber-Bosch nitrogen fixation alone underwrites
            // ~half of modern food supply. `MedicalIntervention` ×2.0
            // — germ theory + sanitation collapse child mortality.
            // `PowerGeneration` ×1.15 (energy abundance).
            ToolKind::Mechanisation => Real::from_int(10),
            ToolKind::ChemicalSynthesis => Real::from_int(3),
            ToolKind::PowerGeneration => Real::percent(115),
            ToolKind::MedicalIntervention => Real::from_int(2),
            // Tier-5 (information-age): genetic engineering and
            // medicine push capacity hard. `AdvancedMedicine` ×3.0
            // (antibiotics + vaccines → infant-mortality collapse on
            // top of tier-4 sanitation). `GeneticManipulation` ×2.0
            // (GMO yields, precision agriculture).
            // Autonomous systems + material fabrication add labour
            // amplification on top of Mechanisation; energy storage +
            // cryogenic engineering smooth the seasonal floor.
            ToolKind::GeneticManipulation => Real::from_int(2),
            ToolKind::AdvancedMedicine => Real::from_int(3),
            ToolKind::AutonomousSystems => Real::percent(115),
            ToolKind::MaterialFabrication => Real::percent(110),
            ToolKind::EnergyStorage => Real::percent(110),
            ToolKind::CryogenicEngineering => Real::percent(105),
            ToolKind::OrganicSynthesis => Real::percent(110),
            // Tier-5 transcendence trio: narrative milestones, but
            // they shouldn't be mechanically inert. MetamaterialLattice
            // is the capacity headline (programmable bulk materials
            // — pre-tested for arbitrary load specs).
            ToolKind::MetamaterialLattice => Real::percent(125),
            // Alternate-path additions ─────────────────────────
            // AnimalHusbandry: selective breeding 2-3× over wild
            // domestication — significant tier-2 density bump for
            // biota-rich worlds. Below AnimalSymbiosis's 2.5× since
            // it stacks on top of that (the prereq).
            ToolKind::AnimalHusbandry => Real::percent(140),
            // PreservedFood: small capacity lift (better surplus
            // → bigger population that survives lean seasons), but
            // the headline effect is on food_crisis_resistance.
            ToolKind::PreservedFood => Real::percent(110),
            // BiomimeticDesign: clever construction — modest
            // capacity bump from copying biological efficiency.
            ToolKind::BiomimeticDesign => Real::percent(110),
            // HydraulicWorks: irrigation + reservoir buffering at
            // scale — significant tier-3 boost. Sits between
            // FluidControl (1.20) and Mechanisation (10.0).
            ToolKind::HydraulicWorks => Real::percent(150),
            // PrecisionInstruments: indirect — through the
            // discovery rate it unlocks (faster law-fitting →
            // faster downstream tools). Direct capacity impact
            // small.
            ToolKind::PrecisionInstruments => Real::percent(105),
            // DistributedNetworks: economic coordination at scale.
            // Modest capacity lift; main effect on cohesion +
            // literacy.
            ToolKind::DistributedNetworks => Real::percent(115),
            // WindPower: pre-combustion mechanical energy — sails
            // + windmills mill grain, lift water, drive pumps.
            // Modest tier-2 capacity bump, mostly through saved
            // labour.
            ToolKind::WindPower => Real::percent(115),
            // CodexTradition: indirect — the lifted literacy +
            // transmission do most of the work. Small direct
            // capacity lift through better record-keeping +
            // administration.
            ToolKind::CodexTradition => Real::percent(108),
            // GeneCultureCoevolution: formal selection theory
            // applied to crops + herds. Big tier-4 capacity lift,
            // comparable to industrial chemistry (3×) since
            // controlled breeding underwrites the Green Revolution
            // density jump in real history.
            ToolKind::GeneCultureCoevolution => Real::from_int(3),
            _ => Real::ONE,
        }
    }

    /// Tool's additive contribution to food-crisis floor (raises
    /// the security threshold so the civ tolerates leaner harvests
    /// before tipping into `FoodCrisis` collapse).
    #[allow(clippy::match_same_arms)]
    pub fn food_crisis_resistance_bonus(self) -> Real {
        match self {
            ToolKind::FluidGathering => Real::percent(5),
            ToolKind::OrganizedHunting => Real::percent(10),
            // Tier-2: BulkStorage is the headline contributor
            // (granaries / pottery / sealed vats); BulkCultivation
            // and AnimalSymbiosis add steady-state surplus.
            // BulkStorage's +0.12 represents the spec's ~-40%
            // sensitivity reduction on the FOOD_CRISIS_THRESHOLD
            // (0.30 → ~0.18 floor).
            ToolKind::BulkStorage => Real::percent(12),
            ToolKind::BulkCultivation => Real::percent(8),
            ToolKind::AnimalSymbiosis => Real::percent(5),
            // HerbalMedicine: famine fallback (edible-plant
            // catalogue doubles as starvation-tier food).
            ToolKind::HerbalMedicine => Real::percent(4),
            // AnimalHusbandry: insurance via standing herd. Below
            // BulkStorage's 12 but real.
            ToolKind::AnimalHusbandry => Real::percent(8),
            // PreservedFood: headline effect. Drying / brining /
            // fermentation buffer lean seasons — matches BulkStorage
            // calibration since the function is identical, just the
            // chemistry differs (no kiln required).
            ToolKind::PreservedFood => Real::percent(10),
            // HydraulicWorks: reservoir buffering smooths drought
            // years.
            ToolKind::HydraulicWorks => Real::percent(8),
            // GeneCultureCoevolution: selective-breeding-driven
            // crop + herd resilience — the headline late-game
            // food-security tool.
            ToolKind::GeneCultureCoevolution => Real::percent(15),
            _ => Real::ZERO,
        }
    }

    /// Tool's additive contribution to war strength (folded into
    /// `conflict::strength` as a multiplicative `(1 + Σbonus)`
    /// factor). Tier-1 weapons + organised hunting give the
    /// foundational bonuses.
    #[allow(clippy::match_same_arms)]
    pub fn war_strength_bonus(self) -> Real {
        match self {
            ToolKind::ContactWeapon => Real::percent(10),
            ToolKind::RangedMomentumWeapon => Real::percent(10),
            ToolKind::StoneWorking => Real::percent(5),
            ToolKind::OrganizedHunting => Real::percent(5),
            // Tier-2: MaterialRefining gives the metallurgy bump
            // to weapons; PermanentMasonry hardens defensible
            // strongpoints.
            ToolKind::MaterialRefining => Real::percent(10),
            ToolKind::PermanentMasonry => Real::percent(5),
            // Tier-3: ChemicalProjectile is the headline +0.20
            // (gunpowder is decisive); DefensiveFortification +0.15
            // (walls + keeps); MechanicalAdvantage adds +0.05 (siege
            // engines, lever-arms).
            ToolKind::ChemicalProjectile => Real::percent(20),
            ToolKind::DefensiveFortification => Real::percent(15),
            ToolKind::MechanicalAdvantage => Real::percent(5),
            // Tier-4: Mechanisation +0.10 (mechanised armies);
            // AdvancedMaterials +0.10 (alloyed weapons + armour).
            ToolKind::Mechanisation => Real::percent(10),
            ToolKind::AdvancedMaterials => Real::percent(10),
            // Tier-5: MaterialFabrication +0.05 (custom-fabbed
            // armaments); AutonomousSystems +0.05 (drones).
            ToolKind::MaterialFabrication => Real::percent(5),
            ToolKind::AutonomousSystems => Real::percent(5),
            // Tier-5 transcendence: MetamaterialLattice +0.10
            // (cloaking-grade armour + active-response plating).
            ToolKind::MetamaterialLattice => Real::percent(10),
            // Tier-3 sensorium: DistanceImaging +0.05 — long-range
            // observation as a tactical advantage (rangefinding,
            // forward intelligence).
            ToolKind::DistanceImaging => Real::percent(5),
            // BiomimeticDesign: clever weapon + armour
            // construction (claws, spines, lamellar from biology).
            ToolKind::BiomimeticDesign => Real::percent(5),
            _ => Real::ZERO,
        }
    }

    /// Tool's additive contribution to seasonal floor (raises the
    /// per-cell carrying-capacity floor in extreme months — winter
    /// for high-tilt worlds, etc.). `SimpleShelter` and
    /// `BasicTextiles` are the tier-1 pillars.
    #[allow(clippy::match_same_arms)]
    pub fn seasonal_floor_bonus(self) -> Real {
        match self {
            ToolKind::SimpleShelter => Real::percent(10),
            ToolKind::BasicTextiles => Real::percent(5),
            // Tier-2: PermanentMasonry adds seasonal-floor lift
            // (durable storerooms + sheltered urban interiors);
            // FluidControl helps too (irrigation buffers
            // dry-season collapse); UrbanConstruction adds
            // density-of-shelter.
            ToolKind::PermanentMasonry => Real::percent(5),
            ToolKind::FluidControl => Real::percent(5),
            ToolKind::UrbanConstruction => Real::percent(5),
            _ => Real::ZERO,
        }
    }

    /// Tool's additive contribution to catastrophe resistance
    /// (reduces population loss from disease / volcanism / etc.).
    #[allow(clippy::match_same_arms)]
    pub fn catastrophe_resistance_bonus(self) -> Real {
        match self {
            ToolKind::SimpleShelter => Real::percent(5),
            ToolKind::BasicTextiles => Real::percent(5),
            ToolKind::BasicHealing => Real::percent(10),
            // Tier-2: PermanentMasonry hardens against volcanism /
            // storms; BulkStorage softens the hit from disease /
            // famine waves.
            ToolKind::PermanentMasonry => Real::percent(10),
            ToolKind::BulkStorage => Real::percent(5),
            // Tier-3: DefensiveFortification reads the catastrophe
            // signal — walls don't only resist sieges, they shelter
            // populations from flood / volcanism / weather extremes.
            ToolKind::DefensiveFortification => Real::percent(10),
            // Tier-4: MedicalIntervention is the spec's catastrophe
            // headline (epidemic mitigation); AdvancedMaterials
            // gives a small structural-resilience bump.
            ToolKind::MedicalIntervention => Real::percent(15),
            ToolKind::AdvancedMaterials => Real::percent(5),
            // Tier-5: AdvancedMedicine + GeneticManipulation lift
            // disease resistance further; CryogenicEngineering
            // adds a small bump (cold-storage of medicine /
            // food).
            ToolKind::AdvancedMedicine => Real::percent(15),
            ToolKind::GeneticManipulation => Real::percent(10),
            ToolKind::CryogenicEngineering => Real::percent(5),
            // Tier-5 transcendence: MetamaterialLattice +0.10
            // (active-response structural materials reduce loss to
            // floods, storms, blast events); BioelectricResonator
            // +0.05 (medical diagnostic resonance for early disease
            // detection).
            ToolKind::MetamaterialLattice => Real::percent(10),
            ToolKind::BioelectricResonator => Real::percent(5),
            _ => Real::ZERO,
        }
    }

    /// Tool's multiplicative bonus to species lifespan years.
    /// `0.0` = no extension; `0.20` = +20% biological lifespan.
    /// Aggregated additively across unlocked tools by
    /// `Civ::tool_lifespan_extension_factor()`. Distinct from
    /// `mortality_reduction_per_bracket` (which raises *realised*
    /// life expectancy by cutting deaths-per-tick); this knob
    /// extends the species's *biological* maximum lifespan via
    /// senescence treatment + cellular regeneration. Only the
    /// most advanced tools touch this — pre-modern medicine
    /// reduces deaths but doesn't raise the biological cap.
    #[allow(clippy::match_same_arms)]
    pub fn lifespan_extension_factor(self) -> Real {
        match self {
            // Tier-4: modern medicine starts to nudge the cap
            // (vaccines + clean water moved homo sapiens from
            // ~40 to ~70 mostly through reduced mortality, but
            // a few % comes from raising max lifespan).
            ToolKind::MedicalIntervention => Real::percent(5),
            // Tier-5: advanced medicine adds another +10% via
            // chronic-disease control + organ replacement.
            ToolKind::AdvancedMedicine => Real::percent(10),
            // Tier-5: GeneticManipulation is the headline
            // lifespan extension — direct senescence treatment.
            ToolKind::GeneticManipulation => Real::percent(20),
            // Tier-5 transcendence: BioelectricResonator +0.10
            // (continuous-monitor diagnostics catch organ failure
            // pre-symptom, extending elder lifespan further).
            ToolKind::BioelectricResonator => Real::percent(10),
            _ => Real::ZERO,
        }
    }

    /// Tool's per-bracket per-tick mortality reduction. Returned
    /// as `[infant, juvenile, fertile, elder]`; each entry is a
    /// fraction in `[0, 1]` that scales that bracket's per-tick
    /// mortality down by `(1 - reduction)`. Aggregated additively
    /// across unlocked tools then folded into `PopulationDynamics::
    /// mortality_reduction` each tick before stepping.
    ///
    /// Tier-1 shelter / textiles primarily protect infants and
    /// elders (the climate-vulnerable brackets). Tier-2
    /// `BasicHealing` is the first explicit medical intervention —
    /// targets infants + juveniles. Tier-2 `BulkStorage` and
    /// tier-1 `FoodProcessing` smooth lean-time mortality across
    /// young brackets via reliable nutrition. Tier-3
    /// `FluidControl` (sanitation) cuts infant + juvenile mortality
    /// — the historic clean-water leap. Tier-4
    /// `MedicalIntervention` is the broadest cross-bracket lift.
    /// Tier-5 `AdvancedMedicine` deepens that lift; `GeneticManipulation`
    /// is the elder-bracket headline (senescence treatment).
    #[allow(clippy::match_same_arms)]
    pub fn mortality_reduction_per_bracket(self) -> [Real; 4] {
        let zero = [Real::ZERO; 4];
        match self {
            // Tier-1: shelter + clothing protect infants + elders
            // from cold-stress mortality.
            ToolKind::SimpleShelter => [Real::percent(5), Real::ZERO, Real::ZERO, Real::percent(5)],
            ToolKind::BasicTextiles => [Real::percent(5), Real::ZERO, Real::ZERO, Real::percent(5)],
            ToolKind::FoodProcessing => {
                [Real::percent(5), Real::percent(5), Real::ZERO, Real::ZERO]
            }
            // Tier-2: foundational medicine + storage.
            ToolKind::BasicHealing => [
                Real::percent(15),
                Real::percent(10),
                Real::percent(5),
                Real::ZERO,
            ],
            ToolKind::BulkStorage => [
                Real::percent(5),
                Real::percent(5),
                Real::ZERO,
                Real::percent(5),
            ],
            // Tier-3: sanitation (fluid control = clean water +
            // sewage management). Cuts infant + juvenile mortality
            // sharply — the historical clean-water leap halved
            // childhood deaths.
            ToolKind::FluidControl => {
                [Real::percent(15), Real::percent(10), Real::ZERO, Real::ZERO]
            }
            // Tier-4: modern medicine across the board.
            ToolKind::MedicalIntervention => [
                Real::percent(15),
                Real::percent(15),
                Real::percent(10),
                Real::percent(5),
            ],
            // Tier-5: advanced medicine + senescence treatment.
            ToolKind::AdvancedMedicine => [
                Real::percent(15),
                Real::percent(15),
                Real::percent(15),
                Real::percent(10),
            ],
            // GeneticManipulation: elder-bracket headline.
            ToolKind::GeneticManipulation => [
                Real::percent(10),
                Real::percent(10),
                Real::percent(10),
                Real::percent(20),
            ],
            // Tier-5 transcendence: BioelectricResonator —
            // bioelectric instrumentation reads physiology directly,
            // catching disease in fertile + elder brackets earlier
            // than imaging-based diagnosis.
            ToolKind::BioelectricResonator => [
                Real::ZERO,
                Real::percent(5),
                Real::percent(10),
                Real::percent(15),
            ],
            // HerbalMedicine: meaningful but smaller than
            // BasicHealing — herbal pharmacopoeia helps but
            // doesn't substitute for the broader healing tradition.
            // Tilted toward infant + fertile (childbirth, wound
            // care) over elder (geriatric care needs more than
            // herbs).
            ToolKind::HerbalMedicine => [
                Real::percent(8),
                Real::percent(5),
                Real::percent(5),
                Real::percent(2),
            ],
            _ => zero,
        }
    }

    /// Tool's additive contribution to literacy (per the civ's
    /// `literacy_score` has a recordable-symbology component;
    /// `CulturalEncoding` / `WrittenJurisprudence` / `MassLiteracy` /
    /// `InformationNetworking` are the tools that bump this above
    /// raw discovery-rate. Tier-1 tools all contribute zero —
    /// pre-symbolic technologies don't lift formal literacy.
    #[allow(clippy::match_same_arms)]
    pub fn literacy_bonus(self) -> Real {
        match self {
            // Tier-2: CulturalEncoding gives the headline +0.10
            // (writing systems lift literacy directly); TradeNetworks
            // adds a small +0.05 (settled bands with surplus develop
            // record-keeping conventions even before formal writing).
            ToolKind::CulturalEncoding => Real::percent(10),
            ToolKind::TradeNetworks => Real::percent(5),
            // Tier-3: WrittenJurisprudence is the spec's +0.15
            // headline; AbstractMathematics adds +0.10 (formal
            // notation lifts literacy further); PrecisionTimekeeping
            // a small +0.05 (calendar-keeping is a literacy
            // contributor).
            ToolKind::WrittenJurisprudence => Real::percent(15),
            ToolKind::AbstractMathematics => Real::percent(10),
            ToolKind::PrecisionTimekeeping => Real::percent(5),
            // Tier-4: MassLiteracy is the spec's headline +0.20
            // (universal symbol-encoding access); AnalyticalEngines
            // a small +0.05 (computation as a literacy boost).
            ToolKind::MassLiteracy => Real::percent(20),
            ToolKind::AnalyticalEngines => Real::percent(5),
            // Tier-5: DigitalComputation +0.10 (ubiquitous
            // computational tools); InformationNetworking +0.10
            // (knowledge-access at network speed).
            ToolKind::DigitalComputation => Real::percent(10),
            ToolKind::InformationNetworking => Real::percent(10),
            // AcousticEngineering: amphitheatres / bell towers /
            // public oratory carry oral curriculum further, so
            // literacy lifts even before mass writing.
            ToolKind::AcousticEngineering => Real::percent(8),
            // DistributedNetworks: news + notice propagation lifts
            // literacy across a polity.
            ToolKind::DistributedNetworks => Real::percent(10),
            // PrecisionInstruments: clean measurement reinforces
            // formal-quantitative literacy (every craftsperson
            // now reads a calibrated scale).
            ToolKind::PrecisionInstruments => Real::percent(5),
            // CodexTradition: bound-volume codices make recorded
            // knowledge durable and portable. Headline tier-3
            // literacy boost.
            ToolKind::CodexTradition => Real::percent(12),
            _ => Real::ZERO,
        }
    }

    /// Tool's additive contribution to expansion-rate scaling
    /// (territory growth speed). Tier-1 tools don't accelerate
    /// expansion (a band that has shelter and clubs still spreads
    /// at the same foot-pace). Tier-3 navigation / watercraft
    /// and tier-4 transport tools fill this in via match arms in
    /// later commits.
    #[allow(clippy::match_same_arms)]
    pub fn expansion_rate_bonus(self) -> Real {
        match self {
            // Tier-2: WatercraftConstruction opens water domains
            // for expansion; TradeNetworks accelerates it via
            // exchange + survey.
            ToolKind::WatercraftConstruction => Real::percent(10),
            ToolKind::TradeNetworks => Real::percent(5),
            // Tier-3: LongRangeNavigation +0.15 (sextants /
            // compass cross oceans); MotivePropulsion +0.10
            // (sails / animal traction).
            ToolKind::LongRangeNavigation => Real::percent(15),
            ToolKind::MotivePropulsion => Real::percent(10),
            // Tier-4: HeavyTransport + AerialTransport each +0.20
            // (mechanised land + air); LongRangeCommunication +0.05
            // (synchronised expansion via remote coordination).
            ToolKind::HeavyTransport => Real::percent(20),
            ToolKind::AerialTransport => Real::percent(20),
            ToolKind::LongRangeCommunication => Real::percent(5),
            // Tier-5: OrbitalReach +0.30 (escape velocity opens
            // off-world expansion).
            ToolKind::OrbitalReach => Real::percent(30),
            // Tier-5 transcendence: FieldPropulsionEngine +0.30
            // (reactionless propulsion + atmospheric independence
            // — at parity with OrbitalReach).
            ToolKind::FieldPropulsionEngine => Real::percent(30),
            // Tier-3 sensorium: DistanceImaging +0.05 (telescopes
            // + cartography accelerate frontier surveying);
            // tier-2 RemoteAcoustic +0.05 (sonar + echolocation
            // for safer water transit).
            ToolKind::DistanceImaging => Real::percent(5),
            ToolKind::RemoteAcoustic => Real::percent(5),
            // AmphibiousConstruction +0.05 — habitat-lift via
            // `can_claim_glyph` is the headline, but built
            // amphibian platforms also speed expansion across
            // mixed terrain.
            ToolKind::AmphibiousConstruction => Real::percent(5),
            // WindPower: sail-driven exploration. Same scale as
            // WatercraftConstruction since they share the rigging
            // technology.
            ToolKind::WindPower => Real::percent(10),
            _ => Real::ZERO,
        }
    }

    /// Tool's additive contribution to inter-civ knowledge-
    /// transmission fidelity. Tier-1 has nothing here —
    /// without symbolic encoding, knowledge passes orally and
    /// decays with linguistic distance per the existing
    /// model. `CulturalEncoding` / mass-literacy / networking
    /// fill this in at later tiers.
    #[allow(clippy::match_same_arms)]
    pub fn transmission_fidelity_bonus(self) -> Real {
        match self {
            // Tier-2: CulturalEncoding directly lifts transmission
            // fidelity (written knowledge degrades less across
            // linguistic distance than oral); TradeNetworks adds
            // a small bump (cross-civ contact volume).
            ToolKind::CulturalEncoding => Real::percent(10),
            ToolKind::TradeNetworks => Real::percent(5),
            // Tier-3: WrittenJurisprudence (canonised legal
            // codes are universally interpretable) +0.10;
            // AbstractMathematics +0.05 (formal notation
            // crosses linguistic boundaries); PrecisionTimekeeping
            // +0.05 (shared calendars synchronise records).
            ToolKind::WrittenJurisprudence => Real::percent(10),
            ToolKind::AbstractMathematics => Real::percent(5),
            ToolKind::PrecisionTimekeeping => Real::percent(5),
            // Tier-4: LongRangeCommunication +0.15 (radio /
            // EM telegraphy is the headline transmission lifter);
            // MassLiteracy +0.15 (universal access); AnalyticalEngines
            // +0.05 (computation aids canonisation).
            ToolKind::LongRangeCommunication => Real::percent(15),
            ToolKind::MassLiteracy => Real::percent(15),
            ToolKind::AnalyticalEngines => Real::percent(5),
            // Tier-5: InformationNetworking +0.15 (the spec's
            // "+0.15 networking" headline); DigitalComputation
            // +0.10 (lossless storage + retrieval).
            ToolKind::InformationNetworking => Real::percent(15),
            ToolKind::DigitalComputation => Real::percent(10),
            // Tier-2 sensorium: RemoteAcoustic +0.05 — long-range
            // acoustic signalling is the pre-EM telegraphy
            // (drum / horn networks).
            ToolKind::RemoteAcoustic => Real::percent(5),
            // DistributedNetworks: relay-station news lifts
            // transmission fidelity across the polity.
            ToolKind::DistributedNetworks => Real::percent(8),
            // CodexTradition: bound volumes are the durable
            // long-distance transmission medium. Sits between
            // CulturalEncoding (+10) and LongRangeCommunication
            // (+15) — codices are the pre-radio extreme of
            // recorded transmission.
            ToolKind::CodexTradition => Real::percent(12),
            _ => Real::ZERO,
        }
    }

    /// Tool's additive contribution to per-tick discovery rate
    /// (multiplies the hypothesizer's fit cadence). Tools that
    /// formalise reasoning, accelerate computation, or canonise
    /// records lift the rate at which candidate relations propose
    /// and confirm. Folded multiplicatively as `(1 + Σbonus)` into
    /// `attempt_period` scheduling so a civ with `+0.50` aggregate
    /// runs ~1.5× more candidate-fit attempts per unit time.
    #[allow(clippy::match_same_arms)]
    pub fn discovery_rate_bonus(self) -> Real {
        match self {
            // Tier-2: ExperimentApparatus tightens the
            // observation-to-confirmation loop (controlled
            // conditions yield cleaner samples per attempt).
            ToolKind::ExperimentApparatus => Real::percent(10),
            // Tier-3: PrecisionTimekeeping synchronises
            // measurement; WrittenJurisprudence canonises
            // findings; AbstractMathematics lets formal
            // reasoning narrow candidate forms faster.
            ToolKind::PrecisionTimekeeping => Real::percent(5),
            ToolKind::WrittenJurisprudence => Real::percent(5),
            ToolKind::AbstractMathematics => Real::percent(10),
            // Tier-4: AnalyticalEngines is the headline pre-digital
            // computation jump; MassLiteracy widens the contributor
            // pool.
            ToolKind::AnalyticalEngines => Real::percent(15),
            ToolKind::MassLiteracy => Real::percent(5),
            // Tier-5: DigitalComputation is the headline +0.20;
            // InformationNetworking +0.10 (cross-civ idea exchange);
            // GeneticManipulation +0.05 (life-science instrumentation).
            ToolKind::DigitalComputation => Real::percent(20),
            ToolKind::InformationNetworking => Real::percent(10),
            ToolKind::GeneticManipulation => Real::percent(5),
            // Tier-5 transcendence: BioelectricResonator +0.10
            // — direct-from-physiology measurement is a
            // life-sciences instrumentation jump.
            ToolKind::BioelectricResonator => Real::percent(10),
            // Sensorium-extending tools accelerate discovery in
            // their domain by widening the perceivable channel set:
            // DistanceImaging +0.05 (telescopes/microscopes),
            // RemoteAcoustic +0.03 (sonar/echolocation),
            // ThermalSensor +0.05, FieldSensor +0.05,
            // MagneticSensor +0.05.
            ToolKind::DistanceImaging => Real::percent(5),
            ToolKind::RemoteAcoustic => Real::percent(3),
            ToolKind::ThermalSensor => Real::percent(5),
            ToolKind::FieldSensor => Real::percent(5),
            ToolKind::MagneticSensor => Real::percent(5),
            // PrecisionInstruments: headline tier-4 discovery
            // booster — clean measurement is the scientific-
            // revolution lever. Stacks alongside AnalyticalEngines'
            // computational lift so a civ that builds both gets
            // ~+0.30 aggregate.
            ToolKind::PrecisionInstruments => Real::percent(15),
            // CodexTradition: cross-generation knowledge
            // accumulation accelerates law-fitting (each generation
            // reads its predecessors' results).
            ToolKind::CodexTradition => Real::percent(8),
            _ => Real::ZERO,
        }
    }

    /// Tool's additive contribution to civ cohesion equilibrium.
    /// Folded into `update_cohesion`'s `target` term so the civ's
    /// cohesion drifts toward a higher floor — pushing the
    /// civil-war / breakaway thresholds farther away. Tools that
    /// bind the polity together (canonised law, shared symbology,
    /// network identity) contribute. Capped on the consumer side
    /// when summing into the equilibrium formula.
    #[allow(clippy::match_same_arms)]
    pub fn cohesion_bonus(self) -> Real {
        match self {
            // Tier-2: TradeNetworks (economic interdependence);
            // CulturalEncoding (canonical narrative);
            // UrbanConstruction (settled centres anchor identity).
            ToolKind::TradeNetworks => Real::percent(5),
            ToolKind::CulturalEncoding => Real::percent(5),
            ToolKind::UrbanConstruction => Real::percent(5),
            // Tier-3: WrittenJurisprudence is the headline +0.10
            // (legal codes bind a polity); DefensiveFortification
            // +0.05 (shared defence as identity).
            ToolKind::WrittenJurisprudence => Real::percent(10),
            ToolKind::DefensiveFortification => Real::percent(5),
            // Tier-4: MassLiteracy +0.10 (shared symbology at scale).
            ToolKind::MassLiteracy => Real::percent(10),
            // Tier-5: InformationNetworking +0.10 (network identity
            // — the modern "imagined community" lifter).
            ToolKind::InformationNetworking => Real::percent(10),
            // AcousticEngineering: synchronised civic signalling
            // (bells, horns, public address) coordinates a polity
            // larger than face-to-face range, lifting cohesion at
            // tier-3.
            ToolKind::AcousticEngineering => Real::percent(7),
            // DistributedNetworks: the polity-binding tool — late
            // tier-4 cohesion lift that doesn't depend on the
            // industrial chemistry chain.
            ToolKind::DistributedNetworks => Real::percent(12),
            // HydraulicWorks: shared infrastructure as identity
            // (the irrigation canal everyone tends, the aqueduct
            // every district drinks from).
            ToolKind::HydraulicWorks => Real::percent(5),
            _ => Real::ZERO,
        }
    }

    /// Tool's additive contribution to intra-civ migration rate
    /// (per-tick fraction of fertile adults moving between claimed
    /// cells under demographic pressure). Distinct from
    /// `expansion_rate_bonus` — that scales BFS frontier growth;
    /// this scales how fast existing populations redistribute under
    /// gradients. Folded into `migrate_inter_cell` as a multiplier
    /// `(1 + Σbonus)` on the base 5%-per-tick rate.
    #[allow(clippy::match_same_arms)]
    pub fn migration_speed_bonus(self) -> Real {
        match self {
            // Tier-2: WatercraftConstruction opens river / coast
            // movement.
            ToolKind::WatercraftConstruction => Real::percent(5),
            // Tier-3: MotivePropulsion (sails / animal traction);
            // LongRangeNavigation (knowing where to go, not just
            // how to get there).
            ToolKind::MotivePropulsion => Real::percent(10),
            ToolKind::LongRangeNavigation => Real::percent(5),
            // Tier-4: HeavyTransport + AerialTransport are the
            // mechanised-mobility pair; LongRangeCommunication
            // adds coordination (refugees know where capacity exists).
            ToolKind::HeavyTransport => Real::percent(20),
            ToolKind::AerialTransport => Real::percent(20),
            ToolKind::LongRangeCommunication => Real::percent(5),
            // Tier-5: AutonomousSystems +0.10 (logistics
            // automation); InformationNetworking +0.05
            // (real-time pressure / opportunity dissemination).
            ToolKind::AutonomousSystems => Real::percent(10),
            ToolKind::InformationNetworking => Real::percent(5),
            // Tier-5 transcendence: FieldPropulsionEngine +0.30
            // — once reactionless propulsion lands, intra-planetary
            // movement is bottleneck-free and migrations realise
            // at near-instantaneous speeds.
            ToolKind::FieldPropulsionEngine => Real::percent(30),
            // AmphibiousConstruction +0.05 — built bridges /
            // floating platforms speed migration across mixed
            // terrain (the habitat-lift in `can_claim_glyph` is
            // the headline mechanical effect).
            ToolKind::AmphibiousConstruction => Real::percent(5),
            _ => Real::ZERO,
        }
    }

    /// Tool's additive contribution to the per-tick birth-rate
    /// multiplier. The biological birth_rate (`clutch_size /
    /// fertile_window_months`) is multiplied by `(1 + Σbonus)` so
    /// nutritional + medical tools that improve maternal-foetal
    /// outcomes lift effective fertility. Distinct from
    /// `mortality_reduction_per_bracket[0]` (infant deaths *after*
    /// birth) — this is the conception-through-viable-birth gate.
    #[allow(clippy::match_same_arms)]
    pub fn fertility_bonus(self) -> Real {
        match self {
            // Tier-1: FoodProcessing improves nutritional density —
            // better-nourished fertile adults conceive more often.
            ToolKind::FoodProcessing => Real::percent(5),
            // Tier-2: BulkCultivation (food security); BulkStorage
            // (nutritional consistency through lean seasons);
            // BasicHealing (reduces miscarriage / improves
            // maternal health).
            ToolKind::BulkCultivation => Real::percent(5),
            ToolKind::BulkStorage => Real::percent(3),
            ToolKind::BasicHealing => Real::percent(5),
            // Tier-4: MedicalIntervention (modern obstetrics).
            ToolKind::MedicalIntervention => Real::percent(10),
            // Tier-5: AdvancedMedicine (assisted reproduction,
            // NICUs); GeneticManipulation (fertility treatment).
            ToolKind::AdvancedMedicine => Real::percent(10),
            ToolKind::GeneticManipulation => Real::percent(5),
            // AnimalHusbandry: dietary protein from herd milk +
            // meat improves fertile-bracket conception rates.
            ToolKind::AnimalHusbandry => Real::percent(4),
            // PreservedFood: consistent year-round nutrition
            // (no hungry-spring gap depressing fertility).
            ToolKind::PreservedFood => Real::percent(3),
            // HerbalMedicine: small obstetric improvements
            // (post-partum hygiene, fever tea).
            ToolKind::HerbalMedicine => Real::percent(3),
            _ => Real::ZERO,
        }
    }
}
