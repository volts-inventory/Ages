//! emergent tools. When a civ accumulates a coherent
//! cluster of confirmed relations on a single recognition channel,
//! it proposes a `DynamicTool` whose effects scale with the
//! cluster's depth.
//!
//! ## Why this exists
//!
//! 's `ToolKind` enum gives every species the same 58-variant
//! tech tree. layers on per-species emergent tools so a
//! species that has done deep observation work on its planet's
//! distinctive phenomena (e.g., a hydrocarbon-substrate civ that
//! confirms many laws on `Field::Substance(Fuel)` regularities)
//! can develop tools the static catalog doesn't anticipate.
//!
//! The mechanism is **dual-mode**: static `ToolKind` enum stays
//! exactly as left it. Dynamic tools live in a parallel
//! species-level registry (`Species::dynamic_tool_registry`) and
//! per-civ owned vectors (`Civ::unlocked_dynamic_tools`). The
//! effect aggregators (capacity, war strength, etc.) fold both
//! together with the same combinator (product or sum). Existing
//! tests + match-arm machinery untouched.
//!
//! ## Discovery rule
//!
//! Per-civ check at every `TOOL_EMERGENCE_CHECK_PERIOD_TICKS` (the
//! same 600-tick cadence as 's template emergence).
//!
//! 1. Group the civ's confirmed relations by `Channel`.
//! 2. For each channel with ≥ `EMERGENT_TOOL_CLUSTER_SIZE`
//!    confirmed relations:
//!  - Skip if the species already has a dynamic tool keyed on
//!    this channel (avoid duplicate proposals across civ
//!    generations).
//!  - Mint a `DynamicTool` with:
//!  * id = `species.next_dynamic_tool_id` (then increment)
//!  * `channel_focus` = the channel
//!  * `relation_prereqs` = the confirmed-relation `template_id`s
//!    in the cluster
//!  * effects scaled with cluster size — bigger clusters
//!    produce stronger tools, capped at sensible magnitudes
//!  * tier = 5 (information-age peer; future polish can
//!    derive from cluster's average prereq tier)
//!  * name = `dynamic_<channel>_apparatus_<civ_id>_t<tick>`
//!
//! ## Determinism
//!
//! Iteration: civs sorted by id; channels iterated in their enum
//! order; `relation_id`s sorted by `BTreeMap` order. Magnitude
//! formulas use Q32.32 fixed-point; same seed → same proposals,
//! same magnitudes.

use crate::Civ;
use sim_arith::Real;
use sim_physics::Substance;
use sim_recognition::ChannelKind;
use sim_species::{DynamicTool, DynamicToolEffects, Species};

use crate::discovery::Channel;

/// Map a discovery `Channel` to its corresponding `Substance` —
/// the substance the channel reads — for substance-channel cases.
/// `Temperature` / `WaterDepth` / `ChargeMagnitude` / `Elevation`
/// return `None` (they're scalar fields, not stored as densities)
/// and so produce no resource gate on the resulting dynamic tool.
fn channel_to_substance(channel: Channel) -> Option<Substance> {
    match channel {
        Channel::Fuel => Some(Substance::Fuel),
        Channel::Oxidiser => Some(Substance::Oxidiser),
        Channel::Vapour => Some(Substance::Vapour),
        Channel::Ice => Some(Substance::Ice),
        Channel::Fossil => Some(Substance::Fossil),
        Channel::Temperature
        | Channel::WaterDepth
        | Channel::ChargeMagnitude
        | Channel::Elevation => None,
    }
}

/// Threshold a dynamic substance-channel tool's resource prereq
/// should clear. Set as a single small stockpile (1 unit summed
/// across territory) — dynamic tools sit at tier-5 conventionally,
/// but they emerge from runtime cluster discovery rather than
/// authored progression, so it makes more sense for the gate to be
/// "the substrate is reachable" than to scale with tier.
const DYNAMIC_TOOL_RESOURCE_THRESHOLD: Real = Real::from_int(1);

/// Cadence of the dynamic-tool emergence scan, in ticks.
/// 600 ticks ≈ 50 sim-years, matching 's template emergence.
pub const TOOL_EMERGENCE_CHECK_PERIOD_TICKS: u64 = 240;

/// Minimum cluster size: the civ must have confirmed at least
/// this many relations on a single channel before it can propose
/// a tool focused on that channel. Higher = fewer, deeper tools;
/// lower = more, shallower tools. 5 is calibrated to fire 1-3
/// times per civ on a typical seed-42 / 5000-tick run.
pub const EMERGENT_TOOL_CLUSTER_SIZE: usize = 5;

/// Refinement step: a channel that has *already* minted a dynamic
/// tool can mint a stronger refined tool when its cluster has
/// grown by at least this many templates beyond the prior
/// proposal's cluster size. Without this, every channel was a
/// one-shot — a civ that built a deep canon on one channel got
/// the same single tool as a civ with the bare minimum 5 templates,
/// and the tech tree felt prematurely tapped-out. With it, a civ
/// that takes its Temperature canon from 5 → 10 → 15 → 20
/// templates gets a sequence of progressively stronger thermal
/// tools (effects scale with cluster_size up to
/// `EMERGENT_TOOL_MAX_SCALE_CLUSTER`).
pub const EMERGENT_TOOL_REFINEMENT_STEP: usize = 5;

/// Maximum cluster size we credit toward effect magnitudes —
/// past this, more relations don't make the tool stronger. Caps
/// the scaling so a civ that confirms hundreds of relations on
/// one channel doesn't get a runaway-effective tool.
const EMERGENT_TOOL_MAX_SCALE_CLUSTER: i64 = 20;

/// One emergent-tool proposal returned to the caller (sim/core)
/// which threads it through `Species::dynamic_tool_registry` +
/// `Civ::unlocked_dynamic_tools` and emits the event.
#[derive(Debug, Clone)]
pub struct EmergentToolProposal {
    pub tool: DynamicTool,
    pub proposing_civ_id: u32,
    /// The cluster size at proposal time. Surfaced for the report
    /// so it can say "civ 3 invented X from a cluster of 7
    /// confirmed water-depth laws."
    pub cluster_size: usize,
}

/// Whether `tick` is a tool-emergence-check tick.
pub fn is_tool_emergence_tick(tick: u64) -> bool {
    tick > 0 && tick.is_multiple_of(TOOL_EMERGENCE_CHECK_PERIOD_TICKS)
}

/// Substrate-aware variant: stretches the check period by the inverse
/// of the planet's metabolism so slow-substrate worlds run the same
/// number of tool-emergence checks per generation as fast ones.
#[must_use]
pub fn is_tool_emergence_tick_for_metabolism(tick: u64, metabolism: sim_arith::Real) -> bool {
    let period = crate::demographics::streak_ticks_for_metabolism(
        TOOL_EMERGENCE_CHECK_PERIOD_TICKS,
        metabolism,
    );
    tick > 0 && tick.is_multiple_of(period)
}

/// Map a measurement channel to a recognition-side `ChannelKind`
/// for the dynamic tool's `channel_focus`. The pairing is rough —
/// a confirmed relation on `Channel::Temperature` usually was
/// observed via `InfraredThermal` or `VisualLight` on the
/// recognition side, but the tool's narrative-focus channel is
/// chosen for clarity.
fn channel_to_kind(channel: Channel) -> ChannelKind {
    match channel {
        Channel::Temperature => ChannelKind::InfraredThermal,
        Channel::ChargeMagnitude => ChannelKind::ElectricField,
        Channel::WaterDepth => ChannelKind::AcousticWater,
        Channel::Elevation => ChannelKind::Tactile,
        Channel::Fuel | Channel::Oxidiser | Channel::Vapour | Channel::Ice | Channel::Fossil => {
            ChannelKind::ChemicalTaste
        }
    }
}

/// Derive effect magnitudes from cluster size + channel focus.
/// Saturates at `EMERGENT_TOOL_MAX_SCALE_CLUSTER`. Every channel
/// shares the scientific-instrument baseline:
///
/// - capacity multiplier: `1 + 0.20 × scale` (up to ×1.20)
/// - literacy bonus: `0.05 × scale` (up to +0.05)
/// - transmission fidelity: `0.05 × scale` (up to +0.05)
///
/// On top of the baseline, each channel adds a flavour profile so
/// emergent tools carry the character of *what the species figured
/// out* — a `Vapour` cluster behaves like sanitation; a `Fuel`
/// cluster like combustion engineering; a `WaterDepth` cluster like
/// hydrology. Magnitudes are intentionally smaller than the static
/// `ToolKind` headlines so emergent tools complement rather than
/// upstage authored tools.
fn effects_for_cluster(cluster_size: usize, channel: Channel) -> DynamicToolEffects {
    let n = i64::try_from(cluster_size.min(usize::MAX)).unwrap_or(i64::MAX);
    let scale = Real::from_int(n.min(EMERGENT_TOOL_MAX_SCALE_CLUSTER))
        / Real::from_int(EMERGENT_TOOL_MAX_SCALE_CLUSTER);
    let mut effects = DynamicToolEffects {
        capacity_multiplier: Real::ONE + Real::percent(20) * scale,
        food_crisis_bonus: Real::ZERO,
        war_strength_bonus: Real::ZERO,
        seasonal_floor_bonus: Real::ZERO,
        catastrophe_resistance_bonus: Real::ZERO,
        literacy_bonus: Real::percent(5) * scale,
        expansion_rate_bonus: Real::ZERO,
        transmission_fidelity_bonus: Real::percent(5) * scale,
        mortality_reduction_per_bracket: [Real::ZERO; 4],
        lifespan_extension_factor: Real::ZERO,
        discovery_rate_bonus: Real::ZERO,
        cohesion_bonus: Real::ZERO,
        migration_speed_bonus: Real::ZERO,
        fertility_bonus: Real::ZERO,
    };
    match channel {
        // Temperature: thermal-management instrumentation. Lifts
        // the seasonal floor and catastrophe resistance — the
        // species learns to buffer against thermal extremes.
        Channel::Temperature => {
            effects.seasonal_floor_bonus = Real::percent(5) * scale;
            effects.catastrophe_resistance_bonus = Real::percent(5) * scale;
        }
        // ChargeMagnitude: electromagnetic instrumentation —
        // detectors, batteries, primitive electronics. Accelerates
        // the science loop directly.
        Channel::ChargeMagnitude => {
            effects.discovery_rate_bonus = Real::percent(8) * scale;
        }
        // WaterDepth: hydrology — irrigation lifts food security,
        // waterways speed intra-civ migration.
        Channel::WaterDepth => {
            effects.food_crisis_bonus = Real::percent(5) * scale;
            effects.migration_speed_bonus = Real::percent(8) * scale;
        }
        // Elevation: cartographic / topographic knowledge —
        // territorial expansion benefits from knowing the terrain.
        Channel::Elevation => {
            effects.expansion_rate_bonus = Real::percent(8) * scale;
        }
        // Fuel: combustion engineering — extra capacity from
        // applied energy abundance, on top of the baseline.
        Channel::Fuel => {
            effects.capacity_multiplier = effects.capacity_multiplier + Real::percent(10) * scale;
        }
        // Oxidiser: reactive chemistry — gunpowder, propellants,
        // explosive ordnance. Lifts war strength.
        Channel::Oxidiser => {
            effects.war_strength_bonus = Real::percent(8) * scale;
        }
        // Vapour: atmospheric / sanitation knowledge — clean-water
        // and disease-vector understanding cuts infant + juvenile
        // mortality (the historical "sanitation leap").
        Channel::Vapour => {
            effects.mortality_reduction_per_bracket = [
                Real::percent(8) * scale,
                Real::percent(5) * scale,
                Real::ZERO,
                Real::ZERO,
            ];
        }
        // Ice: cryogenic preservation — buffers seasonal scarcity
        // and stretches catastrophe resistance.
        Channel::Ice => {
            effects.seasonal_floor_bonus = Real::percent(5) * scale;
            effects.catastrophe_resistance_bonus = Real::percent(5) * scale;
        }
        // Fossil: fossil-fuel energy — capacity boost on top of
        // the scientific-instrument baseline.
        Channel::Fossil => {
            effects.capacity_multiplier = effects.capacity_multiplier + Real::percent(10) * scale;
        }
    }
    effects
}

fn channel_label(channel: Channel) -> &'static str {
    match channel {
        Channel::Temperature => "thermal",
        Channel::ChargeMagnitude => "field",
        Channel::WaterDepth => "fluid",
        Channel::Elevation => "altimetric",
        Channel::Fuel => "fuel",
        Channel::Oxidiser => "oxidiser",
        Channel::Vapour => "vapour",
        Channel::Ice => "cryogenic",
        Channel::Fossil => "fossil",
    }
}

/// Scan a civ's confirmed relations for emergent-tool proposals.
/// Returns the new tools the species should adopt + the civ
/// should unlock. Caller (sim/core) is responsible for inserting
/// into `Species::dynamic_tool_registry`, bumping
/// `next_dynamic_tool_id`, copying the tool into
/// `Civ::unlocked_dynamic_tools`, and emitting events.
pub fn propose_dynamic_tools(civ: &Civ, species: &Species, tick: u64) -> Vec<EmergentToolProposal> {
    if !civ.is_active() {
        return Vec::new();
    }

    // Collect confirmed relations across active figures, grouped
    // by channel. BTreeMap iteration is sorted by Channel's
    // PartialOrd (enum order) — deterministic.
    let mut by_channel: std::collections::BTreeMap<Channel, Vec<u32>> =
        std::collections::BTreeMap::new();
    let mut seen: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    for figure in &civ.figures {
        if figure.retired_tick.is_some() {
            continue;
        }
        for (rid, rel) in &figure.hypothesizer.confirmed {
            if !seen.insert(*rid) {
                continue;
            }
            by_channel
                .entry(rel.channel)
                .or_default()
                .push(rel.template_id);
        }
    }

    // Per-channel max cluster size among prior species-wide
    // dynamic tools. Channels see a *refined* proposal when the
    // current cluster has grown by ≥ `EMERGENT_TOOL_REFINEMENT_STEP`
    // beyond the largest existing proposal; channels with no prior
    // proposal accept any cluster ≥ EMERGENT_TOOL_CLUSTER_SIZE.
    // Replaces the prior one-shot `already_focused` set which
    // capped each channel at a single tool regardless of how deep
    // the canon grew.
    let mut max_existing_cluster_per_channel: std::collections::BTreeMap<ChannelKind, usize> =
        std::collections::BTreeMap::new();
    for t in species.dynamic_tool_registry.values() {
        let existing = t.relation_prereqs.len();
        max_existing_cluster_per_channel
            .entry(t.channel_focus)
            .and_modify(|m| {
                if existing > *m {
                    *m = existing;
                }
            })
            .or_insert(existing);
    }

    let mut proposals = Vec::new();
    let mut next_id = species.next_dynamic_tool_id;
    for (channel, mut template_ids) in by_channel {
        if template_ids.len() < EMERGENT_TOOL_CLUSTER_SIZE {
            continue;
        }
        let kind = channel_to_kind(channel);
        // Refinement gate: if a prior tool exists for this
        // channel, the new cluster must have grown by at least
        // `EMERGENT_TOOL_REFINEMENT_STEP` templates beyond the
        // largest prior cluster. First proposal on a channel skips
        // this check (no `max_existing` entry).
        if let Some(&prior_max) = max_existing_cluster_per_channel.get(&kind) {
            if template_ids.len() < prior_max + EMERGENT_TOOL_REFINEMENT_STEP {
                continue;
            }
        }
        template_ids.sort_unstable();
        template_ids.dedup();
        let cluster_size = template_ids.len();
        let name = format!(
            "dynamic_{}_apparatus_civ_{}_t{}",
            channel_label(channel),
            civ.id,
            tick
        );
        // Substance-channel clusters carry a material-resource
        // gate; abstract-channel (temperature / charge / …)
        // clusters carry none. Threshold is the same DYNAMIC_TOOL_
        // RESOURCE_THRESHOLD for every substance — the gate is
        // "civ has access to the substrate" not a tier-graduated
        // stockpile.
        let resource_prereqs: Vec<(u32, Real)> = match channel_to_substance(channel) {
            Some(substance) => vec![(
                u32::try_from(substance.idx()).unwrap_or(u32::MAX),
                DYNAMIC_TOOL_RESOURCE_THRESHOLD,
            )],
            None => Vec::new(),
        };
        let tool = DynamicTool {
            id: next_id,
            name,
            tier: 5,
            channel_focus: kind,
            relation_prereqs: template_ids,
            resource_prereqs,
            effects: effects_for_cluster(cluster_size, channel),
            discovered_at_tick: tick,
            discovered_by_civ_id: civ.id,
        };
        next_id = next_id.saturating_add(1);
        proposals.push(EmergentToolProposal {
            tool,
            proposing_civ_id: civ.id,
            cluster_size,
        });
    }
    proposals
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_emergence_period_fires_on_multiples() {
        assert!(!is_tool_emergence_tick(0));
        assert!(is_tool_emergence_tick(TOOL_EMERGENCE_CHECK_PERIOD_TICKS));
        assert!(is_tool_emergence_tick(
            TOOL_EMERGENCE_CHECK_PERIOD_TICKS * 5
        ));
        assert!(!is_tool_emergence_tick(
            TOOL_EMERGENCE_CHECK_PERIOD_TICKS - 1
        ));
    }

    #[test]
    fn effects_scale_with_cluster_size() {
        let small = effects_for_cluster(EMERGENT_TOOL_CLUSTER_SIZE, Channel::Temperature);
        let big = effects_for_cluster(
            usize::try_from(EMERGENT_TOOL_MAX_SCALE_CLUSTER).unwrap_or(usize::MAX),
            Channel::Temperature,
        );
        // Bigger cluster → higher capacity multiplier.
        assert!(big.capacity_multiplier > small.capacity_multiplier);
        // Saturated cluster on Temperature gives capacity ×1.20
        // (the baseline cap; Temperature adds no capacity flavour).
        assert_eq!(big.capacity_multiplier, Real::ONE + Real::percent(20));
        // Past-saturation cluster doesn't grow further.
        let huge = effects_for_cluster(1000, Channel::Temperature);
        assert_eq!(huge.capacity_multiplier, big.capacity_multiplier);
    }

    #[test]
    fn effects_specialise_per_channel() {
        let n = usize::try_from(EMERGENT_TOOL_MAX_SCALE_CLUSTER).unwrap_or(usize::MAX);
        // Vapour cluster lifts infant + juvenile mortality
        // reduction (sanitation flavour).
        let vapour = effects_for_cluster(n, Channel::Vapour);
        assert!(vapour.mortality_reduction_per_bracket[0] > Real::ZERO);
        assert!(vapour.mortality_reduction_per_bracket[1] > Real::ZERO);
        assert_eq!(vapour.mortality_reduction_per_bracket[2], Real::ZERO);
        // Oxidiser cluster lifts war strength.
        let oxidiser = effects_for_cluster(n, Channel::Oxidiser);
        assert!(oxidiser.war_strength_bonus > Real::ZERO);
        assert_eq!(oxidiser.mortality_reduction_per_bracket[0], Real::ZERO);
        // ChargeMagnitude lifts discovery rate.
        let charge = effects_for_cluster(n, Channel::ChargeMagnitude);
        assert!(charge.discovery_rate_bonus > Real::ZERO);
        // WaterDepth lifts food-crisis bonus + migration speed.
        let water = effects_for_cluster(n, Channel::WaterDepth);
        assert!(water.food_crisis_bonus > Real::ZERO);
        assert!(water.migration_speed_bonus > Real::ZERO);
        // Fuel + Fossil stack capacity above the baseline.
        let fuel = effects_for_cluster(n, Channel::Fuel);
        let temp = effects_for_cluster(n, Channel::Temperature);
        assert!(fuel.capacity_multiplier > temp.capacity_multiplier);
    }

    #[test]
    fn channel_to_kind_covers_all_channels() {
        // Smoke: every Channel must map to a ChannelKind without
        // panicking. Each variant is exercised via Channel::ALL.
        for channel in Channel::ALL {
            let _ = channel_to_kind(channel);
            let _ = channel_label(channel);
        }
    }
}
