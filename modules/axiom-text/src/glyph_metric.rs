//! Per-glyph, size-independent metrics keyed by glyph index (design units).

use axiom_kernel::{BinaryReader, BinaryWriter};

use crate::text_error::{TextError, TextResult};

/// The layout metrics of one glyph, in font design units. These drive layout at
/// every size (scaled by `font_size / units_per_em`); the atlas pixels a glyph
/// samples live separately in a [`crate::size_layer::SizeLayer`]. The glyph
/// metrics table is stored sorted strictly ascending by [`GlyphMetric::glyph`],
/// which makes lookup a binary search and duplicates detectable on decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlyphMetric {
    /// The glyph's index within its font (its stable identity).
    pub glyph: u32,
    /// Horizontal pen advance after drawing this glyph, in design units.
    pub advance: i32,
    /// Left bearing: pen-to-ink horizontal offset, in design units.
    pub bearing_x: i32,
    /// Top bearing: baseline-to-ink-top vertical offset, in design units.
    pub bearing_y: i32,
    /// Ink width in design units (`0` for a whitespace glyph).
    pub width: u32,
    /// Ink height in design units (`0` for a whitespace glyph).
    pub height: u32,
}

impl GlyphMetric {
    /// Append the glyph's fixed-width record.
    pub(crate) fn write_to(self, writer: &mut BinaryWriter) {
        writer.write_u32(self.glyph);
        writer.write_i32(self.advance);
        writer.write_i32(self.bearing_x);
        writer.write_i32(self.bearing_y);
        writer.write_u32(self.width);
        writer.write_u32(self.height);
    }

    /// Read a record written by [`GlyphMetric::write_to`]; truncation is
    /// `MalformedFont`.
    pub(crate) fn read_from(reader: &mut BinaryReader<'_>) -> TextResult<GlyphMetric> {
        reader
            .read_u32()
            .and_then(|glyph| reader.read_i32().map(|advance| (glyph, advance)))
            .and_then(|(glyph, advance)| reader.read_i32().map(|bx| (glyph, advance, bx)))
            .and_then(|(glyph, advance, bx)| reader.read_i32().map(|by| (glyph, advance, bx, by)))
            .and_then(|(glyph, advance, bx, by)| {
                reader.read_u32().map(|w| (glyph, advance, bx, by, w))
            })
            .and_then(|(glyph, advance, bx, by, w)| {
                reader.read_u32().map(|h| GlyphMetric {
                    glyph,
                    advance,
                    bearing_x: bx,
                    bearing_y: by,
                    width: w,
                    height: h,
                })
            })
            .map_err(|_| TextError::MalformedFont)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let g = GlyphMetric {
            glyph: 7,
            advance: 600,
            bearing_x: 40,
            bearing_y: 700,
            width: 520,
            height: 700,
        };
        let mut w = BinaryWriter::new();
        g.write_to(&mut w);
        let bytes = w.into_bytes();
        assert_eq!(
            GlyphMetric::read_from(&mut BinaryReader::new(&bytes)).unwrap(),
            g
        );
    }

    #[test]
    fn truncation_is_malformed() {
        assert_eq!(
            GlyphMetric::read_from(&mut BinaryReader::new(&[0, 0, 0])),
            Err(TextError::MalformedFont)
        );
    }
}
