//! CPU-side texture description.

use crate::resource_id::ResourceId;

/// One CPU-side texture: id, name, dimensions, and an opaque RGBA8
/// pixel buffer.
///
/// The vertical slice ships only built-in deterministic textures
/// (solid colour, checker). No image decoding, no file loading.
#[derive(Debug, Clone, PartialEq)]
pub struct TextureData {
    id: ResourceId,
    name: &'static str,
    width: u32,
    height: u32,
    rgba8_pixels: Vec<u8>,
}

impl TextureData {
    /// Build a texture, validating that `rgba8_pixels` has
    /// `width * height * 4` bytes.
    ///
    /// Panics are intentionally avoided — invalid sizes are reported
    /// by returning `None`.
    pub fn new(
        id: ResourceId,
        name: &'static str,
        width: u32,
        height: u32,
        rgba8_pixels: Vec<u8>,
    ) -> Option<Self> {
        let expected = (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)?;
        if rgba8_pixels.len() != expected {
            return None;
        }
        Some(TextureData {
            id,
            name,
            width,
            height,
            rgba8_pixels,
        })
    }

    pub const fn id(&self) -> ResourceId {
        self.id
    }

    pub const fn name(&self) -> &'static str {
        self.name
    }

    pub const fn width(&self) -> u32 {
        self.width
    }

    pub const fn height(&self) -> u32 {
        self.height
    }

    pub fn rgba8_pixels(&self) -> &[u8] {
        &self.rgba8_pixels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_texture_is_built() {
        let t = TextureData::new(
            ResourceId::from_raw(1),
            "solid",
            2,
            2,
            vec![255; 16],
        )
        .unwrap();
        assert_eq!(t.width(), 2);
        assert_eq!(t.height(), 2);
        assert_eq!(t.rgba8_pixels().len(), 16);
    }

    #[test]
    fn wrong_size_pixel_buffer_is_rejected() {
        assert!(
            TextureData::new(
                ResourceId::from_raw(1),
                "x",
                2,
                2,
                vec![255; 15],
            )
            .is_none()
        );
    }

    #[test]
    fn equality_requires_all_fields() {
        let a =
            TextureData::new(ResourceId::from_raw(1), "x", 1, 1, vec![1, 2, 3, 4]).unwrap();
        let b =
            TextureData::new(ResourceId::from_raw(1), "x", 1, 1, vec![1, 2, 3, 4]).unwrap();
        let c =
            TextureData::new(ResourceId::from_raw(1), "x", 1, 1, vec![1, 2, 3, 5]).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
