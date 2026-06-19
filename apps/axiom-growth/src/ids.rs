//! Stable identifier newtypes shared across the Growth substrate.
//!
//! These are deliberately plain so every subsystem (worldgen, atlas, streaming,
//! gameplay) names the same id types. Region/plate/triangle ids index into the
//! flat per-region arrays of [`crate::model_planet::PlanetGlobe`]; chunk coords
//! address the streamed game world.

/// A region (icosphere site) index on the overworld. Audit: OW-E1/E3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct RegionId(pub u32);

/// A tectonic plate index. Audit: worldgen `tectonic_plates`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct PlateId(pub u32);

/// A triangle (dual-mesh face) index. Audit: `triangle_values`, rivers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct TriangleId(pub u32);

/// A derived biome id. Audit: OW-E3 derived biome, `biomes.xml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct BiomeId(pub u32);

/// Integer chunk coordinate in the streamed game world (16 m chunks).
/// Audit: GW coordinate notes (`CHUNK_SIZE` = 16 cells, 1 m cells).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ChunkCoord {
    pub x: i32,
    pub z: i32,
}

impl ChunkCoord {
    pub fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }
}

impl RegionId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}
