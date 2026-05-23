//! Predator-prey LV cycle, functional-response, and competition
//! tests (Sprint 2 Item 6, P2.6).

use crate::*;
use sim_arith::Real;
use sim_species::{
    EcosystemRole, FunctionalResponse, Habitat, Interaction, InteractionKind, InteractionMatrix,
    ProducerMetabolism, SpeciesId, ToleranceEnvelope,
};

/// Helper for the LV-cycle tests. Builds a fresh two-species
/// (producer + predator) ecosystem with the given per-pair
/// `half_saturation` fraction, runs 1000 ticks, and returns the
/// predator-biomass history.
fn run_lv_pair(half_saturation_frac: Real) -> Vec<Real> {
    let prey_id = SpeciesId(0);
    let pred_id = SpeciesId(1);
    let species = vec![
        EcoSpecies {
            species_id: prey_id,
            role: EcosystemRole::Producer {
                metabolism: ProducerMetabolism::Photoautotroph,
            },
            biomass: Real::from_int(800),
            is_extant: true,
            low_biomass_streak: 0,
            habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
            tolerance: ToleranceEnvelope::aqueous_default(),
        },
        EcoSpecies {
            species_id: pred_id,
            role: EcosystemRole::PrimaryConsumer,
            biomass: Real::from_int(50),
            is_extant: true,
            low_biomass_streak: 0,
            habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
            tolerance: ToleranceEnvelope::aqueous_default(),
        },
    ];
    let mut matrix = InteractionMatrix::new();
    matrix.insert(
        pred_id,
        prey_id,
        Interaction {
            kind: InteractionKind::Predation,
            strength: Real::from((50, 100)),
            functional_response: FunctionalResponse::Saturating,
            half_saturation: half_saturation_frac,
        },
    );
    let mut eco = PlanetEcosystem::new(species, matrix, Real::from_int(10_000));
    let mut history: Vec<Real> = Vec::with_capacity(1000);
    for _ in 0..1000 {
        eco.step();
        history.push(eco.species.get(&pred_id).unwrap().biomass);
    }
    history
}

/// Count the number of derivative sign changes (zero-crossings of
/// `dB/dt`) in a biomass time series. Two crossings per cycle (one
/// peak, one trough); a monotonic trajectory yields ≤ 1.
fn count_sign_changes(history: &[Real]) -> usize {
    let mut sign_changes = 0usize;
    let mut last_sign: i32 = 0;
    for w in history.windows(2) {
        let d = w[1] - w[0];
        let s: i32 = if d > Real::ZERO {
            1
        } else if d < Real::ZERO {
            -1
        } else {
            0
        };
        if s != 0 && last_sign != 0 && s != last_sign {
            sign_changes += 1;
        }
        if s != 0 {
            last_sign = s;
        }
    }
    sign_changes
}

/// Locate the first peak (rising-then-falling inflection) in a
/// biomass history. Used as a proxy for half-period in damped LV
/// cycles — different `half_saturation` calibrations shift the
/// first peak by tens of ticks. Returns `None` if the trajectory
/// is monotonic across the full window.
fn first_peak_tick(history: &[Real]) -> Option<usize> {
    let mut last_sign: i32 = 0;
    for (i, w) in history.windows(2).enumerate() {
        let d = w[1] - w[0];
        let s: i32 = if d > Real::ZERO {
            1
        } else if d < Real::ZERO {
            -1
        } else {
            0
        };
        if s == -1 && last_sign == 1 {
            return Some(i);
        }
        if s != 0 {
            last_sign = s;
        }
    }
    None
}

#[test]
fn predator_prey_pair_exhibits_lotka_volterra_cycles() {
    // Two-species ecosystem: producer (prey) + primary consumer
    // (predator). Run 1000 ticks; locate the first peak in the
    // predator-biomass trajectory. Genuine LV oscillation produces
    // a rise → peak → fall; a monotonic decay yields no peak.
    //
    // Sprint 2 Item P2.6 extension: run two pairs with different
    // per-pair `half_saturation` values (lynx-hare specialist 0.30
    // vs the back-compat generic-mutualism 0.50 baseline) and
    // verify they produce visibly different oscillation periods —
    // measured as the tick index of the first peak. The two pairs
    // share every other parameter (capacity, initial biomass,
    // strength, functional-response shape) so any difference in
    // peak tick is direct evidence the per-pair `half_saturation`
    // is threading through to the functional response. The higher-k
    // pair keeps the response further in its linear regime so
    // predator growth lags prey growth more — peak arrives later in
    // the window. Empirically (Q32.32 numerics at this seed) the
    // 0.30 pair peaks near tick ~177 and the 0.50 pair peaks near
    // tick ~244; the test requires a separation of ≥ 25 ticks.
    //
    // The capacity (10_000) is set well above the Lindeman cap so
    // the predator's growth is rate-limited by the functional
    // response, not by the cap binding immediately — otherwise
    // both biomasses sit pinned at the pyramid ceiling and never
    // oscillate. Predator initial biomass = 50 keeps it well above
    // the extinction threshold (`0.001 × 10_000 = 10`) at the start;
    // the LV trough is also above 10, so the species never
    // accumulates a confirmation-window streak.
    let mid_k_history = run_lv_pair(Real::from_ratio(30, 100)); // lynx-hare 0.30
    let high_k_history = run_lv_pair(Real::from_ratio(50, 100)); // generic 0.50

    let mid_peak = first_peak_tick(&mid_k_history)
        .expect("mid-k (0.30) predator biomass never peaked — monotonic trajectory, not LV cycle");
    let high_peak = first_peak_tick(&high_k_history)
        .expect("high-k (0.50) predator biomass never peaked — monotonic trajectory, not LV cycle");

    // Each pair must also show at least one sign change in the
    // derivative so the "predator biomass actually oscillates"
    // headline of the original test still holds.
    let mid_changes = count_sign_changes(&mid_k_history);
    let high_changes = count_sign_changes(&high_k_history);
    assert!(
        mid_changes >= 1,
        "mid-k (0.30) predator biomass derivative did not change sign ({mid_changes}) — no oscillation",
    );
    assert!(
        high_changes >= 1,
        "high-k (0.50) predator biomass derivative did not change sign ({high_changes}) — no oscillation",
    );

    // Visibly different periods — the two calibrations must
    // produce first peaks at least 25 ticks apart. (Same
    // capacity / strength / functional-response shape; only the
    // half-saturation differs, so any separation > the
    // discretisation noise is direct evidence the field is
    // threaded through.)
    let diff = mid_peak.abs_diff(high_peak);
    assert!(
        diff >= 25,
        "mid-k (0.30) and high-k (0.50) pairs peak at indistinguishable ticks (mid_peak={mid_peak}, high_peak={high_peak}, diff={diff}) — per-pair half_saturation is not threading through to the functional response",
    );
    // Direction sanity check — higher k → later peak (lower-k
    // saturates faster on the rising prey limb so the cycle
    // accelerates).
    assert!(
        high_peak > mid_peak,
        "higher-k pair (0.50) peaked at {high_peak} but lower-k pair (0.30) peaked at {mid_peak} — direction wrong; lower-k should peak earlier",
    );
}

#[test]
fn low_half_saturation_predator_dampens_cycle_faster() {
    // Sprint 2 Item P2.6: a predator with a very low
    // `half_saturation = 0.05` saturates almost immediately on any
    // prey biomass; per-capita consumption is effectively a
    // constant `≈ strength × pred`, which behaves more like a
    // density-independent grazer than a classical LV oscillator.
    // The system damps to steady state in fewer oscillations than
    // a high-K (0.50) pair where the functional response stays in
    // its linear regime and the predator-prey coupling sustains
    // classical cycles for hundreds of ticks.
    //
    // Acceptance: in 500 ticks, the low-K (0.05) pair settles with
    // strictly *fewer* derivative sign changes than the high-K
    // (0.50) pair — direct evidence that lowering the
    // half-saturation moves the system off the LV limit-cycle
    // attractor toward a damped equilibrium.
    let prey_id = SpeciesId(0);
    let pred_id = SpeciesId(1);
    let make_eco = |half_sat: Real| -> PlanetEcosystem {
        let species = vec![
            EcoSpecies {
                species_id: prey_id,
                role: EcosystemRole::Producer {
                    metabolism: ProducerMetabolism::Photoautotroph,
                },
                biomass: Real::from_int(800),
                is_extant: true,
                low_biomass_streak: 0,
                habitat: sim_species::Habitat::Terrestrial,
                cell_biomass: Vec::new(),
                tolerance: ToleranceEnvelope::aqueous_default(),
            },
            EcoSpecies {
                species_id: pred_id,
                role: EcosystemRole::PrimaryConsumer,
                biomass: Real::from_int(50),
                is_extant: true,
                low_biomass_streak: 0,
                habitat: sim_species::Habitat::Terrestrial,
                cell_biomass: Vec::new(),
                tolerance: ToleranceEnvelope::aqueous_default(),
            },
        ];
        let mut matrix = InteractionMatrix::new();
        matrix.insert(
            pred_id,
            prey_id,
            Interaction {
                kind: InteractionKind::Predation,
                strength: Real::from((50, 100)),
                functional_response: FunctionalResponse::Saturating,
                half_saturation: half_sat,
            },
        );
        PlanetEcosystem::new(species, matrix, Real::from_int(10_000))
    };

    let run = |eco: &mut PlanetEcosystem| -> Vec<Real> {
        let mut history: Vec<Real> = Vec::with_capacity(500);
        for _ in 0..500 {
            eco.step();
            history.push(eco.species.get(&pred_id).unwrap().biomass);
        }
        history
    };

    let mut eco_low = make_eco(Real::from_ratio(5, 100)); // 0.05
    let mut eco_high = make_eco(Real::from_ratio(50, 100)); // 0.50
    let low_history = run(&mut eco_low);
    let high_history = run(&mut eco_high);

    let low_changes = count_sign_changes(&low_history);
    let high_changes = count_sign_changes(&high_history);

    // High-k pair stays on the LV limit cycle: must oscillate.
    assert!(
        high_changes >= 2,
        "high-K (0.50) baseline did not oscillate (only {high_changes} sign changes) — test fixture broken",
    );
    // Low-k pair settles to steady state faster: strictly fewer
    // sign changes than the high-K pair over the same 500-tick
    // window.
    assert!(
        low_changes < high_changes,
        "low-K (0.05) predator did not dampen faster: low_changes={low_changes}, high_changes={high_changes} (expected low < high)",
    );
}

#[test]
fn competition_pair_excludes_at_equilibrium() {
    // Two PrimaryConsumers competing for the same producer pool.
    // One starts at higher biomass (asymmetric initial condition);
    // the stronger competitor should drive the weaker toward
    // extinction. Distinct dynamic from predation — no oscillation,
    // monotonic collapse on the losing side.
    let prod = SpeciesId(0);
    let strong = SpeciesId(1);
    let weak = SpeciesId(2);

    let species = vec![
        EcoSpecies {
            species_id: prod,
            role: EcosystemRole::Producer {
                metabolism: ProducerMetabolism::Photoautotroph,
            },
            biomass: Real::from_int(500),
            is_extant: true,
            low_biomass_streak: 0,
            habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
            tolerance: ToleranceEnvelope::aqueous_default(),
        },
        EcoSpecies {
            species_id: strong,
            role: EcosystemRole::PrimaryConsumer,
            biomass: Real::from_int(40),
            is_extant: true,
            low_biomass_streak: 0,
            habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
            tolerance: ToleranceEnvelope::aqueous_default(),
        },
        EcoSpecies {
            species_id: weak,
            role: EcosystemRole::PrimaryConsumer,
            biomass: Real::from_int(5),
            is_extant: true,
            low_biomass_streak: 0,
            habitat: Habitat::Terrestrial,
            cell_biomass: Vec::new(),
            tolerance: ToleranceEnvelope::aqueous_default(),
        },
    ];
    let mut matrix = InteractionMatrix::new();
    // Both species predate the producer.
    matrix.insert(
        strong,
        prod,
        Interaction {
            kind: InteractionKind::Predation,
            strength: Real::from((3, 100)),
            functional_response: FunctionalResponse::Saturating,
            half_saturation: Interaction::default_half_saturation(),
        },
    );
    matrix.insert(
        weak,
        prod,
        Interaction {
            kind: InteractionKind::Predation,
            strength: Real::from((3, 100)),
            functional_response: FunctionalResponse::Saturating,
            half_saturation: Interaction::default_half_saturation(),
        },
    );
    // Strong out-competes weak (symmetric competition with the
    // strong side winning because of initial-biomass asymmetry +
    // higher inflicted competition strength).
    matrix.insert(
        strong,
        weak,
        Interaction {
            kind: InteractionKind::Competition,
            strength: Real::from((8, 100)),
            functional_response: FunctionalResponse::Linear,
            half_saturation: Interaction::default_half_saturation(),
        },
    );
    matrix.insert(
        weak,
        strong,
        Interaction {
            kind: InteractionKind::Competition,
            strength: Real::from((2, 100)),
            functional_response: FunctionalResponse::Linear,
            half_saturation: Interaction::default_half_saturation(),
        },
    );

    let mut eco = PlanetEcosystem::new(species, matrix, Real::from_int(500));

    let weak_initial = eco.species.get(&weak).unwrap().biomass;
    let strong_initial = eco.species.get(&strong).unwrap().biomass;
    for _ in 0..1000 {
        eco.step();
    }
    let weak_final = eco.species.get(&weak).unwrap().biomass;
    let strong_final = eco.species.get(&strong).unwrap().biomass;

    // The weak species should collapse to a small fraction of its
    // starting biomass. The strong one should persist (not have
    // collapsed too).
    assert!(
        weak_final < weak_initial / Real::from_int(2),
        "weak species did not collapse (initial {weak_initial:?} -> final {weak_final:?})",
    );
    assert!(
        strong_final > weak_final,
        "strong competitor did not outlast weak (strong {strong_final:?} vs weak {weak_final:?})",
    );
    let _ = strong_initial;
}

#[test]
fn functional_response_linear_is_identity_in_prey() {
    let k = Real::from_int(10);
    let prey = Real::from_int(7);
    assert_eq!(
        functional_response(FunctionalResponse::Linear, prey, k),
        prey
    );
}

#[test]
fn functional_response_saturating_caps_at_one() {
    let k = Real::from_int(1);
    // prey → ∞: prey/(k+prey) → 1.
    let huge = Real::from_int(1_000_000);
    let r = functional_response(FunctionalResponse::Saturating, huge, k);
    assert!(r > Real::from((99, 100)));
    assert!(r <= Real::ONE);
}

#[test]
fn functional_response_sigmoidal_caps_at_one() {
    // Q32.32 can hold (2^31 - 1), so prey² must stay below that.
    // Pick prey = 1000, k = 1 → 1_000_000 / (1 + 1_000_000) ≈ 0.999.
    let k = Real::from_int(1);
    let big = Real::from_int(1_000);
    let r = functional_response(FunctionalResponse::Sigmoidal, big, k);
    assert!(r > Real::from((99, 100)));
    assert!(r <= Real::ONE);
}

#[test]
fn functional_response_saturating_uses_holling_type_ii() {
    // At prey == k, response = 0.5.
    let k = Real::from_int(4);
    let prey = Real::from_int(4);
    let r = functional_response(FunctionalResponse::Saturating, prey, k);
    assert_eq!(r, Real::from((1, 2)));
}

#[test]
fn functional_response_sigmoidal_uses_holling_type_iii() {
    // At prey == k, response = 0.5.
    let k = Real::from_int(4);
    let prey = Real::from_int(4);
    let r = functional_response(FunctionalResponse::Sigmoidal, prey, k);
    assert_eq!(r, Real::from((1, 2)));
}
