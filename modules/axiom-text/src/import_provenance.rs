//! The importer settings recorded inside a compiled font, for reproducibility.

use axiom_kernel::{BinaryReader, BinaryWriter};

use crate::text_error::{TextError, TextResult};

/// The subset of importer settings baked into a `.axfont` so the asset explains
/// how it was generated (and so a regeneration can be checked for byte
/// equality). It is deliberately host-neutral: no timestamps, no source paths, no
/// machine metadata — only the deterministic knobs that shaped the atlas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ImportProvenance {
    /// The `axiom-font-import` schema version that produced the asset.
    pub tool_version: u16,
    /// Padding pixels kept between packed glyphs.
    pub padding: u32,
    /// The atlas page width the packer targeted.
    pub atlas_width: u32,
    /// The atlas page height the packer targeted.
    pub atlas_height: u32,
}

impl ImportProvenance {
    /// Append the provenance record.
    pub(crate) fn write_to(self, writer: &mut BinaryWriter) {
        writer.write_u16(self.tool_version);
        writer.write_u32(self.padding);
        writer.write_u32(self.atlas_width);
        writer.write_u32(self.atlas_height);
    }

    /// Read a record written by [`ImportProvenance::write_to`]; truncation is
    /// `MalformedFont`.
    pub(crate) fn read_from(reader: &mut BinaryReader<'_>) -> TextResult<ImportProvenance> {
        reader
            .read_u16()
            .and_then(|tool_version| reader.read_u32().map(|padding| (tool_version, padding)))
            .and_then(|(tool_version, padding)| {
                reader.read_u32().map(|w| (tool_version, padding, w))
            })
            .and_then(|(tool_version, padding, atlas_width)| {
                reader.read_u32().map(|atlas_height| ImportProvenance {
                    tool_version,
                    padding,
                    atlas_width,
                    atlas_height,
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
        let p = ImportProvenance {
            tool_version: 1,
            padding: 1,
            atlas_width: 256,
            atlas_height: 256,
        };
        let mut w = BinaryWriter::new();
        p.write_to(&mut w);
        let bytes = w.into_bytes();
        assert_eq!(
            ImportProvenance::read_from(&mut BinaryReader::new(&bytes)).unwrap(),
            p
        );
    }

    #[test]
    fn truncation_is_malformed() {
        assert_eq!(
            ImportProvenance::read_from(&mut BinaryReader::new(&[0])),
            Err(TextError::MalformedFont)
        );
    }
}
