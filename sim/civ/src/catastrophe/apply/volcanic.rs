//! Volcanic-catastrophe handler: cell-targeted eruption that
//! zeroes the cell's fuel, drops cell temperature 50 K, and
//! drains the eruption cell's regional cohort + every extant
//! ecosystem species' biomass at that cell. Volcanic-cooldown
//! scales with crust composition (`volcanic_cooldown_factor`),
//! so basaltic baseline + hydrocarbon shorter + older crusts
//! longer.

use crate::Civ;
use sim_arith::{Pop, Real};
use sim_ecosystem::PlanetEcosystem;
use sim_physics::{PhysicsState, Substance};
use sim_species::Species;
use sim_world::Planet;

use super::super::damage::{apply_resistance_and_dormancy, catastrophe_cell_conditions};
use super::super::factors::volcanic_cooldown_factor;
use super::super::kind::CatastropheKind;
use super::super::record::CatastropheRecord;
use super::super::triggers::volcanic_fires;
use super::super::{VOLCANIC_COOLDOWN_TICKS, VOLCANIC_POP_LOSS};

/// Try to fire the volcanic catastrophe this tick. Returns
/// `Some(record)` if it fires (and mutates `civ`, `state`,
/// optional `ecosystem`); `None` otherwise.
///
/// Cooldown handling: caller has not yet committed any
/// catastrophe this tick, so we own the cooldown check here.
pub(super) fn try_apply(
    civ: &mut Civ,
    state: &mut PhysicsState,
    planet: &Planet,
    species: &Species,
    tick: u64,
    ecosystem: &mut Option<&mut PlanetEcosystem>,
) -> Option<CatastropheRecord> {
    // volcanic cooldown scales with crust — Basaltic
    // baseline, Hydrocarbon shorter (more frequent), older crusts
    // longer. Computed in Q32.32 then converted back to ticks.
    //
    // Planet-scale realism: divide the crust-adjusted cooldown by
    // the planet's surface area factor (`radius²`) so a bigger
    // world's regions churn proportionally more — more volcanic
    // events per century. Earth-radius (1.0) is a no-op.
    let volcanic_factor = volcanic_cooldown_factor(planet.crust);
    let area_factor = planet.radius * planet.radius;
    let scaled_cooldown_real =
        Real::from_int(i64::try_from(VOLCANIC_COOLDOWN_TICKS).unwrap_or(i64::MAX))
            * volcanic_factor
            / area_factor.max(Real::percent(1));
    let scaled_volcanic_cooldown: u64 =
        u64::try_from(scaled_cooldown_real.raw().to_num::<i64>().max(1))
            .unwrap_or(VOLCANIC_COOLDOWN_TICKS);
    let volcanic_ready = civ
        .last_volcanic_tick
        .is_none_or(|t| tick.saturating_sub(t) >= scaled_volcanic_cooldown);
    if !volcanic_ready {
        return None;
    }
    let cell = volcanic_fires(state)?;
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
    let cell_conds = catastrophe_cell_conditions(state, planet, cell, Real::ZERO, Real::ZERO);
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
    Some(CatastropheRecord {
        kind: CatastropheKind::Volcanic,
        fraction_lost: frac,
    })
}

#[cfg(test)]
mod tests {
    use super::super::check_and_apply;
    use super::super::test_helpers::*;
    use super::super::super::kind::CatastropheKind;
    use super::super::super::VOLCANIC_COOLDOWN_TICKS;
    use crate::Civ;
    use sim_arith::{Pop, Real};
    use sim_physics::Substance;

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
}
