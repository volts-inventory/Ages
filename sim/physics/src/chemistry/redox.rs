//! Multi-oxidiser redox ladder (Sprint 2 Item 9).
//!
//! Each `MetabolicSubstrate` exposes a list of `Oxidiser`s ranked by
//! standard reduction potential (volts). The single-oxidiser
//! abstraction the rest of `chemistry` ships (one `Substance::Oxidiser`
//! channel, one combustion reaction per fuel) is preserved as the
//! kinetic kernel; this module sits *alongside* it and supplies the
//! ladder data that `sim-ecosystem` uses to partition
//! `Chemoautotroph` producer growth.
//!
//! ## Why volts
//!
//! Reduction potential `E°` (V vs. SHE) ranks how strongly an
//! oxidiser pulls electrons. Higher = stronger oxidiser:
//!
//! - `O2` (+1.23 V) — Earth's atmospheric workhorse.
//! - `NO3-` (+0.96 V) — denitrification niche.
//! - `Fe3+` (+0.77 V) — ferric-iron respiration.
//! - `SO4` (+0.5 V) — sulphate reduction.
//! - `SiO2` (+0.1 V) — silicate worlds only; very weak.
//! - `CO2` (-0.24 V) — methanogenesis. Net energy is *small* but
//!   non-zero with a suitable electron donor (H2).
//! - `N2H4` (-1.16 V) — hydrazine; reducing, only on cold ammoniacal
//!   worlds where it's accidentally available as a low-potential
//!   electron acceptor.
//!
//! Chemoautotroph producers consume oxidisers high-potential first
//! (greedy ladder); the `partition_chemoautotroph_growth` helper
//! drives that. Once a niche oxidiser depletes, the next species in
//! the queue gets the next-best one — letting two competing
//! chemolithotrophs partition the niche even when they share a
//! substrate.
//!
//! ## Substrate-relative selection
//!
//! `oxidiser_ladder(tag)` returns a substrate-specific ladder. The
//! ladders intentionally omit `O2` from cold-reducing substrates
//! (`Ammoniacal`, `Hydrocarbon`) where free oxygen would never
//! co-exist with the solvent. `Silicate` keeps `O2` but at reduced
//! density (the lattice traps it) and adds `SiO2` as a niche
//! oxidiser unique to that substrate.
//!
//! ## Determinism
//!
//! All quantities are `Real` (Q32.32). Ladder is a sorted `Vec` —
//! deterministic by construction; the partition helper iterates the
//! producers in `BTreeMap` order; depletion happens in-place on the
//! caller-owned ladder copy.

use crate::chemistry::substrate::MetabolicSubstrate;
use sim_arith::Real;

/// One alternative electron acceptor on a substrate's redox ladder.
///
/// `name` — short canonical symbol (`"O2"`, `"NO3-"`, etc.). Used
/// only for diagnostic display + test introspection; not key'd off
/// anywhere in the kernel.
///
/// `reduction_potential` — standard reduction potential E° in volts
/// vs. the standard hydrogen electrode. Higher = stronger oxidiser
/// = preferred by chemoautotroph metabolism.
///
/// `available_density` — relative per-cell baseline abundance,
/// normalised to "atmospheric O2 on an Earth-like Aqueous planet =
/// 1.0". This is the pool a chemolithotroph draws against; the
/// `partition_chemoautotroph_growth` helper deducts from it as
/// biomass grows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Oxidiser {
    pub name: &'static str,
    pub reduction_potential: Real,
    pub available_density: Real,
}

impl Oxidiser {
    /// Construct from rationals so the call sites read as the
    /// volts + density they actually are. Both arguments are
    /// `(numerator, denominator)` pairs.
    pub fn new(name: &'static str, e_volts: (i64, i64), density: (i64, i64)) -> Self {
        Self {
            name,
            reduction_potential: Real::from(e_volts),
            available_density: Real::from(density),
        }
    }
}

/// Substrate-relative oxidiser ladder. Returned sorted from
/// strongest oxidiser (highest E°) to weakest, so the greedy
/// partition can walk it head-first without re-sorting per tick.
///
/// `substrate_tag` matches the existing `Chemistry::for_planet`
/// convention: `"aqueous" | "ammoniacal" | "hydrocarbon" | "silicate"`.
/// Any other tag falls through to the Aqueous default.
///
/// For callers that already carry a typed `MetabolicSubstrate`, use
/// `oxidiser_ladder_for_substrate` to avoid the string round-trip.
///
/// Values mirror plan v2 Item 9:
/// - Aqueous: O2 / NO3- / Fe3+ / SO4 (oxidising solvent, broad
///   choice).
/// - Ammoniacal: NO3- / N2H4 (mostly weak; no free O2 in NH3
///   solvent).
/// - Hydrocarbon: SO4 / CO2 (very weak; methanogenic niche).
/// - Silicate: O2 / Fe3+ / `SiO2` (high-T-stable acceptors).
#[must_use]
pub fn oxidiser_ladder(substrate_tag: &str) -> Vec<Oxidiser> {
    let mut ladder: Vec<Oxidiser> = match substrate_tag {
        "ammoniacal" => vec![
            // NO3- can be sourced from photolytic NH3 chemistry; weak
            // density. N2H4 (hydrazine) is exotic but plausible in
            // cold NH3 atmospheres.
            Oxidiser::new("NO3-", (96, 100), (3, 10)),
            Oxidiser::new("N2H4", (-116, 100), (1, 10)),
        ],
        "hydrocarbon" => vec![
            // No free O2 on a CH4 world. Sulphate (from volcanic
            // outgassing) is the strongest realistic acceptor; CO2 is
            // weak but abundant — the methanogen niche.
            Oxidiser::new("SO4", (50, 100), (2, 10)),
            Oxidiser::new("CO2", (-24, 100), (4, 10)),
        ],
        "silicate" => vec![
            // High-T crystalline life. O2 is trapped in the lattice
            // at reduced density (0.3 vs. Earth's 1.0). Fe3+ is
            // abundant in silicate melts. SiO2 itself is a very weak
            // but extremely abundant acceptor.
            Oxidiser::new("O2", (123, 100), (3, 10)),
            Oxidiser::new("Fe3+", (77, 100), (5, 10)),
            Oxidiser::new("SiO2", (10, 100), (1, 1)),
        ],
        // Aqueous default — Earth-like terrestrial water world.
        _ => vec![
            Oxidiser::new("O2", (123, 100), (1, 1)),
            Oxidiser::new("NO3-", (96, 100), (3, 10)),
            Oxidiser::new("Fe3+", (77, 100), (1, 10)),
            Oxidiser::new("SO4", (50, 100), (2, 10)),
        ],
    };
    // Defence-in-depth: sort by reduction_potential descending so a
    // future ladder entry inserted out-of-order still gets the
    // strongest-first guarantee the partition helper relies on. The
    // sort is by raw Q32.32 bit pattern via `PartialOrd::partial_cmp`
    // followed by `Ord::cmp` — both deterministic.
    ladder.sort_by(|a, b| b.reduction_potential.cmp(&a.reduction_potential));
    ladder
}

/// Enum-typed form of `oxidiser_ladder`. Forwards to the string-tag
/// implementation via `MetabolicSubstrate::tag()` so the ladder
/// data lives in exactly one place and callers can pick whichever
/// form is cleaner at the call site.
#[must_use]
pub fn oxidiser_ladder_for_substrate(substrate: &MetabolicSubstrate) -> Vec<Oxidiser> {
    oxidiser_ladder(substrate.tag())
}

/// Per-species chemoautotroph growth assignment after walking the
/// ladder. Tuple is `(species_index, oxidiser_name, growth_units)`.
///
/// `species_index` corresponds to the producer's position in the
/// input slice — the caller maps it back to a `SpeciesId`. Stored as
/// an index rather than as a `SpeciesId` so the physics crate stays
/// free of `sim-species` (one-way dependency: ecosystem → physics).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChemoautotrophShare {
    pub species_index: usize,
    pub oxidiser_name: &'static str,
    pub growth_units: Real,
}

/// Partition a per-tick growth budget across the available oxidiser
/// ladder, greedy by reduction potential.
///
/// Inputs:
/// - `ladder` — caller-owned mutable copy of `oxidiser_ladder()`.
///   The helper deducts from `available_density` as it assigns
///   growth, so subsequent calls (or subsequent ticks) see the
///   residual.
/// - `growth_demand_per_species` — per-species growth desire (the
///   amount the species *wants* to add to its biomass this tick).
///   Order matches the slice index; that order also defines
///   priority on the ladder — the first species takes the strongest
///   oxidiser first.
///
/// Returns one `ChemoautotrophShare` per species in input order. A
/// species whose oxidisers all deplete returns `growth_units =
/// Real::ZERO` and `oxidiser_name = ""` — the caller treats that as
/// "this niche was full this tick" and applies any extinction-
/// pressure penalty downstream.
///
/// **Pyramid invariant**: the helper never returns more
/// `growth_units` than the species demanded — depletion only ever
/// caps the growth, never amplifies it.
#[must_use]
pub fn partition_chemoautotroph_growth(
    ladder: &mut [Oxidiser],
    growth_demand_per_species: &[Real],
) -> Vec<ChemoautotrophShare> {
    let mut out: Vec<ChemoautotrophShare> = Vec::with_capacity(growth_demand_per_species.len());
    for (species_index, demand) in growth_demand_per_species.iter().enumerate() {
        let mut remaining = *demand;
        let mut assigned: Real = Real::ZERO;
        let mut chosen_name: &'static str = "";
        if remaining <= Real::ZERO {
            out.push(ChemoautotrophShare {
                species_index,
                oxidiser_name: chosen_name,
                growth_units: Real::ZERO,
            });
            continue;
        }
        // Walk the ladder strongest-first. Each oxidiser supplies as
        // much growth as `available_density` allows, then the next
        // species (or the same species, if it still has demand)
        // falls through to the next oxidiser. The first oxidiser
        // that actually contributes anything becomes the species'
        // `oxidiser_name` for the report — we want the dominant
        // acceptor, not the last-resort weak one, so we record the
        // name on the first non-zero deduction.
        for ox in ladder.iter_mut() {
            if remaining <= Real::ZERO {
                break;
            }
            if ox.available_density <= Real::ZERO {
                continue;
            }
            // Energy yield scales with reduction potential.
            // Negative-potential oxidisers (CO2, N2H4) yield very
            // little but non-zero energy; the per-unit-growth cost
            // is `1 / (E° + 1.5)` (offset by 1.5 V so the
            // hydrazine-end of the ladder remains positive). A
            // strong O2 acceptor at 1.23 V needs ~0.37 oxidiser
            // per unit growth; weak CO2 at -0.24 V needs ~0.79
            // per unit. So `available_density / cost_per_growth`
            // is the max growth this oxidiser can supply.
            let cost_offset = Real::from((150, 100));
            let cost_per_unit = Real::ONE / (ox.reduction_potential + cost_offset);
            let max_supply = ox.available_density / cost_per_unit;
            let supply = remaining.min(max_supply);
            if supply > Real::ZERO {
                if chosen_name.is_empty() {
                    chosen_name = ox.name;
                }
                ox.available_density = ox.available_density - supply * cost_per_unit;
                if ox.available_density < Real::ZERO {
                    ox.available_density = Real::ZERO;
                }
                assigned = assigned + supply;
                remaining = remaining - supply;
            }
        }
        out.push(ChemoautotrophShare {
            species_index,
            oxidiser_name: chosen_name,
            growth_units: assigned,
        });
    }
    out
}

/// Per-unit energy yield for a given oxidiser as a fraction of the
/// strongest acceptor on Earth (O2 at +1.23 V). Maps reduction
/// potential to a normalised `[0, 1]` energy factor via a linear
/// rescale of the +1.23 V to -1.5 V band. Used by the alt-oxidiser
/// combustion energy-budget helper.
///
/// `E° = +1.23` (O2) → 1.0 (Earth-like maximum).
/// `E° = +0.5` (SO4) → ~0.73.
/// `E° = -0.24` (CO2) → ~0.46 (methanogenic niche still positive
/// energy net, just much smaller).
/// `E° = -1.5` (sub-hydrazine) → 0.0 (no useful energy).
#[must_use]
pub fn energy_yield_factor(reduction_potential: Real) -> Real {
    // Window: -1.5 V (zero) to +1.23 V (unity). 2.73 V span.
    let window = Real::from((273, 100));
    let zero_point = Real::from((-150, 100));
    let raw = (reduction_potential - zero_point) / window;
    raw.clamp01()
}

/// Net per-unit-fuel energy yield for combustion against an
/// arbitrary oxidiser at the given `base_combustion_energy`. The
/// base value is the kinetic kernel's per-unit enthalpy
/// (`Chemistry::for_planet`'s `lh_combustion`); the alt-oxidiser
/// helper scales it by `energy_yield_factor(E°)` so a CO2-atmosphere
/// fuel cell still produces net positive energy, just less of it.
///
/// Returns `Real::ZERO` when the oxidiser is too weak to drive the
/// reaction (factor clamps to zero at E° ≤ -1.5 V).
#[must_use]
pub fn alt_oxidiser_combustion_energy(
    base_combustion_energy: Real,
    oxidiser: &Oxidiser,
    fuel_units: Real,
) -> Real {
    if fuel_units <= Real::ZERO {
        return Real::ZERO;
    }
    let factor = energy_yield_factor(oxidiser.reduction_potential);
    base_combustion_energy * factor * fuel_units
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aqueous_ladder_is_sorted_strongest_first() {
        let l = oxidiser_ladder("aqueous");
        assert!(!l.is_empty());
        for w in l.windows(2) {
            assert!(
                w[0].reduction_potential >= w[1].reduction_potential,
                "ladder not sorted: {} ({:?}) before {} ({:?})",
                w[0].name,
                w[0].reduction_potential,
                w[1].name,
                w[1].reduction_potential,
            );
        }
        assert_eq!(l[0].name, "O2");
    }

    #[test]
    fn hydrocarbon_ladder_has_co2() {
        let l = oxidiser_ladder("hydrocarbon");
        assert!(l.iter().any(|o| o.name == "CO2"));
        // No O2 on a methane world.
        assert!(!l.iter().any(|o| o.name == "O2"));
    }

    #[test]
    fn partition_assigns_strongest_oxidiser_to_first_species() {
        let mut ladder = oxidiser_ladder("aqueous");
        let demand = vec![Real::from_int(1), Real::from_int(1)];
        let shares = partition_chemoautotroph_growth(&mut ladder, &demand);
        assert_eq!(shares.len(), 2);
        // First species takes the strongest (O2).
        assert_eq!(shares[0].oxidiser_name, "O2");
        // Both species get the full demand (O2 is plentiful at
        // density 1.0).
        assert_eq!(shares[0].growth_units, Real::from_int(1));
        assert_eq!(shares[1].growth_units, Real::from_int(1));
    }

    #[test]
    fn energy_yield_clamps_at_zero_for_very_weak_oxidiser() {
        let weak = Real::from_int(-2);
        let y = energy_yield_factor(weak);
        assert_eq!(y, Real::ZERO);
    }

    #[test]
    fn energy_yield_at_o2_potential_is_unity() {
        let o2 = Real::from((123, 100));
        let y = energy_yield_factor(o2);
        // Within ε of 1.0 (the window rescale isn't *exactly*
        // unity-mapping due to Q32.32 integer-ratio rounding; check
        // it's >= 0.99).
        assert!(y >= Real::from((99, 100)), "O2 yield {y:?} below 0.99");
    }
}
