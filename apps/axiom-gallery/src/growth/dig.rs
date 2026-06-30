//! Dig/terraform handler: lowers a cell, marks the chunk edited, emits a diff,
//! yields material. Audit: GW-E4/E11/E15. Scaffold (M1).
use crate::growth::chunkstore::ChunkStore;
use crate::growth::ids::ChunkCoord;
use crate::growth::inventory::Inventory;
use crate::growth::model_world::{Diff, CHUNK_VERT_SIDE};

/// Apply a dig at a chunk cell: lower height, mark edited, emit CellChanged,
/// add yield to inventory. Audit: GW-E4/E11/E15. Scaffold.
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
    inventory.add(0, 1); // yield: 1 unit of material id 0. Audit: GW-E15 (table TBD).
    out.push(Diff::CellChanged {
        coord,
        lx,
        lz,
        new_height: new_h,
    });
    true
}
