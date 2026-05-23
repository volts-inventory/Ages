//! `sim-population` — 4-bracket cohort step + biology-derived rates.
//!
//! Pop is tracked per civ as a `Cohort` split into four brackets:
//! `infant / juvenile / fertile / elder`. Only the fertile bracket
//! produces births; only the fertile bracket carries economic /
//! military weight. Each tick, individuals age out at a rate
//! determined by their bracket's duration in months
//! (= `lifespan_years × bracket_fraction × 12`), die at a rate
//! derived from the species' per-bracket survival fraction over
//! that duration, and (for fertile) produce births = `clutch_size /
//! fertile_window_months × fertile.count`.
//!
//! No homo-sapiens calibration baseline. Rates fall out of biology:
//! a clutch=200, lifespan=4yr, maturity=10% species lands on
//! ~14 births/fertile-adult/month inherently; a clutch=1,
//! lifespan=200yr, maturity=20% species lands on ~0.0005/month.
//! Both numerically stable, both derived from the same formulas.
//!
//! Per-bracket food multipliers in `PopulationBiology::food_multipliers`
//! mean a cell's effective demand under N infants + M juveniles +
//! K fertile + L elders is `0.3N + 0.6M + 1.0K + 0.9L`, not just
//! `N + M + K + L`. The food-security ratio compares this weighted
//! demand to capacity — so an age-skewed cohort (lots of
//! dependents) feels stress harder than a fertile-heavy one.
//!
//! Module layout (CC4 split):
//! - `cohort` — `Cohort` struct + bracket arithmetic
//! - `dynamics` — `PopulationDynamics`, `step_with_capacity`,
//!   `food_security`
//! - `lifecycle` — per-`Lifecycle`-variant dispatcher
//! - `tests` — shared fixture + integration assertions

#![allow(clippy::module_name_repetitions)]

pub mod cohort;
pub mod dynamics;
pub mod lifecycle;

pub use cohort::Cohort;
pub use dynamics::{food_security, PopulationDynamics};
pub use lifecycle::{
    step_aquatic, step_eusocial, step_for_lifecycle, step_insect, step_microbial, step_modular,
    step_plant, EusocialColony, LifecycleState,
};

#[cfg(test)]
mod tests;
