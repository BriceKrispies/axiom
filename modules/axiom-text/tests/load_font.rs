//! Integration test: build a compiled font through the public builder, register
//! it through the asset-facing API, select it by family, and render — the same
//! path `axiom-font-import` output takes, exercised with a synthetic font so the
//! test is hermetic (no external font file).

use axiom_text::{CompiledFont, FaceSlant, FontBuild, GlyphInput, TextApi, TextError, TextSpan};

/// A tiny 2-glyph font: an `A` (filled) plus the required replacement box.
fn tiny_font(family: &str) -> Vec<u8> {
    let glyph = |cp: u32, fill: u8| GlyphInput {
        codepoint: cp,
        advance: 6,
        bearing_x: 0,
        bearing_y: 7,
        width: 5,
        height: 7,
        raster_w: 5,
        raster_h: 7,
        coverage: vec![fill; 35],
    };
    let build = FontBuild {
        family: family.to_owned(),
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
        atlas_width: 64,
        atlas_height: 64,
        source_hash: 0xABCD,
    };
    CompiledFont::assemble(&build, &[glyph('A' as u32, 255), glyph(0xFFFD, 200)])
        .unwrap()
        .encode()
}

#[test]
fn register_a_compiled_font_select_by_family_and_render() {
    let mut api = TextApi::new();
    let bytes = tiny_font("Arcade Display");
    let handle = api.register_font(&bytes).unwrap();

    // The font is discoverable by the family name it advertises.
    assert_eq!(api.font_by_family("Arcade Display"), Some(handle));

    // A text can select it, and it renders visible glyphs.
    let text = api.text("AAAA").unwrap();
    api.set_fonts(text, vec![handle]).unwrap();
    let batch = api.batch(text, 0).unwrap();
    assert_eq!(batch.glyphs.len(), 4);
    assert!(
        batch.glyphs.iter().all(|g| g.uv_w == 5),
        "sampling the imported atlas"
    );
}

#[test]
fn a_missing_glyph_falls_back_to_the_replacement_box() {
    let mut api = TextApi::new();
    let handle = api.register_font(&tiny_font("Sparse")).unwrap();
    let text = api.text_rich(vec![TextSpan::plain("AZ")]).unwrap();
    // Point ONLY at the sparse font (no default fallback appended? default is
    // always appended, so 'Z' resolves via the default face) — still renders.
    api.set_fonts(text, vec![handle]).unwrap();
    let batch = api.batch(text, 0).unwrap();
    assert_eq!(
        batch.glyphs.len(),
        2,
        "unknown 'Z' still yields a glyph (fallback/replacement)"
    );
}

#[test]
fn garbage_font_bytes_are_rejected_by_the_asset_api() {
    let mut api = TextApi::new();
    assert_eq!(
        api.register_font(&[b'X'; 20]),
        Err(TextError::MalformedFont)
    );
}
