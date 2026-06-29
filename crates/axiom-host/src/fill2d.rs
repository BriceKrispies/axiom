//! The resolved fill + stroke style of a filled 2D shape.

use axiom_kernel::Meters;

use crate::handles::PaintId;
use crate::rgba::Rgba;

/// A resolved stroke: an outline colour and a width.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stroke2d {
    pub color: Rgba,
    pub width: Meters,
}

impl Stroke2d {
    /// Construct a stroke from a colour and a width.
    pub const fn new(color: Rgba, width: Meters) -> Self {
        Stroke2d { color, width }
    }
}

/// The resolved fill style of a shape: a solid colour **or** a registered paint
/// (gradient) referenced by [`PaintId`], plus an optional stroke. Exactly one
/// of `fill_color` / `fill_paint` is `Some` for a filled shape; both may be
/// `None` for a stroke-only shape. A command's fill **references** a paint by
/// id — it never inlines gradient stops.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fill2d {
    pub fill_color: Option<Rgba>,
    pub fill_paint: Option<PaintId>,
    pub stroke: Option<Stroke2d>,
}

impl Fill2d {
    /// A solid-colour fill with no stroke.
    pub const fn color(color: Rgba) -> Self {
        Fill2d {
            fill_color: Some(color),
            fill_paint: None,
            stroke: None,
        }
    }

    /// A gradient/paint fill (by id) with no stroke.
    pub const fn paint(paint: PaintId) -> Self {
        Fill2d {
            fill_color: None,
            fill_paint: Some(paint),
            stroke: None,
        }
    }

    /// A stroke-only shape (no fill).
    pub const fn stroked(stroke: Stroke2d) -> Self {
        Fill2d {
            fill_color: None,
            fill_paint: None,
            stroke: Some(stroke),
        }
    }

    /// This style with a stroke added.
    pub const fn with_stroke(self, stroke: Stroke2d) -> Self {
        Fill2d {
            stroke: Some(stroke),
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::Ratio;

    fn ratio(v: f32) -> Ratio {
        Ratio::new(v).unwrap()
    }

    fn meters(v: f32) -> Meters {
        Meters::new(v).unwrap()
    }

    fn red() -> Rgba {
        Rgba::new(ratio(1.0), ratio(0.0), ratio(0.0), ratio(1.0))
    }

    #[test]
    fn color_fill_sets_only_color() {
        let f = Fill2d::color(red());
        assert_eq!(f.fill_color, Some(red()));
        assert_eq!(f.fill_paint, None);
        assert_eq!(f.stroke, None);
    }

    #[test]
    fn paint_fill_sets_only_paint() {
        let f = Fill2d::paint(PaintId::from_raw(2));
        assert_eq!(f.fill_color, None);
        assert_eq!(f.fill_paint, Some(PaintId::from_raw(2)));
    }

    #[test]
    fn stroked_sets_only_stroke() {
        let s = Stroke2d::new(red(), meters(1.5));
        let f = Fill2d::stroked(s);
        assert_eq!(f.fill_color, None);
        assert_eq!(f.fill_paint, None);
        assert_eq!(f.stroke, Some(s));
    }

    #[test]
    fn with_stroke_preserves_fill_and_adds_stroke() {
        let s = Stroke2d::new(red(), meters(3.0));
        let f = Fill2d::color(red()).with_stroke(s);
        assert_eq!(f.fill_color, Some(red()));
        assert_eq!(f.stroke, Some(s));
    }
}
