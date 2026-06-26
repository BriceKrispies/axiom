//! [`World`] — a composed procedural world: terrain, biomes, and placed objects.

use axiom_kernel::{BinaryWriter, StableHash};

/// A composed world: a `width × height` grid of terrain heights and biome codes,
/// plus a set of placed object positions. Neutral data the caller renders or
/// simulates — `levelgen` translates the three domain modules' contracts into this
/// one shape (it cannot *name* their internal types, so it stores the read-out
/// values). Returned by [`crate::LevelGenApi`] and read through its methods.
#[derive(Debug, PartialEq, Eq)]
pub struct World {
    width: u32,
    height: u32,
    heights: Vec<i32>,
    biomes: Vec<u8>,
    objects: Vec<(u32, u32)>,
}

impl World {
    pub(crate) fn new(
        width: u32,
        height: u32,
        heights: Vec<i32>,
        biomes: Vec<u8>,
        objects: Vec<(u32, u32)>,
    ) -> Self {
        World {
            width,
            height,
            heights,
            biomes,
            objects,
        }
    }

    /// The world width in cells.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// The world height in cells.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Terrain heights, row-major.
    pub fn heights(&self) -> &[i32] {
        &self.heights
    }

    /// Biome codes, row-major (one per cell).
    pub fn biomes(&self) -> &[u8] {
        &self.biomes
    }

    /// Placed object positions.
    pub fn objects(&self) -> &[(u32, u32)] {
        &self.objects
    }

    /// The canonical bytes: dimensions, then the heights, biomes, and objects.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        writer.write_u32(self.width);
        writer.write_u32(self.height);
        writer.write_u64(self.heights.len() as u64);
        self.heights
            .iter()
            .for_each(|&h| writer.write_u32(h as u32));
        writer.write_u64(self.biomes.len() as u64);
        self.biomes
            .iter()
            .for_each(|&b| writer.write_u32(u32::from(b)));
        writer.write_u64(self.objects.len() as u64);
        self.objects.iter().for_each(|&(x, y)| {
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
