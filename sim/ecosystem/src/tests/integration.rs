//! Full-planet end-to-end tests.
//!
//! Reserved for ecosystem-level integration suites that exercise the
//! whole `PlanetEcosystem::step` over many ticks against a complete
//! sampled biota. The existing end-to-end coverage lives in the
//! workspace-level `tests/` directory (e.g. `T15`, `T18`, `T20`,
//! `T21` in `sim/core/tests/`); this module is the home for any new
//! per-crate integration test that needs the in-crate test harness
//! rather than the workspace one.
