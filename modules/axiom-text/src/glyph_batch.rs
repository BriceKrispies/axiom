//! The backend-neutral, ordered glyph batch — the runtime product of layout.
//!
//! A [`GlyphBatch`] contains only neutral data: font handles, atlas page/UV
//! coordinates, positions, colours, and placement. It holds no GPU handle, no
//! WebGPU/WebGL object, no canvas context, no DOM node, and no JS value. An app
//! (or a legal adapter) translates it into a renderer contract.

use axiom_host::Pixels;
use axiom_math::Vec2;

use crate::color::Rgba;
use crate::font_registry::FontHandle;
use crate::placement::TextPlacement;

/// One resolved glyph ready to draw, as pure data. The atlas rectangle is in
/// atlas pixels (`uv_*`); an app normalises it by the font's page size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphInstance {
    /// The font that supplies this glyph's pixels (recorded per glyph so fallback
    /// is visible to the backend).
    pub font: FontHandle,
    /// Atlas page index within that font's nearest size layer.
    pub page: u32,
    /// The glyph index within the font.
    pub glyph: u32,
    /// Start of this glyph's source `char` range in the text.
    pub source_start: u32,
    /// Length in `char`s of this glyph's source range (usually 1).
    pub source_len: u32,
    /// Top-left of the glyph quad in text-local pixels (before placement).
    pub position: Vec2,
    /// Glyph quad size in pixels.
    pub size: Vec2,
    /// Atlas source rectangle left, in atlas pixels.
    pub uv_x: u32,
    /// Atlas source rectangle top, in atlas pixels.
    pub uv_y: u32,
    /// Atlas source rectangle width, in atlas pixels.
    pub uv_w: u32,
    /// Atlas source rectangle height, in atlas pixels.
    pub uv_h: u32,
    /// Fill colour (text/span opacity already folded into alpha).
    pub color: Rgba,
    /// Outline stroke width in pixels (`0` = none).
    pub outline_width: Pixels,
    /// Outline colour.
    pub outline_color: Rgba,
    /// Drop-shadow offset in pixels.
    pub shadow_offset: Vec2,
    /// Drop-shadow colour (transparent = none).
    pub shadow_color: Rgba,
    /// A stable ordering key (draw order): `line * 2^20 + column`.
    pub order: u64,
}

/// An ordered batch of glyph instances plus the placement of the whole text. The
/// glyphs are already in stable draw order.
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphBatch {
    /// Placement of the whole text (screen or world).
    pub placement: TextPlacement,
    /// The ordered glyph instances.
    pub glyphs: Vec<GlyphInstance>,
}

impl GlyphBatch {
    /// The number of glyph instances.
    pub fn len(&self) -> usize {
        self.glyphs.len()
    }

    /// Whether the batch is empty.
    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_reports_length() {
        let batch = GlyphBatch {
            placement: TextPlacement::default(),
            glyphs: Vec::new(),
        };
        assert!(batch.is_empty());
        assert_eq!(batch.len(), 0);
    }
}
