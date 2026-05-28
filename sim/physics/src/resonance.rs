//! Speculative resonance / attention field (`Ψ`) — the field-and-
//! resonance vision-boundary extension.
//!
//! A per-cell scalar standing for the planet's *resonant
//! environment*: the electromagnetic / bioelectric coupling medium a
//! field-sensing biology perceives directly. It is driven by each
//! cell's electromagnetic activity (magnetic-field magnitude and
//! charge), amplified by the crust's piezoelectric fraction, and it
//! propagates between neighbours (a dense, noble-gas-rich atmosphere
//! carries it efficiently). Each step it relaxes toward a local
//! equilibrium set by that drive.
//!
//! Determinism: pure Q32.32, pair-flux diffusion (sum-conserving,
//! bounded), no RNG, no system time. No *existing* law reads
//! `resonance`, so installing this law leaves every legacy channel
//! (temperature, charge, climate) bit-identical — the field is
//! additive. Its consumers are new: recognition templates
//! (`Field::Resonance`), the discovery hypothesizer
//! (`Channel::Resonance`), and the functional tier-5 tools that gate
//! on confirmed resonance relations.

use crate::laws::Law;
use crate::state::PhysicsState;
use sim_arith::Real;

/// Resonance-field evolution law. Equilibrium drive per cell is
/// `piezo_gain × (field_coupling·|B| + charge_coupling·|charge|)`;
/// the field relaxes toward it and diffuses to neighbours.
#[derive(Debug, Clone, Copy)]
pub struct ResonanceField {
    /// Crust piezoelectric gain (Earth quartz baseline ≈ 0.05;
    /// piezoelectric-archetype worlds 0.20–0.40). Multiplies the
    /// whole equilibrium drive, so a basaltic crust keeps the field
    /// near zero while a piezoelectric crust makes it prominent.
    pub piezo_gain: Real,
    /// Coupling from magnetic-field magnitude `|B|` into the drive.
    /// Folds the planet's magnetosphere factor in at build time.
    pub field_coupling: Real,
    /// Coupling from `|charge|` into the drive (lightning / static
    /// build-up excites the field even on weak-dipole worlds).
    pub charge_coupling: Real,
    /// Fraction of the gap to equilibrium closed per unit `dt`.
    pub relax_rate: Real,
    /// Pair-flux neighbour-diffusion coefficient. Scaled by
    /// atmosphere density at build time — denser air carries the
    /// resonance farther.
    pub propagation: Real,
    /// Clamp ceiling. Keeps the field inside Q32.32 headroom and
    /// gives recognition thresholds a stable scale.
    pub max: Real,
}

impl ResonanceField {
    /// Earth-baseline couplings: a basaltic, Earth-dipole world has a
    /// faint resonance field driven mostly by charge build-up.
    pub fn earth_like() -> Self {
        Self {
            piezo_gain: Real::percent(5),
            field_coupling: Real::percent(1),
            charge_coupling: Real::from_ratio(1, 1000),
            relax_rate: Real::percent(10),
            propagation: Real::percent(2),
            max: Real::from_int(1000),
        }
    }

    /// Build per-planet couplings from the crust's piezoelectric
    /// fraction (`0..1`), a magnetosphere `field_factor` (None 0 /
    /// Weak 1 / Strong 3), and an atmosphere-derived `propagation`
    /// coefficient. A piezoelectric-crust, strong-dipole, dense-
    /// atmosphere world (the field-and-resonance archetype) gets a
    /// strong, far-propagating field; a basaltic no-dipole world gets
    /// a vanishing one.
    pub fn for_coupling(piezo_fraction: Real, field_factor: Real, propagation: Real) -> Self {
        let mut base = Self::earth_like();
        base.piezo_gain = piezo_fraction.max(Real::from_ratio(1, 100)).min(Real::ONE);
        base.field_coupling = base.field_coupling * field_factor;
        base.propagation = propagation;
        base
    }
}

impl Law for ResonanceField {
    fn integrate(&self, state: &mut PhysicsState, dt: Real) {
        self.diffuse(state, dt);
        self.relax(state, dt);
    }
}

impl ResonanceField {
    /// Pair-flux neighbour diffusion. Same `j > i` single-multiply
    /// scheme as `Electromagnetism::diffuse_charge`, so the `+delta`
    /// and `-delta` are exact negations and the field's sum is
    /// conserved by the diffusion pass bit-exactly.
    fn diffuse(&self, state: &mut PhysicsState, dt: Real) {
        let grid = state.grid().clone();
        let prev = state.resonance().to_vec();
        let next = state.resonance_mut();
        next.copy_from_slice(&prev);

        for (cid, axial) in grid.cells() {
            let i = cid.0 as usize;
            for nb in grid.neighbours(axial) {
                let j = nb.0 as usize;
                if j > i {
                    let delta = self.propagation * dt * (prev[j] - prev[i]);
                    next[i] = next[i] + delta;
                    next[j] = next[j] - delta;
                }
            }
        }
    }

    /// Local relaxation toward the EM-driven equilibrium, clamped to
    /// `[0, max]`. Reads post-Magnetism `|B|` and post-EM `|charge|`.
    fn relax(&self, state: &mut PhysicsState, dt: Real) {
        let n = state.resonance().len();
        for i in 0..n {
            let b = state.magnetic_field_magnitude(i);
            let q = state.charge()[i].abs();
            let psi = state.resonance()[i];
            let drive = self.field_coupling * b + self.charge_coupling * q;
            let eq = (self.piezo_gain * drive).min(self.max);
            let updated = (psi + self.relax_rate * dt * (eq - psi))
                .max(Real::ZERO)
                .min(self.max);
            state.resonance_mut()[i] = updated;
        }
    }
}
