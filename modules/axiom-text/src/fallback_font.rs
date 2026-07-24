//! The engine-owned deterministic fallback font, built into a [`CompiledFont`].
//!
//! Axiom's clean default must not depend on a host-installed system font, so the
//! engine vendors its own: a compact 5×7 bitmap face authored here as source
//! data and compiled — at construction time, in pure code — into the *same*
//! `.axfont` representation every other font uses. There is exactly one font path
//! in the runtime; the fallback is not a special case.
//!
//! Coverage is `A`–`Z`, `0`–`9`, space, and `. , ! : # -`, with `a`–`z` aliased
//! to the uppercase glyphs (the face is caps-only by design — legible, neutral,
//! and small). Any other codepoint resolves to the replacement box glyph. A
//! richer face is produced offline by `axiom-font-import`.

use crate::atlas_page::AtlasPage;
use crate::codepoint_entry::CodepointEntry;
use crate::compiled_font::CompiledFont;
use crate::face_metrics::FaceMetrics;
use crate::face_slant::FaceSlant;
use crate::glyph_metric::GlyphMetric;
use crate::glyph_raster::GlyphRaster;
use crate::import_provenance::ImportProvenance;
use crate::size_layer::SizeLayer;

/// Cell width in atlas pixels (5 ink columns + 1 gap).
const CELL_W: u32 = 6;
/// Cell height in atlas pixels (7 ink rows + 1 gap).
const CELL_H: u32 = 8;
/// Cells per atlas row.
const COLS: u32 = 16;
/// Design units per em (equal to the native raster height, so size 8 = 1×).
const UPEM: u16 = 8;
/// The replacement box glyph, index 0. A hollow rectangle any missing glyph maps
/// to.
const BOX: [u8; 7] = [
    0b11111, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11111,
];

/// The authored glyphs, in glyph-index order starting at 1 (index 0 is [`BOX`]).
/// Each entry is `(codepoint, 7 rows of 5 bits, MSB = leftmost column)`.
const GLYPHS: &[(u32, [u8; 7])] = &[
    (' ' as u32, [0, 0, 0, 0, 0, 0, 0]),
    (
        'A' as u32,
        [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
    ),
    (
        'B' as u32,
        [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
    ),
    (
        'C' as u32,
        [
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
    ),
    (
        'D' as u32,
        [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
    ),
    (
        'E' as u32,
        [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
    ),
    (
        'F' as u32,
        [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
    ),
    (
        'G' as u32,
        [
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111,
        ],
    ),
    (
        'H' as u32,
        [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
    ),
    (
        'I' as u32,
        [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ],
    ),
    (
        'J' as u32,
        [
            0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100,
        ],
    ),
    (
        'K' as u32,
        [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
    ),
    (
        'L' as u32,
        [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
    ),
    (
        'M' as u32,
        [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
    ),
    (
        'N' as u32,
        [
            0b10001, 0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001,
        ],
    ),
    (
        'O' as u32,
        [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        'P' as u32,
        [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
    ),
    (
        'Q' as u32,
        [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
    ),
    (
        'R' as u32,
        [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
    ),
    (
        'S' as u32,
        [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
    ),
    (
        'T' as u32,
        [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
    ),
    (
        'U' as u32,
        [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        'V' as u32,
        [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
    ),
    (
        'W' as u32,
        [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001,
        ],
    ),
    (
        'X' as u32,
        [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
    ),
    (
        'Y' as u32,
        [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
    ),
    (
        'Z' as u32,
        [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
    ),
    (
        '0' as u32,
        [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
    ),
    (
        '1' as u32,
        [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
    ),
    (
        '2' as u32,
        [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
    ),
    (
        '3' as u32,
        [
            0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110,
        ],
    ),
    (
        '4' as u32,
        [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
    ),
    (
        '5' as u32,
        [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
    ),
    (
        '6' as u32,
        [
            0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        '7' as u32,
        [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
    ),
    (
        '8' as u32,
        [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        '9' as u32,
        [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
        ],
    ),
    (',' as u32, [0, 0, 0, 0, 0b00100, 0b00100, 0b01000]),
    ('.' as u32, [0, 0, 0, 0, 0, 0b00110, 0b00110]),
    (
        '!' as u32,
        [0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0, 0b00100],
    ),
    (':' as u32, [0, 0b00110, 0b00110, 0, 0b00110, 0b00110, 0]),
    (
        '#' as u32,
        [
            0b01010, 0b01010, 0b11111, 0b01010, 0b11111, 0b01010, 0b01010,
        ],
    ),
    ('-' as u32, [0, 0, 0, 0b11111, 0, 0, 0]),
];

/// The total glyph count (box + authored).
const GLYPH_COUNT: u32 = 1 + GLYPHS.len() as u32;
/// Cell rows needed to hold every glyph.
const CELL_ROWS: u32 = GLYPH_COUNT.div_ceil(COLS);
/// Atlas page width in pixels.
const PAGE_W: u32 = COLS * CELL_W;
/// Atlas page height in pixels.
const PAGE_H: u32 = CELL_ROWS * CELL_H;

/// The bitmap rows for a glyph index (box at 0, then the authored table).
fn glyph_rows(glyph: u32) -> Option<[u8; 7]> {
    [BOX].get(glyph as usize).copied().or_else(|| {
        GLYPHS
            .get(glyph.wrapping_sub(1) as usize)
            .map(|entry| entry.1)
    })
}

/// The coverage value (`0` or `255`) of one atlas pixel, computed purely from its
/// position — no mutation, so the atlas is a `map`/`collect` with no `for` loop.
fn pixel(p: u32) -> u8 {
    let (col, row) = (p % PAGE_W, p / PAGE_W);
    let (in_x, in_y) = (col % CELL_W, row % CELL_H);
    let glyph = (row / CELL_H) * COLS + (col / CELL_W);
    let inside = (in_x < 5) & (in_y < 7);
    let bit = glyph_rows(glyph)
        .and_then(|rows| rows.get(in_y as usize).copied())
        .map(|bits| (bits >> 4u32.saturating_sub(in_x)) & 1)
        .unwrap_or(0);
    (bit & u8::from(inside)) * 255
}

/// The atlas source rectangle for a glyph index.
fn raster(glyph: u32) -> GlyphRaster {
    GlyphRaster {
        glyph,
        page: 0,
        x: (glyph % COLS) * CELL_W,
        y: (glyph / COLS) * CELL_H,
        w: 5,
        h: 7,
    }
}

/// A deterministic content hash of the authored bitmap data (provenance only).
fn source_hash() -> u64 {
    GLYPHS
        .iter()
        .fold(1469598103934665603u64, |hash, (cp, rows)| {
            rows.iter().fold(hash ^ u64::from(*cp), |h, row| {
                (h ^ u64::from(*row)).wrapping_mul(1099511628211)
            })
        })
}

/// Build the engine default font: the vendored 5×7 bitmap face as a fully-formed,
/// validated [`CompiledFont`].
pub(crate) fn default_font() -> CompiledFont {
    let uppercase = GLYPHS.iter().enumerate().filter_map(|(index, (cp, _))| {
        (('A' as u32)..=('Z' as u32))
            .contains(cp)
            .then_some(CodepointEntry {
                codepoint: cp + 32,
                glyph: index as u32 + 1,
            })
    });
    let mut codepoints: Vec<CodepointEntry> = GLYPHS
        .iter()
        .enumerate()
        .map(|(index, (cp, _))| CodepointEntry {
            codepoint: *cp,
            glyph: index as u32 + 1,
        })
        .chain(uppercase)
        .chain(core::iter::once(CodepointEntry {
            codepoint: 0xFFFD,
            glyph: 0,
        }))
        .collect();
    codepoints.sort_by_key(|entry| entry.codepoint);

    let glyphs = (0..GLYPH_COUNT)
        .map(|glyph| GlyphMetric {
            glyph,
            advance: 6,
            bearing_x: 0,
            bearing_y: 7,
            width: 5,
            height: 7,
        })
        .collect();

    let page = AtlasPage {
        width: PAGE_W,
        height: PAGE_H,
        pixels: (0..PAGE_W * PAGE_H).map(pixel).collect(),
    };
    let rasters = (0..GLYPH_COUNT).map(raster).collect();

    CompiledFont {
        source_hash: source_hash(),
        metrics: FaceMetrics {
            family: "Axiom Default".to_owned(),
            face: "Regular".to_owned(),
            units_per_em: UPEM,
            ascent: 7,
            descent: -1,
            line_gap: 1,
            weight: 400,
            slant: FaceSlant::Upright,
            replacement_codepoint: 0xFFFD,
        },
        codepoints,
        glyphs,
        kerning: Vec::new(),
        size_layers: vec![SizeLayer {
            pixel_size: u32::from(UPEM),
            pages: vec![page],
            rasters,
        }],
        provenance: ImportProvenance {
            tool_version: 1,
            padding: 1,
            atlas_width: PAGE_W,
            atlas_height: PAGE_H,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_font_is_valid_and_covers_ascii_letters() {
        let font = default_font();
        assert_eq!(font.validate(), Ok(()));
        assert_eq!(font.family(), "Axiom Default");
        // Uppercase, lowercase alias, digit, and replacement all resolve.
        assert!(font.glyph_for_codepoint('H' as u32).is_some());
        assert_eq!(
            font.glyph_for_codepoint('h' as u32),
            font.glyph_for_codepoint('H' as u32)
        );
        assert!(font.glyph_for_codepoint('7' as u32).is_some());
        assert_eq!(font.glyph_for_codepoint(0xFFFD), Some(0));
        assert!(font.metric(0).is_some());
    }

    #[test]
    fn default_font_round_trips_through_axfont_bytes() {
        let font = default_font();
        let bytes = font.encode();
        assert_eq!(CompiledFont::decode(&bytes).unwrap(), font);
        // Deterministic: re-encoding yields identical bytes.
        assert_eq!(default_font().encode(), bytes);
    }

    #[test]
    fn atlas_has_ink_and_is_correctly_sized() {
        let font = default_font();
        let layer = &font.size_layers[0];
        assert_eq!(layer.pages[0].pixels.len(), (PAGE_W * PAGE_H) as usize);
        assert!(
            layer.pages[0].pixels.iter().any(|&p| p == 255),
            "letters must have ink"
        );
        // Space glyph (index 1) is blank in its cell.
        let space = raster(1);
        let blank = (space.y..space.y + space.h).all(|y| {
            (space.x..space.x + space.w)
                .all(|x| layer.pages[0].pixels[(y * PAGE_W + x) as usize] == 0)
        });
        assert!(blank, "space glyph must be blank");
    }
}
