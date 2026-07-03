//! The pure value-type vocabulary `WorldApi` traffics in.

use axiom_kernel::Meters;
use axiom_streaming::ChunkCoord;

/// How a streaming world is sized and paced. Held by a [`crate::WorldApi`] and
/// consulted every frame.
#[derive(Debug, Clone)]
pub struct WorldConfig {
    /// World size of one chunk's side (metres). The camera's focus chunk is its
    /// ground position divided by this.
    pub chunk_size: Meters,
    /// Residency load ring radius, in chunks: the `[-load_radius, load_radius]²`
    /// square of chunks around the focus is kept loaded.
    pub load_radius: i32,
    /// Residency hysteresis margin, in chunks: a loaded chunk is only evicted
    /// once it falls outside the wider `load_radius + margin` keep square, so a
    /// focus jittering across a boundary does not thrash.
    pub margin: i32,
    /// Ascending LOD distance bands (metres): a visible chunk's level is how many
    /// of these its camera distance exceeds (`0` = nearest / highest detail).
    pub lod_bands: Vec<Meters>,
}

/// A loaded chunk the camera can see this frame, tagged with its level of detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibleChunk {
    /// Which chunk.
    pub coord: ChunkCoord,
    /// Its level of detail this frame (`0` = nearest / highest detail).
    pub lod: u8,
}

/// The plan for one frame: what to load, what to unload, and what to draw.
///
/// `load` / `unload` are the residency delta (the caller generates the loaded
/// chunks' payloads and tears down the unloaded ones); `visible` is the subset of
/// currently-loaded chunks inside the camera frustum, each with its LOD.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorldFramePlan {
    /// Chunks newly entering the load ring — generate their payloads.
    pub load: Vec<ChunkCoord>,
    /// Chunks leaving the keep ring — tear their payloads down.
    pub unload: Vec<ChunkCoord>,
    /// Loaded chunks the camera sees this frame, each with its LOD level.
    pub visible: Vec<VisibleChunk>,
}
