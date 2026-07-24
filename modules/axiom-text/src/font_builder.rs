//! The one sanctioned path for *assembling* a compiled font from primitive glyph
//! data: [`CompiledFont::assemble`]. The offline `axiom-font-import` tool (and the
//! engine fallback) parse/rasterize glyphs however they like, then hand primitive
//! coverage bitmaps here; all `.axfont` structure — glyph-index assignment,
//! deterministic shelf packing, table sorting, and validation — lives in this
//! module so the format has a single source of truth.

use crate::atlas_page::AtlasPage;
use crate::codepoint_entry::CodepointEntry;
use crate::compiled_font::CompiledFont;
use crate::face_metrics::FaceMetrics;
use crate::face_slant::FaceSlant;
use crate::glyph_metric::GlyphMetric;
use crate::glyph_raster::GlyphRaster;
use crate::import_provenance::ImportProvenance;
use crate::size_layer::SizeLayer;
use crate::text_error::{TextError, TextResult};

/// The face-level parameters for [`CompiledFont::assemble`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontBuild {
    /// Family name.
    pub family: String,
    /// Face name.
    pub face: String,
    /// Design units per em.
    pub units_per_em: u16,
    /// Ascent in design units.
    pub ascent: i32,
    /// Descent in design units (negative below baseline).
    pub descent: i32,
    /// Line gap in design units.
    pub line_gap: i32,
    /// Weight class.
    pub weight: u16,
    /// Slant.
    pub slant: FaceSlant,
    /// Replacement codepoint (must be present among the glyphs).
    pub replacement_codepoint: u32,
    /// The raster pixel size these glyph bitmaps were rendered at.
    pub pixel_size: u32,
    /// Padding pixels between packed glyphs.
    pub padding: u32,
    /// Target atlas page width.
    pub atlas_width: u32,
    /// Maximum atlas page height (packing overflows this → error).
    pub atlas_height: u32,
    /// The source content hash to record (provenance).
    pub source_hash: u64,
}

/// One glyph handed to [`CompiledFont::assemble`]: design-unit metrics plus a
/// coverage bitmap at the build's pixel size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphInput {
    /// The codepoint this glyph renders.
    pub codepoint: u32,
    /// Advance in design units.
    pub advance: i32,
    /// Left bearing in design units.
    pub bearing_x: i32,
    /// Top bearing in design units.
    pub bearing_y: i32,
    /// Ink width in design units.
    pub width: u32,
    /// Ink height in design units.
    pub height: u32,
    /// Raster width in pixels.
    pub raster_w: u32,
    /// Raster height in pixels.
    pub raster_h: u32,
    /// Row-major coverage, one byte per pixel (`raster_w * raster_h` bytes).
    pub coverage: Vec<u8>,
}

/// A packed rectangle for one glyph (its atlas position).
#[derive(Debug, Clone, Copy)]
struct Packed {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

/// The running shelf-packer cursor.
#[derive(Debug, Clone, Copy)]
struct Cursor {
    x: u32,
    y: u32,
    row_h: u32,
}

impl CompiledFont {
    /// Assemble and validate a compiled font from primitive glyph data. Glyphs
    /// are assigned indices in ascending-codepoint order; coverage is packed into
    /// one atlas page with a deterministic shelf packer. Fails
    /// `AtlasPackingOverflow` if the glyphs do not fit `atlas_height`, or with a
    /// validation error (e.g. `MissingReplacementGlyph`).
    pub fn assemble(build: &FontBuild, glyphs: &[GlyphInput]) -> TextResult<CompiledFont> {
        let mut sorted: Vec<GlyphInput> = glyphs.to_vec();
        sorted.sort_by_key(|g| g.codepoint);

        let codepoints = sorted
            .iter()
            .enumerate()
            .map(|(index, g)| CodepointEntry {
                codepoint: g.codepoint,
                glyph: index as u32,
            })
            .collect();
        let metrics = sorted
            .iter()
            .enumerate()
            .map(|(index, g)| GlyphMetric {
                glyph: index as u32,
                advance: g.advance,
                bearing_x: g.bearing_x,
                bearing_y: g.bearing_y,
                width: g.width,
                height: g.height,
            })
            .collect();

        pack(&sorted, build).and_then(|(page, rasters)| {
            let font = CompiledFont {
                source_hash: build.source_hash,
                metrics: FaceMetrics {
                    family: build.family.clone(),
                    face: build.face.clone(),
                    units_per_em: build.units_per_em,
                    ascent: build.ascent,
                    descent: build.descent,
                    line_gap: build.line_gap,
                    weight: build.weight,
                    slant: build.slant,
                    replacement_codepoint: build.replacement_codepoint,
                },
                codepoints,
                glyphs: metrics,
                kerning: Vec::new(),
                size_layers: vec![SizeLayer {
                    pixel_size: build.pixel_size.max(1),
                    pages: vec![page],
                    rasters,
                }],
                provenance: ImportProvenance {
                    tool_version: 1,
                    padding: build.padding,
                    atlas_width: build.atlas_width,
                    atlas_height: build.atlas_height,
                },
            };
            font.validate().map(|()| font)
        })
    }
}

/// Shelf-pack the glyph rasters into one atlas page (deterministic; input order
/// is ascending codepoint).
fn pack(glyphs: &[GlyphInput], build: &FontBuild) -> TextResult<(AtlasPage, Vec<GlyphRaster>)> {
    let pad = build.padding;
    let width = build.atlas_width.max(1);
    let start = Cursor {
        x: 0,
        y: 0,
        row_h: 0,
    };
    let (placed, cursor) =
        glyphs
            .iter()
            .fold((Vec::<Packed>::new(), start), |(mut placed, cur), g| {
                let advance = g.raster_w + pad;
                let wrap = (cur.x > 0) & (cur.x + advance > width);
                let cur = wrap
                    .then(|| Cursor {
                        x: 0,
                        y: cur.y + cur.row_h,
                        row_h: 0,
                    })
                    .unwrap_or(cur);
                placed.push(Packed {
                    x: cur.x,
                    y: cur.y,
                    w: g.raster_w,
                    h: g.raster_h,
                });
                (
                    placed,
                    Cursor {
                        x: cur.x + advance,
                        y: cur.y,
                        row_h: cur.row_h.max(g.raster_h + pad),
                    },
                )
            });
    let height = (cursor.y + cursor.row_h).max(1);
    (height <= build.atlas_height)
        .then_some(())
        .ok_or(TextError::AtlasPackingOverflow)
        .map(|()| {
            let mut pixels = vec![0u8; (width * height) as usize];
            glyphs
                .iter()
                .zip(placed.iter())
                .for_each(|(g, p)| blit(&mut pixels, width, g, *p));
            let rasters = placed
                .iter()
                .enumerate()
                .map(|(index, p)| GlyphRaster {
                    glyph: index as u32,
                    page: 0,
                    x: p.x,
                    y: p.y,
                    w: p.w,
                    h: p.h,
                })
                .collect();
            (
                AtlasPage {
                    width,
                    height,
                    pixels,
                },
                rasters,
            )
        })
}

/// Copy one glyph's coverage into the atlas at its packed position.
fn blit(pixels: &mut [u8], atlas_w: u32, glyph: &GlyphInput, at: Packed) {
    (0..at.h).for_each(|dy| {
        (0..at.w).for_each(|dx| {
            let src = (dy * glyph.raster_w + dx) as usize;
            let dst = ((at.y + dy) * atlas_w + at.x + dx) as usize;
            glyph
                .coverage
                .get(src)
                .filter(|_| dst < pixels.len())
                .map(|&cov| pixels[dst] = cov);
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glyph(cp: u32, w: u32, h: u32, fill: u8) -> GlyphInput {
        GlyphInput {
            codepoint: cp,
            advance: 6,
            bearing_x: 0,
            bearing_y: h as i32,
            width: w,
            height: h,
            raster_w: w,
            raster_h: h,
            coverage: vec![fill; (w * h) as usize],
        }
    }

    fn build() -> FontBuild {
        FontBuild {
            family: "Test".to_owned(),
            face: "Regular".to_owned(),
            units_per_em: 8,
            ascent: 7,
            descent: -1,
            line_gap: 1,
            weight: 400,
            slant: FaceSlant::Upright,
            replacement_codepoint: 0xFFFD,
            pixel_size: 8,
            padding: 1,
            atlas_width: 16,
            atlas_height: 64,
            source_hash: 123,
        }
    }

    #[test]
    fn assembles_validates_and_is_deterministic() {
        let glyphs = vec![glyph('A' as u32, 5, 7, 255), glyph(0xFFFD, 5, 7, 255)];
        let font = CompiledFont::assemble(&build(), &glyphs).unwrap();
        assert_eq!(font.validate(), Ok(()));
        assert!(font.glyph_for_codepoint('A' as u32).is_some());
        assert!(font.replacement_glyph().is_some());
        // Byte-identical on repeat, and encodes to a decodable asset.
        let bytes = font.encode();
        assert_eq!(
            CompiledFont::assemble(&build(), &glyphs).unwrap().encode(),
            bytes
        );
        assert_eq!(CompiledFont::decode(&bytes).unwrap(), font);
    }

    #[test]
    fn shelf_packer_wraps_to_new_rows() {
        // Three 6px+1pad glyphs into a 16px atlas → wraps after two per row.
        let glyphs: Vec<GlyphInput> = ['A', 'B', 'C', '\u{FFFD}']
            .iter()
            .map(|c| glyph(*c as u32, 6, 5, 200))
            .collect();
        let font = CompiledFont::assemble(&build(), &glyphs).unwrap();
        let rasters = &font.size_layers[0].rasters;
        assert!(
            rasters.iter().any(|r| r.y > 0),
            "packer used a second shelf"
        );
    }

    #[test]
    fn atlas_overflow_is_reported() {
        let mut b = build();
        b.atlas_height = 4;
        let glyphs = vec![glyph('A' as u32, 5, 7, 255), glyph(0xFFFD, 5, 7, 255)];
        assert_eq!(
            CompiledFont::assemble(&b, &glyphs),
            Err(TextError::AtlasPackingOverflow)
        );
    }

    #[test]
    fn missing_replacement_glyph_fails_validation() {
        let glyphs = vec![glyph('A' as u32, 5, 7, 255)]; // no U+FFFD
        assert_eq!(
            CompiledFont::assemble(&build(), &glyphs),
            Err(TextError::MissingReplacementGlyph)
        );
    }
}
