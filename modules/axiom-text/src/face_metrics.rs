//! A compiled font's face-wide metrics: names, em size, and vertical extents.

use axiom_kernel::{BinaryReader, BinaryWriter};

use crate::face_slant::FaceSlant;
use crate::text_error::{TextError, TextResult};

/// The size-independent metrics of one font face, all vertical extents expressed
/// in font design units (see [`FaceMetrics::units_per_em`]). Layout scales these
/// by `font_size / units_per_em`, so the compiled asset is resolution-independent
/// and one face serves every requested size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceMetrics {
    /// The declared font family (e.g. `"Axiom Default"`), UTF-8.
    pub family: String,
    /// The specific face name within the family (e.g. `"Regular"`), UTF-8.
    pub face: String,
    /// Design units per em. Every metric below is in these units. Must be
    /// non-zero.
    pub units_per_em: u16,
    /// Distance from the baseline up to the ascender, in design units.
    pub ascent: i32,
    /// Distance from the baseline down to the descender (negative below the
    /// baseline), in design units. Must be strictly below `ascent`.
    pub descent: i32,
    /// Extra leading between lines, in design units (non-negative).
    pub line_gap: i32,
    /// The OpenType weight class (100..=900; 400 normal, 700 bold).
    pub weight: u16,
    /// Whether the face is upright, italic, or oblique.
    pub slant: FaceSlant,
    /// The Unicode codepoint whose glyph stands in for any unmapped character.
    pub replacement_codepoint: u32,
}

impl FaceMetrics {
    /// The full em height (ascent minus descent) in design units.
    pub const fn em_height(&self) -> i32 {
        self.ascent - self.descent
    }

    /// The default single-line advance (ascent − descent + line gap) in design
    /// units.
    pub const fn line_advance(&self) -> i32 {
        self.em_height() + self.line_gap
    }

    /// Reject impossible metrics: a zero em, an ascent not strictly above the
    /// descent, or a negative line gap.
    pub fn validate(&self) -> TextResult<()> {
        ((self.units_per_em > 0) & (self.ascent > self.descent) & (self.line_gap >= 0))
            .then_some(())
            .ok_or(TextError::InvalidFontMetrics)
    }

    /// Append the metrics: two length-prefixed UTF-8 names, then the numeric
    /// fields in a fixed order.
    pub(crate) fn write_to(&self, writer: &mut BinaryWriter) {
        writer.write_byte_slice(self.family.as_bytes());
        writer.write_byte_slice(self.face.as_bytes());
        writer.write_u16(self.units_per_em);
        writer.write_i32(self.ascent);
        writer.write_i32(self.descent);
        writer.write_i32(self.line_gap);
        writer.write_u16(self.weight);
        writer.write_u8(self.slant.raw());
        writer.write_u32(self.replacement_codepoint);
    }

    /// Read metrics written by [`FaceMetrics::write_to`]. A truncated stream is
    /// `MalformedFont`; a non-UTF-8 name is `InvalidFontMetadataUtf8`; an unknown
    /// slant byte is `MalformedFont`.
    pub(crate) fn read_from(reader: &mut BinaryReader<'_>) -> TextResult<FaceMetrics> {
        read_name(reader)
            .and_then(|family| read_name(reader).map(|face| (family, face)))
            .and_then(|(family, face)| {
                read_numerics(reader).map(|numerics| (family, face, numerics))
            })
            .map(
                |(family, face, (units_per_em, ascent, descent, line_gap, weight, slant, repl))| {
                    FaceMetrics {
                        family,
                        face,
                        units_per_em,
                        ascent,
                        descent,
                        line_gap,
                        weight,
                        slant,
                        replacement_codepoint: repl,
                    }
                },
            )
    }
}

/// Read a length-prefixed UTF-8 string, mapping the two distinct failures.
fn read_name(reader: &mut BinaryReader<'_>) -> TextResult<String> {
    reader
        .read_byte_slice()
        .map_err(|_| TextError::MalformedFont)
        .and_then(|bytes| {
            core::str::from_utf8(bytes)
                .map(str::to_owned)
                .map_err(|_| TextError::InvalidFontMetadataUtf8)
        })
}

type Numerics = (u16, i32, i32, i32, u16, FaceSlant, u32);

/// Read the fixed numeric block, mapping truncation and an unknown slant byte.
fn read_numerics(reader: &mut BinaryReader<'_>) -> TextResult<Numerics> {
    reader
        .read_u16()
        .and_then(|upem| reader.read_i32().map(|ascent| (upem, ascent)))
        .and_then(|(upem, ascent)| reader.read_i32().map(|descent| (upem, ascent, descent)))
        .and_then(|(upem, ascent, descent)| {
            reader.read_i32().map(|gap| (upem, ascent, descent, gap))
        })
        .and_then(|(upem, ascent, descent, gap)| {
            reader
                .read_u16()
                .map(|weight| (upem, ascent, descent, gap, weight))
        })
        .and_then(|(upem, ascent, descent, gap, weight)| {
            reader
                .read_u8()
                .map(|slant| (upem, ascent, descent, gap, weight, slant))
        })
        .and_then(|(upem, ascent, descent, gap, weight, slant)| {
            reader
                .read_u32()
                .map(|repl| (upem, ascent, descent, gap, weight, slant, repl))
        })
        .map_err(|_| TextError::MalformedFont)
        .and_then(|(upem, ascent, descent, gap, weight, slant_raw, repl)| {
            FaceSlant::from_raw(slant_raw)
                .ok_or(TextError::MalformedFont)
                .map(|slant| (upem, ascent, descent, gap, weight, slant, repl))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> FaceMetrics {
        FaceMetrics {
            family: "Axiom Default".to_owned(),
            face: "Regular".to_owned(),
            units_per_em: 1000,
            ascent: 800,
            descent: -200,
            line_gap: 100,
            weight: 400,
            slant: FaceSlant::Upright,
            replacement_codepoint: 0xFFFD,
        }
    }

    #[test]
    fn round_trips_and_reports_derived_extents() {
        let m = sample();
        assert_eq!(m.em_height(), 1000);
        assert_eq!(m.line_advance(), 1100);
        assert_eq!(m.validate(), Ok(()));
        let mut w = BinaryWriter::new();
        m.write_to(&mut w);
        let bytes = w.into_bytes();
        assert_eq!(
            FaceMetrics::read_from(&mut BinaryReader::new(&bytes)).unwrap(),
            m
        );
    }

    #[test]
    fn rejects_impossible_metrics() {
        let mut zero_em = sample();
        zero_em.units_per_em = 0;
        assert_eq!(zero_em.validate(), Err(TextError::InvalidFontMetrics));
        let mut inverted = sample();
        inverted.ascent = -300;
        assert_eq!(inverted.validate(), Err(TextError::InvalidFontMetrics));
        let mut neg_gap = sample();
        neg_gap.line_gap = -1;
        assert_eq!(neg_gap.validate(), Err(TextError::InvalidFontMetrics));
    }

    #[test]
    fn truncation_is_malformed() {
        let m = sample();
        let mut w = BinaryWriter::new();
        m.write_to(&mut w);
        let bytes = w.into_bytes();
        assert_eq!(
            FaceMetrics::read_from(&mut BinaryReader::new(&bytes[..bytes.len() - 1])),
            Err(TextError::MalformedFont)
        );
        assert_eq!(
            FaceMetrics::read_from(&mut BinaryReader::new(&[])),
            Err(TextError::MalformedFont)
        );
    }

    #[test]
    fn non_utf8_name_is_reported() {
        let mut w = BinaryWriter::new();
        w.write_byte_slice(&[0xFF, 0xFE]); // invalid UTF-8 family
        let bytes = w.into_bytes();
        assert_eq!(
            FaceMetrics::read_from(&mut BinaryReader::new(&bytes)),
            Err(TextError::InvalidFontMetadataUtf8)
        );
    }

    #[test]
    fn unknown_slant_byte_is_malformed() {
        let m = sample();
        let mut w = BinaryWriter::new();
        m.write_to(&mut w);
        let mut bytes = w.into_bytes();
        // The slant byte sits just before the trailing u32 replacement codepoint.
        let slant_pos = bytes.len() - 5;
        bytes[slant_pos] = 9;
        assert_eq!(
            FaceMetrics::read_from(&mut BinaryReader::new(&bytes)),
            Err(TextError::MalformedFont)
        );
    }
}
