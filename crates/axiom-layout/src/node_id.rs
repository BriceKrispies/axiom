//! Stable caller-assigned identity for a layout node.

/// A caller-assigned id for a layout node, used to read its solved rectangle back
/// from a [`crate::LayoutResult`]. The app picks the raw values — typically one per
/// on-screen region (a board, a panel, a HUD bar) — so it can name each region's
/// result without threading indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(u32);

impl NodeId {
    /// A node id from a raw value the caller chooses.
    pub const fn from_raw(raw: u32) -> Self {
        NodeId(raw)
    }

    /// The raw value.
    pub const fn raw(self) -> u32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_its_raw_value() {
        assert_eq!(NodeId::from_raw(7).raw(), 7);
    }

    #[test]
    fn distinct_ids_are_unequal_and_ordered() {
        assert_ne!(NodeId::from_raw(1), NodeId::from_raw(2));
        assert!(NodeId::from_raw(1) < NodeId::from_raw(2));
        // Copy + Hash round-trip (used as a BTreeMap key in the result).
        let id = NodeId::from_raw(3);
        assert_eq!(id, id);
    }
}
