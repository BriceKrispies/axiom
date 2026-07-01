//! Player intents routed to sim mutations. Dig is the first verb.
use crate::growth::ids::ChunkCoord;

/// A player intent.
#[derive(Debug, Clone)]
pub enum Intent {
    Dig {
        coord: ChunkCoord,
        lx: u32,
        lz: u32,
    },
    Place {
        coord: ChunkCoord,
        lx: u32,
        lz: u32,
        item: u32,
    },
    Influence {
        target: u64,
    },
}

/// Routes an intent to its handler.
#[derive(Debug, Default)]
pub struct IntentRouter;

impl IntentRouter {
    pub fn new() -> Self {
        Self
    }
}
