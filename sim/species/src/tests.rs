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
        assert!(s.t0_loss >= Real::from_ratio(5, 100), "seed {seed}");
        assert!(s.t0_loss <= Real::from_ratio(70, 100), "seed {seed}");
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
