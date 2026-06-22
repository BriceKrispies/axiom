//! A small, deterministic, local 64-bit hash (FNV-1a).
//!
//! These hashes are **diagnostics only** — byte equality is always the source of
//! truth for determinism. We implement the hash locally (no external crate, no
//! `std` `RandomState`) so it is fully deterministic and reproducible across
//! runs, platforms, and processes. Computation is branchless (a `fold`).

/// FNV-1a 64-bit offset basis.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64-bit prime.
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Deterministic FNV-1a hash of a byte slice. Empty input hashes to the offset
/// basis. Pure and branchless.
pub(crate) fn hash_bytes(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(FNV_OFFSET, |acc, &b| (acc ^ u64::from(b)).wrapping_mul(FNV_PRIME))
}

/// Deterministically fold a sequence of `u64` words into one hash (used to
/// combine the per-artifact hashes + frame identity into a `final_hash`).
pub(crate) fn hash_words(words: &[u64]) -> u64 {
    words
        .iter()
        .fold(FNV_OFFSET, |acc, &w| (acc ^ w).wrapping_mul(FNV_PRIME))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_is_the_offset_basis() {
        assert_eq!(hash_bytes(&[]), FNV_OFFSET);
        assert_eq!(hash_words(&[]), FNV_OFFSET);
    }

    #[test]
    fn hash_is_deterministic_for_the_same_bytes() {
        let a = hash_bytes(b"axiom-recording");
        let b = hash_bytes(b"axiom-recording");
        assert_eq!(a, b);
    }

    #[test]
    fn different_bytes_hash_differently() {
        assert_ne!(hash_bytes(b"alpha"), hash_bytes(b"beta"));
        // Order matters (FNV-1a is sequential).
        assert_ne!(hash_bytes(&[1, 2, 3]), hash_bytes(&[3, 2, 1]));
    }

    #[test]
    fn word_fold_is_deterministic_and_order_sensitive() {
        assert_eq!(hash_words(&[1, 2, 3]), hash_words(&[1, 2, 3]));
        assert_ne!(hash_words(&[1, 2, 3]), hash_words(&[3, 2, 1]));
    }
}
