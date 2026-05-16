//! tool-effect aggregators + sensorium gating +
//! domain-claim gating. Each accessor folds the per-tool
//! contribution from `unlocked_tools` and `unlocked_dynamic_tools`
//! into a single value the call sites consume. `BTreeSet` iteration
//! order is deterministic; Real arithmetic is Q32.32 fixed-point;
//! both invariants preserve byte-identical NDJSON replay.

use crate::apparatus::{pick_apparatus_cell, pick_apparatus_channels, Apparatus};
use crate::{tech, tech::ToolKind, Civ};
use sim_arith::Real;
use sim_recognition::RecognitionLibrary;
use std::collections::BTreeSet;

impl Civ {
    /// Multiplicative carrying-capacity factor from unlocked tools.
    /// Folded as a product (`Real::ONE`-default per tool means no
    /// effect; ×1.15 / ×2.0 / ×3.0 stack multiplicatively as the
    /// civ accumulates tools).
    pub fn tool_capacity_multiplier(&self) -> Real {
        let static_part = self
            .unlocked_tools
            .iter()
            .map(|t| t.capacity_multiplier())
            .fold(Real::ONE, |acc, m| acc * m);
        // dynamic-tool effects fold in alongside static-tool
        // effects via the same combinator (product for capacity).
        let dynamic_part = self
            .unlocked_dynamic_tools
            .iter()
            .map(|t| t.effects.capacity_multiplier)
            .fold(Real::ONE, |acc, m| acc * m);
        static_part * dynamic_part
    }

    /// Additive shift applied to the food-crisis floor (`security`
    /// must dip below `FOOD_CRISIS_THRESHOLD - bonus` to count as
    /// a crisis tick). Tools like `FluidGathering`,
    /// `OrganizedHunting`, `BulkStorage` push the threshold lower
    /// so a civ with food-security tools tolerates leaner runs
    /// without collapsing.
    pub fn tool_food_crisis_bonus(&self) -> Real {
        let static_part = self
            .unlocked_tools
            .iter()
            .map(|t| t.food_crisis_resistance_bonus())
            .fold(Real::ZERO, |acc, b| acc + b);
        let dynamic_part = self
            .unlocked_dynamic_tools
            .iter()
            .map(|t| t.effects.food_crisis_bonus)
            .fold(Real::ZERO, |acc, b| acc + b);
        static_part + dynamic_part
    }

    /// War-strength multiplicative factor: `1 + Σ(per-tool bonus)`.
    /// Folds into `conflict::strength` so a civ with weapons +
    /// fortification + projectiles enters fights stronger than its
    /// raw population × literacy product would suggest.
    pub fn tool_war_strength_multiplier(&self) -> Real {
        let static_bonus = self
            .unlocked_tools
            .iter()
            .map(|t| t.war_strength_bonus())
            .fold(Real::ZERO, |acc, b| acc + b);
        let dynamic_bonus = self
            .unlocked_dynamic_tools
            .iter()
            .map(|t| t.effects.war_strength_bonus)
            .fold(Real::ZERO, |acc, b| acc + b);
        Real::ONE + static_bonus + dynamic_bonus
    }

    /// Additive bonus to the seasonal-capacity floor — raises the
    /// per-cell carrying capacity in extreme-season months so a
    /// civ with shelter / fluid-control / energy-storage tools
    /// keeps its population fed through winter.
    pub fn tool_seasonal_floor_bonus(&self) -> Real {
        let static_part = self
            .unlocked_tools
            .iter()
            .map(|t| t.seasonal_floor_bonus())
            .fold(Real::ZERO, |acc, b| acc + b);
        let dynamic_part = self
            .unlocked_dynamic_tools
            .iter()
            .map(|t| t.effects.seasonal_floor_bonus)
            .fold(Real::ZERO, |acc, b| acc + b);
        static_part + dynamic_part
    }

    /// Additive catastrophe-resistance bonus (population loss from
    /// disease / volcanism / etc. is reduced by this fraction).
    pub fn tool_catastrophe_resistance_bonus(&self) -> Real {
        let static_part = self
            .unlocked_tools
            .iter()
            .map(|t| t.catastrophe_resistance_bonus())
            .fold(Real::ZERO, |acc, b| acc + b);
        let dynamic_part = self
            .unlocked_dynamic_tools
            .iter()
            .map(|t| t.effects.catastrophe_resistance_bonus)
            .fold(Real::ZERO, |acc, b| acc + b);
        static_part + dynamic_part
    }

    /// Additive literacy bonus folded into `literacy_score` after
    /// the sigmoid clamp. Tier-1 tools contribute zero; encoding /
    /// jurisprudence / mass-literacy / networking are the lifters.
    pub fn tool_literacy_bonus(&self) -> Real {
        let static_part = self
            .unlocked_tools
            .iter()
            .map(|t| t.literacy_bonus())
            .fold(Real::ZERO, |acc, b| acc + b);
        let dynamic_part = self
            .unlocked_dynamic_tools
            .iter()
            .map(|t| t.effects.literacy_bonus)
            .fold(Real::ZERO, |acc, b| acc + b);
        static_part + dynamic_part
    }

    /// Additive bonus to expansion rate (territory growth speed).
    /// Tier-3 navigation / watercraft and tier-4 transport tools
    /// raise this; tier-1 tools all return zero.
    pub fn tool_expansion_rate_bonus(&self) -> Real {
        let static_part = self
            .unlocked_tools
            .iter()
            .map(|t| t.expansion_rate_bonus())
            .fold(Real::ZERO, |acc, b| acc + b);
        let dynamic_part = self
            .unlocked_dynamic_tools
            .iter()
            .map(|t| t.effects.expansion_rate_bonus)
            .fold(Real::ZERO, |acc, b| acc + b);
        static_part + dynamic_part
    }

    /// Aggregate multiplicative lifespan extension from every
    /// unlocked tool. Returned as a fraction (0.20 = +20%
    /// biological lifespan) capped at 1.00 (= 2× lifespan max).
    /// `effective_lifespan_years` multiplies the species's base
    /// lifespan by `(1 + this)` so a fully-equipped civ stretches
    /// its biological cap by up to 2×. Refreshed each tick via
    /// the per-tick `dynamics_for_civ` re-derivation so newly-
    /// unlocked tools take effect immediately.
    pub fn tool_lifespan_extension_factor(&self) -> Real {
        let cap = Real::ONE;
        let static_part = self
            .unlocked_tools
            .iter()
            .map(|t| t.lifespan_extension_factor())
            .fold(Real::ZERO, |a, b| a + b);
        let dynamic_part = self
            .unlocked_dynamic_tools
            .iter()
            .map(|t| t.effects.lifespan_extension_factor)
            .fold(Real::ZERO, |a, b| a + b);
        (static_part + dynamic_part).min(cap)
    }

    /// Aggregate per-bracket mortality reduction from every
    /// unlocked tool. Each bracket's reduction is summed
    /// additively across unlocked tools then capped at 0.80 so
    /// the bracket can never reach zero per-tick mortality (a
    /// fully-equipped civ still loses *some* people; tools soften
    /// the rate, they don't grant immortality). Returned as
    /// `[infant, juvenile, fertile, elder]`. Refreshed each tick
    /// in `step_population_per_cell` before the population step
    /// so newly-unlocked tools take effect immediately.
    pub fn tool_mortality_reduction_per_bracket(&self) -> [Real; 4] {
        let cap = Real::percent(80);
        let mut acc = [Real::ZERO; 4];
        for tool in &self.unlocked_tools {
            let r = tool.mortality_reduction_per_bracket();
            for i in 0..4 {
                acc[i] = acc[i] + r[i];
            }
        }
        for tool in &self.unlocked_dynamic_tools {
            let r = tool.effects.mortality_reduction_per_bracket;
            for i in 0..4 {
                acc[i] = acc[i] + r[i];
            }
        }
        for v in &mut acc {
            *v = (*v).min(cap);
        }
        acc
    }

    /// Additive bonus to per-civ discovery rate (hypothesizer fit
    /// cadence). Folded into the per-tick attempt scheduling by
    /// `step_per_figure` / `step_with_cosmology_and_doubt` so
    /// `attempt_period` shrinks by `1 / (1 + Σbonus)`. Capped at
    /// `+1.50` so the cadence can't collapse to a single tick on
    /// stacked late-game civs.
    pub fn tool_discovery_rate_bonus(&self) -> Real {
        let static_part = self
            .unlocked_tools
            .iter()
            .map(|t| t.discovery_rate_bonus())
            .fold(Real::ZERO, |acc, b| acc + b);
        let dynamic_part = self
            .unlocked_dynamic_tools
            .iter()
            .map(|t| t.effects.discovery_rate_bonus)
            .fold(Real::ZERO, |acc, b| acc + b);
        (static_part + dynamic_part).min(Real::percent(150))
    }

    /// Additive bonus to civ cohesion equilibrium target. Folded
    /// into `update_cohesion`'s target term before clamping; tools
    /// that bind a polity together (canonised law, shared symbology,
    /// urban anchors, network identity) lift the equilibrium so
    /// civil-war / breakaway thresholds drift farther away. Capped
    /// at `+0.40` so even a fully-equipped late-game civ can't push
    /// the equilibrium past `1.40` (which clamps to `1.0` anyway,
    /// but the cap keeps the contribution interpretable).
    pub fn tool_cohesion_bonus(&self) -> Real {
        let static_part = self
            .unlocked_tools
            .iter()
            .map(|t| t.cohesion_bonus())
            .fold(Real::ZERO, |acc, b| acc + b);
        let dynamic_part = self
            .unlocked_dynamic_tools
            .iter()
            .map(|t| t.effects.cohesion_bonus)
            .fold(Real::ZERO, |acc, b| acc + b);
        (static_part + dynamic_part).min(Real::percent(40))
    }

    /// Additive bonus to intra-civ migration rate (per-tick fraction
    /// of fertile adults redistributing under pressure between
    /// claimed cells). Folded into `migrate_inter_cell` as
    /// `migration_rate * (1 + Σbonus)`. Capped at `+1.00` so the
    /// rate can at most double from the base 5% per tick.
    pub fn tool_migration_speed_bonus(&self) -> Real {
        let static_part = self
            .unlocked_tools
            .iter()
            .map(|t| t.migration_speed_bonus())
            .fold(Real::ZERO, |acc, b| acc + b);
        let dynamic_part = self
            .unlocked_dynamic_tools
            .iter()
            .map(|t| t.effects.migration_speed_bonus)
            .fold(Real::ZERO, |acc, b| acc + b);
        (static_part + dynamic_part).min(Real::ONE)
    }

    /// Additive bonus to per-tick birth-rate multiplier. Folded
    /// into `PopulationDynamics::birth_rate_multiplier` as
    /// `1 + Σbonus`. Refreshed each tick from the civ's unlocked
    /// tools before stepping the cohort, mirroring how
    /// `mortality_reduction` is refreshed. Capped at `+0.50` so
    /// stacked nutrition + medicine can lift biological fertility
    /// by at most 50% — the conception-through-viable-birth gate,
    /// not infant-survival (which is already covered by
    /// `mortality_reduction_per_bracket[0]`).
    pub fn tool_fertility_bonus(&self) -> Real {
        let static_part = self
            .unlocked_tools
            .iter()
            .map(|t| t.fertility_bonus())
            .fold(Real::ZERO, |acc, b| acc + b);
        let dynamic_part = self
            .unlocked_dynamic_tools
            .iter()
            .map(|t| t.effects.fertility_bonus)
            .fold(Real::ZERO, |acc, b| acc + b);
        (static_part + dynamic_part).min(Real::percent(50))
    }

    /// Additive bonus to inter-civ knowledge-transmission fidelity
    /// (lifts the comprehension formula). Tier-1 tools contribute zero;
    /// `CulturalEncoding` / mass-literacy / networking lift it.
    pub fn tool_transmission_fidelity_bonus(&self) -> Real {
        let static_part = self
            .unlocked_tools
            .iter()
            .map(|t| t.transmission_fidelity_bonus())
            .fold(Real::ZERO, |acc, b| acc + b);
        let dynamic_part = self
            .unlocked_dynamic_tools
            .iter()
            .map(|t| t.effects.transmission_fidelity_bonus)
            .fold(Real::ZERO, |acc, b| acc + b);
        static_part + dynamic_part
    }

    /// lift a raw seasonal capacity factor by the civ's
    /// `tool_seasonal_floor_bonus`. The bonus shrinks the seasonal
    /// penalty proportionally — `effective = 1 - (1 - raw) * (1 - bonus)`.
    /// At bonus = 0 this is the identity (raw factor passes through).
    /// At bonus = 1 (theoretical max) the seasonal penalty is fully
    /// eliminated. Tier-1 `SimpleShelter` (+0.10) on a planet whose
    /// worst-season factor is 0.80 lifts the effective factor to ~0.82.
    pub(crate) fn effective_seasonal_factor(&self, raw_factor: Real) -> Real {
        let bonus = self.tool_seasonal_floor_bonus().min(Real::ONE);
        Real::ONE - (Real::ONE - raw_factor) * (Real::ONE - bonus)
    }

    /// reduce a base catastrophe loss fraction by the civ's
    /// `tool_catastrophe_resistance_bonus`. Capped at 0.80 so a
    /// fully-equipped civ still loses at least 20% of the planned
    /// loss (catastrophes still hurt; tools soften the blow).
    /// Bonus = 0 ⇒ raw fraction passes through; bonus = 0.50 ⇒
    /// loss is halved.
    pub fn apply_catastrophe_resistance(&self, base_frac: Real) -> Real {
        let resistance = self
            .tool_catastrophe_resistance_bonus()
            .min(Real::percent(80));
        base_frac * (Real::ONE - resistance)
    }

    /// `true` if this civ can claim a cell with the given
    /// terrain glyph, given its species' native habitat. The rule:
    ///
    /// - Coast (`░`) is always claimable for both habitats — it's
    ///   the transition zone.
    /// - Aquatic civs claim water cells (deep ocean, shallow sea,
    ///   coast) natively; land cells require
    ///   `ToolKind::AmphibiousConstruction`.
    /// - Terrestrial civs claim land cells natively; water cells
    ///   require `ToolKind::AmphibiousConstruction`.
    /// - Amphibious civs claim either domain natively.
    /// - Gas band is uniformly uninhabitable ( multiplier 0).
    ///
    /// Combined with 's universal habitability threshold —
    /// deep ocean and gas remain hard walls for both habitats
    /// regardless of tech, since the multiplier is zero.
    #[must_use]
    pub fn can_claim_glyph(&self, glyph: char, species_habitat: sim_species::Habitat) -> bool {
        use sim_species::Habitat;
        let has_amphibious_tech = self
            .unlocked_tools
            .contains(&tech::ToolKind::AmphibiousConstruction);
        if has_amphibious_tech || matches!(species_habitat, Habitat::Amphibious) {
            return true;
        }
        match species_habitat {
            Habitat::Aquatic => sim_world::is_water_glyph(glyph),
            Habitat::Terrestrial | Habitat::Airborne => sim_world::is_land_glyph(glyph),
            Habitat::Amphibious => true,
        }
    }

    /// Apply a tool unlock to the civ: union the granted channels
    /// into `unlocked_channels`, then walk `recognition_lib` and
    /// compute which templates are now perceivable that weren't
    /// before. Refreshes the hypothesizer's available form vocab
    /// over the new perceivable set. Returns the newly-
    /// perceivable template ids so the caller can emit them.
    pub fn apply_tool_unlock(
        &mut self,
        tool: ToolKind,
        species_baseline: &BTreeSet<u32>,
        recognition_lib: &RecognitionLibrary,
    ) -> Vec<u32> {
        if !self.unlocked_tools.insert(tool) {
            return Vec::new();
        }
        for c in tool.granted_channels() {
            self.unlocked_channels.insert(*c);
        }
        // when the experiment apparatus unlocks, allocate one
        // apparatus cell inside the civ's territory. Subsequent ticks
        // walk `apparatus_cells` to clamp + sample. Civs founded
        // before any cell-claiming has happened (rare edge case)
        // record nothing; the next call to `apply_tool_unlock` for
        // the same tool short-circuits at the `unlocked_tools.insert`
        // check above so a re-attempt won't double-allocate.
        if tool == ToolKind::ExperimentApparatus {
            if let Some(cell) = pick_apparatus_cell(self) {
                let (clamp_channel, measure_channel) = pick_apparatus_channels();
                self.apparatus_cells.push(Apparatus {
                    cell,
                    clamp_channel,
                    measure_channel,
                });
            }
        }
        let mut newly = Vec::new();
        for t in &recognition_lib.templates {
            let already = species_baseline.contains(&t.id)
                || self.extra_perceivable_templates.contains(&t.id);
            if already {
                continue;
            }
            if t.channels
                .iter()
                .any(|c| self.unlocked_channels.contains(c))
            {
                self.extra_perceivable_templates.insert(t.id);
                newly.push(t.id);
            }
        }
        self.refresh_available_forms(species_baseline, recognition_lib);
        newly
    }
}
