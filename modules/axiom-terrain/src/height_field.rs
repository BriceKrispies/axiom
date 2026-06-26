//! [`HeightField`] — a deterministic grid of integer terrain heights.

use axiom_kernel::{BinaryWriter, StableHash};

/// A `width × height` grid of integer heights, row-major. Neutral data — heights
/// are unitless integers; turning them into world geometry (a mesh, voxels) is a
/// caller's concern, not this module's. Returned by [`crate::TerrainApi`] and read
/// through its methods.
#[derive(Debug, PartialEq, Eq)]
pub struct HeightField {
    width: u32,
    height: u32,
    heights: Vec<i32>,
}

impl HeightField {
    pub(crate) fn new(width: u32, height: u32, heights: Vec<i32>) -> Self {
        HeightField {
            width,
            height,
            heights,
        }
    }

    /// The grid width in cells.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// The grid height in cells.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The height at cell `(cx, cy)`. An out-of-range cell reads `0` — branchless
    /// and panic-free, and a too-wide `cx` never wraps into the next row.
    pub fn at(&self, cx: u32, cy: u32) -> i32 {
        let valid = (cx < self.width) & (cy < self.height);
        let index = valid.then(|| (cy as usize) * (self.width as usize) + (cx as usize));
        index
            .and_then(|i| self.heights.get(i))
            .copied()
            .unwrap_or(0)
    }

    /// The heights, row-major.
    pub fn heights(&self) -> &[i32] {
        &self.heights
    }

    /// The canonical bytes: width, height, count, then each height as little-endian
    /// `u32` (its two's-complement bit pattern).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        writer.write_u32(self.width);
        writer.write_u32(self.height);
        writer.write_u64(self.heights.len() as u64);
        self.heights
            .iter()
            .for_each(|&h| writer.write_u32(h as u32));
        writer.into_bytes()
    }

    /// The stable digest over [`Self::to_bytes`].
    pub fn digest(&self) -> StableHash {
        StableHash::of_bytes(&self.to_bytes())
    }
}
