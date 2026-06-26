//! [`StableHash`] — a deterministic, platform-stable 64-bit digest over canonical
//! bytes (FNV-1a).
//!
//! The kernel owns this because a stable digest over **canonical bytes** is a
//! broadly-shared, kernel-shaped primitive: every layer that serializes a value
//! (through [`crate::BinaryWriter`]/[`crate::Reflect`]) can index and label those
//! bytes with a digest, and independent producers — the `recording` module's
//! determinism reports, and the procedural-generation layers' artifact/trace
//! provenance — must all compute the *same* digest for the *same* bytes. A single
//! kernel primitive guarantees that; a per-module copy would not.
//!
//! It is a **diagnostic index, never the proof**: byte equality remains the
//! source of truth for determinism (the stance the engine already takes in
//! `modules/axiom-recording`); a digest only labels and locates bytes, so a hash
//! match is a hint and a byte match is the verdict. Pure and branchless (a
//! `fold`); no ambient state, no platform dependence (FNV-1a over the same
//! little-endian canonical encodings everywhere).

/// FNV-1a 64-bit offset basis.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64-bit prime.
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// A stable 64-bit digest of canonical bytes. Deterministic across runs,
/// processes, and platforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StableHash(u64);

impl StableHash {
    /// Wrap a raw digest value — e.g. one read back from a stored golden or a
    /// provenance record.
    pub const fn from_raw(raw: u64) -> Self {
        StableHash(raw)
    }

    /// The raw 64-bit digest.
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// Digest a byte slice. Empty input digests to the FNV offset basis. Pure,
    /// branchless (a `fold`).
    pub fn of_bytes(bytes: &[u8]) -> Self {
        StableHash(bytes.iter().fold(FNV_OFFSET, |acc, &b| {
            (acc ^ u64::from(b)).wrapping_mul(FNV_PRIME)
        }))
    }

    /// Digest a sequence of 64-bit words — the order-sensitive way to fold
    /// already-computed digests (e.g. a list of per-artifact hashes) into one
    /// combined digest. Pure, branchless.
    pub fn of_words(words: &[u64]) -> Self {
        StableHash(
            words
                .iter()
                .fold(FNV_OFFSET, |acc, &w| (acc ^ w).wrapping_mul(FNV_PRIME)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_is_the_offset_basis() {
        assert_eq!(StableHash::of_bytes(&[]).raw(), FNV_OFFSET);
        assert_eq!(StableHash::of_words(&[]).raw(), FNV_OFFSET);
    }

    #[test]
    fn of_bytes_is_deterministic_and_byte_sensitive() {
        assert_eq!(
            StableHash::of_bytes(b"axiom"),
            StableHash::of_bytes(b"axiom")
        );
        assert_ne!(
            StableHash::of_bytes(b"alpha"),
            StableHash::of_bytes(b"beta")
        );
        // Order matters (FNV-1a is sequential).
        assert_ne!(
            StableHash::of_bytes(&[1, 2, 3]),
            StableHash::of_bytes(&[3, 2, 1])
        );
    }

    #[test]
    fn a_single_bit_flip_changes_the_digest() {
        let base = StableHash::of_bytes(&[0b0000_0000]);
        let flipped = StableHash::of_bytes(&[0b0000_0001]);
        assert_ne!(base, flipped);
    }

    #[test]
    fn of_words_is_deterministic_and_order_sensitive() {
        assert_eq!(
            StableHash::of_words(&[1, 2, 3]),
            StableHash::of_words(&[1, 2, 3])
        );
        assert_ne!(
            StableHash::of_words(&[1, 2, 3]),
            StableHash::of_words(&[3, 2, 1])
        );
    }

    #[test]
    fn from_raw_and_raw_round_trip() {
        assert_eq!(StableHash::from_raw(0xDEAD_BEEF).raw(), 0xDEAD_BEEF);
        // A digest equals its own raw value re-wrapped.
        let digest = StableHash::of_bytes(b"axiom");
        assert_eq!(StableHash::from_raw(digest.raw()), digest);
    }

    #[test]
    fn value_semantics_clone_copy_and_debug() {
        let digest = StableHash::of_bytes(b"axiom");
        let copied = digest;
        assert_eq!(copied, digest.clone());
        assert!(!format!("{digest:?}").is_empty());
    }
}
