//! Authoritative streamed-chunk store: pairs the reusable residency ring
//! (`axiom_streaming::Residency`) with the app-owned chunk payloads.
//!
//! The ring owns *which* coordinates are resident and the deterministic
//! load/unload delta as the focus moves; this store owns *what* each resident
//! coordinate holds (its generated [`Chunk`] heights + edit state) and turns the
//! ring's delta into the app's presentation [`Diff`]s. Audit: GW vision, GW-E3
//! (persist edits), GW-E9 (sim unload).
use std::collections::HashMap;

use axiom_streaming::{ChunkCoord, Residency};

use crate::model_planet::PlanetSurfaceAtlas;
use crate::model_world::{Chunk, Diff, GameWorldLocalMap};

/// Stream radius in chunks. Audit: GW (k_stream_radius_chunks).
pub const STREAM_RADIUS_CHUNKS: i32 = 2;

/// Eviction hysteresis: a chunk is only unloaded once it is beyond
/// `STREAM_RADIUS_CHUNKS + STREAM_UNLOAD_MARGIN`, so a player pacing across a
/// chunk boundary does not thrash-reload. Audit: GW-E9.
pub const STREAM_UNLOAD_MARGIN: i32 = 1;

#[derive(Debug, Default)]
pub struct ChunkStore {
    /// Which coordinates are resident + the load/unload delta authority.
    residency: Residency,
    /// The payload for each resident coordinate (heights + edit state).
    loaded: HashMap<ChunkCoord, Chunk>,
}

impl ChunkStore {
    pub fn new() -> Self {
        Self {
            residency: Residency::new(),
            loaded: HashMap::new(),
        }
    }

    pub fn loaded_count(&self) -> usize {
        self.loaded.len()
    }

    pub fn get(&self, coord: ChunkCoord) -> Option<&Chunk> {
        self.loaded.get(&coord)
    }

    pub fn get_mut(&mut self, coord: ChunkCoord) -> Option<&mut Chunk> {
        self.loaded.get_mut(&coord)
    }

    /// Stream chunks for a new focus `center`: load the coordinates the residency
    /// ring newly admits (generating each [`Chunk`] payload and emitting a
    /// `ChunkLoaded`), and unload the coordinates it evicts beyond
    /// `radius + margin` (dropping the payload and emitting a `ChunkUnloaded`).
    /// Edited chunks are marked dirty to the ring, so they are never evicted.
    /// Audit: GW-E3 (persist edits), GW-E9 (sim unload).
    #[allow(clippy::too_many_arguments)] // app-tier streaming entry, moved as-is in the gallery de-merge
    pub fn stream(
        &mut self,
        center: ChunkCoord,
        radius: i32,
        margin: i32,
        atlas: &PlanetSurfaceAtlas,
        localmap: &GameWorldLocalMap,
        seed: u64,
        out: &mut Vec<Diff>,
    ) {
        let loaded = &self.loaded;
        let delta = self.residency.apply(center, radius, margin, |c| {
            loaded.get(&c).map(|ch| ch.edited).unwrap_or(false)
        });
        for coord in delta.load {
            let chunk = crate::gameworld::generate_chunk(coord, atlas, localmap, seed);
            out.push(Diff::ChunkLoaded {
                coord,
                heights: chunk.height_samples.clone(),
            });
            self.loaded.insert(coord, chunk);
        }
        for coord in delta.unload {
            self.loaded.remove(&coord);
            out.push(Diff::ChunkUnloaded { coord });
        }
    }
}
