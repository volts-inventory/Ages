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
/// substrate **alone**. Kept on the wire as the substrate-keyed
/// `RunMetadata::planet_type_labels` table for legacy consumers,
/// but the viewport renders via `planet_archetype` so the label
/// actually reflects the planet's surface.
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

/// Surface-aware planet archetype. The substrate-only `planet_type`
/// labels every aqueous-biology planet "ocean world" even when 0 % of
/// its surface holds liquid water — call this instead from any
/// renderer that has the `PlanetMap` (water-depth grid) in hand.
///
/// Inputs:
/// - `substrate`        — `aqueous` / `ammoniacal` / `hydrocarbon` / `silicate`
/// - `mean_t_k`         — planet-mean surface temperature (K)
/// - `freeze_k`, `boil_k` — substrate solvent freeze/boil (already
///   perturbed via `Planet::substrate_perturbation_q32`)
/// - `terrain_peak_m`   — 0 → no rocky surface (gas giant)
/// - `ocean_frac`       — fraction of cells with `water_depth > 0`,
///   in `[0, 1]`. (For non-aqueous substrates the renderer still
///   threads water depth as a generic surface-liquid proxy.)
///
/// Decision: substrate × thermal regime (frozen / liquid / vapor
/// relative to its own solvent) × ocean coverage band.
#[must_use]
pub fn planet_archetype(
    substrate: &str,
    mean_t_k: f64,
    freeze_k: f64,
    boil_k: f64,
    terrain_peak_m: f64,
    ocean_frac: f64,
) -> &'static str {
    if terrain_peak_m <= 0.0 {
        return "gas giant";
    }
    let frozen = freeze_k > 0.0 && mean_t_k < freeze_k;
    let vapor = boil_k > 0.0 && mean_t_k > boil_k;
    let cover = if ocean_frac >= 0.50 {
        Cover::Dominant
    } else if ocean_frac >= 0.15 {
        Cover::Significant
    } else if ocean_frac >= 0.02 {
        Cover::Sparse
    } else {
        Cover::Dry
    };
    match (substrate, frozen, vapor, cover) {
        // ── aqueous (water solvent) ──
        ("aqueous", true, _, _) => "ice world",
        ("aqueous", _, true, _) => "hothouse world",
        ("aqueous", false, false, Cover::Dominant) => "ocean world",
        ("aqueous", false, false, Cover::Significant) => "continental world",
        ("aqueous", false, false, Cover::Sparse) => "arid world",
        ("aqueous", false, false, Cover::Dry) => "desert world",
        // ── hydrocarbon (methane/ethane solvent) ──
        ("hydrocarbon", true, _, _) => "frozen methane world",
        ("hydrocarbon", _, true, _) => "scorched hydrocarbon world",
        ("hydrocarbon", false, false, Cover::Dominant) => "methane sea world",
        ("hydrocarbon", false, false, Cover::Significant) => "methane-lake world",
        ("hydrocarbon", false, false, Cover::Sparse) => "frigid arid world",
        ("hydrocarbon", false, false, Cover::Dry) => "frigid desert",
        // ── ammoniacal (ammonia solvent) ──
        ("ammoniacal", true, _, _) => "frozen ammonia world",
        ("ammoniacal", _, true, _) => "scorched ammonia world",
        ("ammoniacal", false, false, Cover::Dominant) => "ammonia sea world",
        ("ammoniacal", false, false, Cover::Significant) => "ammonia-lake world",
        ("ammoniacal", false, false, Cover::Sparse) => "cold arid world",
        ("ammoniacal", false, false, Cover::Dry) => "cold desert",
        // ── silicate (molten-rock solvent) ──
        // Silicate freeze ≈ 1687 K — anything below is solid rock,
        // which a human reader will recognise as "rocky world"
        // regardless of what the biology runs on. Liquid silicate
        // (any cover) is the textbook lava world. Vapor (T > 3500 K)
        // is exotic territory.
        ("silicate", true, _, _) => "rocky world",
        ("silicate", _, true, _) => "vaporised silicate world",
        ("silicate", false, false, _) => "lava world",
        _ => "unknown world",
    }
}

#[derive(Copy, Clone)]
enum Cover {
    Dominant,
    Significant,
    Sparse,
    Dry,
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

    /// Substrate-only `planet_type` over-labelled every aqueous-
    /// biology planet "ocean world" — seed 42 surfaces with 0 wet
    /// cells but still got the label. `planet_archetype` consults
    /// the surface water-coverage fraction + thermal regime, so the
    /// label tracks geography instead of biology.
    #[test]
    fn planet_archetype_matrix_aqueous() {
        let (fz, bo, peak) = (273.15, 373.15, 14060.0);
        // Earth-like: 70 % ocean coverage in liquid range → ocean
        assert_eq!(
            planet_archetype("aqueous", 288.0, fz, bo, peak, 0.70),
            "ocean world"
        );
        // Earth's actual 0.71 — boundary check
        assert_eq!(
            planet_archetype("aqueous", 288.0, fz, bo, peak, 0.50),
            "ocean world"
        );
        // Continental, 30 % seas
        assert_eq!(
            planet_archetype("aqueous", 288.0, fz, bo, peak, 0.30),
            "continental world"
        );
        // Arid (a few inland seas)
        assert_eq!(
            planet_archetype("aqueous", 288.0, fz, bo, peak, 0.08),
            "arid world"
        );
        // Seed-42 regression: 0 wet cells, mid-band temperature
        assert_eq!(
            planet_archetype("aqueous", 366.0, fz, bo, peak, 0.0),
            "desert world"
        );
        // Mars-like: below freeze → ice world regardless of water-frac
        assert_eq!(
            planet_archetype("aqueous", 210.0, fz, bo, peak, 0.0),
            "ice world"
        );
        // Europa: aqueous biology, frozen surface
        assert_eq!(
            planet_archetype("aqueous", 100.0, fz, bo, peak, 0.0),
            "ice world"
        );
        // Venus-like: above boil → hothouse regardless of water-frac
        assert_eq!(
            planet_archetype("aqueous", 735.0, fz, bo, peak, 0.0),
            "hothouse world"
        );
    }

    #[test]
    fn planet_archetype_matrix_hydrocarbon() {
        let (fz, bo, peak) = (90.7, 111.7, 8000.0);
        // Titan: methane in liquid range, small north-polar lakes
        assert_eq!(
            planet_archetype("hydrocarbon", 94.0, fz, bo, peak, 0.03),
            "frigid arid world"
        );
        // Methane ocean world (hypothetical)
        assert_eq!(
            planet_archetype("hydrocarbon", 100.0, fz, bo, peak, 0.65),
            "methane sea world"
        );
        // Below methane freeze
        assert_eq!(
            planet_archetype("hydrocarbon", 50.0, fz, bo, peak, 0.0),
            "frozen methane world"
        );
        // Above methane boil
        assert_eq!(
            planet_archetype("hydrocarbon", 200.0, fz, bo, peak, 0.0),
            "scorched hydrocarbon world"
        );
    }

    #[test]
    fn planet_archetype_matrix_ammoniacal() {
        let (fz, bo, peak) = (195.4, 239.8, 9000.0);
        assert_eq!(
            planet_archetype("ammoniacal", 220.0, fz, bo, peak, 0.55),
            "ammonia sea world"
        );
        assert_eq!(
            planet_archetype("ammoniacal", 220.0, fz, bo, peak, 0.05),
            "cold arid world"
        );
        assert_eq!(
            planet_archetype("ammoniacal", 150.0, fz, bo, peak, 0.0),
            "frozen ammonia world"
        );
        assert_eq!(
            planet_archetype("ammoniacal", 300.0, fz, bo, peak, 0.0),
            "scorched ammonia world"
        );
    }

    #[test]
    fn planet_archetype_matrix_silicate() {
        let (fz, bo, peak) = (1687.0, 3538.0, 12000.0);
        // Cool silicate → just rock; no liquid medium for biology
        assert_eq!(
            planet_archetype("silicate", 600.0, fz, bo, peak, 0.0),
            "rocky world"
        );
        // Mid silicate-melt range → lava world regardless of cover
        assert_eq!(
            planet_archetype("silicate", 2500.0, fz, bo, peak, 0.0),
            "lava world"
        );
        assert_eq!(
            planet_archetype("silicate", 2500.0, fz, bo, peak, 0.80),
            "lava world"
        );
        // Above silicate boil
        assert_eq!(
            planet_archetype("silicate", 4000.0, fz, bo, peak, 0.0),
            "vaporised silicate world"
        );
    }

    #[test]
    fn planet_archetype_gas_giant_short_circuits() {
        // terrain_peak == 0 → gas giant regardless of any other field
        assert_eq!(
            planet_archetype("aqueous", 288.0, 273.15, 373.15, 0.0, 0.70),
            "gas giant"
        );
        assert_eq!(
            planet_archetype("ammoniacal", 130.0, 195.4, 239.8, 0.0, 0.0),
            "gas giant"
        );
    }
}
