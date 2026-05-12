use super::*;
use sim_arith::Real;
use sim_recognition::ChannelKind;
use sim_species::ManipulationKind;
use sim_world::Crust;
use std::collections::{BTreeMap, BTreeSet};

fn species_with(channels: &[ChannelKind]) -> BTreeSet<ChannelKind> {
    channels.iter().copied().collect()
}

fn with_tool_extension() -> BTreeSet<ManipulationKind> {
    [ManipulationKind::ToolExtension].into_iter().collect()
}

#[test]
fn distance_imaging_blocked_for_no_visual_species() {
    let species = species_with(&[ChannelKind::Tactile, ChannelKind::AcousticWater]);
    assert!(!is_buildable(
        ToolKind::DistanceImaging,
        &species,
        &with_tool_extension(),
        true,
        true,
        Crust::Basaltic,
    ));
}

#[test]
fn distance_imaging_unlocks_for_visual_species() {
    let species = species_with(&[ChannelKind::VisualLight, ChannelKind::Tactile]);
    assert!(is_buildable(
        ToolKind::DistanceImaging,
        &species,
        &with_tool_extension(),
        true,
        true,
        Crust::Basaltic,
    ));
}

#[test]
fn magnetic_sensor_blocked_without_magnetosphere() {
    let species = species_with(&[ChannelKind::Tactile]);
    assert!(!is_buildable(
        ToolKind::MagneticSensor,
        &species,
        &with_tool_extension(),
        false,
        true,
        Crust::Basaltic,
    ));
}

#[test]
fn tier5_field_propulsion_blocked_on_basaltic_crust() {
    let species = species_with(&[ChannelKind::ElectricField, ChannelKind::Tactile]);
    assert!(!is_buildable(
        ToolKind::FieldPropulsionEngine,
        &species,
        &with_tool_extension(),
        true,
        true,
        Crust::Basaltic,
    ));
}

#[test]
fn tier5_field_propulsion_buildable_on_piezoelectric_crust() {
    let species = species_with(&[ChannelKind::ElectricField, ChannelKind::Tactile]);
    assert!(is_buildable(
        ToolKind::FieldPropulsionEngine,
        &species,
        &with_tool_extension(),
        true,
        true,
        Crust::Piezoelectric,
    ));
}

#[test]
fn magnetic_sensor_buildable_with_magnetosphere() {
    let species = species_with(&[ChannelKind::Tactile]);
    assert!(is_buildable(
        ToolKind::MagneticSensor,
        &species,
        &with_tool_extension(),
        true,
        true,
        Crust::Basaltic,
    ));
}

#[test]
fn field_sensor_blocked_without_em_medium() {
    let species = species_with(&[ChannelKind::Tactile]);
    assert!(!is_buildable(
        ToolKind::FieldSensor,
        &species,
        &with_tool_extension(),
        false,
        false,
        Crust::Basaltic,
    ));
}

/// FluidJet-only species: jet-propulsion biology (squid / salp /
/// archerfish analogues) reaches a substantial tech surface
/// through native body-plan affordances. The xenobiology audit
/// makes jet-propulsion the canonical `MotivePropulsion` path
/// (squid jet propulsion is literal motive propulsion) and admits
/// jet species to water-jet stoneworking (industrial water-cutting),
/// pressure-clamped experiments, lateral-line acoustic sensing,
/// chemical rocketry, and hydropower. Tools whose physical substrate
/// has no jet-biology analogue (precision optics, EM field
/// sensors, magnetic sensors, solid-state computation, the
/// transcendence trio's lattice / resonator pair) remain blocked.
#[test]
fn fluid_jet_species_reaches_diverse_paths_but_not_optical_or_lattice() {
    let species = species_with(&[ChannelKind::VisualLight, ChannelKind::Tactile]);
    let jet_only: BTreeSet<ManipulationKind> = [ManipulationKind::FluidJet].into_iter().collect();
    // Tier-1 + xenobiology-grounded mid-tier paths that accept
    // FluidJet should be buildable. (RemoteAcoustic also lists
    // FluidJet but is gated separately by `prereq_channels` on
    // AcousticAir / AcousticWater — covered by a separate channel-
    // gate test, not this one.)
    for tool in [
        ToolKind::RangedMomentumWeapon,
        ToolKind::FoodProcessing,
        ToolKind::FluidGathering,
        ToolKind::OrganizedHunting,
        ToolKind::FluidControl,
        ToolKind::MotivePropulsion,
        ToolKind::PowerGeneration,
        ToolKind::ExperimentApparatus,
    ] {
        assert!(
            is_buildable(tool, &species, &jet_only, true, true, Crust::Basaltic),
            "{tool:?} should accept a FluidJet-only species"
        );
    }
    // Tools whose physical substrate has no jet-biology analogue
    // remain blocked: precision optics, EM field / magnetic sensors,
    // solid-state digital computation, the transcendence trio's
    // lattice / resonator pair, thermochromic temperature sensing.
    for tool in [
        ToolKind::ThermalSensor,
        ToolKind::DistanceImaging,
        ToolKind::FieldSensor,
        ToolKind::MagneticSensor,
        ToolKind::DigitalComputation,
        ToolKind::BioelectricResonator,
        ToolKind::MetamaterialLattice,
    ] {
        assert!(
            !is_buildable(tool, &species, &jet_only, true, true, Crust::Basaltic),
            "{tool:?} should be blocked for a FluidJet-only species"
        );
    }
}

/// Coverage canary: every `ManipulationKind` variant must be
/// accepted by at least one tier-1 tool so no random species
/// generation outcome leaves a species frozen at zero tools. The
/// prior global `MANIPULATION_PREREQ = ToolExtension` gate routinely
/// produced no-tool species on Sparse-biosphere worlds; the per-tool
/// table guarantees every body plan has an applied-knowledge entry
/// point.
#[test]
fn every_manipulation_kind_has_tier1_path() {
    let species = species_with(&[ChannelKind::Tactile]);
    let tier1 = [
        ToolKind::LocalisedCombustion,
        ToolKind::ContactWeapon,
        ToolKind::RangedMomentumWeapon,
        ToolKind::SimpleShelter,
        ToolKind::FoodProcessing,
        ToolKind::FluidGathering,
        ToolKind::BasicTextiles,
        ToolKind::StoneWorking,
        ToolKind::OrganizedHunting,
        ToolKind::BasicHealing,
    ];
    let all_kinds = [
        ManipulationKind::LimbGrasp,
        ManipulationKind::Tentacle,
        ManipulationKind::MouthBeak,
        ManipulationKind::TonguePrehensile,
        ManipulationKind::Trunk,
        ManipulationKind::Mandible,
        ManipulationKind::FluidJet,
        ManipulationKind::ToolExtension,
        ManipulationKind::WebConstruct,
        ManipulationKind::Burrow,
        ManipulationKind::ElectricDischarge,
        ManipulationKind::ChemicalSecretion,
    ];
    for kind in all_kinds {
        let manips: BTreeSet<ManipulationKind> = [kind].into_iter().collect();
        let reachable = tier1.iter().any(|tool| {
            is_buildable(*tool, &species, &manips, true, true, Crust::Basaltic)
        });
        assert!(
            reachable,
            "{kind:?} must have at least one tier-1 tool path — the \
             per-tool manipulation_prereqs table left this body plan \
             with zero tier-1 entries"
        );
    }
}

/// ExperimentApparatus accepts every manipulation mode. A clamp-
/// and-measure rig is a function (hold a channel at a known
/// value, observe response), not a specific physical form: every
/// body plan can build one with its native affordance —
/// ChemicalSecretion runs controlled-concentration baths,
/// WebConstruct weaves a calibrated chamber, FluidJet holds a
/// pressure clamp, ElectricDischarge clamps field strength,
/// Burrow excavates a controlled-volume cell. The substrate gate
/// (confirmed `fire`) plus the per-channel clamp ladders already
/// encode "which experiments are meaningful here."
#[test]
fn apparatus_accepts_every_manipulation_kind() {
    let species = species_with(&[ChannelKind::Tactile]);
    let all_kinds = [
        ManipulationKind::LimbGrasp,
        ManipulationKind::Tentacle,
        ManipulationKind::MouthBeak,
        ManipulationKind::TonguePrehensile,
        ManipulationKind::Trunk,
        ManipulationKind::Mandible,
        ManipulationKind::FluidJet,
        ManipulationKind::ToolExtension,
        ManipulationKind::WebConstruct,
        ManipulationKind::Burrow,
        ManipulationKind::ElectricDischarge,
        ManipulationKind::ChemicalSecretion,
    ];
    for kind in all_kinds {
        let manips: BTreeSet<ManipulationKind> = [kind].into_iter().collect();
        assert!(
            is_buildable(
                ToolKind::ExperimentApparatus,
                &species,
                &manips,
                true,
                true,
                Crust::Basaltic,
            ),
            "ExperimentApparatus must accept {kind:?} — a clamp-and-\
             measure rig is a function, not a body-plan-specific form"
        );
    }
    // Empty manipulation set is still rejected — the species needs
    // *some* deliberate-state affordance.
    let empty: BTreeSet<ManipulationKind> = BTreeSet::new();
    assert!(!is_buildable(
        ToolKind::ExperimentApparatus,
        &species,
        &empty,
        true,
        true,
        Crust::Basaltic,
    ));
}

#[test]
fn time_gate_respects_tier() {
    // TIER_UNLOCK_PERIOD_TICKS scales with the months-per-
    // year unit; tier N unlocks at `tier × period` ticks.
    let p = TIER_UNLOCK_PERIOD_TICKS;
    assert!(!time_gate_open(ToolKind::ThermalSensor, p / 2));
    assert!(time_gate_open(ToolKind::ThermalSensor, 2 * p));
    assert!(!time_gate_open(ToolKind::DistanceImaging, 3 * p - 1));
    assert!(time_gate_open(ToolKind::DistanceImaging, 3 * p));
    assert!(!time_gate_open(ToolKind::MagneticSensor, 4 * p - 1));
    assert!(time_gate_open(ToolKind::MagneticSensor, 4 * p));
}

#[test]
fn ids_are_unique() {
    let mut seen = BTreeSet::new();
    for t in ToolKind::ALL {
        assert!(seen.insert(t.id()));
    }
}

/// `tool_prereqs` must form a DAG. The cheap structural
/// invariant we enforce — every prereq has a strictly lower
/// `tier()` than the dependent tool — guarantees acyclicity
/// without an explicit visit-mark traversal: a cycle would
/// require A's tier < B's tier < A's tier, contradiction. The
/// test is a forward-compatibility canary for Agent-B's tool
/// additions (+).
#[test]
fn tool_prereqs_form_a_dag() {
    for tool in ToolKind::ALL {
        for prereq in tool.tool_prereqs() {
            assert!(
                prereq.tier() < tool.tier(),
                "tool_prereq cycle / tier-inversion: {tool:?} (tier {}) \
                 lists {prereq:?} (tier {}) as a prereq; prereq tier \
                 must be strictly lower than the dependent tool's tier",
                tool.tier(),
                prereq.tier(),
            );
            // Self-loop check (subsumed by tier check, but
            // explicit for readability).
            assert_ne!(*prereq, tool, "{tool:?} cannot list itself as a prereq");
        }
    }
}

/// every `relation_prereqs` `template_id` must exist in
/// the `earth_like_default` recognition library — otherwise the
/// prereq is structurally unsatisfiable.
#[test]
fn relation_prereq_template_ids_are_real() {
    let lib = sim_recognition::RecognitionLibrary::earth_like_default();
    let known: BTreeSet<u32> = lib.templates.iter().map(|t| t.id).collect();
    for tool in ToolKind::ALL {
        for (tid, _ch) in tool.relation_prereqs() {
            assert!(
                known.contains(tid),
                "{tool:?} lists template_id {tid} as a relation prereq, \
                 but no such template exists in the earth_like_default library"
            );
        }
    }
}

/// tier-5: ids 49-58, all tier 5 (information-age — sits
/// alongside the pre-existing transcendence trio at ids 6-8).
#[test]
fn tier5_information_age_ids_are_in_reserved_range() {
    let tier5_information_age = [
        ToolKind::DigitalComputation,
        ToolKind::InformationNetworking,
        ToolKind::GeneticManipulation,
        ToolKind::OrbitalReach,
        ToolKind::AdvancedMedicine,
        ToolKind::MaterialFabrication,
        ToolKind::AutonomousSystems,
        ToolKind::EnergyStorage,
        ToolKind::CryogenicEngineering,
        ToolKind::OrganicSynthesis,
    ];
    for t in tier5_information_age {
        assert_eq!(t.tier(), 5, "{t:?} should be tier 5");
        assert!(
            (49..=58).contains(&t.id()),
            "{t:?} id {} not in reserved tier-5 range 49-58",
            t.id()
        );
    }
    // Pre-existing transcendence trio also tier 5; pinned for
    // completeness.
    for t in ToolKind::TIER_FIVE {
        assert_eq!(t.tier(), 5);
    }
}

/// final substrate-divergence canary: a no-fire civ
/// CANNOT reach `OrbitalReach` (combustion-locked through
/// `AerialTransport`'s `MaterialRefining` requirement), but
/// CAN reach `DigitalComputation` (alternate path:
/// `AnalyticalEngines` chains through `PrecisionTimekeeping` +
/// `AbstractMathematics`, none of which require fire). The
/// information age is reachable for no-fire civs, but the
/// off-world era is not.
#[test]
fn no_fire_seed_reaches_information_age_but_not_orbit() {
    use crate::discovery::ConfirmedRelation;
    let species = species_with(&[ChannelKind::Tactile, ChannelKind::VisualLight]);
    let mut confirmed: BTreeMap<u32, ConfirmedRelation> = BTreeMap::new();
    // Confirmed tidal_extremum (gravity) AND lightning_buildup
    // (EM) — no-fire civ that has otherwise-rich science.
    for (rid, tid) in [(4001u32, 14u32), (4002, 2)] {
        confirmed.insert(
            rid,
            ConfirmedRelation {
                relation_id: rid,
                template_id: tid,
                channel: crate::discovery::Channel::Temperature,
                form: crate::forms::Form::Linear,
                params: vec![Real::from_int(1)],
                residual: Real::ZERO,
                confidence: Real::ONE,
                n_samples: 32,
                confirmed_at_tick: 1000,
                low_confidence_streak: 0,
                cooldown_until: 0,
                refinement: None,
                initial_residual: Real::ZERO,
                falsification_streak: 0,
                inherited_from_tick: None,
                inherited_from_civ_id: None,
            },
        );
    }
    // Civ has unlocked the no-fire-friendly tier 1→4 chain that
    // reaches DigitalComputation: StoneWorking, MechanicalAdvantage,
    // CulturalEncoding (via BasicTextiles), AbstractMathematics,
    // PrecisionTimekeeping, AnalyticalEngines.
    let mut unlocked: BTreeSet<ToolKind> = BTreeSet::new();
    for t in [
        ToolKind::StoneWorking,
        ToolKind::BasicTextiles,
        ToolKind::CulturalEncoding,
        ToolKind::MechanicalAdvantage,
        ToolKind::PrecisionTimekeeping,
        ToolKind::AbstractMathematics,
        ToolKind::AnalyticalEngines,
    ] {
        unlocked.insert(t);
    }
    let mature_lit = Real::from_ratio(75, 100);

    // DigitalComputation IS reachable.
    assert!(is_unlocked(
        ToolKind::DigitalComputation,
        &species,
        &with_tool_extension(),
        true,
        true,
        Crust::Basaltic,
        10_000_000,
        mature_lit,
        &confirmed,
        &unlocked,
    ));
    // OrbitalReach is NOT — the chain requires AerialTransport,
    // which requires MaterialRefining, which requires confirmed
    // fire. Substrate-divergence enforced.
    assert!(!is_unlocked(
        ToolKind::OrbitalReach,
        &species,
        &with_tool_extension(),
        true,
        true,
        Crust::Basaltic,
        10_000_000,
        mature_lit,
        &confirmed,
        &unlocked,
    ));
}

/// effect-aggregate stress test: a fire-civ that has
/// stacked the full capacity-multiplier chain (tier-1 fire,
/// food, stone; tier-2 cultivation, domestication, fluid,
/// urban, refining; tier-3 mech-adv, artisanal; tier-4
/// mechanisation, chem-synth, power, medical; tier-5 genetic,
/// advanced-medicine, autonomous, material-fab, energy, cryo,
/// organic) reaches a deeply-multiplied capacity. We don't
/// pin the exact multiplier (rounding in fixed-point makes
/// that fragile) — just confirm it's > ×30, which would be
/// impossible without all tiers stacking.
#[test]
fn full_capacity_stack_exceeds_30x() {
    let stack = [
        ToolKind::LocalisedCombustion,     // ×1.15
        ToolKind::FoodProcessing,          // ×1.15
        ToolKind::StoneWorking,            // ×1.05
        ToolKind::BulkCultivation,         // ×2.0
        ToolKind::AnimalSymbiosis,         // ×1.5
        ToolKind::FluidControl,            // ×1.20
        ToolKind::UrbanConstruction,       // ×1.10
        ToolKind::MaterialRefining,        // ×1.05
        ToolKind::MechanicalAdvantage,     // ×1.10
        ToolKind::ArtisanalSpecialisation, // ×1.10
        ToolKind::Mechanisation,           // ×3.0
        ToolKind::ChemicalSynthesis,       // ×1.20
        ToolKind::PowerGeneration,         // ×1.15
        ToolKind::MedicalIntervention,     // ×1.10
        ToolKind::GeneticManipulation,     // ×1.20
        ToolKind::AdvancedMedicine,        // ×1.20
        ToolKind::AutonomousSystems,       // ×1.15
        ToolKind::MaterialFabrication,     // ×1.10
        ToolKind::EnergyStorage,           // ×1.10
        ToolKind::CryogenicEngineering,    // ×1.05
        ToolKind::OrganicSynthesis,        // ×1.10
    ];
    let total = stack
        .iter()
        .map(|t| t.capacity_multiplier())
        .fold(Real::ONE, |a, b| a * b);
    assert!(
        total > Real::from_int(30),
        "full-stack capacity multiplier {total:?} should exceed ×30"
    );
}

/// tier-4: ids 39-48, all tier 4.
#[test]
fn tier4_ids_are_in_reserved_range() {
    let tier4 = [
        ToolKind::Mechanisation,
        ToolKind::LongRangeCommunication,
        ToolKind::ChemicalSynthesis,
        ToolKind::MedicalIntervention,
        ToolKind::AdvancedMaterials,
        ToolKind::HeavyTransport,
        ToolKind::PowerGeneration,
        ToolKind::AnalyticalEngines,
        ToolKind::MassLiteracy,
        ToolKind::AerialTransport,
    ];
    for t in tier4 {
        assert_eq!(t.tier(), 4, "{t:?} should be tier 4");
        assert!(
            (39..=48).contains(&t.id()),
            "{t:?} id {} not in reserved tier-4 range 39-48",
            t.id()
        );
    }
}

/// tier-4 substrate divergence: a no-fire civ that
/// reached tier-3 `MechanicalAdvantage` CAN reach tier-4
/// `Mechanisation` (alternate-path-friendly per the agreed
/// design) but CANNOT reach `AerialTransport` (combustion-
/// locked through `MaterialRefining` tool prereq).
#[test]
fn no_fire_seed_reaches_mechanisation_but_not_aerial() {
    use crate::discovery::ConfirmedRelation;
    let species = species_with(&[ChannelKind::Tactile, ChannelKind::VisualLight]);
    let mut confirmed: BTreeMap<u32, ConfirmedRelation> = BTreeMap::new();
    confirmed.insert(
        3001,
        ConfirmedRelation {
            relation_id: 3001,
            template_id: 14, // tidal_extremum (gravity)
            channel: crate::discovery::Channel::WaterDepth,
            form: crate::forms::Form::Linear,
            params: vec![Real::from_int(1)],
            residual: Real::ZERO,
            confidence: Real::ONE,
            n_samples: 32,
            confirmed_at_tick: 1000,
            low_confidence_streak: 0,
            cooldown_until: 0,
            refinement: None,
            initial_residual: Real::ZERO,
            falsification_streak: 0,
            inherited_from_tick: None,
            inherited_from_civ_id: None,
        },
    );
    // No-fire civ that has somehow reached tier-3
    // MechanicalAdvantage (via StoneWorking + tidal_extremum).
    let mut unlocked: BTreeSet<ToolKind> = BTreeSet::new();
    unlocked.insert(ToolKind::StoneWorking);
    unlocked.insert(ToolKind::MechanicalAdvantage);
    let mature_lit = Real::from_ratio(60, 100);

    // Mechanisation IS reachable.
    assert!(is_unlocked(
        ToolKind::Mechanisation,
        &species,
        &with_tool_extension(),
        true,
        true,
        Crust::Basaltic,
        10_000_000,
        mature_lit,
        &confirmed,
        &unlocked,
    ));
    // AerialTransport is NOT — needs MaterialRefining (combustion).
    assert!(!is_unlocked(
        ToolKind::AerialTransport,
        &species,
        &with_tool_extension(),
        true,
        true,
        Crust::Basaltic,
        10_000_000,
        mature_lit,
        &confirmed,
        &unlocked,
    ));
}

/// tier-3: ids 30-38 reserved (`AmphibiousConstruction`
/// at id 9 is the 10th tier-3 tool but pre-).
#[test]
fn tier3_ids_are_in_reserved_range() {
    let tier3_q166 = [
        ToolKind::ChemicalProjectile,
        ToolKind::PrecisionTimekeeping,
        ToolKind::MechanicalAdvantage,
        ToolKind::LongRangeNavigation,
        ToolKind::WrittenJurisprudence,
        ToolKind::AbstractMathematics,
        ToolKind::ArtisanalSpecialisation,
        ToolKind::DefensiveFortification,
        ToolKind::MotivePropulsion,
    ];
    for t in tier3_q166 {
        assert_eq!(t.tier(), 3, "{t:?} should be tier 3");
        assert!(
            (30..=38).contains(&t.id()),
            "{t:?} id {} not in reserved tier-3 range 30-38",
            t.id()
        );
    }
    // AmphibiousConstruction is the pre-existing tier-3 tool
    // at id 9; pinned for completeness.
    assert_eq!(ToolKind::AmphibiousConstruction.tier(), 3);
    assert_eq!(ToolKind::AmphibiousConstruction.id(), 9);
}

/// substrate-divergence headline: a no-fire seed reaches
/// `MechanicalAdvantage` via the alternate path
/// (`StoneWorking` + confirmed `tidal_extremum`) but is locked
/// out of `ChemicalProjectile` (combustion branch). This is
/// the genuinely-different-tech-trees-per-seed payoff.
#[test]
fn no_fire_seed_reaches_mechanical_but_not_chemical() {
    use crate::discovery::ConfirmedRelation;
    let species = species_with(&[ChannelKind::Tactile, ChannelKind::VisualLight]);
    let mut confirmed: BTreeMap<u32, ConfirmedRelation> = BTreeMap::new();
    // Civ has confirmed tidal_extremum (template 14) — gravity
    // mechanics. Did NOT confirm fire (template 1).
    confirmed.insert(
        2001,
        ConfirmedRelation {
            relation_id: 2001,
            template_id: 14, // tidal_extremum
            channel: crate::discovery::Channel::WaterDepth,
            form: crate::forms::Form::Linear,
            params: vec![Real::from_int(1)],
            residual: Real::ZERO,
            confidence: Real::ONE,
            n_samples: 32,
            confirmed_at_tick: 1000,
            low_confidence_streak: 0,
            cooldown_until: 0,
            refinement: None,
            initial_residual: Real::ZERO,
            falsification_streak: 0,
            inherited_from_tick: None,
            inherited_from_civ_id: None,
        },
    );
    // Civ has unlocked StoneWorking (a tier-1 tool, no fire
    // dependency) — but NOT LocalisedCombustion or anything
    // downstream of it.
    let mut unlocked: BTreeSet<ToolKind> = BTreeSet::new();
    unlocked.insert(ToolKind::StoneWorking);
    let mature_lit = Real::from_ratio(50, 100);

    // MechanicalAdvantage IS reachable on the alternate path.
    assert!(is_unlocked(
        ToolKind::MechanicalAdvantage,
        &species,
        &with_tool_extension(),
        true,
        true,
        Crust::Basaltic,
        10_000_000,
        mature_lit,
        &confirmed,
        &unlocked,
    ));
    // ChemicalProjectile is NOT reachable — combustion-branch
    // is locked.
    assert!(!is_unlocked(
        ToolKind::ChemicalProjectile,
        &species,
        &with_tool_extension(),
        true,
        true,
        Crust::Basaltic,
        10_000_000,
        mature_lit,
        &confirmed,
        &unlocked,
    ));
}

/// tier-2: ids 20-29 reserved, all tier 2.
#[test]
fn tier2_ids_are_in_reserved_range() {
    let tier2 = [
        ToolKind::BulkCultivation,
        ToolKind::AnimalSymbiosis,
        ToolKind::BulkStorage,
        ToolKind::MaterialRefining,
        ToolKind::CulturalEncoding,
        ToolKind::FluidControl,
        ToolKind::WatercraftConstruction,
        ToolKind::PermanentMasonry,
        ToolKind::TradeNetworks,
        ToolKind::UrbanConstruction,
    ];
    for t in tier2 {
        assert_eq!(t.tier(), 2, "{t:?} should be tier 2");
        assert!(
            (20..=29).contains(&t.id()),
            "{t:?} id {} not in reserved tier-2 range 20-29",
            t.id()
        );
    }
}

/// substrate divergence canary, tier 2: the same no-fire
/// seed that's locked out of `LocalisedCombustion` is also
/// locked out of `BulkStorage` and `MaterialRefining` (both
/// gate on confirmed fire law via relation prereq AND on
/// `LocalisedCombustion` via tool prereq). Each gate alone
/// suffices; together the substrate divergence is doubly
/// enforced.
#[test]
fn tier2_combustion_chain_blocked_without_fire() {
    use crate::discovery::ConfirmedRelation;
    let species = species_with(&[ChannelKind::Tactile, ChannelKind::VisualLight]);
    // No confirmed fire relation; no LocalisedCombustion
    // unlocked.
    let confirmed: BTreeMap<u32, ConfirmedRelation> = BTreeMap::new();
    let unlocked: BTreeSet<ToolKind> = BTreeSet::new();
    for tool in [ToolKind::BulkStorage, ToolKind::MaterialRefining] {
        assert!(
            !is_unlocked(
                tool,
                &species,
                &with_tool_extension(),
                true,
                true,
                Crust::Basaltic,
                10_000_000,
                Real::from_ratio(80, 100),
                &confirmed,
                &unlocked,
            ),
            "{tool:?} should be blocked without confirmed fire"
        );
    }
}

/// tier-2 capacity: `BulkCultivation` + `AnimalSymbiosis`
/// stack to ×3.0 multiplicatively (×2.0 × ×1.5 = 3.0). This
/// is the carrying-capacity peak before tier-3+ adds further
/// multipliers; tier-4 `Mechanisation` will push it to ×9 on
/// top of the prior stack.
#[test]
fn tier2_capacity_stacks() {
    let cult = ToolKind::BulkCultivation.capacity_multiplier();
    let dom = ToolKind::AnimalSymbiosis.capacity_multiplier();
    let stacked = cult * dom;
    assert_eq!(stacked, Real::from_int(3));
}

/// tier-1: ids 10-19 are reserved for tier-1 capabilities
/// and must not collide with the existing 1-9 sensorium tool
/// ids. (`ids_are_unique` enforces global uniqueness; this test
/// pins the ranges.)
#[test]
fn tier1_ids_are_in_reserved_range() {
    let tier1 = [
        ToolKind::LocalisedCombustion,
        ToolKind::ContactWeapon,
        ToolKind::RangedMomentumWeapon,
        ToolKind::SimpleShelter,
        ToolKind::FoodProcessing,
        ToolKind::FluidGathering,
        ToolKind::BasicTextiles,
        ToolKind::StoneWorking,
        ToolKind::OrganizedHunting,
        ToolKind::BasicHealing,
    ];
    for t in tier1 {
        assert_eq!(t.tier(), 1, "{t:?} should be tier 1");
        assert!(
            (10..=19).contains(&t.id()),
            "{t:?} id {} not in reserved tier-1 range 10-19",
            t.id()
        );
    }
}

/// substrate divergence canary: a no-fire seed (any
/// substrate that never observes the `fire` template) must NOT
/// be able to unlock `LocalisedCombustion` regardless of how
/// much science it does on other phenomena. Validates that
/// the `relation_prereq` is the substrate gate — the test
/// confirms a structurally-unsatisfiable check.
#[test]
fn localised_combustion_blocked_without_confirmed_fire() {
    use crate::discovery::ConfirmedRelation;
    let species = species_with(&[ChannelKind::Tactile, ChannelKind::VisualLight]);
    let mut confirmed: BTreeMap<u32, ConfirmedRelation> = BTreeMap::new();
    // Civ has confirmed laws on water and fertile_land but
    // never on fire — a typical aqueous / methane / ammonia
    // substrate where the `fire` template never fires.
    confirmed.insert(
        1001,
        ConfirmedRelation {
            relation_id: 1001,
            template_id: 5, // surface_water
            channel: crate::discovery::Channel::WaterDepth,
            form: crate::forms::Form::Linear,
            params: vec![Real::from_int(1)],
            residual: Real::ZERO,
            confidence: Real::ONE,
            n_samples: 32,
            confirmed_at_tick: 100,
            low_confidence_streak: 0,
            cooldown_until: 0,
            refinement: None,
            initial_residual: Real::ZERO,
            falsification_streak: 0,
            inherited_from_tick: None,
            inherited_from_civ_id: None,
        },
    );
    let unlocked: BTreeSet<ToolKind> = BTreeSet::new();
    // Massive observation count + literacy don't matter — the
    // missing relation prereq alone blocks the unlock.
    let mature_literacy = Real::from_ratio(80, 100);
    assert!(!is_unlocked(
        ToolKind::LocalisedCombustion,
        &species,
        &with_tool_extension(),
        true,
        true,
        Crust::Basaltic,
        10_000_000, // way above threshold
        mature_literacy,
        &confirmed,
        &unlocked,
    ));
}

/// Resource gate: tools with non-empty `resource_prereqs` block
/// when the civ's territory carries no biofuel/water/fossil. A
/// fresh `PhysicsState` (every cell zero) fails every resource-
/// gated tool; populating one cell with the prereq substance at
/// the threshold flips the gate to satisfied.
#[test]
fn resource_prereqs_block_unlock_when_territory_is_barren() {
    use sim_physics::{HexGrid, PhysicsState, Substance};

    let grid = HexGrid::new(4, 4);
    let mut state = PhysicsState::new(grid);
    let mut claimed: BTreeSet<u32> = BTreeSet::new();
    claimed.insert(0);
    claimed.insert(1);

    // Empty state — no biofuel, no water, no fossil.
    assert!(
        !resource_prereqs_satisfied(ToolKind::LocalisedCombustion, &state, &claimed),
        "LocalisedCombustion should fail on a barren claim (no Fuel)"
    );
    assert!(
        !resource_prereqs_satisfied(ToolKind::ChemicalSynthesis, &state, &claimed),
        "ChemicalSynthesis should fail on a barren claim (no Fossil)"
    );
    // Tools without resource prereqs always pass.
    assert!(
        resource_prereqs_satisfied(ToolKind::DistanceImaging, &state, &claimed),
        "DistanceImaging has no resource prereq, should always pass"
    );

    // Drop 1.0 unit of biofuel into cell 0 — total summed across
    // claim is 1.0, exactly the LocalisedCombustion threshold.
    state.substance_mut(Substance::Fuel.idx())[0] = Real::from_int(1);
    assert!(
        resource_prereqs_satisfied(ToolKind::LocalisedCombustion, &state, &claimed),
        "LocalisedCombustion should pass with 1.0 Fuel summed"
    );
    // Higher-tier biofuel demand still fails (5.0 threshold).
    assert!(
        !resource_prereqs_satisfied(ToolKind::MaterialRefining, &state, &claimed),
        "MaterialRefining demands 5.0 Fuel; 1.0 is insufficient"
    );

    // Spread biofuel across both claimed cells: total 5.0
    // satisfies MaterialRefining.
    state.substance_mut(Substance::Fuel.idx())[0] = Real::from_int(2);
    state.substance_mut(Substance::Fuel.idx())[1] = Real::from_int(3);
    assert!(
        resource_prereqs_satisfied(ToolKind::MaterialRefining, &state, &claimed),
        "MaterialRefining should pass with 5.0 Fuel summed across claim"
    );

    // Fossil-gated tools still fail without Fossil substrate.
    assert!(
        !resource_prereqs_satisfied(ToolKind::ChemicalSynthesis, &state, &claimed),
        "ChemicalSynthesis still fails — no Fossil in territory"
    );
    state.substance_mut(Substance::Fossil.idx())[0] = Real::from_int(1);
    assert!(
        resource_prereqs_satisfied(ToolKind::ChemicalSynthesis, &state, &claimed),
        "ChemicalSynthesis should pass once Fossil density reaches 1.0"
    );
}

/// Tool consumption: an unlocked Fuel-tool draws fuel + oxidiser
/// from its civ's territory each tick, producing matching ash.
/// Mass-conservative against the combustion mirror; deterministic
/// across cells (`BTreeSet` ordering). Non-consumable prereqs
/// (water / ice) are unaffected.
#[test]
fn apply_tool_consumption_burns_fuel_with_combustion_mirror() {
    use crate::tech::apply_tool_consumption;
    use crate::Civ;
    use sim_physics::{HexGrid, PhysicsState, Substance};

    let grid = HexGrid::new(3, 3);
    let mut state = PhysicsState::new(grid);
    // Seed cells 0 + 1 with substantial fuel + oxidiser; ash zero.
    for cell in 0..2 {
        state.substance_mut(Substance::Fuel.idx())[cell] = Real::from_int(10);
        state.substance_mut(Substance::Oxidiser.idx())[cell] = Real::from_int(10);
    }

    // Build a single-civ slice with LocalisedCombustion unlocked
    // and cells 0 + 1 claimed.
    let mut civ = Civ::new(0, 0, Real::from_int(100));
    civ.claimed_cells.insert(0);
    civ.claimed_cells.insert(1);
    civ.unlocked_tools.insert(ToolKind::LocalisedCombustion);
    let civs = vec![civ];

    let sum_substance = |state: &PhysicsState, s: Substance| -> Real {
        let densities = state.substance(s.idx());
        let mut total = Real::ZERO;
        for v in densities.iter().take(2) {
            total = total + *v;
        }
        total
    };
    let initial_fuel = sum_substance(&state, Substance::Fuel);
    let initial_ox = sum_substance(&state, Substance::Oxidiser);
    let initial_ash = sum_substance(&state, Substance::Ash);
    let initial_total = initial_fuel + initial_ox + initial_ash;

    // Run consumption for 1000 ticks — enough to make the draw
    // measurable while staying well below the cells' supply.
    for _ in 0..1000 {
        apply_tool_consumption(&mut state, &civs);
    }

    let final_fuel = sum_substance(&state, Substance::Fuel);
    let final_ox = sum_substance(&state, Substance::Oxidiser);
    let final_ash = sum_substance(&state, Substance::Ash);
    let final_total = final_fuel + final_ox + final_ash;

    // Fuel + oxidiser drained.
    assert!(final_fuel < initial_fuel, "fuel should drain");
    assert!(final_ox < initial_ox, "oxidiser should drain");
    // Ash produced.
    assert!(final_ash > initial_ash, "ash should grow");
    // Mass conserved across the (fuel, oxidiser, ash) trio.
    assert_eq!(initial_total, final_total, "mass conservation across burnable trio");
    // Stoichiometry: 1 fuel + 1 oxidiser → 2 ash.
    let consumed_fuel = initial_fuel - final_fuel;
    let consumed_ox = initial_ox - final_ox;
    assert_eq!(
        consumed_fuel, consumed_ox,
        "1:1 fuel + oxidiser consumption mirrors combustion stoichiometry"
    );
    let produced_ash = final_ash - initial_ash;
    assert_eq!(
        produced_ash,
        consumed_fuel + consumed_fuel,
        "2 ash per unit of consumed fuel — combustion mirror"
    );
}

/// Tool consumption stays at zero for water-only tools — civs
/// drawing on `FluidGathering` don't deplete the river. Only
/// `Fuel` and `Fossil` are consumable per `is_consumable`.
#[test]
fn apply_tool_consumption_does_not_deplete_water() {
    use crate::tech::apply_tool_consumption;
    use crate::Civ;
    use sim_physics::{HexGrid, PhysicsState, Substance};

    let grid = HexGrid::new(3, 3);
    let mut state = PhysicsState::new(grid);
    state.substance_mut(Substance::Water.idx())[0] = Real::from_int(10);
    state.substance_mut(Substance::Oxidiser.idx())[0] = Real::from_int(10);

    let mut civ = Civ::new(0, 0, Real::from_int(100));
    civ.claimed_cells.insert(0);
    civ.unlocked_tools.insert(ToolKind::FluidGathering);
    let civs = vec![civ];

    let initial_water = state.substance(Substance::Water.idx())[0];
    for _ in 0..1000 {
        apply_tool_consumption(&mut state, &civs);
    }
    let final_water = state.substance(Substance::Water.idx())[0];
    assert_eq!(initial_water, final_water, "water is read-only at the consumption layer");
}

/// effect-multiplier stacking: a civ with both
/// `LocalisedCombustion` (×1.15) and `FoodProcessing` (×1.15)
/// gets ×1.15² = ×1.3225 from those two alone. `StoneWorking`
/// adds another ×1.05, bringing the product to ×1.388625.
#[test]
fn capacity_multiplier_stacks_multiplicatively() {
    let combustion = ToolKind::LocalisedCombustion.capacity_multiplier();
    let food = ToolKind::FoodProcessing.capacity_multiplier();
    let stone = ToolKind::StoneWorking.capacity_multiplier();
    assert_eq!(combustion, Real::from_ratio(115, 100));
    assert_eq!(food, Real::from_ratio(115, 100));
    assert_eq!(stone, Real::from_ratio(105, 100));
    let stacked = combustion * food * stone;
    // 1.15 × 1.15 × 1.05 ≈ 1.3886; check the integer ratio
    // 138 < stacked × 100 < 140.
    let scaled = stacked * Real::from_int(100);
    assert!(
        scaled > Real::from_int(138) && scaled < Real::from_int(140),
        "stacked multiplier should be ≈ 1.388 ({scaled:?})"
    );
}

/// war strength: weapons + organised hunting stack
/// additively in the `(1 + Σbonus)` wrap. Two weapons +
/// hunting + stoneworking = 0.10 + 0.10 + 0.05 + 0.05 = 0.30
/// bonus, so `war_strength_multiplier` returns 1.30.
///
/// Tolerance check rather than `assert_eq!`: Q32.32
/// `from_ratio(10, 100)` isn't exactly 0.1 (no finite binary
/// fraction equals 0.1), so a sum of four such values
/// accumulates ~1 ULP of drift relative to a freshly-converted
/// `from_ratio(30, 100)`. Both forms are deterministic — the
/// drift is the cost of the fraction not being binary-exact —
/// so the cross-machine determinism contract still holds.
#[test]
fn war_strength_bonus_sums_additively() {
    let total = ToolKind::ContactWeapon.war_strength_bonus()
        + ToolKind::RangedMomentumWeapon.war_strength_bonus()
        + ToolKind::OrganizedHunting.war_strength_bonus()
        + ToolKind::StoneWorking.war_strength_bonus();
    let expected = Real::from_ratio(30, 100);
    let diff = (total - expected).abs();
    assert!(
        diff < Real::from_ratio(1, 1_000_000),
        "war_strength_bonus sum {total:?} differs from expected {expected:?} by more than 1ppm"
    );
}
