//! Label vocabulary — single source of truth for the
//! human-readable strings the viewport, post-run narrator, and any
//! future presentation consumer use to describe planets, species,
//! cognition, sociality, and communication.
//!
//! Originally these functions lived inline in `viewport.rs`, which
//! made them hard to reuse from `sim_core` (which needs them in
//! order to *populate* the `RunMetadata` event with the same
//! tables) and from any future renderer. They now live in
//! this dedicated module so:
//!
//! - **viewport.rs** imports the functions for in-process use.
//! - **`sim_core::metadata`** iterates `KNOWN_*` const arrays and
//!   builds a `protocol::RunMetadata` from the function outputs,
//!   then emits it in the NDJSON event stream.
//! - **`narrate.py`** reads the same `RunMetadata` from the
//!   event log instead of duplicating the tables itself.
//!
//! That single chain — Rust function → metadata builder →
//! `RunMetadata` event → `narrate.py` lookup — means a label
//! change in this file propagates to every consumer. The
//! "no `sim_report` → `sim_physics` dep" rule still stands;
//! substrate freeze/boil ranges (the *only* upstream
//! `sim_physics` data the viewport needs) flow through
//! `RunMetadata` instead of a direct dep.

use protocol::RunMetadata;

// ── known keys for metadata builders ──

/// Substrate enum keys that appear in `Planet::metabolic_substrate`
/// (mirrors `sim_world::MetabolicSubstrate`). Order is the canonical
/// "aqueous → silicate" enum order so iteration is deterministic.
pub const KNOWN_SUBSTRATES: &[&str] = &["aqueous", "ammoniacal", "hydrocarbon", "silicate"];

/// Atmosphere enum keys that appear in `Planet::atmosphere`
/// (mirrors `sim_world::Atmosphere`).
pub const KNOWN_ATMOSPHERES: &[&str] = &["none", "thin", "oxidising", "reducing", "hazy"];

/// Host-species wellbeing badge keys, in temperature-position
/// order from coldest to hottest.
pub const KNOWN_BADGES: &[&str] = &[
    "frozen-out",
    "near-freezing",
    "thriving",
    "near-boiling",
    "boiling-off",
    "vacuum",
];

/// All `sim_species::ModalityKind` debug names. Order mirrors
/// the enum declaration in `sim_species`.
pub const KNOWN_MODALITIES: &[&str] = &[
    "AcousticAir",
    "AcousticWater",
    "Seismic",
    "VisualLight",
    "VisualPolarization",
    "Bioluminescent",
    "ChemicalPheromone",
    "ChemicalTaste",
    "Tactile",
    "ElectricField",
    "MagneticSense",
    "InfraredThermal",
    "RadioNative",
    "Gestural",
    "Postural",
];

/// All `sim_species::ManipulationKind` debug names. Order mirrors
/// the enum declaration in `sim_species`.
pub const KNOWN_MANIPULATIONS: &[&str] = &[
    "LimbGrasp",
    "Tentacle",
    "MouthBeak",
    "TonguePrehensile",
    "Trunk",
    "Mandible",
    "FluidJet",
    "ToolExtension",
    "WebConstruct",
    "Burrow",
    "ElectricDischarge",
    "ChemicalSecretion",
];

/// Trait-bucket boundaries for 0..1 scalars (cognition,
/// sociality, communication-fidelity). Three buckets: low,
/// medium, high.
pub const TIER_THRESHOLDS: [f64; 2] = [0.34, 0.67];

/// Cognition tier labels in low → high order.
pub const COG_TIER_LABELS: [&str; 3] = ["low", "medium", "high"];

/// Sociality tier labels in low → high order.
pub const SOCIALITY_TIER_LABELS: [&str; 3] = ["solitary", "social", "eusocial"];

/// Communication-fidelity tier labels in low → high order.
pub const COMM_TIER_LABELS: [&str; 3] = ["noisy", "clear", "precise"];

// ── label functions (pure mappings) ──

/// Planet-type archetype noun derived from the metabolic
/// substrate. Card line 1 reads as `"{planet_type} · {badge}"`.
#[must_use]
pub fn planet_type(substrate: &str) -> &'static str {
    match substrate {
        "aqueous" => "ocean world",
        "ammoniacal" => "ammonia world",
        "hydrocarbon" => "methane world",
        "silicate" => "lava world",
        _ => "unknown world",
    }
}

/// Biochemistry implied by the substrate. Aqueous /
/// ammoniacal / hydrocarbon worlds run carbon biochemistry;
/// silicate is the lone silicon-substrate entry.
#[must_use]
pub fn substrate_biochem(substrate: &str) -> &'static str {
    match substrate {
        "silicate" => "silicon",
        _ => "carbon",
    }
}

/// Descriptive atmosphere label. Internal `Atmosphere`
/// variants (`Oxidising`, `Reducing`, `Hazy`, `Thin`, `None`)
/// expand to forms that hint at composition without requiring
/// chemistry background.
#[must_use]
pub fn atmosphere_descriptor(atm: &str) -> &'static str {
    match atm {
        "none" => "vacuum",
        "thin" => "thin",
        "oxidising" => "oxygen-rich",
        "reducing" => "methane-rich",
        "hazy" => "hazy",
        _ => "unknown",
    }
}

/// Human-readable rewrite of the host-species wellbeing badge.
/// The internal names (`frozen-out`, `near-freezing`, `thriving`,
/// `near-boiling`, `boiling-off`, `vacuum`) are precise but
/// hyphenated; this maps them to single-word adjectives that
/// read naturally next to a planet-type noun.
#[must_use]
pub fn friendly_badge(badge: &str) -> &'static str {
    match badge {
        "frozen-out" => "frozen",
        "near-freezing" => "cold",
        "thriving" => "habitable",
        "near-boiling" => "hot",
        "boiling-off" => "scorching",
        "vacuum" => "vacuum",
        _ => "unknown",
    }
}

/// Format the planet's atmospheric composition as a one-line
/// snapshot of the top three channels by mass fraction. Returns
/// `None` for vacuum (sum near zero) or older event logs that
/// default all channels to 0. Output looks like
/// `air: 78%N₂ 21%O₂ 1%Ar`. Channels below 0.5% are skipped.
#[must_use]
pub fn format_atmospheric_composition(p: &protocol::PlanetDerived) -> Option<String> {
    use crate::q32::q32_to_f64;
    let channels = [
        ("N\u{2082}", q32_to_f64(p.atmospheric_n2_q32)),
        ("O\u{2082}", q32_to_f64(p.atmospheric_o2_q32)),
        ("CO\u{2082}", q32_to_f64(p.atmospheric_co2_q32)),
        ("CH\u{2084}", q32_to_f64(p.atmospheric_ch4_q32)),
        ("NH\u{2083}", q32_to_f64(p.atmospheric_nh3_q32)),
        ("H\u{2082}O", q32_to_f64(p.atmospheric_h2o_q32)),
        ("H\u{2082}", q32_to_f64(p.atmospheric_h2_q32)),
        ("Ar", q32_to_f64(p.atmospheric_ar_q32)),
        ("oth", q32_to_f64(p.atmospheric_other_q32)),
    ];
    let total: f64 = channels.iter().map(|(_, v)| v).sum();
    if total < 0.05 {
        return None;
    }
    let mut sorted: Vec<(&str, f64)> = channels
        .iter()
        .filter(|(_, v)| *v >= 0.005)
        .copied()
        .collect();
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    sorted.truncate(3);
    let parts: Vec<String> = sorted
        .iter()
        .map(|(n, v)| format!("{:.0}%{}", v * 100.0, n))
        .collect();
    Some(format!("air: {}", parts.join(" ")))
}

/// Short label for a `ModalityKind`. The most-readable labels
/// expand to fuller forms; compound or physically short variants
/// stay abbreviated.
#[must_use]
pub fn short_modality(m: &str) -> &'static str {
    match m {
        "AcousticAir" => "ac-air",
        "AcousticWater" => "ac-wtr",
        "Seismic" => "seismic",
        "VisualLight" => "vision",
        "VisualPolarization" => "vis-pol",
        "Bioluminescent" => "biolum",
        "ChemicalPheromone" => "phero",
        "ChemicalTaste" => "chem",
        "Tactile" => "tactile",
        "ElectricField" => "elec",
        "MagneticSense" => "mag",
        "InfraredThermal" => "ir",
        "RadioNative" => "radio",
        "Gestural" => "gesture",
        "Postural" => "posture",
        _ => "?",
    }
}

/// Short label for a `ManipulationKind`.
#[must_use]
pub fn short_manip(m: &str) -> &'static str {
    match m {
        "LimbGrasp" => "limbs",
        "Tentacle" => "tentacle",
        "MouthBeak" => "beak",
        "TonguePrehensile" => "tongue",
        "Trunk" => "trunk",
        "Mandible" => "mandible",
        "FluidJet" => "jet",
        "ToolExtension" => "tools",
        "WebConstruct" => "webs",
        "Burrow" => "burrow",
        "ElectricDischarge" => "zap",
        "ChemicalSecretion" => "secrete",
        _ => "?",
    }
}

/// Tier bucket for the cognition scalar.
#[must_use]
pub fn cog_tier(c: f64) -> &'static str {
    tier_label(c, &COG_TIER_LABELS)
}

/// Tier bucket for the sociality scalar.
#[must_use]
pub fn sociality_label(s: f64) -> &'static str {
    tier_label(s, &SOCIALITY_TIER_LABELS)
}

/// Tier bucket for the communication-fidelity scalar.
#[must_use]
pub fn comm_label(c: f64) -> &'static str {
    tier_label(c, &COMM_TIER_LABELS)
}

#[inline]
fn tier_label(value: f64, labels: &[&'static str; 3]) -> &'static str {
    if value < TIER_THRESHOLDS[0] {
        labels[0]
    } else if value < TIER_THRESHOLDS[1] {
        labels[1]
    } else {
        labels[2]
    }
}

/// Substrate-relative habitability badge for the host species.
/// Callers pass the freeze/boil values they already received via
/// the `RunMetadata` event so the function is presentation-only
/// (no `sim_report → sim_physics` dependency).
///
/// Returns one of the internal names (`frozen-out`,
/// `near-freezing`, `thriving`, `near-boiling`, `boiling-off`,
/// or `vacuum`); pair with `friendly_badge` for the display
/// adjective.
#[must_use]
pub fn host_species_status(
    metabolic_substrate: &str,
    atmosphere: &str,
    mean_t_k: f64,
    freeze_k: f64,
    boil_k: f64,
) -> &'static str {
    if atmosphere == "none" && metabolic_substrate != "silicate" {
        // Most substrates need an atmosphere; silicate
        // tolerates vacuum.
        return "vacuum";
    }
    let range = boil_k - freeze_k;
    if range <= 0.0 {
        return "thriving";
    }
    let pos = (mean_t_k - freeze_k) / range;
    if pos < 0.0 {
        "frozen-out"
    } else if pos < 0.25 {
        "near-freezing"
    } else if pos > 1.0 {
        "boiling-off"
    } else if pos > 0.75 {
        "near-boiling"
    } else {
        "thriving"
    }
}

// ── builder for RunMetadata ──

/// Build a `RunMetadata` from the label functions in this
/// module + the substrate freeze/boil ranges supplied by the
/// caller (which gets them from `sim_physics::chemistry`).
///
/// `sim_core` calls this once at run start to populate the
/// `RunMetadata` event that goes into the NDJSON stream. By
/// keeping the builder here (rather than in `sim_core`) we
/// guarantee the wire-format tables match the in-process
/// label functions exactly — there's no second copy to drift.
#[must_use]
pub fn build_run_metadata<F>(substrate_range_k: F) -> RunMetadata
where
    F: Fn(&str) -> (f64, f64),
{
    let mut metadata = RunMetadata::default();
    for &substrate in KNOWN_SUBSTRATES {
        let (freeze, boil) = substrate_range_k(substrate);
        metadata.substrate_freeze_k.insert(substrate.into(), freeze);
        metadata.substrate_boil_k.insert(substrate.into(), boil);
        metadata
            .planet_type_labels
            .insert(substrate.into(), planet_type(substrate).into());
        metadata
            .planet_biochem_labels
            .insert(substrate.into(), substrate_biochem(substrate).into());
    }
    for &atm in KNOWN_ATMOSPHERES {
        metadata
            .atmosphere_labels
            .insert(atm.into(), atmosphere_descriptor(atm).into());
    }
    for &badge in KNOWN_BADGES {
        metadata
            .friendly_badge_labels
            .insert(badge.into(), friendly_badge(badge).into());
    }
    for &m in KNOWN_MODALITIES {
        metadata
            .modality_short_labels
            .insert(m.into(), short_modality(m).into());
    }
    for &m in KNOWN_MANIPULATIONS {
        metadata
            .manipulation_short_labels
            .insert(m.into(), short_manip(m).into());
    }
    metadata.tier_thresholds = TIER_THRESHOLDS.to_vec();
    metadata.cog_tier_labels = COG_TIER_LABELS.iter().map(|s| (*s).into()).collect();
    metadata.sociality_tier_labels = SOCIALITY_TIER_LABELS.iter().map(|s| (*s).into()).collect();
    metadata.comm_tier_labels = COMM_TIER_LABELS.iter().map(|s| (*s).into()).collect();
    metadata
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_metadata_populates_every_table() {
        let m = build_run_metadata(|sub| match sub {
            "aqueous" => (273.15, 373.15),
            "ammoniacal" => (195.4, 239.8),
            "hydrocarbon" => (90.7, 111.7),
            "silicate" => (1687.0, 3538.0),
            _ => (0.0, 0.0),
        });
        assert_eq!(m.substrate_freeze_k.len(), KNOWN_SUBSTRATES.len());
        assert_eq!(m.substrate_boil_k.len(), KNOWN_SUBSTRATES.len());
        assert_eq!(m.planet_type_labels.len(), KNOWN_SUBSTRATES.len());
        assert_eq!(m.planet_biochem_labels.len(), KNOWN_SUBSTRATES.len());
        assert_eq!(m.atmosphere_labels.len(), KNOWN_ATMOSPHERES.len());
        assert_eq!(m.friendly_badge_labels.len(), KNOWN_BADGES.len());
        assert_eq!(m.modality_short_labels.len(), KNOWN_MODALITIES.len());
        assert_eq!(m.manipulation_short_labels.len(), KNOWN_MANIPULATIONS.len());
        assert_eq!(m.tier_thresholds.len(), 2);
        assert_eq!(m.cog_tier_labels.len(), 3);
        assert_eq!(m.sociality_tier_labels.len(), 3);
        assert_eq!(m.comm_tier_labels.len(), 3);
    }

    #[test]
    fn host_species_status_applies_thresholds() {
        // Aqueous freeze=273.15 boil=373.15 → range 100K
        // pos = 0.01 → near-freezing
        assert_eq!(
            host_species_status("aqueous", "oxidising", 274.15, 273.15, 373.15),
            "near-freezing"
        );
        // pos = 0.5 → thriving
        assert_eq!(
            host_species_status("aqueous", "oxidising", 323.15, 273.15, 373.15),
            "thriving"
        );
        // pos = 0.99 → near-boiling
        assert_eq!(
            host_species_status("aqueous", "oxidising", 372.15, 273.15, 373.15),
            "near-boiling"
        );
        // pos > 1 → boiling-off
        assert_eq!(
            host_species_status("aqueous", "oxidising", 400.0, 273.15, 373.15),
            "boiling-off"
        );
        // No atmosphere on aqueous → vacuum
        assert_eq!(
            host_species_status("aqueous", "none", 300.0, 273.15, 373.15),
            "vacuum"
        );
        // No atmosphere on silicate → not vacuum-gated (silicate
        // tolerates vacuum). Temp 2600K is mid-range
        // (~50% between freeze 1687 and boil 3538) → thriving.
        assert_eq!(
            host_species_status("silicate", "none", 2600.0, 1687.0, 3538.0),
            "thriving"
        );
    }
}
