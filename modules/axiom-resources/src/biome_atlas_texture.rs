//! The built-in deterministic biome atlas texture.
//!
//! A single RGBA8 atlas packing four terrain biomes — sand, grass, rock, snow —
//! into a 2×2 grid of equal cells. Terrain meshes sample a biome by emitting UVs
//! into the matching cell (see [`biome_cell_origin`]). Each cell is its biome's
//! base colour with a fine 2-pixel checker shading so the surface reads as
//! texture rather than a flat fill. Built branchlessly: the biome and the
//! shading are both table lookups indexed by integer pixel arithmetic — no
//! `if`/`for`/`match`.

use crate::resource_id::ResourceId;
use crate::texture_data::TextureData;

/// Edge length of the built-in biome atlas, in pixels.
const SIZE: u32 = 64;
/// Atlas columns / rows (a 2×2 packing of four biomes).
pub const ATLAS_COLS: u32 = 2;
pub const ATLAS_ROWS: u32 = 2;

/// The four biome base colours, indexed by biome id `cy * ATLAS_COLS + cx`:
/// 0 = sand, 1 = grass, 2 = rock, 3 = snow.
const BIOMES: [[u8; 4]; 4] = [
    [194, 178, 128, 255], // sand   (0,0)
    [86, 137, 64, 255],   // grass  (1,0)
    [120, 115, 110, 255], // rock   (0,1)
    [235, 240, 245, 255], // snow   (1,1)
];

/// The top-left UV of biome `biome`'s cell in the atlas, in `[0,1]` UV space.
/// A terrain vertex tagged with `biome` samples that biome by offsetting a
/// fractional position within the `1/ATLAS_COLS × 1/ATLAS_ROWS` cell starting
/// here. Out-of-range biome ids wrap into the 4-cell grid.
pub fn biome_cell_origin(biome: u32) -> (f32, f32) {
    let b = biome & 3;
    let cx = b % ATLAS_COLS;
    let cy = b / ATLAS_COLS;
    (cx as f32 / ATLAS_COLS as f32, cy as f32 / ATLAS_ROWS as f32)
}

/// Build the canonical `SIZE`×`SIZE` biome atlas.
pub fn build_biome_atlas_texture(id: ResourceId, name: &'static str) -> TextureData {
    let cell_w = SIZE / ATLAS_COLS;
    let cell_h = SIZE / ATLAS_ROWS;
    let pixels: Vec<u8> = (0..SIZE)
        .flat_map(move |y| {
            (0..SIZE).flat_map(move |x| {
                let biome = ((y / cell_h) * ATLAS_COLS + (x / cell_w)) as usize;
                let base = BIOMES[biome];
                let shade = [0u8, 16][(((x / 2) + (y / 2)) & 1) as usize];
                [
                    base[0].saturating_sub(shade),
                    base[1].saturating_sub(shade),
                    base[2].saturating_sub(shade),
                    255,
                ]
            })
        })
        .collect();
    TextureData::new(id, name, SIZE, SIZE, pixels).expect("biome atlas texture is well-formed")
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
        let t = build_biome_atlas_texture(ResourceId::from_raw(1), "biomes");
        assert_eq!(t.width(), SIZE);
        assert_eq!(t.height(), SIZE);
        assert_eq!(t.rgba8_pixels().len(), (SIZE * SIZE * 4) as usize);
    }

    #[test]
    fn each_quadrant_carries_its_biome_base_colour() {
        let t = build_biome_atlas_texture(ResourceId::from_raw(1), "biomes");
        // Top-left corner of each cell has no shading (x/2 + y/2 even = 0),
        // so it equals the biome's exact base colour.
        assert_eq!(pixel(&t, 0, 0), BIOMES[0]); // sand
        assert_eq!(pixel(&t, SIZE / 2, 0), BIOMES[1]); // grass
        assert_eq!(pixel(&t, 0, SIZE / 2), BIOMES[2]); // rock
        assert_eq!(pixel(&t, SIZE / 2, SIZE / 2), BIOMES[3]); // snow
    }

    #[test]
    fn shading_darkens_alternate_pixels_within_a_cell() {
        let t = build_biome_atlas_texture(ResourceId::from_raw(1), "biomes");
        // (2,0): (x/2 + y/2) = 1 (odd) -> shaded darker than the base.
        let shaded = pixel(&t, 2, 0);
        assert!(shaded[0] < BIOMES[0][0]);
        assert_eq!(shaded[3], 255);
    }

    #[test]
    fn cell_origins_cover_the_four_corners_of_uv_space() {
        assert_eq!(biome_cell_origin(0), (0.0, 0.0));
        assert_eq!(biome_cell_origin(1), (0.5, 0.0));
        assert_eq!(biome_cell_origin(2), (0.0, 0.5));
        assert_eq!(biome_cell_origin(3), (0.5, 0.5));
        // Out-of-range biome ids wrap into the 4-cell grid.
        assert_eq!(biome_cell_origin(4), biome_cell_origin(0));
    }

    #[test]
    fn is_deterministic() {
        let a = build_biome_atlas_texture(ResourceId::from_raw(3), "biomes");
        let b = build_biome_atlas_texture(ResourceId::from_raw(3), "biomes");
        assert_eq!(a, b);
    }
}
