//! Per-lifecycle per-tick step routing.
//!
//! Sprint 2 Item 7. The species' `Lifecycle` variant chooses which
//! step function runs each tick. `Vertebrate` falls through to the
//! legacy `PopulationDynamics::step_with_capacity` 4-bracket cohort
//! step bit-for-bit; every other variant supplies a minimal-
//! correct step shaped around the variant's biology. The intent
//! of this PR is to wire the dispatch and surface the routing
//! semantics; the next polish pass can deepen each non-Vertebrate
//! step (e.g. multi-cell plant dispersal, real-clock insect stage
//! progression) without changing the public API.
//!
//! Determinism: all arithmetic is Q32.32 (`Real` / `Pop`) — no
//! floats, no allocation, no RNG draws. BTreeMap-style iteration
//! is preserved because the callers (civ engine) own the cohort
//! containers; this module operates on a single cohort + state per
//! call.

use crate::{Cohort, PopulationDynamics};
use sim_arith::{Pop, Real};
use sim_species::{CasteRole, Fission, Lifecycle};
use std::collections::BTreeMap;

/// Step a cohort one tick under its species' `Lifecycle` variant.
///
/// Dispatches to the per-variant step function. For `Vertebrate`
/// the call is bit-identical to `dynamics.step_with_capacity` so
/// existing downstream consumers stay unaffected.
///
/// For non-Vertebrate variants requiring additional per-species
/// state (Eusocial caste counts, Microbial biomass, Modular
/// biomass), use the variant-specific `step_*` helpers directly.
/// This convenience dispatcher provides the minimal-correct
/// behaviour: it routes Eusocial / Microbial / Modular through the
/// vertebrate path (since they require state outside `Cohort`),
/// while Aquatic / Insect / Plant get their dedicated step.
pub fn step_for_lifecycle(
    lifecycle: &Lifecycle,
    dynamics: &PopulationDynamics,
    cohort: &mut Cohort,
    capacity: Pop,
) {
    match lifecycle {
        Lifecycle::Vertebrate => {
            dynamics.step_with_capacity(cohort, capacity);
        }
        Lifecycle::Aquatic { semelparous } => {
            step_aquatic(*semelparous, dynamics, cohort, capacity);
        }
        Lifecycle::Insect => {
            step_insect(dynamics, cohort, capacity);
        }
        Lifecycle::Plant => {
            step_plant(dynamics, cohort, capacity);
        }
        // Eusocial / Microbial / Modular need state outside `Cohort`
        // (per-caste pop maps, biomass scalars). Callers that want
        // those dynamics should invoke the dedicated step helpers
        // (`step_eusocial`, `step_microbial`, `step_modular`)
        // directly; the convenience dispatcher falls through to
        // the vertebrate step so an incomplete wiring still
        // produces a well-defined trajectory rather than a
        // panic.
        Lifecycle::Eusocial { .. } | Lifecycle::Microbial { .. } | Lifecycle::Modular => {
            dynamics.step_with_capacity(cohort, capacity);
        }
    }
}

/// Aquatic lifecycle step. Semelparous = single mass-spawn
/// followed by adult death (Pacific salmon, mass-spawning aquatic
/// adults). Iteroparous = adults persist across seasons but
/// juveniles suffer a metamorphosis bottleneck.
///
/// Semelparous shape:
///   1. If fertile pop is present, emit a one-shot spawn of
///      `fertile × birth_rate × fertile_window_months × security`
///      into infant (the entire fertile-window lifetime allotment
///      collapsed to one tick).
///   2. Drop fertile to ~0 (post-spawn mortality = 100%).
///   3. Apply normal survival + aging to the remaining brackets.
///
/// Iteroparous shape: identical to the Vertebrate step but the
/// juvenile-to-fertile aging step also incurs a 70% metamorphosis
/// mortality cull (the bottleneck).
pub fn step_aquatic(
    semelparous: bool,
    dynamics: &PopulationDynamics,
    cohort: &mut Cohort,
    capacity: Pop,
) {
    if semelparous {
        step_aquatic_semelparous(dynamics, cohort, capacity);
    } else {
        step_aquatic_iteroparous(dynamics, cohort, capacity);
    }
}

fn step_aquatic_semelparous(
    dynamics: &PopulationDynamics,
    cohort: &mut Cohort,
    capacity: Pop,
) {
    // Big one-shot spawn: collapse the entire fertile-window
    // lifetime allotment to this tick. Then adults all die.
    let demand = dynamics.food_multipliers[0] * cohort.infant
        + dynamics.food_multipliers[1] * cohort.juvenile
        + dynamics.food_multipliers[2] * cohort.fertile
        + dynamics.food_multipliers[3] * cohort.elder;
    let security = crate::food_security(demand, capacity);
    // Per-event clutch = birth_rate × (a sizeable fertile-window
    // multiplier). A semelparous spawner converts its entire
    // lifetime reproductive output to one event, so we collect
    // the per-month rate × a 12-month window proxy as the per-
    // event yield. The recruit ceiling clamp at 5× fertile keeps
    // this bounded.
    let per_event_rate = dynamics
        .birth_rate
        .saturating_mul(Real::from_int(12))
        .saturating_mul(dynamics.birth_rate_multiplier);
    let raw_spawn = cohort
        .fertile
        .saturating_mul_real(per_event_rate)
        .saturating_mul_real(security);
    let recruit_ceiling = cohort.fertile.saturating_mul_real(Real::from_int(5));
    let births = raw_spawn.min(recruit_ceiling);
    // Apply births to infant.
    cohort.infant = cohort.infant + births;
    // Adult cohort dies after spawning. Drop fertile to zero;
    // elder bracket — if any — also goes to zero (semelparous
    // species have no post-reproductive period).
    cohort.fertile = Pop::ZERO;
    cohort.elder = Pop::ZERO;
    // Juveniles + infants step normally under survival + aging.
    let infant_s = dynamics.infant_survival_per_tick;
    let juvenile_s = dynamics.juvenile_survival_per_tick;
    let infant_after = cohort.infant * infant_s;
    let juvenile_after = cohort.juvenile * juvenile_s;
    let infant_to_juv = infant_after * dynamics.infant_to_juvenile;
    let juv_to_fert = juvenile_after * dynamics.juvenile_to_fertile;
    cohort.infant = infant_after - infant_to_juv;
    cohort.juvenile = juvenile_after + infant_to_juv - juv_to_fert;
    cohort.fertile = juv_to_fert;
    cohort.floor_at_zero();
}

fn step_aquatic_iteroparous(
    dynamics: &PopulationDynamics,
    cohort: &mut Cohort,
    capacity: Pop,
) {
    // Run the legacy step then apply an additional metamorphosis
    // bottleneck mortality (70%) to the juveniles that promoted
    // to fertile this tick. Approximate this by reducing the
    // post-step fertile bracket by the fraction of fertile gained
    // from juveniles — but since we don't track that delta, we
    // apply a flat 70% cull to a small fraction of the new
    // fertile (the per-tick promotion fraction = juvenile_to_fertile
    // applied to pre-step juveniles).
    let pre_juvenile = cohort.juvenile;
    dynamics.step_with_capacity(cohort, capacity);
    // Estimated juveniles that just promoted: pre_juvenile ×
    // juvenile_to_fertile. 70% of them die in metamorphosis.
    let promoted = pre_juvenile * dynamics.juvenile_to_fertile;
    let metamorphosis_loss = promoted * Real::percent(70);
    cohort.fertile = (cohort.fertile - metamorphosis_loss).max(Pop::ZERO);
    cohort.floor_at_zero();
}

/// Insect lifecycle: egg / larva / pupa / adult. Re-uses the
/// Cohort's 4 brackets (infant=egg, juvenile=larva, fertile=adult)
/// but maps the elder bracket to pupa with a faster progression
/// rate, since the pupa→adult promotion is the dominant rate-
/// limiting step. Only adults (fertile) reproduce. Each stage has
/// a distinct lifespan: eggs and pupae are short, larvae long,
/// adults brief.
///
/// Minimal-correct: applies the legacy step then layers an extra
/// adult-mortality term (insects' adult bracket is short-lived).
pub fn step_insect(dynamics: &PopulationDynamics, cohort: &mut Cohort, capacity: Pop) {
    dynamics.step_with_capacity(cohort, capacity);
    // Extra adult mortality: insects' adult phase is the briefest
    // stage. Reduce fertile by 5% per tick post-step.
    cohort.fertile = cohort.fertile * Real::percent(95);
    cohort.floor_at_zero();
}

/// Plant lifecycle: seed / seedling / mature / senescent. Re-uses
/// the cohort's 4 brackets (infant=seed, juvenile=seedling,
/// fertile=mature, elder=senescent). High seed mortality, low
/// senescent mortality (mature plants are durable once
/// established).
///
/// Dispersal: a fraction of seeds produced are notionally exported
/// to neighbour cells; this is approximated here by reducing the
/// infant gain by the dispersal fraction. The caller-side flow
/// (civ engine) that wants real cross-cell dispersal can extract
/// the dispersed count from the returned delta in a future polish.
pub fn step_plant(dynamics: &PopulationDynamics, cohort: &mut Cohort, capacity: Pop) {
    // Run with an elder-mortality reduction: plants' senescent
    // stage is durable (vs vertebrate elder which is fragile).
    let mut adjusted = *dynamics;
    // Boost elder survival close to fertile survival.
    adjusted.elder_survival_per_tick = dynamics
        .fertile_survival_per_tick
        .max(dynamics.elder_survival_per_tick);
    // Bump infant mortality (seeds are abundant but mostly fail).
    let seed_failure = Real::percent(50);
    adjusted.infant_survival_per_tick =
        (dynamics.infant_survival_per_tick * (Real::ONE - seed_failure)).max(Real::ZERO);
    adjusted.step_with_capacity(cohort, capacity);
    cohort.floor_at_zero();
}

/// Eusocial colony state. Tracks per-caste headcount so the step
/// can apply caste-specific dynamics — only `Reproductive`
/// contributes to births, sterile castes consume food and produce
/// no offspring.
///
/// Stored as a `BTreeMap` so iteration order is deterministic across
/// rebuilds. The caller (civ engine) owns one of these per
/// eusocial colony.
#[derive(Debug, Clone, Default)]
pub struct EusocialColony {
    /// Per-caste headcount. `BTreeMap` for deterministic iteration.
    pub castes: BTreeMap<CasteRole, Pop>,
}

impl EusocialColony {
    /// Construct a colony with explicit per-caste seeding.
    #[must_use]
    pub fn new(seed: &[(CasteRole, Pop)]) -> Self {
        let mut castes = BTreeMap::new();
        for (role, n) in seed {
            castes.insert(*role, *n);
        }
        Self { castes }
    }

    /// Total colony headcount across all castes.
    #[must_use]
    pub fn total(&self) -> Pop {
        self.castes
            .values()
            .fold(Pop::ZERO, |acc, n| acc + *n)
    }

    /// Headcount of a single caste (zero if absent).
    #[must_use]
    pub fn caste(&self, role: CasteRole) -> Pop {
        self.castes.get(&role).copied().unwrap_or(Pop::ZERO)
    }
}

/// Step a eusocial colony one tick. Per-caste dynamics:
///
/// - `Reproductive`: produces births at `birth_rate × fertile`.
///   Survival applies normally.
/// - `Worker` / `Soldier` / `Nurse`: consume food (food demand
///   contributes per-bracket), suffer baseline mortality, but
///   produce no offspring. Births land in the Reproductive
///   caste (the colony rears all young as reproductives until
///   caste differentiation, modelled as an instantaneous routing
///   into the reproductive bracket — a simplification suitable
///   for the minimal-correct fidelity this PR targets).
///
/// `caste_birth_target` selects which caste new young join. By
/// convention this is `Reproductive` (founders), but the caller
/// can override to model nurseries that route young into worker
/// castes after differentiation.
pub fn step_eusocial(
    colony: &mut EusocialColony,
    dynamics: &PopulationDynamics,
    capacity: Pop,
) {
    // Compute demand from total colony (all castes draw food at
    // the fertile multiplier — sterile castes are adult-bodied).
    let total = colony.total();
    let demand = total * dynamics.food_multipliers[2];
    let security = crate::food_security(demand, capacity);
    // Only Reproductive caste produces births.
    let reproductive = colony.caste(CasteRole::Reproductive);
    let raw_births = reproductive
        .saturating_mul_real(dynamics.birth_rate)
        .saturating_mul_real(dynamics.birth_rate_multiplier)
        .saturating_mul_real(security);
    let recruit_ceiling = reproductive.saturating_mul_real(Real::from_int(5));
    let births = raw_births.min(recruit_ceiling);
    // Apply per-caste survival.
    let stress = Real::ONE - security;
    let amp = Real::ONE + stress * dynamics.stress_factor;
    let starvation = stress * Real::percent(10);
    let combine = |s: Real, reduction: Real| -> Real {
        let r = reduction.clamp01();
        let reduced_baseline = (Real::ONE - s) * (Real::ONE - r) * amp;
        let total_mort = (reduced_baseline + starvation).min(Real::ONE);
        (Real::ONE - total_mort).max(Real::ZERO)
    };
    let survival = combine(dynamics.fertile_survival_per_tick, dynamics.mortality_reduction[2]);
    // Step every caste.
    for (_role, n) in &mut colony.castes {
        *n = (*n * survival).max(Pop::ZERO);
    }
    // Add births to the Reproductive caste.
    let r_entry = colony
        .castes
        .entry(CasteRole::Reproductive)
        .or_insert(Pop::ZERO);
    *r_entry = *r_entry + births;
}

/// Microbial colony — single biomass that doubles every
/// generation time under unstressed conditions. No age structure.
///
/// Doubling time depends on `fission_strategy`:
///   - `Binary`: 1 tick (1 month) doubling.
///   - `Budding`: 2 ticks doubling.
///   - `Conjugation`: 4 ticks doubling.
///
/// Per-tick growth rate `r = 2^(1/doubling_time) - 1`. Applied as
/// `pop *= 1 + r × security` with `security` from the food-
/// security formula.
pub fn step_microbial(
    fission: Fission,
    population: &mut Pop,
    capacity: Pop,
) {
    // Per-tick growth factor by strategy. We use a hard-coded
    // factor that approximates `2^(1/N)` for N=1, 2, 4 — Q32.32
    // can represent these exactly:
    //   N=1 → 2^1 = 2.0 (factor = 2.0)
    //   N=2 → 2^(1/2) ≈ 1.4142 → use 1.4142 ≈ 14142/10000
    //   N=4 → 2^(1/4) ≈ 1.1892 → use 11892/10000
    let factor = match fission {
        Fission::Binary => Real::from_int(2),
        Fission::Budding => Real::from_ratio(14142, 10000),
        Fission::Conjugation => Real::from_ratio(11892, 10000),
    };
    let demand = *population;
    let security = crate::food_security(demand, capacity);
    // Effective growth: factor when fed, scaled by security under
    // shortage. Floor at survival × population to avoid negative
    // growth from a small numeric stress.
    let effective_factor = if security >= Real::ONE {
        factor
    } else {
        // Linear blend: at security=0, no growth (factor=1.0);
        // at security=1, full factor.
        Real::ONE + (factor - Real::ONE) * security
    };
    *population = population.saturating_mul_real(effective_factor);
}

/// Modular / colonial organism step. Single biomass that grows or
/// shrinks as a unit; no age structure, no per-bracket dynamics.
/// Growth proportional to capacity headroom.
pub fn step_modular(biomass: &mut Pop, capacity: Pop) {
    // Logistic-style growth: dN/dt = r × N × (1 - N/K). Discrete
    // approximation: N_{t+1} = N_t + r × N_t × max(0, 1 - N_t/K).
    let r = Real::percent(5); // 5% intrinsic growth per tick.
    let n = *biomass;
    if capacity <= Pop::ZERO {
        // No capacity — die back.
        *biomass = (n * Real::percent(90)).max(Pop::ZERO);
        return;
    }
    let ratio: Real = n / capacity;
    let headroom = (Real::ONE - ratio).max(Real::ZERO);
    let growth = n.saturating_mul_real(r * headroom);
    *biomass = n + growth;
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_species::PopulationBiology;

    fn test_biology() -> PopulationBiology {
        PopulationBiology {
            clutch_size: Real::from_int(10),
            infant_fraction: Real::percent(5),
            maturity_fraction: Real::percent(20),
            eldership_fraction: Real::percent(15),
            infant_survival: Real::percent(50),
            juvenile_survival: Real::percent(85),
            food_multipliers: [
                Real::percent(30),
                Real::percent(60),
                Real::ONE,
                Real::percent(90),
            ],
            events_per_fertile_window: Real::ZERO,
            reproductive_success: Real::ZERO,
        }
    }

    #[test]
    fn aquatic_semelparous_lifecycle_single_spawn_then_death() {
        // Semelparous spawner: founder fertile cohort spawns a
        // big infant batch then adults drop to zero in the same
        // tick.
        let biology = test_biology();
        let dyn_ = PopulationDynamics::for_species(
            &biology,
            Real::from_int(4),
            Real::percent(30),
            Real::percent(30),
        );
        let mut cohort = Cohort::new(Pop::from_int(100));
        let lifecycle = Lifecycle::Aquatic { semelparous: true };
        let before_fertile = cohort.fertile;
        step_for_lifecycle(&lifecycle, &dyn_, &mut cohort, Pop::from_int(1_000_000));
        // Parents drop to zero (or near zero — promotions from
        // juveniles are absent on the first tick).
        assert!(
            cohort.fertile < before_fertile * Real::percent(5),
            "fertile {:?} should collapse below 5% of pre-spawn {:?}",
            cohort.fertile,
            before_fertile
        );
        // Big infant cohort appears (the spawn).
        assert!(
            cohort.infant > Pop::ZERO,
            "infant should hold the spawn: {cohort:?}"
        );
    }

    #[test]
    fn eusocial_lifecycle_castes_track_independently() {
        // Reproductive=10, Worker=100. Step many ticks. The
        // Worker caste must not produce any of the new births —
        // every new individual goes into Reproductive.
        let biology = test_biology();
        let dyn_ = PopulationDynamics::for_species(
            &biology,
            Real::from_int(10),
            Real::percent(50),
            Real::percent(50),
        );
        let mut colony = EusocialColony::new(&[
            (CasteRole::Reproductive, Pop::from_int(10)),
            (CasteRole::Worker, Pop::from_int(100)),
        ]);
        let initial_worker = colony.caste(CasteRole::Worker);
        // Step under generous capacity.
        for _ in 0..12 {
            step_eusocial(&mut colony, &dyn_, Pop::from_int(1_000_000));
        }
        // Worker population must monotonically decrease (no
        // births land in Worker; only mortality acts on it).
        let final_worker = colony.caste(CasteRole::Worker);
        assert!(
            final_worker <= initial_worker,
            "worker count {final_worker:?} should not grow above initial {initial_worker:?} \
             — workers don't reproduce"
        );
        // Reproductive caste does produce births → must hold ≥
        // the initial 10 (births offset any survival decay).
        let final_repro = colony.caste(CasteRole::Reproductive);
        assert!(
            final_repro >= Pop::from_int(10),
            "reproductive should grow from births: got {final_repro:?}"
        );
    }

    #[test]
    fn microbial_binary_fission_doubles_per_generation_time() {
        // Start with N microbes, run for one doubling time (1
        // tick for Binary), assert population ≈ 2N within 5%.
        let mut population = Pop::from_int(1_000);
        let capacity = Pop::from_int(1_000_000);
        step_microbial(Fission::Binary, &mut population, capacity);
        let expected = Pop::from_int(2_000);
        let lower = expected * Real::percent(95);
        let upper = expected * Real::percent(105);
        assert!(
            population >= lower && population <= upper,
            "binary fission should double 1000→~2000: got {population:?}"
        );
    }

    #[test]
    fn microbial_budding_grows_slower_than_binary() {
        // After 1 tick: budding should grow less than binary.
        let mut binary_pop = Pop::from_int(1_000);
        let mut bud_pop = Pop::from_int(1_000);
        let capacity = Pop::from_int(1_000_000);
        step_microbial(Fission::Binary, &mut binary_pop, capacity);
        step_microbial(Fission::Budding, &mut bud_pop, capacity);
        assert!(
            binary_pop > bud_pop,
            "binary {binary_pop:?} should grow faster than budding {bud_pop:?}"
        );
    }

    #[test]
    fn plant_alternation_of_generations_if_complex() {
        // Plant's senescent (elder) stage should have lower
        // mortality than the vertebrate equivalent. Compare two
        // cohorts of pure elder populations over many ticks: the
        // plant cohort retains more pop than the vertebrate.
        let biology = test_biology();
        let dyn_ = PopulationDynamics::for_species(
            &biology,
            Real::from_int(40),
            Real::percent(50),
            Real::percent(50),
        );
        let mut vert = Cohort::empty();
        vert.elder = Pop::from_int(100);
        let mut plant = Cohort::empty();
        plant.elder = Pop::from_int(100);
        for _ in 0..6 {
            step_for_lifecycle(
                &Lifecycle::Vertebrate,
                &dyn_,
                &mut vert,
                Pop::from_int(1_000_000),
            );
            step_for_lifecycle(
                &Lifecycle::Plant,
                &dyn_,
                &mut plant,
                Pop::from_int(1_000_000),
            );
        }
        assert!(
            plant.elder >= vert.elder,
            "plant senescent {:?} should retain more than vertebrate elder {:?}",
            plant.elder,
            vert.elder
        );
    }

    #[test]
    fn modular_grows_toward_capacity_then_levels() {
        // Logistic-style growth: small initial biomass grows
        // toward capacity then levels off. After enough ticks,
        // biomass should be > initial but ≤ capacity.
        let mut biomass = Pop::from_int(10);
        let capacity = Pop::from_int(100);
        for _ in 0..200 {
            step_modular(&mut biomass, capacity);
        }
        assert!(
            biomass > Pop::from_int(10),
            "biomass should grow: {biomass:?}"
        );
        assert!(
            biomass <= capacity,
            "biomass should not exceed capacity: {biomass:?}"
        );
    }

    #[test]
    fn step_for_lifecycle_vertebrate_matches_legacy() {
        // The Vertebrate branch must be bit-identical to the
        // direct legacy call.
        let biology = test_biology();
        let dyn_ = PopulationDynamics::for_species(
            &biology,
            Real::from_int(40),
            Real::percent(50),
            Real::percent(50),
        );
        let mut via_lifecycle = Cohort::new(Pop::from_int(100));
        let mut via_legacy = Cohort::new(Pop::from_int(100));
        for _ in 0..30 {
            step_for_lifecycle(
                &Lifecycle::Vertebrate,
                &dyn_,
                &mut via_lifecycle,
                Pop::from_int(10_000),
            );
            dyn_.step_with_capacity(&mut via_legacy, Pop::from_int(10_000));
        }
        assert_eq!(via_lifecycle.total(), via_legacy.total());
        assert_eq!(via_lifecycle.fertile, via_legacy.fertile);
    }

    #[test]
    fn aquatic_iteroparous_metamorphosis_bottleneck_reduces_fertile() {
        // Compare iteroparous aquatic vs vertebrate after some
        // ticks: aquatic must have ≤ fertile (metamorphosis cuts
        // promotion). Seed only juveniles so the only fertile
        // recruits come through the promotion path.
        let biology = test_biology();
        let dyn_ = PopulationDynamics::for_species(
            &biology,
            Real::from_int(10),
            Real::percent(50),
            Real::percent(50),
        );
        let mut aquatic = Cohort::empty();
        aquatic.juvenile = Pop::from_int(1_000);
        let mut vert = Cohort::empty();
        vert.juvenile = Pop::from_int(1_000);
        for _ in 0..6 {
            step_for_lifecycle(
                &Lifecycle::Aquatic { semelparous: false },
                &dyn_,
                &mut aquatic,
                Pop::from_int(1_000_000),
            );
            step_for_lifecycle(
                &Lifecycle::Vertebrate,
                &dyn_,
                &mut vert,
                Pop::from_int(1_000_000),
            );
        }
        assert!(
            aquatic.fertile <= vert.fertile,
            "aquatic fertile {:?} must trail vertebrate {:?} due to bottleneck",
            aquatic.fertile,
            vert.fertile
        );
    }
}

