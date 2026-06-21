//! Shared game-world (streamed, metre-scale) data model.
//!
//! Audit: "Streaming/game-world requirements". The overworld atlas is read-only
//! macro context; the game world is a second metre-scale pass streamed as
//! chunks around the player and is player-editable. `ChunkStore` is the
//! authoritative source of truth; presentation consumes diffs.

use crate::ids::ChunkCoord;

/// Cells per chunk side. Audit: GW coordinate notes (`CHUNK_SIZE` = 16).
pub const CHUNK_SIZE_CELLS: usize = 16;
/// Height samples per side (cells + 1). Audit: GW-7.1 (17×17 for 16×16).
pub const CHUNK_VERT_SIDE: usize = CHUNK_SIZE_CELLS + 1;
/// Metres per cell. Audit: GW coordinate notes (1 m cells → 256 m/chunk… 16 m here).
pub const CELL_SIZE_M: f32 = 1.0;

/// Tangent frame mapping chunk/world-metre coordinates to a unit direction on
/// the planet, anchored at a chosen play location. Audit: GW-E1 `GameWorldLocalMap`.
#[derive(Debug, Clone, Default)]
pub struct GameWorldLocalMap {
    /// Anchor point on the unit sphere (play start). Audit: GW-E1 anchor.
    pub anchor_dir: [f32; 3],
    /// East/north tangent basis at the anchor (unit).
    pub tangent_east: [f32; 3],
    pub tangent_north: [f32; 3],
    pub planet_radius_m: f32,
}

/// One streamed chunk. Authoritative cell state lives here. Audit: GW vision.
#[derive(Debug, Clone)]
pub struct Chunk {
    pub coord: ChunkCoord,
    /// Row-major height grid, `CHUNK_VERT_SIDE * CHUNK_VERT_SIDE` samples (m).
    /// Audit: GW-E2 atlas-shaped heights, GW-E19 smooth/seam-coherent.
    pub height_samples: Vec<f32>,
    /// Whether the player has edited this chunk (preserve on re-request).
    /// Audit: GW-E3 edits persist.
    pub edited: bool,
}

impl Chunk {
    pub fn new(coord: ChunkCoord) -> Self {
        Self {
            coord,
            height_samples: vec![0.0; CHUNK_VERT_SIDE * CHUNK_VERT_SIDE],
            edited: false,
        }
    }

    pub fn height_at(&self, lx: usize, lz: usize) -> f32 {
        self.height_samples[lz * CHUNK_VERT_SIDE + lx]
    }

    pub fn set_height(&mut self, lx: usize, lz: usize, h: f32) {
        self.height_samples[lz * CHUNK_VERT_SIDE + lx] = h;
    }
}

/// A diff emitted to presentation. Audit: GW DiffQueue (ChunkLoaded/Unloaded/CellChanged).
#[derive(Debug, Clone)]
pub enum Diff {
    ChunkLoaded {
        coord: ChunkCoord,
        heights: Vec<f32>,
    },
    ChunkUnloaded {
        coord: ChunkCoord,
    },
    CellChanged {
        coord: ChunkCoord,
        lx: u32,
        lz: u32,
        new_height: f32,
    },
}
