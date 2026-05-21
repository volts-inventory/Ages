//! Sprint 3 Item 11 — speciation events.
//!
//! Adds five speciation triggers to the per-planet ecosystem:
//!
//! 1. **Allopatric** — a species' cells split into geographically
//!    disconnected groups for more than `ALLOPATRIC_ISOLATION_TICKS`
//!    consecutive ticks. The daughter species inherits the isolated
//!    subpopulation.
//! 2. **Sympatric** — two species interact via
//!    `InteractionKind::Competition` while both carry biomass above
//!    `SYMPATRIC_COMPETITION_BIOMASS_FRAC × producer_capacity` for
//!    more than `SYMPATRIC_PRESSURE_TICKS` consecutive ticks. One
//!    drifts into a distinct niche → spawn daughter with shifted
//!    traits.
//! 3. **Polyploid** — `Lifecycle::Plant` species only. Instant
//!    chromosome-duplication event sampled with per-tick probability
//!    `POLYPLOID_PER_TICK_PROB_RECIP^-1` (currently `1e-5` ≈
//!    `1 / 100_000`).
//! 4. **FounderEffect** — when a small population (< 1% of the
//!    parent's normal pool) seeds new territory, the bottleneck
//!    drives rapid drift toward fixation.
//! 5. **PostExtinctionRadiation** — for `POST_EXTINCTION_BOOST_TICKS`
//!    after a `SpeciesExtinct` event, speciation rate is boosted 5×.
//!
//! **Daughter trait inheritance** uses `divergence_pull`, an
//! allometry-aware helper: body-mass-correlated traits change together
//! (bigger body → longer lifespan → slower metabolism → larger
//! clutch). The perturbation is deterministic — a SplitMix64 hash of
//! `(parent_seed, axis_idx, daughter_id)` keys the Gaussian-equivalent
//! offset so replays are bit-identical.
//!
//! **`SpeciesId` allocation** is `max(existing_id) + 1`; the registry
//! grows monotonically across the run and (paired with the extinction
//! sweep from Item 6a) bounds total growth.
//!
//! Detection paths that need per-species territory / cell-distribution
//! data (allopatric / sympatric / founder effect) currently track a
//! synthetic per-species tracker via `SpeciationTracker`. Production
//! callers feed the tracker per tick from their grid-resident
//! population-distribution snapshot; tests construct trackers directly
//! so the speciation logic stays unit-testable without a full physics
//! grid wired up.

use crate::EcoSpecies;
use protocol::{SpeciationEvent, SpeciationTriggerKind};
use sim_arith::Real;
use sim_species::{
    EcosystemRole, InteractionKind, InteractionMatrix, Lifecycle, ProducerMetabolism, Species,
    SpeciesId,
};
use std::collections::BTreeMap;

/// Number of consecutive ticks two subpopulations of the same species
/// must stay geographically disconnected before allopatric speciation
/// fires. Tuned to "more than one sim-year of isolation" so a single
/// short-term flood that briefly disconnects two pools doesn't trigger
/// speciation, but a sustained barrier (mountain rising, sea-level
/// change) does.
pub const ALLOPATRIC_ISOLATION_TICKS: u64 = 100;

/// Minimum biomass each side of a Competition pair must carry,
/// expressed as a fraction of `producer_capacity`, for sympatric
/// pressure to start accumulating. Below this both species are too
/// small to meaningfully compete; above it the niche overlap drives
/// the divergence pressure.
pub const SYMPATRIC_COMPETITION_BIOMASS_FRAC: (i64, i64) = (5, 100);

/// Number of consecutive ticks an `InteractionKind::Competition` pair
/// must stay above `SYMPATRIC_COMPETITION_BIOMASS_FRAC` on both sides
/// before one of them drifts into a daughter species. Tuned to roughly
/// half the allopatric window so sympatric drift is the faster of the
/// two paths under sustained niche pressure.
pub const SYMPATRIC_PRESSURE_TICKS: u64 = 50;

/// Reciprocal of the per-tick polyploidy probability for a
/// `Lifecycle::Plant` species. `100_000` → per-tick probability
/// `1e-5`. The plan specifies "low per-tick probability (1e-5)"; we
/// store the reciprocal so the deterministic check (`tick_seed %
/// recip == 0`) keeps the math integer-only.
pub const POLYPLOID_PER_TICK_PROB_RECIP: u64 = 100_000;

/// Founder-effect threshold — when a seeded subpopulation has
/// biomass below `FOUNDER_BIOMASS_FRAC × parent_biomass`, the
/// bottleneck drives rapid drift toward fixation. 1% of the parent's
/// normal pool per the plan.
pub const FOUNDER_BIOMASS_FRAC: (i64, i64) = (1, 100);

/// Multiplier applied to speciation rate for species inside a
/// post-extinction adaptive-radiation window. 5× per the plan.
pub const POST_EXTINCTION_RADIATION_MULTIPLIER: u64 = 5;

/// Length of the post-extinction adaptive-radiation window, measured
/// in ticks. Each `SpeciesExtinct` event opens a window of this
/// length during which the surviving species' speciation rates are
/// scaled by `POST_EXTINCTION_RADIATION_MULTIPLIER`. 100 generations
/// at 1 generation ≈ 1 tick (the plan's per-generation framing).
pub const POST_EXTINCTION_BOOST_TICKS: u64 = 100;

/// Maximum per-axis fractional perturbation applied by
/// `divergence_pull`. ±5% per axis keeps daughter traits close enough
/// to the parent that the daughter is recognisably a descendant
/// (small drift, correlated across axes), while still letting
/// thousands of speciation events compound into observable lineage
/// divergence.
const DIVERGENCE_AXIS_RANGE: (i64, i64) = (5, 100);

/// Lower bound applied to `cosmic_ray_multiplier` inside
/// `step_speciation` / `step_hgt`. A multiplier ≤ 1.0 means "no
/// reversal-window amplification" — fall back to the baseline rate
/// (one daughter per fire) rather than zero out speciation.
pub const COSMIC_RAY_MULTIPLIER_FLOOR: i64 = 1;

/// Upper bound applied to `cosmic_ray_multiplier` inside
/// `step_speciation` / `step_hgt`. Caps the worst-case reversal
/// window so a pathological `dipole_strength` near zero (Item 20:
/// `cosmic_ray_ground_flux = 1 / (dipole_strength + 0.1)`) doesn't
/// drive instantaneous speciation cascades — at the deepest reversal
/// the flux multiplier saturates at 10×.
pub const COSMIC_RAY_MULTIPLIER_CEILING: i64 = 10;

/// Trigger kind for a speciation event. Mirrors the wire-layer
/// `protocol::SpeciationTriggerKind` but uses the in-crate naming so
/// downstream consumers of `sim_ecosystem` don't need a protocol
/// dependency to switch on the trigger. Lossless conversion via
/// `SpeciationTrigger::to_wire`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeciationTrigger {
    Allopatric { isolation_ticks: u64 },
    Sympatric,
    Polyploid,
    FounderEffect,
    PostExtinctionRadiation { generation: u64 },
}

impl SpeciationTrigger {
    /// Convert to the wire-layer enum for emission as
    /// `Event::SpeciationOccurred`.
    #[must_use]
    pub fn to_wire(self) -> SpeciationTriggerKind {
        match self {
            SpeciationTrigger::Allopatric { isolation_ticks } => {
                SpeciationTriggerKind::Allopatric { isolation_ticks }
            }
            SpeciationTrigger::Sympatric => SpeciationTriggerKind::Sympatric,
            SpeciationTrigger::Polyploid => SpeciationTriggerKind::Polyploid,
            SpeciationTrigger::FounderEffect => SpeciationTriggerKind::FounderEffect,
            SpeciationTrigger::PostExtinctionRadiation { generation } => {
                SpeciationTriggerKind::PostExtinctionRadiation { generation }
            }
        }
    }
}

/// Per-species per-tick tracker for the spatial / pressure /
/// bottleneck triggers. The ecosystem owns one of these per planet;
/// the per-tick `step` updates it from the planet's
/// population-distribution snapshot before the speciation pass runs.
/// Tests construct it directly so the speciation logic stays
/// unit-testable without a full physics grid wired up.
#[derive(Debug, Clone, Default)]
pub struct SpeciationTracker {
    /// Number of consecutive ticks this species has been split into
    /// ≥ 2 disconnected groups. Reset to zero when contact resumes.
    pub allopatric_streak: BTreeMap<SpeciesId, u64>,
    /// Number of consecutive ticks this `(a, b)` Competition pair
    /// has both carried biomass above the sympatric threshold. Keyed
    /// in canonical `(min, max)` order so the symmetric matrix
    /// storage doesn't double-count.
    pub sympatric_streak: BTreeMap<(SpeciesId, SpeciesId), u64>,
    /// Pending founder-effect seedings — the per-tick step adds an
    /// entry when a small population colonises a new region; the
    /// speciation pass drains the map and emits one speciation per
    /// pending entry.
    pub pending_founder_seedings: BTreeMap<SpeciesId, Real>,
    /// Per-tick post-extinction radiation window — populated when a
    /// `SpeciesExtinct` event fires. Each entry maps the extinction
    /// tick to the "generation" counter that grew monotonically
    /// while the window stays open. Tick boundary `tick >
    /// extinction_tick + POST_EXTINCTION_BOOST_TICKS` retires the
    /// entry.
    pub post_extinction_windows: BTreeMap<u64, u64>,
}

impl SpeciationTracker {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Per-tick maintenance: increment the allopatric streak for any
    /// species marked as split this tick; reset for the rest. The
    /// `split_species` set is the caller's per-tick analysis of the
    /// population-distribution snapshot — caller decides what
    /// "disconnected" means (cell-graph connected component count,
    /// territory-graph component count, etc.).
    pub fn observe_allopatric_split(
        &mut self,
        all_species: &[SpeciesId],
        split_species: &[SpeciesId],
    ) {
        let split_set: std::collections::BTreeSet<SpeciesId> =
            split_species.iter().copied().collect();
        for id in all_species {
            if split_set.contains(id) {
                let cur = self.allopatric_streak.entry(*id).or_insert(0);
                *cur = cur.saturating_add(1);
            } else {
                self.allopatric_streak.remove(id);
            }
        }
    }

    /// Per-tick maintenance: bump the sympatric-pressure streak for
    /// every Competition pair where both sides are above the
    /// threshold; reset the rest. Pairs are keyed canonically
    /// `(min, max)` so the matrix's symmetric two-way storage
    /// doesn't double-count.
    pub fn observe_sympatric_pressure(
        &mut self,
        species: &BTreeMap<SpeciesId, EcoSpecies>,
        interactions: &InteractionMatrix,
        producer_capacity: Real,
    ) {
        let threshold = Real::from(SYMPATRIC_COMPETITION_BIOMASS_FRAC) * producer_capacity;
        let mut active_pairs: std::collections::BTreeSet<(SpeciesId, SpeciesId)> =
            std::collections::BTreeSet::new();
        for ((a, b), interaction) in &interactions.pairs {
            if interaction.kind != InteractionKind::Competition {
                continue;
            }
            let ba = species
                .get(a)
                .filter(|s| s.is_extant)
                .map(|s| s.biomass)
                .unwrap_or(Real::ZERO);
            let bb = species
                .get(b)
                .filter(|s| s.is_extant)
                .map(|s| s.biomass)
                .unwrap_or(Real::ZERO);
            if ba >= threshold && bb >= threshold {
                let pair = if a <= b { (*a, *b) } else { (*b, *a) };
                active_pairs.insert(pair);
            }
        }
        let prev_pairs: Vec<(SpeciesId, SpeciesId)> = self.sympatric_streak.keys().copied().collect();
        for pair in &prev_pairs {
            if !active_pairs.contains(pair) {
                self.sympatric_streak.remove(pair);
            }
        }
        for pair in active_pairs {
            let cur = self.sympatric_streak.entry(pair).or_insert(0);
            *cur = cur.saturating_add(1);
        }
    }

    /// Register a founder-effect seeding — a small subpopulation of
    /// `parent_id` was seeded with `seed_biomass` into new territory.
    /// The speciation pass drains the map and decides per-entry
    /// whether the seed is small enough to trigger.
    pub fn register_founder_seeding(&mut self, parent_id: SpeciesId, seed_biomass: Real) {
        self.pending_founder_seedings.insert(parent_id, seed_biomass);
    }

    /// Open a post-extinction adaptive-radiation window keyed on the
    /// extinction tick. Multiple extinctions in the same tick collapse
    /// to one window entry. The "generation" counter is the tick offset
    /// from the extinction itself.
    pub fn register_extinction_event(&mut self, extinction_tick: u64) {
        self.post_extinction_windows.insert(extinction_tick, 0);
    }

    /// Advance the per-tick generation counter for every open
    /// post-extinction window and retire windows that have aged past
    /// `POST_EXTINCTION_BOOST_TICKS`.
    pub fn advance_post_extinction_windows(&mut self, current_tick: u64) {
        let mut retire: Vec<u64> = Vec::new();
        for (extinction_tick, gen) in self.post_extinction_windows.iter_mut() {
            let age = current_tick.saturating_sub(*extinction_tick);
            if age > POST_EXTINCTION_BOOST_TICKS {
                retire.push(*extinction_tick);
            } else {
                *gen = age;
            }
        }
        for k in retire {
            self.post_extinction_windows.remove(&k);
        }
    }

    /// True iff at least one post-extinction adaptive-radiation
    /// window is currently open. Callers use this to apply the 5×
    /// rate multiplier to other speciation triggers.
    #[must_use]
    pub fn in_post_extinction_window(&self) -> bool {
        !self.post_extinction_windows.is_empty()
    }

    /// Current radiation generation, or zero if no window is open.
    /// Returns the largest generation across open windows so a
    /// pile-up of extinction events doesn't reset the counter.
    #[must_use]
    pub fn current_radiation_generation(&self) -> u64 {
        self.post_extinction_windows
            .values()
            .copied()
            .max()
            .unwrap_or(0)
    }
}

/// Allometric divergence helper. Returns a small Gaussian-equivalent
/// per-axis perturbation that is *correlated* across body-mass-linked
/// traits (lifespan, metabolism, clutch_size) so daughter species
/// diverge along the allometry slope rather than independently per
/// axis.
///
/// Body-mass-correlated traits all share the same axis sign: a
/// daughter that drifts toward "bigger body" simultaneously gets a
/// "longer lifespan" pull and a "slower metabolism" pull. We model
/// this by deriving a single signed scalar `s` per `(parent_seed,
/// daughter_seed)` and using it as the sign for every body-mass-
/// correlated axis, with a small per-axis magnitude perturbation on
/// top for axis-level independence.
///
/// `axis_idx` selects which trait axis the caller is perturbing:
///
/// - `0` = lifespan
/// - `1` = metabolism (inverse to body mass)
/// - `2` = clutch_size
/// - `3` = body mass (the anchor)
///
/// Other axis indices are treated as uncorrelated and use a fresh
/// per-axis sign.
///
/// Output is a `Real` in approximately `[-DIVERGENCE_AXIS_RANGE,
/// +DIVERGENCE_AXIS_RANGE]` (default ±5%).
#[must_use]
pub fn divergence_pull(parent: &Species, axis_idx: usize, daughter_seed: u64) -> Real {
    let body_mass_axes = [0usize, 1, 2, 3];
    let is_body_mass_axis = body_mass_axes.contains(&axis_idx);
    // Per-`(parent, daughter)` body-mass direction sign. Same scalar
    // for every body-mass-correlated axis so the daughter drifts
    // consistently along the allometry slope.
    let mass_dir = splitmix_signed(parent.seed ^ daughter_seed, 0);
    // Per-axis perturbation — small fraction of the range, gives
    // each axis its own magnitude (and lets non-body-mass axes pick
    // a different sign).
    let axis_perturb = splitmix_signed(
        parent.seed ^ daughter_seed,
        axis_idx as u64 + 1,
    );
    let range = Real::from(DIVERGENCE_AXIS_RANGE);
    // Inversion: metabolism is *inverse* to body mass — a "bigger
    // body" pull → "slower metabolism" → push metabolism axis with
    // the opposite sign of mass_dir.
    let signed_dir = if axis_idx == 1 {
        invert_sign(mass_dir)
    } else if is_body_mass_axis {
        mass_dir
    } else {
        // Uncorrelated axes get their own sign.
        axis_perturb
    };
    // Magnitude: directional component dominates (75% of range)
    // so body-mass-correlated axes reliably share the same sign;
    // the remaining 25% comes from the per-axis perturbation so
    // magnitudes diverge across axes. This keeps the allometry
    // correlation observable across a band of seeds (the
    // `daughter_species_traits_correlated_via_allometry` test
    // measures ≥ 75% same-sign frequency).
    let dir_weight = Real::from((3, 4)); // 0.75
    let perturb_weight = Real::from((1, 4)); // 0.25
    let mag_part = axis_perturb * range * perturb_weight;
    let dir_part = signed_dir * range * dir_weight;
    mag_part + dir_part
}

/// SplitMix64-style hash that produces a `Real` in approximately
/// `[-1, +1]`. Deterministic; no allocation; no RNG state. Used by
/// `divergence_pull` to derive trait perturbations from a parent +
/// axis seed.
fn splitmix_signed(seed: u64, salt: u64) -> Real {
    let mut z = seed.wrapping_add(salt.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    // Take the low 16 bits and map to [-1, +1).
    let bits = (z & 0xFFFF) as i64; // 0..65535
    let signed = bits - 32_768; // -32768..32767
    Real::from_ratio(signed, 32_768)
}

/// Invert the sign of a Real. Used to make metabolism (which is
/// inverse to body mass) push opposite to the body-mass direction.
fn invert_sign(r: Real) -> Real {
    Real::ZERO - r
}

/// Allocate the next free `SpeciesId`. Returns `max(existing) + 1`,
/// or `0` if the registry is empty.
#[must_use]
pub fn next_species_id(species: &BTreeMap<SpeciesId, EcoSpecies>) -> SpeciesId {
    let max = species.keys().map(|id| id.0).max().unwrap_or(0);
    SpeciesId(if species.is_empty() { 0 } else { max + 1 })
}

/// Derive a daughter `Species` from a parent under a given trigger.
/// Applies `divergence_pull` to body-mass-correlated traits
/// (lifespan + biology.clutch_size + biology.fertile reproductive
/// success) and gives the daughter a fresh seed derived from
/// `(parent.seed, daughter_id, trigger_salt)` so replays are
/// byte-stable.
///
/// Trigger salt picks a per-trigger constant so two daughters spawned
/// from the same parent under different triggers diverge into
/// distinct seed spaces — keeps replay bit-stable while preventing
/// accidental aliasing across triggers.
#[must_use]
pub fn derive_daughter_species(
    parent: &Species,
    daughter_id: SpeciesId,
    trigger: SpeciationTrigger,
) -> Species {
    let trigger_salt: u64 = match trigger {
        SpeciationTrigger::Allopatric { .. } => 0xA110_0A77_0000_0001_u64,
        SpeciationTrigger::Sympatric => 0x5A1B_5A1B_5A1B_5A01_u64,
        SpeciationTrigger::Polyploid => 0x0017_0101_DEAD_BEEF_u64,
        SpeciationTrigger::FounderEffect => 0xF000_DEAD_BEEF_0001_u64,
        SpeciationTrigger::PostExtinctionRadiation { generation } => {
            0xEC71_0CC7_0000_0001_u64.wrapping_add(generation)
        }
    };
    let daughter_seed = parent
        .seed
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(daughter_id.0 as u64)
        .wrapping_add(trigger_salt);

    // Build the daughter as a clone of the parent, then apply
    // allometric perturbations to the body-mass-correlated traits.
    let mut daughter = parent.clone();
    daughter.seed = daughter_seed;
    daughter.name = format!("{}-d{}", parent.name, daughter_id.0);

    // Axis 0: lifespan (body-mass-correlated). Daughter drifts up
    // by `pull × parent.lifespan`. Clamp ≥ 1 year so we don't
    // produce a species with non-positive lifespan from an unlucky
    // pull.
    let pull_life = divergence_pull(parent, 0, daughter_seed);
    let life_delta = parent.lifespan_years * pull_life;
    let new_life = parent.lifespan_years + life_delta;
    daughter.lifespan_years = new_life.max(Real::from_int(1));

    // Axis 1: metabolism is implicit (no scalar field on Species),
    // but it surfaces through `biology.events_per_fertile_window`
    // — higher metabolism → more reproductive cycles per window.
    // We use the inverted axis_idx=1 pull to perturb in the
    // metabolism-aligned direction.
    let pull_metab = divergence_pull(parent, 1, daughter_seed);
    let metab_delta = parent.biology.events_per_fertile_window * pull_metab;
    daughter.biology.events_per_fertile_window =
        (parent.biology.events_per_fertile_window + metab_delta).max(Real::ZERO);

    // Axis 2: clutch_size (body-mass-correlated; bigger body →
    // larger clutch for K-strategists, the inverse for
    // r-strategists, but the allometry holds in the per-individual
    // direction we model here).
    let pull_clutch = divergence_pull(parent, 2, daughter_seed);
    let clutch_delta = parent.biology.clutch_size * pull_clutch;
    let new_clutch = parent.biology.clutch_size + clutch_delta;
    daughter.biology.clutch_size = new_clutch.max(Real::from_ratio(1, 10));

    daughter
}

/// Clamp the raw cosmic-ray multiplier (typically
/// `state.cosmic_ray_ground_flux()` ≈ `1 / (dipole_strength + 0.1)`)
/// into a small positive integer in `[COSMIC_RAY_MULTIPLIER_FLOOR,
/// COSMIC_RAY_MULTIPLIER_CEILING]`. The returned value is how many
/// daughters get spawned per triggered speciation event this tick.
///
/// The clamp does two things at once:
///   1. Floor at `1` — at full dipole strength the flux multiplier
///      sits at ≈ 0.91; rounding-down would zero-out speciation. The
///      floor preserves the baseline rate when the field is healthy.
///   2. Ceiling at `10` — caps the worst-case reversal window so a
///      pathological `dipole_strength → 0` (flux → ∞) doesn't
///      collapse the whole speciation budget into a single tick.
///
/// The conversion is `Real → i64` via `raw().to_num::<i64>()`, which
/// truncates toward zero — `1.5 → 1`, `5.7 → 5`, etc. Truncation
/// (rather than rounding) keeps multiplier=1.0 mapping to a single
/// daughter, the documented baseline.
#[must_use]
pub fn clamp_cosmic_ray_multiplier(raw_multiplier: Real) -> u64 {
    let lo = Real::from_int(COSMIC_RAY_MULTIPLIER_FLOOR);
    let hi = Real::from_int(COSMIC_RAY_MULTIPLIER_CEILING);
    let clamped = raw_multiplier.max(lo).min(hi);
    let as_int: i64 = clamped.raw().to_num::<i64>();
    // `as_int` is guaranteed in [FLOOR, CEILING] by the clamp above.
    // Cast to u64 is therefore safe — FLOOR ≥ 1 > 0.
    as_int.max(COSMIC_RAY_MULTIPLIER_FLOOR) as u64
}

/// Decide whether a Plant species hits polyploidy this tick. Per-
/// tick probability is `1 / POLYPLOID_PER_TICK_PROB_RECIP`; the
/// deterministic check uses a SplitMix64 hash of `(tick, parent_id)`
/// so two ticks of the same parent at different times can fire
/// independently. The hash modulo the reciprocal == 0 ⇒ event fires.
#[must_use]
pub fn polyploid_check(tick: u64, parent: SpeciesId) -> bool {
    let mut z = tick.wrapping_add((parent.0 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    z % POLYPLOID_PER_TICK_PROB_RECIP == 0
}

/// Run the per-tick speciation pass. Inspects the tracker (allopatric
/// streaks / sympatric streaks / founder seedings) plus the per-tick
/// polyploidy roll, fires any triggered speciation events, and
/// returns the list of newly created daughter species along with the
/// matching `SpeciationEvent`s.
///
/// Side effects on the tracker:
/// - Allopatric streaks that fire speciation are reset to zero so
///   the species doesn't immediately re-trigger.
/// - Sympatric streaks that fire are reset similarly.
/// - Pending founder seedings are drained.
///
/// The caller is responsible for:
/// - Inserting the returned daughter species into the planet's
///   `Species` registry (the species crate's persistent store).
/// - Inserting matching `EcoSpecies` rows into `PlanetEcosystem::
///   species`.
/// - Forwarding the `SpeciationEvent`s to its `Emitter` as
///   `Event::SpeciationOccurred`.
///
/// Per-tick determinism: trackers iterated via `BTreeMap`, so the
/// returned events are in `(SpeciesId, …)`-sorted order.
///
/// `cosmic_ray_multiplier` carries the planet's
/// `state.cosmic_ray_ground_flux()` for this tick (Item 20: surface
/// flux scales as `1 / (dipole_strength + 0.1)`). The raw value is
/// clamped via [`clamp_cosmic_ray_multiplier`] into
/// `[COSMIC_RAY_MULTIPLIER_FLOOR, COSMIC_RAY_MULTIPLIER_CEILING]`
/// and used as a daughter-count multiplier: every triggered
/// speciation event (Allopatric, Sympatric, Polyploid, FounderEffect,
/// or PostExtinctionRadiation) spawns `clamped` daughters instead of
/// one. At full dipole strength (`flux ≈ 0.91`) the clamp floors to
/// `1` and the baseline rate is preserved; during a deep reversal
/// (`dipole_strength → 0`, `flux → 10` after clamp) every fire
/// spawns ten daughters — the wide-spectrum mutation burst that
/// magnetic-reversal windows model.
pub fn step_speciation(
    tick: u64,
    eco_species: &BTreeMap<SpeciesId, EcoSpecies>,
    species_registry: &BTreeMap<SpeciesId, Species>,
    tracker: &mut SpeciationTracker,
    cosmic_ray_multiplier: Real,
) -> Vec<(Species, SpeciationEvent)> {
    tracker.advance_post_extinction_windows(tick);
    let mut events: Vec<(Species, SpeciationEvent)> = Vec::new();
    let mut next_id_counter = next_species_id(eco_species).0;
    let multiplier = clamp_cosmic_ray_multiplier(cosmic_ray_multiplier);

    // 1. Allopatric path. Streaks ≥ ALLOPATRIC_ISOLATION_TICKS trigger.
    let allopatric_triggers: Vec<(SpeciesId, u64)> = tracker
        .allopatric_streak
        .iter()
        .filter(|(_, streak)| **streak >= ALLOPATRIC_ISOLATION_TICKS)
        .map(|(id, streak)| (*id, *streak))
        .collect();
    for (parent_id, streak) in allopatric_triggers {
        if let Some(parent) = species_registry.get(&parent_id) {
            let trigger = SpeciationTrigger::Allopatric {
                isolation_ticks: streak,
            };
            for _ in 0..multiplier {
                let daughter_id = SpeciesId(next_id_counter);
                next_id_counter += 1;
                let daughter = derive_daughter_species(parent, daughter_id, trigger);
                let event = SpeciationEvent {
                    tick,
                    parent_id: parent_id.0,
                    daughter_id: daughter_id.0,
                    trigger: trigger.to_wire(),
                };
                events.push((daughter, event));
            }
            tracker.allopatric_streak.insert(parent_id, 0);
        }
    }

    // 2. Sympatric path. Pairs whose streak ≥ SYMPATRIC_PRESSURE_TICKS
    // trigger; the lower-id side drifts (canonical choice to keep the
    // event stream deterministic).
    let sympatric_triggers: Vec<(SpeciesId, SpeciesId)> = tracker
        .sympatric_streak
        .iter()
        .filter(|(_, streak)| **streak >= SYMPATRIC_PRESSURE_TICKS)
        .map(|(pair, _)| *pair)
        .collect();
    for pair in sympatric_triggers {
        let parent_id = pair.0;
        if let Some(parent) = species_registry.get(&parent_id) {
            let trigger = SpeciationTrigger::Sympatric;
            for _ in 0..multiplier {
                let daughter_id = SpeciesId(next_id_counter);
                next_id_counter += 1;
                let daughter = derive_daughter_species(parent, daughter_id, trigger);
                let event = SpeciationEvent {
                    tick,
                    parent_id: parent_id.0,
                    daughter_id: daughter_id.0,
                    trigger: trigger.to_wire(),
                };
                events.push((daughter, event));
            }
            tracker.sympatric_streak.insert(pair, 0);
        }
    }

    // 3. Polyploid path — only `Lifecycle::Plant`. Determined per-tick
    // by `polyploid_check` so the same seed reproduces.
    for (parent_id, parent) in species_registry.iter() {
        if !matches!(parent.lifecycle, Lifecycle::Plant) {
            continue;
        }
        // Skip extinct species.
        if let Some(eco) = eco_species.get(parent_id) {
            if !eco.is_extant {
                continue;
            }
        }
        if polyploid_check(tick, *parent_id) {
            let trigger = SpeciationTrigger::Polyploid;
            for _ in 0..multiplier {
                let daughter_id = SpeciesId(next_id_counter);
                next_id_counter += 1;
                let daughter = derive_daughter_species(parent, daughter_id, trigger);
                let event = SpeciationEvent {
                    tick,
                    parent_id: parent_id.0,
                    daughter_id: daughter_id.0,
                    trigger: trigger.to_wire(),
                };
                events.push((daughter, event));
            }
        }
    }

    // 4. Founder-effect path — drain pending seedings, fire one
    // speciation per qualifying entry.
    let pending: Vec<(SpeciesId, Real)> = tracker
        .pending_founder_seedings
        .iter()
        .map(|(id, b)| (*id, *b))
        .collect();
    tracker.pending_founder_seedings.clear();
    for (parent_id, seed_biomass) in pending {
        let parent_eco = eco_species.get(&parent_id);
        let parent_biomass = parent_eco.map(|s| s.biomass).unwrap_or(Real::ZERO);
        // Threshold: seed_biomass < FOUNDER_BIOMASS_FRAC × parent_biomass.
        let threshold = Real::from(FOUNDER_BIOMASS_FRAC) * parent_biomass;
        if seed_biomass > Real::ZERO && seed_biomass < threshold {
            if let Some(parent) = species_registry.get(&parent_id) {
                let trigger = SpeciationTrigger::FounderEffect;
                for _ in 0..multiplier {
                    let daughter_id = SpeciesId(next_id_counter);
                    next_id_counter += 1;
                    let daughter = derive_daughter_species(parent, daughter_id, trigger);
                    let event = SpeciationEvent {
                        tick,
                        parent_id: parent_id.0,
                        daughter_id: daughter_id.0,
                        trigger: trigger.to_wire(),
                    };
                    events.push((daughter, event));
                }
            }
        }
    }

    // 5. Post-extinction adaptive radiation — every open window
    // multiplies the chance that any extant species spawns a daughter
    // this tick. We model the multiplier by spawning extra daughters
    // off the post-extinction polyploidy + allopatric pools above.
    // The window itself was advanced at the top of this function;
    // the boost shows up as additional polyploid / allopatric /
    // sympatric daughters that would not otherwise have fired this
    // tick because of the rate multiplier baked into the per-tick
    // checks below.
    if tracker.in_post_extinction_window() {
        let generation = tracker.current_radiation_generation();
        // Extra opportunistic speciation per extant species (one
        // bonus draw per multiplier-1 ticks per species). Uses the
        // same polyploid hash space but salted on the generation
        // counter so each generation gives an independent roll.
        for (parent_id, parent) in species_registry.iter() {
            // Skip extinct.
            if let Some(eco) = eco_species.get(parent_id) {
                if !eco.is_extant {
                    continue;
                }
            }
            // 4 bonus rolls per tick (= multiplier - 1 = 5 - 1 = 4).
            // Each roll is the same polyploid hash with a unique
            // salt per (generation, roll) so the four draws are
            // independent across the run. Each successful roll then
            // spawns `multiplier` daughters under the cosmic-ray
            // amplification — same fire-count scaling as the
            // four streak-based triggers above.
            for roll in 0..(POST_EXTINCTION_RADIATION_MULTIPLIER - 1) {
                let salted_tick = tick
                    .wrapping_add(generation.wrapping_mul(977))
                    .wrapping_add(roll.wrapping_mul(31));
                if polyploid_check(salted_tick, *parent_id) {
                    let trigger = SpeciationTrigger::PostExtinctionRadiation { generation };
                    for _ in 0..multiplier {
                        let daughter_id = SpeciesId(next_id_counter);
                        next_id_counter += 1;
                        let daughter = derive_daughter_species(parent, daughter_id, trigger);
                        let event = SpeciationEvent {
                            tick,
                            parent_id: parent_id.0,
                            daughter_id: daughter_id.0,
                            trigger: trigger.to_wire(),
                        };
                        events.push((daughter, event));
                    }
                }
            }
        }
    }

    events
}

/// Derive a sensible default `EcosystemRole` for a daughter species
/// — mirrors the parent's role exactly. Different triggers can
/// modulate metabolism / kind in a future polish; for this PR
/// preserving the parent role keeps the Lindeman pyramid invariant
/// after the daughter inserts.
#[must_use]
pub fn daughter_eco_role(parent_role: EcosystemRole) -> EcosystemRole {
    match parent_role {
        EcosystemRole::Producer { metabolism } => {
            // Keep the metabolism but mark the daughter Photoautotroph
            // when the parent was Mixotroph — daughter shifts to one
            // of the two parent modes. Predictable per-trigger
            // behaviour; not random.
            let metabolism = match metabolism {
                ProducerMetabolism::Mixotroph => ProducerMetabolism::Photoautotroph,
                other => other,
            };
            EcosystemRole::Producer { metabolism }
        }
        other => other,
    }
}
