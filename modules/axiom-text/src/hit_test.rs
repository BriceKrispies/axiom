//! Point → glyph / source-boundary hit testing over a [`LaidOutText`].
//!
//! These are pure reads of the layout cache. All queries operate on whole `char`
//! (Unicode scalar) boundaries — the safest boundary this runtime supports — so a
//! caret or selection can never land inside a UTF-8 sequence.

use axiom_math::Vec2;

use crate::laid_out_text::LaidOutText;

impl LaidOutText {
    /// The index of the glyph whose quad contains `point` (text-local pixels), or
    /// `None` if the point is outside every glyph.
    pub fn glyph_at(&self, point: Vec2) -> Option<usize> {
        self.glyphs.iter().position(|g| {
            (point.x >= g.position.x)
                & (point.x <= g.position.x + g.size.x)
                & (point.y >= g.position.y)
                & (point.y <= g.position.y + g.size.y)
        })
    }

    /// The source `char` index of the nearest glyph to `point`, or `None` when
    /// the text has no glyphs. Snaps to the closest glyph centre — the caret
    /// boundary a click maps to.
    pub fn source_at(&self, point: Vec2) -> Option<u32> {
        self.glyphs
            .iter()
            .map(|g| {
                let cx = g.position.x + g.size.x * 0.5;
                let cy = g.position.y + g.size.y * 0.5;
                let d = (cx - point.x) * (cx - point.x) + (cy - point.y) * (cy - point.y);
                (d, g.source_index)
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, source)| source)
    }

    /// The source `char` range `(start, len)` a glyph maps back to, or `None` for
    /// an out-of-range glyph index.
    pub fn glyph_source(&self, glyph_index: usize) -> Option<u32> {
        self.glyphs.get(glyph_index).map(|g| g.source_index)
    }
}

#[cfg(test)]
mod tests {
    use crate::compiled_font::CompiledFont;
    use crate::fallback_font::default_font;
    use crate::font_registry::FontHandle;
    use crate::layout::{lay_out, LayoutConfig};
    use crate::span::TextSpan;
    use crate::style::TextStyle;
    use axiom_math::Vec2;

    fn laid(text: &str) -> crate::laid_out_text::LaidOutText {
        let font = default_font();
        let fonts: [(FontHandle, &CompiledFont); 1] = [(
            FontHandle {
                index: 0,
                generation: 0,
            },
            &font,
        )];
        lay_out(
            &[TextSpan::plain(text)],
            TextStyle::default(),
            &fonts,
            &LayoutConfig::default(),
        )
    }

    #[test]
    fn glyph_at_finds_and_misses() {
        let t = laid("HI");
        let first = t.glyph_bounds(0).unwrap();
        let inside = Vec2::new(
            first.0.get() + first.2.get() * 0.5,
            first.1.get() + first.3.get() * 0.5,
        );
        assert_eq!(t.glyph_at(inside), Some(0));
        assert_eq!(t.glyph_at(Vec2::new(-100.0, -100.0)), None);
    }

    #[test]
    fn source_at_snaps_to_nearest_and_maps_back() {
        let t = laid("HI");
        assert_eq!(
            t.source_at(Vec2::new(-100.0, 0.0)),
            Some(0),
            "far left → first char"
        );
        assert_eq!(
            t.source_at(Vec2::new(1000.0, 0.0)),
            Some(1),
            "far right → last char"
        );
        assert_eq!(t.glyph_source(1), Some(1));
        assert_eq!(t.glyph_source(99), None);
    }

    #[test]
    fn empty_text_hit_tests_are_none() {
        let t = laid("");
        assert_eq!(t.source_at(Vec2::ZERO), None);
        assert_eq!(t.glyph_at(Vec2::ZERO), None);
    }
}
