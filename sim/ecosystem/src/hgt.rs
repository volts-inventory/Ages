//! Horizontal gene transfer for `Lifecycle::Microbial` species
//! (Sprint 3 Item 11a).
//!
//! Prokaryotes dominate evolutionary innovation through HGT, not
//! through vertical descent. Two co-located microbial species in the
//! same niche have a low per-tick probability of swapping a trait —
//! one species' tolerance for high radiation or a wider temperature
//! envelope "leaks" into the other. We model this as a deterministic,
//! per-tick interpolation rather than a literal copy: each successful
//! trial nudges the recipient toward the donor by
//! `0.05` of the value difference.
//!
//! ## Determinism
//!
//! All randomness flows through a `splitmix64` hash of
//! `(rng_seed, tick, donor_id, recipient_id, slot)`. No RNG state is
//! threaded across calls; rerunning a tick with the same inputs
//! produces the same trial outcomes and the same per-tick event
//! ordering. Species iteration walks the `BTreeMap` in `SpeciesId`
//! order so the unordered pair `(a, b)` is always considered as
//! donor `a → b` first, then `b → a`, both sides getting
//! independent trials.
//!
//! ## Probability
//!
//! Per pair per tick:
//!
//! ```text
//! p = HGT_BASE_RATE × cell_overlap × time_overlap
//! ```
//!
//! `cell_overlap` defaults to `1.0` until the spatial layer wires in
//! per-species cell occupancy (Item 11a documents the proxy in the
//! plan). `time_overlap` is always `1.0` in a single tick. With
//! `HGT_BASE_RATE = 1e-4` an average pair experiences one HGT event
//! per ~10 000 ticks (~833 sim years on monthly cadence) — rare
//! enough that two arbitrarily-chosen Microbial species don't
//! homogenise, fast enough that meaningful gene flow accumulates
//! over geologic time.
//!
//! ## Trait set
//!
//! Four scalar axes are eligible for transfer in this PR:
//!
//! - `DormancyCapability` → `Species::dormancy_capability`
//! - `TemperatureToleranceLow` → `Species::tolerance.temp_range.0`
//! - `TemperatureToleranceHigh` → `Species::tolerance.temp_range.1`
//! - `RadiationMax` → `Species::tolerance.radiation_max`
//!
//! The enum is extensible — additional trait axes can be added
//! without breaking the wire schema (the variants serialise as
//! snake-case strings). Per-trial trait selection is uniform over
//! the four axes via the second `SplitMix64` draw.
//!
//! ## What HGT does *not* do here
//!
//! - Speciation — handled by Item 11 (separate agent).
//! - Whole-genome replacement — interpolation only, no overwrite.
//! - Lifecycle changes — `Lifecycle::Microbial` stays microbial; the
//!   `fission_strategy` field is not eligible for swap (it's an
//!   enum, not a scalar). HGT of fission strategies in real life
//!   would correspond to a much rarer plasmid event; outside scope
//!   for the per-tick low-rate trial.

use protocol::{HgtEvent, TraitName};
use sim_arith::Real;
use sim_species::{Lifecycle, Species, SpeciesId};
use std::collections::BTreeMap;

/// Per-pair per-tick base HGT probability. Calibrated so an
/// arbitrary pair of co-located Microbial species experiences one
/// HGT event per ~10 000 ticks (~833 sim years at monthly cadence).
/// Multiplied by `cell_overlap × time_overlap` to get the realised
/// probability for a specific pair.
pub const HGT_BASE_RATE: (i64, i64) = (1, 10_000);

/// Fraction of the donor-recipient value gap pulled into the
/// recipient on each successful HGT trial. With `HGT_INTERPOLATION
/// = 0.05` a single trial moves the recipient 5% of the way toward
/// the donor; long-run repeated trials drive the pair toward the
/// midpoint without ever literal-copying the donor value.
pub const HGT_INTERPOLATION: (i64, i64) = (5, 100);

/// Run one tick of horizontal gene transfer over every Microbial-
/// Microbial species pair in `species`. Returns the audit-trail of
/// successful HGT events in `(donor_id, recipient_id)`-sorted order.
///
/// - `rng_seed`: run-level seed (typically the world seed). Combined
///   with `tick + (donor_id, recipient_id)` via `SplitMix64` to derive
///   the per-trial random draws.
/// - `tick`: current sim tick. Lets each tick re-roll independent
///   trials while staying deterministic.
///
/// Non-Microbial species are silently skipped — the early
/// `lifecycle` check is the only place this path checks for
/// Microbial. Mixed-lifecycle pairs (one Microbial, one Vertebrate)
/// never participate.
pub fn step_hgt(
    species: &mut BTreeMap<SpeciesId, Species>,
    tick: u64,
    rng_seed: u64,
) -> Vec<HgtEvent> {
    // Collect Microbial species ids in sorted order — BTreeMap
    // iteration is deterministic.
    let microbial_ids: Vec<SpeciesId> = species
        .iter()
        .filter_map(|(id, s)| {
            if matches!(s.lifecycle, Lifecycle::Microbial { .. }) {
                Some(*id)
            } else {
                None
            }
        })
        .collect();

    if microbial_ids.len() < 2 {
        return Vec::new();
    }

    let base_rate = Real::from(HGT_BASE_RATE);
    // Cell-overlap proxy: spatial cell-occupancy data isn't surfaced
    // through `Species` yet (the per-cell biota layer is downstream
    // of this module). Use `1.0` as the documented proxy — every
    // co-extant Microbial pair is treated as fully co-located. A
    // future polish pass can plumb per-cell overlap through to here
    // by multiplying `cell_overlap` against the cell-set intersection
    // size before the trial.
    let cell_overlap = Real::ONE;
    // Time-overlap is always 1.0 within a single tick (both species
    // are extant for the full tick window or they aren't).
    let time_overlap = Real::ONE;
    let p_per_pair = base_rate * cell_overlap * time_overlap;
    let interp = Real::from(HGT_INTERPOLATION);
    let one_minus_interp = Real::ONE - interp;

    let mut events: Vec<HgtEvent> = Vec::new();
    // Walk ordered pairs (a, b) with a < b, then run two independent
    // trials per pair: a → b (donor a) and b → a (donor b). Real-
    // life HGT is directional; modelling both directions gives both
    // species independent chances to acquire each other's traits in
    // the same tick.
    for (i, donor_id) in microbial_ids.iter().enumerate() {
        for recipient_id in microbial_ids.iter().skip(i + 1) {
            // Trial 1: donor_id → recipient_id.
            try_trial(
                species,
                tick,
                rng_seed,
                *donor_id,
                *recipient_id,
                p_per_pair,
                interp,
                one_minus_interp,
                &mut events,
            );
            // Trial 2: recipient_id → donor_id (swap the roles for
            // the second trial; the splitmix draw uses the swapped
            // (donor, recipient) tuple so the two trials are
            // independent).
            try_trial(
                species,
                tick,
                rng_seed,
                *recipient_id,
                *donor_id,
                p_per_pair,
                interp,
                one_minus_interp,
                &mut events,
            );
        }
    }
    events
}

#[allow(clippy::too_many_arguments)]
fn try_trial(
    species: &mut BTreeMap<SpeciesId, Species>,
    tick: u64,
    rng_seed: u64,
    donor_id: SpeciesId,
    recipient_id: SpeciesId,
    p: Real,
    interp: Real,
    one_minus_interp: Real,
    events: &mut Vec<HgtEvent>,
) {
    // Probability draw via `SplitMix64` slot 0; trait-selection draw
    // via slot 1. Independent seeds keep the two draws uncorrelated.
    let trial = splitmix64_for(rng_seed, tick, donor_id, recipient_id, 0);
    if !trial_succeeds(trial, p) {
        return;
    }
    let trait_pick = splitmix64_for(rng_seed, tick, donor_id, recipient_id, 1);
    let trait_name = pick_trait(trait_pick);

    // Read donor's trait value (immutable borrow) before the mutable
    // borrow on the recipient. Skip silently if either id has been
    // removed between the collection pass and now (shouldn't happen
    // — BTreeMap iteration order is stable — but the guard keeps the
    // function total).
    let donor_value = match species.get(&donor_id) {
        Some(s) => read_trait(s, trait_name),
        None => return,
    };
    let recipient = match species.get_mut(&recipient_id) {
        Some(s) => s,
        None => return,
    };
    let recipient_value = read_trait(recipient, trait_name);
    let new_value = recipient_value * one_minus_interp + donor_value * interp;
    write_trait(recipient, trait_name, new_value);
    events.push(HgtEvent {
        tick,
        donor_id: donor_id.0,
        recipient_id: recipient_id.0,
        trait_swapped: trait_name,
    });
}

/// Read the scalar trait value off a species. Centralised here so the
/// trial path doesn't reach into the species struct from two places.
fn read_trait(s: &Species, trait_name: TraitName) -> Real {
    match trait_name {
        TraitName::DormancyCapability => s.dormancy_capability,
        TraitName::TemperatureToleranceLow => s.tolerance.temp_range.0,
        TraitName::TemperatureToleranceHigh => s.tolerance.temp_range.1,
        TraitName::RadiationMax => s.tolerance.radiation_max,
    }
}

/// Write the scalar trait value back into a species. Temperature
/// edits preserve the `lo <= hi` invariant by re-ordering if the
/// interpolation crossed.
fn write_trait(s: &mut Species, trait_name: TraitName, value: Real) {
    match trait_name {
        TraitName::DormancyCapability => {
            s.dormancy_capability = value.clamp01();
        }
        TraitName::TemperatureToleranceLow => {
            let hi = s.tolerance.temp_range.1;
            let lo = value;
            if lo <= hi {
                s.tolerance.temp_range.0 = lo;
            } else {
                // Crossed — swap so the envelope stays valid.
                s.tolerance.temp_range.0 = hi;
                s.tolerance.temp_range.1 = lo;
            }
        }
        TraitName::TemperatureToleranceHigh => {
            let lo = s.tolerance.temp_range.0;
            let hi = value;
            if lo <= hi {
                s.tolerance.temp_range.1 = hi;
            } else {
                s.tolerance.temp_range.0 = hi;
                s.tolerance.temp_range.1 = lo;
            }
        }
        TraitName::RadiationMax => {
            s.tolerance.radiation_max = value.max(Real::ZERO);
        }
    }
}

/// Uniform-over-four-axes trait selection from a 64-bit hash draw.
/// `low_bits % 4` would also work; the `as u8` + match form avoids
/// an integer modulo and reads as the small explicit table it is.
fn pick_trait(hash: u64) -> TraitName {
    match (hash & 0b11) as u8 {
        0 => TraitName::DormancyCapability,
        1 => TraitName::TemperatureToleranceLow,
        2 => TraitName::TemperatureToleranceHigh,
        _ => TraitName::RadiationMax,
    }
}

/// Convert a `SplitMix64` draw into a `[0, 1)` uniform `Real` and
/// compare against `p`. We sample the high 24 bits — `Real` is
/// Q32.32, so the denominator `2^24` and the numerator `[0, 2^24)`
/// both sit safely inside the 32-bit integer side of the format
/// (anything past `2^31` overflows `Real::from_ratio`). 24 bits give
/// ~1.7e7 distinct probability levels, well below the `1e-4` per-
/// trial probability we care about — granularity is not a concern.
fn trial_succeeds(hash: u64, p: Real) -> bool {
    // Take the high 24 bits as an unsigned magnitude in [0, 2^24).
    let bits = (hash >> 40) as i64; // in [0, 2^24)
    let u = Real::from_ratio(bits, 1_i64 << 24);
    u < p
}

/// `SplitMix64` hash of `(seed, tick, donor_id, recipient_id, slot)`.
/// Same fold pattern as `CognitionAxes::from_scalar_with_seed` and
/// `derive_tolerance_envelope` — deterministic, no RNG state,
/// produces ~uniform 64-bit output for any input combination.
fn splitmix64_for(
    seed: u64,
    tick: u64,
    donor_id: SpeciesId,
    recipient_id: SpeciesId,
    slot: u64,
) -> u64 {
    // Fold the five inputs into one 64-bit seed via the
    // `0x9E37_79B9_7F4A_7C15` (2^64 / phi) golden-ratio constant.
    // Each input gets its own multiplier so different inputs don't
    // alias each other.
    let mut z = seed;
    z = z.wrapping_add(tick.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = z.wrapping_add(u64::from(donor_id.0).wrapping_mul(0xBF58_476D_1CE4_E5B9));
    z = z.wrapping_add(u64::from(recipient_id.0).wrapping_mul(0x94D0_49BB_1331_11EB));
    z = z.wrapping_add(slot.wrapping_mul(0xD2B7_4407_B1CE_6E93));
    // `SplitMix64` finaliser.
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_species::{
        CognitionAxes, CognitionTopology, EcosystemRole, Fission, Habitat,
        PopulationBiology, ProducerMetabolism, ToleranceEnvelope,
    };
    use std::collections::{BTreeMap, BTreeSet};

    /// Minimal Microbial species fixture — uses sensible defaults so
    /// tests only set the fields they care about.
    fn make_microbial(id: u32, dormancy: Real) -> (SpeciesId, Species) {
        let species_id = SpeciesId(id);
        let species = Species {
            seed: u64::from(id),
            name: format!("Microbe-{id}"),
            cognition: Real::from_ratio(1, 10),
            cognition_axes: CognitionAxes::uniform(Real::from_ratio(1, 10)),
            sociality: Real::from_ratio(1, 10),
            communication_fidelity: Real::from_ratio(1, 10),
            lifespan_years: Real::from_int(1),
            modalities: Vec::new(),
            manipulation_modes: Vec::new(),
            perceivable_templates: BTreeSet::new(),
            t0_loss: Real::from_ratio(1, 10),
            cognition_topology: CognitionTopology::DistributedRedundant,
            habitat: Habitat::Aquatic,
            discovered_templates: BTreeMap::new(),
            next_discovered_template_id: 1000,
            dynamic_tool_registry: BTreeMap::new(),
            next_dynamic_tool_id: 1000,
            initial_cosmology: [Real::ZERO; 5],
            biology: PopulationBiology {
                clutch_size: Real::from_int(100),
                infant_fraction: Real::from_ratio(1, 100),
                maturity_fraction: Real::from_ratio(1, 100),
                eldership_fraction: Real::ZERO,
                infant_survival: Real::from_ratio(5, 100),
                juvenile_survival: Real::from_ratio(20, 100),
                food_multipliers: [
                    Real::from_ratio(3, 10),
                    Real::from_ratio(6, 10),
                    Real::ONE,
                    Real::from_ratio(9, 10),
                ],
                events_per_fertile_window: Real::ONE,
                reproductive_success: Real::ZERO,
            },
            tolerance: ToleranceEnvelope::aqueous_default(),
            lifecycle: Lifecycle::Microbial {
                fission_strategy: Fission::Binary,
            },
            role: EcosystemRole::Producer {
                metabolism: ProducerMetabolism::Chemoautotroph,
            },
            dormancy_capability: dormancy,
            is_extant: true,
        };
        (species_id, species)
    }

    /// Vertebrate fixture for the "only microbial" test.
    fn make_vertebrate(id: u32, dormancy: Real) -> (SpeciesId, Species) {
        let (sid, mut s) = make_microbial(id, dormancy);
        s.lifecycle = Lifecycle::Vertebrate;
        (sid, s)
    }

    /// Two Microbial species with dormancy 0.1 and 0.9 should
    /// converge toward each other when stepped for many ticks. The
    /// HGT base rate is `1e-4`, so on average ~1 trial per ~10k
    /// ticks per direction — run for 200k ticks and assert that
    /// both species' dormancy values have moved off their starting
    /// extremes by a measurable amount.
    #[test]
    fn hgt_propagates_trait_between_colocated_microbial_species() {
        let mut species: BTreeMap<SpeciesId, Species> = BTreeMap::new();
        let (id_a, a) = make_microbial(1, Real::from_ratio(1, 10));
        let (id_b, b) = make_microbial(2, Real::from_ratio(9, 10));
        species.insert(id_a, a);
        species.insert(id_b, b);

        let start_a = species[&id_a].dormancy_capability;
        let start_b = species[&id_b].dormancy_capability;
        let start_gap = (start_b - start_a).abs();

        let mut total_events = 0usize;
        let mut dormancy_events = 0usize;
        for tick in 0..200_000u64 {
            let events = step_hgt(&mut species, tick, 0xC0FF_EE42);
            for e in &events {
                total_events += 1;
                if matches!(e.trait_swapped, TraitName::DormancyCapability) {
                    dormancy_events += 1;
                }
            }
        }

        // Probabilistic sanity: at p=1e-4 per pair per direction with
        // 2 directions, expected events over 200k ticks ≈ 40, so
        // total_events should be well above 0 deterministically.
        assert!(
            total_events > 0,
            "no HGT events fired in 200k ticks (expected ~40)",
        );
        assert!(
            dormancy_events > 0,
            "no DormancyCapability swaps in 200k ticks — \
             trait-selection draw never picked variant 0",
        );

        let end_a = species[&id_a].dormancy_capability;
        let end_b = species[&id_b].dormancy_capability;
        let end_gap = (end_b - end_a).abs();

        // Convergence: the gap between the two species' dormancy
        // values must shrink. The interpolation rule
        // `r = r*0.95 + d*0.05` strictly pulls the recipient
        // toward the donor, so any successful DormancyCapability
        // trial reduces |b - a|. We assert a meaningful (>5%)
        // shrink rather than strict-monotone since the trait-
        // selection draw also picks the other three axes.
        assert!(
            end_gap < start_gap,
            "expected gap to shrink (start {start_gap:?} -> end {end_gap:?})",
        );
        // Stronger check: at least one dormancy_event fired, so the
        // shrinkage must reflect at least one 5% pull. With one
        // event the gap drops by ~5%; we use a loose 1% floor to
        // stay robust against the precise number of swaps that
        // fired.
        let one_percent = Real::from_ratio(1, 100);
        let gap_drop = start_gap - end_gap;
        assert!(
            gap_drop > start_gap * one_percent,
            "expected gap to drop by >1% (drop {gap_drop:?} vs \
             start {start_gap:?})",
        );
    }

    /// Mixed-lifecycle pair (Vertebrate + Microbial) must never
    /// produce HGT events; a Microbial + Microbial pair must.
    #[test]
    fn hgt_only_fires_for_microbial_lifecycle() {
        // Case 1: Vertebrate + Microbial — no events ever.
        let mut species: BTreeMap<SpeciesId, Species> = BTreeMap::new();
        let (id_a, a) = make_vertebrate(1, Real::from_ratio(1, 10));
        let (id_b, b) = make_microbial(2, Real::from_ratio(9, 10));
        species.insert(id_a, a);
        species.insert(id_b, b);

        let mut events: Vec<HgtEvent> = Vec::new();
        for tick in 0..200_000u64 {
            let e = step_hgt(&mut species, tick, 0xC0FF_EE42);
            events.extend(e);
        }
        assert!(
            events.is_empty(),
            "mixed-lifecycle pair produced {} HGT events (expected 0)",
            events.len(),
        );

        // Verify the Vertebrate species' traits stayed pinned at
        // their starting values — HGT must not touch them at all.
        let v_dorm = species[&id_a].dormancy_capability;
        assert_eq!(
            v_dorm,
            Real::from_ratio(1, 10),
            "vertebrate dormancy moved despite no events",
        );

        // Case 2: Microbial + Microbial — at least one event over
        // the same 200k ticks (sanity that the setup is otherwise
        // identical and the gate is the *only* difference).
        let mut species2: BTreeMap<SpeciesId, Species> = BTreeMap::new();
        let (id_c, c) = make_microbial(1, Real::from_ratio(1, 10));
        let (id_d, d) = make_microbial(2, Real::from_ratio(9, 10));
        species2.insert(id_c, c);
        species2.insert(id_d, d);

        let mut events2: Vec<HgtEvent> = Vec::new();
        for tick in 0..200_000u64 {
            let e = step_hgt(&mut species2, tick, 0xC0FF_EE42);
            events2.extend(e);
        }
        assert!(
            !events2.is_empty(),
            "microbial-microbial pair produced 0 events in 200k \
             ticks (expected ~40)",
        );
    }
}
