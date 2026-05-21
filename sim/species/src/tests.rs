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
