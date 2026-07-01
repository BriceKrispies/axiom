//! Stable identifier newtypes shared across the Growth substrate.
//!
//! These are deliberately plain so every subsystem (worldgen, atlas, streaming,
//! gameplay) names the same id types. Region/plate/triangle ids index into the
//! flat per-region arrays of [`crate::growth::model_planet::PlanetGlobe`]; chunk coords
//! address the streamed game world.

// The region (icosphere site) index is the topology's own vocabulary: it lives in
// the `axiom-geosphere` layer alongside the icosphere + region graph that hand it
// out, and every Growth subsystem names that one type.
pub use axiom_geosphere::RegionId;

// The streamed game world is addressed by integer chunk coordinates (16 m
// chunks). That coordinate is the vocabulary of the residency ring that streams
// them, so it lives in the `axiom-streaming` module alongside the ring that hands
// it out; every Growth subsystem names that one type. Audit: GW coordinate notes
// (`CHUNK_SIZE` = 16 cells, 1 m cells).
pub use axiom_streaming::ChunkCoord;

/// A tectonic plate index. Audit: worldgen `tectonic_plates`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct PlateId(pub u32);

/// A triangle (dual-mesh face) index. Audit: `triangle_values`, rivers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct TriangleId(pub u32);

/// A derived biome id. Audit: OW-E3 derived biome, `biomes.xml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct BiomeId(pub u32);
