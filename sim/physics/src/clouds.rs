//! Cloud microphysics (Sprint 5 Item 23).
//!
//! Per-cell cloud fraction derived from vapour saturation plus a
//! vertical-motion proxy. Items 13 (ice albedo) and 14 (per-cell
//! greenhouse) read `cloud_fraction` already, but until this
//! module landed the field was a permanent zero — clouds were a
//! constant. Real-planet climates differ wildly with cloud cover:
//! a fully overcast world bounces back ~half of incoming
//! shortwave, while a clear-sky world soaks it up. The greenhouse
//! signal flips the other way — high-altitude cirrus is nearly
//! transparent to incoming sunlight but very effective at
//! trapping outgoing longwave, so a cirrus-dominated atmosphere
//! warms; low-altitude stratus is the opposite, blocking sunlight
//! more than it traps heat.
//!
//! The law authors two per-cell fields:
//!
//! - `cloud_fraction`: how much of the cell is cloudy, in
//!   `[0, 1]`. Relaxed each tick toward a target derived from
//!   how saturated the cell is and whether air is rising (warm
//!   surface above a cool upper layer drives convection upward,
//!   which cools the air adiabatically and pushes vapour past
//!   its saturation cap).
//! - `cloud_type`: a one-byte discriminant (see [`CloudType`]).
//!   High-elevation cells or cells with strong updraft tip into
//!   cirrus; everything else stays stratus. Cirrus contributes
//!   less albedo and more greenhouse forcing; stratus does the
//!   reverse. The coupling lands in
//!   [`crate::albedo::effective_albedo_for`] (reads
//!   `cloud_type` to pick the cloud peak albedo) and in
//!   [`crate::radiation::Radiation::integrate`] (reads
//!   `cloud_type` to weight the greenhouse contribution).
//!
//! ## Saturation drive
//!
//! Supersaturation is `vapour[cell] / sat_cap(T[cell])` — the
//! ratio of how much vapour the cell holds to how much it can
//! hold. Above the `supersaturation_threshold` (~0.9 of cap by
//! default), cells with rising air grow clouds. Below, clouds
//! decay at a small fixed per-tick rate so a cell that briefly
//! supersaturates and then dries out loses its cover gradually
//! rather than instantly.
//!
//! ## Vertical-motion proxy
//!
//! We don't have a true vertical velocity field. The minimum-
//! viable proxy: the surface-vs-upper-layer temperature gap
//! authored by [`crate::vertical::VerticalConvection`]. A warm
//! surface beneath a cool upper layer is convectively unstable
//! — warm air rises. The gap doubles as both the "is air
//! rising?" gate (positive → yes) and the "how strong?" lever
//! (large gap → strong updraft → cirrus formation).
//!
//! ## Determinism
//!
//! `Real` math throughout (Q32.32 via `sim_arith`); no `f64`, no
//! `HashMap`, no state-dependent branching beyond the strict
//! threshold comparisons that already characterise the integrator
//! family. Bit-exact across runs by construction.

use crate::hydrology::saturation_vapour_cap;
use crate::laws::Law;
use crate::state::PhysicsState;
use sim_arith::Real;

/// Per-cell cloud morphology (Sprint 5 Item 23).
///
/// One byte per cell when stored in `PhysicsState::cloud_type`:
/// `Stratus = 0`, `Cirrus = 1`. Use [`CloudType::from_byte`] /
/// [`CloudType::as_byte`] for the conversion at the storage
/// boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudType {
    /// Low- to mid-altitude clouds with thick liquid-water
    /// content. Bright shortwave-blocker (~0.5 peak albedo), modest
    /// greenhouse contribution. The default if no condition tips
    /// the cell into cirrus.
    Stratus,
    /// High-altitude ice clouds. Nearly transparent to incoming
    /// shortwave (~0.2 peak albedo) but very effective at trapping
    /// outgoing longwave (high greenhouse contribution). Forms
    /// over high-elevation cells or cells with strong rising air.
    Cirrus,
}

impl CloudType {
    /// Decode a one-byte storage value into the typed variant.
    /// `0` → `Stratus`, anything else (`1` by convention) →
    /// `Cirrus`. Defensive on out-of-range inputs so a future
    /// field-extension can't crash here without first failing a
    /// match somewhere louder.
    #[must_use]
    pub fn from_byte(b: u8) -> Self {
        if b == 0 {
            CloudType::Stratus
        } else {
            CloudType::Cirrus
        }
    }

    /// Encode the variant into its one-byte storage form. Inverse
    /// of [`CloudType::from_byte`].
    #[must_use]
    pub fn as_byte(self) -> u8 {
        match self {
            CloudType::Stratus => 0,
            CloudType::Cirrus => 1,
        }
    }
}

/// Per-unit-cloud-fraction greenhouse contribution from cirrus
/// clouds (Sprint 5 Item 23), in K. Cirrus is a strong longwave
/// trap — high-altitude ice crystals absorb upwelling IR almost
/// as effectively as a fully overcast stratus deck but pass
/// almost all incoming shortwave through. Sized so a fully
/// cirrus-overcast cell (cloud_fraction = 1) adds ~15 K to
/// `T_eq` — large enough to dominate weak per-substance forcing
/// in a vapour-poor atmosphere, small enough not to swamp the
/// existing greenhouse cap.
pub fn cirrus_greenhouse_k() -> Real {
    Real::from_int(15)
}

/// Per-unit-cloud-fraction greenhouse contribution from stratus
/// clouds (Sprint 5 Item 23), in K. Lower-altitude clouds are
/// less effective at trapping outgoing longwave than cirrus
/// because they emit at a warmer temperature (closer to surface
/// temperature → smaller `T_surface - T_cloud` gradient). A fully
/// stratus-overcast cell adds ~5 K — non-zero (clouds do still
/// trap some IR) but much less than cirrus.
pub fn stratus_greenhouse_k() -> Real {
    Real::from_int(5)
}

/// Cloud microphysics law (Sprint 5 Item 23). Updates per-cell
/// `cloud_fraction` from vapour saturation + vertical motion and
/// classifies each cell as cirrus or stratus.
#[derive(Debug, Clone, Copy)]
pub struct Clouds {
    /// Supersaturation ratio (vapour / sat_cap) above which a cell
    /// with rising air starts forming clouds. 0.9 = clouds form
    /// when the cell is at 90 % of its saturation cap, matching
    /// the empirical "clouds form just below saturation because
    /// real air has condensation nuclei" picture.
    pub supersaturation_threshold: Real,
    /// Surface elevation above which a cell forms cirrus
    /// (high-altitude clouds) rather than stratus. In the same
    /// units as `state.elevation()` (metres). 5000 m sits above
    /// the trade-wind boundary layer and below the stratospheric
    /// floor — about where real cirrus decks live.
    pub cirrus_altitude_threshold: Real,
    /// Surface-vs-upper-layer temperature gap above which a cell
    /// counts as "strong updraft" for the cirrus classifier.
    /// Cells with a gap above this *and* rising air get
    /// reclassified as cirrus even if their elevation is below
    /// the altitude threshold. Captures the "strong convection
    /// punches a cumulonimbus anvil into the cirrus regime"
    /// picture without needing a real cloud-top-height model.
    pub cirrus_updraft_threshold: Real,
    /// Per-tick growth rate of `cloud_fraction` for cells that
    /// meet the formation criteria. 5 % per tick gives ~20-tick
    /// formation timescale — fast enough that a cell that drifts
    /// into the saturation regime grows cover within a sim-month,
    /// slow enough that single-tick spikes don't flip a clear-sky
    /// cell to overcast.
    pub formation_rate: Real,
    /// Per-tick decay rate of `cloud_fraction` for cells that
    /// don't meet the formation criteria. 5 % per tick mirrors
    /// the formation rate; symmetric growth / decay so a cell
    /// hovering near the threshold neither grows nor shrinks
    /// cover indefinitely.
    pub decay_rate: Real,
}

impl Clouds {
    /// Earth-like defaults. Supersaturation threshold 0.9 of cap;
    /// cirrus above 5 km surface elevation or with a 50 K
    /// surface-vs-upper-layer gap.
    #[must_use]
    pub fn earth_like() -> Self {
        Self {
            supersaturation_threshold: Real::percent(90),
            cirrus_altitude_threshold: Real::from_int(5_000),
            cirrus_updraft_threshold: Real::from_int(50),
            formation_rate: Real::percent(5),
            decay_rate: Real::percent(5),
        }
    }
}

impl Law for Clouds {
    fn integrate(&self, state: &mut PhysicsState, dt: Real) {
        let n = state.grid().n_cells();
        let temps = state.temperature().to_vec();
        let upper = state.upper_temperature().to_vec();
        let vapour = state
            .substance(crate::chemistry::Substance::Vapour.idx())
            .to_vec();
        let elevation = state.elevation().to_vec();
        let formation = (self.formation_rate * dt).clamp01();
        let decay = (self.decay_rate * dt).clamp01();

        // Per-cell pass. Read inputs, write `cloud_fraction` and
        // `cloud_type` in lockstep so a future caller that splits
        // the slices can interleave without ordering hazards.
        let cloud_fraction = state.cloud_fraction_mut();
        let mut next_fraction: Vec<Real> = cloud_fraction.to_vec();
        for i in 0..n {
            // Vertical-motion proxy: positive `surface - upper`
            // gap means the surface is warmer than the upper
            // layer → convectively unstable → air rises. The
            // `VerticalConvection` law maintains the lapse-rate
            // gap at steady state (~30 K on Earth), so any cell
            // with a gap above zero counts as "air rising" — we
            // use the magnitude for the updraft-strength
            // classifier below.
            let vertical_gap = temps[i] - upper[i];
            let rising = vertical_gap > Real::ZERO;

            // Supersaturation: how saturated this cell is. Cap is
            // strictly positive (the floor in
            // `saturation_vapour_cap` guarantees ≥ 100), so the
            // divide is safe.
            let cap = saturation_vapour_cap(temps[i]);
            let supersaturation = if cap > Real::ZERO {
                vapour[i] / cap
            } else {
                Real::ZERO
            };

            // Cells grow clouds when (a) they're near or above
            // saturation AND (b) air is rising. Otherwise the
            // cell loses cover at the decay rate.
            let cur = next_fraction[i];
            let target_growth = supersaturation > self.supersaturation_threshold && rising;
            if target_growth {
                // Linear relaxation toward `1.0` at the formation
                // rate. Caps naturally at 1 via `clamp01` below.
                let delta = (Real::ONE - cur) * formation;
                next_fraction[i] = (cur + delta).clamp01();
            } else {
                let delta = cur * decay;
                next_fraction[i] = (cur - delta).max(Real::ZERO);
            }
        }
        cloud_fraction.copy_from_slice(&next_fraction);

        // Cloud-type classifier. High-altitude cells or cells
        // with a strong updraft tip into cirrus; everything else
        // stays stratus. Default is stratus (byte 0) so cells
        // with zero cloud fraction don't accidentally flip to
        // cirrus on a stale vertical-gap signal — but we set the
        // byte unconditionally so the field stays consistent with
        // the latest temperature and elevation state.
        let cloud_type_dst = state.cloud_type_mut();
        for i in 0..n {
            let high_altitude = elevation[i] >= self.cirrus_altitude_threshold;
            let vertical_gap = temps[i] - upper[i];
            let strong_updraft =
                vertical_gap >= self.cirrus_updraft_threshold && vertical_gap > Real::ZERO;
            let cirrus = high_altitude || strong_updraft;
            cloud_type_dst[i] = if cirrus {
                CloudType::Cirrus.as_byte()
            } else {
                CloudType::Stratus.as_byte()
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::HexGrid;

    #[test]
    fn cloud_type_byte_roundtrip() {
        assert_eq!(CloudType::from_byte(0), CloudType::Stratus);
        assert_eq!(CloudType::from_byte(1), CloudType::Cirrus);
        // Defensive: any out-of-range byte decodes to Cirrus
        // (the non-default variant) so a bug that writes garbage
        // surfaces visibly rather than silently defaulting.
        assert_eq!(CloudType::from_byte(255), CloudType::Cirrus);
        assert_eq!(CloudType::Stratus.as_byte(), 0);
        assert_eq!(CloudType::Cirrus.as_byte(), 1);
    }

    #[test]
    fn cloud_fraction_rises_with_vapour_supersaturation() {
        // Sprint 5 Item 23 required test. Seed a cell with
        // vapour above the saturation cap and a positive
        // surface-vs-upper-layer gap (air rising). After running
        // `Clouds::integrate`, `cloud_fraction` should rise from
        // zero.
        let grid = HexGrid::new(3, 3);
        let mut state = PhysicsState::new(grid);
        let centre = state.grid().cell_id(crate::grid::Axial::new(1, 1)).0 as usize;
        // Mild surface temperature so sat_cap is small and the
        // saturation drive engages even with modest vapour.
        state.temperature_mut()[centre] = Real::from_int(280);
        // Upper layer cooler than surface → positive gap → air
        // rising.
        state.upper_temperature_mut()[centre] = Real::from_int(250);
        // Vapour far above `sat_cap(280)` (~15_864) so the
        // supersaturation ratio is well above the 0.9 threshold.
        state.substance_mut(crate::chemistry::Substance::Vapour.idx())[centre] =
            Real::from_int(30_000);
        let initial = state.cloud_fraction()[centre];
        assert_eq!(initial, Real::ZERO);

        let clouds = Clouds::earth_like();
        // Several ticks so the linear relaxation has time to
        // accumulate visible cover.
        for _ in 0..20 {
            clouds.integrate(&mut state, Real::ONE);
        }
        let after = state.cloud_fraction()[centre];
        assert!(
            after > initial,
            "cloud_fraction should rise under supersaturation + rising air: \
             initial={initial:?} after={after:?}"
        );
        // Sanity: it should be a substantive rise, not just
        // numerical noise.
        assert!(
            after > Real::percent(10),
            "cloud_fraction should grow visibly after 20 ticks: {after:?}"
        );
    }

    #[test]
    fn cloud_type_albedo_greenhouse_correctly_signed() {
        // Sprint 5 Item 23 required test. Build two reference
        // cells: one cirrus, one stratus, both with the same
        // cloud_fraction. Assert that cirrus contributes a lower
        // albedo (sunlight passes through) and a higher
        // greenhouse value (longwave is trapped) than stratus.
        use crate::albedo::{base_albedo_for, effective_albedo_for, Crust};

        let base = base_albedo_for(Real::ZERO, Real::ZERO, Crust::Default); // bare rock
        let cloud_f = Real::ONE; // full cover for both cells

        let cirrus_albedo =
            effective_albedo_for(base, Real::ZERO, Real::ZERO, cloud_f, CloudType::Cirrus);
        let stratus_albedo =
            effective_albedo_for(base, Real::ZERO, Real::ZERO, cloud_f, CloudType::Stratus);
        assert!(
            cirrus_albedo < stratus_albedo,
            "cirrus albedo should be below stratus albedo: \
             cirrus={cirrus_albedo:?} stratus={stratus_albedo:?}"
        );

        // Greenhouse: cirrus > stratus.
        let cirrus_gh = cirrus_greenhouse_k();
        let stratus_gh = stratus_greenhouse_k();
        assert!(
            cirrus_gh > stratus_gh,
            "cirrus greenhouse should exceed stratus greenhouse: \
             cirrus={cirrus_gh:?} stratus={stratus_gh:?}"
        );
    }

    #[test]
    fn clouds_decay_without_supersaturation() {
        // A pre-seeded cloud_fraction with no vapour and no
        // rising air should decay over time, not stay constant.
        let grid = HexGrid::new(3, 3);
        let mut state = PhysicsState::new(grid);
        let centre = state.grid().cell_id(crate::grid::Axial::new(1, 1)).0 as usize;
        state.cloud_fraction_mut()[centre] = Real::percent(50);
        // No vapour, equal upper/surface temperature → no
        // rising air, no supersaturation.
        state.temperature_mut()[centre] = Real::from_int(280);
        state.upper_temperature_mut()[centre] = Real::from_int(280);

        let clouds = Clouds::earth_like();
        let initial = state.cloud_fraction()[centre];
        for _ in 0..20 {
            clouds.integrate(&mut state, Real::ONE);
        }
        let after = state.cloud_fraction()[centre];
        assert!(
            after < initial,
            "cloud_fraction should decay without saturation + rising air: \
             initial={initial:?} after={after:?}"
        );
    }

    #[test]
    fn high_altitude_cell_classifies_as_cirrus() {
        // Cells at or above the cirrus altitude threshold should
        // come out classified as cirrus even without a strong
        // updraft.
        let grid = HexGrid::new(2, 1);
        let mut state = PhysicsState::new(grid);
        let mountain = state.grid().cell_id(crate::grid::Axial::new(1, 0)).0 as usize;
        state.elevation_mut()[mountain] = Real::from_int(6_000); // above 5000 m threshold
        state.temperature_mut()[mountain] = Real::from_int(280);
        state.upper_temperature_mut()[mountain] = Real::from_int(280); // no updraft

        let clouds = Clouds::earth_like();
        clouds.integrate(&mut state, Real::ONE);
        assert_eq!(
            CloudType::from_byte(state.cloud_type()[mountain]),
            CloudType::Cirrus,
            "high-altitude cell should classify as cirrus"
        );
    }

    #[test]
    fn strong_updraft_classifies_as_cirrus() {
        // A low-altitude cell with a strong surface-vs-upper
        // gap (large updraft) should still tip into cirrus —
        // the convective-anvil case.
        let grid = HexGrid::new(2, 1);
        let mut state = PhysicsState::new(grid);
        let cell = state.grid().cell_id(crate::grid::Axial::new(0, 0)).0 as usize;
        state.elevation_mut()[cell] = Real::ZERO; // sea level
        state.temperature_mut()[cell] = Real::from_int(330);
        state.upper_temperature_mut()[cell] = Real::from_int(200); // 130 K gap

        let clouds = Clouds::earth_like();
        clouds.integrate(&mut state, Real::ONE);
        assert_eq!(
            CloudType::from_byte(state.cloud_type()[cell]),
            CloudType::Cirrus,
            "strong updraft should classify as cirrus"
        );
    }

    #[test]
    fn low_altitude_quiet_cell_stays_stratus() {
        // Sea-level cell with no significant updraft should
        // classify as stratus — the default low-altitude regime.
        let grid = HexGrid::new(2, 1);
        let mut state = PhysicsState::new(grid);
        let cell = state.grid().cell_id(crate::grid::Axial::new(0, 0)).0 as usize;
        state.elevation_mut()[cell] = Real::ZERO;
        state.temperature_mut()[cell] = Real::from_int(280);
        state.upper_temperature_mut()[cell] = Real::from_int(270); // 10 K gap, below 50 K threshold

        let clouds = Clouds::earth_like();
        clouds.integrate(&mut state, Real::ONE);
        assert_eq!(
            CloudType::from_byte(state.cloud_type()[cell]),
            CloudType::Stratus,
            "low-altitude quiet cell should stay stratus"
        );
    }

    #[test]
    fn clouds_integrate_is_deterministic() {
        let grid = HexGrid::new(4, 4);
        let mut a = PhysicsState::new(grid.clone());
        let mut b = PhysicsState::new(grid);
        for (i, t) in a.temperature_mut().iter_mut().enumerate() {
            *t = Real::from_int(270 + i64::try_from(i).unwrap() % 30);
        }
        for (i, t) in b.temperature_mut().iter_mut().enumerate() {
            *t = Real::from_int(270 + i64::try_from(i).unwrap() % 30);
        }
        for (i, v) in a
            .substance_mut(crate::chemistry::Substance::Vapour.idx())
            .iter_mut()
            .enumerate()
        {
            *v = Real::from_int(5_000 + i64::try_from(i).unwrap() * 100);
        }
        for (i, v) in b
            .substance_mut(crate::chemistry::Substance::Vapour.idx())
            .iter_mut()
            .enumerate()
        {
            *v = Real::from_int(5_000 + i64::try_from(i).unwrap() * 100);
        }
        let clouds = Clouds::earth_like();
        for _ in 0..30 {
            clouds.integrate(&mut a, Real::ONE);
            clouds.integrate(&mut b, Real::ONE);
        }
        assert_eq!(a.cloud_fraction(), b.cloud_fraction());
        assert_eq!(a.cloud_type(), b.cloud_type());
    }
}
