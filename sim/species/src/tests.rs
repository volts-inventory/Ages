use super::*;
use sim_arith::Real;
use sim_recognition::RecognitionLibrary;
use sim_world::{sample_planet, Atmosphere, BiosphereClass, Composition};
use std::collections::BTreeSet;

fn fixture(seed: u64) -> Species {
    let planet = sample_planet(seed);
    let lib = RecognitionLibrary::earth_like_default();
    derive(&planet, &lib)
}

#[test]
fn derive_is_deterministic() {
    let a = fixture(42);
    let b = fixture(42);
    assert_eq!(a.seed, b.seed);
    assert_eq!(a.cognition, b.cognition);
    assert_eq!(a.sociality, b.sociality);
    assert_eq!(a.lifespan_years, b.lifespan_years);
    assert_eq!(a.t0_loss, b.t0_loss);
    assert_eq!(a.modalities.len(), b.modalities.len());
    assert_eq!(a.perceivable_templates, b.perceivable_templates);
}

#[test]
fn different_seeds_yield_different_species() {
    // Walk a band — at least one trait or modality count must
    // differ. With independent RNG streams the chance of full
    // collision is astronomically small.
    let a = fixture(1);
    let b = fixture(2);
    let same = a.cognition == b.cognition
        && a.sociality == b.sociality
        && a.lifespan_years == b.lifespan_years
        && a.modalities.len() == b.modalities.len()
        && a.perceivable_templates == b.perceivable_templates;
    assert!(!same, "seeds 1 and 2 produced identical species");
}

#[test]
fn t0_loss_stays_in_clamp_range() {
    // Spec: clamp(_, 0.05, 0.70). Walk a band of seeds.
    for seed in 0..256u64 {
        let s = fixture(seed);
        assert!(s.t0_loss >= Real::percent(5), "seed {seed}");
        assert!(s.t0_loss <= Real::percent(70), "seed {seed}");
    }
}

#[test]
fn species_only_holds_modalities_their_planet_supports() {
    for seed in 0..128u64 {
        let planet = sample_planet(seed);
        let lib = RecognitionLibrary::earth_like_default();
        let s = derive(&planet, &lib);
        for m in &s.modalities {
            assert!(
                    modality_supported(m.kind, &planet),
                    "seed {seed} has unsupported modality {:?} (atm={:?}, comp={:?}, mag={:?}, bio={:?})",
                    m.kind,
                    planet.atmosphere,
                    planet.composition,
                    planet.magnetosphere,
                    planet.biosphere
                );
        }
    }
}

#[test]
fn sub_surface_ocean_loses_visual_light() {
    // Sub-surface oceans are dark — visual_light must never seed.
    let mut found = false;
    for seed in 0..1024u64 {
        let planet = sample_planet(seed);
        if planet.composition == Composition::SubSurfaceOcean {
            let lib = RecognitionLibrary::earth_like_default();
            let s = derive(&planet, &lib);
            for m in &s.modalities {
                assert_ne!(m.kind, ModalityKind::VisualLight);
            }
            found = true;
            if found {
                break;
            }
        }
    }
    assert!(found, "no SubSurfaceOcean planet in 1024 seeds");
}

#[test]
fn no_atmosphere_disables_acoustic_air() {
    let mut found = false;
    for seed in 0..1024u64 {
        let planet = sample_planet(seed);
        if planet.atmosphere == Atmosphere::None {
            let lib = RecognitionLibrary::earth_like_default();
            let s = derive(&planet, &lib);
            for m in &s.modalities {
                assert_ne!(m.kind, ModalityKind::AcousticAir);
            }
            found = true;
            break;
        }
    }
    assert!(found, "no Atmosphere::None planet in 1024 seeds");
}

#[test]
fn perceivable_templates_intersects_modalities_with_channels() {
    // For every seed: every perceivable template must have at
    // least one of its channels in the species' modality set.
    for seed in 0..128u64 {
        let planet = sample_planet(seed);
        let lib = RecognitionLibrary::earth_like_default();
        let s = derive(&planet, &lib);
        let mod_set: BTreeSet<ModalityKind> = s.modalities.iter().map(|m| m.kind).collect();
        for tid in &s.perceivable_templates {
            let channels = template_channels(*tid);
            assert!(
                channels.iter().any(|c| mod_set.contains(c)),
                "seed {seed} marks template {tid} perceivable but no modality matches",
            );
        }
    }
}

#[test]
fn habitat_reconciles_with_boiled_vs_liquid_ocean() {
    use crate::sampling::derive_habitat;
    use crate::types::{
        Habitat, Manipulation, ManipulationKind, Modality, ModalityKind,
    };

    // A water-native ocean world, a touch/tentacle species with no
    // water-acoustic sensing (mirrors seed 495's Ylithar shape).
    let modalities = vec![Modality {
        kind: ModalityKind::Tactile,
        range_m: Real::ZERO,
        fidelity: Real::ZERO,
        bandwidth: Real::ZERO,
    }];
    let manipulations = vec![Manipulation {
        kind: ManipulationKind::Tentacle,
        force_n: Real::ZERO,
        precision_m: Real::ZERO,
        dexterity_score: Real::ZERO,
        dof_count: 0,
    }];

    let mut planet = sample_planet(42);
    planet.composition = Composition::OceanWorld;
    planet.metabolic_substrate = sim_world::MetabolicSubstrate::Aqueous;
    planet.substrate_perturbation = Real::ZERO;
    planet.surface_pressure = Real::from_int(101_325); // ~1 atm → boil ~373 K

    // Liquid ocean (290 K): the tentacled species belongs in the
    // water even without water-acoustic sensing.
    planet.mean_temperature = Real::from_int(290);
    assert_eq!(
        derive_habitat(&planet, &modalities, &manipulations),
        Habitat::Aquatic,
        "tentacled species on a liquid ocean world should be aquatic"
    );

    // Boiled dry (500 K, above the ~373 K boil point): the surface
    // ocean is gone, so the species keeps land habits instead of
    // being stranded in nonexistent water.
    planet.mean_temperature = Real::from_int(500);
    assert_eq!(
        derive_habitat(&planet, &modalities, &manipulations),
        Habitat::Terrestrial,
        "a boiled-dry ocean world should not force an aquatic habitat"
    );
}

#[test]
fn biosphere_none_yields_minimal_modality_count() {
    // `BiosphereClass::None` is no longer reachable through
    // `sample_planet` (every seed produces a habitable world of
    // some metabolic substrate). The species-derivation path that
    // handles biosphere=None still exists for callers that
    // construct a Planet manually (e.g. tests of edge-case
    // catastrophes). Build one directly here to cover that path.
    let mut planet = sample_planet(0);
    planet.biosphere = BiosphereClass::None;
    planet.atmosphere = sim_world::Atmosphere::None;
    let lib = RecognitionLibrary::earth_like_default();
    let s = derive(&planet, &lib);
    assert!(s.modalities.len() <= 1);
    assert_eq!(s.manipulation_modes.len(), 1);
}

#[test]
fn perceivable_firings_drops_unperceived() {
    let planet = sample_planet(42);
    let lib = RecognitionLibrary::earth_like_default();
    let s = derive(&planet, &lib);
    let firings = vec![
        sim_recognition::Firing {
            template_id: 1,
            cell: 0,
        },
        sim_recognition::Firing {
            template_id: 9999, // unknown id, no channels -> not perceivable.
            cell: 1,
        },
    ];
    let kept = s.perceivable_firings(&firings);
    for f in &kept {
        assert!(s.can_perceive(f.template_id));
    }
    // The 9999 firing must be dropped regardless of species seed.
    assert!(!kept.iter().any(|f| f.template_id == 9999));
}

#[test]
fn extremophile_species_occupies_high_radiation_cells() {
    // Construct an explicit extremophile tolerance with radiation_max
    // = 5.0 (silicate-shaped). It must accept a cell with rad = 3.0
    // (still inside its envelope), with all other axes at the centre
    // of the envelope so radiation is the only differentiating axis.
    let extremophile = ToleranceEnvelope {
        temp_range: (Real::from_int(1687), Real::from_int(3538)),
        ph_range: (Real::ZERO, Real::from_int(14)),
        salinity_range: (Real::ZERO, Real::from_int(200)),
        radiation_max: Real::from_int(5),
        pressure_range: (Real::ONE, Real::from_int(100)),
    };
    // Centre-of-envelope conditions for the non-radiation axes.
    let t = Real::from_int((1687 + 3538) / 2);
    let ph = Real::from_int(7);
    let sal = Real::from_int(100);
    let p = Real::from_int(50);
    let rad = Real::from_int(3);
    assert!(
        extremophile.contains(t, ph, sal, rad, p),
        "extremophile (radiation_max=5.0) must accept rad=3.0 cell"
    );

    // A default aqueous species must reject the same radiation level.
    // Use the aqueous-default helper to mirror the back-compat fixture.
    let aqueous = ToleranceEnvelope::aqueous_default();
    // Construct conditions that sit inside the aqueous temp/ph/sal/p
    // envelope so the radiation gate is the *only* axis the contains
    // check can fail on — anything else would be ambiguous.
    let aq_t = Real::from_int(300); // 300 K — well inside aqueous (273, 373)
    let aq_ph = Real::from_int(7);
    let aq_sal = Real::from_int(20);
    let aq_p = Real::ONE;
    assert!(
        !aqueous.contains(aq_t, aq_ph, aq_sal, rad, aq_p),
        "aqueous default (radiation_max=0.5) must exclude rad=3.0 cell"
    );
    // Sanity: the same cell sans the radiation excess does pass —
    // confirms it's the radiation axis (not some other) that rejects.
    let low_rad = Real::from_ratio(1, 10);
    assert!(
        aqueous.contains(aq_t, aq_ph, aq_sal, low_rad, aq_p),
        "aqueous default must accept the same cell with low rad"
    );
}

#[test]
fn mass_extinction_differential_survival_by_tolerance() {
    use crate::apply_catastrophe;
    // Radiation-burst catastrophe: rad=4.0 across the affected cells.
    // Tolerant species (silicate-shaped) has radiation_max=5.0 → the
    // burst sits at 4/5 = 80% of the ceiling, so radiation_score
    // = 1 - 4/5 = 0.20. To push it above 80% survival, give the
    // tolerant species a wider ceiling so the burst sits well inside
    // its envelope.
    //
    // Spec target: tolerant ≥ 80% survival, intolerant ≤ 20%.
    let tolerant = ToleranceEnvelope {
        temp_range: (Real::from_int(200), Real::from_int(400)),
        ph_range: (Real::ZERO, Real::from_int(14)),
        salinity_range: (Real::ZERO, Real::from_int(200)),
        // radiation_max=20 → burst at rad=4 → fit = 1 - 4/20 = 0.80
        radiation_max: Real::from_int(20),
        pressure_range: (Real::ONE, Real::from_int(10)),
    };
    let intolerant = ToleranceEnvelope::aqueous_default();

    // Non-radiation axes pinned to the centre of the *tolerant*
    // envelope so its non-radiation scores stay at 1.0 — radiation
    // becomes the binding axis. The aqueous envelope's centre differs
    // on temperature / pressure, but the intolerant species' rad=4.0
    // is so far above its radiation_max=0.5 that the radiation gate
    // dominates regardless.
    //  tolerant temp_range (200, 400) → centre 300
    //  tolerant pressure_range (1, 10) → centre 5.5
    //  tolerant salinity_range (0, 200) → centre 100
    //  tolerant ph_range (0, 14) → centre 7
    let t = Real::from_int(300);
    let ph = Real::from_int(7);
    let sal = Real::from_int(100);
    let p = Real::from_ratio(55, 10);
    let rad = Real::from_int(4);

    let surv_tolerant = apply_catastrophe(&tolerant, t, ph, sal, rad, p);
    let surv_intolerant = apply_catastrophe(&intolerant, t, ph, sal, rad, p);

    // Tolerant: limited by the smallest-margin axis. Radiation
    // score = 1 - 4/20 = 0.80. The temp axis is at 310 K in a
    // (200, 400) range — margin from centre 300 K is small but the
    // axis_score is still > 0.80. Verify the spec target.
    assert!(
        surv_tolerant >= Real::percent(80),
        "tolerant survival expected >= 0.80, got {surv_tolerant:?}"
    );
    // Intolerant: aqueous has radiation_max=0.5; rad=4 is far above
    // → match_score = 0; survival = 0 which is well below the 0.20
    // threshold the spec calls for.
    assert!(
        surv_intolerant <= Real::percent(20),
        "intolerant survival expected <= 0.20, got {surv_intolerant:?}"
    );
}

#[test]
fn tolerance_envelope_is_deterministic_across_fixtures() {
    // The per-species jitter must be a pure function of seed +
    // substrate. Two derivations of the same seed produce bit-equal
    // envelopes; the envelope must also be non-degenerate (low < high
    // on every range axis).
    let a = fixture(7);
    let b = fixture(7);
    assert_eq!(a.tolerance, b.tolerance);
    assert!(a.tolerance.temp_range.0 <= a.tolerance.temp_range.1);
    assert!(a.tolerance.ph_range.0 <= a.tolerance.ph_range.1);
    assert!(a.tolerance.salinity_range.0 <= a.tolerance.salinity_range.1);
    assert!(a.tolerance.pressure_range.0 <= a.tolerance.pressure_range.1);
    assert!(a.tolerance.radiation_max >= Real::ZERO);
}

#[test]
fn surface_solvent_template_channels_match_substrate() {
    // Each of the four per-substrate surface-solvent templates
    // (Sprint 2 Item 8) maps to a substrate-appropriate channel
    // set. Water → visual + tactile; ammonia adds chemical-taste
    // + acoustic-air; methane is acoustic-water-led; silicate
    // melt is tactile + seismic + visual-light.
    let water = template_channels(50);
    assert!(water.contains(&ModalityKind::VisualLight));
    assert!(water.contains(&ModalityKind::Tactile));

    let ammonia = template_channels(51);
    assert!(ammonia.contains(&ModalityKind::ChemicalTaste));
    assert!(ammonia.contains(&ModalityKind::Tactile));
    assert!(ammonia.contains(&ModalityKind::AcousticAir));

    let methane = template_channels(52);
    assert!(methane.contains(&ModalityKind::AcousticWater));
    assert!(methane.contains(&ModalityKind::Tactile));
    assert!(methane.contains(&ModalityKind::ChemicalTaste));

    let silicate = template_channels(53);
    assert!(silicate.contains(&ModalityKind::Tactile));
    assert!(silicate.contains(&ModalityKind::Seismic));
    assert!(silicate.contains(&ModalityKind::VisualLight));

    // Non-existent template id stays empty.
    assert!(template_channels(9999).is_empty());
}

#[test]
fn cognition_axes_diverge_from_scalar() {
    // Earlier `derive` populated `cognition_axes` via
    // `CognitionAxes::uniform(cognition)`, so every axis aliased
    // the scalar bit-for-bit. Downstream consumers that wired to
    // `cognition_axes.working_memory` saw the legacy scalar with
    // no per-axis differentiation. The production path now uses
    // `from_scalar_with_seed`, which perturbs each axis
    // independently. Walk several species: assert the three axes
    // are NOT all identical for at least one seed and that
    // `average()` stays close to the scalar.
    let mut any_diverged = false;
    for seed in 0..64u64 {
        let s = fixture(seed);
        let axes = s.cognition_axes;
        let all_equal = axes.working_memory == axes.abstraction
            && axes.abstraction == axes.social;
        if !all_equal {
            any_diverged = true;
        }
        // average() ≈ scalar — within ±0.05 (clamp at extremes
        // can introduce a small drift; well below the threshold
        // that would shift any legacy downstream formula).
        let avg = axes.average();
        let drift = (avg - s.cognition).abs();
        assert!(
            drift <= Real::percent(5),
            "seed {seed}: axes.average()={avg:?} drifted >0.05 from cognition={:?}",
            s.cognition
        );
    }
    assert!(
        any_diverged,
        "no species across 64 seeds produced divergent axes — \
         from_scalar_with_seed must perturb per-axis"
    );
}

// ---------------------------------------------------------------
// Sprint 2 Item 7b — dormancy_capability + DormantPool tests.
// ---------------------------------------------------------------

#[test]
fn dormancy_field_is_present_and_in_range() {
    // Every sampled species has `dormancy_capability` ∈ [0, 1].
    for seed in 0..256u64 {
        let s = fixture(seed);
        assert!(
            s.dormancy_capability >= Real::ZERO && s.dormancy_capability <= Real::ONE,
            "seed {seed}: dormancy_capability {:?} out of [0, 1]",
            s.dormancy_capability,
        );
    }
}

#[test]
fn dormancy_derivation_is_deterministic() {
    let a = fixture(7);
    let b = fixture(7);
    assert_eq!(a.dormancy_capability, b.dormancy_capability);
}

#[test]
fn apply_catastrophe_with_dormancy_zero_is_identity() {
    let base = Real::percent(40);
    let out = apply_catastrophe_with_dormancy(Real::ZERO, base, Real::ONE);
    assert_eq!(out, base);
}

#[test]
fn apply_catastrophe_with_dormancy_full_zeroes_damage() {
    let base = Real::percent(40);
    let out = apply_catastrophe_with_dormancy(Real::ONE, base, Real::ONE);
    assert_eq!(out, Real::ZERO);
}

#[test]
fn dormant_species_survives_catastrophe_at_reduced_rate() {
    // Sprint 2 Item 7b spec test #1 — species-crate variant via
    // the synthetic helper. (The catastrophe-crate variant in
    // sim/civ/src/catastrophe/mod.rs covers the wired pipeline.)
    let base = Real::percent(40);
    let low = apply_catastrophe_with_dormancy(Real::ZERO, base, Real::ONE);
    let high = apply_catastrophe_with_dormancy(Real::percent(90), base, Real::ONE);
    // dormancy=0.9 → effective = base × 0.10. low / high = 10×.
    assert_eq!(low, base);
    assert_eq!(high, base * Real::percent(10));
    let ratio = high / low;
    // Q32.32 representation of 0.10 (= 10 / 100) isn't exact; use
    // a tight tolerance band around 10%.
    let tol = Real::from_ratio(1, 10_000);
    assert!(
        (ratio - Real::percent(10)).abs() <= tol,
        "expected ratio ≈ 0.10, got {ratio:?}",
    );
}

#[test]
fn dormant_pool_resurrect_step_respects_target_cap() {
    // Q32.32 representation of 1% is not exact, so use a small
    // tolerance for the magnitudes derived from it.
    let tol = Real::from_ratio(1, 1_000_000);

    let mut pool = DormantPool {
        population: Real::from_int(100),
        entered_tick: 0,
    };
    let mut active = Real::from_int(950);
    let target = Real::from_int(1000);
    // 1% of 100 = 1.0; but the headroom is 50, so revive ≈ 1.0
    // (well under the cap).
    let revived = pool.resurrect_step(&mut active, target);
    assert!(
        (revived - Real::ONE).abs() <= tol,
        "expected revived ≈ 1.0, got {revived:?}",
    );
    assert!(
        (active - Real::from_int(951)).abs() <= tol,
        "expected active ≈ 951, got {active:?}",
    );
    assert!(
        (pool.population - Real::from_int(99)).abs() <= tol,
        "expected pool ≈ 99, got {:?}",
        pool.population,
    );

    // Now move active to one below the target: the next revive
    // should clamp to the remaining headroom.
    active = Real::from_int(999) + Real::from_ratio(5, 10);
    let revived = pool.resurrect_step(&mut active, target);
    // headroom = 0.5, revive_want ≈ 0.99 — clamp to 0.5.
    assert!(
        (revived - Real::from_ratio(5, 10)).abs() <= tol,
        "expected revived ≈ 0.5, got {revived:?}",
    );
    assert!(
        (active - target).abs() <= tol,
        "expected active ≈ target, got {active:?}",
    );
}

#[test]
fn dormant_pool_resurrect_step_is_noop_when_empty() {
    let mut pool = DormantPool::EMPTY;
    let mut active = Real::from_int(10);
    let revived = pool.resurrect_step(&mut active, Real::from_int(100));
    assert_eq!(revived, Real::ZERO);
    assert_eq!(active, Real::from_int(10));
}

#[test]
fn seed_bank_resurrection_repopulates_post_extinction_event() {
    // Sprint 2 Item 7b spec test #2.
    //
    // A catastrophic extinction has reduced the active population
    // to a token survivor; the seed-bank dormant pool holds the
    // 1000 individuals that crypto-bio'd through it. Over 500
    // ticks of resurrection at 1%/tick we want the active pool to
    // recover to ≥99% of the pre-event level (target = 1000).
    let pre_event = Real::from_int(1000);
    let mut pool = DormantPool {
        population: Real::from_int(1000),
        entered_tick: 0,
    };
    let mut active = Real::from_int(1);
    for _ in 0..500 {
        pool.resurrect_step(&mut active, pre_event);
    }
    // After 500 ticks at 1%/tick, the geometric-decay reserve has
    // released 1 − 0.99^500 ≈ 0.9934 of its mass into the active
    // pool. With an initial active of 1, the active should be at
    // least 99% of pre_event.
    let bound = pre_event * Real::percent(99);
    assert!(
        active >= bound,
        "seed-bank failed to recover: active={active:?} bound={bound:?}",
    );
    // Conservation: pool + active ≤ pre_event + initial_active
    // (no creation). With initial active = 1, this is ≤ 1001.
    let total = active + pool.population;
    assert!(
        total <= Real::from_int(1001),
        "conservation violated: total={total:?} > 1001",
    );
}

/// Sprint 3 Item 10: communication-channel modality couples into
/// transmission speed. The pheromone species' aggregate
/// `communication_speed_multiplier` is strictly less than the
/// acoustic species'. The downstream comprehension formula
/// multiplies by this scalar, so the same successor-comprehension
/// formula yields a lower comprehension score for the pheromone
/// species under identical other conditions.
#[test]
fn comm_channel_modality_affects_transmission_speed() {
    use crate::{CognitionTopology, Modality, ModalityKind};
    let acoustic = Modality {
        kind: ModalityKind::AcousticAir,
        range_m: Real::from_int(100),
        fidelity: Real::ONE,
        bandwidth: Real::ONE,
    };
    let acoustic_speed = CognitionTopology::transmission_speed_for_modality(acoustic.kind);
    assert_eq!(acoustic_speed, Real::ONE);

    let pheromone = Modality {
        kind: ModalityKind::ChemicalPheromone,
        range_m: Real::from_int(100),
        fidelity: Real::ONE,
        bandwidth: Real::ONE,
    };
    let pheromone_speed = CognitionTopology::transmission_speed_for_modality(pheromone.kind);
    assert_eq!(pheromone_speed, Real::from_ratio(2, 10));
    assert!(pheromone_speed < acoustic_speed);

    // End-to-end via `Species::communication_speed_multiplier`.
    let base = fixture(1);
    let mut acoustic_species = base.clone();
    acoustic_species.modalities = vec![acoustic];
    let mut pheromone_species = base;
    pheromone_species.modalities = vec![pheromone];
    let acoustic_mult = acoustic_species.communication_speed_multiplier();
    let pheromone_mult = pheromone_species.communication_speed_multiplier();
    assert!(
        pheromone_mult < acoustic_mult,
        "chemical-pheromone species must have slower transmission than acoustic: \
         pheromone={pheromone_mult:?} acoustic={acoustic_mult:?}",
    );
    assert_eq!(acoustic_mult, Real::ONE);
    assert_eq!(pheromone_mult, Real::from_ratio(2, 10));
}

/// Sprint 3 Item 10: a species with both pheromone and acoustic
/// channels inherits the *fastest* channel — knowledge propagates
/// on the fastest available substrate.
#[test]
fn species_with_mixed_modalities_inherits_fastest_channel() {
    use crate::{Modality, ModalityKind};
    let mut s = fixture(7);
    s.modalities = vec![
        Modality {
            kind: ModalityKind::ChemicalPheromone,
            range_m: Real::from_int(1),
            fidelity: Real::ONE,
            bandwidth: Real::ONE,
        },
        Modality {
            kind: ModalityKind::AcousticAir,
            range_m: Real::from_int(100),
            fidelity: Real::ONE,
            bandwidth: Real::ONE,
        },
    ];
    assert_eq!(s.communication_speed_multiplier(), Real::ONE);
}

/// Sprint 3 Item 10: Acentric species retain knowledge across
/// generations far better than Centralized ones. Both
/// `transmission_decay_years` and `transmission_decay_ticks`
/// stretch by 5× (1 / 0.2) for Acentric. Q32.32 division
/// introduces sub-ε rounding so we assert "close to 5×" rather
/// than bit-exact equality.
#[test]
fn acentric_topology_extends_transmission_decay_window() {
    let mut centralized = fixture(11);
    centralized.cognition_topology = crate::CognitionTopology::Centralized;
    let mut acentric = centralized.clone();
    acentric.cognition_topology = crate::CognitionTopology::Acentric;
    let cent_years = centralized.transmission_decay_years();
    let acen_years = acentric.transmission_decay_years();
    let target = cent_years * Real::from_int(5);
    let diff = if acen_years > target {
        acen_years - target
    } else {
        target - acen_years
    };
    // Sub-ε tolerance — Q32.32 round-trip via `/ 0.2`.
    assert!(
        diff < Real::from_ratio(1, 100),
        "Acentric decay must be ~5× Centralized; got acentric={acen_years:?}, expected~{target:?}",
    );
    assert!(acentric.transmission_decay_ticks() > centralized.transmission_decay_ticks());
}