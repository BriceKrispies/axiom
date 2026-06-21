//! Authoritative streamed-chunk store with focus-radius load/unload and edit
//! preservation. Audit: GW vision, GW-E3 (persist edits), GW-E9 (sim unload).
use std::collections::HashMap;

use crate::ids::ChunkCoord;
use crate::model_planet::PlanetSurfaceAtlas;
use crate::model_world::{Chunk, Diff, GameWorldLocalMap};

/// Stream radius in chunks. Audit: GW (k_stream_radius_chunks).
pub const STREAM_RADIUS_CHUNKS: i32 = 2;

#[derive(Debug, Default)]
pub struct ChunkStore {
    loaded: HashMap<ChunkCoord, Chunk>,
}

impl ChunkStore {
    pub fn new() -> Self {
        Self {
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

    /// Ensure chunks within `radius` of `center` are loaded; emit ChunkLoaded
    /// diffs for newly generated ones. Preserves edited chunks. Audit: GW-E3.
    pub fn request(
        &mut self,
        center: ChunkCoord,
        radius: i32,
        atlas: &PlanetSurfaceAtlas,
        localmap: &GameWorldLocalMap,
        seed: u64,
        out: &mut Vec<Diff>,
    ) {
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let c = ChunkCoord::new(center.x + dx, center.z + dz);
                if self.loaded.contains_key(&c) {
                    continue;
                }
                let chunk = crate::gameworld::generate_chunk(c, atlas, localmap, seed);
                out.push(Diff::ChunkLoaded {
                    coord: c,
                    heights: chunk.height_samples.clone(),
                });
                self.loaded.insert(c, chunk);
            }
        }
    }

    /// Unload chunks outside `radius + margin`; preserve edited ones. Audit: GW-E9.
    pub fn unload_far(
        &mut self,
        center: ChunkCoord,
        radius: i32,
        margin: i32,
        out: &mut Vec<Diff>,
    ) {
        let keep = radius + margin;
        let mut remove = Vec::new();
        for (&c, chunk) in &self.loaded {
            let outside = (c.x - center.x).abs() > keep || (c.z - center.z).abs() > keep;
            if outside && !chunk.edited {
                remove.push(c);
            }
        }
        for c in remove {
            self.loaded.remove(&c);
            out.push(Diff::ChunkUnloaded { coord: c });
        }
    }
}
