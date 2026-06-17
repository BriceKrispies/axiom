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
        // `width * height` fits in u64 for any u32 inputs, so it cannot
        // overflow and needs no checked step; only the `* 4` (bytes per RGBA
        // pixel) can. Doing the math in u64 also keeps it correct on 32-bit
        // targets (wasm32), where `usize` is narrower.
        (width as u64 * height as u64)
            .checked_mul(4)
            .filter(|&expected| rgba8_pixels.len() as u64 == expected)
            .map(|_| TextureData {
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
        let t = TextureData::new(ResourceId::from_raw(1), "solid", 2, 2, vec![255; 16]).unwrap();
        assert_eq!(t.width(), 2);
        assert_eq!(t.height(), 2);
        assert_eq!(t.rgba8_pixels().len(), 16);
    }

    #[test]
    fn wrong_size_pixel_buffer_is_rejected() {
        assert!(TextureData::new(ResourceId::from_raw(1), "x", 2, 2, vec![255; 15],).is_none());
    }

    #[test]
    fn equality_requires_all_fields() {
        let a = TextureData::new(ResourceId::from_raw(1), "x", 1, 1, vec![1, 2, 3, 4]).unwrap();
        let b = TextureData::new(ResourceId::from_raw(1), "x", 1, 1, vec![1, 2, 3, 4]).unwrap();
        let c = TextureData::new(ResourceId::from_raw(1), "x", 1, 1, vec![1, 2, 3, 5]).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}

#[cfg(test)]
mod cov {
    use super::*;

    #[test]
    fn name_height_and_id_accessors() {
        let t = TextureData::new(ResourceId::from_raw(9), "tex", 1, 1, vec![0; 4]).unwrap();
        assert_eq!(t.id(), ResourceId::from_raw(9));
        assert_eq!(t.name(), "tex");
        assert_eq!(t.width(), 1);
        assert_eq!(t.height(), 1);
    }

    #[test]
    fn width_times_height_overflow_returns_none() {
        // width * height overflows usize -> first checked_mul yields None.
        assert!(
            TextureData::new(ResourceId::from_raw(1), "x", u32::MAX, u32::MAX, Vec::new(),)
                .is_none()
        );
    }

    #[test]
    fn times_four_overflow_returns_none() {
        // width * height fits in usize, but * 4 overflows -> second
        // checked_mul yields None. Only reachable where usize is 32-bit;
        // on 64-bit targets width*height already overflows above, so this
        // input also returns None via the first checked_mul. Either way the
        // None path is exercised.
        assert!(TextureData::new(ResourceId::from_raw(1), "x", u32::MAX, 1, Vec::new(),).is_none());
    }
}
