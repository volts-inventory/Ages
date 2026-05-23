//! Planet-module free helpers split out of `planet.rs` in CB4.
//!
//! Substance aggregation, planet-wide CO2 delta application, and the
//! `(tick, affector, affected)` SplitMix64 hash used by the virus-
//! outbreak branch. Also houses the `MutualismKind` / `ParasiteKind`
//! lookups consumed by the interaction step.

use sim_arith::Real;
use sim_physics::{PhysicsState, Substance};
use sim_species::{EcosystemRole, MutualismKind, ParasiteKind, SpeciesId};
use std::collections::BTreeMap;

/// Aggregate per-substance density across every cell of the planet.
pub(super) fn sum_substance(state: &PhysicsState, substance: Substance) -> Real {
    state
        .substance(substance.idx())
        .iter()
        .copied()
        .fold(Real::ZERO, |a, b| a + b)
}

/// Apply a planet-wide CO2 delta — positive = add to atmosphere,
/// negative = remove from atmosphere. Distributes the change
/// uniformly across cells (per-cell delta = total / n_cells). When
/// the requested removal exceeds the per-cell stock, the per-cell
/// value clamps at zero — the *available* CO2 was already gated by
/// the producer-growth path so this clamp protects against rounding
/// drift only.
pub(super) fn apply_co2_delta(state: &mut PhysicsState, delta: Real) {
    if delta == Real::ZERO {
        return;
    }
    let co2 = state.substance_mut(Substance::CO2.idx());
    let n = co2.len();
    if n == 0 {
        return;
    }
    let per_cell = delta / Real::from_int(n as i64);
    for c in co2.iter_mut() {
        let next = *c + per_cell;
        *c = if next < Real::ZERO { Real::ZERO } else { next };
    }
}

/// P3.1 helper — look up the `MutualismKind` of whichever side of the
/// `(a, b)` pair carries the `Mutualist { kind }` role payload. Returns
/// `None` if neither side does (back-compat fixtures with hand-built
/// matrices that use `InteractionKind::Mutualism` on non-mutualist
/// pairs, or fixtures that don't tag the role at all). When both sides
/// happen to be mutualists (uncommon but valid — two mutualist species
/// cooperating), returns the affector's kind so the per-direction
/// dispatch stays deterministic.
pub(super) fn lookup_mutualism_kind(
    roles: &BTreeMap<SpeciesId, EcosystemRole>,
    affector: SpeciesId,
    affected: SpeciesId,
) -> Option<MutualismKind> {
    if let Some(EcosystemRole::Mutualist { kind }) = roles.get(&affector) {
        return Some(*kind);
    }
    if let Some(EcosystemRole::Mutualist { kind }) = roles.get(&affected) {
        return Some(*kind);
    }
    None
}

/// P3.1 helper — look up the `ParasiteKind` of whichever side of the
/// `(a, b)` pair carries the `Parasite { kind }` role payload. Returns
/// `None` if neither side does (back-compat fixtures with hand-built
/// matrices that use `InteractionKind::Parasitism` on non-parasite
/// pairs). Affector takes precedence — the typical wiring has the
/// parasite as the affector preying on its host.
pub(super) fn lookup_parasite_kind(
    roles: &BTreeMap<SpeciesId, EcosystemRole>,
    affector: SpeciesId,
    affected: SpeciesId,
) -> Option<ParasiteKind> {
    if let Some(EcosystemRole::Parasite { kind }) = roles.get(&affector) {
        return Some(*kind);
    }
    if let Some(EcosystemRole::Parasite { kind }) = roles.get(&affected) {
        return Some(*kind);
    }
    None
}

/// P3.1 helper — SplitMix64-style hash of `(tick, affector_id,
/// affected_id)`. Used by the virus-parasite branch to derive a
/// deterministic tie-break order when multiple virus parasites fire
/// on the same outbreak tick. The cadence (period gate) is the firing
/// condition; this hash exists so future extensions (e.g. random
/// host-shopping among multiple candidates) have a deterministic
/// stream available without revisiting the call site.
#[must_use]
pub fn virus_outbreak_hash(tick: u64, affector: u32, affected: u32) -> u64 {
    let mut z = tick
        .wrapping_add((affector as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
        .wrapping_add((affected as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}
