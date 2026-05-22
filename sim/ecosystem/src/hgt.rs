//! Horizontal gene transfer for `Lifecycle::Microbial` species
//! (Sprint 3 Item 11a, P3.3 plasmid-sweep rewrite).
//!
//! ## Mechanism
//!
//! Real prokaryote HGT is a *selection* event, not a smooth nudge.
//! A donor cell's plasmid jumps to a recipient; the plasmid then
//! either sweeps to fixation (the receiver's lineage *snaps* to
//! carrying the donor's trait value) or is lost when the carried
//! trait is misaligned with the recipient's local niche. The model
//! is two-phase:
//!
//! 1. **HGT trial (acquisition).** Each tick, every ordered pair of
//!    co-located `Microbial` species has a low per-tick probability of
//!    a one-way plasmid transfer. On success, the recipient receives a
//!    `Plasmid { trait_delta, trait_value }` carrying the donor's
//!    *value* (snapshot of the donor's actual trait at acquisition
//!    time). No interpolation, no immediate write to the recipient's
//!    own trait — just a deposit into the recipient's plasmid registry.
//! 2. **Selection (sweep-vs-loss).** Each tick, every plasmid in every
//!    species is evaluated against the local environmental conditions.
//!    The plasmid's *selection coefficient* compares the carried value
//!    to the local niche; if it would *widen the recipient's fit*
//!    (sweep score above [`SWEEP_THRESHOLD`]), the species' actual
//!    trait snaps to the plasmid value and the plasmid is removed
//!    (it has done its job). If the carried value is misaligned
//!    (sweep score below threshold), the plasmid is removed with
//!    probability proportional to its badness of fit
//!    (1 − fit_score).
//!
//! ## Why selection, not interpolation
//!
//! Smooth 5%-per-trial interpolation produces monotone trait drift
//! toward the population mean: two microbes either bury their
//! signal in the noise or homogenise to a midpoint that may not
//! survive their local cell. Real plasmid biology produces *bimodal*
//! outcomes — either the recipient "becomes" the donor on that
//! trait, or the plasmid is lost. The sweep/loss split also lets a
//! single rare HGT event matter: under the right local conditions a
//! single successful trial can shift a recipient's
//! `radiation_max` from 0.5 to 5.0 in one sweep, rather than
//! diluting the donor's extremophile adaptation by 95%.
//!
//! ## Determinism
//!
//! All randomness flows through `splitmix64` hashes of
//! `(rng_seed, tick, donor_id, recipient_id, slot)` for HGT trials
//! and `(rng_seed, tick, species_id, plasmid_id, slot)` for the
//! sweep/loss decision. No RNG state is threaded across calls;
//! rerunning a tick with the same inputs produces the same outcomes.
//! Plasmid `id`s are allocated monotonically from each species'
//! `next_plasmid_id` counter so iteration order is acquisition
//! order.
//!
//! ## Trait set
//!
//! Same four scalar axes as the legacy interpolation model:
//!
//! - `DormancyCapability` → `Species::dormancy_capability`
//! - `TemperatureToleranceLow` → `Species::tolerance.temp_range.0`
//! - `TemperatureToleranceHigh` → `Species::tolerance.temp_range.1`
//! - `RadiationMax` → `Species::tolerance.radiation_max`
//!
//! ## What HGT does *not* do here
//!
//! - Speciation — handled by Item 11 (separate module).
//! - Whole-genome replacement — at most a single trait axis snaps to
//!   the donor value per sweep.
//! - Lifecycle changes — `Lifecycle::Microbial` stays microbial.

use crate::speciation::clamp_cosmic_ray_multiplier;
use protocol::{HgtEvent, TraitName};
use sim_arith::Real;
use sim_species::{Lifecycle, Plasmid, Species, SpeciesId};
use std::collections::BTreeMap;

/// Per-pair per-tick base HGT acquisition probability. Calibrated so
/// an arbitrary pair of co-located Microbial species experiences one
/// plasmid acquisition per ~10 000 ticks (~833 sim years at monthly
/// cadence). Multiplied by `cell_overlap × time_overlap` to get the
/// realised probability for a specific pair.
pub const HGT_BASE_RATE: (i64, i64) = (1, 10_000);

/// Selection-coefficient threshold above which a plasmid sweeps to
/// fixation. The coefficient is the *improvement* in
/// `tolerance.match_score` (or the analogous scalar fit for
/// `DormancyCapability`) the plasmid would deliver if its value were
/// adopted. `0.05` = a 5% improvement is enough to fix; below that
/// the plasmid evaluates as a probabilistic-loss candidate.
pub const SWEEP_THRESHOLD: (i64, i64) = (5, 100);

/// Local environmental conditions evaluated against plasmid trait
/// values to compute the per-tick sweep/loss decision. Same five-axis
/// shape as `ToleranceEnvelope::match_score` so a plasmid's selection
/// coefficient is just "what `match_score` would become if I adopted
/// this value." Production callers feed the cell-aggregate
/// conditions from the planet's biota layer; tests construct it
/// directly.
#[derive(Debug, Clone, Copy)]
pub struct LocalConditions {
    pub temperature: Real,
    pub ph: Real,
    pub salinity: Real,
    pub radiation: Real,
    pub pressure: Real,
}

impl LocalConditions {
    /// Earth-surface baseline — 288 K, pH 7, salinity 35 g/L, low
    /// radiation, 1 atm. Useful as a default for tests that don't
    /// care about precise local conditions.
    #[must_use]
    pub fn earth_surface() -> Self {
        Self {
            temperature: Real::from_int(288),
            ph: Real::from_int(7),
            salinity: Real::from_int(35),
            radiation: Real::from_ratio(1, 10),
            pressure: Real::ONE,
        }
    }
}

/// Run one tick of horizontal gene transfer over every Microbial-
/// Microbial species pair in `species` *and* one tick of plasmid
/// sweep/loss evaluation across every microbial species' plasmid
/// registry. Returns the audit-trail of successful HGT
/// **acquisitions** (not sweeps) in `(donor_id, recipient_id)`-sorted
/// order.
///
/// - `rng_seed`: run-level seed (typically the world seed). Combined
///   with `tick + (donor_id, recipient_id, slot)` via `SplitMix64` to
///   derive the per-trial random draws.
/// - `tick`: current sim tick. Lets each tick re-roll independent
///   trials while staying deterministic.
/// - `cosmic_ray_multiplier`: scales the per-pair acquisition
///   probability (P1.2 magnetic-reversal coupling). Clamped to
///   `[0, 10]` by [`clamp_cosmic_ray_multiplier`] (T8 bidirectional —
///   a strong dipole truncates the multiplier to zero, suppressing
///   HGT trials).
/// - `local`: the planet-wide-aggregate environmental conditions
///   used to evaluate plasmid sweep/loss. Production callers feed
///   the per-cell or cell-aggregate conditions from the biota
///   layer; tests construct directly.
///
/// Non-Microbial species are silently skipped — every check is gated
/// on `Lifecycle::Microbial`. Mixed-lifecycle pairs (one Microbial,
/// one Vertebrate) never participate.
pub fn step_hgt(
    species: &mut BTreeMap<SpeciesId, Species>,
    tick: u64,
    rng_seed: u64,
    cosmic_ray_multiplier: Real,
    local: LocalConditions,
) -> Vec<HgtEvent> {
    // -----------------------------------------------------------------
    // Phase 1: HGT trials — deposit plasmids in recipients.
    // -----------------------------------------------------------------
    let events = run_acquisition_trials(species, tick, rng_seed, cosmic_ray_multiplier);

    // -----------------------------------------------------------------
    // Phase 2: Sweep / loss evaluation — for every microbial species,
    // walk its plasmid registry and either snap the trait + remove the
    // plasmid (sweep) or drop the plasmid (loss).
    // -----------------------------------------------------------------
    evaluate_plasmids(species, tick, rng_seed, local);

    events
}

/// Phase 1 of `step_hgt` — run per-pair HGT trials and deposit
/// successful acquisitions as new plasmids on the recipient.
fn run_acquisition_trials(
    species: &mut BTreeMap<SpeciesId, Species>,
    tick: u64,
    rng_seed: u64,
    cosmic_ray_multiplier: Real,
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
    // co-extant Microbial pair is treated as fully co-located.
    let cell_overlap = Real::ONE;
    let time_overlap = Real::ONE;
    let cosmic_mult_int = clamp_cosmic_ray_multiplier(cosmic_ray_multiplier);
    let cosmic_mult = Real::from_int(cosmic_mult_int as i64);
    let p_per_pair = base_rate * cell_overlap * time_overlap * cosmic_mult;

    let mut events: Vec<HgtEvent> = Vec::new();
    for (i, donor_id) in microbial_ids.iter().enumerate() {
        for recipient_id in microbial_ids.iter().skip(i + 1) {
            // Trial 1: donor_id → recipient_id.
            try_acquisition(
                species,
                tick,
                rng_seed,
                *donor_id,
                *recipient_id,
                p_per_pair,
                &mut events,
            );
            // Trial 2: recipient_id → donor_id (independent splitmix
            // draws via the swapped tuple).
            try_acquisition(
                species,
                tick,
                rng_seed,
                *recipient_id,
                *donor_id,
                p_per_pair,
                &mut events,
            );
        }
    }
    events
}

fn try_acquisition(
    species: &mut BTreeMap<SpeciesId, Species>,
    tick: u64,
    rng_seed: u64,
    donor_id: SpeciesId,
    recipient_id: SpeciesId,
    p: Real,
    events: &mut Vec<HgtEvent>,
) {
    // Probability draw via `SplitMix64` slot 0; trait-selection draw
    // via slot 1.
    let trial = splitmix64_for(rng_seed, tick, donor_id, recipient_id, 0);
    if !trial_succeeds(trial, p) {
        return;
    }
    let trait_pick = splitmix64_for(rng_seed, tick, donor_id, recipient_id, 1);
    let trait_name = pick_trait(trait_pick);

    // Snapshot the donor's value before the mutable borrow on the
    // recipient.
    let donor_value = match species.get(&donor_id) {
        Some(s) => read_trait(s, trait_name),
        None => return,
    };
    let recipient = match species.get_mut(&recipient_id) {
        Some(s) => s,
        None => return,
    };
    let plasmid_id = recipient.next_plasmid_id;
    recipient.next_plasmid_id = recipient.next_plasmid_id.saturating_add(1);
    recipient.plasmids.insert(
        plasmid_id,
        Plasmid {
            id: plasmid_id,
            trait_delta: trait_name,
            trait_value: donor_value,
            acquired_tick: tick,
        },
    );
    events.push(HgtEvent {
        tick,
        donor_id: donor_id.0,
        recipient_id: recipient_id.0,
        trait_swapped: trait_name,
    });
}

/// Phase 2 of `step_hgt` — walk every species' plasmid registry,
/// evaluate each plasmid against local conditions, and either sweep
/// (snap species' actual trait to plasmid value, drop plasmid) or
/// lose (drop plasmid with probability proportional to misfit).
///
/// Plasmid-id iteration order is `BTreeMap`-stable so replays are
/// byte-identical.
fn evaluate_plasmids(
    species: &mut BTreeMap<SpeciesId, Species>,
    tick: u64,
    rng_seed: u64,
    local: LocalConditions,
) {
    let sweep_threshold = Real::from(SWEEP_THRESHOLD);
    for (species_id, sp) in species.iter_mut() {
        if !matches!(sp.lifecycle, Lifecycle::Microbial { .. }) {
            continue;
        }
        if sp.plasmids.is_empty() {
            continue;
        }
        // Collect plasmid ids in deterministic order so we can mutate
        // `sp` while iterating.
        let plasmid_ids: Vec<u32> = sp.plasmids.keys().copied().collect();
        for pid in plasmid_ids {
            // Each plasmid may have been removed by an earlier sweep
            // on the same axis this tick; re-check.
            let plasmid = match sp.plasmids.get(&pid).copied() {
                Some(p) => p,
                None => continue,
            };
            let current = read_trait(sp, plasmid.trait_delta);
            let current_fit = fit_with_value(sp, plasmid.trait_delta, current, local);
            let candidate_fit =
                fit_with_value(sp, plasmid.trait_delta, plasmid.trait_value, local);
            let improvement = candidate_fit - current_fit;
            if improvement > sweep_threshold {
                // Sweep: snap the trait to the plasmid value and
                // retire the plasmid. The write_trait helper
                // preserves the lo ≤ hi temp invariant.
                write_trait(sp, plasmid.trait_delta, plasmid.trait_value);
                sp.plasmids.remove(&pid);
                continue;
            }
            // Loss: drop the plasmid with probability proportional to
            // its badness of fit. `1 - candidate_fit` ∈ [0, 1]; a
            // candidate that would perfectly match local conditions
            // is never dropped, a candidate that scores 0 is always
            // dropped this tick.
            let loss_prob = (Real::ONE - candidate_fit).clamp01();
            let draw = splitmix64_for(rng_seed, tick, *species_id, SpeciesId(pid), 2);
            if trial_succeeds(draw, loss_prob) {
                sp.plasmids.remove(&pid);
            }
        }
    }
}

/// Compute the would-be tolerance-fit score for a hypothetical
/// scenario in which the species' `trait_name` axis took the value
/// `value`. For temperature axes we rebuild the envelope; for
/// `RadiationMax` we substitute `radiation_max`; for
/// `DormancyCapability` we use a 1-D "value matches local
/// hostility" score because dormancy isn't an envelope axis.
fn fit_with_value(
    sp: &Species,
    trait_name: TraitName,
    value: Real,
    local: LocalConditions,
) -> Real {
    match trait_name {
        TraitName::DormancyCapability => {
            // Dormancy is adaptive when local conditions sit outside
            // the species' tolerance envelope — high dormancy buys
            // survival under hostile conditions. We define fit as
            // `1 - |dormancy_target - value|` where `dormancy_target`
            // is `1.0` if the local conditions fall *outside* the
            // species' tolerance envelope (high-dormancy adaptive)
            // and `0.0` if they sit comfortably *inside*
            // (low-dormancy adaptive: dormancy is metabolically
            // costly when not needed).
            let in_envelope = sp.tolerance.contains(
                local.temperature,
                local.ph,
                local.salinity,
                local.radiation,
                local.pressure,
            );
            let target = if in_envelope { Real::ZERO } else { Real::ONE };
            let diff = (value - target).abs();
            (Real::ONE - diff).clamp01()
        }
        TraitName::TemperatureToleranceLow => {
            let mut tol = sp.tolerance;
            // Preserve the lo ≤ hi invariant: if `value > hi`, treat
            // it as a swap (axis_score would silently return 0 for
            // an inverted envelope).
            let (lo, hi) = if value <= tol.temp_range.1 {
                (value, tol.temp_range.1)
            } else {
                (tol.temp_range.1, value)
            };
            tol.temp_range = (lo, hi);
            tol.match_score(
                local.temperature,
                local.ph,
                local.salinity,
                local.radiation,
                local.pressure,
            )
        }
        TraitName::TemperatureToleranceHigh => {
            let mut tol = sp.tolerance;
            let (lo, hi) = if value >= tol.temp_range.0 {
                (tol.temp_range.0, value)
            } else {
                (value, tol.temp_range.0)
            };
            tol.temp_range = (lo, hi);
            tol.match_score(
                local.temperature,
                local.ph,
                local.salinity,
                local.radiation,
                local.pressure,
            )
        }
        TraitName::RadiationMax => {
            let mut tol = sp.tolerance;
            tol.radiation_max = value.max(Real::ZERO);
            tol.match_score(
                local.temperature,
                local.ph,
                local.salinity,
                local.radiation,
                local.pressure,
            )
        }
    }
}

/// Read the scalar trait value off a species. Centralised here so
/// the trial / sweep paths don't reach into the species struct from
/// multiple places.
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
/// sweep crossed.
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
/// both sit safely inside the 32-bit integer side of the format.
fn trial_succeeds(hash: u64, p: Real) -> bool {
    let bits = (hash >> 40) as i64; // in [0, 2^24)
    let u = Real::from_ratio(bits, 1_i64 << 24);
    u < p
}

/// `SplitMix64` hash of `(seed, tick, donor_id, recipient_id, slot)`.
/// Same fold pattern as `CognitionAxes::from_scalar_with_seed` and
/// `derive_tolerance_envelope`.
fn splitmix64_for(
    seed: u64,
    tick: u64,
    donor_id: SpeciesId,
    recipient_id: SpeciesId,
    slot: u64,
) -> u64 {
    let mut z = seed;
    z = z.wrapping_add(tick.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = z.wrapping_add(u64::from(donor_id.0).wrapping_mul(0xBF58_476D_1CE4_E5B9));
    z = z.wrapping_add(u64::from(recipient_id.0).wrapping_mul(0x94D0_49BB_1331_11EB));
    z = z.wrapping_add(slot.wrapping_mul(0xD2B7_4407_B1CE_6E93));
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
            plasmids: BTreeMap::new(),
            next_plasmid_id: 0,
            is_extant: true,
        };
        (species_id, species)
    }

    /// Earth-surface local conditions sitting comfortably inside the
    /// aqueous-default tolerance envelope.
    fn benign_local() -> LocalConditions {
        LocalConditions::earth_surface()
    }

    /// P3.3 acceptance test #1: a successful HGT trial *adds* a
    /// plasmid to the recipient's registry (no smooth interpolation).
    #[test]
    fn successful_hgt_adds_plasmid_to_recipient() {
        // Two Microbial species; donor has an extremophile-grade
        // radiation_max so any RadiationMax plasmid it deposits is a
        // candidate for sweep (depending on tick local conditions).
        let (id_a, mut donor) = make_microbial(1, Real::from_ratio(5, 10));
        let (id_b, recipient) = make_microbial(2, Real::from_ratio(5, 10));
        donor.tolerance = ToleranceEnvelope {
            temp_range: (Real::from_int(200), Real::from_int(500)),
            ph_range: (Real::ZERO, Real::from_int(14)),
            salinity_range: (Real::ZERO, Real::from_int(200)),
            radiation_max: Real::from_int(20),
            pressure_range: (Real::ONE, Real::from_int(100)),
        };
        let mut species: BTreeMap<SpeciesId, Species> = BTreeMap::new();
        species.insert(id_a, donor);
        species.insert(id_b, recipient);

        // Local conditions chosen so that every plasmid scores a
        // near-perfect fit when adopted (well inside both species'
        // envelopes after merge). This keeps the loss probability low
        // enough that the first acquisition isn't immediately
        // re-evaluated and dropped on the very same tick the
        // acquisition fires. (Sweep is also avoided by keeping the
        // local conditions inside *both* species' current envelopes
        // for non-radiation axes.)
        let local = LocalConditions {
            temperature: Real::from_int(300),
            ph: Real::from_int(7),
            salinity: Real::from_int(20),
            radiation: Real::ZERO,
            pressure: Real::ONE,
        };

        // Track the maximum plasmid count observed across the run —
        // the post-tick state may have already swept (and retired)
        // the plasmid, so we must observe the *peak* count, not the
        // final.
        let mut peak_plasmids = 0usize;
        let mut acquisition_events = 0usize;
        for tick in 0..200_000u64 {
            let events = step_hgt(&mut species, tick, 0xC0FF_EE42, Real::ONE, local);
            acquisition_events += events.len();
            let recipient_count = species[&id_b].plasmids.len();
            let donor_count = species[&id_a].plasmids.len();
            peak_plasmids = peak_plasmids.max(recipient_count).max(donor_count);
            if peak_plasmids > 0 && acquisition_events > 0 {
                // Acquisition fired and we observed a plasmid in
                // someone's registry at least once → spec satisfied.
                return;
            }
        }

        // Probabilistic sanity: at p=1e-4 per pair per direction × 2
        // directions over 200k ticks, expected ~40 acquisitions. If
        // we observed zero, something is broken.
        assert!(
            acquisition_events > 0,
            "no HGT acquisitions over 200k ticks (expected ~40)",
        );
        assert!(
            peak_plasmids > 0,
            "HGT events fired but no plasmid was ever observed in any \
             recipient's registry — acquisition path is broken",
        );
    }

    /// P3.3 acceptance test #2: when local conditions favour the
    /// plasmid's carried value, the plasmid sweeps to fixation
    /// (recipient's actual trait snaps to plasmid value within a
    /// small number of ticks).
    #[test]
    fn plasmid_sweeps_when_selection_favours_trait() {
        // Set up: recipient starts with aqueous-default
        // radiation_max=0.5. Hand-inject a plasmid carrying
        // radiation_max=20 (extremophile). Pick local conditions
        // with radiation=5 — well above the recipient's current
        // ceiling (fit score = 0) and well inside the plasmid's
        // (fit score >> 0). The selection improvement is large,
        // sweep must fire on the first eligible evaluation.
        let (_id, mut sp) = make_microbial(1, Real::from_ratio(5, 10));
        // Preconditions: aqueous_default radiation_max = 0.5.
        assert_eq!(sp.tolerance.radiation_max, Real::from_ratio(5, 10));
        sp.plasmids.insert(
            0,
            Plasmid {
                id: 0,
                trait_delta: TraitName::RadiationMax,
                trait_value: Real::from_int(20),
                acquired_tick: 0,
            },
        );
        sp.next_plasmid_id = 1;

        let mut species: BTreeMap<SpeciesId, Species> = BTreeMap::new();
        species.insert(SpeciesId(1), sp);

        // Local conditions: radiation = 5 (above current ceiling,
        // well inside plasmid's). Temperature centred in aqueous
        // (273, 373) so radiation is the differentiating axis.
        let local = LocalConditions {
            temperature: Real::from_int(300),
            ph: Real::from_int(7),
            salinity: Real::from_int(20),
            radiation: Real::from_int(5),
            pressure: Real::ONE,
        };

        // Run a handful of ticks (a single sweep evaluation should
        // suffice; budget of 16 gives slack).
        for tick in 0..16u64 {
            step_hgt(&mut species, tick, 0xC0FF_EE42, Real::ONE, local);
        }

        // Post-condition: species' radiation_max snapped to 20 and
        // the plasmid is gone (sweeps retire the plasmid).
        let final_rad = species[&SpeciesId(1)].tolerance.radiation_max;
        assert_eq!(
            final_rad,
            Real::from_int(20),
            "plasmid did not sweep: radiation_max stayed at {final_rad:?}",
        );
        assert!(
            species[&SpeciesId(1)].plasmids.is_empty(),
            "plasmid not retired after sweep: {:?}",
            species[&SpeciesId(1)].plasmids,
        );
    }

    /// P3.3 acceptance test #3: when local conditions disfavour the
    /// plasmid's carried value, the plasmid is dropped within a
    /// small number of ticks (probabilistic loss proportional to
    /// misfit).
    #[test]
    fn plasmid_lost_when_selection_disfavours_trait() {
        // Set up: recipient with aqueous-default tolerance
        // (radiation_max=0.5, temp_range=(273, 373)). Hand-inject a
        // plasmid carrying TemperatureToleranceHigh=200 — *narrower*
        // than the current envelope. Pick local conditions at
        // temp=350 (still inside current envelope but *outside* the
        // proposed narrower one). Adopting the plasmid would push
        // the temp envelope's upper edge below the actual local
        // temperature → match_score collapses → fit_score for the
        // plasmid is 0 → loss probability ≈ 1.0.
        let (_id, mut sp) = make_microbial(1, Real::from_ratio(5, 10));
        sp.plasmids.insert(
            0,
            Plasmid {
                id: 0,
                trait_delta: TraitName::TemperatureToleranceHigh,
                trait_value: Real::from_int(200),
                acquired_tick: 0,
            },
        );
        sp.next_plasmid_id = 1;

        let mut species: BTreeMap<SpeciesId, Species> = BTreeMap::new();
        species.insert(SpeciesId(1), sp);

        // Local: temp=350 (inside current envelope (273, 373)).
        let local = LocalConditions {
            temperature: Real::from_int(350),
            ph: Real::from_int(7),
            salinity: Real::from_int(20),
            radiation: Real::from_ratio(1, 10),
            pressure: Real::ONE,
        };

        // The plasmid's adopted envelope (200, 273) excludes
        // temp=350 → candidate_fit = 0 → loss probability 100%.
        // After one tick the plasmid is gone.
        let mut last_seen = 0u64;
        let mut dropped_at: Option<u64> = None;
        for tick in 0..16u64 {
            step_hgt(&mut species, tick, 0xC0FF_EE42, Real::ONE, local);
            if species[&SpeciesId(1)].plasmids.is_empty() {
                dropped_at = Some(tick);
                break;
            }
            last_seen = tick;
        }
        assert!(
            dropped_at.is_some(),
            "plasmid still present after 16 ticks (last seen tick {last_seen})",
        );
        // Also assert the species' actual trait was *not* shifted —
        // a loss must not modify the host's trait.
        let temp_hi = species[&SpeciesId(1)].tolerance.temp_range.1;
        assert_eq!(
            temp_hi,
            Real::from_int(373),
            "host's TemperatureToleranceHigh shifted from a loss (expected 373, got {temp_hi:?})",
        );
    }

    /// Regression: mixed-lifecycle pair (Vertebrate + Microbial)
    /// must never produce HGT acquisitions; a Microbial + Microbial
    /// pair must.
    #[test]
    fn hgt_only_fires_for_microbial_lifecycle() {
        // Case 1: Vertebrate + Microbial — no events ever, no
        // plasmids ever.
        let mut species: BTreeMap<SpeciesId, Species> = BTreeMap::new();
        let (id_a, mut a) = make_microbial(1, Real::from_ratio(1, 10));
        a.lifecycle = Lifecycle::Vertebrate;
        let (id_b, b) = make_microbial(2, Real::from_ratio(9, 10));
        species.insert(id_a, a);
        species.insert(id_b, b);

        let local = benign_local();
        let mut events: Vec<HgtEvent> = Vec::new();
        for tick in 0..200_000u64 {
            let e = step_hgt(&mut species, tick, 0xC0FF_EE42, Real::ONE, local);
            events.extend(e);
        }
        assert!(
            events.is_empty(),
            "mixed-lifecycle pair produced {} HGT events (expected 0)",
            events.len(),
        );
        assert!(
            species[&id_a].plasmids.is_empty(),
            "vertebrate side received a plasmid (expected 0)",
        );
        assert!(
            species[&id_b].plasmids.is_empty(),
            "microbial side received a plasmid from vertebrate donor (expected 0)",
        );

        // Case 2: Microbial + Microbial — at least one acquisition
        // event over 200k ticks.
        let mut species2: BTreeMap<SpeciesId, Species> = BTreeMap::new();
        let (id_c, c) = make_microbial(1, Real::from_ratio(1, 10));
        let (id_d, d) = make_microbial(2, Real::from_ratio(9, 10));
        species2.insert(id_c, c);
        species2.insert(id_d, d);

        let mut events2: Vec<HgtEvent> = Vec::new();
        for tick in 0..200_000u64 {
            let e = step_hgt(&mut species2, tick, 0xC0FF_EE42, Real::ONE, local);
            events2.extend(e);
        }
        assert!(
            !events2.is_empty(),
            "microbial-microbial pair produced 0 events in 200k ticks (expected ~40)",
        );
    }
}
