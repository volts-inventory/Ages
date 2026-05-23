//! F3 — `ToleranceEnvelope` on `EcoSpecies`. The civ-side P0.4 fix
//! gated catastrophe damage on the per-species tolerance envelope's
//! match_score; F3 extends that gate into the trophic web so an
//! extremophile producer (high `radiation_max`) survives a radiation
//! burst differently from a narrow-envelope aqueous producer in the
//! ecosystem step.

use super::capacity;
use crate::*;
use sim_arith::Real;
use sim_species::{
    EcosystemRole, Habitat, InteractionMatrix, ProducerMetabolism, SpeciesId, ToleranceEnvelope,
};

#[test]
fn eco_species_has_tolerance_envelope_post_init() {
    // Both sampling paths (legacy `sample_ecosystem` and substrate-
    // aware `sample_ecosystem_with_substrate`) must populate the new
    // `EcoSpecies::tolerance` field on every record. The legacy path
    // pins to the Aqueous substrate (back-compat); the
    // substrate-aware path threads the planet's substrate through
    // `sample_tolerance_for_substrate`.
    let eco_legacy = sample_ecosystem(42, capacity());
    assert!(
        !eco_legacy.species.is_empty(),
        "sample_ecosystem produced no species"
    );
    for s in eco_legacy.species.values() {
        // Aqueous defaults: temp_range straddles 273-373 K after
        // ±20% jitter — exercise the invariant that the envelope is
        // well-formed (lo < hi) on every axis rather than the
        // numeric values (which depend on the per-species seed).
        assert!(
            s.tolerance.temp_range.0 < s.tolerance.temp_range.1,
            "temp_range malformed for species {:?}: {:?}",
            s.species_id,
            s.tolerance.temp_range,
        );
        assert!(
            s.tolerance.ph_range.0 < s.tolerance.ph_range.1,
            "ph_range malformed: {:?}",
            s.tolerance.ph_range
        );
        // Radiation max is positive (extremophiles can sit very high;
        // generalists sit near the Aqueous baseline of 0.5).
        assert!(
            s.tolerance.radiation_max > Real::ZERO,
            "radiation_max non-positive for species {:?}: {:?}",
            s.species_id,
            s.tolerance.radiation_max,
        );
    }

    // Silicate substrate: a much higher temperature window
    // (1687-3538 K base) and a much higher radiation_max (5.0 base)
    // than aqueous. The substrate-aware path should produce
    // distinguishable envelopes.
    let eco_silicate = sample_ecosystem_with_substrate(42, "silicate", capacity());
    let aqueous_temp_avg = {
        let s = eco_legacy.species.values().next().unwrap();
        (s.tolerance.temp_range.0 + s.tolerance.temp_range.1) / Real::from_int(2)
    };
    let silicate_temp_avg = {
        let s = eco_silicate.species.values().next().unwrap();
        (s.tolerance.temp_range.0 + s.tolerance.temp_range.1) / Real::from_int(2)
    };
    assert!(
        silicate_temp_avg > aqueous_temp_avg * Real::from_int(3),
        "silicate envelope did not shift temp range upward: \
         silicate_avg={silicate_temp_avg:?}, aqueous_avg={aqueous_temp_avg:?}"
    );
}

#[test]
fn extremophile_eco_species_survives_radiation_better_than_aqueous() {
    // Two EcoSpecies, identical except for their `ToleranceEnvelope`.
    // The radiation burst (rad = 1.1) sits well above the aqueous
    // envelope's `radiation_max = 0.5` ⇒ match_score = 0 ⇒ full
    // raw_loss_frac applies. The extremophile envelope's
    // `radiation_max = 20` keeps rad inside the envelope on a 0.945
    // axis_score; the other axes are centred so the radiation axis
    // is the binding constraint. After
    // `apply_catastrophe_at_cell(0.10, ...)`, the extremophile's
    // surviving biomass must exceed the aqueous's by a substantial
    // margin (>3×, mirroring the civ-side P0.4 acceptance bound).
    let aqueous_id = SpeciesId(0);
    let extremo_id = SpeciesId(1);
    let starting_biomass = Real::from_int(1_000);

    let aqueous = EcoSpecies {
        species_id: aqueous_id,
        role: EcosystemRole::Producer {
            metabolism: ProducerMetabolism::Photoautotroph,
        },
        biomass: starting_biomass,
        is_extant: true,
        low_biomass_streak: 0,
        habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
        tolerance: ToleranceEnvelope::aqueous_default(),
    };
    let extremophile_tolerance = ToleranceEnvelope {
        temp_range: (Real::from_int(200), Real::from_int(400)),
        ph_range: (Real::from_int(5), Real::from_int(9)),
        salinity_range: (Real::from_int(10), Real::from_int(30)),
        radiation_max: Real::from_int(20),
        pressure_range: (Real::from_ratio(5, 10), Real::from_ratio(15, 10)),
    };
    let extremophile = EcoSpecies {
        species_id: extremo_id,
        role: EcosystemRole::Producer {
            metabolism: ProducerMetabolism::Photoautotroph,
        },
        biomass: starting_biomass,
        is_extant: true,
        low_biomass_streak: 0,
        habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
        tolerance: extremophile_tolerance,
    };
    let species = vec![aqueous, extremophile];
    let mut eco = PlanetEcosystem::new(species, InteractionMatrix::new(), Real::from_int(10_000));

    // Cell conditions: temperature, pH, salinity, pressure centred on
    // the aqueous envelope so the *only* axis that takes the aqueous
    // species below match_score = 1 is radiation. This isolates the
    // radiation gate as the binding constraint and matches the civ-
    // side P0.4 test's structure.
    let cell_t = Real::from_int(300);
    let cell_ph = Real::from_int(7);
    let cell_sal = Real::from_int(20);
    let cell_rad = Real::from_ratio(11, 10); // 1.1 — above aqueous radiation_max = 0.5.
    let cell_p = Real::ONE;
    let raw_loss = Real::percent(50); // 50% headline severity.

    eco.apply_catastrophe_at_cell(raw_loss, cell_t, cell_ph, cell_sal, cell_rad, cell_p);

    let aqueous_after = eco.species.get(&aqueous_id).unwrap().biomass;
    let extremo_after = eco.species.get(&extremo_id).unwrap().biomass;

    // Aqueous species: rad=1.1 > radiation_max=0.5 ⇒ match_score = 0
    // ⇒ full 50% loss ⇒ ~500 surviving.
    let aqueous_loss = starting_biomass - aqueous_after;
    let extremo_loss = starting_biomass - extremo_after;
    assert!(
        aqueous_loss > Real::ZERO,
        "aqueous species took no damage: after={aqueous_after:?}"
    );
    assert!(
        aqueous_loss >= extremo_loss * Real::from_int(3),
        "expected aqueous biomass loss >= 3× extremophile loss; \
         aqueous_loss={aqueous_loss:?}, extremo_loss={extremo_loss:?}"
    );
    // Headline observable: more extremophile biomass survives.
    assert!(
        extremo_after > aqueous_after,
        "extremophile survivors must exceed aqueous survivors; \
         extremo_after={extremo_after:?}, aqueous_after={aqueous_after:?}"
    );
}
