//! The built-in monospace bitmap font (SPEC-04 §9 option 1, the baked-atlas
//! strategy) used by the Tier-0 text path.
//!
//! `axiom-draw2d` is glyph-index-only: a [`crate::draw2d`] text draw resolves a
//! string into a [`GlyphRun`] of atlas sub-rects + advances against a [`FontHandle`],
//! and the backend turns those sub-rects into pixels by sampling a baked atlas
//! texture — exactly like a sprite. This module owns the **one** thing the
//! resolver and the atlas-baker must agree on: the fixed ASCII grid layout. The
//! browser harness (`web/src/harness.ts`) bakes a white atlas on the same grid,
//! and the flattened text command names it by the reserved [`FONT_ATLAS_TEXTURE`]
//! id.
//!
//! Determinism: the glyph **layout** (which cell, which advance) is pure integer
//! arithmetic here — so `measure_text` is reproducible across platforms (the
//! spec's measure-reproducibility requirement, SPEC-04 §9). Only the atlas
//! *pixels* are baked browser-side, and pixels are presentation-only — they never
//! re-enter sim.

use axiom_host::{FontHandle, GlyphRun, Glyph2d, Rect, TextureId};
use axiom_kernel::Meters;
use axiom_math::Vec2;

/// The handle [`crate::GameBridge::load_font`] returns for the built-in font; the
/// only font Tier-0 ships, so every text draw resolves against it.
pub const BUILTIN_FONT: FontHandle = FontHandle::from_raw(1);

/// The reserved [`TextureId`] naming the baked monospace atlas. The harness bakes
/// the atlas under this id; a flattened text command names it so the presenter
/// samples the glyph cells exactly as a sprite samples a texture. Held high to
/// avoid colliding with render-target texture ids (which count from 0).
pub const FONT_ATLAS_TEXTURE: TextureId = TextureId::from_raw(0x00F0_0000);

/// Columns in the atlas grid; rows follow from the printable-ASCII count.
pub const ATLAS_COLS: u32 = 16;
/// The first printable ASCII codepoint placed in the atlas (space).
pub const FIRST_CHAR: u32 = 32;
/// The last printable ASCII codepoint placed in the atlas (`~`).
pub const LAST_CHAR: u32 = 126;
/// One glyph cell's width in atlas pixels.
pub const CELL_W: f32 = 8.0;
/// One glyph cell's height in atlas pixels.
pub const CELL_H: f32 = 16.0;

/// The monospace advance as a fraction of the font size (the cell aspect ratio):
/// every glyph advances the pen by `font_size * ADVANCE_RATIO`.
const ADVANCE_RATIO: f32 = CELL_W / CELL_H;

/// The cell index `0..=(LAST_CHAR - FIRST_CHAR)` for codepoint `code`. Control
/// codes (`< FIRST_CHAR`) and out-of-range codepoints fold to the nearest cell
/// (space / `~`), so every character resolves to a real cell — Tier-0 single-line
/// ASCII; no shaping, no fallback font.
fn cell_index(code: u32) -> u32 {
    code.saturating_sub(FIRST_CHAR).min(LAST_CHAR - FIRST_CHAR)
}

/// The atlas sub-rect (in atlas pixels) of the glyph cell at `index`.
pub fn cell_source(index: u32) -> Rect {
    let col = index % ATLAS_COLS;
    let row = index / ATLAS_COLS;
    Rect::new(
        Vec2::new(col as f32 * CELL_W, row as f32 * CELL_H),
        Vec2::new(CELL_W, CELL_H),
    )
}

/// A finite [`Meters`] from an `f32` (non-finite ⇒ zero), mirroring the boundary
/// helpers in [`crate::draw2d`].
fn meters(value: f32) -> Meters {
    Meters::new(value).unwrap_or_else(|_| Meters::new(0.0).expect("0.0 is finite"))
}

/// Resolve `value` into a [`GlyphRun`] at `font_size` (surface units) against the
/// built-in monospace font: one glyph per char, each naming its atlas cell with a
/// `font_size * ADVANCE_RATIO` advance and a `font_size` line height.
pub fn glyph_run(value: &str, font_size: f32) -> GlyphRun {
    let advance = meters(font_size * ADVANCE_RATIO);
    let glyphs: Vec<Glyph2d> = value
        .chars()
        .map(|c| Glyph2d::new(cell_source(cell_index(c as u32)), advance))
        .collect();
    GlyphRun::new(glyphs, meters(font_size))
}

/// The measured extent of `value` at `font_size`: width is the monospace advance
/// times the character count, height is the line height. Pure integer/scalar math
/// — no atlas, no rasterization — so it is platform-reproducible.
pub fn measure(value: &str, font_size: f32) -> (f32, f32) {
    let count = value.chars().count() as f32;
    (font_size * ADVANCE_RATIO * count, font_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printable_ascii_spans_six_rows() {
        // 95 printable chars (32..=126) over 16 columns ⇒ rows 0..=5.
        assert_eq!(cell_index(FIRST_CHAR), 0);
        assert_eq!(cell_index(LAST_CHAR), LAST_CHAR - FIRST_CHAR);
        assert_eq!((LAST_CHAR - FIRST_CHAR) / ATLAS_COLS, 5);
    }

    #[test]
    fn control_and_out_of_range_codepoints_fold_into_the_grid() {
        // A newline (10) folds to the first cell (space); a high codepoint folds
        // to the last cell — never an out-of-atlas sub-rect.
        assert_eq!(cell_index(10), 0);
        assert_eq!(cell_index(0x1F600), LAST_CHAR - FIRST_CHAR);
    }

    #[test]
    fn cell_source_walks_the_grid_left_to_right_then_down() {
        // Cell 0 is the top-left; cell 16 wraps to the second row.
        assert_eq!(cell_source(0), Rect::new(Vec2::ZERO, Vec2::new(CELL_W, CELL_H)));
        assert_eq!(
            cell_source(ATLAS_COLS),
            Rect::new(Vec2::new(0.0, CELL_H), Vec2::new(CELL_W, CELL_H)),
        );
    }

    #[test]
    fn glyph_run_has_one_glyph_per_char_with_monospace_advance() {
        let run = glyph_run("Hi!", 16.0);
        assert_eq!(run.glyphs.len(), 3);
        // Monospace: every advance is font_size * (CELL_W / CELL_H) = 16 * 0.5.
        run.glyphs
            .iter()
            .for_each(|g| assert_eq!(g.advance, meters(8.0)));
        assert_eq!(run.line_height, meters(16.0));
    }

    #[test]
    fn glyph_run_measure_matches_the_standalone_measure() {
        // The run's own measure (sum of advances) agrees with `measure`.
        let run = glyph_run("score", 20.0);
        let (w, h) = measure("score", 20.0);
        let m = run.measure(BUILTIN_FONT);
        assert_eq!(m.width, meters(w));
        assert_eq!(m.height, meters(h));
        // 5 chars * 20 * 0.5 = 50 wide, 20 tall.
        assert_eq!((w, h), (50.0, 20.0));
    }

    #[test]
    fn empty_string_measures_to_zero_width() {
        assert_eq!(measure("", 16.0), (0.0, 16.0));
        assert!(glyph_run("", 16.0).glyphs.is_empty());
    }

    #[test]
    fn the_builtin_handles_are_the_reserved_constants() {
        assert_eq!(BUILTIN_FONT, FontHandle::from_raw(1));
        assert_eq!(FONT_ATLAS_TEXTURE, TextureId::from_raw(0x00F0_0000));
    }
}
