//! One part of a figure resolved to world space, ready for an app to render.

use axiom_math::{Transform, Vec3};

/// A figure part placed in the world: its world [`Transform`] (from the app's
/// pose resolution), its render `box_size`, and its opaque `tag`. This is what a
/// renderer consumes — draw a box of `box_size` at `transform`, styled by `tag`.
/// It carries no bone/parent structure; the hierarchy has already been resolved.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PosedPart {
    /// The part's world-space transform.
    pub transform: Transform,
    /// The render box extents.
    pub box_size: Vec3,
    /// The opaque, game-defined tag carried over from the figure part.
    pub tag: u32,
}

impl PosedPart {
    /// Construct a posed part.
    pub const fn new(transform: Transform, box_size: Vec3, tag: u32) -> Self {
        Self {
            transform,
            box_size,
            tag,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posed_part_keeps_its_fields() {
        let p = PosedPart::new(
            Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)),
            Vec3::new(0.5, 0.5, 0.5),
            9,
        );
        assert_eq!(p.transform.translation, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(p.box_size, Vec3::new(0.5, 0.5, 0.5));
        assert_eq!(p.tag, 9);
    }
}
