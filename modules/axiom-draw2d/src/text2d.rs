//! Text as a resolved run of glyph sub-rects + advances — never a rasterized
//! font. This keeps `axiom-draw2d` glyph-index-only (SPEC-04 §9 option 1): the
//! glyph atlas pixels are an `axiom-assets` asset / the app's job; the module
//! traffics glyph rects and advances and a [`FontHandle`] name only.

use axiom_kernel::Meters;

use crate::handles::FontHandle;
use crate::rect::Rect;
use crate::rgba::Rgba;

/// One resolved glyph: its sub-rect in the font's baked atlas, and its
/// horizontal advance. Shape-identical to a sprite sample — exactly why text
/// reuses the sprite path and needs no new backend code.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glyph2d {
    pub source: Rect,
    pub advance: Meters,
}

impl Glyph2d {
    /// Construct a glyph from its atlas sub-rect and advance.
    pub const fn new(source: Rect, advance: Meters) -> Self {
        Glyph2d { source, advance }
    }
}

/// A resolved run of glyphs plus the line height for the run's font size. The
/// run carries pre-shaped advances, so measuring is pure summation — no font
/// registry, no rasterization, lives in the module.
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphRun {
    pub glyphs: Vec<Glyph2d>,
    pub line_height: Meters,
}

impl GlyphRun {
    /// Construct a run from its glyphs and the line height.
    pub fn new(glyphs: Vec<Glyph2d>, line_height: Meters) -> Self {
        GlyphRun {
            glyphs,
            line_height,
        }
    }

    /// Measure the run against `font`: width is the sum of glyph advances,
    /// height is the run's line height.
    ///
    /// The run is already shaped against a baked font, so `font` does not change
    /// the measurement of an already-resolved run (the advances are the metric
    /// table's output); it is accepted for API symmetry with
    /// [`crate::Draw2dApi::text`] and to name which atlas shaped the run.
    pub fn measure(&self, font: FontHandle) -> TextMetrics {
        let _ = font;
        let total: f32 = self.glyphs.iter().map(|g| g.advance.get()).sum();
        // The sum of finitely-many finite advances is finite; the defensive
        // fallback (an impossible non-finite total) yields the line height.
        let width = Meters::new(total).unwrap_or(self.line_height);
        TextMetrics::new(width, self.line_height)
    }
}

/// Horizontal text alignment, as a small `Copy` tag carried on the command for
/// the backend to honour — never branched on inside the module.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextAlign(u8);

impl TextAlign {
    /// Align to the left edge.
    pub const LEFT: TextAlign = TextAlign(0);
    /// Centre horizontally.
    pub const CENTER: TextAlign = TextAlign(1);
    /// Align to the right edge.
    pub const RIGHT: TextAlign = TextAlign(2);

    /// The raw discriminant.
    pub const fn raw(self) -> u8 {
        self.0
    }
}

/// The resolved style of a text draw: the font, the glyph colour, and the
/// alignment. Placement rides on the command's baked transform.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextDraw2d {
    pub font: FontHandle,
    pub color: Rgba,
    pub align: TextAlign,
}

impl TextDraw2d {
    /// Construct a text style from its font, colour, and alignment.
    pub const fn new(font: FontHandle, color: Rgba, align: TextAlign) -> Self {
        TextDraw2d { font, color, align }
    }
}

/// The measured extent of a glyph run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextMetrics {
    pub width: Meters,
    pub height: Meters,
}

impl TextMetrics {
    /// Construct metrics from a width and a height.
    pub const fn new(width: Meters, height: Meters) -> Self {
        TextMetrics { width, height }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meters(v: f32) -> Meters {
        Meters::new(v).unwrap()
    }

    fn glyph(advance: f32) -> Glyph2d {
        use axiom_math::Vec2;
        Glyph2d::new(Rect::new(Vec2::ZERO, Vec2::new(8.0, 12.0)), meters(advance))
    }

    #[test]
    fn measure_sums_advances_and_keeps_line_height() {
        let run = GlyphRun::new(vec![glyph(4.0), glyph(5.0), glyph(6.0)], meters(14.0));
        let m = run.measure(FontHandle::from_raw(1));
        assert_eq!(m.width, meters(15.0));
        assert_eq!(m.height, meters(14.0));
    }

    #[test]
    fn measure_of_empty_run_is_zero_width() {
        let run = GlyphRun::new(Vec::new(), meters(10.0));
        let m = run.measure(FontHandle::from_raw(0));
        assert_eq!(m.width, meters(0.0));
        assert_eq!(m.height, meters(10.0));
    }

    #[test]
    fn text_align_discriminants_are_distinct() {
        assert_eq!(TextAlign::LEFT.raw(), 0);
        assert_eq!(TextAlign::CENTER.raw(), 1);
        assert_eq!(TextAlign::RIGHT.raw(), 2);
        assert_ne!(TextAlign::LEFT, TextAlign::RIGHT);
    }

    #[test]
    fn text_draw_fields_round_trip() {
        use axiom_kernel::Ratio;
        let one = Ratio::new(1.0).unwrap();
        let color = Rgba::new(one, one, one, one);
        let t = TextDraw2d::new(FontHandle::from_raw(7), color, TextAlign::CENTER);
        assert_eq!(t.font, FontHandle::from_raw(7));
        assert_eq!(t.color, color);
        assert_eq!(t.align, TextAlign::CENTER);
    }

    #[test]
    fn glyph_fields_round_trip() {
        let g = glyph(9.0);
        assert_eq!(g.advance, meters(9.0));
    }
}
