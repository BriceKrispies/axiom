//! A posed figure bound to the scene node it animates.

use crate::posed_part::PosedPart;

/// A posed articulated figure **bound to a scene node id** — the value that
/// makes a "character" one engine object instead of a figure blob living beside
/// a scene node the app hand-syncs each frame.
///
/// `node` is the opaque scene node id (the same `u64` an app stamps onto that
/// node's renderable `AnimationRef`, so the two sides name the same object; the
/// figure module never depends on `axiom-scene`, so it holds only the raw id).
/// `parts` are the figure's [`PosedPart`]s already resolved to world space (from
/// an `axiom-animation` model pose). Given this, an app draws one coherent
/// object: the node supplies identity + root transform, its renderable supplies
/// mesh/material/texture, and this binding supplies the posed limb boxes.
#[derive(Debug, Clone, PartialEq)]
pub struct BoundFigure {
    node: u64,
    parts: Vec<PosedPart>,
}

impl BoundFigure {
    /// Bind `parts` (a figure posed to world space) to scene node `node`.
    pub fn new(node: u64, parts: Vec<PosedPart>) -> Self {
        BoundFigure { node, parts }
    }

    /// The scene node id this posed figure animates.
    pub const fn node(&self) -> u64 {
        self.node
    }

    /// The figure's world-space posed parts, in part order.
    pub fn parts(&self) -> &[PosedPart] {
        &self.parts
    }

    /// The number of posed parts.
    pub fn part_count(&self) -> usize {
        self.parts.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::{Transform, Vec3};

    fn part(x: f32) -> PosedPart {
        PosedPart::new(
            Transform::from_translation(Vec3::new(x, 0.0, 0.0)),
            Vec3::new(0.5, 0.5, 0.5),
            1,
        )
    }

    #[test]
    fn binds_node_and_parts() {
        let b = BoundFigure::new(42, vec![part(1.0), part(2.0)]);
        assert_eq!(b.node(), 42);
        assert_eq!(b.part_count(), 2);
        assert_eq!(b.parts()[1].transform.translation, Vec3::new(2.0, 0.0, 0.0));
    }

    #[test]
    fn an_empty_binding_has_no_parts() {
        let b = BoundFigure::new(1, vec![]);
        assert_eq!(b.part_count(), 0);
        assert!(b.parts().is_empty());
    }

    #[test]
    fn equality_requires_node_and_parts() {
        let a = BoundFigure::new(1, vec![part(1.0)]);
        let b = BoundFigure::new(1, vec![part(1.0)]);
        let c = BoundFigure::new(2, vec![part(1.0)]);
        let d = BoundFigure::new(1, vec![part(9.0)]);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }
}
