//! emergent recognition templates. When a civ confirms a
//! `ThresholdStep` law on a `(template, channel)` pair, the species
//! gains a corresponding species-level recognition template that
//! fires anywhere the channel passes the same threshold.
//!
//! ## Why this exists
//!
//! Pre- the recognition library was authored at compile time —
//! every species saw the same 39 templates. Civilisations on
//! different substrates ended up with different *firings* of those
//! templates but couldn't recognise regularities the authored
//! library hadn't anticipated. closes that gap: when a civ's
//! own science discovers a sharp threshold in physics state (a
//! `ThresholdStep` fit), the species canon adds a new template
//! firing on cells matching that threshold. Subsequent civs of
//! the same species inherit the broader recognition vocabulary
//! and can chain further laws on top of it.
//!
//! ## Discovery rule
//!
//! Per-civ check at every `EMERGENCE_CHECK_PERIOD_TICKS`. For each
//! confirmed relation in the civ's hypothesizer (sorted by
//! `relation_id` for determinism):
//!
//! 1. The fitted form must be `ThresholdStep` with at least one
//!    parameter that names the threshold value.
//! 2. The relation's `channel` must map to a recognition `Field`
//!    (not all measurement channels — `Elevation` for instance
//!    has no recognition-side equivalent yet).
//! 3. No authored template's `Signature::Above(field, threshold)`
//!    or `Signature::Below(field, threshold)` already covers the
//!    same `(field, threshold)` within
//!    `EMERGENCE_THRESHOLD_TOLERANCE` (20% relative). Stops the
//!    discovery from re-deriving authored templates over and over.
//! 4. No previously-discovered template covers the same pair.
//!
//! When all four hold, mint a `DiscoveredTemplate` with:
//! - id = `species.next_discovered_template_id` (then increment)
//! - signature = `Signature::Above(field, threshold)`
//! - channels = the discovering civ's perceivable channel set
//!   intersected with the channel's natural sensorium
//! - tags = `[FormTag::Threshold]` so downstream form selection
//!   prefers threshold-style fits on the new template
//!
//! ## Determinism
//!
//! Iteration order: civs sorted by id; each civ's confirmed
//! relations sorted by `relation_id` (`BTreeMap` default order).
//! Threshold extraction reads `params[0]` (the cut point in
//! `ThresholdStep`'s parameter convention). Id assignment is
//! monotonic. Same seed → same proposals.

use crate::discovery::{Channel, ConfirmedRelation};
use crate::forms::Form;
use crate::Civ;
use sim_arith::Real;
use sim_recognition::{
    ChannelKind, DiscoveredTemplate, Field, FormTag, RecognitionLibrary, Signature,
};
use sim_species::Species;

/// Cadence of the emergence-discovery scan, in ticks.
/// unit: 12 ticks/year, so 600 ticks ≈ 50 sim-years between
/// proposal opportunities. Slow enough that the species' canon
/// doesn't spam new templates every generation; fast enough that
/// a long run (multiple thousand-year horizons) still produces
/// meaningful discovery.
pub const EMERGENCE_CHECK_PERIOD_TICKS: u64 = 600;

/// Relative tolerance for "this proposed threshold is the same as
/// an existing template's threshold." Set to 20% so slight numerical
/// drift between a fitted threshold and an authored constant doesn't
/// produce duplicate discoveries.
const EMERGENCE_THRESHOLD_TOLERANCE: (i64, i64) = (20, 100);

/// One emergent-template proposal. Returned to the caller (sim/core)
/// which threads it through `Species::discovered_templates` and emits
/// the `TemplateDiscovered` event.
#[derive(Debug, Clone)]
pub struct EmergentTemplateProposal {
    pub template: DiscoveredTemplate,
    /// The civ that observed the regularity that produced the
    /// proposal. Recorded so the report can attribute the discovery.
    pub proposing_civ_id: u32,
    /// Authored or previously-discovered template id whose
    /// confirmed-fit on this civ produced the proposal.
    pub origin_template_id: u32,
}

/// Map a measurement channel (the civ's discovery surface) to the
/// recognition `Field` (the recognition-pipeline-side fact). Returns
/// `None` for channels that don't correspond to a single physics
/// field — `Elevation` reads a per-cell scalar the recognition layer
/// doesn't currently expose; `Fuel`/`Oxidiser`/`Vapour`/`Ice` map to
/// `Field::Substance(_)` with the matching variant.
fn channel_to_field(channel: Channel) -> Option<Field> {
    match channel {
        Channel::Temperature => Some(Field::Temperature),
        Channel::WaterDepth => Some(Field::WaterDepth),
        Channel::ChargeMagnitude => Some(Field::Charge),
        Channel::MagneticField => Some(Field::MagneticMagnitude),
        Channel::Fuel => Some(Field::Substance(sim_physics::Substance::Fuel)),
        Channel::Oxidiser => Some(Field::Substance(sim_physics::Substance::Oxidiser)),
        Channel::Vapour => Some(Field::Substance(sim_physics::Substance::Vapour)),
        Channel::Ice => Some(Field::Substance(sim_physics::Substance::Ice)),
        Channel::Fossil => Some(Field::Substance(sim_physics::Substance::Fossil)),
        // Elevation has no recognition-side equivalent yet — the
        // physics state's elevation is a static planet feature, not
        // a tickable cell field. Defer until recognition gains an
        // elevation `Field`.
        Channel::Elevation => None,
    }
}

/// Channels the new discovered template should belong to. Each
/// physics field has a "natural sensorium" — heat reads through
/// thermal sensors (or visual fire-watching), charge through
/// electric-field sensing, water depth through tactile / acoustic.
fn natural_channels(field: Field) -> Vec<ChannelKind> {
    match field {
        Field::Temperature => vec![ChannelKind::InfraredThermal, ChannelKind::VisualLight],
        Field::Charge => vec![ChannelKind::ElectricField],
        Field::WaterDepth => vec![ChannelKind::Tactile, ChannelKind::AcousticWater],
        Field::Substance(_) => vec![ChannelKind::ChemicalTaste],
        Field::MagneticMagnitude => vec![ChannelKind::MagneticSense],
        Field::WindMagnitude => vec![ChannelKind::Tactile, ChannelKind::AcousticAir],
    }
}

/// Whether `(field, threshold)` is already covered by any signature
/// in the authored library (within the relative tolerance) or by a
/// previously-discovered template.
fn already_covered(
    field: Field,
    threshold: Real,
    library: &RecognitionLibrary,
    discovered: &std::collections::BTreeMap<u32, DiscoveredTemplate>,
) -> bool {
    let tolerance = Real::from_ratio(
        EMERGENCE_THRESHOLD_TOLERANCE.0,
        EMERGENCE_THRESHOLD_TOLERANCE.1,
    );
    // Allowed gap = max(|threshold|, 1.0) * tolerance. The max-with-1
    // floor keeps the gap positive when the fitted threshold is near
    // zero — without it a threshold of 0.0 would always read
    // "covered".
    let one = Real::ONE;
    let scale = if threshold.abs() > one {
        threshold.abs()
    } else {
        one
    };
    let allowed_gap = scale * tolerance;

    let signature_matches = |sig: &Signature| -> bool {
        match sig {
            Signature::Above(f, t) | Signature::Below(f, t) | Signature::AbsAbove(f, t) => {
                if *f == field {
                    let diff = if *t > threshold {
                        *t - threshold
                    } else {
                        threshold - *t
                    };
                    diff < allowed_gap
                } else {
                    false
                }
            }
            // `All` / `Any` composites — recurse into subs. A composite
            // template that contains a same-field threshold within
            // tolerance counts as coverage.
            Signature::All(subs) | Signature::Any(subs) => subs.iter().any(|s| {
                // Rebuild closure via direct call — avoid infinite
                // type recursion by inlining the leaf check.
                matches!(s,
                Signature::Above(f2, t2) | Signature::Below(f2, t2) | Signature::AbsAbove(f2, t2)
                    if *f2 == field
                        && {
                            let diff = if *t2 > threshold {
                                *t2 - threshold
                            } else {
                                threshold - *t2
                            };
                            diff < allowed_gap
                        })
            }),
            _ => false,
        }
    };

    library
        .templates
        .iter()
        .any(|t| signature_matches(&t.signature))
        || discovered.values().any(|t| signature_matches(&t.signature))
}

/// Whether `tick` is an emergence-check tick. Per-civ scheduling
/// happens at the same global cadence so all civs propose on the
/// same ticks — keeps the event log tidy and reduces per-tick cost.
pub fn is_emergence_tick(tick: u64) -> bool {
    tick > 0 && tick.is_multiple_of(EMERGENCE_CHECK_PERIOD_TICKS)
}

/// Substrate-aware variant: stretches the emergence-check period by
/// the inverse of the planet's metabolism so slow-substrate worlds
/// run the same number of checks per generation as fast ones. Used by
/// sim/core's production path; the bare `is_emergence_tick` stays for
/// callers that don't have a planet in scope (tests).
#[must_use]
pub fn is_emergence_tick_for_metabolism(tick: u64, metabolism: sim_arith::Real) -> bool {
    let period =
        crate::demographics::streak_ticks_for_metabolism(EMERGENCE_CHECK_PERIOD_TICKS, metabolism);
    tick > 0 && tick.is_multiple_of(period)
}

/// Scan a civ's confirmed relations for emergent-template proposals.
/// Returns the new templates the species should adopt. Caller is
/// responsible for inserting into `Species::discovered_templates`,
/// bumping `next_discovered_template_id`, and emitting events.
pub fn propose_discovered_templates(
    civ: &Civ,
    species: &Species,
    library: &RecognitionLibrary,
    tick: u64,
) -> Vec<EmergentTemplateProposal> {
    if !civ.is_active() {
        return Vec::new();
    }

    let mut proposals = Vec::new();
    let mut next_id = species.next_discovered_template_id;
    // Build a working set of "already-discovered" templates that
    // includes both the species' existing set AND proposals made
    // earlier in this same scan — prevents two civs of the same
    // species from each proposing the same template in the same tick.
    let mut working: std::collections::BTreeMap<u32, DiscoveredTemplate> =
        species.discovered_templates.clone();

    // Collect the union of confirmed relations across the civ's
    // active figures. Sort by relation_id for deterministic order.
    let mut seen_relation_ids: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    for figure in &civ.figures {
        if figure.retired_tick.is_some() {
            continue;
        }
        for (rid, rel) in &figure.hypothesizer.confirmed {
            if !seen_relation_ids.insert(*rid) {
                continue;
            }
            if let Some(proposal) =
                propose_from_relation(rel, civ, library, &working, next_id, tick)
            {
                next_id = next_id.saturating_add(1);
                working.insert(proposal.template.id, proposal.template.clone());
                proposals.push(proposal);
            }
        }
    }
    proposals
}

/// Try to derive one proposal from a single confirmed relation.
/// Returns `None` if the relation isn't a `ThresholdStep`, the
/// channel doesn't map to a recognition `Field`, or the
/// `(field, threshold)` is already covered.
fn propose_from_relation(
    rel: &ConfirmedRelation,
    civ: &Civ,
    library: &RecognitionLibrary,
    discovered: &std::collections::BTreeMap<u32, DiscoveredTemplate>,
    next_id: u32,
    tick: u64,
) -> Option<EmergentTemplateProposal> {
    if rel.form != Form::ThresholdStep {
        return None;
    }
    if rel.params.is_empty() {
        return None;
    }
    let field = channel_to_field(rel.channel)?;
    // ThresholdStep params: `[a (below), b (above), t (cutpoint)]`.
    // The cutpoint at index 2 is the threshold the signature keys
    // on. `params_in_real_units` rescales it back to SI from
    // fit-space.
    let real_params = rel.params_in_real_units();
    let threshold = *real_params.get(2)?;

    if already_covered(field, threshold, library, discovered) {
        return None;
    }

    let channels = natural_channels(field);
    let name = format!(
        "discovered_field_{}_civ_{}_t{}",
        field_label(field),
        civ.id,
        tick
    );
    let template = DiscoveredTemplate {
        id: next_id,
        name,
        signature: Signature::Above(field, threshold),
        tags: vec![FormTag::Threshold],
        channels,
        discovered_at_tick: tick,
        discovered_by_civ_id: civ.id,
        origin_template_id: rel.template_id,
    };
    Some(EmergentTemplateProposal {
        template,
        proposing_civ_id: civ.id,
        origin_template_id: rel.template_id,
    })
}

fn field_label(field: Field) -> &'static str {
    match field {
        Field::Temperature => "temperature",
        Field::Charge => "charge",
        Field::WaterDepth => "water_depth",
        Field::Substance(s) => match s {
            sim_physics::Substance::Water => "water",
            sim_physics::Substance::Ice => "ice",
            sim_physics::Substance::Vapour => "vapour",
            sim_physics::Substance::Fuel => "fuel",
            sim_physics::Substance::Oxidiser => "oxidiser",
            sim_physics::Substance::Ash => "ash",
            sim_physics::Substance::Fossil => "fossil",
        },
        Field::MagneticMagnitude => "magnetic_magnitude",
        Field::WindMagnitude => "wind_magnitude",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emergence_period_fires_on_multiples() {
        assert!(!is_emergence_tick(0));
        assert!(is_emergence_tick(EMERGENCE_CHECK_PERIOD_TICKS));
        assert!(is_emergence_tick(EMERGENCE_CHECK_PERIOD_TICKS * 7));
        assert!(!is_emergence_tick(EMERGENCE_CHECK_PERIOD_TICKS - 1));
        assert!(!is_emergence_tick(EMERGENCE_CHECK_PERIOD_TICKS + 1));
    }

    #[test]
    fn channel_to_field_maps_known_channels() {
        assert_eq!(
            channel_to_field(Channel::Temperature),
            Some(Field::Temperature)
        );
        assert_eq!(
            channel_to_field(Channel::ChargeMagnitude),
            Some(Field::Charge)
        );
        assert_eq!(
            channel_to_field(Channel::WaterDepth),
            Some(Field::WaterDepth)
        );
        // Elevation has no recognition-side Field yet.
        assert_eq!(channel_to_field(Channel::Elevation), None);
    }

    #[test]
    fn already_covered_detects_authored_templates() {
        let lib = RecognitionLibrary::earth_like_default();
        let empty: std::collections::BTreeMap<u32, DiscoveredTemplate> =
            std::collections::BTreeMap::new();
        // The authored `lightning_buildup` template has
        // `Signature::Above(Field::Charge, 5)`. A proposal at charge
        // = 5.0 ± 20% should be flagged as covered.
        assert!(already_covered(
            Field::Charge,
            Real::from_int(5),
            &lib,
            &empty
        ));
        assert!(already_covered(
            Field::Charge,
            Real::from_ratio(55, 10),
            &lib,
            &empty
        ));
        // Far enough away → not covered.
        assert!(!already_covered(
            Field::Charge,
            Real::from_int(100),
            &lib,
            &empty
        ));
    }

    #[test]
    fn already_covered_detects_prior_discovered() {
        let lib = RecognitionLibrary::earth_like_default();
        let mut prior = std::collections::BTreeMap::new();
        prior.insert(
            sim_recognition::DISCOVERED_TEMPLATE_ID_START,
            DiscoveredTemplate {
                id: sim_recognition::DISCOVERED_TEMPLATE_ID_START,
                name: "discovered_temperature_proxy".to_string(),
                signature: Signature::Above(Field::Temperature, Real::from_int(400)),
                tags: vec![FormTag::Threshold],
                channels: vec![],
                discovered_at_tick: 100,
                discovered_by_civ_id: 1,
                origin_template_id: 0,
            },
        );
        // Proposal within tolerance of 400 → covered.
        assert!(already_covered(
            Field::Temperature,
            Real::from_int(420),
            &lib,
            &prior
        ));
        // Different field → not covered by the prior even if
        // numerically close.
        assert!(!already_covered(
            Field::WaterDepth,
            Real::from_int(420),
            &lib,
            &prior
        ));
    }
}
