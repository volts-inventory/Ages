//! Per-tick extinction sweep. Split out of `planet.rs` in CB4.
//!
//! Houses [`PlanetEcosystem::detect_extinctions`], the
//! `EXTINCTION_CONFIRMATION_TICKS` streak counter that flips a
//! species' `is_extant` flag and emits a `SpeciesExtinct` event when
//! the per-species biomass sits below
//! `EXTINCTION_THRESHOLD_FRAC × producer_capacity` for that long.

use protocol::{ExtinctionCause, SpeciesExtinct};
use sim_arith::Real;

use crate::constants::{EXTINCTION_CONFIRMATION_TICKS, EXTINCTION_THRESHOLD_FRAC};

use super::PlanetEcosystem;

impl PlanetEcosystem {
    /// Per-species low-biomass streak counter. Once a species has
    /// been below the absolute threshold (`EXTINCTION_THRESHOLD_FRAC
    /// × producer_capacity`) for `EXTINCTION_CONFIRMATION_TICKS` in
    /// a row, flip `is_extant = false` and emit a
    /// `SpeciesExtinct { cause = PopulationCollapse }`. The species
    /// stays in `self.species` for history / replay determinism;
    /// later passes of `apply_interactions` / `grow_producers` /
    /// `decay_consumers` skip it via the `is_extant` guard.
    ///
    /// Iteration order is `BTreeMap`-deterministic so the event
    /// stream is byte-stable across rebuilds.
    pub(super) fn detect_extinctions(&mut self, tick: u64) -> Vec<SpeciesExtinct> {
        let threshold =
            Real::from(EXTINCTION_THRESHOLD_FRAC) * self.producer_capacity;
        let mut events = Vec::new();
        for s in self.species.values_mut() {
            if !s.is_extant {
                // Already extinct — keep the streak at zero so a
                // future rewilding (not implemented this PR) starts
                // fresh.
                s.low_biomass_streak = 0;
                continue;
            }
            if s.biomass < threshold {
                s.low_biomass_streak = s.low_biomass_streak.saturating_add(1);
                if s.low_biomass_streak >= EXTINCTION_CONFIRMATION_TICKS {
                    s.is_extant = false;
                    s.low_biomass_streak = 0;
                    events.push(SpeciesExtinct {
                        tick,
                        species_id: s.species_id.0,
                        cause: ExtinctionCause::PopulationCollapse,
                    });
                }
            } else {
                s.low_biomass_streak = 0;
            }
        }
        events
    }
}
