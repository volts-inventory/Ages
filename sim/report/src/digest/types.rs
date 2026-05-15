//! Record types fed by the event-stream digest. The `Digest::build`
//! walker lives in `build`; this file holds the data shapes.

use protocol::{Event, PlanetDerived, PlanetMap, SpeciesDerived};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct Digest {
    pub schema_version: u32,
    pub seed: u64,
    pub ages_version: String,
    pub planet: Option<PlanetDerived>,
    pub planet_map: Option<PlanetMap>,
    pub species: Option<SpeciesDerived>,
    pub run_end: Option<RunEnd>,
    /// Civ chapters keyed by `civ_id`, walked in founding-tick order.
    pub civs: BTreeMap<u32, CivChapter>,
    pub founding_order: Vec<u32>,
    /// First-tick co-existence between two civs.
    pub contacts: Vec<Contact>,
    /// Conflicts between concurrent civs.
    pub conflicts: Vec<ConflictRecord>,
    /// Declared wars between concurrent civs. Each entry
    /// brackets a `WarDeclared` and (when present) the matching
    /// `PeaceConcluded`; `conflicts` between the two ticks for the
    /// same pair are the per-skirmish records inside the war.
    pub wars: Vec<WarRecord>,
    /// M8 — trade routes opened during the run. Each record
    /// brackets a `TradeRouteEstablished` and (when present)
    /// the matching `TradeRouteClosed` event. Routes still open
    /// at run end have `end_tick = None`.
    pub trade_routes: Vec<TradeRouteRecord>,
    /// Cross-civ knowledge diffusion between concurrent peaceful civs.
    pub diffusions: Vec<TransferRecord>,
    /// Inter-civ knowledge transmission across a collapse boundary.
    pub transmissions: Vec<TransferRecord>,
    /// `relation_id → (template_name, channel)` — built from
    /// `RelationConfirmed` events; used to label refinement and
    /// transfer events that only carry the id.
    pub relation_names: BTreeMap<u32, RelationLabel>,
    /// Recognition firings indexed by `template_id` → count. Used in
    /// the run summary as a coverage estimate.
    pub firing_counts: BTreeMap<u32, u64>,
    /// Recognition template id → human-readable name; learned from
    /// recognition firings. Useful when no relation has been
    /// confirmed for a template (e.g. perceived but never fitted).
    pub template_names: BTreeMap<u32, String>,
    /// All raw events in file order. Kept so the highlights pass and
    /// any future timeline sections can re-walk without re-reading
    /// the log.
    pub events: Vec<Event>,
}

#[derive(Debug, Clone)]
pub struct RunEnd {
    pub tick: u64,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct RelationLabel {
    pub template_id: u32,
    pub template_name: String,
    pub channel: String,
}

#[derive(Debug, Clone)]
pub struct Contact {
    pub tick: u64,
    pub civ_a: u32,
    pub civ_b: u32,
}

#[derive(Debug, Clone)]
pub struct ConflictRecord {
    pub tick: u64,
    pub winner_civ_id: u32,
    pub loser_civ_id: u32,
    pub disputed_cell_count: u32,
    pub loss_fraction_q32: i64,
    pub loser_defeated: bool,
}

/// One war's lifecycle, from `WarDeclared` to the matching
/// `PeaceConcluded`. `civ_a < civ_b` (normalised pair).
/// `end_tick` + `peace_reason` are `None` while the war is still
/// active at run end.
#[derive(Debug, Clone)]
pub struct WarRecord {
    pub start_tick: u64,
    pub end_tick: Option<u64>,
    pub aggressor_civ_id: u32,
    pub defender_civ_id: u32,
    pub civ_a: u32,
    pub civ_b: u32,
    pub start_belligerence_q32: i64,
    pub start_drive_q32: i64,
    pub start_kinship_q32: i64,
    /// `defeated` / `belligerence_dropped` / `territory_resolved`
    /// / `unresolved` (still active at run end).
    pub peace_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TransferRecord {
    pub tick: u64,
    pub source_civ_id: u32,
    pub dest_civ_id: u32,
    pub dest_figure_id: u32,
    pub relation_id: u32,
    pub source_form: String,
    pub comprehension_q32: i64,
}

#[derive(Debug, Clone)]
pub struct CivChapter {
    pub civ_id: u32,
    pub parent_civ_id: Option<u32>,
    pub founded_tick: u64,
    pub founding_figure_count: u32,
    pub initial_population_q32: i128,
    pub collapsed: Option<CollapseRecord>,
    /// Discoveries this civ confirmed (its own original work, not
    /// inherited). Sorted by tick of confirmation.
    pub discoveries: Vec<DiscoveryRecord>,
    /// Refinements this civ applied to its own (or inherited)
    /// relations. Includes proposals + their resolution.
    pub refinements: Vec<RefinementRecord>,
    /// Tools this civ unlocked, in unlock order.
    pub techs: Vec<TechRecord>,
    /// Catastrophes that struck this civ.
    pub catastrophes: Vec<CatastropheRecord>,
    /// Cosmology shifts emitted for this civ.
    pub cosmology_shifts: Vec<CosmologyRecord>,
    /// Per-civ religion / customs vector snapshots emitted
    /// when the religion vector drifted past `RELIGION_EMIT_THRESHOLD`.
    pub religion_shifts: Vec<ReligionRecord>,
    /// Named figures of this civ, in birth order.
    pub figures: Vec<FigureRecord>,
    /// Cell ids the civ claimed at founding. Used to render
    /// the per-civ territory map. Empty if the `CivFounded` event
    /// didn't carry the field (e.g. malformed log; the protocol
    /// emits it for every founding).
    pub claimed_cells: Vec<u32>,
    /// Territory snapshots over time — first entry is the founding
    /// claim, subsequent entries are emitted from
    /// `CivTerritoryChanged` events as population pushes the
    /// target cell count up or down. Renderer uses the last entry
    /// for the territory map and the count history for the
    /// expansion/contraction sparkline.
    pub territory_history: Vec<TerritorySnapshot>,
    /// Scientific-lifecycle counters.
    /// `RelationRevalidated` (inherited relations the civ
    /// re-confirmed on its own data); `RelationLapsed`
    /// (inherited relations the civ rejected);
    /// `RelationFalsified` (own laws that drifted past the
    /// prediction-tolerance threshold). Surface these as
    /// "civ X inherited 14 laws; 11 graduated, 3 lapsed; in its
    /// lifetime it falsified 9 of its own."
    pub revalidated_count: u32,
    pub lapsed_count: u32,
    pub falsified_count: u32,
    /// Emergent recognition templates this civ
    /// proposed during its lifetime. Sorted by tick of discovery.
    /// The species adopts each template into its canon — every
    /// subsequent civ inherits the broader recognition vocabulary —
    /// but the discovering civ gets the biographical credit.
    pub discovered_templates: Vec<DiscoveredTemplateRecord>,
    /// Emergent dynamic tools this civ invented from
    /// confirmed-relation clusters. Sorted by tick of invention.
    pub invented_tools: Vec<InventedToolRecord>,
    /// Life expectancy snapshots, in months at birth, sampled
    /// from `CivLifeExpectancyChanged` events. The first entry is
    /// the founding-tick value; subsequent entries pin every
    /// 2-year-or-greater shift. The renderer summarises this as
    /// "founded with 28y expectancy; reached 71y after
    /// `MedicalIntervention`" so the demographic transition has a
    /// visible per-civ timeline.
    pub life_expectancy_history: Vec<LifeExpectancySnapshot>,
    /// M8 — surplus accumulator snapshots, pinned from every
    /// `CivSurplusChanged` event the civ emitted. First entry is
    /// the first emission (`previous_q32` typically `0`); the
    /// renderer surfaces the founded → peak → current arc.
    pub surplus_history: Vec<SurplusSnapshot>,
}

/// One snapshot of a civ's economic surplus, pinned from a
/// `CivSurplusChanged` event.
#[derive(Debug, Clone)]
pub struct SurplusSnapshot {
    pub tick: u64,
    pub surplus_q32: i64,
    pub previous_q32: i64,
}

/// M8 — one trade route's lifecycle.
/// `end_tick` + `close_reason` are `None` while the route is
/// still active at run end. Pair is normalised so `civ_a < civ_b`.
#[derive(Debug, Clone)]
pub struct TradeRouteRecord {
    pub start_tick: u64,
    pub end_tick: Option<u64>,
    pub civ_a: u32,
    pub civ_b: u32,
    /// `war_declared` / `civ_collapsed` / etc. from the
    /// `TradeRouteClosed` payload, or `None` if still active.
    pub close_reason: Option<String>,
}

/// Single life-expectancy snapshot pinned from a
/// `CivLifeExpectancyChanged` event.
#[derive(Debug, Clone)]
pub struct LifeExpectancySnapshot {
    pub tick: u64,
    pub life_expectancy_months_q32: i64,
}

/// Discovered-template entry for the civ's chapter.
#[derive(Debug, Clone)]
pub struct DiscoveredTemplateRecord {
    pub tick: u64,
    pub template_id: u32,
    pub template_name: String,
    pub origin_template_id: u32,
    pub threshold_si: f64,
}

/// Invented-tool entry for the civ's chapter.
#[derive(Debug, Clone)]
pub struct InventedToolRecord {
    pub tick: u64,
    pub tool_id: u32,
    pub tool_name: String,
    pub channel_focus: String,
    pub cluster_size: u32,
    pub tier: u8,
    pub capacity_multiplier_q32: i64,
    pub literacy_bonus_q32: i64,
    pub transmission_fidelity_bonus_q32: i64,
}

#[derive(Debug, Clone)]
pub struct TerritorySnapshot {
    pub tick: u64,
    pub claimed_cells: Vec<u32>,
    pub population_q32: i128,
    /// Per-cell population breakdown, in the same order as
    /// `claimed_cells`. Empty for older event logs that pre-date
    /// the field; readers (the keyframe renderer) treat empty as
    /// "no per-cell density available, render flat ownership".
    pub cell_populations_q32: Vec<i128>,
    /// Per-cell carrying capacity, in the same order as
    /// `claimed_cells`. Lets the colored viewport's pop-digit
    /// scale read each cell as `pop / cap` saturation. Empty for
    /// older event logs; readers fall back to a frame-relative
    /// max-pop scale.
    pub cell_capacities_q32: Vec<i128>,
}

#[derive(Debug, Clone)]
pub struct CollapseRecord {
    pub tick: u64,
    pub reason: String,
    pub final_population_q32: i128,
    pub final_figure_count: u32,
}

#[derive(Debug, Clone)]
pub struct DiscoveryRecord {
    pub tick: u64,
    pub relation_id: u32,
    pub figure_id: u32,
    pub template_id: u32,
    pub template_name: String,
    pub channel: String,
    pub form: String,
    pub params_q32: Vec<i64>,
    pub residual_q32: i64,
    pub confidence_q32: i64,
    pub n_samples: u32,
}

#[derive(Debug, Clone)]
pub enum RefinementOutcome {
    Proposed,
    Confirmed {
        new_params_q32: Vec<i64>,
        new_residual_q32: i64,
        new_confidence_q32: i64,
    },
    Rejected {
        reason: String,
    },
}

#[derive(Debug, Clone)]
pub struct RefinementRecord {
    pub tick: u64,
    pub figure_id: u32,
    pub relation_id: u32,
    pub old_form: String,
    pub new_form: String,
    pub outcome: RefinementOutcome,
}

#[derive(Debug, Clone)]
pub struct TechRecord {
    pub tick: u64,
    pub tool_id: u32,
    pub tool_name: String,
    pub tier: u8,
    pub granted_channels: Vec<String>,
    pub newly_perceivable_template_ids: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct CatastropheRecord {
    pub tick: u64,
    pub kind: String,
    pub fraction_lost_q32: i64,
}

#[derive(Debug, Clone)]
pub struct CosmologyRecord {
    pub tick: u64,
    pub axes_q32: [i64; 5],
    pub dogmatism_q32: i64,
}

/// Per-civ religion-vector snapshot. Mirrors `CosmologyRecord`
/// but with three axes (`theology`, `ritual`, `sacred_time`).
#[derive(Debug, Clone)]
pub struct ReligionRecord {
    pub tick: u64,
    pub axes_q32: [i64; 3],
    pub dogmatism_q32: i64,
}

#[derive(Debug, Clone)]
pub struct FigureRecord {
    pub tick: u64,
    pub id: u32,
    pub name: String,
    /// Personality scalars as `Q32.32` raw bits, mirroring the
    /// `FigureBorn` protocol event. The renderer converts via
    /// `q32_to_f64` for display.
    pub charisma_q32: i64,
    pub curiosity_q32: i64,
    pub doubt_q32: i64,
    pub communicativeness_q32: i64,
    /// Cell the figure observes. Surfaced
    /// as the figure's marker on the per-civ territory map.
    pub cell_assignment: u32,
}
