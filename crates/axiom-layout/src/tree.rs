//! The flat, topologically-ordered node tree the solver walks.
//!
//! Nodes are stored in a flat `Vec` in insertion order, with each node holding its
//! parent's **index**. The builder guarantees the spine invariant a recursion-free
//! solver needs: a parent is always added before its children, so a parent's index
//! is strictly less than any of its children's. The solver can then assign rects in
//! a single index-order pass (a parent lays out its children before they are
//! reached), exactly like the scene layer's transform propagation.

use crate::layout_style::LayoutStyle;
use crate::node_id::NodeId;

/// One node: its caller id, its style, and the index of its parent (`None` for the
/// root). Internal — the app builds nodes through [`LayoutTreeBuilder`] and reads
/// results through [`crate::LayoutResult`], so it never names this type.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LayoutNode {
    id: NodeId,
    style: LayoutStyle,
    parent: Option<usize>,
}

impl LayoutNode {
    pub(crate) const fn id(&self) -> NodeId {
        self.id
    }

    pub(crate) const fn style(&self) -> &LayoutStyle {
        &self.style
    }

    pub(crate) const fn parent(&self) -> Option<usize> {
        self.parent
    }
}

/// A built layout tree: a flat, parent-before-child node array ready to solve.
#[derive(Debug, Clone)]
pub struct LayoutTree {
    nodes: Vec<LayoutNode>,
}

impl LayoutTree {
    pub(crate) fn nodes(&self) -> &[LayoutNode] {
        &self.nodes
    }

    /// The number of nodes.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the tree has no nodes.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

/// Assembles a [`LayoutTree`]. Add the root, then add children referencing a
/// parent index the builder already returned — which keeps the array
/// parent-before-child by construction.
#[derive(Debug, Clone)]
pub struct LayoutTreeBuilder {
    nodes: Vec<LayoutNode>,
}

impl LayoutTreeBuilder {
    /// An empty builder.
    pub fn new() -> Self {
        LayoutTreeBuilder { nodes: Vec::new() }
    }

    /// Add the root node, returning its index (the first call returns `0`). The
    /// root fills the safe-area-inset viewport when solved.
    pub fn root(&mut self, id: NodeId, style: LayoutStyle) -> usize {
        self.push(id, style, None)
    }

    /// Add a child of the node at `parent`, returning its index. Because `parent`
    /// is an index the builder already handed out, it is strictly less than this
    /// child's index — the parent-before-child invariant the solver relies on.
    pub fn child(&mut self, parent: usize, id: NodeId, style: LayoutStyle) -> usize {
        self.push(id, style, Some(parent))
    }

    /// Append a node and return its index. Branchless: a push then `len - 1`.
    fn push(&mut self, id: NodeId, style: LayoutStyle, parent: Option<usize>) -> usize {
        self.nodes.push(LayoutNode { id, style, parent });
        self.nodes.len() - 1
    }

    /// Finish, producing the immutable tree.
    pub fn build(self) -> LayoutTree {
        LayoutTree { nodes: self.nodes }
    }
}

impl Default for LayoutTreeBuilder {
    fn default() -> Self {
        LayoutTreeBuilder::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(raw: u32) -> NodeId {
        NodeId::from_raw(raw)
    }

    #[test]
    fn builds_a_parent_before_child_array() {
        let mut b = LayoutTreeBuilder::new();
        let root = b.root(id(0), LayoutStyle::new());
        let a = b.child(root, id(1), LayoutStyle::new());
        let _b2 = b.child(root, id(2), LayoutStyle::new());
        let _grandchild = b.child(a, id(3), LayoutStyle::new());
        let tree = b.build();

        assert_eq!(tree.len(), 4);
        assert!(!tree.is_empty());
        // Root is index 0 with no parent; every child's parent index precedes it.
        assert_eq!(tree.nodes()[0].id(), id(0));
        assert!(tree.nodes()[0].parent().is_none());
        tree.nodes()
            .iter()
            .enumerate()
            .skip(1)
            .for_each(|(i, node)| {
                assert!(node.parent().expect("non-root has a parent") < i);
            });
    }

    #[test]
    fn node_exposes_its_id_style_and_parent() {
        let mut b = LayoutTreeBuilder::new();
        let mut style = LayoutStyle::new();
        style.direction = crate::style_enums::Direction::Column;
        let root = b.root(id(5), style);
        let child = b.child(root, id(6), LayoutStyle::new());
        let tree = b.build();
        assert_eq!(tree.nodes()[root].id(), id(5));
        assert_eq!(
            tree.nodes()[root].style().direction,
            crate::style_enums::Direction::Column
        );
        assert_eq!(tree.nodes()[child].parent(), Some(root));
    }

    #[test]
    fn default_builder_is_empty() {
        let tree = LayoutTreeBuilder::default().build();
        assert!(tree.is_empty());
        assert_eq!(tree.len(), 0);
    }
}
