//! Tool unlock evaluation: `is_buildable` (species + planet hard
//! gates), `time_gate_open` (legacy wall-clock gate kept for tests),
//! `is_unlocked` (production gate: civ-maturity counts + literacy +
//! relation prereqs + tool prereqs), and serendipity-path helpers
//! (`serendipity_missing_prereqs`, `serendipity_roll`).
//!
//! The civ-maturity counts (`min_civ_confirmed_relations` +
//! `min_civ_experimental_relations`) replaced the earlier
//! `observation_threshold` gate. Tools come from confirmed laws
//! the civ has fit through its hypothesizer, not from raw template
//! firings — environment doesn't unlock tech, science does. The
//! experimental count is gated additionally on apparatus-supported
//! confirmations, so a civ that never builds `ExperimentApparatus`
//! tops out at tier-2 by construction.

use super::{ToolKind, TIER_UNLOCK_PERIOD_TICKS};
use crate::discovery::ConfirmedRelation;
use sim_arith::Real;
use sim_physics::{PhysicsState, Substance};
use sim_recognition::ChannelKind;
use sim_species::ManipulationKind;
use sim_world::Crust;
use std::collections::BTreeMap;

/// Sum a `Substance`'s density across the cells the civ has claimed.
/// Empty `claimed_cells` returns `Real::ZERO`. Out-of-range cell ids
/// are silently skipped — a no-op rather than a panic, since
/// `claimed_cells` is owned-civ data and physics state can be
/// re-laid-out across schema migrations.
#[must_use]
pub fn claim_substance_total(
    state: &PhysicsState,
    claimed_cells: &std::collections::BTreeSet<u32>,
    substance: Substance,
) -> Real {
    let densities = state.substance(substance.idx());
    let mut total = Real::ZERO;
    for cell in claimed_cells {
        if let Some(v) = densities.get(*cell as usize) {
            total = total + *v;
        }
    }
    total
}

/// Material-resource prereq check. Returns `true` iff every
/// `(Substance, threshold)` pair in `tool.resource_prereqs()` has a
/// summed-density across `claimed_cells` at or above the threshold.
/// Empty prereqs always pass. Hard gate — no serendipity bypass.
#[must_use]
pub fn resource_prereqs_satisfied(
    tool: ToolKind,
    state: &PhysicsState,
    claimed_cells: &std::collections::BTreeSet<u32>,
) -> bool {
    for (substance, threshold) in tool.resource_prereqs() {
        if claim_substance_total(state, claimed_cells, *substance) < *threshold {
            return false;
        }
    }
    true
}

/// Number of unmet `resource_prereqs` entries for a tool. Used by
/// the serendipity-path counter so a "near-miss" civ that's one
/// resource unit short doesn't get a free pass: resource gates
/// count toward the same "≥ 2 missing → no serendipity" rule that
/// relations + tools obey, but the resource gate is a hard floor —
/// even a 1-missing-only civ doesn't unlock without the substrate.
#[must_use]
pub fn resource_prereqs_missing_count(
    tool: ToolKind,
    state: &PhysicsState,
    claimed_cells: &std::collections::BTreeSet<u32>,
) -> u32 {
    let mut missing: u32 = 0;
    for (substance, threshold) in tool.resource_prereqs() {
        if claim_substance_total(state, claimed_cells, *substance) < *threshold {
            missing = missing.saturating_add(1);
        }
    }
    missing
}

pub fn is_buildable(
    tool: ToolKind,
    species_channels: &std::collections::BTreeSet<ChannelKind>,
    species_manipulations: &std::collections::BTreeSet<ManipulationKind>,
    has_magnetosphere: bool,
    has_atmosphere_or_ocean: bool,
    crust: Crust,
) -> bool {
    // Per-tool manipulation gate. The species must possess at
    // least one manipulation mode the tool accepts; an empty
    // `manipulation_prereqs` slice means "no manipulation gate"
    // (purely social / cognitive tools like `TradeNetworks`).
    //
    // Replaces the prior global `ToolExtension`-only gate so that
    // chemical-secretion / web / burrow / jet body plans can reach
    // tier-1 applied knowledge instead of being frozen at zero
    // tools forever. Substrate divergence still holds — high-
    // precision instrument tools (sensorium tier-2+, apparatus,
    // tier-5 narrative tools) keep tight manipulation prereqs.
    let manip_prereqs = tool.manipulation_prereqs();
    if !manip_prereqs.is_empty()
        && !manip_prereqs
            .iter()
            .any(|m| species_manipulations.contains(m))
    {
        return false;
    }
    let prereqs = tool.prereq_channels();
    if !prereqs.is_empty() && !prereqs.iter().any(|c| species_channels.contains(c)) {
        return false;
    }
    if let Some(allowed) = tool.crust_prereqs() {
        if !allowed.contains(&crust) {
            return false;
        }
    }
    match tool {
        // MagneticSensor and FieldPropulsionEngine both need a
        // planetary magnetic field to register / couple to.
        ToolKind::MagneticSensor | ToolKind::FieldPropulsionEngine => has_magnetosphere,
        ToolKind::FieldSensor => has_atmosphere_or_ocean,
        _ => true,
    }
}

/// Time-gated unlock decision. Returns true iff `current_tick` has
/// reached the tool's tier × `TIER_UNLOCK_PERIOD_TICKS`.
/// **Retired ** — `is_unlocked` is the production gate now;
/// kept for tests that exercise the wall-clock semantics directly.
pub fn time_gate_open(tool: ToolKind, current_tick: u64) -> bool {
    current_tick >= u64::from(tool.tier()) * TIER_UNLOCK_PERIOD_TICKS
}

/// Production unlock gate. Tool unlocks iff:
/// - `is_buildable` (manipulation prereq + native-channel prereq +
///   planet feature) holds, AND
/// - civ has fit at least `min_civ_confirmed_relations` confirmed
///   relations of its own (firing + measurement), AND
/// - of those, at least `min_civ_experimental_relations` were
///   experimentally confirmed via `ExperimentApparatus`, AND
/// - civ's `literacy_score()` reaches `literacy_floor`, AND
/// - every `(template_id, _)` in `relation_prereqs` has at least
///   one matching confirmed relation in `confirmed_relations`, AND
/// - every `ToolKind` in `tool_prereqs` is present in
///   `unlocked_tools`.
///
/// `confirmed_relations` is the union of `Hypothesizer.confirmed`
/// (firing relations) across the civ's active figures, indexed by
/// `relation_id`; the caller (`sim/core/src/lib.rs`) is responsible
/// for collecting it. `civ_confirmed_count` is the total confirmed
/// relations (firing + measurement) across the civ; passed
/// separately so the gate can read it without paying for a second
/// union over measurement maps. `civ_experimental_count` is the
/// subset of *measurement* relations whose
/// `ConfirmedMeasurement.is_experimental == true`.
/// `unlocked_tools` is the civ's `unlocked_tools` set.
#[allow(clippy::too_many_arguments)]
pub fn is_unlocked(
    tool: ToolKind,
    species_channels: &std::collections::BTreeSet<ChannelKind>,
    species_manipulations: &std::collections::BTreeSet<ManipulationKind>,
    has_magnetosphere: bool,
    has_atmosphere_or_ocean: bool,
    crust: Crust,
    civ_confirmed_count: u32,
    civ_experimental_count: u32,
    civ_literacy: Real,
    confirmed_relations: &BTreeMap<u32, ConfirmedRelation>,
    unlocked_tools: &std::collections::BTreeSet<ToolKind>,
) -> bool {
    if !is_buildable(
        tool,
        species_channels,
        species_manipulations,
        has_magnetosphere,
        has_atmosphere_or_ocean,
        crust,
    ) {
        return false;
    }
    if civ_confirmed_count < tool.min_civ_confirmed_relations() {
        return false;
    }
    if civ_experimental_count < tool.min_civ_experimental_relations() {
        return false;
    }
    if civ_literacy < tool.literacy_floor() {
        return false;
    }
    // relation-prereq check: each `(template_id, _)` pair
    // must have at least one confirmed relation matching the
    // template_id. The `ChannelKind` in the pair is narrative
    // documentation; the lookup is template-level because the
    // confirmed-relation map keys on physics-channel
    // (Temperature, WaterDepth, …) rather than sensory-channel,
    // and "the civ has fit a law about template X" is the
    // semantic the prereq actually wants to express.
    for (prereq_template, _channel_tag) in tool.relation_prereqs() {
        let satisfied = confirmed_relations
            .values()
            .any(|cr| cr.template_id == *prereq_template);
        if !satisfied {
            return false;
        }
    }
    // tool-prereq check: every prerequisite tool must
    // already be unlocked.
    for prereq_tool in tool.tool_prereqs() {
        if !unlocked_tools.contains(prereq_tool) {
            return false;
        }
    }
    true
}

/// serendipity-path unlock check. Real innovation has
/// "lucky leapfrogs" — Newton sees an apple fall before formal
/// gravity prereqs are in place; alchemists stumble onto chemistry
/// before atomic theory. The strict `is_unlocked` path locks every
/// tool to a complete prereq DAG; this auxiliary path allows a
/// civ to skip *exactly one* missing prereq (relation OR tool) if
/// the rest of the gates pass.
///
/// Returns `Some(missing_prereq_count)` (always `1`) when the
/// civ is *almost there* — buildable, observed enough, literate
/// enough, has all but one prereq. Returns `None` when the civ
/// is fully blocked (≥ 2 missing) or when the standard
/// `is_unlocked` already passes.
///
/// The caller (sim/core's tech-unlock loop) gates this on a
/// per-tick deterministic probability roll keyed on
/// `(planet_seed, civ_id, tool_id, tick)`, modulated by the civ's
/// literacy and accumulated science. Probability is tiny per tick
/// (~1e-5) but accumulates over thousands of ticks so a civ
/// stuck on one prereq for many sim-years has decent odds of
/// breakthroughs without ever satisfying the prereq formally.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn serendipity_missing_prereqs(
    tool: ToolKind,
    species_channels: &std::collections::BTreeSet<ChannelKind>,
    species_manipulations: &std::collections::BTreeSet<ManipulationKind>,
    has_magnetosphere: bool,
    has_atmosphere_or_ocean: bool,
    crust: Crust,
    civ_confirmed_count: u32,
    civ_experimental_count: u32,
    civ_literacy: Real,
    confirmed_relations: &BTreeMap<u32, ConfirmedRelation>,
    unlocked_tools: &std::collections::BTreeSet<ToolKind>,
) -> Option<u32> {
    // Hard physics-and-biology gates aren't relaxable. A civ
    // without visual sensors doesn't serendipitously discover
    // optics; without ToolExtension they don't fashion mechanisms.
    if !is_buildable(
        tool,
        species_channels,
        species_manipulations,
        has_magnetosphere,
        has_atmosphere_or_ocean,
        crust,
    ) {
        return None;
    }
    // Civ-maturity floors are hard — serendipity is a lucky
    // leap from "one prereq away," not a free pass past
    // experimentation + time. A civ short on confirmed work
    // doesn't accidentally invent quantum computing.
    if civ_confirmed_count < tool.min_civ_confirmed_relations() {
        return None;
    }
    if civ_experimental_count < tool.min_civ_experimental_relations() {
        return None;
    }
    // Literacy gate is softer (represents the civ's general
    // scribal infrastructure). Allow serendipity at 75% of strict
    // floor.
    let lit_floor = tool.literacy_floor() * Real::percent(75);
    if civ_literacy < lit_floor {
        return None;
    }
    // Count missing prereqs. Serendipity activates only at
    // exactly 1 missing — fewer means the strict path already
    // unlocked, more means the civ is too far behind for a
    // single lucky leap.
    let mut missing: u32 = 0;
    for (prereq_template, _) in tool.relation_prereqs() {
        let satisfied = confirmed_relations
            .values()
            .any(|cr| cr.template_id == *prereq_template);
        if !satisfied {
            missing = missing.saturating_add(1);
            if missing > 1 {
                return None;
            }
        }
    }
    for prereq_tool in tool.tool_prereqs() {
        if !unlocked_tools.contains(prereq_tool) {
            missing = missing.saturating_add(1);
            if missing > 1 {
                return None;
            }
        }
    }
    if missing == 1 {
        Some(1)
    } else {
        None
    }
}

/// per-tick serendipity probability roll. Returns true with
/// probability `p` where `p` scales by `civ_literacy × (1 +
/// total_confirmed_relations / 100)` — a literate civ with rich
/// science has a meaningfully higher per-tick chance than an
/// observation-only civ. Clamped at 1e-3 per tick (the most
/// literate species still doesn't unlock 1+ tools per tick).
///
/// Determinism: keyed on `(planet_seed, civ_id, tool_id, tick)`
/// via `ChaCha20Rng` so byte-replay holds across runs. Two runs
/// with the same seed produce the same exact serendipitous-unlock
/// sequence.
#[must_use]
pub fn serendipity_roll(
    planet_seed: u64,
    civ_id: u32,
    tool_id: u32,
    tick: u64,
    civ_literacy: Real,
    total_confirmed_relations: u32,
) -> bool {
    use rand::Rng;
    use rand_chacha::rand_core::SeedableRng;
    // Base per-tick probability: 1e-5 (one in 100 000 ticks ≈
    // one chance per ~7 baseline-years per (civ × tool) at the
    // reference literacy + science level). With ~50 tools and a
    // civ life of ~1000 years, roughly 0.6 expected serendipitous
    // unlocks per civ — rare but tangible.
    let base_per_million: i64 = 10;
    // Literacy multiplier: 0.5× at literacy 0, 1× at 0.5, 2× at 1.0.
    let lit_x100 = (civ_literacy.clamp01() * Real::from_int(150) + Real::from_int(50))
        .raw()
        .to_num::<i64>()
        .max(50);
    // Science multiplier: 1× per 100 confirmed relations, capped
    // at 5×. A mature canon cross-pollinates more.
    let sci_x100 = ((i64::from(total_confirmed_relations) / 100) + 1).min(5) * 100;
    // Combine: prob_per_million = base × (lit/100) × (sci/100).
    let prob_per_million = (base_per_million * lit_x100 * sci_x100) / 10_000;
    let prob_per_million = prob_per_million.clamp(1, 1_000); // clamp [1e-6, 1e-3]
    let seed = planet_seed
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(u64::from(civ_id))
        .wrapping_mul(0xBF58_476D_1CE4_E5B9)
        .wrapping_add(u64::from(tool_id))
        .wrapping_mul(0x94D0_49BB_1331_11EB)
        .wrapping_add(tick);
    let mut rng = rand_chacha::ChaCha20Rng::seed_from_u64(seed);
    let roll: i64 = rng.gen_range(0..1_000_000);
    roll < prob_per_million
}
