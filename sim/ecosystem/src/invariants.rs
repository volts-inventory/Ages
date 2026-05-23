//! Lindeman pyramid invariants.
//!
//! Per-habitat assimilation efficiency
//! ([`lindeman_assimilation_for_habitat`]) is the *physical* knob the
//! step loop uses during predation — calibrated so the pyramid emerges
//! at steady state without a corrective post-step cap (P2.5 dropped
//! `enforce_lindeman_pyramid`). The companion
//! [`PlanetEcosystem::check_lindeman_invariant`] is a read-only debug
//! check tests use to assert "no runaway" once the dynamics have
//! settled.

use sim_arith::Real;
use sim_species::{EcosystemRole, Habitat};

use crate::constants::LINDEMAN_OVERSHOOT_DEBUG_MAX;
use crate::planet::PlanetEcosystem;

/// Per-habitat assimilation efficiency (P2.5).
#[must_use]
pub fn lindeman_assimilation_for_habitat(habitat: Habitat) -> Real {
    match habitat {
        Habitat::Aquatic => Real::from_ratio(1, 30),
        Habitat::Terrestrial | Habitat::Subterranean | Habitat::Endolithic => {
            Real::from_ratio(1, 10)
        }
        Habitat::Amphibious | Habitat::Airborne => Real::from_ratio(15, 100),
    }
}

/// A Lindeman pyramid invariant violation reported by
/// [`PlanetEcosystem::check_lindeman_invariant`] (P2.5). Names the
/// upper tier whose biomass blew past
/// `LINDEMAN_OVERSHOOT_DEBUG_MAX × per-habitat-ratio × lower-tier
/// biomass`. Returned (not panicked) so tests can decide what to do
/// with it; the production step loop doesn't check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LindemanViolation {
    /// Tier index (1 = primary, 2 = secondary, 3 = apex) that
    /// overshot.
    pub upper_tier: u8,
    /// Biomass total of the offending upper tier.
    pub upper_biomass: Real,
    /// Biomass total of the lower tier feeding it.
    pub lower_biomass: Real,
    /// The maximum-allowed ratio
    /// (`LINDEMAN_OVERSHOOT_DEBUG_MAX × max_assimilation_ratio`) the
    /// upper-over-lower ratio crossed.
    pub allowed_slack: Real,
}

impl PlanetEcosystem {
    /// Lindeman pyramid invariant check (P2.5). Returns `Ok(())` if
    /// each consumer tier sits at no more than
    /// `LINDEMAN_OVERSHOOT_DEBUG_MAX × max_assimilation_ratio` times
    /// the lower tier; otherwise returns a `LindemanViolation` naming
    /// the offending tier pair and the magnitude of the overshoot.
    ///
    /// Replaces the corrective `enforce_lindeman_pyramid` from before
    /// the P2.5 fix — that one *scaled biomasses down* on every tick,
    /// which was double-bookkeeping the per-habitat assimilation
    /// efficiency already applied during the predation step. This
    /// function is a *read-only* invariant: it never modifies state.
    ///
    /// Skipped when the lower tier is below
    /// `producer_capacity × 1%` — a tier collapse isn't a Lindeman
    /// runaway, it's a cascade-extinction case the extinction rule
    /// handles, and the ratio diverges meaninglessly there.
    ///
    /// Intended for use in test invariants + debug assertions; the
    /// production step loop *does not* call this on every tick
    /// because hand-built fixtures that start with an inverted
    /// pyramid (e.g. the keystone-cascade test) would trip it
    /// before the dynamics had any chance to play out. Tests that
    /// want to assert "the pyramid held throughout the run" should
    /// call this themselves at the end of the simulated period.
    #[must_use]
    pub fn check_lindeman_invariant(&self) -> Result<(), LindemanViolation> {
        let max_ratio = self.max_consumer_assimilation();
        let slack = Real::from_int(LINDEMAN_OVERSHOOT_DEBUG_MAX) * max_ratio;
        let collapse_floor = self.producer_capacity * Real::from_ratio(1, 100);

        let producer_total = self.tier_biomass(0);
        if producer_total <= collapse_floor {
            return Ok(());
        }
        let primary_total = self.tier_biomass(1);
        if primary_total > producer_total * slack {
            return Err(LindemanViolation {
                upper_tier: 1,
                upper_biomass: primary_total,
                lower_biomass: producer_total,
                allowed_slack: slack,
            });
        }

        if primary_total > collapse_floor {
            let secondary_total = self.tier_biomass(2);
            if secondary_total > primary_total * slack {
                return Err(LindemanViolation {
                    upper_tier: 2,
                    upper_biomass: secondary_total,
                    lower_biomass: primary_total,
                    allowed_slack: slack,
                });
            }

            if secondary_total > collapse_floor {
                let apex_total = self.tier_biomass(3);
                if apex_total > secondary_total * slack {
                    return Err(LindemanViolation {
                        upper_tier: 3,
                        upper_biomass: apex_total,
                        lower_biomass: secondary_total,
                        allowed_slack: slack,
                    });
                }
            }
        }
        Ok(())
    }

    /// Largest per-habitat Lindeman assimilation ratio held by any
    /// extant non-Producer species in the ecosystem. Used as the
    /// conservative bound for the debug invariant (the higher the
    /// efficiency the higher the legitimate steady-state ratio).
    ///
    /// Falls back to the canonical terrestrial ratio (1/10) when no
    /// consumer is present so an empty-consumer planet still gets a
    /// sensible bound.
    fn max_consumer_assimilation(&self) -> Real {
        let mut best = Real::from_ratio(1, 10);
        for s in self.species.values() {
            if !s.is_extant {
                continue;
            }
            if matches!(s.role, EcosystemRole::Producer { .. }) {
                continue;
            }
            let r = lindeman_assimilation_for_habitat(s.habitat);
            if r > best {
                best = r;
            }
        }
        best
    }
}
