//! The resolved per-sprite style (the verified-missing sprite fields).

use axiom_math::Vec2;

use crate::rect::Rect;
use crate::rgba::Rgba;

/// The resolved style of a sprite draw: the atlas/flip-book `source` sub-rect,
/// the `anchor` (the sprite-local pivot, in `0..=1` of the sprite), a `tint`,
/// and per-axis flips. Placement and size ride on the command's baked
/// transform; these are the per-sprite parameters a backend samples with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpriteDraw2d {
    pub source: Rect,
    pub anchor: Vec2,
    pub tint: Rgba,
    pub flip_x: bool,
    pub flip_y: bool,
}

impl SpriteDraw2d {
    /// Construct a sprite style from its source sub-rect, anchor, tint, and flips.
    pub const fn new(source: Rect, anchor: Vec2, tint: Rgba, flip_x: bool, flip_y: bool) -> Self {
        SpriteDraw2d {
            source,
            anchor,
            tint,
            flip_x,
            flip_y,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::Ratio;

    fn white() -> Rgba {
        let one = Ratio::new(1.0).unwrap();
        Rgba::new(one, one, one, one)
    }

    #[test]
    fn fields_round_trip() {
        let src = Rect::new(Vec2::ZERO, Vec2::new(16.0, 16.0));
        let s = SpriteDraw2d::new(src, Vec2::new(0.5, 0.5), white(), true, false);
        assert_eq!(s.source, src);
        assert_eq!(s.anchor, Vec2::new(0.5, 0.5));
        assert_eq!(s.tint, white());
        assert!(s.flip_x);
        assert!(!s.flip_y);
    }

    #[test]
    fn equality_distinguishes_flips() {
        let src = Rect::new(Vec2::ZERO, Vec2::ONE);
        let a = SpriteDraw2d::new(src, Vec2::ZERO, white(), false, false);
        let b = SpriteDraw2d::new(src, Vec2::ZERO, white(), false, true);
        assert_ne!(a, b);
    }
}
