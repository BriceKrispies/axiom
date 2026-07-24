//! The compile pipeline: sfnt bytes → rasterized glyphs → validated `.axfont`.

use axiom_text::{CompiledFont, FaceSlant, FontBuild, GlyphInput};
use fontdue::{Font, FontSettings};

/// Everything the compiler needs beyond the source bytes.
#[derive(Debug, Clone)]
pub struct ImportOptions {
    /// Raster pixel size (also the compiled font's design em, so metrics are in
    /// pixels at this size).
    pub pixel_size: u32,
    /// Codepoints to include (already parsed from `--ranges`).
    pub codepoints: Vec<u32>,
    /// Family name to advertise.
    pub family: String,
    /// Target atlas width.
    pub atlas_width: u32,
    /// Maximum atlas height.
    pub atlas_height: u32,
    /// Padding between packed glyphs.
    pub padding: u32,
    /// Replacement codepoint (a box is synthesised if the font lacks it).
    pub replacement: u32,
}

/// Compile raw sfnt bytes into a verified `.axfont` byte stream.
pub fn compile(sfnt: &[u8], opt: &ImportOptions) -> Result<Vec<u8>, String> {
    let font = Font::from_bytes(sfnt, FontSettings::default()).map_err(str::to_owned)?;
    let px = opt.pixel_size.max(1) as f32;
    let line = font
        .horizontal_line_metrics(px)
        .ok_or_else(|| "font has no horizontal line metrics".to_owned())?;

    let mut glyphs: Vec<GlyphInput> = Vec::new();
    let mut have_replacement = false;
    for &cp in &opt.codepoints {
        let ch = match char::from_u32(cp) {
            Some(c) => c,
            None => continue,
        };
        if font.lookup_glyph_index(ch) == 0 && cp != opt.replacement {
            continue; // font does not cover this codepoint
        }
        let (m, coverage) = font.rasterize(ch, px);
        glyphs.push(GlyphInput {
            codepoint: cp,
            advance: m.advance_width.round() as i32,
            bearing_x: m.xmin,
            bearing_y: m.ymin + m.height as i32,
            width: m.width as u32,
            height: m.height as u32,
            raster_w: m.width as u32,
            raster_h: m.height as u32,
            coverage,
        });
        have_replacement |= cp == opt.replacement;
    }
    if !have_replacement {
        glyphs.push(box_glyph(opt.replacement, opt.pixel_size));
    }

    let build = FontBuild {
        family: opt.family.clone(),
        face: "Regular".to_owned(),
        units_per_em: opt.pixel_size.clamp(1, u32::from(u16::MAX)) as u16,
        ascent: line.ascent.round() as i32,
        descent: line.descent.round() as i32,
        line_gap: line.line_gap.round().max(0.0) as i32,
        weight: 400,
        slant: FaceSlant::Upright,
        replacement_codepoint: opt.replacement,
        pixel_size: opt.pixel_size,
        padding: opt.padding,
        atlas_width: opt.atlas_width,
        atlas_height: opt.atlas_height,
        source_hash: fnv1a(sfnt),
    };

    let asset =
        CompiledFont::assemble(&build, &glyphs).map_err(|e| format!("assemble failed: {e:?}"))?;
    let bytes = asset.encode();
    // Immediately verify by decoding the generated asset back.
    let decoded =
        CompiledFont::decode(&bytes).map_err(|e| format!("verify decode failed: {e:?}"))?;
    if decoded != asset {
        return Err("verify mismatch: re-decoded font differs from source".to_owned());
    }
    Ok(bytes)
}

/// A synthetic hollow-box replacement glyph, for fonts lacking one.
fn box_glyph(codepoint: u32, pixel_size: u32) -> GlyphInput {
    let w = (pixel_size * 3 / 5).max(3);
    let h = (pixel_size * 4 / 5).max(4);
    let coverage: Vec<u8> = (0..w * h)
        .map(|p| {
            let (x, y) = (p % w, p / w);
            let border = (x == 0) || (x == w - 1) || (y == 0) || (y == h - 1);
            if border {
                255
            } else {
                0
            }
        })
        .collect();
    GlyphInput {
        codepoint,
        advance: w as i32 + 1,
        bearing_x: 0,
        bearing_y: h as i32,
        width: w,
        height: h,
        raster_w: w,
        raster_h: h,
        coverage,
    }
}

/// FNV-1a hash of the source bytes (provenance identity).
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_glyph_has_a_border_and_correct_size() {
        let g = box_glyph(0xFFFD, 32);
        assert_eq!(g.coverage.len(), (g.raster_w * g.raster_h) as usize);
        assert!(
            g.coverage.iter().any(|&c| c == 255),
            "box has an ink border"
        );
        assert!(g.coverage.iter().any(|&c| c == 0), "box is hollow");
        assert_eq!(g.codepoint, 0xFFFD);
    }

    #[test]
    fn fnv_is_deterministic_and_content_sensitive() {
        assert_eq!(fnv1a(b"abc"), fnv1a(b"abc"));
        assert_ne!(fnv1a(b"abc"), fnv1a(b"abd"));
    }
}
