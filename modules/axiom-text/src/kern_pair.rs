//! One kerning adjustment between an ordered pair of glyphs.

use axiom_kernel::{BinaryReader, BinaryWriter};

use crate::text_error::{TextError, TextResult};

/// A kerning adjustment applied to the pen advance when `right` follows `left`,
/// in design units (negative pulls the pair closer). Kerning pairs are stored
/// sorted strictly ascending by `(left, right)`, so a lookup is a binary search
/// and a duplicate pair is caught on decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KernPair {
    /// The preceding glyph index.
    pub left: u32,
    /// The following glyph index.
    pub right: u32,
    /// Signed advance adjustment in design units.
    pub adjust: i32,
}

impl KernPair {
    /// The `(left, right)` ordering key.
    pub const fn key(self) -> (u32, u32) {
        (self.left, self.right)
    }

    /// Append the pair.
    pub(crate) fn write_to(self, writer: &mut BinaryWriter) {
        writer.write_u32(self.left);
        writer.write_u32(self.right);
        writer.write_i32(self.adjust);
    }

    /// Read a pair written by [`KernPair::write_to`]; truncation is
    /// `MalformedFont`.
    pub(crate) fn read_from(reader: &mut BinaryReader<'_>) -> TextResult<KernPair> {
        reader
            .read_u32()
            .and_then(|left| reader.read_u32().map(|right| (left, right)))
            .and_then(|(left, right)| {
                reader.read_i32().map(|adjust| KernPair {
                    left,
                    right,
                    adjust,
                })
            })
            .map_err(|_| TextError::MalformedFont)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_exposes_key() {
        let p = KernPair {
            left: 2,
            right: 5,
            adjust: -30,
        };
        assert_eq!(p.key(), (2, 5));
        let mut w = BinaryWriter::new();
        p.write_to(&mut w);
        let bytes = w.into_bytes();
        assert_eq!(
            KernPair::read_from(&mut BinaryReader::new(&bytes)).unwrap(),
            p
        );
    }

    #[test]
    fn truncation_is_malformed() {
        assert_eq!(
            KernPair::read_from(&mut BinaryReader::new(&[1, 0, 0, 0])),
            Err(TextError::MalformedFont)
        );
    }
}
