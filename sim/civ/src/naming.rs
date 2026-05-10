//! Deterministic civilisation naming.
//!
//! Each `(seed, civ_id)` pair maps to one of `STEMS.len() * ENDINGS.len()`
//! kingdom-feeling names — same input pair, same output, byte-for-byte
//! across runs.

/// Deterministic civilisation name from `(seed, civ_id)`. 64
/// kingdom-feeling stems × 6 endings → 384 distinct names before
/// collisions. Same `(seed, civ_id)` always picks the same name —
/// byte-for-byte across runs. Uses a magic constant distinct from
/// the planet-name pool and the species-name pool so all
/// three streams pick independently (so `Eldoria` on a `Vela-c`
/// planet hosting `Pyrrites` is chosen by three separate hashes,
/// not chained through a shared seed).
///
/// The pool feels political — Sumer / Akkad / Babylon / Persia /
/// Carthage — rather than scientific or fantastical, so the
/// reader's mental model frames each civ as a kingdom-or-empire
/// distinct from neighbours, not a faction-id with a sticker.
#[must_use]
pub fn civ_name_from_seed(seed: u64, civ_id: u32) -> String {
    const STEMS: [&str; 64] = [
        "Eldor", "Karn", "Verum", "Volkar", "Aurel", "Korath", "Thalas", "Mira", "Nyx", "Drak",
        "Sard", "Iron", "Stein", "Brel", "Dal", "Faer", "Gor", "Hal", "Iza", "Jak", "Kalor",
        "Lurin", "Marn", "Nor", "Olum", "Pyr", "Quin", "Ras", "Sumer", "Tarn", "Ulth", "Vask",
        "Wend", "Xand", "Yoth", "Zar", "Akkad", "Babyl", "Cendr", "Dorn", "Esh", "Fyr", "Garn",
        "Helmar", "Idris", "Jorund", "Kossak", "Lyon", "Morv", "Nemed", "Orel", "Persa", "Quor",
        "Ryne", "Skara", "Tarsh", "Urum", "Verg", "Wraek", "Xerxa", "Yaran", "Zoran", "Cathar",
        "Bolm",
    ];
    const ENDINGS: [&str; 6] = ["ia", "ath", "is", "on", "an", "ic"];
    // Distinct magic constant from the planet-name pool (no XOR salt — uses raw seed)
    // and the species-name pool (`0xFEED_FACE_BAAD_F00D`). The civ id mixes in via XOR
    // before the modulo so neighbouring civ_ids on the same seed land
    // in different stem buckets — adjacent ids `1` and `2` would
    // otherwise both pick the same stem if only the seed drove the
    // hash.
    let mixed = seed ^ u64::from(civ_id) ^ 0xC1A5_5C0D_E152_BEEF;
    let stem_idx = usize::try_from(mixed % (STEMS.len() as u64)).unwrap_or(0);
    let end_idx = usize::try_from((mixed >> 6) % (ENDINGS.len() as u64)).unwrap_or(0);
    format!("{}{}", STEMS[stem_idx], ENDINGS[end_idx])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Same `(seed, civ_id)` always picks the same name.
    /// Determinism is the simulator's hard contract — replays must
    /// see byte-identical names. Verified for several seeds × ids.
    #[test]
    fn civ_name_from_seed_is_deterministic() {
        for seed in [0u64, 1, 42, 12345, u64::MAX, 0xDEAD_BEEF_CAFE_BABE] {
            for civ_id in [0u32, 1, 2, 7, 42, u32::MAX] {
                let a = civ_name_from_seed(seed, civ_id);
                let b = civ_name_from_seed(seed, civ_id);
                let c = civ_name_from_seed(seed, civ_id);
                assert_eq!(a, b, "seed={seed} civ_id={civ_id}");
                assert_eq!(b, c, "seed={seed} civ_id={civ_id}");
                assert!(!a.is_empty(), "seed={seed} civ_id={civ_id} produced empty");
            }
        }
    }

    /// `civ_ids` 1..=16 on a single seed produce mostly-distinct
    /// names. Pool size is 64 × 6 = 384 so 16 ids should very rarely
    /// collide; we tolerate up to 4 collisions to give the hash room
    /// without flagging false-positive failures.
    #[test]
    fn civ_name_from_seed_has_low_collision_rate() {
        for seed in [0u64, 1, 42, 12345, u64::MAX] {
            let mut names: Vec<String> = (1..=16)
                .map(|civ_id| civ_name_from_seed(seed, civ_id))
                .collect();
            names.sort();
            let total = names.len();
            names.dedup();
            let unique = names.len();
            let collisions = total - unique;
            assert!(
                collisions <= 4,
                "seed={seed} produced {collisions} collisions across 16 civ_ids: too many"
            );
        }
    }

    /// The seed and `civ_id` both contribute to the chosen name —
    /// changing either one (in isolation) changes the resulting name
    /// for at least most cases. Sanity check that the magic XOR
    /// actually mixes both inputs.
    #[test]
    fn civ_name_from_seed_uses_both_inputs() {
        let a = civ_name_from_seed(42, 1);
        let b = civ_name_from_seed(42, 2);
        let c = civ_name_from_seed(43, 1);
        // Not strictly required to differ for every triple, but for
        // these particular small inputs the hash should split them.
        assert_ne!(a, b, "civ_id should affect name");
        assert_ne!(a, c, "seed should affect name");
    }
}
