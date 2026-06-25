//! [`BiomeMap`] — a deterministic field of biome codes.

use axiom_kernel::{BinaryWriter, StableHash};

/// A field of biome codes, one per cell, in generation order. A code is a small
/// `u8` (see [`crate::BiomeApi`]'s named constants). Returned by the facade and
/// read through its methods.
#[derive(Debug, PartialEq, Eq)]
pub struct BiomeMap {
    codes: Vec<u8>,
}

impl BiomeMap {
    pub(crate) fn new(codes: Vec<u8>) -> Self {
        BiomeMap { codes }
    }

    /// The biome codes, in generation order.
    pub fn codes(&self) -> &[u8] {
        &self.codes
    }

    /// How many cells were classified.
    pub fn len(&self) -> usize {
        self.codes.len()
    }

    /// Whether the map is empty.
    pub fn is_empty(&self) -> bool {
        self.codes.is_empty()
    }

    /// The canonical bytes: count, then each code (as a little-endian `u32`).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        writer.write_u64(self.codes.len() as u64);
        self.codes
            .iter()
            .for_each(|&code| writer.write_u32(u32::from(code)));
        writer.into_bytes()
    }

    /// The stable digest over [`Self::to_bytes`].
    pub fn digest(&self) -> StableHash {
        StableHash::of_bytes(&self.to_bytes())
    }
}
