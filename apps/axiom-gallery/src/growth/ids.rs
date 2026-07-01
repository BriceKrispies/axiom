//! Stable identifier newtypes shared across the Growth substrate.
//!
//! These are deliberately plain so every subsystem (worldgen, atlas, streaming,
//! gameplay) names the same id types. Region/plate/triangle ids index into the
//! flat per-region arrays of [`crate::growth::model_planet::PlanetGlobe`]; chunk coords
//! address the streamed game world.

/// A region (icosphere site) index on the overworld.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct RegionId(pub u32);

/// A tectonic plate index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct PlateId(pub u32);

/// A triangle (dual-mesh face) index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct TriangleId(pub u32);

/// A derived biome id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct BiomeId(pub u32);

/// Integer chunk coordinate in the streamed game world (16 m chunks).
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
