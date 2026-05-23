//! Per-cell catastrophe + cell-level biomass reduction. Split out of
//! `planet.rs` in CB4. Houses [`PlanetEcosystem::apply_catastrophe_at_cell`]
//! and [`PlanetEcosystem::reduce_at_cell`] — both keep the per-cell
//! biomass distribution in sync with the planet-wide aggregate via the
//! `sum(cell_biomass) == biomass` invariant.

use sim_arith::Real;
use sim_species::SpeciesId;

use super::PlanetEcosystem;

impl PlanetEcosystem {
    /// F2 — reduce a single species' biomass at one specific cell by
    /// `fraction ∈ [0, 1]`. Used by the catastrophe path: a volcanic
    /// eruption on cell `c` drains the local producer pool *only*,
    /// without crashing the planet-wide aggregate. Updates both
    /// `cell_biomass[cell]` and the aggregate `biomass` so the
    /// invariant `sum(cell_biomass) == biomass` is preserved.
    ///
    /// No-op if `n_cells == 0` (legacy aggregate-only fixtures), if
    /// the species id is missing, if the species is already extinct,
    /// or if the cell index is out of range. Fraction is clamped to
    /// `[0, 1]` so a buggy caller can't increase biomass via a
    /// negative fraction or eat past zero.
    pub fn reduce_at_cell(&mut self, species_id: SpeciesId, cell: usize, fraction: Real) {
        if self.n_cells == 0 {
            return;
        }
        let frac = if fraction < Real::ZERO {
            Real::ZERO
        } else if fraction > Real::ONE {
            Real::ONE
        } else {
            fraction
        };
        let Some(s) = self.species.get_mut(&species_id) else {
            return;
        };
        if !s.is_extant {
            return;
        }
        if cell >= s.cell_biomass.len() {
            return;
        }
        let before = s.cell_biomass[cell];
        if before <= Real::ZERO {
            return;
        }
        let loss = before * frac;
        let after = before - loss;
        s.cell_biomass[cell] = if after < Real::ZERO { Real::ZERO } else { after };
        // Recompute the aggregate from cells so rounding drift stays
        // bounded. `biomass` is a cached value; the cell slice is the
        // truth-source once `n_cells > 0`.
        s.biomass = s
            .cell_biomass
            .iter()
            .copied()
            .fold(Real::ZERO, |a, b| a + b);
    }

    /// F3 — apply a catastrophe to every extant species in the
    /// ecosystem, scaling per-species biomass loss by
    /// `(1 - tolerance.match_score(cell_T, cell_pH, cell_sal,
    /// cell_rad, cell_p))`. Mirrors the P0.4 pattern used by
    /// `sim_civ::catastrophe::apply_resistance_and_dormancy` on the
    /// civ-bearing `Species`, extended into the trophic web so
    /// extremophile producers + consumers survive radiation bursts /
    /// thermal pulses that would otherwise wipe out narrow-envelope
    /// peers uniformly.
    ///
    /// `raw_loss_frac` is the headline severity in `[0, 1]` (the same
    /// fraction the civ-side path receives from its catastrophe
    /// pipeline); the tolerance term softens it to `raw_loss_frac ×
    /// (1 - match_score)` so:
    /// - `match_score = 1` (cell sits at envelope centre) ⇒ zero
    ///   biomass loss.
    /// - `match_score = 0` (cell outside envelope) ⇒ full
    ///   `raw_loss_frac` biomass loss.
    ///
    /// Cell conditions are passed as the local conditions during the
    /// catastrophe — for instance a radiation burst supplies `rad`
    /// near or above the typical species' `radiation_max`. The
    /// ecosystem currently runs as a single planet-wide aggregate
    /// (per-cell biota is a deferred refactor — see the post-fix
    /// xeno review N2), so the cell conditions are treated as the
    /// planet-wide event signature.
    pub fn apply_catastrophe_at_cell(
        &mut self,
        raw_loss_frac: Real,
        cell_t: Real,
        cell_ph: Real,
        cell_sal: Real,
        cell_rad: Real,
        cell_p: Real,
    ) {
        if raw_loss_frac <= Real::ZERO {
            return;
        }
        for s in self.species.values_mut() {
            if !s.is_extant {
                continue;
            }
            let survival_match =
                s.tolerance.match_score(cell_t, cell_ph, cell_sal, cell_rad, cell_p);
            let loss_frac = raw_loss_frac * (Real::ONE - survival_match);
            if loss_frac <= Real::ZERO {
                continue;
            }
            let loss = s.biomass * loss_frac;
            s.biomass = (s.biomass - loss).max(Real::ZERO);
        }
    }
}
