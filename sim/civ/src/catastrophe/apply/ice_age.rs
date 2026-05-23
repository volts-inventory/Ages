//! Ice-age handler: gated on cold-planet baseline + civ
//! maturity. Severity scales with planet's mean temperature
//! (colder planets suffer worse ice ages). Pushes communitarian
//! (huddle-together) + hierarchical (centralized resource
//! management). Ice-age drops the cell's read-out temperature by
//! `ice_age_temp_drop_k` so the temperature gate fires for
//! narrow-envelope species.

use crate::cosmology::Cosmology;
use crate::Civ;
use sim_arith::{Pop, Real};
use sim_ecosystem::PlanetEcosystem;
use sim_physics::PhysicsState;
use sim_species::Species;
use sim_world::Planet;

use super::super::cells::densest_claimed_cell;
use super::super::damage::{
    apply_resistance_and_dormancy, catastrophe_cell_conditions, ice_age_temp_drop_k,
};
use super::super::factors::ice_age_severity_factor;
use super::super::kind::CatastropheKind;
use super::super::record::CatastropheRecord;
use super::super::triggers::ice_age_fires;
use super::super::{ICE_AGE_COOLDOWN_TICKS, ICE_AGE_POP_LOSS};

/// Try to fire the ice-age catastrophe this tick.
pub(super) fn try_apply(
    civ: &mut Civ,
    state: &mut PhysicsState,
    planet: &Planet,
    species: &Species,
    tick: u64,
    ecosystem: &mut Option<&mut PlanetEcosystem>,
) -> Option<CatastropheRecord> {
    // Ice age — gated on cold-planet baseline + civ maturity.
    // Pushes communitarian (huddle-together) + hierarchical
    // (centralized resource management).
    let ice_ready = civ
        .last_ice_age_tick
        .is_none_or(|t| tick.saturating_sub(t) >= ICE_AGE_COOLDOWN_TICKS);
    if !(ice_ready && ice_age_fires(planet, civ, tick)) {
        return None;
    }
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
    let cell_conds = catastrophe_cell_conditions(state, planet, ice_cell, temp_drop, Real::ZERO);
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
    Some(CatastropheRecord {
        kind: CatastropheKind::IceAge,
        fraction_lost: frac,
    })
}
