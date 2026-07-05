//! One tag entry inside a [`crate::SceneSnapshot`].

use crate::scene_node_id::SceneNodeId;

/// One coarse-semantic-kind entry in a deterministic scene snapshot, keyed by
/// its node. Rolling `Tag` into the snapshot means perception and render see the
/// **same** scene — a consumer reads "what is this thing" (wall / enemy / door…)
/// straight off the snapshot instead of re-deriving kind app-side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TagSnapshot {
    node: SceneNodeId,
    kind_code: u32,
}

impl TagSnapshot {
    pub const fn new(node: SceneNodeId, kind_code: u32) -> Self {
        TagSnapshot { node, kind_code }
    }

    pub const fn node(&self) -> SceneNodeId {
        self.node
    }

    /// The coarse kind code this node was tagged with (vocabulary owned by the app).
    pub const fn kind_code(&self) -> u32 {
        self.kind_code
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_round_trip() {
        let t = TagSnapshot::new(SceneNodeId::from_raw(4), 7);
        assert_eq!(t.node().raw(), 4);
        assert_eq!(t.kind_code(), 7);
    }

    #[test]
    fn equality_requires_all_fields() {
        let a = TagSnapshot::new(SceneNodeId::from_raw(1), 2);
        let b = TagSnapshot::new(SceneNodeId::from_raw(1), 2);
        let c = TagSnapshot::new(SceneNodeId::from_raw(1), 3);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
