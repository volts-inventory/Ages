//! successor-civ founding from a stateless cohort, plus
//! substrate-derived demographic configuration and the literacy
//! score fed into the cultural-lock + cohesion + transmission paths.

use crate::cosmology;
use crate::demographics::{
    attempt_period_for_cognition, carrying_capacity_per_unit, migration_pressure_threshold,
    scale_attempt_period_for_metabolism,
};
use crate::figures::{found_band, NameGrammar};
use crate::religion;
use crate::{
    Civ, LITERACY_DISCOVERY_WEIGHT, LITERACY_LIFESPAN_DENOM, LITERACY_LIFESPAN_WEIGHT,
    LITERACY_TIER_WEIGHT,
};
use sim_arith::Real;
use sim_population::Cohort;
use sim_species::ModalityKind;
use sim_world::BiosphereClass;
use std::collections::{BTreeMap, BTreeSet};

impl Civ {
    /// founding from a stateless cohort left by a collapsed
    /// predecessor (`parent_civ_id`). The new civ takes over the
    /// stateless cohort (re-tagging `civ_membership` to its own id),
    /// gets a fresh `NameGrammar` reseeded for the new civ id, a
    /// fresh founding band with new per-figure hypothesizers. No
    /// knowledge transfers automatically; handles inherited-
    /// artifact comprehension.
    pub fn refound_from_stateless(
        new_id: u32,
        founded_tick: u64,
        stateless_cohort: Cohort,
        intelligence: Real,
        species_seed: u64,
        species_modalities: &[ModalityKind],
        parent_civ_id: u32,
    ) -> Self {
        let grammar = NameGrammar::derive(species_modalities, new_id, species_seed);
        let attempt_period = attempt_period_for_cognition(intelligence);
        let (figures, next_figure_id) = found_band(
            &grammar,
            new_id,
            species_seed,
            founded_tick,
            1,
            intelligence,
            &[],
            attempt_period,
        );
        let centroid = figures.first().map_or(0, |f| f.cell_assignment);
        let initial_religion = {
            let f = figures.first();
            religion::founding_religion(
                new_id,
                f.map_or(Real::ZERO, |x| x.charisma),
                f.map_or(Real::ZERO, |x| x.doubt),
                f.map_or(Real::ZERO, |x| x.curiosity),
            )
        };
        let mut cohort = stateless_cohort;
        cohort.civ_membership = Some(new_id);
        Self {
            id: new_id,
            name: String::new(),
            founded_tick,
            cohort,
            dynamics: sim_population::PopulationDynamics::earth_like_default(),
            observations: BTreeMap::new(),
            intelligence,
            grammar,
            figures,
            next_figure_id,
            unlocked_tools: BTreeSet::new(),
            unlocked_dynamic_tools: Vec::new(),
            unlocked_channels: BTreeSet::new(),
            extra_perceivable_templates: BTreeSet::new(),
            tech_multiplier: Real::ONE,
            // matches `state::Civ::with_species` default and the
            // calibration in `demographics::carrying_capacity_per_unit`
            // (50,000/fuel-unit). `configure_substrate` overwrites
            // with a biosphere-derived value during full founding.
            carrying_capacity_per_unit: Real::from_int(50_000),
            migration_pressure_threshold: Real::from_ratio(85, 100),
            collapsed_tick: None,
            last_discovery_tick: founded_tick,
            last_territory_emit_tick: founded_tick,
            low_food_streak: 0,
            parent_civ_id: Some(parent_civ_id),
            firings_by_template: BTreeMap::new(),
            cosmology: cosmology::Cosmology::NEUTRAL,
            last_emitted_cosmology: cosmology::Cosmology::NEUTRAL,
            religion: initial_religion,
            last_emitted_religion: initial_religion,
            cultural_lock_streak: 0,
            last_refinement_tick: founded_tick,
            last_volcanic_tick: None,
            last_disease_tick: None,
            last_asteroid_tick: None,
            last_solar_flare_tick: None,
            last_ice_age_tick: None,
            last_catastrophe_tick: None,
            region_cohorts: BTreeMap::new(),
            claimed_cells: BTreeSet::new(),
            territory_centroid: centroid,
            peak_claimed_cells: 0,
            tiny_territory_streak: 0,
            depopulation_streak: 0,
            cohesion: Real::ONE,
            civil_war_streak: 0,
            cohesion_breakaway_streak: 0,
            last_emitted_cohesion: Real::ONE,
            last_emitted_life_expectancy_months: Real::ZERO,
            cognition_delta: Real::ZERO,
            sociality_delta: Real::ZERO,
            lifespan_delta_years: Real::ZERO,
            communication_fidelity_delta: Real::ZERO,
            apparatus_cells: Vec::new(),
        }
    }

    /// install substrate-derived demographic constants on the
    /// civ. Called once per civ at founding by sim/core; replaces the
    /// flat 50 / 0.85 placeholders with values derived from the
    /// planet's biosphere, gravity, and the species' cognition +
    /// sociality. The metabolism arg stretches every founding figure's
    /// hypothesizer `attempt_period` so theory-formation cadence
    /// tracks the planet's biological time-scale. Idempotent — safe
    /// to re-call as substrate changes.
    pub fn configure_substrate(
        &mut self,
        biosphere: BiosphereClass,
        gravity: Real,
        cognition: Real,
        sociality: Real,
        metabolism: Real,
    ) {
        self.carrying_capacity_per_unit = carrying_capacity_per_unit(biosphere, gravity, cognition);
        self.migration_pressure_threshold = migration_pressure_threshold(sociality);
        let scaled = scale_attempt_period_for_metabolism(
            attempt_period_for_cognition(cognition),
            metabolism,
        );
        for figure in &mut self.figures {
            figure.hypothesizer.attempt_period = scaled;
        }
    }

    /// literacy score in `[0, 1]`. Discovery-rate proxy with
    /// persistence-tier and lifespan modifiers. See `q58.md` for
    /// the formula and placeholder weights.
    pub fn literacy_score(&self, current_tick: u64) -> Real {
        let unique_confirmed: u64 = self
            .figures
            .iter()
            .flat_map(|f| f.hypothesizer.confirmed.keys())
            .collect::<BTreeSet<_>>()
            .len() as u64;
        let n_disc = Real::from_int(i64::try_from(unique_confirmed).unwrap_or(i64::MAX));
        // M3 ships with no / persistence-tier transitions
        // wired; v1 leaves the index at zero.
        let tier_index = Real::ZERO;
        let age_ticks = current_tick.saturating_sub(self.founded_tick);
        let age = Real::from_int(i64::try_from(age_ticks).unwrap_or(i64::MAX));
        let lifespan_term = age / Real::from_int(LITERACY_LIFESPAN_DENOM);
        let raw = Real::from_ratio(LITERACY_DISCOVERY_WEIGHT.0, LITERACY_DISCOVERY_WEIGHT.1)
            * n_disc
            + Real::from_ratio(LITERACY_TIER_WEIGHT.0, LITERACY_TIER_WEIGHT.1) * tier_index
            + Real::from_ratio(LITERACY_LIFESPAN_WEIGHT.0, LITERACY_LIFESPAN_WEIGHT.1)
                * lifespan_term;
        // sigmoid_lite(x) = x / (1 + |x|)
        let base = raw / (Real::ONE + raw.abs());
        // tool literacy bonus: CulturalEncoding (+0.10),
        // WrittenJurisprudence (+0.15), MassLiteracy (+0.20),
        // InformationNetworking (+0.15) lift the literacy score
        // above raw discovery-rate. Tier-1 tools contribute zero
        // (pre-symbolic), so this is a no-op until tier-2 lands.
        // Capped at 1.0 — literacy is a 0-1 score.
        (base + self.tool_literacy_bonus()).min(Real::ONE)
    }

    /// settlement-scale multiplier for inter-civ
    /// transmission. Buckets the lifetime peak of `claimed_cells`
    /// into a persistence ladder: a civ that grew to 16+ cells
    /// distributed knowledge across a multi-tier settlement
    /// network and leaves more for successors than a civ that
    /// died at hamlet scale. Bucket boundaries match the BFS-ring
    /// settlement labels used by the post-run report.
    pub fn settlement_persistence_multiplier(&self) -> Real {
        match self.peak_claimed_cells {
            0..=1 => Real::from_ratio(85, 100),
            2..=5 => Real::ONE,
            6..=15 => Real::from_ratio(115, 100),
            _ => Real::from_ratio(130, 100),
        }
    }
}
