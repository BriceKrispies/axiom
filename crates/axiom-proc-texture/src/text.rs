//! The **Text** source operator: bitmap-font lettering baked into a texture.
//!
//! A 5×7 uppercase font (A–Z, 0–9, space) blitted onto a background, so a recipe
//! can label a surface — an ad board, a jersey number — without an image asset.
//! The string travels in the parameter list as **packed ASCII**: after the
//! `[width, height, fg, bg, scale, char_count]` header, each following word holds
//! up to four characters (one per byte, low byte first).
//!
//! Like every operator it is branchless: the per-pixel closure selects the glyph,
//! looks its row up in a `const` font table by a computed glyph index, tests the
//! bit, and picks `fg`/`bg` by a table index — no control flow.

use axiom_proc_core::NodeEval;

use crate::color_math::rgba;
use crate::texture_buffer::{TextureBuffer, MAX_DIM};

/// Glyph cell width (5 drawn columns + 1 spacing column) and height.
const GLYPH_W: u32 = 6;
const GLYPH_H: u32 = 7;

/// The 5×7 font, indexed by a compact glyph id: `0` = blank (space / unknown),
/// `1..=26` = `A..=Z`, `27..=36` = `0..=9`. Each row is 5 bits, `0x10` leftmost —
/// the same convention the retro pixel fonts used.
#[rustfmt::skip]
const GLYPHS: [[u8; 7]; 37] = [
    [0, 0, 0, 0, 0, 0, 0], // 0: blank
    [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001], // A
    [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110], // B
    [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110], // C
    [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110], // D
    [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111], // E
    [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000], // F
    [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111], // G
    [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001], // H
    [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111], // I
    [0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100], // J
    [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001], // K
    [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111], // L
    [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001], // M
    [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001], // N
    [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110], // O
    [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000], // P
    [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101], // Q
    [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001], // R
    [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110], // S
    [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100], // T
    [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110], // U
    [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100], // V
    [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001], // W
    [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001], // X
    [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100], // Y
    [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111], // Z
    [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110], // 0
    [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110], // 1
    [0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111], // 2
    [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110], // 3
    [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010], // 4
    [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110], // 5
    [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110], // 6
    [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000], // 7
    [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110], // 8
    [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100], // 9
];

/// The compact glyph id for an ASCII byte: `A..Z → 1..26`, `0..9 → 27..36`,
/// everything else `0` (blank). Branchless: two range predicates select which
/// offset contributes.
fn glyph_index(ch: u8) -> usize {
    let is_alpha = (ch >= b'A') & (ch <= b'Z');
    let is_digit = (ch >= b'0') & (ch <= b'9');
    let alpha = ch.wrapping_sub(b'A').wrapping_add(1) as u32 * u32::from(is_alpha);
    let digit = ch.wrapping_sub(b'0').wrapping_add(27) as u32 * u32::from(is_digit);
    (alpha + digit).min(36) as usize
}

/// **Text** — bitmap-font lettering centred on a background. Params:
/// `[width, height, fg, bg, scale, char_count, packed_0, …]`, each `packed_i`
/// carrying four ASCII bytes. Dimensions clamp into `1..=MAX_DIM`; `scale` and
/// the packed-word count are validated so a recipe can never read past its params.
pub(crate) fn text(ctx: NodeEval<'_, TextureBuffer>) -> Option<TextureBuffer> {
    let p = ctx.params();
    (p.len() >= 6).then_some(()).and_then(|()| {
        let fg = rgba(p[2].as_color());
        let bg = rgba(p[3].as_color());
        let scale = p[4].as_int().max(1);
        let count = p[5].as_int();
        let words = count.div_ceil(4);
        let cw = p[0].as_int().clamp(1, MAX_DIM);
        let ch = p[1].as_int().clamp(1, MAX_DIM);
        (p.len() as u32 >= 6 + words).then(|| {
            // Unpack the characters into an owned glyph-id list (blank for space /
            // unknown), so the per-pixel closure needs no parameter access.
            let ids: Vec<usize> = (0..count)
                .map(|k| glyph_index(((p[(6 + k / 4) as usize].as_int() >> (8 * (k % 4))) & 0xFF) as u8))
                .collect();
            let text_w = count * GLYPH_W * scale;
            let text_h = GLYPH_H * scale;
            let ox = cw.saturating_sub(text_w) / 2;
            let oy = ch.saturating_sub(text_h) / 2;
            TextureBuffer::from_fn(cw, ch, move |x, y| {
                let rx = x.wrapping_sub(ox);
                let ry = y.wrapping_sub(oy);
                let g = rx / (GLYPH_W * scale);
                let cell_x = (rx / scale) % GLYPH_W;
                let cell_y = ry / scale;
                let id = ids.get(g as usize).copied().unwrap_or(0);
                let bits = GLYPHS[id][cell_y.min(GLYPH_H - 1) as usize];
                let on = (bits >> 4u32.saturating_sub(cell_x.min(4))) & 1;
                let lit = (on == 1)
                    & (x >= ox)
                    & (rx < text_w)
                    & (y >= oy)
                    & (ry < text_h)
                    & (cell_x < 5)
                    & (cell_y < GLYPH_H)
                    & (g < count);
                [bg, fg][lit as usize]
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use crate::dispatch::texture_eval;
    use crate::texture_buffer::TextureBuffer;
    use crate::texture_op::TextureOp;
    use axiom_proc_core::ProcCore;
    use axiom_recipe::{Color, Param, RecipeGraph, RecipeId};
    use axiom_space::SpaceApi;

    fn run(params: Vec<Param>) -> Option<TextureBuffer> {
        let mut g = RecipeGraph::new(RecipeId::from_raw(1), 1);
        g.add(TextureOp::Text as u16, params, vec![]);
        ProcCore::new().execute(&g, 3, &SpaceApi::root(), texture_eval).ok()
    }

    fn c(packed: u32) -> Param {
        Param::color(Color::from_packed(packed))
    }

    /// Pack up to four ASCII bytes into one param word (low byte first).
    fn word(chars: &[u8]) -> Param {
        let mut w = 0u32;
        for (i, &b) in chars.iter().enumerate() {
            w |= (b as u32) << (8 * i);
        }
        Param::int(w)
    }

    const FG: u32 = 0xFF_FF_FF_FF;
    const BG: u32 = 0x00_00_00_FF;

    #[test]
    fn missing_header_or_packed_words_fail() {
        // Fewer than the six-word header.
        assert!(run(vec![Param::int(8), Param::int(8), c(FG), c(BG), Param::int(1)]).is_none());
        // Header claims four chars but no packed word follows.
        assert!(run(vec![Param::int(8), Param::int(8), c(FG), c(BG), Param::int(1), Param::int(4)]).is_none());
    }

    #[test]
    fn renders_glyphs_over_background_and_is_deterministic() {
        // "A1@" at scale 1 on a 32×16 canvas: 'A' (a letter) and '1' (a digit)
        // draw; '@' (neither) is blank — this one string exercises all three
        // glyph_index branches, plus lit / spacing / out-of-block / past-end paths.
        let params = vec![Param::int(32), Param::int(16), c(FG), c(BG), Param::int(1), Param::int(3), word(b"A1@")];
        let t = run(params.clone()).unwrap();
        assert_eq!(t, run(params).unwrap(), "deterministic");
        let mut any_fg = false;
        for y in 0..16 {
            for x in 0..32 {
                any_fg |= t.texel(x, y) == [255, 255, 255, 255];
            }
        }
        assert!(any_fg, "at least one glyph stroke is drawn in fg");
        // The top-left corner is outside the centred text block → background.
        assert_eq!(t.texel(0, 0), [0, 0, 0, 255]);
    }

    #[test]
    fn scale_enlarges_and_glyph_index_maps_the_ranges() {
        // A 2× 'Z' still renders (covers scale > 1 and the alphabet upper bound).
        let t = run(vec![Param::int(24), Param::int(20), c(FG), c(BG), Param::int(2), Param::int(1), word(b"Z")]).unwrap();
        let lit = (0..20).any(|y| (0..24).any(|x| t.texel(x, y) == [255, 255, 255, 255]));
        assert!(lit, "a scaled glyph draws");
        // Direct glyph-id checks: A→1, Z→26, 0→27, 9→36, space→0, symbol→0.
        assert_eq!(super::glyph_index(b'A'), 1);
        assert_eq!(super::glyph_index(b'Z'), 26);
        assert_eq!(super::glyph_index(b'0'), 27);
        assert_eq!(super::glyph_index(b'9'), 36);
        assert_eq!(super::glyph_index(b' '), 0);
        assert_eq!(super::glyph_index(b'@'), 0);
    }
}
