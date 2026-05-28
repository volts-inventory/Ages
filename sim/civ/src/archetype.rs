//! Civilizational-archetype classification.
//!
//! A *lever* is the foundational resource/science a developing
//! civilization organizes around. The five originally-authored
//! attractors (combustion, field/resonance, biochemical, cryogenic,
//! mechanical) plus the broader set (hydraulic, exotic-chemistry,
//! plasma/EM, gravitational, photonic, nuclear) are scored as **peer
//! dimensions** — none is privileged, none is a fallback — from the
//! same emergent signals: world profile, species sensorium, cognition
//! topology, and (later, via [`refine_with_run`]) the discovery
//! channels a civ confirms relations on and the tool clusters it
//! unlocks.
//!
//! The classifier is *open*: a run with one dominant lever is a pure
//! archetype; two co-dominant levers read as a named hybrid; a novel
//! mix with no clear winner is surfaced as an emergent, signature-
//! named archetype — the same philosophy the engine already uses for
//! emergent recognition templates and dynamic tools. So paths nobody
//! authored can still be detected and reported.
//!
//! Determinism: pure `Real` arithmetic, no RNG, no system time;
//! lever ties break by `Lever::ALL` order so labels are stable across
//! replays.

use crate::discovery::Channel;
use crate::tech::ToolKind;
use sim_arith::Real;
use sim_species::{CognitionTopology, ModalityKind, Species};
use sim_world::{Atmosphere, BiosphereClass, Crust, Magnetosphere, MetabolicSubstrate, Planet};

/// A foundational lever. Peer dimensions — scored identically, with
/// no default and no fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Lever {
    Combustion,
    FieldResonance,
    Biochemical,
    Cryogenic,
    Mechanical,
    Hydraulic,
    ExoticChemistry,
    PlasmaEm,
    Gravitational,
    Photonic,
    Nuclear,
}

impl Lever {
    /// Canonical order. Doubles as the deterministic tiebreak when two
    /// levers score equal.
    pub const ALL: [Lever; 11] = [
        Lever::Combustion,
        Lever::FieldResonance,
        Lever::Biochemical,
        Lever::Cryogenic,
        Lever::Mechanical,
        Lever::Hydraulic,
        Lever::ExoticChemistry,
        Lever::PlasmaEm,
        Lever::Gravitational,
        Lever::Photonic,
        Lever::Nuclear,
    ];

    pub fn idx(self) -> usize {
        Self::ALL.iter().position(|&l| l == self).unwrap()
    }

    pub fn name(self) -> &'static str {
        match self {
            Lever::Combustion => "combustion",
            Lever::FieldResonance => "field_resonance",
            Lever::Biochemical => "biochemical",
            Lever::Cryogenic => "cryogenic",
            Lever::Mechanical => "mechanical",
            Lever::Hydraulic => "hydraulic",
            Lever::ExoticChemistry => "exotic_chemistry",
            Lever::PlasmaEm => "plasma_em",
            Lever::Gravitational => "gravitational",
            Lever::Photonic => "photonic",
            Lever::Nuclear => "nuclear",
        }
    }
}

/// Overlay cognition mode. Orthogonal to the resource lever — a
/// collective or substrate-distributed mind can sit on *any* lever,
/// which is why the framework-bending "collective-intelligence" and
/// "information/substrate" paths are modelled as an overlay rather
/// than competing levers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CognitionMode {
    /// One integrated mind per individual (vertebrate/cephalopod).
    Individual,
    /// Eusocial hive — the colony is the cognitive unit.
    Collective,
    /// Substrate-distributed (slime-mold/acentric); the medium is the
    /// memory. The literal "information/substrate" overlay.
    SubstrateDistributed,
}

impl CognitionMode {
    pub fn name(self) -> &'static str {
        match self {
            CognitionMode::Individual => "individual",
            CognitionMode::Collective => "collective",
            CognitionMode::SubstrateDistributed => "substrate_distributed",
        }
    }
}

/// Per-lever score vector. Index aligns with [`Lever::ALL`]. Scores
/// are clamped to `[0, 1]`; they need not sum to one.
#[derive(Debug, Clone, Copy)]
pub struct LeverScores {
    pub scores: [Real; 11],
}

impl LeverScores {
    fn zero() -> Self {
        Self {
            scores: [Real::ZERO; 11],
        }
    }

    fn add(&mut self, lever: Lever, amount: Real) {
        let i = lever.idx();
        self.scores[i] = (self.scores[i] + amount).min(Real::ONE).max(Real::ZERO);
    }

    pub fn get(&self, lever: Lever) -> Real {
        self.scores[lever.idx()]
    }

    /// Levers sorted by score descending; ties broken by
    /// [`Lever::ALL`] order for determinism.
    pub fn ranked(&self) -> Vec<(Lever, Real)> {
        let mut v: Vec<(Lever, Real)> = Lever::ALL.iter().map(|&l| (l, self.get(l))).collect();
        v.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(core::cmp::Ordering::Equal)
                .then_with(|| a.0.idx().cmp(&b.0.idx()))
        });
        v
    }
}

/// The open classification of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchetypeLabel {
    /// One lever clearly dominates.
    Pure(Lever),
    /// Two co-dominant levers — a named hybrid (ordered by score,
    /// then `Lever::ALL`).
    Hybrid(Lever, Lever),
    /// No clear winner: a novel mix surfaced as an emergent archetype,
    /// carrying its top three signature dimensions.
    Emergent {
        dominant: Lever,
        secondary: Lever,
        tertiary: Lever,
    },
}

impl ArchetypeLabel {
    /// A human-readable name. Pure levers use their lever name; hybrids
    /// join the two; emergent archetypes are signature-named from their
    /// top dimensions (mirrors emergent-template / dynamic-tool naming).
    pub fn name(self) -> String {
        match self {
            ArchetypeLabel::Pure(l) => l.name().to_string(),
            ArchetypeLabel::Hybrid(a, b) => format!("{}/{}", a.name(), b.name()),
            ArchetypeLabel::Emergent {
                dominant,
                secondary,
                tertiary,
            } => format!(
                "emergent_{}_{}_{}", dominant.name(), secondary.name(), tertiary.name()
            ),
        }
    }
}

/// A run's full archetype profile: the score vector, the open label,
/// and the cognition overlay.
#[derive(Debug, Clone)]
pub struct ArchetypeProfile {
    pub scores: LeverScores,
    pub label: ArchetypeLabel,
    pub cognition: CognitionMode,
}

// Classification thresholds (tuned so the canonical fixtures land on
// their intended attractor; see tests). Expressed as Real ratios so
// the whole path stays fixed-point.
fn pure_floor() -> Real {
    Real::from_ratio(40, 100)
}
fn pure_margin() -> Real {
    Real::from_ratio(12, 100)
}
fn hybrid_floor() -> Real {
    Real::from_ratio(25, 100)
}

/// Classify a score vector into the open label space.
pub fn classify(scores: &LeverScores) -> ArchetypeLabel {
    let ranked = scores.ranked();
    let (top_lever, top) = ranked[0];
    let (second_lever, second) = ranked[1];
    let (third_lever, _third) = ranked[2];

    if top >= pure_floor() && (top - second) >= pure_margin() {
        ArchetypeLabel::Pure(top_lever)
    } else if top >= hybrid_floor() && second >= hybrid_floor() {
        ArchetypeLabel::Hybrid(top_lever, second_lever)
    } else {
        ArchetypeLabel::Emergent {
            dominant: top_lever,
            secondary: second_lever,
            tertiary: third_lever,
        }
    }
}

fn has_modality(species: &Species, kind: ModalityKind) -> bool {
    species.modalities.iter().any(|m| m.kind == kind)
}

fn has_field_sense(species: &Species) -> bool {
    has_modality(species, ModalityKind::ElectricField)
        || has_modality(species, ModalityKind::MagneticSense)
        || has_modality(species, ModalityKind::RadioNative)
}

/// Cognition overlay from the species' topology.
pub fn cognition_mode(species: &Species) -> CognitionMode {
    match species.cognition_topology {
        CognitionTopology::Collective => CognitionMode::Collective,
        CognitionTopology::Acentric => CognitionMode::SubstrateDistributed,
        CognitionTopology::Centralized | CognitionTopology::DistributedRedundant => {
            CognitionMode::Individual
        }
    }
}

/// Score every lever from the world + species prior (run start). The
/// realized archetype can later be refined by a civ's confirmed-
/// relation channels and unlocked tools via [`refine_with_run`].
pub fn score_world_species(planet: &Planet, species: &Species) -> LeverScores {
    let mut s = LeverScores::zero();
    let comp = &planet.crustal_composition;
    let oxidising = planet.atmosphere == Atmosphere::Oxidising;

    // --- Combustion: fire + oxidiser + accessible fuel.
    if oxidising {
        s.add(Lever::Combustion, Real::from_ratio(50, 100));
    }
    if planet.crust == Crust::Hydrocarbon {
        s.add(Lever::Combustion, Real::from_ratio(30, 100));
    }
    s.add(Lever::Combustion, comp.hydrocarbon.min(Real::from_ratio(30, 100)));

    // --- Field/resonance: piezoelectric crust + strong dipole + a
    // field-sensing biology (the resonance-field substrate is real on
    // exactly these worlds).
    if planet.crust == Crust::Piezoelectric {
        s.add(Lever::FieldResonance, Real::from_ratio(40, 100));
    }
    s.add(
        Lever::FieldResonance,
        (comp.piezoelectric * Real::from_ratio(60, 100)).min(Real::from_ratio(40, 100)),
    );
    match planet.magnetosphere {
        Magnetosphere::Strong => s.add(Lever::FieldResonance, Real::from_ratio(25, 100)),
        Magnetosphere::Weak => s.add(Lever::FieldResonance, Real::from_ratio(8, 100)),
        Magnetosphere::None => {}
    }
    if has_field_sense(species) {
        s.add(Lever::FieldResonance, Real::from_ratio(22, 100));
    }

    // --- Biochemical: life-dense, ore/fuel-poor, water world.
    match planet.biosphere {
        BiosphereClass::HyperBiodiverse => s.add(Lever::Biochemical, Real::from_ratio(45, 100)),
        BiosphereClass::Lush => s.add(Lever::Biochemical, Real::from_ratio(22, 100)),
        _ => {}
    }
    if planet.metabolic_substrate == MetabolicSubstrate::Aqueous {
        s.add(Lever::Biochemical, Real::from_ratio(12, 100));
    }
    // Ore- and fuel-poor crust is the biochemical signature (life is
    // the only easy lever).
    if comp.ferrous < Real::from_ratio(8, 100) && comp.hydrocarbon < Real::from_ratio(3, 100) {
        s.add(Lever::Biochemical, Real::from_ratio(20, 100));
    }

    // --- Cryogenic: cold solvent + meagre insolation + exotic-phase
    // / superconductor-friendly crust.
    match planet.metabolic_substrate {
        MetabolicSubstrate::Hydrocarbon => s.add(Lever::Cryogenic, Real::from_ratio(45, 100)),
        MetabolicSubstrate::Ammoniacal => s.add(Lever::Cryogenic, Real::from_ratio(40, 100)),
        _ => {}
    }
    // Weak, distant sunlight (< ~800 W/m²) is the energy-starved gift.
    if planet.stellar_luminosity < Real::from_int(800) {
        s.add(Lever::Cryogenic, Real::from_ratio(25, 100));
    }
    if planet.crust == Crust::RareEarth {
        s.add(Lever::Cryogenic, Real::from_ratio(12, 100));
    }

    // --- Mechanical: no fire (low-O2) + abundant kinetic energy.
    if !oxidising {
        s.add(Lever::Mechanical, Real::from_ratio(30, 100));
    }
    if planet.moon_count >= 2 {
        s.add(Lever::Mechanical, Real::from_ratio(18, 100));
    } else if planet.moon_count == 1 {
        s.add(Lever::Mechanical, Real::from_ratio(8, 100));
    }
    if planet.metabolic_substrate == MetabolicSubstrate::Aqueous {
        s.add(Lever::Mechanical, Real::from_ratio(8, 100));
    }

    // --- Hydraulic: water + pressure emphasis (a wet, dense-atmosphere
    // water world without a stronger competing lever).
    if planet.metabolic_substrate == MetabolicSubstrate::Aqueous
        && matches!(
            planet.atmosphere,
            Atmosphere::Oxidising | Atmosphere::Reducing | Atmosphere::Hazy
        )
    {
        s.add(Lever::Hydraulic, Real::from_ratio(22, 100));
    }
    s.add(
        Lever::Hydraulic,
        (planet.biosphere_density * Real::from_ratio(10, 100)).min(Real::from_ratio(10, 100)),
    );

    // --- Exotic-chemistry: non-oxidative reducing/hazy chemistry on a
    // non-water solvent that is not cold-dominated.
    if matches!(planet.atmosphere, Atmosphere::Reducing | Atmosphere::Hazy) {
        s.add(Lever::ExoticChemistry, Real::from_ratio(25, 100));
    }
    if planet.metabolic_substrate == MetabolicSubstrate::Silicate {
        s.add(Lever::ExoticChemistry, Real::from_ratio(25, 100));
    }

    // --- Plasma/EM: strong dipole + electrically active air, but NOT
    // the piezoelectric/field-sense resonance combination (that scores
    // FieldResonance instead).
    if planet.magnetosphere == Magnetosphere::Strong && planet.crust != Crust::Piezoelectric {
        s.add(Lever::PlasmaEm, Real::from_ratio(30, 100));
    }
    if matches!(planet.atmosphere, Atmosphere::Reducing | Atmosphere::Hazy) {
        s.add(Lever::PlasmaEm, Real::from_ratio(12, 100));
    }

    // --- Gravitational/tidal: many/large moons + high gravity.
    if planet.moon_count >= 3 {
        s.add(Lever::Gravitational, Real::from_ratio(30, 100));
    } else if planet.moon_count == 2 {
        s.add(Lever::Gravitational, Real::from_ratio(15, 100));
    }
    if planet.gravity() > Real::from_int(13) {
        s.add(Lever::Gravitational, Real::from_ratio(20, 100));
    }

    // --- Photonic: bright star + a light-sensing biology + clear air.
    if planet.stellar_luminosity > Real::from_int(1800) {
        s.add(Lever::Photonic, Real::from_ratio(32, 100));
    }
    if has_modality(species, ModalityKind::VisualLight) {
        s.add(Lever::Photonic, Real::from_ratio(20, 100));
    }
    if matches!(planet.atmosphere, Atmosphere::Thin | Atmosphere::Oxidising) {
        s.add(Lever::Photonic, Real::from_ratio(8, 100));
    }

    // --- Nuclear: radiogenic/exotic crust + an unshielded, radiation-
    // hardened niche.
    if planet.crust == Crust::RareEarth {
        s.add(Lever::Nuclear, Real::from_ratio(28, 100));
    }
    if planet.magnetosphere == Magnetosphere::None {
        s.add(Lever::Nuclear, Real::from_ratio(18, 100));
    }
    if species.tolerance.radiation_max > Real::from_int(2) {
        s.add(Lever::Nuclear, Real::from_ratio(18, 100));
    }

    s
}

/// Full run-start profile: prior scores + open label + cognition
/// overlay.
pub fn classify_world_species(planet: &Planet, species: &Species) -> ArchetypeProfile {
    let scores = score_world_species(planet, species);
    let label = classify(&scores);
    ArchetypeProfile {
        scores,
        label,
        cognition: cognition_mode(species),
    }
}

/// Primary lever a confirmed-relation channel points at. The channel a
/// civ actually fits laws over is direct evidence of the lever it is
/// developing.
fn channel_lever(channel: Channel) -> Option<Lever> {
    match channel {
        Channel::Fuel | Channel::Oxidiser | Channel::Fossil => Some(Lever::Combustion),
        Channel::ChargeMagnitude | Channel::Resonance => Some(Lever::FieldResonance),
        Channel::MagneticField => Some(Lever::PlasmaEm),
        Channel::WaterDepth | Channel::Vapour => Some(Lever::Hydraulic),
        Channel::Ice => Some(Lever::Cryogenic),
        Channel::Elevation => Some(Lever::Mechanical),
        // Temperature is thermodynamically neutral — every lever reads
        // it — so it points at no single archetype.
        Channel::Temperature => None,
    }
}

/// Primary lever a tool's unlock points at. A tool in a civ's roster
/// is strong evidence the civ climbed that lever's branch of the DAG.
fn tool_lever(tool: ToolKind) -> Option<Lever> {
    use ToolKind as T;
    match tool {
        T::LocalisedCombustion | T::MaterialRefining | T::ChemicalSynthesis
        | T::ChemicalProjectile | T::PowerGeneration => Some(Lever::Combustion),
        T::FieldSensor | T::MagneticSensor | T::BioelectricResonator
        | T::FieldPropulsionEngine | T::MetamaterialLattice => Some(Lever::FieldResonance),
        T::AnimalSymbiosis | T::BiomimeticDesign | T::GeneCultureCoevolution
        | T::GeneticManipulation | T::OrganicSynthesis | T::AdvancedMedicine
        | T::EcosystemEngineering => Some(Lever::Biochemical),
        T::CryogenicEngineering | T::EnergyStorage => Some(Lever::Cryogenic),
        T::MechanicalAdvantage | T::WindPower | T::PrecisionTimekeeping
        | T::AnalyticalEngines | T::Mechanisation | T::MotivePropulsion => Some(Lever::Mechanical),
        T::HydraulicWorks | T::FluidControl | T::WatercraftConstruction => Some(Lever::Hydraulic),
        T::DistanceImaging => Some(Lever::Photonic),
        _ => None,
    }
}

/// Refine the world+species prior with a civ's *realized* trajectory:
/// the discovery channels it has confirmed relations on (`(channel,
/// count)` pairs) and the tools it has unlocked. This is what turns
/// the prior into the realized archetype — the branches the civ
/// actually climbed, not just the ones its world made likely.
pub fn refine_with_run(
    prior: &LeverScores,
    confirmed_by_channel: &[(Channel, u32)],
    unlocked_tools: &[ToolKind],
) -> LeverScores {
    let mut s = *prior;

    // Confirmed relations: small per-relation weight, capped so a
    // flood on one channel can't peg a lever on its own.
    let per_relation = Real::from_ratio(2, 100);
    let channel_cap = Real::from_ratio(35, 100);
    for &(channel, count) in confirmed_by_channel {
        if let Some(lever) = channel_lever(channel) {
            let contribution = (per_relation * Real::from_int(i64::from(count))).min(channel_cap);
            s.add(lever, contribution);
        }
    }

    // Unlocked tools: a flat membership bonus per tool on its lever's
    // branch.
    let per_tool = Real::from_ratio(10, 100);
    for &tool in unlocked_tools {
        if let Some(lever) = tool_lever(tool) {
            s.add(lever, per_tool);
        }
    }

    s
}

/// Realized profile: refine the prior with run signals, then classify.
pub fn classify_realized(
    planet: &Planet,
    species: &Species,
    confirmed_by_channel: &[(Channel, u32)],
    unlocked_tools: &[ToolKind],
) -> ArchetypeProfile {
    let prior = score_world_species(planet, species);
    let scores = refine_with_run(&prior, confirmed_by_channel, unlocked_tools);
    let label = classify(&scores);
    ArchetypeProfile {
        scores,
        label,
        cognition: cognition_mode(species),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_recognition::RecognitionLibrary;
    use sim_world::sample_planet;

    fn profile_for_seed(seed: u64) -> (Planet, ArchetypeProfile) {
        let planet = sample_planet(seed);
        let lib = RecognitionLibrary::earth_like_default();
        let species = sim_species::derive(&planet, &lib);
        let profile = classify_world_species(&planet, &species);
        (planet, profile)
    }

    #[test]
    fn every_seed_classifies_without_panic_and_scores_bounded() {
        for seed in 0..40u64 {
            let (_planet, profile) = profile_for_seed(seed);
            for sc in profile.scores.scores {
                assert!(sc >= Real::ZERO && sc <= Real::ONE, "score out of [0,1]: {sc:?}");
            }
            // The label must reference levers that actually scored the
            // top of the ranked vector (internal consistency).
            let ranked = profile.scores.ranked();
            match profile.label {
                ArchetypeLabel::Pure(l) => assert_eq!(l, ranked[0].0),
                ArchetypeLabel::Hybrid(a, b) => {
                    assert_eq!(a, ranked[0].0);
                    assert_eq!(b, ranked[1].0);
                }
                ArchetypeLabel::Emergent {
                    dominant,
                    secondary,
                    tertiary,
                } => {
                    assert_eq!(dominant, ranked[0].0);
                    assert_eq!(secondary, ranked[1].0);
                    assert_eq!(tertiary, ranked[2].0);
                }
            }
        }
    }

    #[test]
    fn classification_is_deterministic() {
        let a = profile_for_seed(42).1;
        let b = profile_for_seed(42).1;
        assert_eq!(a.label, b.label);
        assert_eq!(a.scores.scores, b.scores.scores);
    }

    #[test]
    fn ranked_breaks_ties_by_lever_order() {
        // All-zero scores → ranked order is exactly Lever::ALL.
        let zero = LeverScores::zero();
        let ranked = zero.ranked();
        for (i, (lever, _)) in ranked.iter().enumerate() {
            assert_eq!(lever.idx(), i, "tie-break must follow Lever::ALL order");
        }
        // All-zero is the maximally-ambiguous case → Emergent.
        assert!(matches!(classify(&zero), ArchetypeLabel::Emergent { .. }));
    }

    #[test]
    fn pure_combustion_signature_labels_combustion() {
        // Hand-built score vector: combustion clearly dominant.
        let mut s = LeverScores::zero();
        s.add(Lever::Combustion, Real::from_ratio(80, 100));
        s.add(Lever::Hydraulic, Real::from_ratio(20, 100));
        assert_eq!(classify(&s), ArchetypeLabel::Pure(Lever::Combustion));
    }

    #[test]
    fn co_dominant_levers_label_hybrid() {
        let mut s = LeverScores::zero();
        s.add(Lever::FieldResonance, Real::from_ratio(45, 100));
        s.add(Lever::Biochemical, Real::from_ratio(42, 100));
        match classify(&s) {
            ArchetypeLabel::Hybrid(a, b) => {
                assert_eq!(a, Lever::FieldResonance);
                assert_eq!(b, Lever::Biochemical);
            }
            other => panic!("expected hybrid, got {other:?}"),
        }
    }

    #[test]
    fn hybrid_name_joins_levers() {
        let label = ArchetypeLabel::Hybrid(Lever::FieldResonance, Lever::Biochemical);
        assert_eq!(label.name(), "field_resonance/biochemical");
    }

    #[test]
    fn realized_refinement_can_override_a_flat_prior() {
        use crate::discovery::Channel;
        use crate::tech::ToolKind;
        // Start from a flat, ambiguous prior.
        let prior = LeverScores::zero();
        // A civ that confirmed many fuel/oxidiser relations and unlocked
        // the combustion lineage should read Combustion.
        let channels = [(Channel::Fuel, 12u32), (Channel::Oxidiser, 10u32)];
        let tools = [
            ToolKind::LocalisedCombustion,
            ToolKind::MaterialRefining,
            ToolKind::ChemicalSynthesis,
        ];
        let refined = refine_with_run(&prior, &channels, &tools);
        assert_eq!(classify(&refined), ArchetypeLabel::Pure(Lever::Combustion));
    }

    #[test]
    fn realized_refinement_reads_biochemical_from_bio_tools() {
        use crate::discovery::Channel;
        use crate::tech::ToolKind;
        let prior = LeverScores::zero();
        let channels: [(Channel, u32); 0] = [];
        let tools = [
            ToolKind::AnimalSymbiosis,
            ToolKind::BiomimeticDesign,
            ToolKind::GeneticManipulation,
            ToolKind::EcosystemEngineering,
            ToolKind::AdvancedMedicine,
        ];
        let refined = refine_with_run(&prior, &channels, &tools);
        assert_eq!(classify(&refined), ArchetypeLabel::Pure(Lever::Biochemical));
    }

    #[test]
    fn refinement_respects_score_ceiling() {
        let prior = LeverScores::zero();
        use crate::discovery::Channel;
        // A huge confirmed-relation count must stay clamped to <= 1.0.
        let channels = [(Channel::Fuel, 100_000u32)];
        let refined = refine_with_run(&prior, &channels, &[]);
        assert!(refined.get(Lever::Combustion) <= Real::ONE);
    }
}
