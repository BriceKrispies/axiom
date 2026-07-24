//! One raster size of a font: its atlas pages plus each glyph's source rect.

use axiom_kernel::{BinaryReader, BinaryWriter};

use crate::atlas_page::AtlasPage;
use crate::font_table::{read_table, write_table};
use crate::glyph_raster::GlyphRaster;
use crate::text_error::{TextError, TextResult};

/// A font rasterized at one pixel size: the atlas pages holding coverage and, per
/// glyph, the rectangle to sample. A compiled font carries one layer per imported
/// size; the fallback bitmap font has exactly one. Layout is size-independent
/// (from design-unit metrics), so a layer only supplies pixels — the runtime
/// picks the layer nearest a requested size and scales its coverage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SizeLayer {
    /// The pixel size this layer was rasterized at (non-zero).
    pub pixel_size: u32,
    /// Atlas pages for this size.
    pub pages: Vec<AtlasPage>,
    /// Per-glyph source rectangles, sorted strictly ascending by glyph index.
    pub rasters: Vec<GlyphRaster>,
}

impl SizeLayer {
    /// Look up a glyph's source rectangle (binary search; the table is sorted).
    pub fn raster(&self, glyph: u32) -> Option<GlyphRaster> {
        self.rasters
            .binary_search_by_key(&glyph, |r| r.glyph)
            .ok()
            .and_then(|index| self.rasters.get(index).copied())
    }

    /// Validate the layer: a non-zero pixel size, every page well-formed, rasters
    /// sorted-and-unique by glyph, and every raster inside a real page.
    pub fn validate(&self) -> TextResult<()> {
        (self.pixel_size > 0)
            .then_some(())
            .ok_or(TextError::InvalidAtlasDimensions)
            .and_then(|()| self.pages.iter().try_for_each(AtlasPage::validate))
            .and_then(|()| {
                self.rasters
                    .windows(2)
                    .all(|pair| pair[0].glyph < pair[1].glyph)
                    .then_some(())
                    .ok_or(TextError::DuplicateGlyph)
            })
            .and_then(|()| {
                self.rasters
                    .iter()
                    .all(|raster| raster.fits_in(&self.pages))
                    .then_some(())
                    .ok_or(TextError::InvalidAtlasPage)
            })
    }

    /// Append the layer: pixel size, then the pages table, then the rasters
    /// table.
    pub(crate) fn write_to(&self, writer: &mut BinaryWriter) {
        writer.write_u32(self.pixel_size);
        write_table(writer, &self.pages, AtlasPage::write_to);
        write_table(writer, &self.rasters, |raster, w| raster.write_to(w));
    }

    /// Read a layer written by [`SizeLayer::write_to`]; truncation is
    /// `MalformedFont`.
    pub(crate) fn read_from(reader: &mut BinaryReader<'_>) -> TextResult<SizeLayer> {
        reader
            .read_u32()
            .map_err(|_| TextError::MalformedFont)
            .and_then(|pixel_size| {
                read_table(reader, AtlasPage::read_from).map(|pages| (pixel_size, pages))
            })
            .and_then(|(pixel_size, pages)| {
                read_table(reader, GlyphRaster::read_from).map(|rasters| SizeLayer {
                    pixel_size,
                    pages,
                    rasters,
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer() -> SizeLayer {
        SizeLayer {
            pixel_size: 16,
            pages: vec![AtlasPage {
                width: 4,
                height: 4,
                pixels: vec![0; 16],
            }],
            rasters: vec![
                GlyphRaster {
                    glyph: 1,
                    page: 0,
                    x: 0,
                    y: 0,
                    w: 2,
                    h: 2,
                },
                GlyphRaster {
                    glyph: 3,
                    page: 0,
                    x: 2,
                    y: 0,
                    w: 2,
                    h: 2,
                },
            ],
        }
    }

    #[test]
    fn round_trips_validates_and_looks_up() {
        let l = layer();
        assert_eq!(l.validate(), Ok(()));
        assert_eq!(l.raster(3).unwrap().x, 2);
        assert_eq!(l.raster(2), None);
        let mut w = BinaryWriter::new();
        l.write_to(&mut w);
        let bytes = w.into_bytes();
        assert_eq!(
            SizeLayer::read_from(&mut BinaryReader::new(&bytes)).unwrap(),
            l
        );
    }

    #[test]
    fn rejects_zero_size_bad_page_unsorted_and_out_of_bounds() {
        let mut zero = layer();
        zero.pixel_size = 0;
        assert_eq!(zero.validate(), Err(TextError::InvalidAtlasDimensions));

        let mut bad_page = layer();
        bad_page.pages[0].pixels.pop();
        assert_eq!(bad_page.validate(), Err(TextError::InvalidAtlasDimensions));

        let mut unsorted = layer();
        unsorted.rasters.reverse();
        assert_eq!(unsorted.validate(), Err(TextError::DuplicateGlyph));

        let mut oob = layer();
        oob.rasters[1].x = 3; // 3 + 2 > page width 4
        assert_eq!(oob.validate(), Err(TextError::InvalidAtlasPage));
    }

    #[test]
    fn truncation_is_malformed() {
        assert_eq!(
            SizeLayer::read_from(&mut BinaryReader::new(&[16, 0])),
            Err(TextError::MalformedFont)
        );
    }
}
