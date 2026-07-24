//! One single-channel glyph atlas page: coverage pixels a size layer samples.

use axiom_kernel::{BinaryReader, BinaryWriter};

use crate::text_error::{TextError, TextResult};

/// A rectangular single-channel (8-bit coverage) atlas page. Storing raw
/// coverage rather than PNG keeps the runtime free of image decoding: the
/// compiled asset is the deterministic pixel truth, and a backend uploads these
/// bytes as an R8 texture. `pixels.len()` is exactly `width * height`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtlasPage {
    /// Page width in pixels (non-zero).
    pub width: u32,
    /// Page height in pixels (non-zero).
    pub height: u32,
    /// Row-major coverage, one byte per pixel; length is `width * height`.
    pub pixels: Vec<u8>,
}

impl AtlasPage {
    /// The expected pixel count for the declared dimensions.
    pub const fn area(&self) -> usize {
        (self.width as usize) * (self.height as usize)
    }

    /// Reject a zero dimension or a payload whose length disagrees with the
    /// declared `width * height`.
    pub fn validate(&self) -> TextResult<()> {
        ((self.width > 0) & (self.height > 0) & (self.pixels.len() == self.area()))
            .then_some(())
            .ok_or(TextError::InvalidAtlasDimensions)
    }

    /// Append the page: dimensions then the length-prefixed pixel payload.
    pub(crate) fn write_to(&self, writer: &mut BinaryWriter) {
        writer.write_u32(self.width);
        writer.write_u32(self.height);
        writer.write_byte_slice(&self.pixels);
    }

    /// Read a page written by [`AtlasPage::write_to`]; truncation is
    /// `MalformedFont`.
    pub(crate) fn read_from(reader: &mut BinaryReader<'_>) -> TextResult<AtlasPage> {
        reader
            .read_u32()
            .and_then(|width| reader.read_u32().map(|height| (width, height)))
            .and_then(|(width, height)| {
                reader.read_byte_slice().map(|pixels| AtlasPage {
                    width,
                    height,
                    pixels: pixels.to_vec(),
                })
            })
            .map_err(|_| TextError::MalformedFont)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_validates() {
        let p = AtlasPage {
            width: 2,
            height: 2,
            pixels: vec![0, 255, 128, 64],
        };
        assert_eq!(p.area(), 4);
        assert_eq!(p.validate(), Ok(()));
        let mut w = BinaryWriter::new();
        p.write_to(&mut w);
        let bytes = w.into_bytes();
        assert_eq!(
            AtlasPage::read_from(&mut BinaryReader::new(&bytes)).unwrap(),
            p
        );
    }

    #[test]
    fn rejects_bad_dimensions() {
        assert_eq!(
            AtlasPage {
                width: 0,
                height: 2,
                pixels: vec![]
            }
            .validate(),
            Err(TextError::InvalidAtlasDimensions)
        );
        assert_eq!(
            AtlasPage {
                width: 2,
                height: 2,
                pixels: vec![1, 2, 3]
            }
            .validate(),
            Err(TextError::InvalidAtlasDimensions)
        );
    }

    #[test]
    fn truncation_is_malformed() {
        assert_eq!(
            AtlasPage::read_from(&mut BinaryReader::new(&[2, 0, 0])),
            Err(TextError::MalformedFont)
        );
    }
}
