//! CO2 coupling + biomass deltas for the biogeochem-coupled tick path.
//! Split out of `planet.rs` in CB4.
//!
//! Hosts the producer-growth-with-CO2 gating, consumer respiration,
//! and the decomposer-chain carbon-budget closure, plus the two
//! public entry points `step_with_biogeochem` /
//! `step_with_biogeochem_at_tick` that thread them together with the
//! Item 9 chemoautotroph partition + syntrophy enforcement.

use protocol::SpeciesExtinct;
use sim_arith::Real;
use sim_physics::{PhysicsState, Substance};
use sim_species::{EcosystemRole, ProducerMetabolism, SpeciesId};

use crate::constants::{DECOMPOSITION_RATE, PRODUCER_GROWTH_RATE, RESPIRATION_RATE};

use super::helpers::{apply_co2_delta, sum_substance};
use super::PlanetEcosystem;

impl PlanetEcosystem {
    /// Run one ecosystem tick *and* exchange carbon with the
    /// atmosphere via the supplied `PhysicsState` (Sprint 2 Item 6b).
    ///
    /// - Each Producer consumes atmospheric `CO2`, rate-limited by
    ///   `min(co2_available, energy_available, base_potential)`.
    ///   Energy comes from `solar_irradiance` for Photoautotrophs,
    ///   from the planet-wide `Oxidiser` pool for Chemoautotrophs,
    ///   and from their sum for Mixotrophs.
    /// - Each Consumer (anything not a Producer) respires a fixed
    ///   fraction of its biomass back to atmospheric `CO2`.
    /// - When at least one Detritivore or Saprotroph is present, a
    ///   small fraction of *all* species' biomass passes through the
    ///   decomposer chain back to `CO2`.
    ///
    /// CO2 deltas are applied uniformly across grid cells (consumed
    /// per-cell as `consumed / n_cells`, returned the same way), so
    /// the per-tick mass change matches the aggregate-level budget
    /// the biogeochem model balances at.
    pub fn step_with_biogeochem(
        &mut self,
        state: &mut PhysicsState,
        solar_irradiance: Real,
    ) {
        let _ = self.step_with_biogeochem_at_tick(state, solar_irradiance, 0);
    }

    /// Same as `step_with_biogeochem` but carries the current sim
    /// tick and returns any `SpeciesExtinct` events that fired this
    /// tick. The extinction sweep (Item 6a) runs after the
    /// biogeochem-coupled passes so a species that lost biomass to
    /// respiration / decomposition can be flagged extinct in the
    /// same tick.
    pub fn step_with_biogeochem_at_tick(
        &mut self,
        state: &mut PhysicsState,
        solar_irradiance: Real,
        tick: u64,
    ) -> Vec<SpeciesExtinct> {
        // F2 — snapshot pre-step aggregates for the per-cell rescale
        // pass below.
        let prev_biomass = self.snapshot_biomass();
        let co2_consumed = self.grow_producers_with_co2(state, solar_irradiance);
        // Item 9 paths: Chemoautotroph oxidiser-ladder partition and
        // syntrophy enforcement still run alongside the biogeochem
        // coupling so a planet with both layers gets the full stack.
        self.partition_chemoautotrophs();
        self.apply_interactions(tick);
        self.enforce_syntrophy();
        self.decay_consumers();
        let respired = self.respire_consumers();
        let decomposed = self.decomposer_chain();
        // P2.5: no post-step Lindeman cap; per-habitat assimilation
        // is the physical mechanism.
        self.clamp_biomasses();
        // F2 — proportionally redistribute the per-species aggregate
        // delta back to each cell. Catastrophe pokes inside the same
        // tick (which run *before* this step in the orchestrator)
        // have already been folded into the aggregate via
        // `reduce_at_cell`, so the rescale preserves their per-cell
        // heterogeneity.
        self.rescale_cell_biomass(&prev_biomass);
        let co2_returned = respired + decomposed;
        apply_co2_delta(state, co2_returned - co2_consumed);
        self.detect_extinctions(tick)
    }

    /// Producer growth coupled to atmospheric `CO2` + an energy
    /// source (Sprint 2 Item 6b). Returns the *total* CO2 actually
    /// consumed across all producers this tick — the caller
    /// subtracts it from the atmosphere.
    ///
    /// For each producer:
    ///   base_potential = r × B × (1 - B/K)      (same logistic shape)
    ///   gated_growth   = min(co2_available_share,
    ///                        energy_available_share,
    ///                        base_potential)
    ///   B' = B + gated_growth
    ///
    /// CO2 + energy are split equally between the producers that
    /// would otherwise grow this tick — every producer "competes"
    /// for the same atmosphere + sunlight pool, and the
    /// equal-split prevents one species from greedily monopolising
    /// the entire CO2 budget in one tick. Chemoautotroph growth via
    /// the multi-oxidiser ladder (Item 9) is layered on top by
    /// `partition_chemoautotrophs`; this path provides the carbon-
    /// budgeted baseline shared by all three metabolism kinds.
    pub(super) fn grow_producers_with_co2(
        &mut self,
        state: &PhysicsState,
        solar_irradiance: Real,
    ) -> Real {
        let growth_rate = Real::from(PRODUCER_GROWTH_RATE);
        let cap = self.producer_capacity;
        if cap <= Real::ZERO {
            return Real::ZERO;
        }
        let total_co2 = sum_substance(state, Substance::CO2);
        let total_oxidiser = sum_substance(state, Substance::Oxidiser);

        // Producers that could grow this tick (extant, non-zero
        // biomass, room under K). Equal-split of CO2 + energy
        // across this set so a small atmosphere can't be drained
        // entirely by the largest producer in one shot.
        let producer_ids: Vec<SpeciesId> = self
            .species
            .iter()
            .filter_map(|(id, s)| {
                if !s.is_extant {
                    return None;
                }
                if matches!(s.role, EcosystemRole::Producer { .. }) {
                    Some(*id)
                } else {
                    None
                }
            })
            .collect();
        if producer_ids.is_empty() {
            return Real::ZERO;
        }
        let n_prod = Real::from_int(producer_ids.len() as i64);
        let co2_share = total_co2 / n_prod;
        let solar_share = solar_irradiance / n_prod;
        let oxidiser_share = total_oxidiser / n_prod;

        let mut total_consumed = Real::ZERO;
        for id in producer_ids {
            let s = match self.species.get_mut(&id) {
                Some(s) => s,
                None => continue,
            };
            let metabolism = match s.role {
                EcosystemRole::Producer { metabolism } => metabolism,
                _ => continue,
            };
            let ratio = s.biomass / cap;
            let slack = Real::ONE - ratio;
            let base_potential = growth_rate * s.biomass * slack;
            if base_potential <= Real::ZERO {
                continue;
            }
            let energy_share = match metabolism {
                ProducerMetabolism::Photoautotroph => solar_share,
                ProducerMetabolism::Chemoautotroph => oxidiser_share,
                ProducerMetabolism::Mixotroph => solar_share + oxidiser_share,
            };
            let gated = base_potential.min(co2_share).min(energy_share);
            if gated <= Real::ZERO {
                continue;
            }
            s.biomass = s.biomass + gated;
            total_consumed = total_consumed + gated;
        }
        total_consumed
    }

    /// Apply `RESPIRATION_RATE` to every extant consumer (anything
    /// not a Producer). Returns total CO2 returned to the atmosphere.
    /// Consumers lose biomass; that biomass becomes atmospheric CO2.
    pub(super) fn respire_consumers(&mut self) -> Real {
        let rate = Real::from(RESPIRATION_RATE);
        let mut total = Real::ZERO;
        for s in self.species.values_mut() {
            if !s.is_extant {
                continue;
            }
            if let EcosystemRole::Producer { .. } = s.role {
                continue;
            }
            let respired = s.biomass * rate;
            if respired <= Real::ZERO {
                continue;
            }
            s.biomass = s.biomass - respired;
            total = total + respired;
        }
        total
    }

    /// Decomposer chain — when at least one Detritivore or
    /// Saprotroph is extant, free `DECOMPOSITION_RATE` × total
    /// biomass back to atmospheric CO2 *and* deduct that mass
    /// proportionally from every extant species pool.
    ///
    /// Closes the carbon budget: each unit of CO2 released to the
    /// atmosphere is balanced by a unit of biomass removed from
    /// the living pool. Models the steady-state dead-matter
    /// pipeline — even healthy populations are shedding some
    /// carbon through the decomposer compartment each tick, and
    /// the carbon that ends up in the atmosphere came from
    /// somebody's biomass.
    pub(super) fn decomposer_chain(&mut self) -> Real {
        let has_decomposer = self.species.values().any(|s| {
            s.is_extant
                && matches!(
                    s.role,
                    EcosystemRole::Detritivore | EcosystemRole::Saprotroph
                )
        });
        if !has_decomposer {
            return Real::ZERO;
        }
        let rate = Real::from(DECOMPOSITION_RATE);
        let mut total_released = Real::ZERO;
        for s in self.species.values_mut() {
            if !s.is_extant {
                continue;
            }
            let released = s.biomass * rate;
            if released <= Real::ZERO {
                continue;
            }
            s.biomass = s.biomass - released;
            total_released = total_released + released;
        }
        total_released
    }
}
