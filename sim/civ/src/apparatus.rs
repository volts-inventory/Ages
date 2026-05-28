//! civ-built experiment apparatus — controlled-conditions
//! intervention on top of the passive observation pipeline.
//!
//! A civ that has unlocked `ToolKind::ExperimentApparatus` allocates a
//! single apparatus cell inside its territory. Each tick, the
//! apparatus *clamps* one physics channel at one of four ladder
//! values before physics integrates, then samples the apparatus
//! cell's `measure_channel` after integration. The sample feeds
//! the civ's hypothesizer measurement pool with the clamp value as
//! `x` and the post-physics reading as `y` — the same fit machinery
//! the natural-observation track uses, but over a controlled
//! `(x, y)` distribution rather than whatever planetary heterogeneity
//! happens to provide.
//!
//! Why this matters: a Galileo-style intervention (hold height
//! fixed, measure fall time) recovers coefficients passive observation
//! can't reliably isolate. Heat-diffusion `α` from a clean
//! source-sink experiment converges in dozens of ticks; from a
//! noisy planetary gradient it can take thousands. Clamp + measure
//! is the simplest deterministic intervention the project can ship
//! without moving outside its deterministic-arithmetic contract.
//!
//! Design constraints:
//!
//! - **Determinism.** Apparatus selection (which cell, which channel)
//!   is deterministic at unlock time; the clamp ladder is a fixed
//!   per-channel constant; the per-tick step index is `tick % 4`.
//!   No `thread_rng`, no float, no `HashMap` iteration.
//! - **Bounded scope.** One apparatus per civ at unlock. The
//!   tier-2 tool gate (`manipulation_prereqs` accepts every
//!   `ManipulationKind` — a clamp-and-measure rig is a function,
//!   not a body-plan-specific form — plus literacy floor 0.30,
//!   observation threshold 30k, confirmed `fire` law) lets any
//!   species that has done enough science move from observation-
//!   only to controlled-conditions intervention through its native
//!   manipulation affordance.
//! - **Substrate respect.** The clamp ladder is in raw physics
//!   units, not fit-space. A 250–500 K temperature ladder works on
//!   any habitable substrate (silicate worlds run at 800+ K so
//!   their apparatus reads at the lower end of the planet's range,
//!   but the clamp values still create a meaningful x-spread).
//! - **No EM artefacts.** Charge clamp values stay below the
//!   discharge threshold (50 in `Electromagnetism::earth_like`)
//!   so an apparatus doesn't fire artificial lightning each tick.

use crate::discovery::{Channel, Hypothesizer, MeasurementChannel};
use crate::Civ;
use sim_arith::{Pop, Real};
use sim_physics::{PhysicsState, Substance};

/// A single apparatus cell + the channel pairing the experiment
/// runs. `clamp_channel` is the controlled variable; `measure_channel`
/// is what the civ reads back after physics integrates. The two may
/// be the same channel — clamping `T = 500 K` and measuring `T` after
/// integration is a textbook diffusion experiment that recovers the
/// heat-conduction coefficient.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Apparatus {
    /// Which cell in the civ's territory the apparatus occupies.
    pub cell: u32,
    /// The channel the apparatus *holds fixed* at the start of each
    /// tick. Must be a clamp-meaningful channel (see
    /// `Channel::is_clampable`).
    pub clamp_channel: Channel,
    /// The channel the apparatus *reads* after physics integrates.
    /// Often equal to `clamp_channel` for diffusion-coefficient
    /// experiments.
    pub measure_channel: Channel,
}

impl Channel {
    /// 4-point clamp ladder per channel, in raw physics units
    /// (not fit-space). The values are channel-physically meaningful:
    /// temperature spans freeze→ignition, charge stays below the EM
    /// discharge threshold so the apparatus doesn't fire artificial
    /// lightning. Returns `None` for channels that don't admit a
    /// useful intervention (`Elevation` is geological — a civ can't
    /// raise mountains in a tick).
    #[must_use]
    #[allow(clippy::match_same_arms)]
    pub fn clamp_ladder(self) -> Option<[Real; 4]> {
        match self {
            // K: ~110 K spread inside the typical-habitable band.
            // Wider ranges (e.g. 250–500 K) over-pumped heat into
            // neighbour cells over thousands of ticks of clamping
            // and overflowed the Q32.32 fit accumulators in the
            // sample buffers that drained from the heat-perturbed
            // region. 250–360 K still provides 4× the natural
            // equator-pole gradient — plenty of x-variance for the
            // fit, while staying inside the linear-response regime
            // for heat conduction so the apparatus doesn't violate
            // the planet's energy budget.
            Channel::Temperature => Some([
                Real::from_int(250),
                Real::from_int(290),
                Real::from_int(320),
                Real::from_int(360),
            ]),
            // Metres of solvent column: dry, wet, shallow, modest.
            // A 100-m clamp would over-fill neighbour cells via
            // gravity flow and push the regional water budget far
            // from equilibrium; 8 m max keeps the perturbation
            // local.
            Channel::WaterDepth => {
                Some([Real::ZERO, Real::ONE, Real::from_int(3), Real::from_int(8)])
            }
            // Below the 50-unit discharge threshold so the apparatus
            // doesn't trigger an artificial lightning event each tick.
            Channel::ChargeMagnitude => Some([
                Real::ZERO,
                Real::from_int(5),
                Real::from_int(15),
                Real::from_int(30),
            ]),
            // Substance densities sit in low single digits in the
            // typical sim; modest ladder keeps the cell plausibly
            // habitable across the experiment.
            Channel::Fuel | Channel::Oxidiser | Channel::Vapour | Channel::Ice => {
                Some([Real::ZERO, Real::ONE, Real::from_int(3), Real::from_int(8)])
            }
            // Geology: not clampable in 1 tick. Fossil deposits
            // are buried hydrocarbon stocks set at worldgen — a
            // civ can't reshape them within an experiment window.
            // MagneticField: the planetary dipole isn't clampable
            // either — civs can't reshape `|B|` inside an
            // experiment window. Resonance is the same — the field
            // is physics-law-driven, not directly clampable.
            // Insolation is stellar-driven and not clampable inside an
            // experiment window either.
            Channel::Elevation
            | Channel::Fossil
            | Channel::MagneticField
            | Channel::Resonance
            | Channel::Optics => None,
        }
    }
}

/// pick the lowest-population cell from a civ's claimed cells
/// to host the apparatus. Lowest-population minimises demographic
/// disruption — the experiment runs in a sparsely-occupied frontier
/// rather than the capital. Deterministic: ties resolve by lowest
/// `cell_id`. Returns `None` if the civ has no claimed cells.
#[must_use]
pub fn pick_apparatus_cell(civ: &Civ) -> Option<u32> {
    civ.claimed_cells.iter().copied().min_by(|a, b| {
        let pop_a = civ
            .region_cohorts
            .get(a)
            .map_or(Pop::ZERO, sim_population::Cohort::total);
        let pop_b = civ
            .region_cohorts
            .get(b)
            .map_or(Pop::ZERO, sim_population::Cohort::total);
        pop_a.cmp(&pop_b).then_with(|| a.cmp(b))
    })
}

/// pick the clamp/measure channel pair for a new apparatus.
/// Prefers `Temperature` (universal — every habitable substrate has
/// meaningful thermal physics), then `WaterDepth` (fluid mechanics),
/// then `ChargeMagnitude` (EM). For each first preference, the
/// apparatus is configured for a diffusion experiment
/// (`measure = clamp`) — that's the sharpest physical signal a
/// single-cell intervention can produce, since neighbours respond to
/// the clamped value through the relevant transport law.
#[must_use]
pub fn pick_apparatus_channels() -> (Channel, Channel) {
    // Temperature is the safe default — heat conduction is universal
    // across every substrate class. Civs on hydrocarbon / silicate /
    // ammoniacal worlds still recover their planet's `α` from a
    // clamped-T-then-relax experiment.
    (Channel::Temperature, Channel::Temperature)
}

/// pre-physics clamp write. For each apparatus on each civ,
/// overwrite the clamp channel's value at the apparatus cell with
/// the tick's clamp-ladder value. Called from `physics_phase`
/// *before* the per-tick physics integration. The ladder index
/// cycles `tick % 4` so a 4-tick window samples every clamp value
/// once.
pub fn write_apparatus_clamps(state: &mut PhysicsState, civs: &[Civ], tick: u64) {
    let step_idx = (tick % 4) as usize;
    for civ in civs {
        if !civ.is_active() {
            continue;
        }
        for app in &civ.apparatus_cells {
            let Some(ladder) = app.clamp_channel.clamp_ladder() else {
                continue;
            };
            let value = ladder[step_idx];
            write_channel_at_cell(state, app.clamp_channel, app.cell as usize, value);
        }
    }
}

/// post-physics apparatus sampling. For each apparatus, read
/// the post-integration `measure_channel` value at the apparatus
/// cell, scale both axes into fit-space, and push the
/// `(clamp_value, measure_value)` pair into the civ's hypothesizer
/// measurement track twice (2× information weighting — controlled
/// samples carry more bits than noisy planetary observations).
/// Records the experimental contribution in
/// `experimental_count_by_relation` so the eventual
/// `MeasurementConfirmed` event flags `is_experimental = true`.
pub fn record_apparatus_samples(
    state: &PhysicsState,
    hypothesizer: &mut Hypothesizer,
    apparatus: &[Apparatus],
    tick: u64,
) {
    let step_idx = (tick % 4) as usize;
    for app in apparatus {
        let Some(ladder) = app.clamp_channel.clamp_ladder() else {
            continue;
        };
        let clamp_raw = ladder[step_idx];
        let x_fit = clamp_raw / app.clamp_channel.scale();
        let y_fit = app.measure_channel.read(state, app.cell as usize);
        let y_ch = MeasurementChannel::Direct(app.measure_channel);
        let x_ch = MeasurementChannel::Direct(app.clamp_channel);
        hypothesizer.record_experimental_measurement(y_ch, x_ch, x_fit, y_fit);
    }
}

fn write_channel_at_cell(state: &mut PhysicsState, channel: Channel, cell: usize, value: Real) {
    match channel {
        Channel::Temperature => state.temperature_mut()[cell] = value,
        Channel::WaterDepth => state.water_depth_mut()[cell] = value,
        Channel::ChargeMagnitude => state.charge_mut()[cell] = value,
        Channel::Fuel => state.substance_mut(Substance::Fuel.idx())[cell] = value,
        Channel::Oxidiser => state.substance_mut(Substance::Oxidiser.idx())[cell] = value,
        Channel::Vapour => state.substance_mut(Substance::Vapour.idx())[cell] = value,
        Channel::Ice => state.substance_mut(Substance::Ice.idx())[cell] = value,
        // Not clampable; covered by `clamp_ladder() == None`.
        Channel::Elevation
        | Channel::Fossil
        | Channel::MagneticField
        | Channel::Resonance
        | Channel::Optics => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tech::ToolKind;
    use sim_recognition::ChannelKind;
    use sim_species::ManipulationKind;
    use sim_world::Crust;
    use std::collections::{BTreeMap, BTreeSet};

    fn species_channels() -> BTreeSet<ChannelKind> {
        let mut s = BTreeSet::new();
        s.insert(ChannelKind::VisualLight);
        s.insert(ChannelKind::InfraredThermal);
        s
    }

    fn species_manipulations() -> BTreeSet<ManipulationKind> {
        let mut s = BTreeSet::new();
        s.insert(ManipulationKind::ToolExtension);
        s
    }

    #[test]
    fn temperature_clamp_ladder_provides_meaningful_x_variance() {
        let ladder = Channel::Temperature.clamp_ladder().unwrap();
        assert_eq!(ladder[0], Real::from_int(250));
        // 110 K spread = 4× a natural equator-pole gradient, plenty
        // for the fit pool to recover a slope.
        assert!(ladder[3] - ladder[0] >= Real::from_int(100));
    }

    #[test]
    fn elevation_is_not_clampable() {
        assert!(Channel::Elevation.clamp_ladder().is_none());
    }

    #[test]
    fn charge_clamp_stays_below_discharge_threshold() {
        // invariant: the apparatus must not fire artificial
        // lightning each tick. The discharge threshold in
        // `Electromagnetism::earth_like` is 50 — the highest clamp
        // value sits well below it.
        let ladder = Channel::ChargeMagnitude.clamp_ladder().unwrap();
        for v in ladder {
            assert!(v < Real::from_int(50));
        }
    }

    #[test]
    fn pick_apparatus_cell_returns_none_for_empty_territory() {
        let civ = Civ::new(0, 0, Pop::from_int(100));
        assert!(pick_apparatus_cell(&civ).is_none());
    }

    #[test]
    fn pick_apparatus_cell_prefers_lowest_population() {
        let mut civ = Civ::new(0, 0, Pop::from_int(100));
        civ.claimed_cells.insert(7);
        civ.claimed_cells.insert(11);
        civ.claimed_cells.insert(3);
        let mut cohorts = BTreeMap::new();
        cohorts.insert(7, sim_population::Cohort::with_civ(Pop::from_int(50), 0));
        cohorts.insert(11, sim_population::Cohort::with_civ(Pop::from_int(20), 0));
        cohorts.insert(3, sim_population::Cohort::with_civ(Pop::from_int(80), 0));
        civ.region_cohorts = cohorts;
        // Cell 11 has population 20 (lowest); apparatus picks it.
        assert_eq!(pick_apparatus_cell(&civ), Some(11));
    }

    #[test]
    fn experiment_apparatus_tool_is_buildable_with_tool_extension() {
        // The apparatus accepts every manipulation mode — a clamp-
        // and-measure rig is a function (hold a channel, observe
        // response), not a body-plan-specific form. The substrate
        // gate (confirmed `fire`) and the per-channel clamp ladders
        // do the real "which experiments are meaningful" work.
        assert!(crate::tech::is_buildable(
            ToolKind::ExperimentApparatus,
            &species_channels(),
            &species_manipulations(),
            true,
            true,
            Crust::Basaltic,
        ));
        // ChemicalSecretion alone is also sufficient — secreted
        // controlled-concentration baths are a controlled experiment.
        let secretion: BTreeSet<ManipulationKind> =
            [ManipulationKind::ChemicalSecretion].into_iter().collect();
        assert!(crate::tech::is_buildable(
            ToolKind::ExperimentApparatus,
            &species_channels(),
            &secretion,
            true,
            true,
            Crust::Basaltic,
        ));
        // Empty manipulation set is still rejected — needs *some*
        // deliberate-state affordance.
        let no_tools: BTreeSet<ManipulationKind> = BTreeSet::new();
        assert!(!crate::tech::is_buildable(
            ToolKind::ExperimentApparatus,
            &species_channels(),
            &no_tools,
            true,
            true,
            Crust::Basaltic,
        ));
    }
}
