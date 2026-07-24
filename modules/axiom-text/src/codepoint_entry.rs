//! One entry of a font's codepoint → glyph-index map.

use axiom_kernel::{BinaryReader, BinaryWriter};

use crate::text_error::{TextError, TextResult};

/// A single `codepoint → glyph` mapping. The map is stored sorted strictly
/// ascending by [`CodepointEntry::codepoint`], so a lookup is a binary search
/// and a duplicate or unsorted codepoint is caught on decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodepointEntry {
    /// The Unicode scalar value.
    pub codepoint: u32,
    /// The glyph index it resolves to (must exist in the glyph metrics table).
    pub glyph: u32,
}

impl CodepointEntry {
    /// Append the entry.
    pub(crate) fn write_to(self, writer: &mut BinaryWriter) {
        writer.write_u32(self.codepoint);
        writer.write_u32(self.glyph);
    }

    /// Read an entry written by [`CodepointEntry::write_to`]; truncation is
    /// `MalformedFont`.
    pub(crate) fn read_from(reader: &mut BinaryReader<'_>) -> TextResult<CodepointEntry> {
        reader
            .read_u32()
            .and_then(|codepoint| {
                reader
                    .read_u32()
                    .map(|glyph| CodepointEntry { codepoint, glyph })
            })
            .map_err(|_| TextError::MalformedFont)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let e = CodepointEntry {
            codepoint: 0x41,
            glyph: 3,
        };
        let mut w = BinaryWriter::new();
        e.write_to(&mut w);
        let bytes = w.into_bytes();
        assert_eq!(
            CodepointEntry::read_from(&mut BinaryReader::new(&bytes)).unwrap(),
            e
        );
    }

    #[test]
    fn truncation_is_malformed() {
        assert_eq!(
            CodepointEntry::read_from(&mut BinaryReader::new(&[1, 0, 0, 0])),
            Err(TextError::MalformedFont)
        );
    }
}
