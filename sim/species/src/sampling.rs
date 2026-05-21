//! Species derivation helpers — modality / manipulation samplers,
//! per-template channel registry, default per-kind parameters,
//! habitat/cosmology bias derivation, and the planet-relative
//! support predicate. The entry-point `derive` lives in `derive`.

use crate::types::{
    CognitionTopology, Habitat, Manipulation, ManipulationKind, Modality, ModalityKind,
    PopulationBiology, ToleranceEnvelope,
};
use rand::Rng;
use rand_chacha::ChaCha20Rng;
use sim_arith::transcendental::ln;
use sim_arith::Real;
use sim_world::{Atmosphere, BiosphereClass, Composition, Magnetosphere, MetabolicSubstrate, Planet};

/// Map a recognition template id to the modality channels that
/// natively sense it. Hand-wired for the M2 canonical 5; expand as
/// the recognition catalog grows. A follow-up promotes this
/// to a richer registry tied to the unlock table.
#[allow(clippy::too_many_lines, clippy::match_same_arms)]
pub fn template_channels(template_id: u32) -> &'static [ModalityKind] {
    match template_id {
        // fire: visible flame + radiated heat + smoke smell + felt
        // warmth on contact-adjacent skin.
        1 => &[
            ModalityKind::VisualLight,
            ModalityKind::InfraredThermal,
            ModalityKind::ChemicalTaste,
            ModalityKind::Tactile,
        ],
        // lightning_buildup: pre-discharge static field; raises hair
        // (tactile) when nearby.
        2 => &[ModalityKind::ElectricField, ModalityKind::Tactile],
        // ice_present: white-bright surface + cold contact + thermal
        // contrast against warmer surroundings.
        3 => &[
            ModalityKind::VisualLight,
            ModalityKind::Tactile,
            ModalityKind::InfraredThermal,
        ],
        // vapour_present: visible haze + humid skin contact + smell.
        4 => &[
            ModalityKind::VisualLight,
            ModalityKind::ChemicalTaste,
            ModalityKind::Tactile,
        ],
        // surface_water: visible body of water; tactile when entered.
        5 => &[ModalityKind::VisualLight, ModalityKind::Tactile],
        // flood_zone: large body of standing water; auditory
        // (waves), tactile (immersion), visible, smell of damp.
        8 => &[
            ModalityKind::AcousticWater,
            ModalityKind::Tactile,
            ModalityKind::VisualLight,
            ModalityKind::ChemicalTaste,
        ],
        // cold_zone: cold contact + thermal contrast; some species
        // use vision (frost, breath) as a cue too.
        9 => &[ModalityKind::Tactile, ModalityKind::InfraredThermal],
        // fertile_land: visible vegetation, scent (pheromones +
        // taste), texture under foot.
        10 => &[
            ModalityKind::VisualLight,
            ModalityKind::ChemicalPheromone,
            ModalityKind::ChemicalTaste,
            ModalityKind::Tactile,
        ],
        // auroral_activity: visible glow + felt EM field; species
        // with electric/magnetic senses or radio reception perceive
        // the sustained charge dramaturgy directly.
        11 => &[
            ModalityKind::VisualLight,
            ModalityKind::ElectricField,
            ModalityKind::MagneticSense,
            ModalityKind::RadioNative,
        ],
        // harmonic_resonance: heard, felt, and (for seismic species)
        // ground-coupled. The signature phenomenon of "field-and-
        // resonance" archetype worlds.
        12 => &[
            ModalityKind::AcousticAir,
            ModalityKind::AcousticWater,
            ModalityKind::Tactile,
            ModalityKind::Seismic,
        ],
        // static_field_gradient: directly felt by electric-/magnetic-
        // sense species; distributed-nervous-system species pick it
        // up via tactile too.
        13 => &[
            ModalityKind::ElectricField,
            ModalityKind::MagneticSense,
            ModalityKind::Tactile,
        ],
        // tidal_extremum: deep-water belts. Heard (waves), felt
        // (immersion + currents), seen (the moving line on the
        // shore), and ground-coupled for seismic species.
        14 => &[
            ModalityKind::AcousticWater,
            ModalityKind::Tactile,
            ModalityKind::VisualLight,
            ModalityKind::Seismic,
        ],
        // ============================================
        // Planet-archetype-specific phenomena.
        // Mirrors sim_recognition's per-template channel
        // declarations.
        // ============================================
        // cryovolcanism: visible plume + felt + ground-coupled +
        // thermal contrast against the cold surroundings.
        15 => &[
            ModalityKind::VisualLight,
            ModalityKind::Tactile,
            ModalityKind::Seismic,
            ModalityKind::InfraredThermal,
        ],
        // ice_quake: ground-shaking, audible, felt.
        16 => &[
            ModalityKind::Seismic,
            ModalityKind::Tactile,
            ModalityKind::AcousticAir,
        ],
        // pressure_storm: visible + audible + felt + EM-active.
        17 => &[
            ModalityKind::VisualLight,
            ModalityKind::AcousticAir,
            ModalityKind::Tactile,
            ModalityKind::ElectricField,
        ],
        // metallic_hydrogen_signal: deep-pressure phenomenon
        // visible only via field/EM/radio sensing.
        18 => &[
            ModalityKind::ElectricField,
            ModalityKind::MagneticSense,
            ModalityKind::RadioNative,
        ],
        // piezoelectric_pulse: directly felt by electric/seismic
        // species; tactile coupling on ground.
        19 => &[
            ModalityKind::ElectricField,
            ModalityKind::Tactile,
            ModalityKind::Seismic,
        ],
        // magnetic_lodestone: felt by magnetic-sense species;
        // tactile through bioelectric pickup.
        20 => &[ModalityKind::MagneticSense, ModalityKind::Tactile],
        // hydrocarbon_seep: smell + visible (oily slick / sheen).
        21 => &[
            ModalityKind::ChemicalTaste,
            ModalityKind::ChemicalPheromone,
            ModalityKind::VisualLight,
        ],
        // superconductor_resonance: EM/field-sensing only.
        22 => &[
            ModalityKind::ElectricField,
            ModalityKind::MagneticSense,
            ModalityKind::RadioNative,
        ],
        // reducing_storm: visible + audible + chemical
        // signature + felt.
        23 => &[
            ModalityKind::ChemicalTaste,
            ModalityKind::VisualLight,
            ModalityKind::AcousticAir,
            ModalityKind::Tactile,
        ],
        // hazy_obscuration: limits visibility — visual species
        // notice the limitation; chemical species smell the haze.
        24 => &[ModalityKind::VisualLight, ModalityKind::ChemicalTaste],
        // ============================================
        // Seasonal templates.
        // ============================================
        // seasonal_thaw: visible (melting) + tactile (warming) +
        // thermal + chemical (water + life smell).
        25 => &[
            ModalityKind::VisualLight,
            ModalityKind::Tactile,
            ModalityKind::InfraredThermal,
            ModalityKind::ChemicalTaste,
        ],
        // polar_winter: tactile cold + thermal contrast +
        // visual (snow, frozen scenery).
        26 => &[
            ModalityKind::Tactile,
            ModalityKind::InfraredThermal,
            ModalityKind::VisualLight,
        ],
        // equatorial_wet: water-noise + tactile + visual + smell
        // of damp.
        27 => &[
            ModalityKind::VisualLight,
            ModalityKind::Tactile,
            ModalityKind::AcousticWater,
            ModalityKind::ChemicalTaste,
        ],
        // axial_extremum: peak cold (or peak heat); same sensory
        // surface as polar_winter / thermal_gradient.
        28 => &[
            ModalityKind::Tactile,
            ModalityKind::InfraredThermal,
            ModalityKind::VisualLight,
        ],
        // silicate_resonance — Silicate-substrate civs sense
        // crystalline lattice resonance via tactile / seismic /
        // EM-field channels. Visible-light is *not* the dominant
        // sense (their world is warm enough to glow but the
        // information channel is structural).
        29 => &[
            ModalityKind::Tactile,
            ModalityKind::Seismic,
            ModalityKind::ElectricField,
            ModalityKind::RadioNative,
        ],
        // methane_seep — Hydrocarbon-substrate civs taste /
        // smell / hear bubbling methane / ethane.
        30 => &[
            ModalityKind::ChemicalTaste,
            ModalityKind::ChemicalPheromone,
            ModalityKind::AcousticAir,
        ],
        // ammoniacal_storm — Ammoniacal-substrate civs taste /
        // hear / feel ammonia weather.
        31 => &[
            ModalityKind::ChemicalTaste,
            ModalityKind::AcousticAir,
            ModalityKind::Tactile,
        ],
        // tidally_locked_terminator — civs cluster at the
        // mild-temperature terminator band; species sense it
        // primarily via vision (the perpetual sunset / sunrise
        // band) and thermal contrast.
        32 => &[
            ModalityKind::VisualLight,
            ModalityKind::InfraredThermal,
            ModalityKind::Tactile,
        ],
        // cryo_lake — Hydrocarbon civs sense liquid-methane
        // bodies via underwater acoustics + cold-tactile + chemical
        // taste.
        33 => &[
            ModalityKind::AcousticWater,
            ModalityKind::Tactile,
            ModalityKind::ChemicalTaste,
        ],
        // crystal_growth — Silicate civs sense ordered
        // crystalline structure via tactile / seismic / EM-field.
        34 => &[
            ModalityKind::Tactile,
            ModalityKind::Seismic,
            ModalityKind::ElectricField,
        ],
        // aurora_polar — visible auroral display + EM /
        // magnetic / radio signal. Cross-substrate.
        35 => &[
            ModalityKind::VisualLight,
            ModalityKind::ElectricField,
            ModalityKind::MagneticSense,
            ModalityKind::RadioNative,
        ],
        // ============================================
        // Per-substrate surface-solvent templates (Sprint 2 Item 8).
        // surface_solvent_water (50): Earth-equivalent surface
        // water — same channel set as the legacy `surface_water`
        // (id 5): visual body, tactile contact.
        50 => &[ModalityKind::VisualLight, ModalityKind::Tactile],
        // surface_solvent_ammonia (51): liquid-ammonia ponds.
        // Ammonia is pungent (ChemicalTaste), the cold liquid is
        // felt (Tactile), and the surface acts as an acoustic
        // medium for the cold reducing atmosphere (AcousticAir).
        51 => &[
            ModalityKind::ChemicalTaste,
            ModalityKind::Tactile,
            ModalityKind::AcousticAir,
        ],
        // surface_solvent_methane (52): Titan-style cryogenic
        // methane lakes. Underwater acoustics carry through the
        // liquid; tactile contact with cryogenic liquid is
        // unmistakable; chemical-taste identifies the hydrocarbon.
        52 => &[
            ModalityKind::AcousticWater,
            ModalityKind::Tactile,
            ModalityKind::ChemicalTaste,
        ],
        // surface_solvent_silicate_melt (53): magma lakes. Heat
        // and viscous flow are felt (Tactile); convective stirring
        // is ground-coupled (Seismic); the melt glows in
        // optical bands (VisualLight).
        53 => &[
            ModalityKind::Tactile,
            ModalityKind::Seismic,
            ModalityKind::VisualLight,
        ],
        _ => &[],
    }
}

/// Whether a planet's environment supports a given modality channel.
/// Drives the `env_gate` filter at species derivation; sub-surface
/// ocean and tidally-locked / atmosphere-less worlds seed visibly
/// different sensoria from Earth-like ones. Match arms enumerated
/// per channel for readability even where adjacent arms produce the
/// same result.
#[allow(clippy::match_same_arms)]
pub fn modality_supported(kind: ModalityKind, planet: &Planet) -> bool {
    match kind {
        ModalityKind::AcousticAir => planet.atmosphere != Atmosphere::None,
        ModalityKind::AcousticWater => matches!(
            planet.composition,
            Composition::OceanWorld | Composition::SubSurfaceOcean
        ),
        ModalityKind::Seismic => planet.composition != Composition::GaseousShell,
        ModalityKind::VisualLight => {
            planet.stellar_luminosity > Real::from_int(200)
                && planet.composition != Composition::SubSurfaceOcean
        }
        ModalityKind::VisualPolarization => {
            planet.atmosphere != Atmosphere::None
                && planet.stellar_luminosity > Real::from_int(200)
                && planet.composition != Composition::SubSurfaceOcean
        }
        ModalityKind::Bioluminescent => planet.biosphere != BiosphereClass::None,
        ModalityKind::ChemicalPheromone => planet.atmosphere != Atmosphere::None,
        ModalityKind::ChemicalTaste => planet.biosphere != BiosphereClass::None,
        ModalityKind::Tactile => true,
        ModalityKind::ElectricField => {
            planet.atmosphere != Atmosphere::None
                || matches!(
                    planet.composition,
                    Composition::OceanWorld | Composition::SubSurfaceOcean
                )
        }
        ModalityKind::MagneticSense => planet.magnetosphere != Magnetosphere::None,
        ModalityKind::InfraredThermal => true,
        // Native radio reception requires both a magnetosphere (so the
        // planet has a coupled EM environment) and an atmosphere thin
        // enough to propagate. Strong magnetosphere is the cleanest
        // gate; refine as sensorium-tech extensions land.
        ModalityKind::RadioNative => planet.magnetosphere == Magnetosphere::Strong,
        ModalityKind::Gestural => {
            planet.stellar_luminosity > Real::from_int(200)
                && planet.composition != Composition::SubSurfaceOcean
        }
        ModalityKind::Postural => true,
    }
}

/// Deterministic species name from the planet seed. 64
/// creature-feeling stems × 4 plural endings → ~256 distinct
/// names before collisions. Same seed → same name. The XOR with
/// a different magic constant from the planet name keeps the two
/// pools independent (so a `Vela-c` planet doesn't mechanically
/// imply a `Vela`-stem species).
#[must_use]
pub fn species_name_from_seed(seed: u64) -> String {
    const STEMS: [&str; 64] = [
        "Kelv", "Tolak", "Velum", "Korin", "Sephar", "Drylan", "Morak", "Phaen", "Iskor", "Hadrun",
        "Quil", "Brask", "Cyran", "Delph", "Erin", "Faun", "Goran", "Hesp", "Iril", "Jolm", "Karn",
        "Lumin", "Marr", "Nyl", "Olar", "Pyrr", "Quor", "Reln", "Sarn", "Tyr", "Ulm", "Vorn",
        "Worth", "Xen", "Ylith", "Zorn", "Auri", "Belth", "Chir", "Doran", "Eshan", "Frev", "Glin",
        "Holm", "Itl", "Jork", "Kym", "Lael", "Munir", "Nev", "Osh", "Polin", "Quet", "Reth",
        "Solm", "Tav", "Ulir", "Vern", "Whan", "Xelt", "Yarn", "Zelv", "Aral", "Brun",
    ];
    const ENDINGS: [&str; 4] = ["ites", "ans", "i", "ar"];
    // Different XOR than the planet-name hash so the two pools
    // pick independently.
    let mixed = seed ^ 0xFEED_FACE_BAAD_F00D;
    let stem_idx = usize::try_from(mixed % (STEMS.len() as u64)).unwrap_or(0);
    let end_idx = usize::try_from((mixed >> 6) % (ENDINGS.len() as u64)).unwrap_or(0);
    format!("{}{}", STEMS[stem_idx], ENDINGS[end_idx])
}

/// Per-seed cosmology pole-position bias. Derives a starting
/// `[empirical, communitarian, reformist, mystical, hierarchical]`
/// vector from species traits + planet — so different species
/// enter the same five-axis debate from different starting
/// positions. Each component is clamped to `[-0.50, +0.50]` so
/// the bias never out-shouts in-life drift (which can reach ±1.0).
///
/// **Bias rules** (additive, then clamped):
///
/// - **Sociality**:
///   high (>0.6) → +communitarian +0.20; low (<0.3) → -communitarian -0.10
/// - **Cognition**: high (>0.6) → +empirical +0.15; low (<0.3) →
///   +mystical +0.10
/// - **Communication fidelity**: low (<0.4) → +mystical +0.15
///   (oral / sensory traditions tilt toward mystery rather than
///   shared canon); high (>0.7) → +empirical +0.10
/// - **Habitat**: aquatic → +communitarian +0.15 (water-bound
///   social structure); terrestrial → +reformist +0.05 (varied
///   biomes drive change)
/// - **Modality count**: rich sensorium (≥4) → +empirical +0.05
///   (multi-channel sensing motivates analytical disposition)
/// - **Axial tilt**: high (>30°) → +reformist +0.15 (seasons
///   drive change-orientation); low (<10°) → +communitarian +0.05
///   (stable seasons reinforce the status quo)
/// - **Crust**: rare-earth / piezoelectric → +empirical +0.05
///   (richer-physics planet biases toward analytical thinking)
///
/// Determinism: pure function of species + planet; no RNG;
/// reproducible per seed.
#[allow(clippy::match_same_arms)]
pub(crate) fn derive_initial_cosmology(
    cognition: Real,
    sociality: Real,
    communication_fidelity: Real,
    habitat: Habitat,
    planet: &Planet,
    modalities: &[Modality],
) -> [Real; 5] {
    let high = Real::percent(60);
    let mid_high = Real::percent(70);
    let low = Real::percent(30);
    let lower = Real::percent(40);

    let mut empirical = Real::ZERO;
    let mut communitarian = Real::ZERO;
    let mut reformist = Real::ZERO;
    let mut mystical = Real::ZERO;
    // Hierarchical stays at zero by default; the cosmology drift
    // mechanism reads catastrophe events to push hierarchical;
    // species-trait-based starting bias on the hierarchical axis
    // doesn't have a clean justification, so leave it at neutral.
    let hierarchical = Real::ZERO;

    // Sociality bias.
    if sociality > high {
        communitarian = communitarian + Real::percent(20);
    } else if sociality < low {
        communitarian = communitarian - Real::percent(10);
    }
    // Cognition bias.
    if cognition > high {
        empirical = empirical + Real::percent(15);
    } else if cognition < low {
        mystical = mystical + Real::percent(10);
    }
    // Communication-fidelity bias.
    if communication_fidelity < lower {
        mystical = mystical + Real::percent(15);
    } else if communication_fidelity > mid_high {
        empirical = empirical + Real::percent(10);
    }
    // Habitat bias.
    match habitat {
        Habitat::Aquatic => communitarian = communitarian + Real::percent(15),
        Habitat::Terrestrial => reformist = reformist + Real::percent(5),
        Habitat::Amphibious => {
            // A species that crosses domains is biased toward both
            // — half the habitat-specific bonuses on each axis.
            communitarian = communitarian + Real::percent(7);
            reformist = reformist + Real::percent(2);
        }
        Habitat::Airborne => reformist = reformist + Real::percent(5),
        // Subterranean: extremely communitarian (constant
        // close-quarters proximity), low reformism (stable
        // niche, slow change).
        Habitat::Subterranean => communitarian = communitarian + Real::percent(20),
        // Endolithic: even slower change; substrate-anchored
        // life is the canonical conservative-niche bias.
        Habitat::Endolithic => communitarian = communitarian + Real::percent(10),
    }
    // Modality-count bias. Rich sensorium → analytical.
    if modalities.len() >= 4 {
        empirical = empirical + Real::percent(5);
    }
    // Axial-tilt bias.
    let high_tilt = Real::from_int(30);
    let low_tilt = Real::from_int(10);
    if planet.axial_tilt_deg > high_tilt {
        reformist = reformist + Real::percent(15);
    } else if planet.axial_tilt_deg < low_tilt {
        communitarian = communitarian + Real::percent(5);
    }
    // Crust bias — exotic crusts present richer physics.
    match planet.crust {
        sim_world::Crust::RareEarth | sim_world::Crust::Piezoelectric => {
            empirical = empirical + Real::percent(5);
        }
        _ => {}
    }

    let cap = Real::percent(50);
    let neg_cap = -cap;
    let clamp = |v: Real| -> Real { v.max(neg_cap).min(cap) };
    [
        clamp(empirical),
        clamp(communitarian),
        clamp(reformist),
        clamp(mystical),
        hierarchical,
    ]
}

/// Derive a species' native habitat domain from its planet,
/// modalities, and manipulation modes. Rules, in precedence order:
///
/// - Sub-surface ocean / ocean-world planets → `Aquatic` when the
///   species has `AcousticWater` (anchored to having evolved in
///   water). Without `AcousticWater`, fall through.
/// - Has both `AcousticWater` and `AcousticAir` modalities (or has
///   `LimbGrasp` *and* `FluidJet`) → `Amphibious` (coastal
///   lifestyle, can cross both domains natively).
/// - Has `AcousticWater` and either `FluidJet` or `Tentacle`
///   manipulation but no `AcousticAir` → `Aquatic` regardless of
///   planet (deep-water cetacean / cephalopod analogue).
/// - Otherwise → `Terrestrial` (the default; fits land-evolved
///   species with `LimbGrasp` / `AcousticAir`).
pub(crate) fn derive_habitat(
    planet: &Planet,
    modalities: &[Modality],
    manipulations: &[Manipulation],
) -> Habitat {
    let has = |kind: ModalityKind| modalities.iter().any(|m| m.kind == kind);
    let has_manip = |kind: ManipulationKind| manipulations.iter().any(|m| m.kind == kind);

    let water_native = matches!(
        planet.composition,
        Composition::SubSurfaceOcean | Composition::OceanWorld
    );

    if water_native && has(ModalityKind::AcousticWater) && !has(ModalityKind::AcousticAir) {
        return Habitat::Aquatic;
    }
    if has(ModalityKind::AcousticWater) && has(ModalityKind::AcousticAir) {
        return Habitat::Amphibious;
    }
    if has_manip(ManipulationKind::LimbGrasp) && has_manip(ManipulationKind::FluidJet) {
        return Habitat::Amphibious;
    }
    if has(ModalityKind::AcousticWater)
        && (has_manip(ManipulationKind::FluidJet) || has_manip(ManipulationKind::Tentacle))
        && !has(ModalityKind::AcousticAir)
    {
        return Habitat::Aquatic;
    }
    // Airborne: land-evolved species that develops flight. Triggered
    // by air-acoustic sensing + a single delicate manipulator (the
    // "flight-forelimb" morphology — wings co-evolved with grasping)
    // on a non-water-native rocky planet. Narrow rule so most
    // species still derive to Terrestrial.
    if !water_native
        && has(ModalityKind::AcousticAir)
        && !has(ModalityKind::AcousticWater)
        && has_manip(ManipulationKind::LimbGrasp)
        && manipulations.len() == 1
    {
        return Habitat::Airborne;
    }
    Habitat::Terrestrial
}

pub(crate) fn sample_unit(rng: &mut ChaCha20Rng) -> Real {
    Real::from_ratio(i64::from(rng.gen_range(0..=1000_i32)), 1000)
}

/// `t0_loss` formula:
/// ```text
/// clamp(0.50 − 0.15·cog − 0.10·soc − 0.10·ln(1 + lifespan/70)
///       − 0.15·comm_fid, 0.05, 0.70)
/// ```
pub(crate) fn compute_t0_loss(cog: Real, soc: Real, lifespan_y: Real, comm_fid: Real) -> Real {
    let lifespan_norm = lifespan_y / Real::from_int(70);
    let log_term = ln(Real::ONE + lifespan_norm);
    let raw = Real::percent(50)
        - Real::percent(15) * cog
        - Real::percent(10) * soc
        - Real::percent(10) * log_term
        - Real::percent(15) * comm_fid;
    raw.max(Real::percent(5)).min(Real::percent(70))
}

pub(crate) fn sample_modalities(planet: &Planet, rng: &mut ChaCha20Rng) -> Vec<Modality> {
    let mut available: Vec<ModalityKind> = ModalityKind::ALL
        .into_iter()
        .filter(|m| modality_supported(*m, planet))
        .collect();

    // Biosphere richness sets channel count: a hyper-biodiverse niche
    // selects for richer sensoria, a sparse one for narrower.
    let target = match planet.biosphere {
        BiosphereClass::HyperBiodiverse => rng.gen_range(5..=7),
        BiosphereClass::Lush => rng.gen_range(3..=5),
        BiosphereClass::Sparse => rng.gen_range(2..=3),
        BiosphereClass::None => 1,
    }
    .min(available.len());

    let mut chosen = Vec::with_capacity(target);

    // Tactile is the universal baseline: any biosphere with at least
    // one channel includes it. Drop it from the pool first so it
    // doesn't get selected twice. Without this, perfectly viable
    // sensoria can end up unable to perceive any of the canonical
    // recognition templates because the random pick missed every
    // visual / thermal / electric channel — and a species that
    // perceives nothing has no observations to seed discoveries on.
    if target >= 1 {
        if let Some(pos) = available.iter().position(|&m| m == ModalityKind::Tactile) {
            available.swap_remove(pos);
            chosen.push(default_modality(ModalityKind::Tactile));
        }
    }
    while chosen.len() < target {
        if available.is_empty() {
            break;
        }
        let i = rng.gen_range(0..available.len());
        chosen.push(default_modality(available.swap_remove(i)));
    }
    chosen
}

#[allow(clippy::too_many_lines)]
pub(crate) fn default_modality(kind: ModalityKind) -> Modality {
    // Per-channel M3 baselines. Range in metres, fidelity in [0, 1],
    // bandwidth in arbitrary signal units pending tuning.
    let (range_m, fidelity, bandwidth) = match kind {
        ModalityKind::AcousticAir => (
            Real::from_int(100),
            Real::from_ratio(7, 10),
            Real::from_int(20),
        ),
        ModalityKind::AcousticWater => (
            Real::from_int(2000),
            Real::from_ratio(8, 10),
            Real::from_int(10),
        ),
        ModalityKind::Seismic => (
            Real::from_int(50),
            Real::from_ratio(4, 10),
            Real::from_int(2),
        ),
        ModalityKind::VisualLight => (
            Real::from_int(5000),
            Real::from_ratio(9, 10),
            Real::from_int(60),
        ),
        ModalityKind::VisualPolarization => (
            Real::from_int(2000),
            Real::from_ratio(6, 10),
            Real::from_int(30),
        ),
        ModalityKind::Bioluminescent => (
            Real::from_int(50),
            Real::from_ratio(5, 10),
            Real::from_int(10),
        ),
        ModalityKind::ChemicalPheromone => {
            (Real::from_int(500), Real::from_ratio(5, 10), Real::ONE)
        }
        ModalityKind::ChemicalTaste => (Real::ONE, Real::from_ratio(8, 10), Real::ONE),
        ModalityKind::Tactile => (Real::ONE, Real::from_ratio(9, 10), Real::from_int(50)),
        ModalityKind::ElectricField => (
            Real::from_int(2),
            Real::from_ratio(7, 10),
            Real::from_int(20),
        ),
        ModalityKind::MagneticSense => (Real::from_int(1000), Real::from_ratio(3, 10), Real::ZERO),
        ModalityKind::InfraredThermal => (
            Real::from_int(50),
            Real::from_ratio(6, 10),
            Real::from_int(5),
        ),
        ModalityKind::RadioNative => (
            Real::from_int(10_000),
            Real::from_ratio(2, 10),
            Real::from_int(100),
        ),
        ModalityKind::Gestural => (
            Real::from_int(20),
            Real::from_ratio(8, 10),
            Real::from_int(10),
        ),
        ModalityKind::Postural => (
            Real::from_int(5),
            Real::from_ratio(7, 10),
            Real::from_int(5),
        ),
    };
    Modality {
        kind,
        range_m,
        fidelity,
        bandwidth,
    }
}

pub(crate) fn sample_manipulation(planet: &Planet, rng: &mut ChaCha20Rng) -> Vec<Manipulation> {
    // Composition gates the candidate body plans: limbs and beaks on
    // rocky land worlds; tentacles and jets on aquatic; pneumatic
    // shapes on gaseous. Tool-use and chemical secretion are universal.
    let candidates: Vec<ManipulationKind> = match planet.composition {
        Composition::OceanWorld | Composition::SubSurfaceOcean => vec![
            ManipulationKind::Tentacle,
            ManipulationKind::MouthBeak,
            ManipulationKind::FluidJet,
            ManipulationKind::ToolExtension,
            ManipulationKind::ElectricDischarge,
            ManipulationKind::ChemicalSecretion,
        ],
        Composition::GaseousShell => vec![
            ManipulationKind::FluidJet,
            ManipulationKind::WebConstruct,
            ManipulationKind::ToolExtension,
            ManipulationKind::ChemicalSecretion,
        ],
        Composition::Rocky => vec![
            ManipulationKind::LimbGrasp,
            ManipulationKind::Tentacle,
            ManipulationKind::MouthBeak,
            ManipulationKind::TonguePrehensile,
            ManipulationKind::Trunk,
            ManipulationKind::Mandible,
            ManipulationKind::ToolExtension,
            ManipulationKind::WebConstruct,
            ManipulationKind::Burrow,
            ManipulationKind::ChemicalSecretion,
        ],
    };

    let target = match planet.biosphere {
        BiosphereClass::HyperBiodiverse => rng.gen_range(2..=4),
        BiosphereClass::Lush => rng.gen_range(1..=3),
        BiosphereClass::Sparse | BiosphereClass::None => 1,
    }
    .min(candidates.len());

    let mut available = candidates;
    let mut chosen = Vec::with_capacity(target);
    for _ in 0..target {
        if available.is_empty() {
            break;
        }
        let i = rng.gen_range(0..available.len());
        chosen.push(default_manipulation(available.swap_remove(i)));
    }
    chosen
}

pub(crate) fn default_manipulation(kind: ManipulationKind) -> Manipulation {
    let (force_n, precision_m, dexterity_score, dof_count) = match kind {
        ManipulationKind::LimbGrasp => (
            Real::from_int(200),
            Real::percent(1),
            Real::from_ratio(8, 10),
            5,
        ),
        ManipulationKind::Tentacle => (
            Real::from_int(50),
            Real::from_ratio(1, 1000),
            Real::from_ratio(9, 10),
            8,
        ),
        ManipulationKind::MouthBeak => (
            Real::from_int(80),
            Real::from_ratio(1, 500),
            Real::from_ratio(7, 10),
            2,
        ),
        ManipulationKind::TonguePrehensile => (
            Real::from_int(20),
            Real::from_ratio(1, 1000),
            Real::from_ratio(8, 10),
            4,
        ),
        ManipulationKind::Trunk => (
            Real::from_int(300),
            Real::percent(1),
            Real::from_ratio(9, 10),
            6,
        ),
        ManipulationKind::Mandible => (
            Real::from_int(150),
            Real::from_ratio(1, 1000),
            Real::from_ratio(6, 10),
            2,
        ),
        ManipulationKind::FluidJet => (
            Real::from_int(40),
            Real::from_ratio(1, 10),
            Real::from_ratio(3, 10),
            1,
        ),
        ManipulationKind::ToolExtension => (
            Real::from_int(500),
            Real::from_ratio(1, 10_000),
            Real::ONE,
            10,
        ),
        ManipulationKind::WebConstruct => (
            Real::from_int(5),
            Real::from_ratio(1, 10_000),
            Real::from_ratio(7, 10),
            3,
        ),
        ManipulationKind::Burrow => (
            Real::from_int(1000),
            Real::from_ratio(1, 10),
            Real::from_ratio(4, 10),
            2,
        ),
        ManipulationKind::ElectricDischarge => (
            Real::from_int(10),
            Real::percent(1),
            Real::from_ratio(2, 10),
            1,
        ),
        ManipulationKind::ChemicalSecretion => {
            (Real::ONE, Real::percent(1), Real::from_ratio(3, 10), 1)
        }
    };
    Manipulation {
        kind,
        force_n,
        precision_m,
        dexterity_score,
        dof_count,
    }
}

/// Derive `PopulationBiology` from already-sampled species traits.
/// Maps coarsely-correlated biological intuitions to numeric fields
/// without sampling new randomness — the function is deterministic
/// in the species seed via its inputs.
///
/// The mapping picks an r/K-strategy axis from
/// `(sociality, lifespan, manipulation_kind)` and then derives
/// `clutch_size`, bracket fractions, and survival rates so the four
/// lever pulls together: a low-sociality short-lived web-spinner
/// lands on r-strategy (large clutch, low juvenile survival, no
/// elders), while a high-sociality long-lived limb-grasper lands
/// on K-strategy (clutch=1, high juvenile survival, substantial
/// elder period). Habitat shifts the r/K axis modestly: aquatic
/// species favour r (broadcast spawning), airborne favour K
/// (parental investment). Cognition topology nudges
/// `maturity_fraction`: `Centralized` species have longer brain-
/// development windows.
///
/// All outputs are clamped so `fertile_fraction >= 0.30` and
/// per-bracket survival stays in plausible ranges; the per-bracket
/// food multipliers are pinned constants since within-species
/// variance there is too small to surface meaningfully.
#[must_use]
pub fn derive_population_biology(
    cognition: Real,
    sociality: Real,
    lifespan_years: Real,
    habitat: Habitat,
    cognition_topology: CognitionTopology,
    manipulation_modes: &[Manipulation],
) -> PopulationBiology {
    // r/K strategy axis in [0, 1]. 0 = pure K (single-offspring,
    // high parental investment, long maturation, elders); 1 = pure r
    // (large clutch, broadcast, no elders).
    //
    // Drivers, all weighted equally:
    //  - low sociality => r (1 - sociality)
    //  - short lifespan => r (clamp(1 - lifespan/100, 0, 1))
    //  - r-leaning manipulation => r (chemical secretion, web,
    //    fluid jet, mandible — all small-bodied / broadcasting
    //    body plans)
    //  - aquatic habitat => +0.10 r (broadcast spawning)
    //  - airborne habitat => -0.10 r (K — most flying species
    //    invest heavily per offspring; brood sizes are small
    //    even where the body plan would otherwise suggest r)
    let lifespan_term = {
        let scaled = lifespan_years / Real::from_int(100);
        let bounded = scaled.clamp01();
        Real::ONE - bounded
    };
    let manip_r_lean = manipulation_r_lean(manipulation_modes);
    let habitat_r_shift = match habitat {
        Habitat::Aquatic => Real::percent(10),
        Habitat::Airborne => -Real::percent(10),
        _ => Real::ZERO,
    };
    let r_axis_raw = (Real::ONE - sociality) * Real::from_ratio(1, 3)
        + lifespan_term * Real::from_ratio(1, 3)
        + manip_r_lean * Real::from_ratio(1, 3)
        + habitat_r_shift;
    let r_axis = r_axis_raw.clamp01();
    // Clutch size: 1 + r_axis² × 4999 (range [1, 5000]). Quadratic
    // ramp so the middle of the r/K axis stays modest (clutch ~1250
    // at r_axis=0.5) and the high end can hit true broadcast-spawner
    // scale (salmon / cod / coral lay 5k+ eggs per spawn). The
    // earlier 500-cap clipped the r-strategist tail; raised to 5000
    // so r=1 species can hit real-organism magnitudes.
    let clutch_size = Real::ONE + r_axis * r_axis * Real::from_int(4999);
    // Infant fraction in [0.01, 0.10]; K-strategists have slightly
    // longer infancy (more parental care, slower growth).
    let infant_fraction = Real::percent(1) + (Real::ONE - r_axis) * Real::percent(9);
    // Maturity fraction in [0.04, 0.40]; K-strategists with
    // Centralized cognition get the long brain-development window.
    let centralized_bonus = match cognition_topology {
        CognitionTopology::Centralized => Real::percent(5),
        CognitionTopology::DistributedRedundant
        | CognitionTopology::Collective
        | CognitionTopology::Acentric => Real::ZERO,
    };
    let maturity_base = Real::percent(4) + (Real::ONE - r_axis) * Real::percent(31);
    let maturity_fraction = (maturity_base + centralized_bonus).min(Real::percent(40));
    // Eldership fraction in [0, 0.30]; only social + smart species
    // evolve a meaningful post-reproductive period. Pure r-strategists
    // have zero elders.
    let elder_drive = sociality * cognition;
    let eldership_fraction =
        (elder_drive * Real::percent(30) * (Real::ONE - r_axis)).min(Real::percent(30));
    // Clamp the fertile_fraction floor *per-strategy*: K-strategists
    // (`r_axis = 0`) keep the historical 0.30 floor — a meaningful
    // reproductive window across their whole life history. Hyper-r-
    // strategists (`r_axis = 1`) drop to a 0.10 floor — a mayfly /
    // semelparous-insect equivalent has hours of fertile life as a
    // fraction of total life, not 30 % of it. Linear interpolation
    // between. The prior hardcoded 0.30 floor was a vertebrate
    // assumption that erased true r-strategists; this preserves
    // numerical safety while allowing biologically valid extremes.
    let fertile_min = Real::percent(30) - r_axis * Real::percent(20);
    let total_non_fertile = infant_fraction + maturity_fraction + eldership_fraction;
    let allowed_non_fertile = Real::ONE - fertile_min;
    let (maturity_fraction, eldership_fraction) = if total_non_fertile > allowed_non_fertile {
        let overflow = total_non_fertile - allowed_non_fertile;
        let new_maturity = (maturity_fraction - overflow).max(Real::percent(4));
        let still_over =
            (infant_fraction + new_maturity + eldership_fraction) - allowed_non_fertile;
        let new_eldership = if still_over > Real::ZERO {
            (eldership_fraction - still_over).max(Real::ZERO)
        } else {
            eldership_fraction
        };
        (new_maturity, new_eldership)
    } else {
        (maturity_fraction, eldership_fraction)
    };
    // Infant survival in [0.05, 0.95], inversely correlated with
    // r_axis (r-strategists invest little per offspring).
    let infant_survival = Real::percent(5) + (Real::ONE - r_axis) * Real::percent(90);
    // Juvenile survival in [0.20, 0.99], same shape but compressed
    // — even r-strategists' juveniles (the ones that survived
    // infancy) have decent prospects.
    let juvenile_survival = Real::percent(20) + (Real::ONE - r_axis) * Real::percent(79);
    // Per-bracket food multipliers: pinned. infants 0.30, juveniles
    // 0.60, fertile 1.00, elder 0.90.
    let food_multipliers = [
        Real::percent(30),
        Real::percent(60),
        Real::ONE,
        Real::percent(90),
    ];
    // Reproductive events across the fertile window.
    // K-strategist (r_axis = 0): 30 events — long-lived individuals
    // each reproduce many times (rat / elephant / human pattern).
    // r-strategist (r_axis = 1): 2 events — short-lived spawners
    // expend their reproductive budget in one or two pulses
    // (salmon / mayfly pattern). The semelparous extreme
    // (single spawn → die) is approximated as `2.0` rather than
    // `1.0` so a small modelling buffer keeps the formula numerically
    // stable across the r-axis without collapsing to zero birth
    // multiplier at the high end.
    //
    // The product `clutch_size × events_per_window` therefore stays
    // bounded — at r_axis=1 it's `500 × 2 = 1000` rather than
    // unbounded; at r_axis=0 it's `1 × 30 = 30`. Per-month rate
    // `(clutch × events) / fertile_months` then sits in a sane
    // ~0.06..~83 range across the whole axis instead of the
    // unbounded ~0.0005..~417 of the legacy formula.
    let events_per_fertile_window =
        (Real::ONE - r_axis) * Real::from_int(30) + r_axis * Real::from_int(2);
    // Reproductive success: per-event probability of actually
    // producing the full clutch. K-strategists (r=0) → 0.5%; r-
    // strategists (r=1) → 10%. Quadratic blend of the two endpoints
    // (`0.005 × (1 - r)² + 0.10 × r²`) so the mid-axis stays below
    // the linear midpoint — a r=0.5 species sits at ~0.026 rather
    // than the linear 0.052, which keeps mid-axis lifetime offspring
    // out of the implausibly-high band that the linear curve
    // produced once the 5000-clutch cap landed. Without this, the
    // `clutch × events / fertile_months` rate overshot real human
    // K-strategist births by ~500×. With it, K-mammal birth rates
    // land in the ~0.001-0.01/mo range (real human ≈ 0.0005/mo
    // per fertile adult; we sit 2-5× above to leave headroom for
    // sociality-driven effective fertility). The recruit-ceiling
    // clamp at `step_with_capacity` becomes the rare safety net
    // rather than the load-bearing demographic limiter.
    let one_minus_r = Real::ONE - r_axis;
    let reproductive_success = one_minus_r * one_minus_r * Real::from_ratio(5, 1000)
        + r_axis * r_axis * Real::from_ratio(100, 1000);
    PopulationBiology {
        clutch_size,
        infant_fraction,
        maturity_fraction,
        eldership_fraction,
        infant_survival,
        juvenile_survival,
        food_multipliers,
        events_per_fertile_window,
        reproductive_success,
    }
}

/// Per-substrate default tolerance envelope (pre-jitter). Captures
/// the "baseline biology" each metabolic substrate's species is built
/// for. The per-species jitter applied in `derive_tolerance_envelope`
/// then shapes individual species into generalists or extremophiles
/// within the substrate's window.
///
/// Numbers per the implementation-plan Sprint 2 Item 7a spec.
#[must_use]
pub(crate) fn substrate_default_envelope(substrate: MetabolicSubstrate) -> ToleranceEnvelope {
    match substrate {
        // Aqueous: liquid water 273-373 K, near-neutral pH, modest
        // salinity, low radiation, Earth-surface pressure range.
        MetabolicSubstrate::Aqueous => ToleranceEnvelope {
            temp_range: (Real::from_int(273), Real::from_int(373)),
            ph_range: (Real::from_int(5), Real::from_int(9)),
            salinity_range: (Real::ZERO, Real::from_int(50)),
            radiation_max: Real::from_ratio(5, 10),
            pressure_range: (Real::from_ratio(5, 10), Real::from_int(2)),
        },
        // Ammoniacal: liquid ammonia regime, basic pH, higher salinity
        // tolerance (NH3 dissolves more), wider pressure band.
        MetabolicSubstrate::Ammoniacal => ToleranceEnvelope {
            temp_range: (Real::from_int(195), Real::from_int(240)),
            ph_range: (Real::from_int(9), Real::from_int(12)),
            salinity_range: (Real::ZERO, Real::from_int(100)),
            radiation_max: Real::from_ratio(8, 10),
            pressure_range: (Real::from_ratio(5, 10), Real::from_int(5)),
        },
        // Hydrocarbon: Titan-cold liquid methane/ethane, mildly acidic
        // (dissolved CO2 / organics), low salinity (poor solvent),
        // tolerates higher radiation + pressure.
        MetabolicSubstrate::Hydrocarbon => ToleranceEnvelope {
            temp_range: (Real::from_int(91), Real::from_int(117)),
            ph_range: (Real::from_int(3), Real::from_int(7)),
            salinity_range: (Real::ZERO, Real::from_int(10)),
            radiation_max: Real::from_ratio(12, 10),
            pressure_range: (Real::ONE, Real::from_int(10)),
        },
        // Silicate: crystalline lattice life — molten silicate
        // temperatures, pH irrelevant (full 0-14), high salinity /
        // radiation / pressure tolerance.
        MetabolicSubstrate::Silicate => ToleranceEnvelope {
            temp_range: (Real::from_int(1687), Real::from_int(3538)),
            ph_range: (Real::ZERO, Real::from_int(14)),
            salinity_range: (Real::ZERO, Real::from_int(200)),
            radiation_max: Real::from_int(5),
            pressure_range: (Real::ONE, Real::from_int(100)),
        },
    }
}

/// Derive the per-species `ToleranceEnvelope`. Starts from
/// `substrate_default_envelope` and applies ±20% jitter per axis,
/// derived deterministically from the species seed via a SplitMix64
/// hash. Each axis gets an independent offset so individual species
/// within a substrate end up with distinguishable envelopes —
/// extremophiles (high radiation_max, wide temperature span) sit
/// alongside generalists (narrow envelopes centred on the substrate
/// midpoint).
///
/// Determinism: pure function of `(seed, substrate)`; no RNG state;
/// reproducible per seed.
///
/// The jitter is applied symmetrically: low edges can move ±20% of
/// the range width inward/outward, high edges can move ±20% of the
/// range width inward/outward, and the `radiation_max` ceiling can
/// move ±20% of its value. Edges are then re-ordered so `lo <= hi`
/// stays an invariant even if the random offsets cross.
#[must_use]
pub(crate) fn derive_tolerance_envelope(
    seed: u64,
    substrate: MetabolicSubstrate,
) -> ToleranceEnvelope {
    let base = substrate_default_envelope(substrate);

    // SplitMix64-style hash of (seed, axis_idx). Same shape as
    // `CognitionAxes::from_scalar_with_seed` so the per-species
    // jitter stays deterministic + free of RNG-state coupling.
    fn axis_offset_signed(seed: u64, axis_idx: u64) -> Real {
        let mut z = seed.wrapping_add(axis_idx.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        // Low 16 bits → signed offset in [-1, +1); scale by 0.20.
        let bits = (z & 0xFFFF) as i64;
        let signed = bits - 32_768;
        // signed / 32768 in [-1, +1), scaled by 0.20.
        Real::from_ratio(signed * 20, 32_768 * 100)
    }

    let jitter_range = |idx_lo: u64, idx_hi: u64, (lo, hi): (Real, Real)| -> (Real, Real) {
        let width = hi - lo;
        let off_lo = axis_offset_signed(seed, idx_lo) * width;
        let off_hi = axis_offset_signed(seed, idx_hi) * width;
        let new_lo = lo + off_lo;
        let new_hi = hi + off_hi;
        // Preserve lo <= hi invariant even if the jitter crosses.
        if new_lo <= new_hi {
            (new_lo, new_hi)
        } else {
            (new_hi, new_lo)
        }
    };

    // Distinct axis indices per envelope dimension keep the offsets
    // independent across temperature / pH / salinity / pressure pairs
    // and the radiation ceiling.
    let temp_range = jitter_range(101, 102, base.temp_range);
    let ph_range = jitter_range(103, 104, base.ph_range);
    let salinity_range = jitter_range(105, 106, base.salinity_range);
    let pressure_range = jitter_range(109, 110, base.pressure_range);

    let rad_off = axis_offset_signed(seed, 107);
    let radiation_max = (base.radiation_max + base.radiation_max * rad_off).max(Real::ZERO);

    ToleranceEnvelope {
        temp_range,
        ph_range,
        salinity_range,
        radiation_max,
        pressure_range,
    }
}

/// Map planet composition → metabolic substrate fallback for callers
/// that don't have a `MetabolicSubstrate` available directly (older
/// `Planet` shapes pre-substrate-first sampler). Prefer reading
/// `planet.metabolic_substrate` whenever possible.
#[must_use]
#[allow(dead_code)]
pub(crate) fn substrate_for_planet(planet: &Planet) -> MetabolicSubstrate {
    planet.metabolic_substrate
}

/// Apply a catastrophe to a species and return the surviving
/// fraction in `[0, 1]`. Survival = `tolerance.match_score(local
/// conditions)` so a species with a tolerance envelope shaped to the
/// catastrophe's local conditions (high radiation, high temperature)
/// survives intact while species with envelopes that exclude those
/// conditions are wiped out. The cause is supplied as the *local
/// cell conditions during the catastrophe*; a radiation burst, for
/// instance, passes `rad` near or above the typical species'
/// `radiation_max` so only extremophiles match.
///
/// This is the catastrophe-survival multiplier referenced by Item
/// 7a. Synthetic for now (no full catastrophe pipeline yet); when
/// the catastrophe step lands it will read the same `match_score`
/// off `species.tolerance` to scale per-tick mortality.
#[must_use]
pub fn apply_catastrophe(
    tolerance: &ToleranceEnvelope,
    t: Real,
    ph: Real,
    sal: Real,
    rad: Real,
    p: Real,
) -> Real {
    tolerance.match_score(t, ph, sal, rad, p)
}

/// r-leaning score for a manipulation set in [0, 1]. Body plans
/// associated with broadcasting / small-body strategies score
/// higher; limbs / trunks / tongues score lower.
fn manipulation_r_lean(modes: &[Manipulation]) -> Real {
    if modes.is_empty() {
        return Real::percent(50);
    }
    let mut score = Real::ZERO;
    for m in modes {
        let per_mode = match m.kind {
            ManipulationKind::ChemicalSecretion
            | ManipulationKind::WebConstruct
            | ManipulationKind::FluidJet
            | ManipulationKind::Mandible
            | ManipulationKind::Burrow => Real::ONE,
            ManipulationKind::ElectricDischarge | ManipulationKind::MouthBeak => Real::percent(60),
            ManipulationKind::Tentacle | ManipulationKind::TonguePrehensile => Real::percent(40),
            ManipulationKind::Trunk
            | ManipulationKind::LimbGrasp
            | ManipulationKind::ToolExtension => Real::ZERO,
        };
        score = score + per_mode;
    }
    let n = i64::try_from(modes.len()).unwrap_or(1);
    score / Real::from_int(n)
}
