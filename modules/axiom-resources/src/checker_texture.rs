//! The built-in deterministic checkerboard texture.
//!
//! A square RGBA8 checkerboard built branchlessly: each pixel selects one of
//! two colours by the parity of its `(x/cell + y/cell)` cell coordinate, using
//! a 2-entry colour table indexed by that parity — no `if`/`for`/`match`.

use crate::resource_id::ResourceId;
use crate::texture_data::TextureData;

/// Edge length of the built-in checker texture, in pixels.
const SIZE: u32 = 64;
/// Edge length of one checker cell, in pixels.
const CELL: u32 = 8;

/// Build the canonical `SIZE`×`SIZE` two-colour checkerboard. `a` is the colour
/// of the `(0,0)` cell; `b` alternates with it. Tinted at draw time by the
/// material's base colour, so a neutral light/dark checker reads as any hue.
pub fn build_checker_texture(
    id: ResourceId,
    name: &'static str,
    a: [u8; 4],
    b: [u8; 4],
) -> TextureData {
    let colors = [a, b];
    let pixels: Vec<u8> = (0..SIZE)
        .flat_map(move |y| {
            (0..SIZE).flat_map(move |x| colors[(((x / CELL) + (y / CELL)) & 1) as usize])
        })
        .collect();
    TextureData::new(id, name, SIZE, SIZE, pixels).expect("checker texture is well-formed")
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: [u8; 4] = [255, 255, 255, 255];
    const B: [u8; 4] = [60, 60, 60, 255];

    fn pixel(t: &TextureData, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * SIZE + x) * 4) as usize;
        let p = &t.rgba8_pixels()[i..i + 4];
        [p[0], p[1], p[2], p[3]]
    }

    #[test]
    fn dimensions_and_byte_count_match() {
        let t = build_checker_texture(ResourceId::from_raw(1), "checker", A, B);
        assert_eq!(t.width(), SIZE);
        assert_eq!(t.height(), SIZE);
        assert_eq!(t.rgba8_pixels().len(), (SIZE * SIZE * 4) as usize);
    }

    #[test]
    fn origin_cell_is_color_a_and_neighbor_cell_is_color_b() {
        let t = build_checker_texture(ResourceId::from_raw(1), "checker", A, B);
        // (0,0) is in the origin cell -> colour a.
        assert_eq!(pixel(&t, 0, 0), A);
        // One cell to the right alternates to colour b.
        assert_eq!(pixel(&t, CELL, 0), B);
        // Diagonally one cell over returns to colour a.
        assert_eq!(pixel(&t, CELL, CELL), A);
    }

    #[test]
    fn is_deterministic() {
        let x = build_checker_texture(ResourceId::from_raw(7), "c", A, B);
        let y = build_checker_texture(ResourceId::from_raw(7), "c", A, B);
        assert_eq!(x, y);
    }
}
