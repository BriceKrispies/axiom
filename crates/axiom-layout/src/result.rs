//! The solver's output: one placed rectangle per node, keyed by [`NodeId`].

use std::collections::BTreeMap;

use crate::layout_rect::LayoutRect;
use crate::node_id::NodeId;

/// The solved layout: a placed [`LayoutRect`] for every node, looked up by the
/// [`NodeId`] the caller assigned. A `BTreeMap` keeps iteration deterministic. An
/// app reads `rect(id)` for each region it cares about and applies it to its
/// surface (canvas size, DOM box, HUD quad).
#[derive(Debug, Clone, Default)]
pub struct LayoutResult {
    rects: BTreeMap<NodeId, LayoutRect>,
}

impl LayoutResult {
    pub(crate) fn new() -> Self {
        LayoutResult {
            rects: BTreeMap::new(),
        }
    }

    /// Record (or overwrite) a node's solved rect. Crate-internal: only the solver
    /// fills the result.
    pub(crate) fn insert(&mut self, id: NodeId, rect: LayoutRect) {
        self.rects.insert(id, rect);
    }

    /// The solved rect for `id`, or `None` if the id was never placed (an unknown
    /// id, or a node whose parent was missing).
    pub fn rect(&self, id: NodeId) -> Option<LayoutRect> {
        self.rects.get(&id).copied()
    }

    /// The number of placed nodes.
    pub fn len(&self) -> usize {
        self.rects.len()
    }

    /// Whether nothing was placed.
    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_returns_rects_by_id() {
        let mut r = LayoutResult::new();
        assert!(r.is_empty());
        let rect = LayoutRect::from_edges(1.0, 2.0, 3.0, 4.0);
        r.insert(NodeId::from_raw(9), rect);
        assert_eq!(r.len(), 1);
        assert_eq!(r.rect(NodeId::from_raw(9)), Some(rect));
        assert_eq!(r.rect(NodeId::from_raw(0)), None);
    }

    #[test]
    fn default_is_empty() {
        let r = LayoutResult::default();
        assert!(r.is_empty());
    }
}
