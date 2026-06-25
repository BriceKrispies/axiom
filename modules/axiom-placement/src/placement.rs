//! [`Placement`] — a deterministic set of integer grid positions.

use axiom_kernel::{BinaryWriter, StableHash};

/// A scatter result: object positions on an integer grid, in generation order.
/// Neutral data — *what* is placed at each cell is a caller's concern, not this
/// module's. Returned by [`crate::PlacementApi`]; accessed through its methods.
#[derive(Debug, PartialEq, Eq)]
pub struct Placement {
    positions: Vec<(u32, u32)>,
}

impl Placement {
    pub(crate) fn new(positions: Vec<(u32, u32)>) -> Self {
        Placement { positions }
    }

    /// The placed positions, in generation order.
    pub fn positions(&self) -> &[(u32, u32)] {
        &self.positions
    }

    /// How many objects were placed.
    pub fn len(&self) -> usize {
        self.positions.len()
    }

    /// Whether nothing was placed.
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    /// The canonical bytes: count, then each `(x, y)` little-endian.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        writer.write_u64(self.positions.len() as u64);
        self.positions.iter().for_each(|&(x, y)| {
            writer.write_u32(x);
            writer.write_u32(y);
        });
        writer.into_bytes()
    }

    /// The stable digest over [`Self::to_bytes`].
    pub fn digest(&self) -> StableHash {
        StableHash::of_bytes(&self.to_bytes())
    }
}
