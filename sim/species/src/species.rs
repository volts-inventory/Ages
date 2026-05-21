//! `Species` struct + impl. Persistent unit of a run; civilizations
//! rise and fall within it.

use crate::types::{
    CognitionAxes, CognitionTopology, DynamicTool, EcosystemRole, Habitat, Lifecycle, Manipulation,
    Modality, PopulationBiology, ToleranceEnvelope,
};
use sim_arith::Real;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct Species {
    pub seed: u64,
    /// Deterministic species name from the seed (e.g.
    /// `Kelvars`, `Tolaks`). Pure flavour; no behaviour
    /// depends on it. Used by viewport / report layers for
    /// human-readable identification ("Kelvars founded civ 1"
    /// reads better than "civ 1 founded").
    pub name: String,
    /// Trait scalars, all in `[0, 1]`. Feed `t0_loss` and the
    /// fit-tolerance / minimum-sample formulas. `cognition` is
    /// the aggregate scalar (the average of `cognition_axes`)
    /// kept for backward compatibility with every downstream
    /// formula that reads `species.cognition`. Future-touch
    /// formulas can read the relevant axis directly off
    /// `cognition_axes` — e.g. hypothesizer-cadence on
    /// `working_memory`, tech-tool-capacity on `abstraction`,
    /// transmission-fidelity on `social`.
    pub cognition: Real,
    /// Multi-axis cognitive profile. Collapsing cognition to a
    /// single scalar means a working-memory-strong species
    /// (cephalopod-like) and a social-cognition-strong species
    /// (canine-like) are interchangeable in every downstream
    /// formula. Axes are independent in `[0, 1]`; the legacy
    /// `cognition` scalar is the unweighted average.
    pub cognition_axes: CognitionAxes,
    pub sociality: Real,
    pub communication_fidelity: Real,
    /// Lifespan in years. Influences `t0_loss` via per-generation
    /// turnover.
    pub lifespan_years: Real,
    /// Modality vector. Multiple channels coexist; downstream
    /// consumers query per channel.
    pub modalities: Vec<Modality>,
    /// Manipulation modes available to the species.
    pub manipulation_modes: Vec<Manipulation>,
    /// Recognition templates the species can perceive natively. The
    /// intersection of `template_channels(t.id)` and the species'
    /// modality set. Sensorium-extending tech widens this set
    /// over a civ's lifetime.
    pub perceivable_templates: BTreeSet<u32>,
    /// Per-generation cultural-memory loss applied
    /// to T0 (oral) tokens. Civ-level tier modifiers attenuate
    /// further via the tier table.
    pub t0_loss: Real,
    /// Cognition substrate topology — one of `Centralized`
    /// (vertebrate-equivalent baseline), `DistributedRedundant`
    /// (cephalopod-equivalent, parallel sensing but capped
    /// abstraction), `Collective` (hive-mind, collapses under
    /// isolation), `Acentric` (slime-mold-equivalent, slow but
    /// cumulative cross-generational memory). Drives per-topology
    /// multipliers on hypothesis-attempt cadence, knowledge-decay
    /// rate, and an abstraction-axis hard cap — see
    /// `CognitionTopology::attempt_period_multiplier`,
    /// `knowledge_decay_multiplier`, `abstraction_cap`,
    /// `isolation_penalty`.
    pub cognition_topology: CognitionTopology,
    /// Native habitat domain. `Aquatic` species evolved in
    /// water (`OceanWorld` / `SubSurfaceOcean` planets with
    /// water-acoustic sensing and fluid-jet / tentacle manipulation); `Terrestrial`
    /// species evolved on land. `Amphibious` species cross both
    /// domains natively (rocky planets with both water-acoustic and
    /// air-acoustic sensing). Drives territorial gating in
    /// `sim_core::compute_territory`: a civ can natively only claim
    /// cells matching its habitat until it unlocks
    /// `ToolKind::AmphibiousConstruction`, after which it can claim
    /// cells in either domain.
    pub habitat: Habitat,
    /// Emergent recognition templates. Civs of this species
    /// propose new templates from observation regularities — the
    /// proposals graduate into this map and become first-class
    /// recognition firings, indistinguishable from authored
    /// templates downstream. Indexed by template id (assigned
    /// starting at `DISCOVERED_TEMPLATE_ID_START` = 1000).
    /// Sorted iteration via `BTreeMap` preserves the determinism
    /// contract.
    pub discovered_templates: BTreeMap<u32, sim_recognition::DiscoveredTemplate>,
    /// Next id allocator for discovered templates. Stays
    /// monotonic across the species' lifetime so collisions are
    /// impossible and replay is byte-stable. Initialised to
    /// `DISCOVERED_TEMPLATE_ID_START` at species genesis.
    pub next_discovered_template_id: u32,
    /// Emergent tool registry. When a civ accumulates a
    /// coherent cluster of confirmed relations on a single
    /// channel, it proposes a `DynamicTool` whose effects scale
    /// with the cluster's depth. The species-level registry
    /// preserves discoveries across civ collapse boundaries so
    /// successor civs that hit the same prereq cluster can
    /// rediscover (rather than duplicate) the tool. Indexed by
    /// id starting at `DYNAMIC_TOOL_ID_START` = 1000 to keep the
    /// id space disjoint from the static `ToolKind` enum (1..=58).
    pub dynamic_tool_registry: BTreeMap<u32, DynamicTool>,
    /// Next id allocator for dynamic tools. Monotonic.
    pub next_dynamic_tool_id: u32,
    /// Per-seed cosmology pole-position bias. The five axes
    /// (`Empirical`, `Communitarian`, `Reformist`, `Mystical`,
    /// `Hierarchical`) used to start every civ at neutral
    /// `[0, 0, 0, 0, 0]`; the species derives a starting bias from
    /// species traits + planet so civs of an aquatic, highly-social
    /// species inherit a `+Communitarian` starting position
    /// (without overriding the axes themselves — the same five
    /// debates exist for every species, but civs *enter* those
    /// debates from per-seed positions). Cosmology drift events
    /// then push from this starting point as before.
    ///
    /// Order: `[empirical, communitarian, reformist, mystical,
    /// hierarchical]`. Each value is bounded `[-1, 1]` like the
    /// existing axes; the bias formula caps at ±0.50 so the
    /// starting position never out-shouts in-life drift.
    pub initial_cosmology: [Real; 5],
    /// Per-species reproductive + life-history biology. Drives
    /// the 4-bracket cohort step (infant / juvenile / fertile /
    /// elder), per-bracket food demand, and per-bracket survival.
    /// Replaces the homo-sapiens-calibrated 3%/yr birth + 2.8%/yr
    /// death heuristic with biology-first rates derived directly
    /// from `clutch_size`, `infant_fraction`, `maturity_fraction`,
    /// `eldership_fraction`, and the per-bracket survival fields.
    pub biology: PopulationBiology,
    /// Environmental tolerance envelope. Habitat occupancy gates
    /// on cell conditions ∩ tolerance; catastrophe survival
    /// multiplies by `tolerance.match_score(local_conditions)` so
    /// extremophile species shaped to high-radiation, high-
    /// temperature, or high-pressure niches differentially survive
    /// catastrophes that wipe out narrower-envelope species.
    /// Defaults derive from `MetabolicSubstrate` and are jittered
    /// ±20% per axis from the species seed at derivation time.
    pub tolerance: ToleranceEnvelope,
    /// Life-history topology. Determines which per-tick step
    /// function the population engine runs each tick. Existing
    /// species default to `Vertebrate` (4-bracket cohort
    /// dynamics, unchanged); future-touch r=1 broadcast-spawner
    /// derivations route through `Aquatic { semelparous: true }`,
    /// social insects through `Eusocial`, etc. See
    /// `sim_population::lifecycle_step` for the per-variant
    /// dynamics.
    pub lifecycle: Lifecycle,
    /// Trophic / functional role in the planet's multi-species
    /// ecosystem. Drives the per-tick ecosystem step (Lindeman
    /// pyramid + functional-response delta) and worldgen
    /// role-distribution constraints. Civ-bearing species are
    /// always a consumer tier with cognition ≥ 0.3.
    pub role: EcosystemRole,
    /// Tardigrade-grade dormancy capability ∈ [0, 1]. A species
    /// with `dormancy = 0.9` takes ~10× less damage from
    /// catastrophes at full severity than one with `dormancy = 0`,
    /// and seeds a dormant population reservoir that revives over
    /// hundreds of ticks (see `DormantPool::resurrect_step`).
    /// Sprint 2 Item 7b.
    pub dormancy_capability: Real,
    /// True iff the species is still alive in the run. The
    /// extinction rule (Sprint 2 Item 6a) flips this off when a
    /// species' biomass / population pool stays below
    /// `EXTINCTION_THRESHOLD` for `EXTINCTION_CONFIRMATION_TICKS`
    /// in a row; the record stays in the per-planet registry for
    /// history / replay determinism but is skipped by the
    /// ecosystem step. Defaults to `true` for back-compat with
    /// every existing literal `Species { ... }` construction.
    pub is_extant: bool,
}

impl Species {
    /// Whether the species can sense a given recognition template
    /// natively, before any sensorium-extending tech.
    pub fn can_perceive(&self, template_id: u32) -> bool {
        self.perceivable_templates.contains(&template_id)
    }

    /// E-fold time for cross-collapse knowledge decay,
    /// in years. Long-lived + highly social species preserve oral
    /// tradition far better — a 200-year elephant-equivalent with
    /// sociality 0.8 keeps knowledge alive ~3× longer than a
    /// 10-year solitary species.
    ///
    /// `CognitionTopology::knowledge_decay_multiplier` further
    /// stretches the constant inversely — an Acentric species
    /// with multiplier 0.2 gets a 5× longer e-fold window,
    /// reflecting that knowledge encoded in cumulative substrate
    /// traces survives generations far better than oral or
    /// cortical stores.
    pub fn transmission_decay_years(&self) -> Real {
        let base = Real::from_int(500)
            + self.lifespan_years * Real::from_int(5)
            + self.sociality * Real::from_int(1000);
        let mult = self
            .cognition_topology
            .knowledge_decay_multiplier()
            .max(Real::percent(1));
        base / mult
    }

    /// Same as `transmission_decay_years` but converted to
    /// month-grained ticks (1 tick = 1 month). Used by transmission's
    /// `age_decay` formula.
    pub fn transmission_decay_ticks(&self) -> u64 {
        let years = self.transmission_decay_years();
        let months_per_year = i64::try_from(protocol::MONTHS_PER_YEAR).unwrap_or(12);
        let ticks_real = years * Real::from_int(months_per_year);
        let raw: i64 = ticks_real.raw().to_num();
        u64::try_from(raw.max(1)).unwrap_or(1)
    }

    /// Aggregate communication-channel transmission-speed
    /// multiplier in `[0.1, 1.0]`. Picks the *fastest* modality
    /// the species has (a species with both pheromone and
    /// acoustic-air channels inherits the acoustic speed for
    /// transmission purposes — knowledge propagates on the
    /// fastest available substrate). Defaults to 1.0 when the
    /// species has no modalities (degenerate test fixtures).
    /// See `CognitionTopology::transmission_speed_for_modality`.
    pub fn communication_speed_multiplier(&self) -> Real {
        if self.modalities.is_empty() {
            return Real::ONE;
        }
        self.modalities
            .iter()
            .map(|m| CognitionTopology::transmission_speed_for_modality(m.kind))
            .fold(Real::ZERO, |a, b| if b > a { b } else { a })
    }

    /// Per-species stress factor for `step_with_capacity`.
    /// Sociality buffers (mutual aid) and cognition buffers
    /// (adaptive behaviour) the death amplification under food
    /// stress. Centred at 4.0 for a balanced species (sociality =
    /// cognition = 0.5); ranges 3.0 (high both) to 5.0 (low both).
    pub fn stress_factor(&self) -> Real {
        (Real::from_int(5) - self.sociality - self.cognition).max(Real::from_int(2))
    }

    /// Copy the subset of firings the species can perceive natively.
    /// `Firing` is `Copy`, so the result is independently owned and
    /// cheap to forward to the civ observation phase.
    pub fn perceivable_firings(
        &self,
        firings: &[sim_recognition::Firing],
    ) -> Vec<sim_recognition::Firing> {
        firings
            .iter()
            .copied()
            .filter(|f| self.can_perceive(f.template_id))
            .collect()
    }
}
