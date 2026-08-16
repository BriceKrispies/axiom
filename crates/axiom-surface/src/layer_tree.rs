//! The iterative linearisation of a surface's layer tree.
//!
//! [`Surface`] is a recursive value type — it holds layers, and every layer
//! holds a surface. **That does not license a recursive walk.** Everything in
//! this layer that has to see the whole tree — validation, the requirements
//! summary, the canonical bytes and the flattener — reads it through this one
//! bounded, iterative linearisation instead.
//!
//! The walk is a fold over `0..=MAX_LAYERS`, expanding the children of one
//! already-discovered surface per step. That is what makes it iterative *and*
//! terminating without a `while`: the bound **is** the layer budget. A tree
//! within budget is fully discovered; a tree over budget produces more entries
//! than the bound allows, which is exactly how
//! [`crate::Surface::validate`] detects it.
//!
//! The order is breadth-first, so a parent's index is always strictly smaller
//! than its children's. Every consumer rests on that: the flattener folds the
//! list in reverse and finds every child already resolved, and the byte reader
//! rebuilds the tree the same way.

use crate::layer::{SurfaceLayer, MAX_LAYERS};
use crate::surface::Surface;

/// The parent index of the root — it has none.
pub(crate) const ROOT_PARENT: usize = usize::MAX;

/// One surface of a linearised layer tree.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SurfaceNode<'a> {
    /// The surface itself.
    pub(crate) surface: &'a Surface,
    /// The index of the surface this one layers over, or [`ROOT_PARENT`].
    pub(crate) parent: usize,
    /// The layer record that attaches this surface to its parent — `None` for
    /// the root, which is layered over nothing.
    pub(crate) layer: Option<&'a SurfaceLayer>,
}

/// Linearise `root` and every surface layered onto it, breadth-first.
///
/// Discovers at most one surface's children per step, `MAX_LAYERS + 1` steps in
/// all, so a tree within budget comes back complete and a tree over budget comes
/// back longer than `MAX_LAYERS + 1` — the signal
/// [`crate::Surface::validate`] rejects.
pub(crate) fn linearize(root: &Surface) -> Vec<SurfaceNode<'_>> {
    let seed = vec![SurfaceNode {
        surface: root,
        parent: ROOT_PARENT,
        layer: None,
    }];
    (0..=MAX_LAYERS).fold(seed, |mut list, index| {
        let children: Vec<SurfaceNode<'_>> = list
            .get(index)
            .map(|node| node.surface.layers())
            .unwrap_or_default()
            .iter()
            .map(|layer| SurfaceNode {
                surface: layer.surface(),
                parent: index,
                layer: Some(layer),
            })
            .collect();
        list.extend(children);
        list
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::LayerBlend;
    use crate::surface_builder::SurfaceBuilder;

    fn plain() -> Surface {
        SurfaceBuilder::new().build().expect("a default surface is legal")
    }

    fn layered(surface: Surface, count: usize) -> Surface {
        (0..count)
            .fold(SurfaceBuilder::new(), |builder, _| {
                builder.layer(SurfaceLayer::new(
                    surface.clone(),
                    SurfaceLayer::opaque_mask(),
                    LayerBlend::Over,
                ))
            })
            .build_unchecked()
    }

    #[test]
    fn a_surface_with_no_layers_linearises_to_itself() {
        let surface = plain();
        let nodes = linearize(&surface);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].parent, ROOT_PARENT);
        assert!(nodes[0].layer.is_none());
        assert_eq!(nodes[0].surface, &surface);
    }

    #[test]
    fn children_follow_their_parent_in_breadth_first_order() {
        let nested = layered(plain(), 1);
        let surface = layered(nested, 2);
        let nodes = linearize(&surface);
        // root, its two layers, then each layer's own single layer.
        assert_eq!(nodes.len(), 5);
        let parents: Vec<usize> = nodes.iter().map(|node| node.parent).collect();
        assert_eq!(parents, vec![ROOT_PARENT, 0, 0, 1, 2]);
        nodes.iter().skip(1).for_each(|node| {
            assert!(node.layer.is_some());
            assert!(node.parent < ROOT_PARENT);
        });
    }

    #[test]
    fn a_tree_over_budget_comes_back_longer_than_the_budget() {
        let surface = layered(plain(), MAX_LAYERS + 1);
        assert!(linearize(&surface).len() > MAX_LAYERS + 1);
    }
}
