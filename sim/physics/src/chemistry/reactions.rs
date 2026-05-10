//! Reaction primitives: `PhaseTransition` (temperature-driven
//! substance redistribution per cell) and `CombustionReaction`
//! (oxidiser + fuel → ash + heat). Both consume `PhysicsState`
//! mutably each integrate step.

use super::substance::Substance;
use crate::state::PhysicsState;
use sim_arith::Real;

/// A temperature-driven phase transition. When `forward_when_hot` is
/// true the reaction proceeds `from → to` for cells with
/// `temperature > threshold`, with rate proportional to the excess.
/// When false, the reaction proceeds for `temperature < threshold`,
/// rate proportional to the deficit. Always mass-conserving within
/// the cell.
///
/// `latent_heat` couples the reaction to the heat field. Sign
/// convention: positive = exothermic (releases heat *into* the
/// cell, raising temperature), negative = endothermic (absorbs heat
/// from the cell, lowering temperature). When mass `m` transitions
/// in a step, the cell temperature changes by `m * latent_heat`
/// (assuming unit heat capacity — refine in M1b calibration if
/// per-substance specific heats matter for recognition templates).
#[derive(Debug, Clone, Copy)]
pub struct PhaseTransition {
    pub from: Substance,
    pub to: Substance,
    pub threshold: Real,
    pub rate: Real,
    pub forward_when_hot: bool,
    pub latent_heat: Real,
}

impl PhaseTransition {
    pub(super) fn apply(self, state: &mut PhysicsState, dt: Real) {
        let n = state.grid().n_cells();
        let temp = state.temperature().to_vec();
        let mut transfers = vec![Real::ZERO; n];

        for (i, t) in temp.iter().enumerate().take(n) {
            let triggered = if self.forward_when_hot {
                *t > self.threshold
            } else {
                *t < self.threshold
            };
            if triggered {
                let delta_t = if self.forward_when_hot {
                    *t - self.threshold
                } else {
                    self.threshold - *t
                };
                let src_density = state.substance(self.from.idx())[i];
                // Reaction rate = rate * delta_t * src_density.
                // Cap by what's available so densities never go
                // negative.
                let raw = self.rate * delta_t * src_density * dt;
                transfers[i] = raw.min(src_density);
            }
        }

        // Apply transfers in a second pass so the reaction sees
        // pre-step densities and mass conservation holds bit-
        // exactly. Latent heat applied to temperature in the same
        // pass.
        for (i, transfer) in transfers.iter().enumerate().take(n) {
            if *transfer != Real::ZERO {
                state.substance_mut(self.from.idx())[i] =
                    state.substance(self.from.idx())[i] - *transfer;
                state.substance_mut(self.to.idx())[i] =
                    state.substance(self.to.idx())[i] + *transfer;
                if self.latent_heat != Real::ZERO {
                    state.temperature_mut()[i] =
                        state.temperature()[i] + (*transfer * self.latent_heat);
                }
            }
        }
    }
}

/// A two-substrate exothermic reaction. `fuel + oxidiser → product`,
/// 1:1:1 mass ratio, requiring `temperature > ignition_threshold`
/// and both substrates present. Mass is conserved (1 unit fuel +
/// 1 unit oxidiser becomes 2 units product). Heat released is
/// `mass_reacted * latent_heat`.
#[derive(Debug, Clone, Copy)]
pub struct CombustionReaction {
    pub fuel: Substance,
    pub oxidiser: Substance,
    pub product: Substance,
    pub ignition_threshold: Real,
    pub rate: Real,
    pub latent_heat: Real,
}

impl CombustionReaction {
    pub(super) fn apply(self, state: &mut PhysicsState, dt: Real) {
        let n = state.grid().n_cells();
        let temp = state.temperature().to_vec();
        let fuel_prev = state.substance(self.fuel.idx()).to_vec();
        let ox_prev = state.substance(self.oxidiser.idx()).to_vec();
        let mut burned = vec![Real::ZERO; n];

        for i in 0..n {
            if temp[i] > self.ignition_threshold
                && fuel_prev[i] > Real::ZERO
                && ox_prev[i] > Real::ZERO
            {
                let excess = temp[i] - self.ignition_threshold;
                // Reaction limited by whichever substrate is scarcer
                // (1:1 stoichiometry).
                let cap = fuel_prev[i].min(ox_prev[i]);
                let raw = self.rate * excess * cap * dt;
                burned[i] = raw.min(cap);
            }
        }

        for i in 0..n {
            if burned[i] != Real::ZERO {
                let m = burned[i];
                state.substance_mut(self.fuel.idx())[i] = fuel_prev[i] - m;
                state.substance_mut(self.oxidiser.idx())[i] = ox_prev[i] - m;
                state.substance_mut(self.product.idx())[i] =
                    state.substance(self.product.idx())[i] + (m + m);
                state.temperature_mut()[i] = state.temperature()[i] + (m * self.latent_heat);
            }
        }
    }
}

/// Photosynthesis-equivalent regrowth: the inverse of combustion.
/// `2 Ash → 1 Fuel + 1 Oxidiser`, mass-conservative, gated on
/// habitable-band temperature, water-cofactor presence, and a
/// per-cell carrying-capacity ceiling
/// (`PhysicsState::biofuel_ceiling`). Without this reaction the
/// planet is on a slow-burn-down trajectory — combustion
/// irreversibly converts biofuel + oxidiser into ash. With it,
/// surface biology closes the loop: cells where wildfires have
/// produced ash slowly relax back toward the biosphere ceiling
/// by drawing the ash + a water cofactor (water itself is not
/// consumed; it gates the reaction the way O₂ gates combustion).
///
/// Rate form is deficit-driven exponential approach:
/// `rate × dt × (ceiling - current_fuel)`, capped by the available
/// ash (each unit of regrown biofuel demands two units of ash).
/// Endothermic — heat drawn from the cell mirrors the latent heat
/// released on burn.
#[derive(Debug, Clone, Copy)]
pub struct BiofuelRegrowth {
    pub fuel: Substance,
    pub oxidiser: Substance,
    pub ash: Substance,
    pub min_temp: Real,
    pub max_temp: Real,
    pub water_cofactor_min: Real,
    /// Per-tick fraction of the deficit that regrows when the
    /// reaction is unblocked. Slow on purpose — biofuel regrowth
    /// should take many ticks so the existing combustion → low
    /// fuel → low pop dynamic still bites in the short term.
    pub rate: Real,
    /// Heat absorbed per unit of regrown biofuel (negative-signed
    /// when set from a positive enthalpy: photosynthesis cools
    /// the cell). Same sign convention as
    /// `PhaseTransition::latent_heat`.
    pub latent_heat: Real,
}

impl BiofuelRegrowth {
    pub(super) fn apply(self, state: &mut PhysicsState, dt: Real) {
        let n = state.grid().n_cells();
        let temp = state.temperature().to_vec();
        let water = state.substance(Substance::Water.idx()).to_vec();
        let fuel_prev = state.substance(self.fuel.idx()).to_vec();
        let ash_prev = state.substance(self.ash.idx()).to_vec();
        let ceiling = state.biofuel_ceiling().to_vec();
        let mut regrown = vec![Real::ZERO; n];

        for i in 0..n {
            if ceiling[i] <= Real::ZERO {
                continue;
            }
            if temp[i] < self.min_temp || temp[i] > self.max_temp {
                continue;
            }
            if water[i] < self.water_cofactor_min {
                continue;
            }
            if fuel_prev[i] >= ceiling[i] {
                continue;
            }
            if ash_prev[i] <= Real::ZERO {
                continue;
            }
            let deficit = ceiling[i] - fuel_prev[i];
            let raw = self.rate * deficit * dt;
            // 2 Ash → 1 Fuel + 1 Oxidiser: ash budget is the
            // binding stoichiometric cap.
            let max_from_ash = ash_prev[i] / Real::from_int(2);
            regrown[i] = raw.min(max_from_ash).min(deficit);
        }

        for i in 0..n {
            if regrown[i] != Real::ZERO {
                let m = regrown[i];
                state.substance_mut(self.ash.idx())[i] = ash_prev[i] - (m + m);
                state.substance_mut(self.fuel.idx())[i] = fuel_prev[i] + m;
                let ox_cur = state.substance(self.oxidiser.idx())[i];
                state.substance_mut(self.oxidiser.idx())[i] = ox_cur + m;
                if self.latent_heat != Real::ZERO {
                    state.temperature_mut()[i] =
                        state.temperature()[i] + (m * self.latent_heat);
                }
            }
        }
    }
}
