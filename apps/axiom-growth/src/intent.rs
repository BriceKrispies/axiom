//! Player intents routed to sim mutations. Audit: GW-E10 IntentRouter,
//! "Ecology/gameplay" intents. Scaffold: dig is the first verb (M1).
use crate::ids::ChunkCoord;

/// A player intent. Audit: interactions.xml / apply_intent.
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

/// Routes an intent to its handler. Audit: GW-E10/E11. Scaffold.
#[derive(Debug, Default)]
pub struct IntentRouter;

impl IntentRouter {
    pub fn new() -> Self {
        Self
    }
}
