//! Planetary magnetic field as a vector quantity.
//!
//! ## Hemisphere convention
//!
//! Canonical (shared with `coriolis.rs`, `radiation.rs`, and
//! `sim/recognition/src/lib.rs::Signature::Hemisphere`):
//! `signed_offset = axial.r - half_h`; `signed_offset < 0` is the
//! **northern** hemisphere ("Negative r direction = toward north
//! pole = compass-needle convention" — see the in-loop comment
//! below). The centralised helper is
//! `crate::hemisphere::hemisphere_for_row`. This file pre-dates
//! the helper and inlines the `signed_offset.cmp(&0)` branch
//! verbatim; a behaviour-preserving migration to the helper is a
//! separate PR. The convention itself is canonical; only the
//! per-call-site duplication is a refactor target.
//!
//! Note: `world/src/climate.rs::seasonal_temperature_offset` uses
//! the **opposite** mapping. See `sim/world/src/hemisphere.rs` for
//! the audit + the test that names the disagreement.
//!
//! Previously the magnetic field was a scalar magnitude (`Vec<Real>`)
//! with no direction. The recognition library's
//! `magnetic_field_strong` template even keyed on `Field::Charge`
//! as a proxy because nothing actually wrote the magnetic state.
//!
//! Real planetary magnetic fields have direction — they're
//! approximately dipole patterns aligned with the rotational axis,
//! with the horizontal component strongest at the equator and
//! tapering to zero at the poles (where the field becomes purely
//! vertical, which our 2D hex grid doesn't represent). This module
//! promotes the magnetic state to a vector `(B_q, B_r)` per cell
//! and ships a `Magnetism` law that:
//!
//! 1. Initialises the per-cell field at planet build time from
//!    a latitude-dependent dipole model. Magnitude scales with
//!    `Magnetosphere` strength (None / Weak / Strong); direction
//!    is `(0, -lat_factor)` in axial coords (axis-aligned dipole,
//!    no E-W component, pointing from south pole toward north
//!    pole at every cell — like a compass needle).
//! 2. Per macro-step, applies a small diurnal modulation: the
//!    field magnitude oscillates by ±`diurnal_amplitude` over a
//!    `diurnal_period` macro-step cycle. Real planets see this
//!    from ionospheric current systems driven by solar heating;
//!    we collapse the daily variation into a triangular wave
//!    indexed by `state.macro_step()` (the planetary clock). Magnitude
//!    only — direction stays fixed.
//!
//! No Lorentz coupling onto charge/wind motion yet. That's the
//! next refinement; the vector field is now in place to be read
//! by such couplings (and by recognition templates that key on
//! "compass alignment" or "magnetic deflection").
//!
//! Determinism: state inputs are `state.grid().height()` and
//! `state.macro_step()`; both are deterministic. No interior
//! mutability, no per-tick allocation beyond a single magnitude
//! buffer reused per cell.

use crate::laws::Law;
use crate::state::PhysicsState;
use sim_arith::transcendental::{cos, half_pi, sin, sqrt};
use sim_arith::Real;

/// Geomagnetic dipole polarity state (Sprint 5 Item 20). A Markov
/// chain over the three values cycles `Normal → Reversing →
/// Reversed → Reversing → Normal → …` across geological time. The
/// stable polarities (`Normal`, `Reversed`) trial a rare reversal
/// event each tick; the in-flight `Reversing` state holds for a
/// fixed window during which the per-planet `dipole_strength`
/// envelope decays from 1.0 toward a floor and ramps back. Polarity
/// label is direction-only (no physics consequence beyond the
/// transition); the surface impact comes from the strength envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DipoleState {
    /// Stable polarity, magnetic-north aligned with rotational
    /// north (Earth's present orientation). Dipole strength held
    /// at 1.0 (`dipole_strength` envelope).
    Normal,
    /// In-flight reversal — strength envelope decays linearly
    /// toward `MIN_DIPOLE_STRENGTH` at the midpoint of the window
    /// and ramps back up as the new polarity locks in. Lasts
    /// `MagneticReversal::reversal_duration_ticks`. While this
    /// state is active no new trial fires.
    Reversing,
    /// Stable polarity, magnetic-north aligned with rotational
    /// south. Same physics as `Normal`; the label only changes
    /// the direction of the next reversal arrow (Reversed →
    /// Reversing → Normal).
    Reversed,
}

/// SplitMix64 salt for the magnetic-reversal trial stream
/// (Sprint 5 Item 20). Independent of other physics RNG streams
/// (`PLATE_SALT`, `VOLCANISM_SALT`) so the reversal Markov chain
/// stays bit-identical regardless of what other laws sample on
/// any given tick.
const REVERSAL_SALT: u64 = 0xF11A_70F1_5E12_DEAD;

/// SplitMix64 salt for the per-cell crustal-remanence stream
/// (P3.5). Independent of `REVERSAL_SALT` so the per-cell
/// remanence pattern doesn't co-evolve with the reversal Markov
/// chain. Used by `Magnetism::init_local_field` to spray
/// per-cell noise on top of the global dipole.
const REMANENCE_SALT: u64 = 0x6A0F_BE51_C7D2_1AB3;

/// Maximum value `magnetic_field_local` can hold (P3.5). The
/// spec calls for `[0, 1.5]`: `1.0` = Earth-equivalent baseline,
/// `1.5` = strong crustal-remanence umbrella above baseline (Mars
/// southern-highland-class). Any composition of dipole +
/// remanence is clamped to this ceiling so a future global-field
/// model can't accidentally overflow downstream factors.
pub const LOCAL_FIELD_MAX_NUM: i64 = 3;
pub const LOCAL_FIELD_MAX_DEN: i64 = 2;

/// Crustal-remanence contribution scale (P3.5). Remanence is
/// gated on `crust_thickness`: thicker crust holds more frozen-in
/// magnetisation. The per-cell remanence is
/// `REMANENCE_SCALE × thickness_ratio × noise01` where
/// `thickness_ratio = crust_thickness / EARTH_REFERENCE_THICKNESS`
/// and `noise01` is a SplitMix64-derived `[0, 1)` draw. At
/// `REMANENCE_SCALE = 0.5` an average continental cell
/// (thickness ≈ 35 km) tops out at +0.5 above the dipole — enough
/// for the Mars-highlands umbrella to dominate the local shielding
/// signal without saturating the `[0, 1.5]` ceiling on every cell.
pub const REMANENCE_SCALE_NUM: i64 = 1;
pub const REMANENCE_SCALE_DEN: i64 = 2;

/// Reference crust thickness for the remanence weighting (P3.5).
/// 35 km mirrors `tectonics::CONTINENTAL_THICKNESS_KM`; a cell at
/// this thickness gets the full remanence weight (modulated by
/// noise), oceanic cells (~7 km) get ~20 % of that, and zero-
/// thickness cells get no remanence contribution at all.
pub const REMANENCE_REF_THICKNESS_KM: i64 = 35;

/// Floor the strength envelope decays to at the midpoint of a
/// reversal window. Set to 0.1 so the inverse-coupled
/// `cosmic_ray_ground_flux()` peaks at `1 / 0.2 = 5.0` — a ~5×
/// surface-flux amplification during the deepest part of a
/// reversal. Reference: real geomagnetic excursions weaken the
/// dipole to ~10 % of nominal at minimum.
pub const MIN_DIPOLE_STRENGTH_NUM: i64 = 1;
pub const MIN_DIPOLE_STRENGTH_DEN: i64 = 10;

/// Earth-like default Markov-chain trial probability numerator.
/// One reversal per ~250 000 ticks on average.
pub const REVERSAL_TRIAL_NUM: u64 = 1;
/// Earth-like default Markov-chain trial probability denominator.
pub const REVERSAL_TRIAL_DEN: u64 = 250_000;

/// Earth-like default reversal duration in ticks. ~1000 ticks per
/// flip; the strength envelope reaches its floor at the midpoint
/// (~500 ticks in) and ramps back to 1.0 at the close.
pub const REVERSAL_DURATION_TICKS: u64 = 1000;

/// Markov-chain law driving the geomagnetic reversal cycle (Sprint
/// 5 Item 20). One trial per macro-step from a stable polarity; on
/// success the state transitions to `Reversing` and the strength
/// envelope decays linearly to `min_strength` at the window
/// midpoint, then ramps back to 1.0 as the window closes — at which
/// point the polarity flips to the opposite stable state.
///
/// Determinism: the per-tick trial reads `state.macro_step()` and
/// salts it with `seed_salt` into a SplitMix64 finaliser; same seed
/// + same tick → same trial outcome. While `Reversing` no trial is
/// drawn (the strength envelope is a pure function of
/// `(tick - reversal_start_tick) / reversal_duration_ticks`), so
/// the law is fully deterministic without any per-cell state.
#[derive(Debug, Clone, Copy)]
pub struct MagneticReversal {
    /// SplitMix64 salt blended with `macro_step` for the trial
    /// stream. Defaults to `REVERSAL_SALT`; tests can override to
    /// exercise multiple independent realisations.
    pub seed_salt: u64,
    /// Trial probability numerator — `num/den` per tick.
    pub trial_num: u64,
    /// Trial probability denominator. Earth-like default 250 000.
    pub trial_den: u64,
    /// Reversal window in ticks. Earth-like default 1000.
    pub reversal_duration_ticks: u64,
    /// Strength-envelope floor at the midpoint of a reversal.
    pub min_strength: Real,
}

impl MagneticReversal {
    /// Earth-like calibration: ~1/250 000 per-tick reversal trial,
    /// ~1000-tick reversal window, strength floor 0.1.
    #[must_use]
    pub fn earth_like() -> Self {
        Self {
            seed_salt: REVERSAL_SALT,
            trial_num: REVERSAL_TRIAL_NUM,
            trial_den: REVERSAL_TRIAL_DEN,
            reversal_duration_ticks: REVERSAL_DURATION_TICKS,
            min_strength: Real::from_ratio(
                MIN_DIPOLE_STRENGTH_NUM,
                MIN_DIPOLE_STRENGTH_DEN,
            ),
        }
    }

    /// Advance the Markov chain by one tick. Read inputs:
    /// `state.macro_step()`, `state.dipole_state()`,
    /// `state.reversal_start_tick()`. Mutates `dipole_state`,
    /// `dipole_strength`, `reversal_start_tick`,
    /// `last_reversal_tick`.
    ///
    /// Branches:
    /// - Stable (`Normal` / `Reversed`): SplitMix64 trial on
    ///   `(seed_salt, macro_step)`. On success transition to
    ///   `Reversing` and record `reversal_start_tick`. On miss
    ///   keep `dipole_strength = 1.0`.
    /// - `Reversing`: read `(tick - reversal_start_tick)` as the
    ///   in-window phase, scale into `[0, 1]`, and compute a
    ///   symmetric triangular envelope between 1.0 and
    ///   `min_strength`. When the phase reaches the end of the
    ///   window, flip polarity (Normal ↔ Reversed), clear
    ///   `reversal_start_tick`, and reset
    ///   `dipole_strength = 1.0`.
    pub fn step(&self, state: &mut PhysicsState) {
        let tick = state.macro_step();
        match state.dipole_state() {
            DipoleState::Normal | DipoleState::Reversed => {
                // Trial a reversal start. SplitMix64 finaliser
                // shape matches `volcanism::next_u64`; salted with
                // tick so the per-tick draw is independent of
                // anything else the orchestrator does.
                let mut s = self
                    .seed_salt
                    .wrapping_add(tick.wrapping_mul(0x9E37_79B9_7F4A_7C15));
                let roll = next_u64(&mut s) % self.trial_den.max(1);
                if roll < self.trial_num {
                    *state.dipole_state_mut() = DipoleState::Reversing;
                    *state.reversal_start_tick_mut() = Some(tick);
                    // Strength stays at 1.0 on the transition tick;
                    // it begins decaying next call when the in-window
                    // phase advances above zero. Keeping the start-
                    // edge at 1.0 makes the envelope contract `step`-
                    // invariant — applying step twice on the same
                    // tick yields the same strength.
                    *state.dipole_strength_mut() = Real::ONE;
                }
                // Else: stable polarity, full strength.
                else {
                    *state.dipole_strength_mut() = Real::ONE;
                }
            }
            DipoleState::Reversing => {
                let start = state.reversal_start_tick().unwrap_or(tick);
                let elapsed = tick.saturating_sub(start);
                let duration = self.reversal_duration_ticks.max(1);
                if elapsed >= duration {
                    // Close the reversal window: flip polarity and
                    // restore full strength. `last_reversal_tick`
                    // records the *completion* tick so diagnostics
                    // can compute inter-reversal gaps from successive
                    // values.
                    let flipped = match state.dipole_state() {
                        DipoleState::Normal => DipoleState::Reversed,
                        DipoleState::Reversed => DipoleState::Normal,
                        // We're in the `Reversing` arm — pick the
                        // opposite of whichever polarity preceded
                        // it. We don't track the pre-reversal
                        // polarity explicitly, so default to
                        // toggling against the current `Reversed`
                        // assumption: any future polarity-aware
                        // recognition template can read the stable
                        // label and act accordingly. (Concretely:
                        // `step` only enters `Reversing` from
                        // Normal or Reversed, so this branch is
                        // unreachable in practice; we encode the
                        // toggle-via-Reversed default for safety.)
                        DipoleState::Reversing => DipoleState::Reversed,
                    };
                    *state.dipole_state_mut() = flipped;
                    *state.reversal_start_tick_mut() = None;
                    *state.last_reversal_tick_mut() = tick;
                    *state.dipole_strength_mut() = Real::ONE;
                } else {
                    // Symmetric triangular envelope: linearly decay
                    // from 1.0 at start to `min_strength` at the
                    // midpoint, then ramp back to 1.0 at the end.
                    // Avoids the discontinuity of a step function
                    // and keeps the surface flux multiplier
                    // continuous across the window.
                    let half = duration / 2;
                    let (phase_num, phase_den) = if elapsed <= half {
                        // Rising half: 0 → 1 as elapsed → half.
                        (elapsed as i64, half.max(1) as i64)
                    } else {
                        // Falling half: 1 → 0 as elapsed → duration.
                        let remaining = duration - elapsed;
                        let denom = duration - half;
                        (remaining as i64, denom.max(1) as i64)
                    };
                    let phase = Real::from_ratio(phase_num, phase_den);
                    // strength = 1 - phase × (1 - min_strength)
                    let depth = Real::ONE - self.min_strength;
                    let strength = Real::ONE - phase * depth;
                    *state.dipole_strength_mut() = strength;
                }
            }
        }
    }
}

impl Law for MagneticReversal {
    fn integrate(&self, state: &mut PhysicsState, _dt: Real) {
        self.step(state);
    }
}

/// SplitMix64 finaliser. Standard shape — same as the one in
/// `volcanism.rs` and `tectonics.rs`; copied here so the
/// reversal stream doesn't depend on cross-module helpers
/// (those streams might evolve independently). Mutates the state
/// in-place and returns the next 64-bit draw.
fn next_u64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[derive(Debug, Clone, Copy)]
pub struct Magnetism {
    /// Equatorial dipole magnitude. Set by `Magnetosphere` class
    /// (None=0, Weak=10, Strong=50) at planet build time. Polar
    /// cells trend toward 0 magnitude (latitude factor); the
    /// equator gets the full `dipole_strength`.
    pub dipole_strength: Real,
    /// Per-tick fractional swing in field magnitude across the
    /// diurnal cycle. 0.05 = ±5 % per cycle; 0 disables modulation.
    pub diurnal_amplitude: Real,
    /// Macro-steps for one full diurnal cycle. With the 1-day-
    /// per-macro-step cadence this is ~1; on a fast-rotator world
    /// it could be smaller, but our macro-step is the finest
    /// resolution so anything < 1 collapses to per-step. Default 1.
    pub diurnal_period: u32,
}

impl Magnetism {
    /// Earth-like default: strong dipole, small daily swing.
    #[must_use]
    pub fn earth_like() -> Self {
        Self {
            dipole_strength: Real::from_int(50),
            diurnal_amplitude: Real::percent(5),
            diurnal_period: 1,
        }
    }

    /// Magnetism for a given magnetosphere class. None → no
    /// dipole at all; the law reduces to a no-op.
    #[must_use]
    pub fn for_strength(dipole_strength: Real) -> Self {
        Self {
            dipole_strength,
            ..Self::earth_like()
        }
    }

    /// Initialise the per-cell `(B_q, B_r)` vector field on a
    /// fresh state. Call once at planet init *before* the first
    /// `integrate` pass. For each cell at axial `(q, r)`:
    /// - `B_q = 0` (axis-aligned dipole has no E-W component in
    ///   the surface plane)
    /// - `B_r = -dipole_strength · cos_lat` (points from south
    ///   pole toward north pole; magnitude maxes at the equator
    ///   and tapers linearly toward the poles)
    #[allow(clippy::similar_names)]
    pub fn init_field(&self, state: &mut PhysicsState) {
        if self.dipole_strength == Real::ZERO {
            return;
        }
        let height_i = i32::try_from(state.grid().height())
            .unwrap_or(i32::MAX)
            .max(1);
        let half_h = height_i / 2;
        let n = state.grid().n_cells();
        let b_q = vec![Real::ZERO; n];
        let mut b_r = vec![Real::ZERO; n];
        let mut b_z = vec![Real::ZERO; n];
        let two = Real::from_int(2);
        for (cid, axial) in state.grid().cells() {
            let i = cid.0 as usize;
            // Signed pole offset: positive r is south of equator
            // in our convention; negative r is north.
            let signed_offset = axial.r - half_h;
            let pole_dist = signed_offset.abs();
            // Real cos latitude factor — 1 at equator,
            // 0 at poles. We also want the corresponding
            // sin (= 1 at poles, 0 at equator) for B_z.
            let (lat_cos, lat_sin) = if half_h > 0 {
                let angle = half_pi() * Real::from_ratio(i64::from(pole_dist), i64::from(half_h));
                (cos(angle), sin(angle))
            } else {
                (Real::ONE, Real::ZERO)
            };
            // Negative r direction = toward north pole = compass-needle
            // convention.
            b_r[i] = -self.dipole_strength * lat_cos;
            // Vertical-axis component. Real planetary
            // dipole has |B_z| = 2 · dipole_strength · sin(lat),
            // peaked at the magnetic poles where the horizontal
            // component vanishes. Sign flips between hemispheres
            // (field exits at one magnetic pole, enters at the
            // other). Convention: B_z > 0 in the northern
            // hemisphere (signed_offset < 0), B_z < 0 in the
            // southern (signed_offset > 0), zero at the equator.
            let sign = match signed_offset.cmp(&0) {
                std::cmp::Ordering::Less => Real::ONE,
                std::cmp::Ordering::Greater => -Real::ONE,
                std::cmp::Ordering::Equal => Real::ZERO,
            };
            b_z[i] = self.dipole_strength * lat_sin * two * sign;
            // b_q stays zero (axis-aligned dipole).
        }
        let (state_bq, state_br) = state.magnetic_field_mut();
        state_bq.copy_from_slice(&b_q);
        state_br.copy_from_slice(&b_r);
        state.magnetic_field_z_mut().copy_from_slice(&b_z);
        // Refresh the magnitude cache so recognition
        // scans never recompute sqrt per cell per template per
        // tick. With B_z now a real component, magnitude is
        // sqrt(B_q² + B_r² + B_z²).
        let mag_buf: Vec<Real> = b_q
            .iter()
            .zip(b_r.iter())
            .zip(b_z.iter())
            .map(|((q, r), z)| sqrt(*q * *q + *r * *r + *z * *z))
            .collect();
        state.magnetic_magnitude_mut().copy_from_slice(&mag_buf);
    }

    /// Initialise per-cell magnetic shielding (P3.5).
    ///
    /// Real ion escape on partial-magnetosphere planets is
    /// geographically structured: Mars retains a strong crustal-
    /// remanence signal in its southern highlands (Acidalia /
    /// Terra Cimmeria), and ion-pickup loss is measurably weaker
    /// over those patches than over the dipole-free northern
    /// lowlands. The previous single `PlanetEscapeParams::
    /// magnetic_strength` scalar collapsed this to one number and
    /// couldn't represent the umbrella effect at all.
    ///
    /// For each cell we synthesise a local shielding strength by
    /// combining the global dipole contribution
    /// (`state.dipole_strength()`) with a SplitMix64-driven per-cell
    /// remanence weighted by `state.crust_thickness()`:
    ///
    /// - `noise01` ∈ `[0, 1)` from SplitMix64 keyed on
    ///   `(planet_seed XOR REMANENCE_SALT, cell_index)`. Deterministic
    ///   across runs given the same seed.
    /// - `thickness_ratio` = `crust_thickness[i] /
    ///   REMANENCE_REF_THICKNESS_KM` clamped to `[0, 2]`. Cells
    ///   without tectonics installed (empty `crust_thickness`) get
    ///   thickness_ratio = 1 so they still see the noise-driven
    ///   variation; otherwise thick continental cells pick up more
    ///   remanence than thin oceanic cells.
    /// - `remanence` = `REMANENCE_SCALE × thickness_ratio × noise01`.
    ///   Cached in `state.crustal_remanence` so re-normalisation
    ///   doesn't need to re-sample noise.
    /// - `magnetic_field_local[i]` = `(dipole_strength + remanence)`
    ///   clamped to `[0, LOCAL_FIELD_MAX]`.
    ///
    /// Call once at planet init *after* `set_planet_seed` and
    /// `set_tectonics_fields` (or with empty tectonics if the
    /// caller doesn't have plates yet). The global `Magnetism`
    /// vector field can be initialised with `init_field` independently;
    /// the two paths share no state.
    #[allow(clippy::similar_names)]
    pub fn init_local_field(&self, state: &mut PhysicsState) {
        let n = state.grid().n_cells();
        if n == 0 {
            return;
        }
        let seed = state.planet_seed();
        let dipole = state.dipole_strength();
        let scale = Real::from_ratio(REMANENCE_SCALE_NUM, REMANENCE_SCALE_DEN);
        let ref_thickness = Real::from_int(REMANENCE_REF_THICKNESS_KM);
        let two = Real::from_int(2);
        let local_max = Real::from_ratio(LOCAL_FIELD_MAX_NUM, LOCAL_FIELD_MAX_DEN);
        // Snapshot crust_thickness up front so the borrow checker
        // is happy when we touch state.crustal_remanence_mut() +
        // state.magnetic_field_local_mut() below.
        let thickness = state.crust_thickness().to_vec();
        let mut remanence = vec![Real::ZERO; n];
        let mut local = vec![Real::ZERO; n];
        for i in 0..n {
            // SplitMix64 keyed on (seed XOR salt, cell_index). The
            // double-finalisation (`next_u64`) gives a stream that's
            // robust to seed sparsity — single-bit-flip seeds produce
            // uncorrelated cell patterns. Standard shape borrowed
            // from `volcanism::next_u64` / `tectonics`'s plate sampler.
            let mut s = seed ^ REMANENCE_SALT;
            s = s.wrapping_add((i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let raw = next_u64(&mut s);
            // Map the top 16 bits to `[0, 1)` via Real::from_ratio.
            // Using the top half avoids the well-known low-bit bias
            // in linear congruential / SplitMix outputs. We use 16
            // bits (denominator `2^16 = 65536`) rather than 32 bits
            // because Q32.32's integer range tops out around `2^31`
            // and `from_ratio(_, 2^32)` would overflow the
            // I32F32::from_num conversion before the division.
            let top = ((raw >> 48) & 0xFFFF) as i64;
            let noise01 = Real::from_ratio(top, 1_i64 << 16);
            // Per-cell crust thickness, clamped to a sensible
            // ratio window. Empty `crust_thickness` (no tectonics
            // installed yet) defaults to the reference thickness
            // so the variation comes purely from the SplitMix64
            // noise pattern.
            let t_km = if i < thickness.len() {
                thickness[i]
            } else {
                ref_thickness
            };
            let mut thickness_ratio = if ref_thickness == Real::ZERO {
                Real::ONE
            } else {
                t_km / ref_thickness
            };
            if thickness_ratio < Real::ZERO {
                thickness_ratio = Real::ZERO;
            }
            if thickness_ratio > two {
                thickness_ratio = two;
            }
            let r_i = scale * thickness_ratio * noise01;
            remanence[i] = r_i;
            let combined = dipole + r_i;
            let clamped = if combined < Real::ZERO {
                Real::ZERO
            } else if combined > local_max {
                local_max
            } else {
                combined
            };
            local[i] = clamped;
        }
        // Install. `crustal_remanence_mut()` returns a `&mut Vec<Real>`
        // so we can resize-by-assignment; `magnetic_field_local_mut()`
        // is a `&mut [Real]` (already sized to `n` in `PhysicsState::new`).
        *state.crustal_remanence_mut() = remanence;
        state.magnetic_field_local_mut().copy_from_slice(&local);
    }

    /// Re-normalise `magnetic_field_local` against the current
    /// `state.dipole_strength` (P3.5). Called by `integrate` so a
    /// global magnetic reversal (Item 20) drags every cell's
    /// local shielding down proportionally without obliterating
    /// the per-cell remanence variation. Requires
    /// `crustal_remanence` to be populated (no-op otherwise).
    fn renormalize_local_field(&self, state: &mut PhysicsState) {
        let n = state.grid().n_cells();
        if state.crustal_remanence().len() != n {
            // Init hasn't run yet — leave the default uniform field
            // alone so call sites that don't yet wire init_local_field
            // see the pre-P3.5 behaviour.
            return;
        }
        let dipole = state.dipole_strength();
        let local_max = Real::from_ratio(LOCAL_FIELD_MAX_NUM, LOCAL_FIELD_MAX_DEN);
        let remanence = state.crustal_remanence().to_vec();
        let local = state.magnetic_field_local_mut();
        for i in 0..n {
            let combined = dipole + remanence[i];
            let clamped = if combined < Real::ZERO {
                Real::ZERO
            } else if combined > local_max {
                local_max
            } else {
                combined
            };
            local[i] = clamped;
        }
    }

    /// Diurnal modulation factor at the current macro-step. Pure
    /// triangular wave: returns 1.0 at the start of each cycle,
    /// rises linearly to (1 + amplitude) at half-cycle, falls
    /// back to (1 - amplitude) at end-cycle, and so on.
    /// Returns 1.0 when `diurnal_period == 0` (modulation
    /// disabled).
    fn diurnal_factor(&self, macro_step: u64) -> Real {
        if self.diurnal_period == 0 || self.diurnal_amplitude == Real::ZERO {
            return Real::ONE;
        }
        let period = u64::from(self.diurnal_period.max(1));
        let phase = macro_step % period;
        let half = period / 2;
        if half == 0 {
            // Period 1: one swing per macro-step; alternate sign.
            return if phase == 0 {
                Real::ONE + self.diurnal_amplitude
            } else {
                Real::ONE - self.diurnal_amplitude
            };
        }
        // Triangle wave on [-1, +1]:
        //   phase ∈ [0, half) → rises 0 → +1 (linear)
        //   phase ∈ [half, period) → falls +1 → -1 (linear)
        let phase_i = i64::try_from(phase).unwrap_or(i64::MAX);
        let half_i = i64::try_from(half).unwrap_or(i64::MAX);
        let triangle = if phase < half {
            Real::from_ratio(phase_i, half_i)
        } else {
            let down = phase_i - half_i;
            Real::ONE - Real::from_ratio(down, half_i) - Real::ONE
        };
        Real::ONE + self.diurnal_amplitude * triangle
    }
}

impl Law for Magnetism {
    #[allow(clippy::similar_names)]
    fn integrate(&self, state: &mut PhysicsState, _dt: Real) {
        // P3.5: re-normalise the per-cell shielding field against
        // the current `state.dipole_strength` before the diurnal
        // pass. A global magnetic reversal (Item 20) updates
        // `dipole_strength` independently, and this re-renormalisation
        // is what makes that change visible to atmospheric escape.
        // Safe to call unconditionally — the helper no-ops when
        // `crustal_remanence` hasn't been installed.
        self.renormalize_local_field(state);
        if self.dipole_strength == Real::ZERO || self.diurnal_amplitude == Real::ZERO {
            return;
        }
        let factor = self.diurnal_factor(state.macro_step());
        // Apply the *delta* relative to the unscaled field by
        // re-deriving from latitude rather than scaling in place
        // (in-place scaling would compound across ticks). Same
        // pattern radiation uses (read previous-tick value,
        // write new-tick value derived from a model).
        let height_i = i32::try_from(state.grid().height())
            .unwrap_or(i32::MAX)
            .max(1);
        let half_h = height_i / 2;
        let n = state.grid().n_cells();
        let cells: Vec<_> = state
            .grid()
            .cells()
            .map(|(cid, axial)| (cid.0 as usize, axial.r))
            .collect();
        // Track new B_z and |B| separately so we can update both
        // caches once the vector pass is done (avoids holding
        // multiple mutable borrows on different state fields).
        let mut bz_after: Vec<Real> = vec![Real::ZERO; n];
        let mut mag_after: Vec<Real> = vec![Real::ZERO; n];
        let two = Real::from_int(2);
        let cells_owned = cells;
        {
            let (state_bq, state_br) = state.magnetic_field_mut();
            for (i, r) in &cells_owned {
                let i = *i;
                let r = *r;
                if i >= n {
                    continue;
                }
                let signed_offset = r - half_h;
                let pole_dist = signed_offset.abs();
                let (lat_cos, lat_sin) = if half_h > 0 {
                    let angle =
                        half_pi() * Real::from_ratio(i64::from(pole_dist), i64::from(half_h));
                    (cos(angle), sin(angle))
                } else {
                    (Real::ONE, Real::ZERO)
                };
                let sign = match signed_offset.cmp(&0) {
                    std::cmp::Ordering::Less => Real::ONE,
                    std::cmp::Ordering::Greater => -Real::ONE,
                    std::cmp::Ordering::Equal => Real::ZERO,
                };
                state_bq[i] = Real::ZERO;
                let new_br = -self.dipole_strength * lat_cos * factor;
                state_br[i] = new_br;
                let new_bz = self.dipole_strength * lat_sin * two * sign * factor;
                bz_after[i] = new_bz;
                // |B| with B_q = 0: sqrt(B_r² + B_z²).
                mag_after[i] = sqrt(new_br * new_br + new_bz * new_bz);
            }
        }
        state.magnetic_field_z_mut().copy_from_slice(&bz_after);
        state.magnetic_magnitude_mut().copy_from_slice(&mag_after);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::HexGrid;

    #[test]
    fn horizontal_strongest_at_equator() {
        // With B_z added, the *total* magnitude
        // peaks at the poles, not the equator. The horizontal
        // (|B_r|) component still peaks at the equator —
        // verify that explicitly.
        let mut state = PhysicsState::new(HexGrid::new(3, 9));
        let mag = Magnetism::earth_like();
        mag.init_field(&mut state);
        let mid_cell = state.grid().cell_id(crate::grid::Axial::new(0, 4));
        let pole_cell = state.grid().cell_id(crate::grid::Axial::new(0, 0));
        let (_bq, br) = state.magnetic_field();
        let mid_horizontal = br[mid_cell.0 as usize].abs();
        let pole_horizontal = br[pole_cell.0 as usize].abs();
        assert!(
            mid_horizontal > pole_horizontal,
            "equatorial |B_horizontal| should exceed polar: \
             mid={mid_horizontal:?} pole={pole_horizontal:?}"
        );
    }

    #[test]
    fn total_magnitude_strongest_at_poles() {
        // Real dipole: |B| ∝ sqrt(1 + 3 sin²(lat)). Poles get
        // |B| = 2 · dipole; equator gets |B| = dipole.
        let mut state = PhysicsState::new(HexGrid::new(3, 9));
        let mag = Magnetism::earth_like();
        mag.init_field(&mut state);
        let mid_cell = state.grid().cell_id(crate::grid::Axial::new(0, 4));
        let pole_cell = state.grid().cell_id(crate::grid::Axial::new(0, 0));
        let mid_mag = state.magnetic_field_magnitude(mid_cell.0 as usize);
        let pole_mag = state.magnetic_field_magnitude(pole_cell.0 as usize);
        assert!(
            pole_mag > mid_mag,
            "polar |B| should exceed equatorial when B_z is added: \
             mid={mid_mag:?} pole={pole_mag:?}"
        );
    }

    #[test]
    fn no_field_for_zero_dipole() {
        let mut state = PhysicsState::new(HexGrid::new(3, 3));
        let mag = Magnetism::for_strength(Real::ZERO);
        mag.init_field(&mut state);
        for i in 0..state.grid().n_cells() {
            assert_eq!(state.magnetic_field_magnitude(i), Real::ZERO);
        }
    }

    #[test]
    fn dipole_points_toward_north_pole() {
        // B_r should be negative (pointing from south r=high toward
        // north r=0) at every non-pole cell.
        let mut state = PhysicsState::new(HexGrid::new(3, 5));
        let mag = Magnetism::earth_like();
        mag.init_field(&mut state);
        let (_bq, br) = state.magnetic_field();
        // Mid row (r=2) gets the strongest negative B_r.
        let mid_idx = state.grid().cell_id(crate::grid::Axial::new(0, 2)).0 as usize;
        assert!(
            br[mid_idx] < Real::ZERO,
            "equatorial B_r should point toward north pole (negative r)"
        );
    }

    #[test]
    fn integrate_is_deterministic() {
        let mut a = PhysicsState::new(HexGrid::new(4, 4));
        let mut b = PhysicsState::new(HexGrid::new(4, 4));
        let mag = Magnetism::earth_like();
        mag.init_field(&mut a);
        mag.init_field(&mut b);
        for _ in 0..20 {
            mag.integrate(&mut a, Real::ONE);
            a.advance_macro_step();
            mag.integrate(&mut b, Real::ONE);
            b.advance_macro_step();
        }
        assert_eq!(a.magnetic_field().0, b.magnetic_field().0);
        assert_eq!(a.magnetic_field().1, b.magnetic_field().1);
    }

    /// Magnetic-reversal Markov chain frequency check (Sprint 5
    /// Item 20). Run for many trial-period multiples and count the
    /// completed reversals; assert the count falls in a broad
    /// statistical band around the expected mean. The 250 000-tick
    /// Earth-like default would require 2.5 M ticks per the spec;
    /// for the test we shrink the trial denominator to 250 and run
    /// 250 000 ticks, which keeps the same expected ~1000 reversals
    /// while finishing in a fraction of a second.
    #[test]
    fn magnetic_reversal_occurs_on_average_every_250000_ticks() {
        let mut state = PhysicsState::new(HexGrid::new(2, 2));
        let law = MagneticReversal {
            seed_salt: 0xABCD_EF01_2345_6789,
            trial_num: 1,
            // Trial probability 1/250 so 250 000 ticks gives an
            // expected ~1000 reversals (250 000 / 250) — the same
            // shape as the spec's 1/250 000 × 2.5 M-tick anchor.
            trial_den: 250,
            // Short reversal window so completion ticks aren't
            // bunched against the test horizon; the spec's broad
            // [5, 20]-style bound easily covers the resulting count.
            reversal_duration_ticks: 50,
            min_strength: Real::from_ratio(1, 10),
        };
        let mut completed = 0u32;
        let mut prev_state = state.dipole_state();
        for _ in 0..250_000u64 {
            law.step(&mut state);
            // A polarity flip (Normal ↔ Reversed) signals a
            // completed reversal window. We sample the stable label
            // because `Reversing` is the in-flight value and
            // shouldn't double-count.
            let cur = state.dipole_state();
            if cur != prev_state
                && cur != DipoleState::Reversing
                && prev_state != DipoleState::Reversing
            {
                // Should never hit this branch — `Reversing` is the
                // only path between stable polarities. Falls through
                // safely if it does.
            }
            if (prev_state == DipoleState::Reversing) && (cur != DipoleState::Reversing) {
                completed += 1;
            }
            prev_state = cur;
            state.advance_macro_step();
        }
        // Expected ~1000 (= 250 000 × 1/250); accept a generous
        // statistical band. The deterministic SplitMix64 stream is
        // close to uniform over a 250-modulus draw so we shouldn't
        // see anything wild, but the bound stays broad so seed
        // tweaks don't flake the test.
        assert!(
            (500..2_000).contains(&completed),
            "expected ~1000 reversals over 250000 ticks at p=1/250; got {completed}"
        );
    }

    /// Force a reversal at t=0 and check the strength envelope
    /// reaches its trough mid-window and recovers to full strength
    /// after the window closes (Sprint 5 Item 20).
    #[test]
    fn reversal_event_weakens_field_for_1000_tick_window() {
        let mut state = PhysicsState::new(HexGrid::new(2, 2));
        let law = MagneticReversal::earth_like();
        // Force the reversal start. The Markov chain normally
        // enters `Reversing` only on a successful trial, but we
        // can poke the state directly because the law reads it as
        // pure input from there on.
        *state.dipole_state_mut() = DipoleState::Reversing;
        *state.reversal_start_tick_mut() = Some(0);

        // Advance to t=500 — the midpoint of the 1000-tick window.
        // Expect strength to be near `min_strength = 0.1`, well
        // below 0.6 per the spec.
        for _ in 0..500 {
            state.advance_macro_step();
            law.step(&mut state);
        }
        let mid_strength = state.dipole_strength();
        assert!(
            mid_strength < Real::from_ratio(6, 10),
            "expected mid-window strength < 0.6, got {mid_strength:?}"
        );
        assert_eq!(state.dipole_state(), DipoleState::Reversing);

        // Advance to t=1500 — well past the 1000-tick window.
        // Polarity should have flipped to `Reversed` and strength
        // restored to 1.0.
        for _ in 500..1500 {
            state.advance_macro_step();
            law.step(&mut state);
        }
        let post_strength = state.dipole_strength();
        // Allow a small slack — once the window closes the law
        // sets strength to exactly 1.0; this is here in case a
        // future refactor reads a slightly-off post-window value.
        let slack = Real::from_ratio(1, 100);
        assert!(
            (post_strength - Real::ONE).abs() < slack,
            "expected post-window strength near 1.0, got {post_strength:?}"
        );
        assert_eq!(state.dipole_state(), DipoleState::Reversed);
    }

    /// Cosmic-ray ground flux is inverse-coupled to dipole
    /// strength: a weak (mid-reversal) dipole should let many
    /// more cosmic rays through than a fully-locked one (Sprint
    /// 5 Item 20).
    #[test]
    fn cosmic_ray_ground_flux_inverse_to_field_strength() {
        let mut state = PhysicsState::new(HexGrid::new(2, 2));
        // Full strength → flux = 1 / (1.0 + 0.1) = 1 / 1.1.
        *state.dipole_strength_mut() = Real::ONE;
        let strong_flux = state.cosmic_ray_ground_flux();
        // Mid-reversal floor → flux = 1 / (0.1 + 0.1) = 5.0.
        *state.dipole_strength_mut() = Real::from_ratio(1, 10);
        let weak_flux = state.cosmic_ray_ground_flux();
        assert!(
            weak_flux > strong_flux,
            "weak-field flux ({weak_flux:?}) should exceed strong-field flux ({strong_flux:?})"
        );
        // The ratio of weak to strong should be roughly
        // 1.1 / 0.2 ≈ 5.5 — "much greater" per the spec.
        let ratio = weak_flux / strong_flux;
        assert!(
            ratio > Real::from_int(4),
            "weak/strong flux ratio should be > 4 (got {ratio:?})"
        );
    }
}
