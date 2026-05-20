//! Canonical hemisphere convention for the physics laws.
//!
//! ## Why this exists
//!
//! Multiple physics laws (`magnetism.rs`, `coriolis.rs`,
//! `radiation.rs`) and the recognition library
//! (`sim/recognition/src/lib.rs::Signature::Hemisphere`) share a
//! convention: **row 0 is the north pole, larger row index is
//! further south**. Equivalently:
//!
//! ```text
//!   signed_offset = axial.r - height/2
//!   signed_offset <  0   →   Northern hemisphere
//!   signed_offset >  0   →   Southern hemisphere
//!   signed_offset == 0   →   Equator
//! ```
//!
//! Each call site previously hand-rolled this with
//! `signed_offset.cmp(&0)`. The expert review flagged that
//! `world/src/climate.rs` accidentally uses the **opposite**
//! mapping (`row > mid` is "northern"); fixing that requires a
//! coordinated seed rebaseline (worldgen RNG draws shift). In the
//! meantime, this helper centralises the canonical convention used
//! by every law that already agrees, so future regressions can be
//! detected before they pile up.
//!
//! Determinism: this is a pure refactor. The helpers return the
//! same `i64` signed offsets and `Hemisphere` discriminants the
//! existing laws already compute.

/// Which hemisphere a cell belongs to under the canonical
/// physics convention (row 0 = north pole).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hemisphere {
    North,
    South,
    Equator,
}

impl Hemisphere {
    /// Sign used by `Coriolis` and `Magnetism` for the
    /// hemisphere-mirror term: `+1` for northern, `-1` for southern,
    /// `0` for equatorial. Matches the existing `match signed_offset
    /// { Less => +1, Greater => -1, Equal => 0 }` branches.
    #[must_use]
    pub fn sign(self) -> i64 {
        match self {
            Hemisphere::North => 1,
            Hemisphere::South => -1,
            Hemisphere::Equator => 0,
        }
    }
}

/// Canonical hemisphere helper: row 0 = north pole, larger row =
/// further south. Equator sits at exactly `row == height / 2` (the
/// existing integer-division `half_h = height_i / 2` boundary).
///
/// Returns the same `Hemisphere::North` / `South` / `Equator`
/// discriminant the laws were already computing inline. The helper
/// is provided so future call sites quote one canonical convention
/// rather than re-deriving the sign branches.
#[must_use]
pub fn hemisphere_for_row(row: i64, height: u32) -> Hemisphere {
    let half_h = i64::from(height) / 2;
    let signed_offset = row - half_h;
    if signed_offset < 0 {
        Hemisphere::North
    } else if signed_offset > 0 {
        Hemisphere::South
    } else {
        Hemisphere::Equator
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_zero_is_north_pole() {
        assert_eq!(hemisphere_for_row(0, 9), Hemisphere::North);
    }

    #[test]
    fn middle_row_is_equator() {
        // height 9 → half_h = 4 → row 4 is the equator.
        assert_eq!(hemisphere_for_row(4, 9), Hemisphere::Equator);
    }

    #[test]
    fn high_row_is_south() {
        assert_eq!(hemisphere_for_row(8, 9), Hemisphere::South);
    }

    #[test]
    fn matches_existing_magnetism_branch() {
        // The same `signed_offset.cmp(&0)` branches the laws use:
        //   Less => +1 (north), Greater => -1 (south), Equal => 0.
        let height = 7u32;
        let half_h = (height as i64) / 2;
        for r in 0..(height as i64) {
            let signed_offset = r - half_h;
            let want_sign = match signed_offset.cmp(&0) {
                std::cmp::Ordering::Less => 1,
                std::cmp::Ordering::Greater => -1,
                std::cmp::Ordering::Equal => 0,
            };
            assert_eq!(
                hemisphere_for_row(r, height).sign(),
                want_sign,
                "row {r}: helper sign disagrees with raw signed_offset"
            );
        }
    }
}
