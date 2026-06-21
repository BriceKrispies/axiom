//! The built-in deterministic UV-grid texture.
//!
//! A square RGBA8 texture whose fill is a `u`→red, `v`→green gradient overlaid
//! with white grid lines every `CELL` pixels. It makes UV mapping and texture
//! orientation legible at a glance (the classic "UV checker"/grid). Built
//! branchlessly: each pixel selects gradient-vs-line from a 2-entry table keyed
//! by whether it lies on a grid line — no `if`/`for`/`match`.

use crate::resource_id::ResourceId;
use crate::texture_data::TextureData;

/// Edge length of the built-in UV-grid texture, in pixels.
const SIZE: u32 = 64;
/// Spacing between grid lines, in pixels.
const CELL: u32 = 8;
/// White grid-line colour.
const LINE: [u8; 4] = [255, 255, 255, 255];

/// Build the canonical `SIZE`×`SIZE` UV-grid texture.
pub fn build_uv_grid_texture(id: ResourceId, name: &'static str) -> TextureData {
    let pixels: Vec<u8> = (0..SIZE)
        .flat_map(move |y| {
            (0..SIZE).flat_map(move |x| {
                let on_line = (((x % CELL) == 0) | ((y % CELL) == 0)) as usize;
                let gradient = [
                    (x * 255 / (SIZE - 1)) as u8,
                    (y * 255 / (SIZE - 1)) as u8,
                    40,
                    255,
                ];
                [gradient, LINE][on_line]
            })
        })
        .collect();
    TextureData::new(id, name, SIZE, SIZE, pixels).expect("uv-grid texture is well-formed")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(t: &TextureData, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * SIZE + x) * 4) as usize;
        let p = &t.rgba8_pixels()[i..i + 4];
        [p[0], p[1], p[2], p[3]]
    }

    #[test]
    fn dimensions_and_byte_count_match() {
        let t = build_uv_grid_texture(ResourceId::from_raw(1), "uv");
        assert_eq!(t.width(), SIZE);
        assert_eq!(t.height(), SIZE);
        assert_eq!(t.rgba8_pixels().len(), (SIZE * SIZE * 4) as usize);
    }

    #[test]
    fn grid_lines_are_white_and_interior_is_a_gradient() {
        let t = build_uv_grid_texture(ResourceId::from_raw(1), "uv");
        // (0,0) sits on both the x and y origin lines -> white.
        assert_eq!(pixel(&t, 0, 0), LINE);
        // An interior pixel away from any line carries the gradient: red grows
        // with x, green grows with y, blue is the constant 40.
        let interior = pixel(&t, 3, 5);
        assert_ne!(interior, LINE);
        assert_eq!(interior[2], 40);
        // Red increases left-to-right between two interior, non-line pixels.
        assert!(pixel(&t, 11, 5)[0] > pixel(&t, 3, 5)[0]);
    }

    #[test]
    fn is_deterministic() {
        let a = build_uv_grid_texture(ResourceId::from_raw(2), "uv");
        let b = build_uv_grid_texture(ResourceId::from_raw(2), "uv");
        assert_eq!(a, b);
    }
}
