//! Civ-lifecycle events: founding, collapse, territory, contact,
//! conflict, knowledge transmission/diffusion, cosmology, cohesion,
//! catastrophe, tech, figure-born, dynamic-tool/template
//! discoveries.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Conflict resolution between two civs whose claimed cells
/// overlap. Strength model: pop × literacy × Hierarchical-bonus.
/// `loser_defeated` true when loser's pop fell below floor and
/// they surrendered the disputed cells to the winner.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConflictResolved {
    pub tick: u64,
    pub winner_civ_id: u32,
    pub loser_civ_id: u32,
    pub disputed_cell_count: u32,
    pub loss_fraction_q32: i64,
    pub loser_defeated: bool,
}

/// War declared between two civs once their per-pair
/// belligerence score crossed `WAR_DECLARE_THRESHOLD`. Belligerence
/// = `drive × (1 − KINSHIP_DAMPENER · kinship)`, where `drive`
/// combines population pressure, defender capacity slack, and
/// attacker strength share, and `kinship` is the average closeness
/// across cosmology + literacy gaps. The triple
/// `(belligerence, drive, kinship)` is included so consumers can
/// surface *why* the war fired.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WarDeclared {
    pub tick: u64,
    pub aggressor_civ_id: u32,
    pub defender_civ_id: u32,
    pub belligerence_q32: i64,
    pub drive_q32: i64,
    pub kinship_q32: i64,
}

/// Why an active war ended. `Defeated`: loser surrendered
/// every disputed cell because per-cell populations fell below
/// `CELL_FLIP_FLOOR`. `BelligerenceDropped`: pair's belligerence
/// score fell below `WAR_END_THRESHOLD` (hysteresis); cells may
/// still overlap. `TerritoryResolved`: overlap emptied without
/// any cell flipping (loser withdrew).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PeaceReason {
    Defeated,
    BelligerenceDropped,
    TerritoryResolved,
}

/// War between `civ_a` and `civ_b` concluded. Pair is
/// normalised so `civ_a < civ_b`. `duration_ticks` is the gap
/// between the matching `WarDeclared` and this event.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PeaceConcluded {
    pub tick: u64,
    pub civ_a: u32,
    pub civ_b: u32,
    pub reason: PeaceReason,
    pub duration_ticks: u64,
}

/// Cross-civ knowledge diffusion: a single relation
/// transmitted between concurrent peaceful civs (both
/// `Hierarchical < 0.4`). Uses the comprehension formula minus
/// the age-decay term (direct contact, not artifacts).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct KnowledgeDiffused {
    pub tick: u64,
    pub source_civ_id: u32,
    pub dest_civ_id: u32,
    pub dest_figure_id: u32,
    pub relation_id: u32,
    pub source_form: String,
    pub comprehension_q32: i64,
}

/// M5: two civs are alive simultaneously. Emitted once per pair
/// per (first-coexistence-tick); used by the post-run report and
/// any consumers that need to know when concurrent-civ dynamics
/// (knowledge diffusion, conflict) start.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CivContact {
    pub tick: u64,
    pub civ_a: u32,
    pub civ_b: u32,
}

/// Catastrophe event. Emitted when a per-civ trigger fires
/// (volcanic eruption from extreme charge × temperature, disease
/// outbreak from crowding × civ-age). `kind` is one of
/// `volcanic` / `disease`. `fraction_lost_q32` is the population-
/// fraction the catastrophe removed; civs that survive can re-
/// found via the post-catastrophe trigger.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CatastropheFired {
    pub tick: u64,
    pub civ_id: u32,
    /// `volcanic` / `disease`. Renamed from `kind` to avoid colliding
    /// with the `Event` enum's `tag = "kind"` discriminant when
    /// serialised to NDJSON; `serde_json` deserialisation rejects
    /// the duplicate field.
    pub catastrophe_kind: String,
    pub fraction_lost_q32: i64,
}

/// Cosmology drift event. Emitted when a civ's 5-axis
/// cosmology vector drifts at least `0.50` (L2 distance) from
/// the last snapshot — the threshold was raised from 0.20
/// because the fast-divergent layer now lives in
/// `ReligionShifted` and cosmology should now move on near-
/// millennium timescales. `axes_q32` packs `[empirical,
/// communitarian, reformist, mystical, hierarchical]` as Q32.32
/// raw bits; `dogmatism_q32` is the L2-norm-derived dogmatism
/// scalar.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CosmologyShifted {
    pub tick: u64,
    pub civ_id: u32,
    pub axes_q32: Vec<i64>,
    pub dogmatism_q32: i64,
}

/// Civ's life expectancy at birth changed by at least 2 years
/// since the last emission. Calculated from the civ's current
/// `PopulationDynamics` (which itself reflects current tech
/// unlocks via `dynamics_for_civ`'s per-tick re-derivation), so
/// the value is the neutral-environment expectancy a Civ would
/// converge to with adequate food. Emitted on civ founding plus
/// any subsequent shift past the 2-year threshold so the post-
/// run report can timeline the demographic transition (e.g.
/// "civ-3's life expectancy went from 28y at founding to 71y
/// after `MedicalIntervention` unlocked at year 1180").
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CivLifeExpectancyChanged {
    pub tick: u64,
    pub civ_id: u32,
    /// Q32.32 raw bits of the life expectancy at birth, in months.
    pub life_expectancy_months_q32: i64,
}

/// Religion / customs drift event. Emitted when a civ's
/// 3-axis religion vector drifts at least `0.20` (L2 distance)
/// from the last snapshot. `axes_q32` packs `[theology, ritual,
/// sacred_time]` as Q32.32 raw bits; `dogmatism_q32` is the
/// religion-vector L2-norm-derived dogmatism. Religion is the
/// fast-divergent layer that absorbs schism dynamics
/// (Reformation, sectarian conflict) and drives intra-species
/// war via the kinship weighting.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReligionShifted {
    pub tick: u64,
    pub civ_id: u32,
    pub axes_q32: Vec<i64>,
    pub dogmatism_q32: i64,
}

/// One predecessor relation comprehended into a successor civ.
/// Emitted per transmitted relation immediately after a
/// `CivFounded` event when the inheritance pipeline runs. The
/// `comprehension_q32` value is the score that brought it
/// across (linguistic + age + tier factors); the successor's
/// confidence is the predecessor's confidence × this score.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct KnowledgeTransmitted {
    pub tick: u64,
    pub source_civ_id: u32,
    pub dest_civ_id: u32,
    pub dest_figure_id: u32,
    pub relation_id: u32,
    pub source_form: String,
    pub comprehension_q32: i64,
}

/// A civilization founded. Emitted when a stateless cohort
/// left by a collapsed predecessor crosses the founding threshold,
/// or once at run start for the inaugural civ. `parent_civ_id`
/// names the predecessor (None for the inaugural civ).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CivFounded {
    pub tick: u64,
    pub civ_id: u32,
    pub parent_civ_id: Option<u32>,
    /// Deterministic kingdom-style civ name (e.g. `Eldoria`,
    /// `Karnath`, `Sumeris`). Sampled from a 64-stem × 6-ending
    /// pool keyed by `seed XOR civ_id XOR magic`. Same
    /// `(seed, civ_id)` always produces the same string. Older
    /// NDJSON files don't carry the field; the
    /// `#[serde(default)]` makes deserialization resilient — those
    /// events deserialize with `name = ""` and the narrator falls
    /// back to `"civ {civ_id}"`.
    #[serde(default)]
    pub name: String,
    /// Q96.32 raw bits of the founding cohort population. Backed by
    /// `Pop` (i128) so modern/future-age civs can carry > 2.1B
    /// without saturating. Field kept as `_q32` for legacy NDJSON
    /// compatibility — the Q-format suffix here documents the
    /// fractional precision (32 bits) common to `Real` (Q32.32) and
    /// `Pop` (Q96.32). Integer width is now i128. Serialized as a
    /// JSON string (`serde`'s tagged-enum buffer doesn't support
    /// `i128` natively — see `pop_bits_serde`).
    #[serde(with = "crate::pop_bits_serde")]
    #[schemars(with = "String")]
    pub initial_population_q32: i128,
    pub founding_figure_count: u32,
    /// Cell ids the civ claims as its territory at founding
    /// Used by the post-run report to render per-civ
    /// territory maps. Inaugural civs claim every cell on the
    /// grid; successor civs inherit the predecessor's claim
    /// (refound) or get a copy of the parent's claim (breakaway).
    pub claimed_cells: Vec<u32>,
    /// Per-cell carrying capacity at founding (Q96.32 raw, one
    /// entry per cell in `claimed_cells` in the same order).
    /// Lets the viewport's pop-digit scale read each founder
    /// cell as `pop / cap` from tick 0 — without it, the renderer
    /// hits the `frame_max_pop` fallback and every founder cell
    /// ties for digit `9`, regardless of the cap formula. Empty
    /// for older NDJSON logs.
    #[serde(default, with = "crate::pop_bits_vec_serde")]
    #[schemars(with = "Vec<String>")]
    pub cell_capacities_q32: Vec<i128>,
}

/// A civilization's territory changed. Emitted when the
/// civ's claimed-cell set adds or drops cells — shrinking during
/// dark ages / population decline, growing as the civ recovers.
/// Tied to population: target cell count is
/// `ceil(population / k)` for a fixed people-per-cell constant.
/// Cells are added/dropped from the centroid outward (BFS), so
/// contraction sheds the most distant cells first. Not emitted on
/// no-op ticks; the report reconstructs the last territory by
/// replaying these.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CivTerritoryChanged {
    pub tick: u64,
    pub civ_id: u32,
    pub claimed_cells: Vec<u32>,
    /// Q96.32 raw bits of the cohort population at the time of the
    /// change. Lets consumers correlate territory size with the
    /// underlying pressure. Backed by `Pop` (i128) — modern civs
    /// reach into the billions, well beyond Q32.32's ±2.1B ceiling.
    /// Wire-encoded as a JSON string (see `pop_bits_serde`).
    #[serde(with = "crate::pop_bits_serde")]
    #[schemars(with = "String")]
    pub population_q32: i128,
    /// Per-cell population breakdown. One entry per cell in
    /// `claimed_cells`, in the same order. Lets the post-run
    /// keyframe maps render density (cell pop / cap) as ASCII
    /// shading rather than just ownership-by-civ. Sums to
    /// `population_q32`. Wire-encoded as a JSON array of decimal
    /// strings (see `pop_bits_vec_serde`).
    #[serde(default, with = "crate::pop_bits_vec_serde")]
    #[schemars(with = "Vec<String>")]
    pub cell_populations_q32: Vec<i128>,
    /// Per-cell carrying capacity (Q96.32 raw). One entry per cell
    /// in `claimed_cells`, in the same order. Computed from the
    /// civ's `cell_capacity` (tech × terrain × seasonal ×
    /// biosphere) at the moment of the territory change. Lets the
    /// colored viewport's pop-digit scale read each cell as a
    /// fraction of *its own* cap — digit `9` = saturated, `0` =
    /// nearly empty — so density tells a "how full is this cell"
    /// story rather than absolute headcount. Empty for older event
    /// logs; consumers fall back to a frame-relative scale. Wire-
    /// encoded as a JSON array of decimal strings.
    #[serde(default, with = "crate::pop_bits_vec_serde")]
    #[schemars(with = "Vec<String>")]
    pub cell_capacities_q32: Vec<i128>,
}

/// A civilization collapsed. Emitted once per civ lifecycle.
/// Reason is one of `food_crisis` / `knowledge_plateau` for the
/// M4-minimum trigger set; cultural-lock and conquest reasons land
/// in v2 once cosmology drift wires and M5 conflict ships. The
/// civ's cohort transitions to stateless after this event; the
/// founding pipeline reads stateless population as input.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CivCollapsed {
    pub tick: u64,
    pub civ_id: u32,
    pub reason: String,
    /// Q96.32 raw bits of the cohort population at collapse.
    /// Wire-encoded as a JSON string (see `pop_bits_serde`).
    #[serde(with = "crate::pop_bits_serde")]
    #[schemars(with = "String")]
    pub final_population_q32: i128,
    pub final_figure_count: u32,
}

/// A sensorium-extending tool unlocked for a civ. Emitted
/// when the time-gate opens and the prereqs hold; permanent for the
/// civ's lifetime. Consumers chain this with subsequent recognition
/// firings to render the latent → perceivable transition.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TechUnlocked {
    pub tick: u64,
    pub civ_id: u32,
    pub tool_id: u32,
    pub tool_name: String,
    pub tier: u8,
    pub granted_channels: Vec<String>,
    /// Recognition template ids that became newly perceivable as a
    /// direct result of this unlock. Empty when the tool extends
    /// range / precision rather than granting a new channel.
    pub newly_perceivable_template_ids: Vec<u32>,
    /// True if this unlock fired via the serendipity path
    /// (one missing prereq waived by a low-probability per-tick
    /// roll keyed on civ literacy and accumulated science) rather
    /// than the strict prereq-met path. Lets consumers narrate
    /// "civ X stumbled onto Y before they had Z" — real innovation
    /// has lucky leapfrogs; locking every unlock to a complete
    /// prereq DAG is the unrealistic case. Defaults to `false` for
    /// older event logs.
    #[serde(default)]
    pub serendipitous: bool,
}

/// A named figure joined the run's roster — emitted when a
/// founding band lands at civ founding and as new figures are added
/// over the civ's lifetime. Already-contributed figures persist in
/// the event log indefinitely so the post-run report can render the
/// full discoverer history even after the civ collapses.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FigureBorn {
    pub tick: u64,
    pub civ_id: u32,
    pub figure_id: u32,
    pub name: String,
    /// Personality scalars — `Q32.32` raw bits.
    /// `charisma` scales cosmology drift + drives
    /// charismatic-founder triggers.
    pub charisma_q32: i64,
    /// `curiosity` reserved for fit-attempt cadence.
    pub curiosity_q32: i64,
    /// `doubt` scales refinement aggressiveness via the
    /// hypothesizer's `switch_margin`.
    pub doubt_q32: i64,
    /// `communicativeness` boosts transmission comprehension
    /// when this figure's parent civ transmits to a successor.
    pub communicativeness_q32: i64,
    /// The cell this figure observes (M3 cell-assignment
    /// scheme: `cell_idx % n_active_figures == cell_assignment`).
    /// In the post-run report this is rendered as the figure's
    /// position-of-attention on the per-civ territory map.
    pub cell_assignment: u32,
}

/// Civ internal-cohesion shift event payload. `cohesion_q32`
/// is Q32.32 raw bits in `[0, 1]`; consumers convert via
/// `i64 as f64 / 2^32`. Emitted when cohesion crosses ≥ 0.05
/// absolute change since the last emission, or when the civ first
/// dips below the civil-war floor (0.10).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CohesionShifted {
    pub tick: u64,
    pub civ_id: u32,
    pub cohesion_q32: i64,
    /// Previous emitted cohesion. Lets consumers narrate the
    /// magnitude / direction of the shift without retaining their
    /// own state.
    pub previous_q32: i64,
}

/// M8 — civ's surplus accumulator shifted by at least
/// `SURPLUS_EMIT_DELTA_FLOOR` (currently 50 pop-equivalents)
/// since the last emission. Carries the new `surplus_q32` raw
/// (Q32.32 fixed-point) and the `previous_q32` for narration.
/// Emitted on civ founding + any subsequent shift; consumers
/// timeline the economic-buffer history.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CivSurplusChanged {
    pub tick: u64,
    pub civ_id: u32,
    pub surplus_q32: i64,
    pub previous_q32: i64,
}

/// Emergent dynamic-tool event.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolDiscovered {
    pub tick: u64,
    pub civ_id: u32,
    /// Auto-assigned id ≥ `DYNAMIC_TOOL_ID_START` (1000).
    pub tool_id: u32,
    pub tool_name: String,
    /// Recognition channel that anchored the tool's specialisation
    /// (e.g. `"infrared_thermal"` for a thermal apparatus).
    pub channel_focus: String,
    /// Number of confirmed relations the cluster contained at
    /// proposal time. Surfaced for the report so it can show
    /// "tool invented from a cluster of 7 thermal laws."
    pub cluster_size: u32,
    /// Tier (1-5). Currently fixed at 5 for all dynamic tools;
    /// future polish can derive from cluster's prereq tiers.
    pub tier: u8,
    /// Per-effect contributions, raw Q32.32 bits. Lets the report
    /// render "this tool gives ×1.18 capacity" without reading
    /// `Real::ONE` rescaling logic into the renderer.
    pub capacity_multiplier_q32: i64,
    pub literacy_bonus_q32: i64,
    pub transmission_fidelity_bonus_q32: i64,
}

/// Emergent recognition template event. Recorded once per
/// discovery; the template's id, name, and origin are pinned so
/// the post-run report can attribute the regularity to a civ.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TemplateDiscovered {
    pub tick: u64,
    pub civ_id: u32,
    /// Auto-assigned id ≥ `DISCOVERED_TEMPLATE_ID_START` (1000).
    pub template_id: u32,
    /// Human-readable label, generated deterministically from the
    /// channel + civ id + tick.
    pub template_name: String,
    /// Authored or previously-discovered template id whose
    /// confirmed-fit produced the proposal. Lets the report
    /// chain "civ X observed template Y → derived new template Z".
    pub origin_template_id: u32,
    /// SI-rescaled threshold value the new `Signature::Above` keys
    /// on. Surfaced for display so the report can render
    /// "discovered: temperature > 273.114 K (water-ice transition)".
    pub threshold_si: f64,
}
