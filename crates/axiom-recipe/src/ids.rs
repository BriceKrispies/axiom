//! Stable identifiers for a recipe and its nodes.

/// A stable identifier for a whole recipe graph. Carried in the serialized bytes
/// so a player references a recipe by a durable id, not by file position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RecipeId(u64);

impl RecipeId {
    /// Construct from a raw value.
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// The raw value.
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// A stable identifier for a node within one recipe: its insertion index. Node
/// ids are dense (`0..node_count`) and a node's inputs reference only
/// strictly-smaller ids, so the graph is acyclic and evaluable in id order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(u32);

impl NodeId {
    /// The absence of a node — what a diagnostic reports when the failure is a
    /// property of the whole graph rather than of one node.
    ///
    /// `u32::MAX`, not `0`: node ids are dense insertion indices starting at
    /// `0`, so `0` is a real node here and the kernel's reserve-zero convention
    /// does not transfer. `u32::MAX` is unreachable because a graph is capped at
    /// `MAX_NODES` nodes.
    pub const NULL: NodeId = NodeId(u32::MAX);

    /// Construct from a raw index.
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    /// The raw index.
    pub const fn raw(self) -> u32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_and_order() {
        assert_eq!(RecipeId::from_raw(7).raw(), 7);
        assert_eq!(NodeId::from_raw(3).raw(), 3);
        assert!(NodeId::from_raw(1) < NodeId::from_raw(2));
        assert!(RecipeId::from_raw(1) < RecipeId::from_raw(2));
    }

    #[test]
    fn the_null_node_is_not_a_reachable_index() {
        assert_eq!(NodeId::NULL.raw(), u32::MAX);
        assert_ne!(NodeId::NULL, NodeId::from_raw(0));
        assert!(NodeId::from_raw(crate::MAX_NODES as u32) < NodeId::NULL);
    }
}
