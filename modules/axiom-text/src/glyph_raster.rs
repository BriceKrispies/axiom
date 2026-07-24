//! Where one glyph's pixels sit inside a size layer's atlas.

use axiom_kernel::{BinaryReader, BinaryWriter};

use crate::atlas_page::AtlasPage;
use crate::text_error::{TextError, TextResult};

/// The atlas source rectangle for one glyph at one raster size: the page it
/// lives on and its pixel rectangle within that page. Layout position comes from
/// the size-independent [`crate::glyph_metric::GlyphMetric`]; this only says
/// which pixels to sample. Rasters are stored sorted strictly ascending by
/// `glyph`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlyphRaster {
    /// The glyph index this rectangle belongs to.
    pub glyph: u32,
    /// Index of the atlas page holding the pixels.
    pub page: u32,
    /// Left pixel of the source rectangle.
    pub x: u32,
    /// Top pixel of the source rectangle.
    pub y: u32,
    /// Source rectangle width in pixels.
    pub w: u32,
    /// Source rectangle height in pixels.
    pub h: u32,
}

impl GlyphRaster {
    /// Whether this rectangle fits entirely within `pages[self.page]`.
    pub fn fits_in(&self, pages: &[AtlasPage]) -> bool {
        pages
            .get(self.page as usize)
            .map(|page| (self.x + self.w <= page.width) & (self.y + self.h <= page.height))
            .unwrap_or(false)
    }

    /// Append the rectangle.
    pub(crate) fn write_to(self, writer: &mut BinaryWriter) {
        [self.glyph, self.page, self.x, self.y, self.w, self.h]
            .into_iter()
            .for_each(|value| writer.write_u32(value));
    }

    /// Read a rectangle written by [`GlyphRaster::write_to`]; truncation is
    /// `MalformedFont`.
    pub(crate) fn read_from(reader: &mut BinaryReader<'_>) -> TextResult<GlyphRaster> {
        reader
            .read_u32()
            .and_then(|glyph| reader.read_u32().map(|page| (glyph, page)))
            .and_then(|(glyph, page)| reader.read_u32().map(|x| (glyph, page, x)))
            .and_then(|(glyph, page, x)| reader.read_u32().map(|y| (glyph, page, x, y)))
            .and_then(|(glyph, page, x, y)| reader.read_u32().map(|w| (glyph, page, x, y, w)))
            .and_then(|(glyph, page, x, y, w)| {
                reader.read_u32().map(|h| GlyphRaster {
                    glyph,
                    page,
                    x,
                    y,
                    w,
                    h,
                })
            })
            .map_err(|_| TextError::MalformedFont)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(w: u32, h: u32) -> AtlasPage {
        AtlasPage {
            width: w,
            height: h,
            pixels: vec![0; (w * h) as usize],
        }
    }

    #[test]
    fn round_trips_and_bounds_check() {
        let r = GlyphRaster {
            glyph: 4,
            page: 0,
            x: 1,
            y: 1,
            w: 3,
            h: 3,
        };
        let pages = [page(8, 8)];
        assert!(r.fits_in(&pages));
        let mut w = BinaryWriter::new();
        r.write_to(&mut w);
        let bytes = w.into_bytes();
        assert_eq!(
            GlyphRaster::read_from(&mut BinaryReader::new(&bytes)).unwrap(),
            r
        );
    }

    #[test]
    fn rejects_out_of_page_and_missing_page() {
        let pages = [page(4, 4)];
        assert!(!GlyphRaster {
            glyph: 0,
            page: 0,
            x: 2,
            y: 0,
            w: 3,
            h: 1
        }
        .fits_in(&pages));
        assert!(!GlyphRaster {
            glyph: 0,
            page: 5,
            x: 0,
            y: 0,
            w: 1,
            h: 1
        }
        .fits_in(&pages));
    }

    #[test]
    fn truncation_is_malformed() {
        assert_eq!(
            GlyphRaster::read_from(&mut BinaryReader::new(&[0, 0, 0, 0, 0])),
            Err(TextError::MalformedFont)
        );
    }
}
