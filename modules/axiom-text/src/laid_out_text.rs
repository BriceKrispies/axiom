//! The result of laying out text: positioned glyphs, line metrics, and bounds.

use axiom_host::Pixels;
use axiom_math::Vec2;

use crate::font_registry::FontHandle;
use crate::style::{StyleOverride, TextStyle};

/// One positioned glyph in text-local pixel space (origin top-left, `+x` right,
/// `+y` down), before placement and effects are applied. Carries its resolved
/// style so the glyph batch and effects read colour/outline/shadow directly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LaidGlyph {
    /// The font supplying this glyph.
    pub font: FontHandle,
    /// Glyph index within the font.
    pub glyph: u32,
    /// Atlas page index in the font's nearest size layer.
    pub page: u32,
    /// Atlas source rectangle (left, top, width, height) in atlas pixels.
    pub uv: [u32; 4],
    /// Top-left of the glyph quad in text-local pixels.
    pub position: Vec2,
    /// Glyph quad size in pixels.
    pub size: Vec2,
    /// Pen advance this glyph consumed (for hit testing), in pixels.
    pub advance: f32,
    /// The resolved style used for layout metrics (font size, spacing).
    pub style: TextStyle,
    /// The span's style override, kept so visual fields (colour, opacity,
    /// outline, shadow) resolve against the *current* base style at batch time —
    /// letting `set_color`/`set_opacity` recolour without a re-layout.
    pub overrides: StyleOverride,
    /// The glyph's source `char` index in the full text.
    pub source_index: u32,
    /// The line this glyph is on.
    pub line: u32,
    /// The glyph's column within its line.
    pub column: u32,
}

/// Metrics for one laid-out line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineMetrics {
    start_glyph: u32,
    glyph_count: u32,
    baseline_y: f32,
    top_y: f32,
    height: f32,
    width: f32,
}

/// A finite laid-out metric wrapped as [`Pixels`] (layout produces finite math).
fn px(value: f32) -> Pixels {
    Pixels::new(value).expect("a laid-out metric is a finite pixel length")
}

impl LineMetrics {
    /// Build a line's metrics (internal — the layout engine fills these).
    pub(crate) fn new(
        start_glyph: u32,
        glyph_count: u32,
        baseline_y: f32,
        top_y: f32,
        height: f32,
        width: f32,
    ) -> LineMetrics {
        LineMetrics {
            start_glyph,
            glyph_count,
            baseline_y,
            top_y,
            height,
            width,
        }
    }

    /// Index of this line's first glyph in [`LaidOutText::glyphs`].
    pub fn start_glyph(&self) -> u32 {
        self.start_glyph
    }
    /// Number of glyphs on this line.
    pub fn glyph_count(&self) -> u32 {
        self.glyph_count
    }
    /// The line's baseline y in text-local pixels.
    pub fn baseline_y(&self) -> Pixels {
        px(self.baseline_y)
    }
    /// The line box top y in text-local pixels.
    pub fn top_y(&self) -> Pixels {
        px(self.top_y)
    }
    /// The line box height in pixels.
    pub fn height(&self) -> Pixels {
        px(self.height)
    }
    /// The inked width of the line in pixels.
    pub fn width(&self) -> Pixels {
        px(self.width)
    }
}

/// The overall bounds and counts of a laid-out text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextBounds {
    width: f32,
    height: f32,
    ascent: f32,
    descent: f32,
    baseline: f32,
    line_count: u32,
    glyph_count: u32,
}

impl TextBounds {
    /// Build overall bounds (internal — the layout engine fills these).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        width: f32,
        height: f32,
        ascent: f32,
        descent: f32,
        baseline: f32,
        line_count: u32,
        glyph_count: u32,
    ) -> TextBounds {
        TextBounds {
            width,
            height,
            ascent,
            descent,
            baseline,
            line_count,
            glyph_count,
        }
    }

    /// The widest line, in pixels.
    pub fn width(&self) -> Pixels {
        px(self.width)
    }
    /// Total height of all lines, in pixels.
    pub fn height(&self) -> Pixels {
        px(self.height)
    }
    /// First-line ascent in pixels.
    pub fn ascent(&self) -> Pixels {
        px(self.ascent)
    }
    /// First-line descent in pixels.
    pub fn descent(&self) -> Pixels {
        px(self.descent)
    }
    /// First baseline y in pixels.
    pub fn baseline(&self) -> Pixels {
        px(self.baseline)
    }
    /// Number of lines.
    pub fn line_count(&self) -> u32 {
        self.line_count
    }
    /// Number of glyphs.
    pub fn glyph_count(&self) -> u32 {
        self.glyph_count
    }
}

/// A fully laid-out text: positioned glyphs, per-line metrics, and bounds. This
/// is the cache the runtime keeps; the glyph batch, measurement, and hit testing
/// are all cheap reads of it.
#[derive(Debug, Clone, PartialEq)]
pub struct LaidOutText {
    pub(crate) glyphs: Vec<LaidGlyph>,
    pub(crate) lines: Vec<LineMetrics>,
    pub(crate) bounds: TextBounds,
}

impl LaidOutText {
    /// The overall bounds.
    pub fn bounds(&self) -> TextBounds {
        self.bounds
    }

    /// The per-line metrics.
    pub fn lines(&self) -> &[LineMetrics] {
        &self.lines
    }

    /// The bounding rectangle of one glyph as `(x, y, w, h)` in text-local
    /// pixels, or `None` if the index is out of range.
    pub fn glyph_bounds(&self, glyph_index: usize) -> Option<(Pixels, Pixels, Pixels, Pixels)> {
        self.glyphs.get(glyph_index).map(|g| {
            (
                px(g.position.x),
                px(g.position.y),
                px(g.size.x),
                px(g.size.y),
            )
        })
    }

    /// The number of laid-out glyphs.
    pub fn glyph_count(&self) -> usize {
        self.glyphs.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty() -> LaidOutText {
        LaidOutText {
            glyphs: Vec::new(),
            lines: Vec::new(),
            bounds: TextBounds::new(0.0, 0.0, 0.0, 0.0, 0.0, 0, 0),
        }
    }

    #[test]
    fn empty_text_reads_are_safe() {
        let t = empty();
        assert_eq!(t.glyph_count(), 0);
        assert_eq!(t.lines().len(), 0);
        assert_eq!(t.glyph_bounds(0), None);
        assert_eq!(t.bounds().width().get(), 0.0);
    }

    #[test]
    fn line_metrics_accessors_return_their_values() {
        let l = LineMetrics::new(2, 3, 10.0, 4.0, 12.0, 40.0);
        assert_eq!(l.start_glyph(), 2);
        assert_eq!(l.glyph_count(), 3);
        assert_eq!(l.baseline_y().get(), 10.0);
        assert_eq!(l.top_y().get(), 4.0);
        assert_eq!(l.height().get(), 12.0);
        assert_eq!(l.width().get(), 40.0);
    }

    #[test]
    fn text_bounds_accessors_return_their_values() {
        let b = TextBounds::new(100.0, 50.0, 8.0, 2.0, 8.0, 3, 12);
        assert_eq!(b.width().get(), 100.0);
        assert_eq!(b.height().get(), 50.0);
        assert_eq!(b.ascent().get(), 8.0);
        assert_eq!(b.descent().get(), 2.0);
        assert_eq!(b.baseline().get(), 8.0);
        assert_eq!(b.line_count(), 3);
        assert_eq!(b.glyph_count(), 12);
    }
}
