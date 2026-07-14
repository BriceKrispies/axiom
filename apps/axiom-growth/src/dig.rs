//! Dig/terraform handler: lowers a cell, marks the chunk edited, emits a diff,
//! yields material.
use crate::chunkstore::ChunkStore;
use crate::ids::ChunkCoord;
use crate::inventory::Inventory;
use crate::model_world::{Diff, CHUNK_VERT_SIDE};

pub fn apply_dig(
    store: &mut ChunkStore,
    inventory: &mut Inventory,
    coord: ChunkCoord,
    lx: u32,
    lz: u32,
    out: &mut Vec<Diff>,
) -> bool {
    let Some(chunk) = store.get_mut(coord) else {
        return false;
    };
    if lx as usize >= CHUNK_VERT_SIDE || lz as usize >= CHUNK_VERT_SIDE {
        return false;
    }
    let new_h = chunk.height_at(lx as usize, lz as usize) - 1.0;
    chunk.set_height(lx as usize, lz as usize, new_h);
    chunk.edited = true;
    inventory.add(0, 1);
    out.push(Diff::CellChanged {
        coord,
        lx,
        lz,
        new_height: new_h,
    });
    true
}
