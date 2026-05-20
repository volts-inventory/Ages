//! `ToolKind::manipulation_prereqs` — body-plan modes a species
//! must possess for each tool. The largest single match in the
//! crate (~1000 lines); extracted from `specs.rs` so the other
//! specs methods stay readable.

use super::super::ToolKind;
use sim_species::ManipulationKind;

impl ToolKind {
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
        // Authoring principle: gates are biological-function-based,
        // not anthropocentric. If a real-world organism (extant or
        // plausible) achieves the tool's *function* through a given
        // body plan, the mode is accepted. Tier-1 applied knowledge
        // is broadly inclusive (animal-level tech doesn't demand a
        // specific morphology); tier-4 / tier-5 narrow toward the
        // body-plan-channel pairings that actually match each tool's
        // physical substrate (electrochemical, biochemical,
        // mechanical, etc.). Substrate divergence is preserved by
        // relation_prereqs (which laws the civ has fit), not by
        // body-plan exclusion.
        match self {
            // ─── tier-1: applied-knowledge / animal-level tech ───
            //
            // LocalisedCombustion: handling fire — placing a brand,
            // building a pyre, transporting embers. Most manipulation
            // modes qualify: ChemicalSecretion via pyrophoric secretion
            // (real-world bombardier-beetle chemistry takes you most
            // of the way), ElectricDischarge via arc ignition (Tesla-
            // coil / electric-eel sparking dry tinder), FluidJet via
            // jetted oxidiser / fuel-mix (flamethrower path).
            ToolKind::LocalisedCombustion => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::TonguePrehensile,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::ElectricDischarge,
                ManipulationKind::FluidJet,
            ],
            // ContactWeapon: melee predation — every predator-mode
            // manipulator qualifies. TonguePrehensile: prehensile-
            // tongue striking (chameleon-style melee). FluidJet:
            // close-range high-pressure cutting (real archerfish-
            // scale forces).
            ToolKind::ContactWeapon => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::TonguePrehensile,
                ManipulationKind::ToolExtension,
                ManipulationKind::WebConstruct,
                ManipulationKind::Burrow,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::ElectricDischarge,
                ManipulationKind::FluidJet,
            ],
            // RangedMomentumWeapon: throwing / spitting / spraying /
            // net-flinging / stun-at-range. ElectricDischarge: ranged
            // bioelectric stun (electric eels reach ~8 m; a fielded
            // species farther). TonguePrehensile: sticky-tongue strike
            // at range.
            ToolKind::RangedMomentumWeapon => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::TonguePrehensile,
                ManipulationKind::ToolExtension,
                ManipulationKind::FluidJet,
                ManipulationKind::WebConstruct,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::ElectricDischarge,
            ],
            // SimpleShelter: dwellings — burrows, web-nests,
            // secreted shells, leaned-stick lean-tos, packed mud,
            // tongue-built nests (a la weaverbirds done with
            // prehensile tongues instead of beaks).
            ToolKind::SimpleShelter => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::TonguePrehensile,
                ManipulationKind::ToolExtension,
                ManipulationKind::WebConstruct,
                ManipulationKind::Burrow,
                ManipulationKind::ChemicalSecretion,
            ],
            // FoodProcessing: butchering / chewing / external
            // digestion / fire-cooking / electric stun-and-render.
            // ElectricDischarge: electrocute prey then process —
            // a real predator strategy. WebConstruct: woven traps
            // + baskets for processing.
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
                ManipulationKind::WebConstruct,
                ManipulationKind::ElectricDischarge,
            ],
            // FluidGathering: carrying / channelling water. Burrow:
            // well-digging / aquifer access — burrowers are THE
            // canonical water-gatherers. Mandible: biting open
            // fluid-bearing fruit / stems.
            ToolKind::FluidGathering => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::TonguePrehensile,
                ManipulationKind::ToolExtension,
                ManipulationKind::FluidJet,
                ManipulationKind::WebConstruct,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::Burrow,
            ],
            // BasicTextiles: cordage / silk / weaving. WebConstruct
            // and ChemicalSecretion (silk-producing glands) are the
            // canonical body-plan paths; MouthBeak qualifies via
            // weaverbird / tailorbird nest-building (literal textile
            // work with a beak).
            ToolKind::BasicTextiles => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::TonguePrehensile,
                ManipulationKind::ToolExtension,
                ManipulationKind::WebConstruct,
                ManipulationKind::ChemicalSecretion,
            ],
            // StoneWorking: shaping stone — not just knapping, but
            // every functional substitute. ChemicalSecretion: acid-
            // boring (limpets / chitons literally bore rock with
            // secreted acid; piddock clams excavate stone homes).
            // Burrow: excavation IS shaping stone at scale.
            // ElectricDischarge: spark-erosion (a real industrial
            // machining technique). FluidJet: water-jet cutting (a
            // standard industrial process — pressurised water cuts
            // granite). WebConstruct: silk-bonded stone composites.
            ToolKind::StoneWorking => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::Burrow,
                ManipulationKind::ElectricDischarge,
                ManipulationKind::FluidJet,
                ManipulationKind::WebConstruct,
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
            // BasicHealing: herbal / pharmaceutical first aid /
            // bandaging / stun-cure rituals. ChemicalSecretion is
            // the natural strength (venom-bearing species are
            // already pharmacologists); WebConstruct: silk bandages
            // and splints; FluidJet: jetted-water wound irrigation;
            // Burrow: mud-pack / clay-bath traditions.
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
                ManipulationKind::WebConstruct,
                ManipulationKind::FluidJet,
                ManipulationKind::Burrow,
            ],

            // ─── tier-2: settlement-tier tools ───
            //
            // BulkCultivation: agriculture at scale. Mandible:
            // leafcutter-ant-style fungus farming. Burrow: subterranean
            // cultivation (also leafcutter ants — their farms ARE
            // burrows). MouthBeak: corvid / parrot seed-harvest
            // operations. WebConstruct: trellis / protective webbing.
            // ElectricDischarge: bioelectric pest control. FluidJet:
            // irrigation-by-jet.
            ToolKind::BulkCultivation => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::TonguePrehensile,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::WebConstruct,
                ManipulationKind::Burrow,
                ManipulationKind::FluidJet,
                ManipulationKind::ElectricDischarge,
            ],
            // AnimalSymbiosis: herding. Mandible: ants herding aphids
            // (extant!). WebConstruct: enclosed-ranching via web
            // barriers. Burrow: keeping symbionts in burrow systems.
            // FluidJet / TonguePrehensile: corralling at distance.
            ToolKind::AnimalSymbiosis => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::TonguePrehensile,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::ElectricDischarge,
                ManipulationKind::WebConstruct,
                ManipulationKind::Burrow,
                ManipulationKind::FluidJet,
            ],
            // BulkStorage: pottery / silos / chitin granaries /
            // woven baskets / cellars / cached food middens. Burrow:
            // THE canonical storage method for burrowing species
            // (food caches in tunnel networks). MouthBeak: cache /
            // larder behaviour (corvids, shrikes). ElectricDischarge:
            // electrified storage (pest-repellent fields).
            ToolKind::BulkStorage => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::TonguePrehensile,
                ManipulationKind::ToolExtension,
                ManipulationKind::WebConstruct,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::Burrow,
                ManipulationKind::ElectricDischarge,
            ],
            // MaterialRefining: smelting / metallurgy / refining.
            // ElectricDischarge: ELECTROLYTIC refining — the modern
            // path for aluminium and many other metals. Burrow:
            // pit-style bloomery furnaces (real pre-industrial
            // metallurgy was largely underground). ChemicalSecretion:
            // secreted flux + smelting agents. Mandible / WebConstruct:
            // bellows-and-crucible work.
            ToolKind::MaterialRefining => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::Mandible,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::ElectricDischarge,
                ManipulationKind::Burrow,
                ManipulationKind::WebConstruct,
            ],
            // CulturalEncoding: writing / mark-making. Any mode that
            // leaves persistent signals — scratches, pheromone trails,
            // bioelectric impressions, woven knot-records (real
            // Andean quipu), dug glyphs, jet-spray pigment.
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
                ManipulationKind::Burrow,
                ManipulationKind::FluidJet,
            ],
            // FluidControl: irrigation — ditches, dams, bored pipes,
            // jet-driven channels, secreted aqueducts, woven
            // filter-channels. Mandible: cutting channels. MouthBeak:
            // beak-bored irrigation. ElectricDischarge: electrokinetic
            // fluid handling.
            ToolKind::FluidControl => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::ToolExtension,
                ManipulationKind::FluidJet,
                ManipulationKind::Burrow,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::WebConstruct,
                ManipulationKind::ElectricDischarge,
            ],
            // WatercraftConstruction: hulls — shaped, woven, secreted,
            // hollowed-from-burrow. FluidJet: jet-driven craft (squid-
            // style propulsion built into the hull). Burrow: hollowed-
            // log canoes (excavated wood). ElectricDischarge: electric-
            // assisted hull-shaping.
            ToolKind::WatercraftConstruction => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::ToolExtension,
                ManipulationKind::WebConstruct,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::FluidJet,
                ManipulationKind::Burrow,
            ],
            // PermanentMasonry: stone construction. Burrow: stone-
            // tunnel architecture (real species do this at scale).
            // Mandible: ant / termite stone shaping. MouthBeak: beak-
            // shaped masonry. ChemicalSecretion: secreted mortar +
            // cement (coral reefs are functionally biological
            // masonry). ElectricDischarge: spark-erosion shaping.
            ToolKind::PermanentMasonry => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::Burrow,
                ManipulationKind::WebConstruct,
                ManipulationKind::ElectricDischarge,
            ],
            // TradeNetworks: pure economic / social institution —
            // no manipulation gate.
            ToolKind::TradeNetworks => &[],
            // UrbanConstruction: city-scale building. Mandible:
            // termite-mound urbanism (literal cities engineered for
            // climate control). MouthBeak: rookery / colonial nest
            // urbanism. Most modes qualify.
            ToolKind::UrbanConstruction => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::ToolExtension,
                ManipulationKind::WebConstruct,
                ManipulationKind::Burrow,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::FluidJet,
                ManipulationKind::ElectricDischarge,
            ],

            // ─── sensorium instruments (tier 2 / 3 / 4) ───
            //
            // The earlier framing ("sub-mm machining only") was
            // anthropocentric — every channel a sensorium tool reads
            // has a biological-sensor analogue, so each instrument
            // has more body-plan paths than the manufactured form
            // suggests.
            //
            // ThermalSensor: thermochromic biological substrates
            // (real example: leaf-mantis pigments shift with temp);
            // pit-viper-style IR (Mandible / MouthBeak insectoid
            // sensilla); electroreceptor temperature dependence
            // (ElectricDischarge).
            ToolKind::ThermalSensor => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::ElectricDischarge,
            ],
            // RemoteAcoustic: WebConstruct is THE acoustic sensor —
            // spiderwebs are calibrated vibration arrays. FluidJet:
            // lateral-line-style flow sensing (real fish biology).
            // ChemicalSecretion: tympanic-membrane secretion (insect
            // and amphibian ears).
            ToolKind::RemoteAcoustic => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::ToolExtension,
                ManipulationKind::WebConstruct,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::FluidJet,
            ],
            // DistanceImaging: ChemicalSecretion can produce
            // biological lenses (copepod eyes form through sequential
            // secretion). Compound-eye morphologies (Mandible /
            // MouthBeak). Bioluminescent imaging arrays.
            ToolKind::DistanceImaging => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
            ],
            // FieldSensor: ElectricDischarge species are field
            // sensors *natively* — building externalised sensors is
            // a near-trivial extension of their own organs.
            // ChemicalSecretion: chemoreceptive arrays.
            ToolKind::FieldSensor => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::ToolExtension,
                ManipulationKind::ElectricDischarge,
                ManipulationKind::ChemicalSecretion,
            ],
            // MagneticSensor: magnetotactic bacteria literally secrete
            // magnetite crystals — ChemicalSecretion is one of the
            // canonical paths in nature. ElectricDischarge: field-
            // sensing extends naturally to magnetic.
            ToolKind::MagneticSensor => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::ElectricDischarge,
            ],
            // AmphibiousConstruction: cross-domain habitats. Most
            // manipulation modes qualify — terrestrial-aquatic
            // engineering is well-served by every body plan.
            ToolKind::AmphibiousConstruction => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::ToolExtension,
                ManipulationKind::WebConstruct,
                ManipulationKind::Burrow,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::FluidJet,
                ManipulationKind::ElectricDischarge,
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
            // ChemicalProjectile: gunpowder weaponry / chemical
            // projectiles. FluidJet: high-pressure chemical-spray
            // weaponry (real bombardier-beetle precedent). MouthBeak /
            // Mandible: grenade-launching grip. ElectricDischarge:
            // railgun-analogue electromagnetic launcher. WebConstruct:
            // sling / web-launcher.
            ToolKind::ChemicalProjectile => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::FluidJet,
                ManipulationKind::ElectricDischarge,
                ManipulationKind::WebConstruct,
            ],
            // PrecisionTimekeeping: clocks. ChemicalSecretion:
            // chemical oscillators (Belousov-Zhabotinsky reactions
            // are literal chemical clocks). ElectricDischarge:
            // bioelectric circadian-mechanism externalisation.
            // WebConstruct: pendulum / tension-resonance clocks.
            ToolKind::PrecisionTimekeeping => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::ElectricDischarge,
                ManipulationKind::WebConstruct,
            ],
            // MechanicalAdvantage: levers / pulleys / wheels.
            // Mandible: leafcutter-ant mandibles already operate at
            // extreme mechanical advantage. FluidJet: hydraulic
            // multiplication. WebConstruct: pulley + tension systems
            // (silk has exceptional strength-to-weight). Burrow:
            // wedge-and-ramp earthwork mechanics. ChemicalSecretion:
            // hydraulic substrates. ElectricDischarge: electromechanical.
            ToolKind::MechanicalAdvantage => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::ToolExtension,
                ManipulationKind::FluidJet,
                ManipulationKind::WebConstruct,
                ManipulationKind::Burrow,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::ElectricDischarge,
            ],
            // LongRangeNavigation: instruments + charts. ElectricDischarge:
            // bioelectric compass (real species use the Earth's
            // magnetic field for navigation). ChemicalSecretion:
            // pheromone-trail navigation (migratory species follow
            // chemical gradients across continents).
            ToolKind::LongRangeNavigation => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::ToolExtension,
                ManipulationKind::ElectricDischarge,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::WebConstruct,
            ],
            // WrittenJurisprudence + AbstractMathematics: notation-
            // bound — same broad palette as CulturalEncoding.
            ToolKind::WrittenJurisprudence | ToolKind::AbstractMathematics => &[
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
                ManipulationKind::Burrow,
                ManipulationKind::FluidJet,
            ],
            // ArtisanalSpecialisation: crafts — every manipulation
            // mode supports specialised craft traditions.
            ToolKind::ArtisanalSpecialisation => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::TonguePrehensile,
                ManipulationKind::ToolExtension,
                ManipulationKind::WebConstruct,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::Burrow,
                ManipulationKind::FluidJet,
                ManipulationKind::ElectricDischarge,
            ],
            // DefensiveFortification: large earthworks / walls.
            // Mandible: termite-mound forts (genuinely impressive
            // fortifications in nature). WebConstruct: defensive
            // web barriers (real spider colony defence).
            ToolKind::DefensiveFortification => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::ToolExtension,
                ManipulationKind::Burrow,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::WebConstruct,
                ManipulationKind::ElectricDischarge,
            ],
            // MotivePropulsion: sails / wheels / paddles / jets.
            // FluidJet is the canonical jet-propulsion species
            // (squid, octopus, salp) — they ARE motive propulsion.
            // ChemicalSecretion: chemical-rocket / pyrophoric
            // propulsion. ElectricDischarge: ion-drive analogues /
            // electric motor predecessors.
            ToolKind::MotivePropulsion => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::ToolExtension,
                ManipulationKind::FluidJet,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::ElectricDischarge,
                ManipulationKind::WebConstruct,
            ],

            // ─── tier-4: industrial ───
            //
            // Industrial tools narrow toward demanding fabrication
            // but each has multiple biological-substrate paths.
            // The principle: any manipulation mode that achieves the
            // tool's function through real-or-plausible biology
            // qualifies, regardless of whether it looks like the
            // Earth-anthropocentric route.
            //
            // Mechanisation: ElectricDischarge → electric motors
            // (the canonical 19th-c industrial driver). FluidJet →
            // hydraulic / pneumatic machines. ChemicalSecretion →
            // bio-machine bulk fabrication. WebConstruct → loom-and-
            // textile mass production. Burrow → subterranean
            // factory architecture.
            ToolKind::Mechanisation => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::ToolExtension,
                ManipulationKind::ElectricDischarge,
                ManipulationKind::FluidJet,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::WebConstruct,
                ManipulationKind::Burrow,
            ],
            // LongRangeCommunication: ChemicalSecretion → pheromone
            // networks (ant supercolonies coordinate across kilometres
            // chemically — this is literal long-range chemical
            // communication). WebConstruct → vibration-network signalling
            // through a structured web (spider colonies, real example).
            // ElectricDischarge → EM telegraphy / radio.
            ToolKind::LongRangeCommunication => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::ToolExtension,
                ManipulationKind::ElectricDischarge,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::WebConstruct,
            ],
            // ChemicalSynthesis: ElectricDischarge → electrolytic
            // synthesis (the Hall-Héroult process is literally this).
            // FluidJet → high-pressure reactor work.
            ToolKind::ChemicalSynthesis => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::ElectricDischarge,
                ManipulationKind::FluidJet,
            ],
            // MedicalIntervention: ElectricDischarge → bioelectric
            // medicine (defibrillation, deep-brain stimulation, vagus-
            // nerve therapy). WebConstruct → suture / surgical-mesh
            // fabrication.
            ToolKind::MedicalIntervention => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::TonguePrehensile,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::ElectricDischarge,
                ManipulationKind::WebConstruct,
            ],
            // AdvancedMaterials: ChemicalSecretion is THE biological
            // advanced-materials path — spider silk is stronger than
            // steel by weight; nacre is a calcium-carbonate composite
            // with crack-deflection geometry; diatom frustules are
            // engineered silica nanostructures. WebConstruct: silk-
            // composite metamaterials. ElectricDischarge: electrodeposited
            // alloys (anodised metals, electroformed shapes).
            ToolKind::AdvancedMaterials => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::WebConstruct,
                ManipulationKind::ElectricDischarge,
            ],
            // HeavyTransport: FluidJet → water-jet propulsion of
            // bulk transport. Burrow → tunnel-rail transport (subway
            // analogues, very real). WebConstruct → cargo-nets +
            // suspended-cable transport. ChemicalSecretion → chemical-
            // fuel-driven heavy vehicles.
            ToolKind::HeavyTransport => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::ToolExtension,
                ManipulationKind::FluidJet,
                ManipulationKind::Burrow,
                ManipulationKind::WebConstruct,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::ElectricDischarge,
            ],
            // PowerGeneration: ChemicalSecretion → biological fuel
            // cells / fermentation power (mitochondria are this; ATP
            // production is literal chemical power generation).
            // FluidJet → hydropower / pneumatic. WebConstruct → kite /
            // sail / wind-catcher arrays. ElectricDischarge species
            // are themselves walking power generators (electric eels).
            ToolKind::PowerGeneration => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::ToolExtension,
                ManipulationKind::ElectricDischarge,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::FluidJet,
                ManipulationKind::WebConstruct,
            ],
            // AnalyticalEngines: ChemicalSecretion → chemical
            // computing (slime-mold-style; real research-grade
            // computation has been done in BZ-reaction substrates).
            // ElectricDischarge → relay / vacuum-tube / transistor
            // logic. WebConstruct → distributed-network computation
            // (the connectome of a colony IS the analytical engine).
            ToolKind::AnalyticalEngines => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::ElectricDischarge,
                ManipulationKind::WebConstruct,
            ],
            // MassLiteracy: notation infrastructure at population
            // scale — WebConstruct (knot-records / quipu), Burrow
            // (carved-tunnel libraries), FluidJet (spray-stencil
            // printing analogues).
            ToolKind::MassLiteracy => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::TonguePrehensile,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::ElectricDischarge,
                ManipulationKind::WebConstruct,
                ManipulationKind::Burrow,
                ManipulationKind::FluidJet,
            ],
            // AerialTransport: ChemicalSecretion → biological gas-
            // bladders / hydrogen-generating bacteria (lighter-than-
            // air is a real biological path). WebConstruct → silk
            // ballooning (spiders genuinely float on engineered silk
            // for hundreds of kilometres). FluidJet → pressure-driven
            // flight (real squid achieve brief flight via jet).
            // ElectricDischarge → ion-propulsion analogues.
            ToolKind::AerialTransport => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::WebConstruct,
                ManipulationKind::FluidJet,
                ManipulationKind::ElectricDischarge,
            ],

            // ─── tier-5: information-age + transcendence trio ───
            //
            // Each tier-5 tool has multiple body-plan paths that
            // match its physical substrate. ToolExtension is the
            // universal manufactured route; ChemicalSecretion is the
            // biochemistry route; ElectricDischarge is the
            // electromagnetic route; WebConstruct is the structured-
            // material / distributed-architecture route.
            //
            // BioelectricResonator: native field-organ engineering.
            // ChemicalSecretion qualifies because bioelectric organs
            // are themselves built from secreted electrochemical
            // tissue (electroplaques in electric eels).
            ToolKind::BioelectricResonator => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::ToolExtension,
                ManipulationKind::ElectricDischarge,
                ManipulationKind::ChemicalSecretion,
            ],
            // FieldPropulsionEngine: a field-coupling species would
            // naturally engineer field-mediated propulsion as an
            // extension of its body organs.
            ToolKind::FieldPropulsionEngine => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::ToolExtension,
                ManipulationKind::ElectricDischarge,
            ],
            // MetamaterialLattice: WebConstruct → spider silk is
            // already a metamaterial (photonic-crystal properties
            // documented in real arachnid biology). ChemicalSecretion
            // → nacre / diatom frustules / butterfly-wing photonic
            // structures are all biological metamaterials.
            ToolKind::MetamaterialLattice => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::ToolExtension,
                ManipulationKind::WebConstruct,
                ManipulationKind::ChemicalSecretion,
            ],
            // DigitalComputation: ElectricDischarge → solid-state
            // electronics (the canonical path). ChemicalSecretion →
            // chemical / molecular computing. WebConstruct →
            // distributed-network architectures.
            ToolKind::DigitalComputation => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::ToolExtension,
                ManipulationKind::ElectricDischarge,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::WebConstruct,
            ],
            // InformationNetworking: ChemicalSecretion → planet-scale
            // pheromone networks (ant supercolonies span continents
            // and coordinate via secreted chemicals). WebConstruct →
            // literal networked vibration / signalling architectures.
            ToolKind::InformationNetworking => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::ToolExtension,
                ManipulationKind::ElectricDischarge,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::WebConstruct,
            ],
            // GeneticManipulation: ChemicalSecretion is the obvious
            // biochemistry-native path; ElectricDischarge → bioelectric
            // gene-therapy / electroporation; WebConstruct → silk-
            // mediated gene-delivery scaffolds (real research area).
            ToolKind::GeneticManipulation => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::ElectricDischarge,
                ManipulationKind::WebConstruct,
            ],
            // OrbitalReach: ChemicalSecretion → chemical rocketry via
            // secreted propellants. FluidJet → pressure-driven launch.
            // ElectricDischarge → ion propulsion. The substrate gate
            // (tool prereqs through AerialTransport / MaterialRefining)
            // still enforces the combustion-locked story; the
            // manipulation gate just doesn't add a separate barrier.
            ToolKind::OrbitalReach => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::FluidJet,
                ManipulationKind::ElectricDischarge,
            ],
            // AdvancedMedicine: ChemicalSecretion-native (the
            // species' own chemistry is medicine), plus ElectricDischarge
            // for bioelectric therapy and WebConstruct for tissue-
            // scaffold / silk-suture / regenerative-mesh techniques
            // (real biomedical applications of spider silk).
            ToolKind::AdvancedMedicine => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::TonguePrehensile,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::ElectricDischarge,
                ManipulationKind::WebConstruct,
            ],
            // MaterialFabrication: ChemicalSecretion → biological
            // additive manufacturing (spider silk extrusion is
            // literal additive printing of a metamaterial fibre).
            // WebConstruct → precision-spinning fabrication.
            // ElectricDischarge → electron-beam / electrochemical
            // deposition fabrication.
            ToolKind::MaterialFabrication => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::WebConstruct,
                ManipulationKind::ElectricDischarge,
            ],
            // AutonomousSystems: ChemicalSecretion → self-regulating
            // chemical systems (termite-mound climate control is an
            // autonomous biological system at city scale).
            // ElectricDischarge → neural / bioelectric autonomous
            // control. WebConstruct → collective-intelligence networks
            // (eusocial-colony decision-making is autonomous-system
            // engineering done biologically).
            ToolKind::AutonomousSystems => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::ElectricDischarge,
                ManipulationKind::WebConstruct,
            ],
            // EnergyStorage: ChemicalSecretion → chemical batteries
            // (every electrochemical cell is chemical-storage; ATP
            // is biological energy storage). ElectricDischarge native
            // path. WebConstruct → flywheel / spring-loaded mechanical
            // storage in silk-tension arrays.
            ToolKind::EnergyStorage => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::ToolExtension,
                ManipulationKind::ElectricDischarge,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::WebConstruct,
            ],
            // CryogenicEngineering: ChemicalSecretion → antifreeze
            // proteins (real biology — Arctic fish and insects secrete
            // them). FluidJet → cryogenic-fluid handling.
            // ElectricDischarge → magneto-caloric / thermoelectric
            // cooling.
            ToolKind::CryogenicEngineering => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::FluidJet,
                ManipulationKind::ElectricDischarge,
            ],
            // OrganicSynthesis: ChemicalSecretion is the substrate-
            // native path. ElectricDischarge → electrolytic organic
            // synthesis (an established branch of electrochemistry).
            ToolKind::OrganicSynthesis => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::ElectricDischarge,
            ],
            // HerbalMedicine: harvesting + processing plant matter.
            // Broad gate — any species that can grasp, ingest, or
            // chemically process plant tissue can develop an herbal
            // pharmacopoeia. ChemicalSecretion species brew via
            // their own secretions (digestive / metabolic). Burrowers
            // collect roots / fungi. Inclusive at tier-2 by design.
            ToolKind::HerbalMedicine => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::TonguePrehensile,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::Burrow,
                ManipulationKind::FluidJet,
            ],
            // AcousticEngineering: shaping acoustic chambers /
            // resonators. Needs precise built-form manipulation.
            // ToolExtension is the canonical match (chisels + plumb
            // lines on stone); LimbGrasp / Tentacle / Trunk all
            // qualify. WebConstruct species can weave resonant
            // membrane structures. Burrowers carve sound chambers.
            ToolKind::AcousticEngineering => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::ToolExtension,
                ManipulationKind::WebConstruct,
                ManipulationKind::Burrow,
            ],
            // AnimalHusbandry: gentle herding, handling, breeding
            // selection. Broad gate at tier-2 — any species that
            // can interact with other animals can build a herd
            // economy. ChemicalSecretion species use pheromones to
            // direct herds.
            ToolKind::AnimalHusbandry => &[
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
            // PreservedFood: cut, brine, dry, ferment. Broad
            // gate. ChemicalSecretion species literally secrete
            // pickling acids / fermentative biota.
            ToolKind::PreservedFood => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::MouthBeak,
                ManipulationKind::Mandible,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::FluidJet,
                ManipulationKind::Burrow,
            ],
            // BiomimeticDesign: detailed structural replication of
            // biological forms. ToolExtension + LimbGrasp /
            // Tentacle / Trunk handle the build; WebConstruct
            // species are *naturally* biomimetic engineers.
            ToolKind::BiomimeticDesign => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::ToolExtension,
                ManipulationKind::WebConstruct,
                ManipulationKind::Mandible,
            ],
            // HydraulicWorks: stone-cutting + aqueduct lining.
            // ToolExtension + the large-grasp body plans. Burrowers
            // dig channels.
            ToolKind::HydraulicWorks => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::ToolExtension,
                ManipulationKind::Burrow,
                ManipulationKind::FluidJet,
            ],
            // PrecisionInstruments: very fine manipulation. Narrow
            // gate. LimbGrasp / Tentacle / ToolExtension are the
            // calibrated-manipulation modes; WebConstruct can weave
            // calibrated structures. ChemicalSecretion can produce
            // narrow-gauge reagent dispensers.
            ToolKind::PrecisionInstruments => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::ToolExtension,
                ManipulationKind::WebConstruct,
                ManipulationKind::ChemicalSecretion,
            ],
            // DistributedNetworks: relay-station construction +
            // signal-handler training. ToolExtension + broad
            // grasping modes — same gate as TradeNetworks.
            ToolKind::DistributedNetworks => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::ElectricDischarge,
            ],
            // WindPower: sail rigging / mill construction. Broad
            // mid-tier gate. WebConstruct species rig
            // membrane-and-cord wind-catchers natively.
            ToolKind::WindPower => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::ToolExtension,
                ManipulationKind::WebConstruct,
                ManipulationKind::Mandible,
            ],
            // CodexTradition: bookbinding / codex fabrication —
            // requires precise built-form construction. Same
            // narrow gate as PrecisionInstruments.
            ToolKind::CodexTradition => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::ToolExtension,
                ManipulationKind::WebConstruct,
                ManipulationKind::ChemicalSecretion,
            ],
            // GeneCultureCoevolution: directed breeding + formal
            // recordkeeping. Same gate as AnimalHusbandry but
            // tighter since formal selection theory needs the
            // record-keeping apparatus.
            ToolKind::GeneCultureCoevolution => &[
                ManipulationKind::LimbGrasp,
                ManipulationKind::Tentacle,
                ManipulationKind::Trunk,
                ManipulationKind::ToolExtension,
                ManipulationKind::ChemicalSecretion,
                ManipulationKind::MouthBeak,
            ],
        }
    }
}
