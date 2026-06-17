//! Built-in deterministic solid-colour and checker textures.

use crate::resource_id::ResourceId;
use crate::texture_data::TextureData;

/// Build a 2×2 solid-colour RGBA8 texture.
pub fn build_solid_color_texture(id: ResourceId, name: &'static str, rgba: [u8; 4]) -> TextureData {
    let mut pixels = Vec::with_capacity(16);
    (0..4).for_each(|_| {
        pixels.extend_from_slice(&rgba);
    });
    TextureData::new(id, name, 2, 2, pixels).expect("2x2 solid-color texture is well-formed")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solid_color_has_correct_pixel_data() {
        let t = build_solid_color_texture(ResourceId::from_raw(1), "white", [255, 255, 255, 255]);
        assert_eq!(t.rgba8_pixels().len(), 16);
        for byte in t.rgba8_pixels() {
            assert_eq!(*byte, 255);
        }
    }

    #[test]
    fn solid_color_is_deterministic() {
        let a = build_solid_color_texture(ResourceId::from_raw(1), "x", [1, 2, 3, 4]);
        let b = build_solid_color_texture(ResourceId::from_raw(1), "x", [1, 2, 3, 4]);
        assert_eq!(a, b);
    }
}
