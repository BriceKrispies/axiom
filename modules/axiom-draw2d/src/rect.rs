//! An axis-aligned 2D rectangle (a draw destination or an atlas sub-rect).

use axiom_math::Vec2;

/// An axis-aligned rectangle given by its minimum corner and its size.
///
/// Used both as a shape's destination ([`crate::Draw2dApi::rect`]) and as a
/// sprite/glyph atlas sub-rect ([`crate::SpriteDraw2d::source`]). Pure data in
/// the draw surface's own units; the backend interprets it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub min: Vec2,
    pub size: Vec2,
}

impl Rect {
    /// Construct from a minimum corner and a size.
    pub const fn new(min: Vec2, size: Vec2) -> Self {
        Rect { min, size }
    }

    /// The maximum corner (`min + size`).
    pub const fn max(self) -> Vec2 {
        self.min.add(self.size)
    }

    /// The geometric centre (`min + size/2`).
    pub fn center(self) -> Vec2 {
        self.min.add(self.size.mul_scalar(0.5))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_is_min_plus_size() {
        let r = Rect::new(Vec2::new(1.0, 2.0), Vec2::new(3.0, 4.0));
        assert_eq!(r.max(), Vec2::new(4.0, 6.0));
    }

    #[test]
    fn center_is_min_plus_half_size() {
        let r = Rect::new(Vec2::new(2.0, 2.0), Vec2::new(4.0, 6.0));
        assert_eq!(r.center(), Vec2::new(4.0, 5.0));
    }

    #[test]
    fn equality_compares_fields() {
        let a = Rect::new(Vec2::ZERO, Vec2::ONE);
        assert_eq!(a, Rect::new(Vec2::ZERO, Vec2::ONE));
        assert_ne!(a, Rect::new(Vec2::ONE, Vec2::ONE));
    }
}
