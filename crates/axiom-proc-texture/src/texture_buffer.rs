//! The neutral RGBA8 texture buffer a texture recipe evaluates to.

/// The largest texture edge a texture operator may produce. Dimensions are
/// clamped into `1..=MAX_DIM`, so a recipe can never ask for an unbounded buffer.
pub const MAX_DIM: u32 = 512;

/// A generated texture: `width * height` row-major RGBA8 pixels. This is the
/// neutral output an app hands to `RunningApp::add_texture_data`; it names no GPU
/// resource and no engine type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureBuffer {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl TextureBuffer {
    /// Build a buffer by evaluating `f` at every `(x, y)`; `f` returns the pixel
    /// as `[r, g, b, a]`. Dimensions are clamped into `1..=MAX_DIM`.
    pub fn from_fn<F: Fn(u32, u32) -> [u8; 4]>(width: u32, height: u32, f: F) -> Self {
        let w = width.clamp(1, MAX_DIM);
        let h = height.clamp(1, MAX_DIM);
        let pixels = (0..w * h).flat_map(|i| f(i % w, i / w)).collect();
        Self {
            width: w,
            height: h,
            pixels,
        }
    }

    /// The width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// The height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The row-major RGBA8 pixels.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Consume the buffer, yielding its pixels.
    pub fn into_pixels(self) -> Vec<u8> {
        self.pixels
    }

    /// The pixel at `(x, y)`, clamped into range so an out-of-bounds sample reads
    /// the nearest edge texel (what blur / gradient sampling wants).
    pub fn texel(&self, x: u32, y: u32) -> [u8; 4] {
        let cx = x.min(self.width - 1);
        let cy = y.min(self.height - 1);
        let base = ((cy * self.width + cx) * 4) as usize;
        [
            self.pixels[base],
            self.pixels[base + 1],
            self.pixels[base + 2],
            self.pixels[base + 3],
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_fn_fills_row_major_and_clamps_dimensions() {
        let t = TextureBuffer::from_fn(2, 3, |x, y| [x as u8, y as u8, 0, 255]);
        assert_eq!((t.width(), t.height()), (2, 3));
        assert_eq!(t.pixels().len(), 2 * 3 * 4);
        assert_eq!(t.texel(1, 2), [1, 2, 0, 255]);
        // Zero / oversize dimensions clamp into 1..=MAX_DIM.
        assert_eq!(TextureBuffer::from_fn(0, 0, |_, _| [0; 4]).width(), 1);
        assert_eq!(
            TextureBuffer::from_fn(9999, 1, |_, _| [0; 4]).width(),
            MAX_DIM
        );
    }

    #[test]
    fn texel_clamps_to_the_nearest_edge() {
        let t = TextureBuffer::from_fn(2, 2, |x, y| [x as u8, y as u8, 0, 255]);
        assert_eq!(t.texel(9, 9), [1, 1, 0, 255]);
        assert_eq!(t.clone().into_pixels().len(), 16);
    }
}
