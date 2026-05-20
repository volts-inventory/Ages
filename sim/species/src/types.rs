//! Species-related data types: modalities + manipulation + habitat
//! topology + dynamic-tool records. The Species struct itself
//! lives in `species`; sampling helpers in `sampling`; the entry-
//! point `derive` in `derive`.

use sim_arith::Real;

/// 15 communication channels. Each modality carries per-channel
/// parameters and is gated on environment-presence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ModalityKind {
    AcousticAir,
    AcousticWater,
    Seismic,
    VisualLight,
    VisualPolarization,
    Bioluminescent,
    ChemicalPheromone,
    ChemicalTaste,
    Tactile,
    ElectricField,
    MagneticSense,
    InfraredThermal,
    RadioNative,
    Gestural,
    Postural,
}

impl ModalityKind {
    pub const ALL: [ModalityKind; 15] = [
        ModalityKind::AcousticAir,
        ModalityKind::AcousticWater,
        ModalityKind::Seismic,
        ModalityKind::VisualLight,
        ModalityKind::VisualPolarization,
        ModalityKind::Bioluminescent,
        ModalityKind::ChemicalPheromone,
        ModalityKind::ChemicalTaste,
        ModalityKind::Tactile,
        ModalityKind::ElectricField,
        ModalityKind::MagneticSense,
        ModalityKind::InfraredThermal,
        ModalityKind::RadioNative,
        ModalityKind::Gestural,
        ModalityKind::Postural,
    ];

    /// Convert to the recognition-side `ChannelKind` enum. The two
    /// enums share the same 15-variant axis; the duplication exists
    /// only to keep `sim/recognition` independent of `sim/species`
    /// (recognition is upstream). Match arms enumerated.
    #[allow(clippy::match_same_arms)]
    pub fn to_channel(self) -> sim_recognition::ChannelKind {
        use sim_recognition::ChannelKind as C;
        match self {
            ModalityKind::AcousticAir => C::AcousticAir,
            ModalityKind::AcousticWater => C::AcousticWater,
            ModalityKind::Seismic => C::Seismic,
            ModalityKind::VisualLight => C::VisualLight,
            ModalityKind::VisualPolarization => C::VisualPolarization,
            ModalityKind::Bioluminescent => C::Bioluminescent,
            ModalityKind::ChemicalPheromone => C::ChemicalPheromone,
            ModalityKind::ChemicalTaste => C::ChemicalTaste,
            ModalityKind::Tactile => C::Tactile,
            ModalityKind::ElectricField => C::ElectricField,
            ModalityKind::MagneticSense => C::MagneticSense,
            ModalityKind::InfraredThermal => C::InfraredThermal,
            ModalityKind::RadioNative => C::RadioNative,
            ModalityKind::Gestural => C::Gestural,
            ModalityKind::Postural => C::Postural,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Modality {
    pub kind: ModalityKind,
    pub range_m: Real,
    pub fidelity: Real,
    pub bandwidth: Real,
}

/// 12 manipulation modes. Per-mode parameters carried; tier
/// gating (e.g. T1+ material culture requires `ToolExtension`) is the
/// downstream consumer's responsibility, not encoded here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ManipulationKind {
    LimbGrasp,
    Tentacle,
    MouthBeak,
    TonguePrehensile,
    Trunk,
    Mandible,
    FluidJet,
    ToolExtension,
    WebConstruct,
    Burrow,
    ElectricDischarge,
    ChemicalSecretion,
}

#[derive(Debug, Clone, Copy)]
pub struct Manipulation {
    pub kind: ManipulationKind,
    pub force_n: Real,
    pub precision_m: Real,
    pub dexterity_score: Real,
    pub dof_count: u8,
}

/// Dynamic tool record — the runtime analogue of `ToolKind`.
/// Where `ToolKind` is a static 58-variant enum with hardcoded
/// effects in match arms, `DynamicTool` carries owned per-tool
/// effects so the catalog can grow at run time.
///
/// **Determinism:** all numeric fields are Q32.32 (`Real`); the
/// id is a u32; the name + `channel_focus` + `relation_prereqs`
/// are derived deterministically from the discovering civ id +
/// tick + proposing-cluster signature. Same seed → same dynamic
/// tools.
#[derive(Debug, Clone)]
pub struct DynamicTool {
    pub id: u32,
    pub name: String,
    /// Tier within the existing 1-5 hierarchy. Tier-5 by
    /// convention for dynamic tools (information-age peers); a
    /// future polish pass can assign tier from the cluster's
    /// average prereq tier.
    pub tier: u8,
    /// Recognition channel that anchors this tool's
    /// "specialisation" — the cluster of confirmed relations that
    /// proposed it. Surfaced for the report so the post-run can
    /// say "Mira's civ 3 invented `dynamic_charge_apparatus`."
    pub channel_focus: sim_recognition::ChannelKind,
    /// Template ids the civ must have confirmed (any one suffices)
    /// for the tool to be available. The species' discovery rule
    /// populates this with the cluster that produced the tool;
    /// future civs of the same species that confirm any of these
    /// templates rediscover the tool.
    pub relation_prereqs: Vec<u32>,
    /// Material-resource prereq mirroring the static `ToolKind`
    /// catalogue: each pair is a `(substance_idx, threshold)`
    /// tuple where the dynamic tool requires the civ's summed
    /// claim-cell density of that substance to clear the
    /// threshold. `substance_idx` is `Substance.idx()` (kept as
    /// `u32` so the species crate doesn't take a `sim_physics`
    /// dep), `threshold` is summed-density in fit-space units.
    /// Derived from the cluster's `Channel` at proposal time —
    /// substance-channel clusters (`Fuel` / `Oxidiser` /
    /// `Vapour` / `Ice` / `Fossil`) inherit the corresponding
    /// substance gate; `Temperature` / `WaterDepth` / `Charge` /
    /// `Elevation` clusters have an empty resource prereq.
    pub resource_prereqs: Vec<(u32, Real)>,
    /// Per-effect-category contribution. Magnitudes derived
    /// deterministically from the cluster size at discovery time;
    /// not later tuneable per civ.
    pub effects: DynamicToolEffects,
    pub discovered_at_tick: u64,
    pub discovered_by_civ_id: u32,
}

/// Dynamic-tool effect contributions. Mirrors the 8 effect
/// categories `ToolKind` has hardcoded match-arm methods for.
/// Defaults to identity (capacity ×1.0 = no change; bonuses 0.0).
/// Brought to 10-category parity with the static catalogue: every
/// effect a hand-authored `ToolKind` can grant is also expressible
/// as an emergent dynamic tool, even if the current
/// `effects_for_cluster` only specialises the scientific-instrument
/// ones (capacity, literacy, transmission). The mortality + lifespan
/// fields default to neutral here so the discovery pipeline can stay
/// untouched until a future polish wants emergent medicine /
/// senescence treatment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DynamicToolEffects {
    /// Multiplicative carrying-capacity factor. Identity = 1.0.
    pub capacity_multiplier: Real,
    pub food_crisis_bonus: Real,
    pub war_strength_bonus: Real,
    pub seasonal_floor_bonus: Real,
    pub catastrophe_resistance_bonus: Real,
    pub literacy_bonus: Real,
    pub expansion_rate_bonus: Real,
    pub transmission_fidelity_bonus: Real,
    /// Per-bracket per-tick mortality reduction
    /// `[infant, juvenile, fertile, elder]`. Each entry in `[0, 1]`
    /// scales that bracket's per-tick mortality down by
    /// `(1 - reduction)`. Mirrors the static
    /// `ToolKind::mortality_reduction_per_bracket()`. Neutral
    /// default = all zeros.
    pub mortality_reduction_per_bracket: [Real; 4],
    /// Multiplicative lifespan extension factor. `0.0` = no
    /// extension; `0.20` = +20% biological lifespan. Mirrors the
    /// static `ToolKind::lifespan_extension_factor()`. Neutral
    /// default = 0.
    pub lifespan_extension_factor: Real,
    /// Additive bonus to per-civ hypothesis fit cadence. Mirrors
    /// the static `ToolKind::discovery_rate_bonus()`. Neutral
    /// default = 0.
    pub discovery_rate_bonus: Real,
    /// Additive bonus to civ cohesion equilibrium target. Mirrors
    /// the static `ToolKind::cohesion_bonus()`. Neutral default = 0.
    pub cohesion_bonus: Real,
    /// Additive bonus to intra-civ migration rate. Mirrors the
    /// static `ToolKind::migration_speed_bonus()`. Neutral
    /// default = 0.
    pub migration_speed_bonus: Real,
    /// Additive bonus to per-tick birth-rate multiplier. Mirrors
    /// the static `ToolKind::fertility_bonus()`. Neutral default = 0.
    pub fertility_bonus: Real,
}

impl DynamicToolEffects {
    /// Default identity — no effect on any category.
    pub fn neutral() -> Self {
        Self {
            capacity_multiplier: Real::ONE,
            food_crisis_bonus: Real::ZERO,
            war_strength_bonus: Real::ZERO,
            seasonal_floor_bonus: Real::ZERO,
            catastrophe_resistance_bonus: Real::ZERO,
            literacy_bonus: Real::ZERO,
            expansion_rate_bonus: Real::ZERO,
            transmission_fidelity_bonus: Real::ZERO,
            mortality_reduction_per_bracket: [Real::ZERO; 4],
            lifespan_extension_factor: Real::ZERO,
            discovery_rate_bonus: Real::ZERO,
            cohesion_bonus: Real::ZERO,
            migration_speed_bonus: Real::ZERO,
            fertility_bonus: Real::ZERO,
        }
    }
}

/// Id space split. Dynamic tools start at 1000; static
/// `ToolKind` ids end at 58. Disjoint by construction.
pub const DYNAMIC_TOOL_ID_START: u32 = 1000;

/// Species habitat domain. See `Species::habitat`.
///
/// The first four (Aquatic / Terrestrial / Amphibious / Airborne)
/// are Earth-typed surface-dwellers. The latter two cover habitats
/// that an Earth-centric typology omits but that are physically
/// plausible (and biologically attested on Earth):
///
/// - `Subterranean` — primary habitat is below-surface excavated
///   space. Treats land as native (claims like Terrestrial) but
///   gains constant subsurface temperature buffering. The
///   morphological cousin is the Burrow manipulation mode.
/// - `Endolithic` — substrate-bound life inhabiting rock pore
///   space directly. Native for Silicate substrates where the
///   "habitat" is the rock itself; treats peaks and inland cells
///   as natively habitable, water cells as marginal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Habitat {
    Aquatic,
    Terrestrial,
    Amphibious,
    /// Land-dwelling but flight-capable. Treats land as primary
    /// habitat (claims and grows on land cells, like Terrestrial),
    /// but innate flight grants a +1 wrong-biome transit tier so
    /// even untrained airborne species can cross 1 water/non-
    /// habitat cell. Higher per-cell tech extends crossing range
    /// further than terrestrial species reach.
    Airborne,
    Subterranean,
    Endolithic,
}

/// Cognition substrate topology. See `Species::cognition_topology`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CognitionTopology {
    Centralized,
    Distributed,
}

/// Multi-axis cognitive profile. Collapsing cognition to a single
/// scalar means a working-memory-strong species (cephalopod-like)
/// and a social-cognition-strong species (canine-like) collapse
/// into the same downstream formula. Three orthogonal axes:
///
/// - `working_memory`: capacity to hold + manipulate symbols
///   in real time. Feeds hypothesizer cadence (fast attempts) +
///   per-fit complexity tolerance.
/// - `abstraction`: depth of formal generalization. Feeds
///   tool-tier reachability (tier-3+ tools require formal
///   abstraction) and Occam-penalty leniency.
/// - `social`: theory of mind, coalition reasoning, transmission
///   fidelity. Feeds knowledge-transmission decay and contact-
///   driven law diffusion.
///
/// All three in `[0, 1]`. The legacy `Species::cognition` scalar
/// is the unweighted average of these axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CognitionAxes {
    pub working_memory: Real,
    pub abstraction: Real,
    pub social: Real,
}

impl CognitionAxes {
    /// Build from a single scalar — replicate the value across
    /// all three axes. Used as the migration path: existing
    /// worldgen samples a scalar `cognition`, and the multi-axis
    /// struct is back-filled here. Future worldgen samples
    /// each axis independently.
    #[must_use]
    pub fn uniform(c: Real) -> Self {
        Self {
            working_memory: c,
            abstraction: c,
            social: c,
        }
    }

    /// Aggregate scalar — unweighted average. Matches the
    /// legacy `Species::cognition` field.
    #[must_use]
    pub fn average(&self) -> Real {
        (self.working_memory + self.abstraction + self.social) / Real::from_int(3)
    }
}

/// Per-species reproductive + life-history biology. Replaces the
/// homo-sapiens-calibrated 3%/yr birth + 2.8%/yr death heuristic
/// with a biology-first model: rates fall out of `clutch_size`,
/// the lifespan-fraction bracket boundaries, and per-bracket
/// survival rates rather than being globally tuned. An r-strategist
/// (large clutch, short juvenile period, no elders, low juvenile
/// survival) and a K-strategist (clutch=1, long maturation, long
/// post-reproductive period, high juvenile survival) end up with
/// dramatically different per-bracket dynamics from the same step
/// loop — both fall out of the same formulas, both numerically
/// stable.
///
/// All fractions sum-bound: `infant + maturity + eldership < 1`,
/// with `fertile = 1 - infant - maturity - eldership` derived.
/// Sampling clamps to keep `fertile_fraction >= 0.30` so even
/// extreme K-strategists retain a meaningful reproductive window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PopulationBiology {
    /// Average offspring per reproductive event per fertile adult.
    /// 1.0 (single-offspring K-strategist) to 500.0 (r-strategist
    /// broadcast spawner). Non-integer because `Real` arithmetic
    /// expects continuous-valued rates and a "1.5 average clutch"
    /// is biologically meaningful.
    pub clutch_size: Real,
    /// Fraction of lifespan spent as infant (newborn, very high
    /// mortality, fully dependent). Range [0.01, 0.10].
    pub infant_fraction: Real,
    /// Fraction of lifespan spent as juvenile (developing, moderate
    /// mortality, partly dependent). Range [0.04, 0.40].
    pub maturity_fraction: Real,
    /// Fraction of lifespan post-fertility (senescent, no births).
    /// Range [0.0, 0.30] — many species have no post-reproductive
    /// period at all (insects, fish, most reptiles); long-lived
    /// social species (elephants, whales, humans) have substantial
    /// elder periods.
    pub eldership_fraction: Real,
    /// Fraction of newborns that survive infancy under
    /// neutral conditions (no food stress). Range [0.05, 0.95].
    /// Inverse-correlated with `clutch_size` — r-strategists invest
    /// little per offspring (low survival), K-strategists invest
    /// heavily (high survival).
    pub infant_survival: Real,
    /// Fraction of juveniles that survive to fertility under
    /// neutral conditions. Range [0.20, 0.99]. Higher than
    /// `infant_survival` since juveniles have already passed the
    /// most vulnerable phase.
    pub juvenile_survival: Real,
    /// Per-tick (= per-month) food-demand multiplier per bracket.
    /// `[infant, juvenile, fertile, elder]`. Fertile is the unit
    /// reference (1.0); infants are tiny consumers (~0.3 — bodies
    /// are small but parental subsidy reduces the headline draw),
    /// juveniles eat moderately less (~0.6), elders eat near-full
    /// (~0.9 — same body mass, lower activity).
    pub food_multipliers: [Real; 4],
}

impl PopulationBiology {
    /// `fertile_fraction = 1 - infant - maturity - eldership`.
    /// Always positive by sampling-time clamps.
    pub fn fertile_fraction(&self) -> Real {
        Real::ONE - self.infant_fraction - self.maturity_fraction - self.eldership_fraction
    }

    /// Length of the fertile window in months for a given
    /// lifespan in years. Pinned to the calibration baseline
    /// (`BASELINE_MONTHS_PER_YEAR = 12`) so per-month rates stay
    /// stable across planets with different orbital periods.
    pub fn fertile_window_months(&self, lifespan_years: Real) -> Real {
        let baseline_months_per_year =
            Real::from_int(i64::try_from(protocol::BASELINE_MONTHS_PER_YEAR).unwrap_or(12));
        lifespan_years * self.fertile_fraction() * baseline_months_per_year
    }
}
