//! Run configuration: the deterministic `Rng` alias, `rng_from_seed`,
//! and the `RunConfig` struct plus its `dev` constructor.

use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use sim_arith::Real;
use sim_physics::OrchestrationConfig;

/// The deterministic RNG used everywhere in the sim. `ChaCha20` is
/// specified bit-for-bit across platforms and reseeds reproducibly.
pub type Rng = ChaCha20Rng;

pub fn rng_from_seed(seed: u64) -> Rng {
    ChaCha20Rng::seed_from_u64(seed)
}

/// Run configuration. The planet is sampled from `seed` at run
/// start (sim-world::sample_planet); same seed → identical run.
#[derive(Debug, Clone)]
pub struct RunConfig {
    pub seed: u64,
    pub max_ticks: u64,
    pub grid_width: u32,
    pub grid_height: u32,
    pub orchestration: OrchestrationConfig,
}

impl RunConfig {
    /// Sensible defaults for a small dev run. 12×8 = 96 cells gives
    /// enough room for distinct per-civ territories and visible
    /// climate-band variation while keeping the ASCII map under
    /// terminal width and per-tick recognition cost trivial.
    pub fn dev(seed: u64, max_ticks: u64) -> Self {
        Self {
            seed,
            max_ticks,
            grid_width: 12,
            grid_height: 8,
            orchestration: OrchestrationConfig {
                // Earlier this was 12 (one macro-step per month within a
                // year-long tick); now 1 (one macro-step per month-long
                // tick). Year-equivalent physics work is preserved.
                macro_steps_per_step: 1,
                fluid_substeps_per_macro: 5,
                heat_substeps_per_macro: 1,
                chemistry_macros_per_substep: 3,
                em_substeps_per_macro: 1,
                fluid_dt: Real::from_ratio(1, 50),
                heat_dt: Real::from_ratio(1, 100),
                chemistry_dt: Real::from_int(3),
                em_dt: Real::from_ratio(1, 100),
            },
        }
    }
}
