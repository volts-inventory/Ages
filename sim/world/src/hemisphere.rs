//! Canonical hemisphere convention helper.
//!
//! The Ages physics laws disagree about which axial-r rows belong
//! to which hemisphere, and the expert review flagged this as a
//! latent footgun:
//!
//! - `physics/src/magnetism.rs` and `physics/src/coriolis.rs` use
//!   `signed_offset = axial.r - half_h`, treating `signed_offset < 0`
//!   (i.e. *small* `r`) as the **northern** hemisphere. This is
//!   `Magnetism`'s explicit declaration: "Negative r direction =
//!   toward north pole = compass-needle convention".
//! - `physics/src/radiation.rs` reads "row 0 = the N pole in our
//!   convention" and computes `sub_solar_row = half_h - raw_offset`,
//!   matching the magnetism convention.
//! - `sim/recognition/src/lib.rs::Signature::Hemisphere` matches
//!   `row < height/2` as northern — same as magnetism / radiation /
//!   coriolis.
//! - `world/src/climate.rs::seasonal_temperature_offset` (line ~59)
//!   uses `row_signed = row - mid` and treats `row_signed > 0`
//!   (i.e. *large* `r`) as northern — the **opposite** convention.
//!
//! The earlier-attempted fix flipped `climate.rs` to match the rest,
//! but doing so changed which cells received which seasonal phase,
//! which shifted worldgen RNG-derived species sampling enough to
//! break determinism (see expert review for full diagnosis).
//!
//! This module is the safe, audit-shaped intermediate step:
//!
//! 1. **Document the canonical convention** here, in one place,
//!    once: row 0 (small r) is the north pole; larger r is further
//!    south. This matches magnetism + coriolis + radiation +
//!    recognition (the majority).
//! 2. **Expose two helpers**: `hemisphere_for_row_physics` returns the
//!    canonical hemisphere — what every law except `climate.rs`
//!    already returns. `hemisphere_for_row_climate` returns the
//!    legacy `climate.rs` mapping. They differ only in sign; the
//!    latter is preserved as a separate function so the bug is
//!    visible and tested-against, not silently aliased away.
//! 3. **Assert via test** that the two helpers return opposite-sign
//!    hemispheres for the same row. The test documents the disagreement
//!    so a future deterministic-seed-rebaseline PR can flip one to the
//!    other knowing exactly what changes.
//!
//! Determinism is preserved: the helpers return the same values the
//! existing code already returns. No behaviour change.

/// Which hemisphere a cell belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hemisphere {
    /// Above the equator. In the canonical (physics) convention,
    /// smaller `axial.r` rows; corresponds to `signed_offset < 0`.
    North,
    /// Below the equator. In the canonical (physics) convention,
    /// larger `axial.r` rows; corresponds to `signed_offset > 0`.
    South,
    /// On the equator (`r == height / 2`).
    Equator,
}

impl Hemisphere {
    /// Sign convention used by physics laws (`Coriolis`, `Magnetism`,
    /// `Radiation`): `+1` = northern, `-1` = southern, `0` = equator.
    /// Same sign carried by `signed_offset = -(r - half_h)` once
    /// you account for the existing "negative r = north" comment.
    #[must_use]
    pub fn physics_sign(self) -> i64 {
        match self {
            Hemisphere::North => 1,
            Hemisphere::South => -1,
            Hemisphere::Equator => 0,
        }
    }

    /// Sign convention used by `climate.rs::seasonal_temperature_offset`:
    /// `+1` for `row > mid` (the legacy "northern is below mid" mapping),
    /// `-1` for `row < mid`, `0` on the equator. Opposite-signed
    /// to `physics_sign` for the same hemisphere.
    #[must_use]
    pub fn climate_legacy_sign(self) -> i64 {
        -self.physics_sign()
    }
}

/// Canonical hemisphere mapping shared by `Magnetism`, `Coriolis`,
/// `Radiation`, and `Signature::Hemisphere`: row 0 is the north
/// pole; larger row index is further south.
///
/// Returns `Hemisphere::Equator` when `row == height / 2` (the same
/// edge case the existing laws short-circuit). Callers in
/// physics paths should prefer this helper over re-deriving
/// `signed_offset` so the convention is centralised.
#[must_use]
pub fn hemisphere_for_row(row: u32, height: u32) -> Hemisphere {
    let mid = (height as i64) / 2;
    let r = row as i64;
    let signed = r - mid;
    if signed < 0 {
        Hemisphere::North
    } else if signed > 0 {
        Hemisphere::South
    } else {
        Hemisphere::Equator
    }
}

/// Legacy climate mapping for backward compatibility with
/// `seasonal_temperature_offset`: returns `Hemisphere::North` for
/// `row > mid`, `Hemisphere::South` for `row < mid`. This is the
/// **opposite** mapping of [`hemisphere_for_row`] and is preserved
/// here so the disagreement is named, tested, and easy to flip
/// in a future seed-aware rebaseline PR without grepping for
/// `row > mid` across the world crate.
#[must_use]
pub fn hemisphere_for_row_climate_legacy(row: u32, height: u32) -> Hemisphere {
    let mid = (height as i64) / 2;
    let r = row as i64;
    let signed = r - mid;
    if signed > 0 {
        Hemisphere::North
    } else if signed < 0 {
        Hemisphere::South
    } else {
        Hemisphere::Equator
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_helper_matches_magnetism_convention() {
        // height 9, mid = 4. Rows 0..4 → north (signed_offset < 0),
        // row 4 → equator, rows 5..9 → south.
        let h = 9;
        assert_eq!(hemisphere_for_row(0, h), Hemisphere::North);
        assert_eq!(hemisphere_for_row(3, h), Hemisphere::North);
        assert_eq!(hemisphere_for_row(4, h), Hemisphere::Equator);
        assert_eq!(hemisphere_for_row(5, h), Hemisphere::South);
        assert_eq!(hemisphere_for_row(8, h), Hemisphere::South);
    }

    #[test]
    fn legacy_climate_helper_returns_opposite_sign() {
        // The disagreement made testable: physics says row 1 is
        // north, climate's legacy mapping says row 1 is south, and
        // their `*_sign` accessors must be exact negatives at every
        // non-equator row. This test fails if either convention
        // flips without the other being updated.
        let h = 9;
        for row in 0u32..h {
            let phys = hemisphere_for_row(row, h);
            let clim = hemisphere_for_row_climate_legacy(row, h);
            assert_eq!(
                phys.physics_sign(),
                -clim.physics_sign(),
                "row {row}: physics={phys:?} clim_legacy={clim:?} \
                 should be opposite signs"
            );
            assert_eq!(
                phys.physics_sign(),
                clim.climate_legacy_sign(),
                "row {row}: physics_sign should equal climate_legacy_sign \
                 of the legacy mapping"
            );
        }
    }

    #[test]
    fn hemisphere_helper_returns_consistent_signs() {
        // hemisphere_for_row(row, height) produces the same sign
        // that magnetism / coriolis would compute via
        // `(r - half_h).signum()` (negated, since their negative-r
        // = north sign convention is the inverse of `signed.signum()`).
        let h = 7;
        let half_h = (h as i64) / 2;
        for row in 0u32..h {
            let r = row as i64;
            let signed_offset = r - half_h;
            let want_sign = match signed_offset.cmp(&0) {
                std::cmp::Ordering::Less => 1,    // north
                std::cmp::Ordering::Greater => -1, // south
                std::cmp::Ordering::Equal => 0,
            };
            let got = hemisphere_for_row(row, h);
            assert_eq!(
                got.physics_sign(),
                want_sign,
                "row {row}: signed_offset={signed_offset} \
                 want_sign={want_sign} got={got:?}"
            );
        }
    }

    #[test]
    fn even_height_splits_evenly() {
        // height 4, mid = 2. Row 2 sits on the equator;
        // row 0,1 are north; row 3 is south.
        let h = 4;
        assert_eq!(hemisphere_for_row(0, h), Hemisphere::North);
        assert_eq!(hemisphere_for_row(1, h), Hemisphere::North);
        assert_eq!(hemisphere_for_row(2, h), Hemisphere::Equator);
        assert_eq!(hemisphere_for_row(3, h), Hemisphere::South);
    }
}
