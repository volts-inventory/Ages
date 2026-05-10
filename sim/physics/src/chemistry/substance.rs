//! `Substance` — the discriminator over the per-cell substance
//! arrays in `PhysicsState`. Order is stable for the lifetime of
//! the run; new substances append at the end so existing indices
//! survive schema bumps.

use crate::state::N_SUBSTANCES;

/// Substance id. Cast to `usize` to index the `substances` vector
/// in `PhysicsState`. Order matters and is stable across the run.
/// New substances append at the end so existing indices remain
/// stable across schema versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Substance {
    Water = 0,
    Ice = 1,
    Vapour = 2,
    /// Generic biological combustible — wood-equivalent for Earth-
    /// like, stand-in for surface biomass. Renewable: regrows toward
    /// a per-cell ceiling (`PhysicsState::biofuel_ceiling`) via the
    /// `BiofuelRegrowth` reaction, which converts `Ash` back into
    /// `Fuel + Oxidiser` (the photosynthesis-equivalent inverse of
    /// combustion). Civ carrying capacity reads this channel as the
    /// biological-stock proxy.
    Fuel = 3,
    /// Generic oxidiser — atmospheric oxygen-equivalent. Replenished
    /// alongside `Fuel` by the regrowth reaction; otherwise depleted
    /// only by combustion.
    Oxidiser = 4,
    /// Combustion residue. Mass-conserves the fuel + oxidiser that
    /// went in. Drawn down by `BiofuelRegrowth` (2 Ash → 1 Fuel + 1
    /// Oxidiser) so the cycle closes mass-conservatively.
    Ash = 5,
    /// Buried fossil hydrocarbons — non-renewable. Worldgen-only
    /// (set from `Crust::Hydrocarbon` contribution). Combusts in a
    /// separate reaction with a higher ignition threshold and higher
    /// energy density than biofuel; once burned to ash, this channel
    /// does not regenerate. The hydrocarbon-seep recognition template
    /// reads this directly to tag fossil-rich crust.
    Fossil = 6,
}

impl Substance {
    pub const fn idx(self) -> usize {
        self as usize
    }
}

/// Sanity check at compile time — `N_SUBSTANCES` must match the count
/// of `Substance` variants currently authored.
const _: () = assert!(N_SUBSTANCES == 7);
