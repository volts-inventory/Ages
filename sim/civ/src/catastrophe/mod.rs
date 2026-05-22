//! catastrophes — the 5-kind death-amplifier set. Each kind
//! has its own per-tick trigger predicate, its own cooldown, and
//! its own population-loss fraction; `check_and_apply` orchestrates
//! the per-civ check + apply step.

mod cells;
mod factors;
mod triggers;

pub use cells::{
    apply_to_cell_and_neighbors, densest_claimed_cell, deterministic_cell_pick, hex_neighbors,
};
pub use factors::{disease_severity_factor, ice_age_severity_factor, volcanic_cooldown_factor};

use crate::cosmology::Cosmology;
use crate::Civ;
use sim_arith::{Pop, Real};
use sim_ecosystem::PlanetEcosystem;
use sim_physics::{PhysicsState, Substance};
use sim_species::{apply_catastrophe_with_dormancy, Species};
use sim_world::Planet;

use triggers::{asteroid_fires, disease_fires, ice_age_fires, solar_flare_fires, volcanic_fires};

/// catastrophe taxonomy. Five kinds — `Volcanic` and
/// `Disease` are the M4-min lithosphere/biosphere triggers,
/// plus three later additions for story diversity:
/// `Asteroid` (rare-event impact), `SolarFlare` (high stellar
/// luminosity + weak magnetosphere → EM disruption), and
/// `IceAge` (sustained planet-mean temperature drop).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatastropheKind {
    Volcanic,
    Disease,
    Asteroid,
    SolarFlare,
    IceAge,
}

impl CatastropheKind {
    pub fn tag(self) -> &'static str {
        match self {
            CatastropheKind::Volcanic => "volcanic",
            CatastropheKind::Disease => "disease",
            CatastropheKind::Asteroid => "asteroid",
            CatastropheKind::SolarFlare => "solar_flare",
            CatastropheKind::IceAge => "ice_age",
        }
    }
}

/// Per-kind cooldown (ticks). Placeholders under. : scaled
/// ×12 so the year-equivalent recurrence matches the old yearly
/// cadence under 1 tick = 1 month.
pub const VOLCANIC_COOLDOWN_TICKS: u64 = 200 * protocol::MONTHS_PER_YEAR;
pub const DISEASE_COOLDOWN_TICKS: u64 = 500 * protocol::MONTHS_PER_YEAR;
pub const ASTEROID_COOLDOWN_TICKS: u64 = 5_000 * protocol::MONTHS_PER_YEAR;
pub const SOLAR_FLARE_COOLDOWN_TICKS: u64 = 800 * protocol::MONTHS_PER_YEAR;
pub const ICE_AGE_COOLDOWN_TICKS: u64 = 4_000 * protocol::MONTHS_PER_YEAR;
pub const DISEASE_AGE_FLOOR_TICKS: u64 = 300 * protocol::MONTHS_PER_YEAR;

/// Population-fraction lost on each kind ( placeholders).
pub const VOLCANIC_POP_LOSS: (i64, i64) = (5, 100);
pub const DISEASE_POP_LOSS: (i64, i64) = (30, 100);
pub const ASTEROID_POP_LOSS: (i64, i64) = (40, 100);
pub const SOLAR_FLARE_POP_LOSS: (i64, i64) = (10, 100);
pub const ICE_AGE_POP_LOSS: (i64, i64) = (20, 100);

/// One catastrophe applied this tick — what kind, and the
/// fraction of population lost.
#[derive(Debug, Clone, Copy)]
pub struct CatastropheRecord {
    pub kind: CatastropheKind,
    pub fraction_lost: Real,
}

/// Per-catastrophe severity factor ∈ [0, 1] for the dormancy
/// damage-reduction formula. Sprint 2 Item 7b pins this at 1.0
/// (full-severity catastrophes) for all five kinds; a future
/// polish pass can expose a per-kind table if a follow-up wants
/// shallow events to bypass dormancy benefit. Centralised here so
/// the constant lives in one place.
const DORMANCY_SEVERITY_FACTOR: Real = Real::ONE;

/// Per-cell baseline radiation flux, Earth-surface units. Sits
/// well below the aqueous-default `radiation_max = 0.5` so a
/// quiet planet doesn't already saturate the radiation gate.
/// Catastrophe-specific deltas (solar flare, etc) are added on
/// top of this in the per-call-site cell-conditions builder.
fn baseline_radiation_flux() -> Real {
    Real::from_ratio(1, 10)
}

/// Post-flare radiation magnitude added on top of the baseline
/// when a solar flare hits. Sits above the aqueous-default
/// `radiation_max = 0.5` so a narrow-envelope species takes the
/// full flare damage, while an extremophile with
/// `radiation_max ≥ 5` still has plenty of envelope headroom.
fn solar_flare_radiation_boost() -> Real {
    Real::ONE
}

/// Post-impact radiation magnitude added on top of the baseline
/// for the ecosystem signature of an asteroid strike (T2). Set
/// well above the aqueous-default `radiation_max = 0.5` so a
/// narrow-envelope eco species takes the full hit while an
/// extremophile with high `radiation_max` retains envelope
/// headroom. Models the impactor's prompt gamma + activation
/// products at the strike site.
fn asteroid_radiation_boost() -> Real {
    Real::from_int(5)
}

/// Drop in cell temperature applied when an ice age fires, in K.
/// Pushes the cell's read-out temperature below the aqueous
/// envelope's lower bound (273 K) for cold-baseline planets so
/// the temperature gate flags the catastrophe to a narrow-
/// envelope species.
fn ice_age_temp_drop_k() -> Real {
    Real::from_int(50)
}

/// Pa per atm — conversion factor between `planet.surface_pressure`
/// (Pa) and the tolerance envelope's pressure range (atm).
fn pa_per_atm() -> Real {
    Real::from_int(101_325)
}

/// Build the `(temperature, pH, salinity, radiation, pressure)`
/// tuple a catastrophe-affected cell exposes to the tolerance
/// envelope. The hex grid only carries temperature + pressure
/// per cell; pH and salinity are derived from planet-level
/// substrate defaults (neutral pH, Earth-ocean-baseline salinity)
/// so the radiation/temperature axes drive the differential.
///
/// `temp_delta_k` adjusts the read-out temperature (negative for
/// ice age cold snap, zero otherwise). `extra_rad` adds to the
/// baseline radiation flux (positive for solar flare; pre-
/// multiplied by `cosmic_ray_ground_flux` at the call site,
/// clamped to `[0.2, 5.0]` in T8 so a magnetic-reversal window
/// amplifies post-flare ground flux while a strong stable dipole
/// attenuates it).
fn catastrophe_cell_conditions(
    state: &PhysicsState,
    planet: &Planet,
    cell: usize,
    temp_delta_k: Real,
    extra_rad: Real,
) -> (Real, Real, Real, Real, Real) {
    let temp_slice = state.temperature();
    let pressure_slice = state.pressure();
    // Per-cell temperature with the catastrophe delta applied.
    // Fall back to the planet mean if the cell index is out of
    // range (defensive — callers always pass a valid index).
    let cell_t = temp_slice
        .get(cell)
        .copied()
        .unwrap_or(planet.mean_temperature);
    let t = (cell_t + temp_delta_k).max(Real::ZERO);
    // Neutral pH — no per-cell ocean-chemistry field yet. Pinned
    // to the centre of the aqueous envelope so the pH axis stays
    // a non-binding gate; substrate-specific pH biases land when
    // a richer ocean-chemistry field exists.
    let ph = Real::from_int(7);
    // Earth-ocean-baseline salinity (g/L). Sits inside every
    // substrate default's salinity range so this axis is non-
    // binding under default planets; a future per-cell salinity
    // field can plug in here.
    let sal = Real::from_int(20);
    // Radiation: baseline ground flux plus the event-specific
    // boost (already scaled by cosmic-ray amplification at the
    // call site for SolarFlare).
    let rad = baseline_radiation_flux() + extra_rad;
    // Pressure: prefer the per-cell state value if non-zero (Pa);
    // otherwise fall back to the planet's surface pressure (Pa).
    // Convert to atm for the tolerance envelope.
    let p_pa = pressure_slice
        .get(cell)
        .copied()
        .filter(|v| *v > Real::ZERO)
        .unwrap_or(planet.surface_pressure);
    let atm = pa_per_atm();
    let p = if atm > Real::ZERO {
        p_pa / atm
    } else {
        Real::ONE
    };
    (t, ph, sal, rad, p)
}

/// Wrap `apply_catastrophe_with_dormancy` with the per-civ
/// existing `apply_catastrophe_resistance` (tools soften the
/// blow), then the per-species dormancy reduction (tardigrade-
/// grade species shrug off catastrophes), then the per-species
/// `ToleranceEnvelope::match_score` so an extremophile shaped to
/// the affected cell's conditions rides out the catastrophe and
/// a narrow-envelope species takes the full hit. Tools first
/// preserves pre-existing behaviour for fixtures with
/// `dormancy = 0` and centre-of-envelope species.
///
/// Formula:
///   base_loss = raw_frac × (1 − civ_tool_resistance) × (1 − match_score)
///   after_dormancy = base_loss × (1 − dormancy × severity)
///
/// `match_score = 1.0` (perfect envelope fit) ⇒ zero damage;
/// `match_score = 0.0` (outside envelope) ⇒ full damage. The
/// returned fraction equals `after_dormancy`. Algebraically
/// identical to the pre-P1.3 form that applied `(1 − match_score)`
/// last; the rearrangement exposes `base_loss` (the loss fraction
/// the species would suffer without its dormancy trait) for the
/// seed-bank routing below.
///
/// **P1.3 — dormant-pool seeding:** in addition to returning the
/// realised loss fraction, this function deposits
/// `pop_before × base_loss × dormancy × severity` into
/// `civ.dormant_pool.population`. That's the fraction of the
/// would-be casualties — the people the catastrophe would have
/// killed without the species' dormancy trait — that enter
/// cryptobiosis instead of dying. `DormantPool::resurrect_step`
/// drains the reservoir back into the active cohort at 1%/tick
/// over the subsequent run. `pre_catastrophe_population` is also
/// bumped to track the largest active cohort ever observed so
/// the resurrection cap stays honest for civs that lose
/// population to multiple consecutive catastrophes.
fn apply_resistance_and_dormancy(
    civ: &mut Civ,
    species: &Species,
    raw_frac: Real,
    cell: (Real, Real, Real, Real, Real),
    tick: u64,
) -> Real {
    let after_tools = civ.apply_catastrophe_resistance(raw_frac);
    let (t, ph, sal, rad, p) = cell;
    let survival_match = species.tolerance.match_score(t, ph, sal, rad, p);
    // `base_loss` is the loss fraction *before* the dormancy
    // reduction — i.e. the share of population that would die if
    // the species had no cryptobiosis trait. Tolerance and tool
    // resistance both fold into this; only the dormancy term is
    // separated out so we can route it into the seed bank.
    let base_loss = after_tools * (Real::ONE - survival_match);
    let after_dormancy = apply_catastrophe_with_dormancy(
        species.dormancy_capability,
        base_loss,
        DORMANCY_SEVERITY_FACTOR,
    );
    // Seed the dormant pool with the would-be casualties absorbed
    // by dormancy: `pop_before × base_loss × dormancy × severity`.
    // Equivalent to `pop_before × (base_loss − after_dormancy)` —
    // the headcount the dormancy multiplier just diverted out of
    // the death column.
    let pop_before = civ.cohort.total();
    // Track the high-water mark of the civ's active population so
    // `DormantPool::resurrect_step`'s cap reflects the largest
    // cohort the civ has ever held — not just the initial founder
    // population.
    if pop_before > civ.pre_catastrophe_population {
        civ.pre_catastrophe_population = pop_before;
    }
    let dormant_share = species.dormancy_capability * DORMANCY_SEVERITY_FACTOR;
    if dormant_share > Real::ZERO && pop_before > Pop::ZERO {
        let pop_before_real = pop_before.to_real_nonneg();
        let dormant_seeded = pop_before_real * base_loss * dormant_share;
        if dormant_seeded > Real::ZERO {
            civ.dormant_pool.population = civ.dormant_pool.population + dormant_seeded;
            civ.dormant_pool.entered_tick = tick;
        }
    }
    after_dormancy
}

/// Per-tick catastrophe check. Mutates the civ (cohort + last_*
/// timestamps), the physics state (volcanic resets a cell), and
/// — F2 (xeno N2) — the planet ecosystem's per-cell biomass for
/// the species directly tied to the cell (producers on a volcanic
/// eruption cell, the densest-claimed cell for disease, etc.).
/// Per-cell coupling makes heterogeneous catastrophes possible:
/// a volcanic eruption on one cell starves only that cell's
/// producers, not the planet-wide aggregate.
/// Returns the record so the caller can emit `CatastropheFired`
/// and update `last_catastrophe_tick`.
///
/// `ecosystem` is `Option` so legacy callers (older fixtures, tests
/// that don't care about the ecosystem coupling) keep working. The
/// production callsite in `sim-core::run` passes `Some(&mut
/// ecosystem)` so the per-cell biomass tracks heterogeneous
/// catastrophe damage; tests that want to assert "the catastrophe
/// reduces eco biomass at cell N only" pass `Some` too.
#[allow(clippy::too_many_lines)]
pub fn check_and_apply(
    civ: &mut Civ,
    state: &mut PhysicsState,
    planet: &Planet,
    species: &Species,
    tick: u64,
    ecosystem: Option<&mut PlanetEcosystem>,
) -> Option<CatastropheRecord> {
    if !civ.is_active() {
        return None;
    }
    // F2 — hold ecosystem behind `Option<&mut>` so each branch can
    // reborrow as needed without moving out.
    let mut ecosystem = ecosystem;
    // Volcanic — check first since its physical signature is
    // explicit; disease is the demographic backstop.
    // volcanic cooldown scales with crust — Basaltic
    // baseline, Hydrocarbon shorter (more frequent), older crusts
    // longer. Computed in Q32.32 then converted back to ticks.
    let volcanic_factor = volcanic_cooldown_factor(planet.crust);
    let scaled_cooldown_real =
        Real::from_int(i64::try_from(VOLCANIC_COOLDOWN_TICKS).unwrap_or(i64::MAX))
            * volcanic_factor;
    let scaled_volcanic_cooldown: u64 =
        u64::try_from(scaled_cooldown_real.raw().to_num::<i64>().max(1))
            .unwrap_or(VOLCANIC_COOLDOWN_TICKS);
    let volcanic_ready = civ
        .last_volcanic_tick
        .is_none_or(|t| tick.saturating_sub(t) >= scaled_volcanic_cooldown);
    if volcanic_ready {
        if let Some(cell) = volcanic_fires(state) {
            // Reset the cell: zero its fuel, drop temperature 50 K.
            state.substance_mut(Substance::Fuel.idx())[cell] = Real::ZERO;
            let cur = state.temperature()[cell];
            state.temperature_mut()[cell] = (cur - Real::from_int(50)).max(Real::ZERO);
            // region-targeted population loss: scales the
            // affected cell's region cohort by the volcanic
            // fraction. Aggregate cohort updates in sync.
            // PermanentMasonry / DefensiveFortification
            // soften the blow via apply_catastrophe_resistance.
            // Tolerance: volcanic spike already mutated cell temp
            // above (down by 50 K post-eruption); read the cell as-is
            // with no extra rad/temp delta so the envelope sees the
            // realised state.
            let raw_frac = Real::from(VOLCANIC_POP_LOSS);
            let cell_conds =
                catastrophe_cell_conditions(state, planet, cell, Real::ZERO, Real::ZERO);
            let frac = apply_resistance_and_dormancy(civ, species, raw_frac, cell_conds, tick);
            let cell_u32 = u32::try_from(cell).unwrap_or(u32::MAX);
            let lost_in_region = civ.drop_cell_pop(cell_u32, frac);
            // For civs without claimed_cells (legacy / tests),
            // fall back to the aggregate-fraction loss so the
            // catastrophe still has an effect.
            if lost_in_region == Pop::ZERO {
                let target = (civ.cohort.total() * (Real::ONE - frac)).max(Pop::ZERO);
                civ.cohort.shrink_to(target);
            }
            // F2 (xeno N2) / T2 — drain ecosystem biomass for
            // every extant species, tolerance-gated by the
            // eruption cell's post-event conditions. Volcanic
            // ejecta + 50K cell-temp drop sterilises the local
            // primary-production layer; species whose envelopes
            // contain the post-event cell (e.g. thermophiles)
            // shrug off the burst, while narrow-envelope species
            // take the headline volcanic loss. Calibrated to the
            // raw volcanic loss fraction (not the post-resistance
            // civ frac) so the eco signature reflects the headline
            // catastrophe severity; each eco species' own
            // tolerance envelope (not the civ species') gates the
            // realised loss.
            if let Some(eco) = ecosystem.as_deref_mut() {
                let (t, ph, sal, rad, p) = cell_conds;
                eco.apply_catastrophe_at_cell(raw_frac, t, ph, sal, rad, p);
            }
            civ.last_volcanic_tick = Some(tick);
            civ.last_catastrophe_tick = Some(tick);
            return Some(CatastropheRecord {
                kind: CatastropheKind::Volcanic,
                fraction_lost: frac,
            });
        }
    }
    // Disease — cell-targeted: starts at the densest cell
    // and spreads to adjacent claimed cells. Pre- it was a
    // uniform civ-wide pop drop, which read as artificial: a
    // plague hits cities first, not equally everywhere.
    //
    // Disease is biology-driven (crowding-disease dynamics tied to
    // generational time), so its cooldown stretches with substrate
    // metabolism — a silicate civ doesn't experience the same plague
    // cadence as an aqueous one in absolute ticks. The physics-
    // driven kinds (volcanic / asteroid / solar / ice age) keep raw
    // cooldowns: those are external to biology.
    let metabolism = planet.metabolic_substrate.metabolism();
    let disease_cooldown =
        crate::demographics::streak_ticks_for_metabolism(DISEASE_COOLDOWN_TICKS, metabolism);
    let disease_ready = civ
        .last_disease_tick
        .is_none_or(|t| tick.saturating_sub(t) >= disease_cooldown);
    if disease_ready && disease_fires(civ, state, planet, tick) {
        // severity scales with biosphere richness. :
        // BasicHealing / MedicalIntervention / AdvancedMedicine /
        // GeneticManipulation reduce the realised loss via
        // apply_catastrophe_resistance — the headline catastrophe-
        // resistance effect for healthcare-bearing civs.
        let base_frac = Real::from(DISEASE_POP_LOSS);
        let severity_frac = base_frac * disease_severity_factor(planet.biosphere);
        // Tolerance: disease originates at the densest claimed cell;
        // fall back to cell 0 if the civ has no per-cell cohorts so
        // the tolerance gate still reads from real per-cell state.
        let disease_cell = densest_claimed_cell(civ).map_or(0, |c| c as usize);
        let cell_conds =
            catastrophe_cell_conditions(state, planet, disease_cell, Real::ZERO, Real::ZERO);
        let frac = apply_resistance_and_dormancy(civ, species, severity_frac, cell_conds, tick);
        let center_frac = frac * Real::from_int(2);
        let neighbor_frac = frac;
        let grid_w = state.grid().width();
        let grid_h = state.grid().height();
        let lost = if let Some(origin) = densest_claimed_cell(civ) {
            apply_to_cell_and_neighbors(
                civ,
                grid_w,
                grid_h,
                origin,
                center_frac,
                neighbor_frac,
                true,
            )
        } else {
            // Fallback for civs without per-cell cohorts (legacy /
            // tests): apply uniform fraction to aggregate.
            let before = civ.cohort.total();
            let target = (before * (Real::ONE - frac)).max(Pop::from_int(10));
            civ.cohort.shrink_to(target)
        };
        let _ = lost;
        civ.last_disease_tick = Some(tick);
        civ.last_catastrophe_tick = Some(tick);
        // pivot toward communitarian + mystical (plague-cosmology pattern).
        let push = Cosmology {
            empirical: Real::ZERO,
            communitarian: Real::percent(15),
            reformist: -Real::percent(5),
            mystical: Real::percent(15),
            hierarchical: Real::percent(5),
        };
        civ.apply_cosmology_push(&push, Real::ONE);
        return Some(CatastropheRecord {
            kind: CatastropheKind::Disease,
            fraction_lost: frac,
        });
    }

    // Asteroid impact — rare, dramatic, hits hard. Gated only by
    // tick-based deterministic firing window + cooldown.
    let asteroid_ready = civ
        .last_asteroid_tick
        .is_none_or(|t| tick.saturating_sub(t) >= ASTEROID_COOLDOWN_TICKS);
    if asteroid_ready && asteroid_fires(tick) {
        // cell-targeted: deterministic impact site per
        // (tick, civ_id). Impact cell takes 2× the global
        // fraction; adjacent claimed cells take 0.5× (debris,
        // fires). If the civ has no claim, fall back to uniform
        // pop drop so a brand-new civ still feels the global
        // aftermath. : catastrophe-resistance tools soften
        // the absolute loss (built shelter survives debris).
        let raw_frac = Real::from(ASTEROID_POP_LOSS);
        // Tolerance: read the deterministic impact cell's conditions
        // for the tolerance gate. No extra rad/temp delta — asteroid
        // damage is kinetic and dust-driven, not radiation-driven, and
        // the cell's pre-impact state is the right baseline for the
        // surviving sub-population.
        let asteroid_cell = deterministic_cell_pick(civ, tick).map_or(0, |c| c as usize);
        let cell_conds =
            catastrophe_cell_conditions(state, planet, asteroid_cell, Real::ZERO, Real::ZERO);
        let frac = apply_resistance_and_dormancy(civ, species, raw_frac, cell_conds, tick);
        let center_frac = frac * Real::from_int(2);
        let neighbor_frac = frac / Real::from_int(2);
        let grid_w = state.grid().width();
        let grid_h = state.grid().height();
        let lost = if let Some(impact) = deterministic_cell_pick(civ, tick) {
            apply_to_cell_and_neighbors(
                civ,
                grid_w,
                grid_h,
                impact,
                center_frac,
                neighbor_frac,
                true,
            )
        } else {
            let before = civ.cohort.total();
            let target = (before * (Real::ONE - frac)).max(Pop::from_int(10));
            civ.cohort.shrink_to(target)
        };
        let _ = lost;
        // T2 — drain ecosystem biomass for every extant species,
        // tolerance-gated by the impact cell's conditions plus the
        // asteroid-specific radiation boost (prompt gamma +
        // activation products). Calibrated to the raw asteroid
        // loss fraction so the eco signature matches the headline
        // catastrophe severity; each eco species' own tolerance
        // envelope gates the realised loss (extremophiles with
        // wide radiation envelopes shrug it off).
        if let Some(eco) = ecosystem.as_deref_mut() {
            let eco_cell_conds = catastrophe_cell_conditions(
                state,
                planet,
                asteroid_cell,
                Real::ZERO,
                asteroid_radiation_boost(),
            );
            let (t, ph, sal, rad, p) = eco_cell_conds;
            eco.apply_catastrophe_at_cell(raw_frac, t, ph, sal, rad, p);
        }
        civ.last_asteroid_tick = Some(tick);
        civ.last_catastrophe_tick = Some(tick);
        // Asteroid pushes mystical strongly + reformist (rebuild
        // pressure) — civilization-shaking event.
        let push = Cosmology {
            empirical: -Real::percent(5),
            communitarian: Real::percent(10),
            reformist: Real::percent(15),
            mystical: Real::percent(20),
            hierarchical: -Real::percent(5),
        };
        civ.apply_cosmology_push(&push, Real::ONE);
        return Some(CatastropheRecord {
            kind: CatastropheKind::Asteroid,
            fraction_lost: frac,
        });
    }

    // Solar flare — gated on planet's stellar luminosity +
    // magnetosphere weakness. Hits modestly; pushes empirical
    // (the species observes the flare directly).
    let flare_ready = civ
        .last_solar_flare_tick
        .is_none_or(|t| tick.saturating_sub(t) >= SOLAR_FLARE_COOLDOWN_TICKS);
    if flare_ready && solar_flare_fires(planet, tick) {
        // catastrophe resistance softens the flare's hit
        // (advanced shielding / underground habitats / radiation
        // medicine).
        // Tolerance: solar flare boosts the cell's radiation flux by
        // the flare magnitude, modulated by the magnetic-reversal
        // cosmic-ray ground-flux multiplier (Item 20) — a flare
        // hitting during a reversal window pushes radiation-sensitive
        // species well past their `radiation_max`, while a flare
        // hitting under a strong stable dipole gets *attenuated* (the
        // shielded surface sees less of the radiation pulse). T8: the
        // raw flux is `1 / (dipole_strength + 0.1)`, so it spans
        // ~[0.0, 10.0] across the dipole envelope. Clamp to
        // `[0.2, 5.0]` — strong magnetospheres dampen by up to 5×
        // (down to 0.2 of nominal), weak/reversing fields amplify by
        // up to 5×. The 0.2 floor preserves a minimum flare effect
        // even on heavily shielded worlds (the flare's particle
        // spectrum isn't entirely magnetic-deflectable).
        let raw_frac = Real::from(SOLAR_FLARE_POP_LOSS);
        let cosmic_amp = state
            .cosmic_ray_ground_flux()
            .max(Real::from_ratio(2, 10))
            .min(Real::from_int(5));
        let rad_boost = solar_flare_radiation_boost() * cosmic_amp;
        let flare_cell = densest_claimed_cell(civ).map_or(0, |c| c as usize);
        let cell_conds =
            catastrophe_cell_conditions(state, planet, flare_cell, Real::ZERO, rad_boost);
        let frac = apply_resistance_and_dormancy(civ, species, raw_frac, cell_conds, tick);
        let before = civ.cohort.total();
        let target = (before * (Real::ONE - frac)).max(Pop::from_int(10));
        let _lost = civ.cohort.shrink_to(target);
        // T2 — drain ecosystem biomass for every extant species,
        // tolerance-gated by the cell's post-flare radiation flux
        // (already cosmic-ray-amplified above). Calibrated to the
        // raw flare loss fraction so the eco signature matches the
        // headline catastrophe severity; each eco species' own
        // tolerance envelope gates the realised loss (extremophiles
        // with `radiation_max ≥ 5` shrug it off, narrow-envelope
        // species take the full hit).
        if let Some(eco) = ecosystem.as_deref_mut() {
            let (t, ph, sal, rad, p) = cell_conds;
            eco.apply_catastrophe_at_cell(raw_frac, t, ph, sal, rad, p);
        }
        civ.last_solar_flare_tick = Some(tick);
        civ.last_catastrophe_tick = Some(tick);
        // Empirical + reformist (the species sees the sky's
        // role in their fate — drives observational science).
        let push = Cosmology {
            empirical: Real::percent(15),
            communitarian: Real::ZERO,
            reformist: Real::percent(10),
            mystical: Real::percent(5),
            hierarchical: Real::ZERO,
        };
        civ.apply_cosmology_push(&push, Real::ONE);
        return Some(CatastropheRecord {
            kind: CatastropheKind::SolarFlare,
            fraction_lost: frac,
        });
    }

    // Ice age — gated on cold-planet baseline + civ maturity.
    // Pushes communitarian (huddle-together) + hierarchical
    // (centralized resource management).
    let ice_ready = civ
        .last_ice_age_tick
        .is_none_or(|t| tick.saturating_sub(t) >= ICE_AGE_COOLDOWN_TICKS);
    if ice_ready && ice_age_fires(planet, civ, tick) {
        // severity scales with planet's mean temperature —
        // colder planets suffer worse ice ages. : catastrophe
        // resistance + cryogenic-engineering tools soften the loss.
        let base_frac = Real::from(ICE_AGE_POP_LOSS);
        let severity_frac =
            (base_frac * ice_age_severity_factor(planet.mean_temperature)).min(Real::percent(60));
        // Tolerance: ice age drops the cell's read-out temperature by
        // `ice_age_temp_drop_k` so the temperature gate fires for
        // narrow-envelope species. Picks the densest-claimed cell as
        // the representative reading.
        let ice_cell = densest_claimed_cell(civ).map_or(0, |c| c as usize);
        let temp_drop = Real::ZERO - ice_age_temp_drop_k();
        let cell_conds =
            catastrophe_cell_conditions(state, planet, ice_cell, temp_drop, Real::ZERO);
        let frac = apply_resistance_and_dormancy(civ, species, severity_frac, cell_conds, tick);
        let before = civ.cohort.total();
        let target = (before * (Real::ONE - frac)).max(Pop::from_int(10));
        let _lost = civ.cohort.shrink_to(target);
        // T2 — drain ecosystem biomass for every extant species,
        // tolerance-gated by the post-cold-snap cell temperature.
        // Calibrated to the climate-scaled severity (not the raw
        // ice-age constant) so the eco signature tracks how cold
        // the planet already runs — colder baseline ⇒ harsher
        // catastrophe. Each eco species' own tolerance envelope
        // gates the realised loss (cold-adapted species with wide
        // lower temp bounds survive; tropical species crash).
        if let Some(eco) = ecosystem.as_deref_mut() {
            let (t, ph, sal, rad, p) = cell_conds;
            eco.apply_catastrophe_at_cell(severity_frac, t, ph, sal, rad, p);
        }
        civ.last_ice_age_tick = Some(tick);
        civ.last_catastrophe_tick = Some(tick);
        let push = Cosmology {
            empirical: Real::ZERO,
            communitarian: Real::percent(20),
            reformist: -Real::percent(5),
            mystical: Real::percent(5),
            hierarchical: Real::percent(15),
        };
        civ.apply_cosmology_push(&push, Real::ONE);
        return Some(CatastropheRecord {
            kind: CatastropheKind::IceAge,
            fraction_lost: frac,
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_physics::HexGrid;
    use sim_recognition::RecognitionLibrary;
    use sim_world::{sample_planet, Magnetosphere};

    /// Default test species — `dormancy_capability = 0` so all
    /// existing pre-Sprint-2-Item-7b catastrophe assertions still
    /// hold (no damage-reduction multiplier). Dormancy-specific
    /// tests below construct their own species with explicit
    /// `dormancy_capability`.
    fn test_species() -> Species {
        let planet = sample_planet(1);
        let lib = RecognitionLibrary::earth_like_default();
        let mut s = sim_species::derive(&planet, &lib);
        s.dormancy_capability = Real::ZERO;
        s
    }

    fn species_with_dormancy(dormancy: Real) -> Species {
        let mut s = test_species();
        s.dormancy_capability = dormancy;
        s
    }

    fn empty_state() -> PhysicsState {
        PhysicsState::new(HexGrid::new(4, 4))
    }

    fn well_fed_state() -> PhysicsState {
        let mut s = PhysicsState::new(HexGrid::new(4, 4));
        for v in s.substance_mut(Substance::Fuel.idx()) {
            *v = Real::from_int(10);
        }
        s
    }

    /// Test fixture: a benign Earth-like planet that doesn't
    /// trigger any of the new gated catastrophes. Lets the
    /// existing volcanic/disease tests run unaffected.
    fn earth_like_planet() -> Planet {
        Planet {
            seed: 0,
            name: "TestPlanet".to_string(),
            // Earth-like mass/radius → derived gravity ≈ 9.81 m/s²
            // (Sprint 5 Item 21).
            mass: Real::ONE,
            radius: Real::ONE,
            composition: sim_world::Composition::Rocky,
            mean_temperature: Real::from_int(288),
            temperature_gradient: Real::from_int(20),
            terrain_peak: Real::from_int(8000),
            terrain_centre_q: 0,
            terrain_centre_r: 0,
            sea_level: Real::from_int(2000),
            atmosphere: sim_world::Atmosphere::Oxidising,
            atmospheric_composition: sim_world::AtmosphericComposition::vacuum(),
            biosphere_density: Real::from_ratio(3, 10),
            crustal_composition: sim_world::CrustalComposition::empty(),
            surface_pressure: Real::from_int(101_325),
            biosphere: sim_world::BiosphereClass::Lush,
            magnetosphere: Magnetosphere::Strong,
            crust: sim_world::Crust::Basaltic,
            stellar_luminosity: Real::from_int(1361),
            orbital_distance_au: Real::ONE,
            moon_count: 1,
            moons: vec![sim_world::Moon {
                mass_relative_x100: 100,
                orbital_period_macros: 28,
                inclination_deg_x10: 51,
                eccentricity: Real::ZERO,
            }],
            orbital_eccentricity_x100: 2,
            axial_tilt_deg: Real::from_int(23),
            day_length_hours: Real::from_int(24),
            orbital_period_months: 12,
            metabolic_substrate: sim_world::MetabolicSubstrate::Aqueous,
            substrate_perturbation: Real::ZERO,
            locking_state: sim_world::LockingState::FreeRotator,
            // Modern-Sun analog: G dwarf at ~45% through its 10 Gyr
            // MS lifetime. After P2.4's faint-young-sun correction,
            // `Star::new` lands at the *faint* ZAMS (0.70× = 953
            // W/m²); construct via `with_age` to keep this fixture
            // at the present-day Sun-on-Earth ~1361 W/m².
            star: sim_world::Star::with_age(
                sim_world::SpectralType::G,
                Real::from_int(1_361),
                Real::from_ratio(45, 10),
                Real::from_int(10),
            ),
        }
    }

    #[test]
    fn no_catastrophe_on_quiet_state() {
        let mut civ = Civ::new(1, 0, Pop::from_int(50));
        let mut state = well_fed_state();
        let r = check_and_apply(
            &mut civ,
            &mut state,
            &earth_like_planet(),
            &test_species(),
            100,
            None,
            );
        assert!(r.is_none());
    }

    #[test]
    fn volcanic_fires_on_extreme_signature() {
        let mut civ = Civ::new(1, 0, Pop::from_int(100));
        let mut state = well_fed_state();
        state.charge_mut()[0] = Real::from_int(120);
        state.temperature_mut()[0] = Real::from_int(700);
        let r = check_and_apply(
            &mut civ,
            &mut state,
            &earth_like_planet(),
            &test_species(),
            50,
            None,
            );
        let rec = r.expect("volcanic should fire");
        assert_eq!(rec.kind, CatastropheKind::Volcanic);
        // Cell 0 fuel reset, temperature dropped, civ pop dropped.
        assert_eq!(state.substance(Substance::Fuel.idx())[0], Real::ZERO);
        assert!(state.temperature()[0] < Real::from_int(700));
        assert!(civ.cohort.total() < Pop::from_int(100));
        assert_eq!(civ.last_volcanic_tick, Some(50));
        assert_eq!(civ.last_catastrophe_tick, Some(50));
    }

    #[test]
    fn volcanic_respects_cooldown() {
        // cooldown lengths derive from VOLCANIC_COOLDOWN_TICKS
        // so the test stays correct as the constant scales.
        let mut civ = Civ::new(1, 0, Pop::from_int(100));
        let mut state = well_fed_state();
        state.charge_mut()[0] = Real::from_int(120);
        state.temperature_mut()[0] = Real::from_int(700);
        let sp = test_species();
        check_and_apply(&mut civ, &mut state, &earth_like_planet(), &sp, 0, None);
        // Re-set the trigger (in case the apply zeroed something).
        state.charge_mut()[0] = Real::from_int(120);
        state.temperature_mut()[0] = Real::from_int(700);
        // Halfway through cooldown — still inside.
        let r = check_and_apply(
            &mut civ,
            &mut state,
            &earth_like_planet(),
            &sp,
            VOLCANIC_COOLDOWN_TICKS / 2,
            None,
            );
        assert!(r.is_none());
        // Past cooldown.
        state.charge_mut()[0] = Real::from_int(120);
        state.temperature_mut()[0] = Real::from_int(700);
        let r = check_and_apply(
            &mut civ,
            &mut state,
            &earth_like_planet(),
            &sp,
            VOLCANIC_COOLDOWN_TICKS + 50,
            None,
            );
        assert!(r.is_some());
    }

    #[test]
    fn disease_fires_under_crowding_after_age_floor() {
        let mut civ = Civ::new(1, 0, Pop::from_int(50));
        let mut state = empty_state();
        // P0.5 — capacity now reads `civ.producer_biomass` rather
        // than `Substance::Fuel`. Calibration mirrors the legacy
        // fuel-tuned setup: producer_biomass = 0.001 × claimed_frac
        // (1.0 for empty claim) × per_unit (50_000) = 50, matching
        // civ pop so crowding = 1.0.
        state.substance_mut(Substance::Fuel.idx())[0] = Real::from_ratio(1, 1000);
        civ.producer_biomass = Real::from_ratio(1, 1000);
        let r = check_and_apply(
            &mut civ,
            &mut state,
            &earth_like_planet(),
            &test_species(),
            DISEASE_AGE_FLOOR_TICKS,
            None,
            );
        let rec = r.expect("disease should fire");
        assert_eq!(rec.kind, CatastropheKind::Disease);
        assert!(civ.cohort.total() < Pop::from_int(50));
        // Cosmology pivoted.
        assert!(civ.cosmology.mystical > Real::ZERO);
    }

    #[test]
    fn disease_blocked_before_age_floor() {
        let mut civ = Civ::new(1, 0, Pop::from_int(50));
        let mut state = empty_state();
        state.substance_mut(Substance::Fuel.idx())[0] = Real::from_ratio(1, 1000);
        civ.producer_biomass = Real::from_ratio(1, 1000);
        let r = check_and_apply(
            &mut civ,
            &mut state,
            &earth_like_planet(),
            &test_species(),
            DISEASE_AGE_FLOOR_TICKS - 1,
            None,
            );
        assert!(r.is_none());
    }

    /// Sprint 2 Item 7b spec test #1.
    ///
    /// Species with `dormancy = 0.9` takes ~10× less damage than
    /// `dormancy = 0` from the same catastrophe. We exercise the
    /// disease pathway because its fixture is the most stable:
    /// known-firing trigger, no per-civ-shelter to confound the
    /// loss math. The two civs are identical apart from the
    /// species' dormancy trait.
    #[test]
    fn dormant_species_survives_catastrophe_at_reduced_rate() {
        let baseline_pop = Pop::from_int(50);
        let dormancy_high = Real::percent(90);

        // Baseline run — dormancy = 0.
        let mut civ_low = Civ::new(1, 0, baseline_pop);
        let mut state_low = empty_state();
        state_low.substance_mut(Substance::Fuel.idx())[0] = Real::from_ratio(1, 1000);
        // P0.5 — match the disease trigger's `civ.producer_biomass`
        // crowding calibration so the test still drives crowding to 1.0.
        civ_low.producer_biomass = Real::from_ratio(1, 1000);
        let rec_low = check_and_apply(
            &mut civ_low,
            &mut state_low,
            &earth_like_planet(),
            &species_with_dormancy(Real::ZERO),
            DISEASE_AGE_FLOOR_TICKS,
            None,
            )
        .expect("baseline disease should fire");

        // Dormant run — dormancy = 0.9, otherwise identical.
        let mut civ_high = Civ::new(1, 0, baseline_pop);
        let mut state_high = empty_state();
        state_high.substance_mut(Substance::Fuel.idx())[0] = Real::from_ratio(1, 1000);
        civ_high.producer_biomass = Real::from_ratio(1, 1000);
        let rec_high = check_and_apply(
            &mut civ_high,
            &mut state_high,
            &earth_like_planet(),
            &species_with_dormancy(dormancy_high),
            DISEASE_AGE_FLOOR_TICKS,
            None,
            )
        .expect("dormant disease should fire");

        // Both should be the same `kind` (disease) — the dormancy
        // multiplier only shrinks fraction_lost, not the trigger.
        assert_eq!(rec_low.kind, CatastropheKind::Disease);
        assert_eq!(rec_high.kind, CatastropheKind::Disease);

        // Effective fraction should be ~10× smaller. Allow a small
        // tolerance because both also pass through
        // `apply_catastrophe_resistance` (which is 1.0 at zero
        // tools) — the ratio is exactly
        // `(1 - 0.9 × 1.0) / (1 - 0 × 1.0) = 0.10`.
        let ratio = rec_high.fraction_lost / rec_low.fraction_lost;
        // ~0.10 ± 1% — Q32.32 is exact for these magnitudes; the
        // tolerance only protects against incidental future
        // resistance bumps that future code paths might apply
        // uniformly to both.
        assert!(
            ratio <= Real::percent(11) && ratio >= Real::percent(9),
            "expected ~0.10× damage with dormancy=0.9, got ratio={ratio:?}",
        );
    }

    /// Flare-firing planet: weak magnetosphere + above-Earth
    /// luminosity satisfy `solar_flare_fires`. Tick used by the
    /// flare tests below: `1567 * MONTHS_PER_YEAR = 18804`.
    fn flare_planet() -> Planet {
        let mut p = earth_like_planet();
        p.magnetosphere = Magnetosphere::Weak;
        p.stellar_luminosity = Real::from_int(1_500);
        p
    }

    /// Extremophile tolerance: radiation-tolerant envelope.
    /// `radiation_max = 20` so the post-flare radiation flux (≈ 1.1)
    /// still scores well inside the envelope. Other axes centred on
    /// the test cell's conditions (T=300 K, pH=7, salinity=20 g/L,
    /// p=1 atm) with margins that score above the radiation axis's
    /// fit so the radiation gate (not an incidental other axis) is
    /// the binding constraint on `match_score`.
    fn extremophile_tolerance() -> sim_species::ToleranceEnvelope {
        sim_species::ToleranceEnvelope {
            temp_range: (Real::from_int(200), Real::from_int(400)),
            ph_range: (Real::from_int(5), Real::from_int(9)),
            salinity_range: (Real::from_int(10), Real::from_int(30)),
            radiation_max: Real::from_int(20),
            pressure_range: (Real::from_ratio(5, 10), Real::from_ratio(15, 10)),
        }
    }

    /// P0.4 acceptance test: same solar flare, two species
    /// differing only in their `ToleranceEnvelope`. Extremophile
    /// (`radiation_max = 20`) survives at >> 3× the rate of an
    /// aqueous-default (`radiation_max = 0.5`) species — measured
    /// as the death-rate ratio (the only metric capable of
    /// resolving the spec target from a 10% flat base loss).
    #[test]
    fn extremophile_species_survives_solar_flare_better_than_aqueous() {
        // Big civ so the 10-pop floor doesn't dominate.
        let initial_pop = Pop::from_int(1_000_000);
        let planet = flare_planet();
        let flare_tick = 1567 * protocol::MONTHS_PER_YEAR;

        // Aqueous species — default narrow envelope, radiation_max
        // = 0.5. Flare rad = 0.1 + 1.0 = 1.1 ⇒ rad_score = 0 ⇒
        // match_score = 0 ⇒ full 10% loss.
        let mut aqueous = test_species();
        aqueous.tolerance = sim_species::ToleranceEnvelope::aqueous_default();
        let mut civ_aq = Civ::new(1, 0, initial_pop);
        // P0.5 — set producer biomass high enough that the disease
        // trigger (crowding ≥ 0.8 of capacity) doesn't preempt the
        // solar-flare path. Cap = producer_biomass × claimed_frac
        // (1.0 for empty claim) × per_unit (50_000) so
        // `producer_biomass = 100` yields cap = 5M, well above the
        // 1M civ pop ⇒ crowding 0.2 ⇒ no disease.
        civ_aq.producer_biomass = Real::from_int(100);
        let mut state_aq = well_fed_state();
        // Pin cell 0 to centre-of-aqueous-envelope T/p so the
        // non-radiation axes don't accidentally bottleneck the
        // aqueous species's match_score below the radiation gate.
        state_aq.temperature_mut()[0] = Real::from_int(300);
        state_aq.pressure_mut()[0] = Real::from_int(101_325);
        let rec_aq = check_and_apply(&mut civ_aq, &mut state_aq, &planet, &aqueous, flare_tick, None)
            .expect("flare must fire on weak-magnetosphere planet at tick=18804");
        assert_eq!(rec_aq.kind, CatastropheKind::SolarFlare);

        // Extremophile — wide envelope, radiation_max = 20. Flare
        // rad = 1.1 ⇒ rad_score = 1 - 1.1/20 ≈ 0.945 ⇒ match_score
        // ≈ 0.945 ⇒ loss = 0.10 × 0.055 ≈ 0.0055.
        let mut extremophile = test_species();
        extremophile.tolerance = extremophile_tolerance();
        let mut civ_ex = Civ::new(2, 0, initial_pop);
        // P0.5 — same producer-biomass override as the aqueous civ
        // so the disease trigger doesn't preempt the flare path.
        civ_ex.producer_biomass = Real::from_int(100);
        let mut state_ex = well_fed_state();
        // Same cell-conditions setup as the aqueous run — only the
        // species' tolerance envelope differs between the two civs.
        state_ex.temperature_mut()[0] = Real::from_int(300);
        state_ex.pressure_mut()[0] = Real::from_int(101_325);
        let rec_ex = check_and_apply(
            &mut civ_ex,
            &mut state_ex,
            &planet,
            &extremophile,
            flare_tick,
            None,
            )
        .expect("flare must fire for extremophile under same conditions");
        assert_eq!(rec_ex.kind, CatastropheKind::SolarFlare);

        // The extremophile's loss fraction must be at least 3× smaller
        // than the aqueous species' — measured as the loss ratio, the
        // only metric that resolves the spec target from a 10% flat
        // base loss. In practice it's ~18× smaller (0.10 vs ~0.0055).
        assert!(
            rec_aq.fraction_lost >= rec_ex.fraction_lost * Real::from_int(3),
            "expected aqueous loss >= 3× extremophile loss; aqueous={:?}, extremophile={:?}",
            rec_aq.fraction_lost,
            rec_ex.fraction_lost,
        );
        // And the extremophile's surviving population is strictly
        // larger than the aqueous one — the headline observable.
        assert!(
            civ_ex.cohort.total() > civ_aq.cohort.total(),
            "extremophile survivors must exceed aqueous survivors; ex={:?}, aq={:?}",
            civ_ex.cohort.total(),
            civ_aq.cohort.total(),
        );
    }

    /// Synthetic test: a species whose envelope sits entirely
    /// outside the catastrophe cell (`match_score = 0`) takes the
    /// full `raw_frac` loss after the resistance + dormancy stack
    /// (both no-ops here ⇒ identity). Exercises the
    /// `apply_resistance_and_dormancy` formula directly to isolate
    /// the tolerance term from per-catastrophe trigger plumbing.
    #[test]
    fn tolerance_match_score_zero_means_full_damage() {
        let mut civ = Civ::new(1, 0, Pop::from_int(100));
        let mut species = test_species();
        // Envelope nowhere near the cell (temp 100-101 K, etc.).
        species.tolerance = sim_species::ToleranceEnvelope {
            temp_range: (Real::from_int(100), Real::from_int(101)),
            ph_range: (Real::from_int(1), Real::from_int(2)),
            salinity_range: (Real::from_int(900), Real::from_int(1_000)),
            radiation_max: Real::from_ratio(1, 1_000),
            pressure_range: (Real::from_int(50), Real::from_int(51)),
        };
        species.dormancy_capability = Real::ZERO;
        // Cell sits outside on every axis — match_score = 0.
        let cell = (
            Real::from_int(300), // T
            Real::from_int(7),   // pH
            Real::from_int(20),  // salinity
            Real::ONE,           // rad (above radiation_max)
            Real::ONE,           // pressure
        );
        let raw = Real::percent(40);
        let out = apply_resistance_and_dormancy(&mut civ, &species, raw, cell, 0);
        // No tools, no dormancy, match_score = 0 ⇒ out == raw exactly.
        assert_eq!(out, raw, "expected full raw_frac loss when match_score = 0");
    }

    /// Synthetic test: a species whose envelope perfectly contains
    /// the cell at its centre (`match_score = 1`) takes ~zero
    /// catastrophe damage. Mirrors the formula's "perfect fit ⇒
    /// no damage" guarantee.
    #[test]
    fn tolerance_match_score_one_means_no_damage() {
        let mut civ = Civ::new(1, 0, Pop::from_int(100));
        let mut species = test_species();
        // Cell at the exact centre of every axis.
        let t_centre = Real::from_int(300);
        let ph_centre = Real::from_int(7);
        let sal_centre = Real::from_int(20);
        let rad_zero = Real::ZERO;
        let p_centre = Real::ONE;
        let half = Real::ONE;
        species.tolerance = sim_species::ToleranceEnvelope {
            temp_range: (t_centre - half, t_centre + half),
            ph_range: (ph_centre - half, ph_centre + half),
            salinity_range: (sal_centre - half, sal_centre + half),
            // Any positive ceiling works — radiation_score returns
            // 1.0 when `rad <= 0`.
            radiation_max: Real::ONE,
            pressure_range: (p_centre - half, p_centre + half),
        };
        species.dormancy_capability = Real::ZERO;
        let cell = (t_centre, ph_centre, sal_centre, rad_zero, p_centre);
        let raw = Real::percent(40);
        let out = apply_resistance_and_dormancy(&mut civ, &species, raw, cell, 0);
        // Perfect centre on every axis ⇒ match_score = 1 ⇒ loss = 0.
        assert_eq!(
            out,
            Real::ZERO,
            "expected zero loss for centre-of-envelope species",
        );
    }

    /// P1.3 acceptance test #1: a high-dormancy species' dormant
    /// pool gets seeded with the would-be casualties from a
    /// catastrophe. `dormancy = 0.9`, disease pathway, match_score
    /// = 0 (envelope nowhere near the cell — full base damage) →
    /// dormant pool must be populated with the surviving-but-
    /// dormant fraction.
    #[test]
    fn catastrophe_seeds_dormant_pool_for_tardigrade_species() {
        let baseline_pop = Pop::from_int(1_000_000);
        let mut civ = Civ::new(1, 0, baseline_pop);
        // P0.5 — set producer biomass so the disease trigger
        // doesn't preempt the path (crowding ≥ 0.8 of capacity).
        civ.producer_biomass = Real::from_int(100);
        let mut state = well_fed_state();
        // Pin cell 0 at centre-of-aqueous-envelope T/p so the
        // tolerance-gate fall-through doesn't accidentally zero
        // out the dormant seeding via match_score = 1.
        state.temperature_mut()[0] = Real::from_int(300);
        state.pressure_mut()[0] = Real::from_int(101_325);
        // Force-fire disease at the age floor.
        state.substance_mut(Substance::Fuel.idx())[0] = Real::from_ratio(1, 1000);
        civ.producer_biomass = Real::from_ratio(1, 1000);
        let species = species_with_dormancy(Real::percent(90));
        let rec = check_and_apply(
            &mut civ,
            &mut state,
            &earth_like_planet(),
            &species,
            DISEASE_AGE_FLOOR_TICKS,
            None,
            )
        .expect("disease must fire for tardigrade species");
        assert_eq!(rec.kind, CatastropheKind::Disease);
        // Headline P1.3 assertion: the dormant pool now holds
        // a non-zero surviving-but-dormant reservoir.
        assert!(
            civ.dormant_pool.population > Real::ZERO,
            "dormant pool must be seeded for dormancy=0.9 species, got {:?}",
            civ.dormant_pool.population,
        );
        // `entered_tick` should match the catastrophe tick so the
        // post-run telemetry can locate the cryptobiosis event.
        assert_eq!(civ.dormant_pool.entered_tick, DISEASE_AGE_FLOOR_TICKS);
    }

    /// P1.3 acceptance test #2: mass-extinction recovery via
    /// seed-bank resurrection. Apply a 100% catastrophe (everyone
    /// dies) to a `dormancy = 0.9` species, then run the
    /// per-tick resurrection step for 500 ticks. Active
    /// population must recover to ≥ 99% of pre-event. Drives the
    /// formula directly (no per-catastrophe firing machinery) so
    /// the test isolates the dormant-pool seeding + resurrection
    /// path from the catastrophe-trigger plumbing.
    #[test]
    fn mass_extinction_recovery_via_seed_bank_resurrection() {
        let pre_event = Pop::from_int(1_000);
        let mut civ = Civ::new(1, 0, pre_event);
        let mut species = test_species();
        species.dormancy_capability = Real::percent(90);
        // Cell outside the species' envelope on every axis →
        // match_score = 0 → full base damage. Combined with
        // raw = 1.0 the would-be loss is 100% of pop.
        species.tolerance = sim_species::ToleranceEnvelope {
            temp_range: (Real::from_int(100), Real::from_int(101)),
            ph_range: (Real::from_int(1), Real::from_int(2)),
            salinity_range: (Real::from_int(900), Real::from_int(1_000)),
            radiation_max: Real::from_ratio(1, 1_000),
            pressure_range: (Real::from_int(50), Real::from_int(51)),
        };
        let cell = (
            Real::from_int(300),
            Real::from_int(7),
            Real::from_int(20),
            Real::ONE,
            Real::ONE,
        );
        // raw = 1.0 — the catastrophe wants to kill everyone.
        let frac = apply_resistance_and_dormancy(&mut civ, &species, Real::ONE, cell, 100);
        // Realised loss ≈ 10% (1 − 0.9 × 1 = 0.1) so the active
        // cohort survives at ≈ 10%. Apply the shrink_to to match
        // what `check_and_apply` does at the call site.
        let target = (civ.cohort.total() * (Real::ONE - frac)).max(Pop::ZERO);
        civ.cohort.shrink_to(target);
        // Dormant pool now holds ≈ 90% × pre_event = 900.
        let expected_dormant = pre_event.to_real_nonneg() * Real::percent(90);
        let tol = Real::from_int(2);
        assert!(
            (civ.dormant_pool.population - expected_dormant).abs() <= tol,
            "expected ≈ 900 dormant, got {:?}",
            civ.dormant_pool.population,
        );
        // Run 500 ticks of resurrection at 1%/tick. Cap on the
        // pre-event population — never exceeded by construction.
        let target = pre_event.to_real_nonneg();
        for _ in 0..500 {
            let mut active = civ.cohort.total().to_real_nonneg();
            let revived = civ.dormant_pool.resurrect_step(&mut active, target);
            // Deposit the revived headcount back into the cohort
            // (fertile bracket; matches the "add to fertile"
            // distribution policy the spec mentions).
            civ.cohort.add_fertile(Pop::from_real(revived));
        }
        // Active population must be ≥ 99% of pre-event.
        let bound = pre_event.to_real_nonneg() * Real::percent(99);
        let active_now = civ.cohort.total().to_real_nonneg();
        assert!(
            active_now >= bound,
            "seed-bank recovery failed: active={active_now:?} bound={bound:?}",
        );
    }

    /// T2 acceptance test #1: non-volcanic catastrophes now strip
    /// the ecosystem's trophic-web biomass too. Pre-T2, only
    /// volcanic touched the ecosystem; asteroid / solar flare /
    /// ice age / disease left the eco pool untouched. T2 wires
    /// asteroid / solar flare / ice age into
    /// `apply_catastrophe_at_cell` (disease stays biology-internal
    /// per spec). This test fires an asteroid and asserts every
    /// extant eco species' biomass drops.
    #[test]
    fn non_volcanic_catastrophes_now_affect_ecosystem() {
        // Asteroid tick must satisfy `tick.is_multiple_of(4733 *
        // MONTHS_PER_YEAR)` AND tick > 0.
        let asteroid_tick = 4733 * protocol::MONTHS_PER_YEAR;
        let initial_pop = Pop::from_int(1_000_000);
        let mut civ = Civ::new(1, 0, initial_pop);
        // P0.5 — keep crowding low enough that disease doesn't
        // preempt the asteroid path. With no claimed cells the
        // claimed_cell_fraction defaults to 1.0; producer_biomass
        // = 100 × per_unit (50_000) = 5M capacity ≫ 1M civ pop, so
        // crowding ≈ 0.2 and disease stays dormant. Asteroid's
        // deterministic_cell_pick returns None on an empty-claim
        // civ and falls back to cell 0 for the conditions read,
        // so the eco call still fires.
        civ.producer_biomass = Real::from_int(100);
        let mut state = well_fed_state();
        // Pin cell 0 at centre-of-aqueous-envelope conditions so
        // tolerance reads from a real cell state.
        state.temperature_mut()[0] = Real::from_int(300);
        state.pressure_mut()[0] = Real::from_int(101_325);

        // Build a small eco fixture with a single producer at
        // narrow aqueous tolerance — the asteroid's
        // `rad += asteroid_radiation_boost (= 5.0)` blows past the
        // aqueous `radiation_max = 0.5`, driving match_score to 0
        // and exposing the species to the full headline loss.
        let mut eco = sim_ecosystem::sample_ecosystem_with_substrate_for_grid(
            42,
            "aqueous",
            Real::from_int(10_000),
            state.grid().n_cells(),
            None,
        );
        // Baseline biomass for all extant species pre-catastrophe.
        let before: std::collections::BTreeMap<sim_species::SpeciesId, Real> = eco
            .species
            .iter()
            .filter_map(|(id, s)| if s.is_extant { Some((*id, s.biomass)) } else { None })
            .collect();
        assert!(
            !before.is_empty(),
            "eco fixture must seed at least one extant species",
        );

        let rec = check_and_apply(
            &mut civ,
            &mut state,
            &earth_like_planet(),
            &test_species(),
            asteroid_tick,
            Some(&mut eco),
        )
        .expect("asteroid must fire at tick = 4733 * MONTHS_PER_YEAR");
        assert_eq!(rec.kind, CatastropheKind::Asteroid);

        // At least one extant species must show a strict biomass
        // drop — the headline T2 observable. We expect *every*
        // species at narrow aqueous tolerance to lose biomass since
        // rad = baseline (0.1) + 5.0 = 5.1 >> radiation_max = 0.5.
        let mut any_dropped = false;
        for (id, b0) in &before {
            let b1 = eco.species.get(id).map(|s| s.biomass).unwrap_or(Real::ZERO);
            if b1 < *b0 {
                any_dropped = true;
            }
            // No species should *gain* biomass from the catastrophe.
            assert!(
                b1 <= *b0,
                "species {:?} biomass increased through asteroid: \
                 before={:?}, after={:?}",
                id,
                b0,
                b1,
            );
        }
        assert!(
            any_dropped,
            "no eco species lost biomass through asteroid — T2 wiring missing?",
        );
    }

    /// T2 acceptance test #2: ecosystem analog of the civ-side P0.4
    /// flare test. Two eco species, identical except for their
    /// `ToleranceEnvelope`. The extremophile (wide radiation
    /// envelope) survives the same flare with strictly more
    /// biomass than the aqueous-default narrow-envelope species.
    /// Mirrors `extremophile_species_survives_solar_flare_better_than_aqueous`
    /// on the eco side now that solar flare is wired into
    /// `apply_catastrophe_at_cell`.
    #[test]
    fn extremophile_eco_species_survives_solar_flare_better() {
        use sim_ecosystem::EcoSpecies;
        use sim_species::{
            EcosystemRole, Habitat, InteractionMatrix, ProducerMetabolism, SpeciesId,
            ToleranceEnvelope,
        };

        let planet = flare_planet();
        let flare_tick = 1567 * protocol::MONTHS_PER_YEAR;

        let initial_pop = Pop::from_int(1_000_000);
        let mut civ = Civ::new(1, 0, initial_pop);
        // P0.5 — bump producer biomass so the disease trigger
        // doesn't preempt the flare path.
        civ.producer_biomass = Real::from_int(100);
        let mut state = well_fed_state();
        // Pin cell 0 at centre-of-aqueous-envelope T/p so non-rad
        // axes are non-binding (only the radiation axis differs
        // between the two eco species).
        state.temperature_mut()[0] = Real::from_int(300);
        state.pressure_mut()[0] = Real::from_int(101_325);

        let starting_biomass = Real::from_int(1_000);
        let aqueous_id = SpeciesId(101);
        let extremo_id = SpeciesId(102);
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
        let extremophile_tol = ToleranceEnvelope {
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
            tolerance: extremophile_tol,
        };
        let mut eco = sim_ecosystem::PlanetEcosystem::new(
            vec![aqueous, extremophile],
            InteractionMatrix::new(),
            Real::from_int(10_000),
        );

        let rec = check_and_apply(
            &mut civ,
            &mut state,
            &planet,
            &test_species(),
            flare_tick,
            Some(&mut eco),
        )
        .expect("flare must fire on weak-magnetosphere planet at tick = 1567 * MONTHS_PER_YEAR");
        assert_eq!(rec.kind, CatastropheKind::SolarFlare);

        let aq_after = eco.species.get(&aqueous_id).unwrap().biomass;
        let ex_after = eco.species.get(&extremo_id).unwrap().biomass;
        let aq_loss = starting_biomass - aq_after;
        let ex_loss = starting_biomass - ex_after;
        // Aqueous species must take real damage (rad = 0.1 + 1.0
        // = 1.1 > radiation_max = 0.5 ⇒ match_score = 0).
        assert!(
            aq_loss > Real::ZERO,
            "aqueous eco species took no damage: after={aq_after:?}",
        );
        // The extremophile's biomass loss must be at least 3×
        // smaller than the aqueous species' — mirrors the civ-side
        // P0.4 acceptance bound. In practice it's ~18× smaller.
        assert!(
            aq_loss >= ex_loss * Real::from_int(3),
            "expected aqueous loss >= 3× extremophile loss; \
             aqueous_loss={aq_loss:?}, extremo_loss={ex_loss:?}",
        );
        // Headline observable: more extremophile biomass survives.
        assert!(
            ex_after > aq_after,
            "extremophile survivors must exceed aqueous survivors; \
             ex_after={ex_after:?}, aq_after={aq_after:?}",
        );
    }

    /// T8 — strong magnetosphere dampens solar-flare damage. Two runs
    /// of the same flare-firing planet (Weak magnetosphere class +
    /// above-Earth luminosity, so `solar_flare_fires` triggers) at
    /// the same tick, differing only in the physics-state
    /// `dipole_strength`:
    ///
    /// * `dipole_strength = 10.0` → `cosmic_ray_ground_flux ≈ 0.099`
    ///   → clamps to `0.2` → flare radiation is *attenuated*.
    /// * `dipole_strength = 0.1` → `cosmic_ray_ground_flux ≈ 5.0`
    ///   → clamps to `5.0` → flare radiation is *amplified* 5×.
    ///
    /// Same aqueous species (narrow radiation envelope) so the
    /// tolerance gate registers the radiation differential as a
    /// `fraction_lost` differential. Strong-dipole loss must be
    /// strictly less than weak/reversing-dipole loss.
    #[test]
    fn strong_magnetosphere_suppresses_flare_damage() {
        // Big civ so the 10-pop floor on `target` doesn't dominate.
        let initial_pop = Pop::from_int(1_000_000);
        let planet = flare_planet();
        let flare_tick = 1567 * protocol::MONTHS_PER_YEAR;

        // Aqueous species — narrow radiation envelope so the cosmic-
        // amp differential bites on `match_score`.
        let mut species = test_species();
        species.tolerance = sim_species::ToleranceEnvelope::aqueous_default();

        // ------------- Strong-dipole run (T8 suppression) -------------
        let mut civ_strong = Civ::new(1, 0, initial_pop);
        // Producer biomass override so the disease trigger doesn't
        // preempt the flare path (matches the extremophile test).
        civ_strong.producer_biomass = Real::from_int(100);
        let mut state_strong = well_fed_state();
        // Centre cell 0 on aqueous-envelope T/p so non-radiation
        // axes don't accidentally bottleneck match_score.
        state_strong.temperature_mut()[0] = Real::from_int(300);
        state_strong.pressure_mut()[0] = Real::from_int(101_325);
        // Strong dipole → flux ≈ 0.099 → clamps to 0.2 (5× suppression).
        *state_strong.dipole_strength_mut() = Real::from_int(10);
        let rec_strong = check_and_apply(
            &mut civ_strong,
            &mut state_strong,
            &planet,
            &species,
            flare_tick,
            None,
        )
        .expect("flare must fire on weak-magnetosphere-class planet at tick=18804");
        assert_eq!(rec_strong.kind, CatastropheKind::SolarFlare);

        // ------------- Reversal-window run (max amplification) --------
        let mut civ_reversal = Civ::new(2, 0, initial_pop);
        civ_reversal.producer_biomass = Real::from_int(100);
        let mut state_reversal = well_fed_state();
        state_reversal.temperature_mut()[0] = Real::from_int(300);
        state_reversal.pressure_mut()[0] = Real::from_int(101_325);
        // Deep-reversal dipole → flux ≈ 5.0 → clamps to 5.0 (5× amp).
        *state_reversal.dipole_strength_mut() = Real::from_ratio(1, 10);
        let rec_reversal = check_and_apply(
            &mut civ_reversal,
            &mut state_reversal,
            &planet,
            &species,
            flare_tick,
            None,
        )
        .expect("flare must fire under reversal-window dipole too");
        assert_eq!(rec_reversal.kind, CatastropheKind::SolarFlare);

        // The strong-magnetosphere run must take strictly less damage
        // than the reversal-window run — the headline T8 observable.
        assert!(
            rec_strong.fraction_lost < rec_reversal.fraction_lost,
            "T8: strong-dipole flare damage ({:?}) must be < reversal-window damage ({:?})",
            rec_strong.fraction_lost,
            rec_reversal.fraction_lost,
        );
        // And the surviving population must be strictly larger under
        // the strong dipole.
        assert!(
            civ_strong.cohort.total() > civ_reversal.cohort.total(),
            "T8: strong-dipole survivors must exceed reversal survivors; \
             strong={:?}, reversal={:?}",
            civ_strong.cohort.total(),
            civ_reversal.cohort.total(),
        );
    }
}
