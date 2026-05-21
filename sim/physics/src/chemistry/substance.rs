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
    /// Atmospheric carbon dioxide — split out from `Vapour` so the
    /// biogeochemical loop (Sprint 2 Item 6b) can move carbon between
    /// the atmosphere and the biosphere independently of the water
    /// cycle. Producers consume `CO2` proportional to their growth
    /// (photosynthesis / chemosynthesis); consumers and decomposers
    /// return `CO2` proportional to respiration + decomposition flux.
    /// Combustion does **not** populate this channel — combustion's
    /// gaseous byproduct stays bundled in `Ash` so the chemistry-only
    /// mass-balance invariant (`2 fuel + 2 oxidiser → 2 ash`) is
    /// preserved bit-for-bit. Biogeochem is the only mover of `CO2`.
    CO2 = 7,
    /// Atmospheric methane — split out as its own channel (Sprint 3
    /// Item 14) so the greenhouse law can read CH4 density per-cell
    /// without aliasing it to the biofuel pool (`Substance::Fuel`).
    /// CH4 is short-lived in real atmospheres: UV photolysis
    /// destroys it on a ~10-year timescale. Modelled here as a
    /// per-tick exponential decay (`ch4 *= 0.999`) applied by the
    /// radiation law. No producer / consumer wiring yet — worldgen
    /// is the only source. Future work: methanogen producers, OH-
    /// radical sink coupled to UV flux.
    Methane = 8,
}

impl Substance {
    pub const fn idx(self) -> usize {
        self as usize
    }
}

/// Sanity check at compile time — `N_SUBSTANCES` must match the count
/// of `Substance` variants currently authored.
const _: () = assert!(N_SUBSTANCES == 9);
