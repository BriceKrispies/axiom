//! A low-resolution RGBA8 colour buffer — the software rasterizer's render
//! target and the canvas blit source.
//!
//! Pixels are stored as 4 bytes each (`r, g, b, a`), row-major, top-left origin
//! — exactly the layout a browser `ImageData` expects, so the wasm binding can
//! hand [`SoftwareFramebuffer::as_rgba_bytes`] straight to `putImageData`. Linear
//! `0.0..=1.0` colours are clamped and quantized to bytes on write. Out-of-bounds
//! writes are silently ignored (the buffer is the clip boundary).

use crate::canvas_depth_cue::to_byte;

/// A `width`×`height` RGBA8 colour buffer. Two buffers are equal iff their size
/// and every byte match.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SoftwareFramebuffer {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl SoftwareFramebuffer {
    /// A transparent-black `width`×`height` buffer (`4·w·h` zero bytes).
    pub(crate) fn new(width: u32, height: u32) -> Self {
        let len = width as usize * height as usize * 4;
        SoftwareFramebuffer {
            width,
            height,
            rgba: vec![0_u8; len],
        }
    }

    /// The buffer width in pixels.
    pub(crate) fn width(&self) -> u32 {
        self.width
    }

    /// The buffer height in pixels.
    pub(crate) fn height(&self) -> u32 {
        self.height
    }

    /// Fill every pixel with `color` (linear RGBA, clamped + quantized).
    pub(crate) fn clear(&mut self, color: [f32; 4]) {
        let bytes = to_rgba8(color);
        self.rgba
            .chunks_exact_mut(4)
            .for_each(|px| px.copy_from_slice(&bytes));
    }

    /// Write `color` at `(x, y)`. Out-of-bounds coordinates are ignored.
    pub(crate) fn set_pixel(&mut self, x: u32, y: u32, color: [f32; 4]) {
        let inside = (x < self.width) & (y < self.height);
        let idx = (y as usize * self.width as usize + x as usize) * 4;
        let bytes = to_rgba8(color);
        inside
            .then_some(idx)
            .and_then(|i| self.rgba.get_mut(i..i + 4))
            .into_iter()
            .for_each(|slot| slot.copy_from_slice(&bytes));
    }

    /// **src-over composite** a straight-alpha linear RGBA `src` onto `(x, y)`.
    /// Out-of-bounds coordinates are ignored. Branchless: per channel
    /// `out = src·a + dst·(1-a)`, with `out_a = a + dst_a·(1-a)`.
    pub(crate) fn composite_pixel(&mut self, x: u32, y: u32, src: [f32; 4]) {
        let inside = (x < self.width) & (y < self.height);
        let idx = (y as usize * self.width as usize + x as usize) * 4;
        inside
            .then_some(idx)
            .and_then(|i| self.rgba.get_mut(i..i + 4))
            .into_iter()
            .for_each(|slot| over(slot, src));
    }

    /// The raw RGBA8 slice (row-major) for the rasterizer's inline pixel writes —
    /// preallocated, written by indexed offset with no per-pixel method call.
    pub(crate) fn rgba_mut(&mut self) -> &mut [u8] {
        &mut self.rgba
    }

    /// Consume the buffer, yielding its RGBA8 bytes (row-major, top-left origin
    /// — the `putImageData` source), handed to the blit without a copy.
    pub(crate) fn into_rgba_bytes(self) -> Vec<u8> {
        self.rgba
    }
}

/// src-over composite a straight-alpha linear RGBA `src` onto a 4-byte RGBA8
/// destination `slot`. Branchless: `out = src·a + dst·(1-a)` per colour channel,
/// `out_a = a + dst_a·(1-a)`. Exact for an opaque destination; a
/// partially-transparent destination uses the premultiplied-over form (no divide).
fn over(slot: &mut [u8], src: [f32; 4]) {
    let a = src[3].clamp(0.0, 1.0);
    let inv = 1.0 - a;
    let chan = |s: f32, d: u8| to_byte(s * a + (d as f32 / 255.0) * inv);
    let da = slot[3] as f32 / 255.0;
    slot[0] = chan(src[0], slot[0]);
    slot[1] = chan(src[1], slot[1]);
    slot[2] = chan(src[2], slot[2]);
    slot[3] = to_byte(a + da * inv);
}

/// Linear `0.0..=1.0` RGBA → clamped, rounded RGBA8 bytes.
fn to_rgba8(color: [f32; 4]) -> [u8; 4] {
    let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    [
        byte(color[0]),
        byte(color[1]),
        byte(color[2]),
        byte(color[3]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One RGBA pixel out of a finished buffer's bytes.
    fn px(bytes: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * w + x) * 4) as usize;
        [bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]
    }

    #[test]
    fn clear_writes_exact_rgba_bytes_for_a_known_colour() {
        let mut fb = SoftwareFramebuffer::new(2, 2);
        assert_eq!(fb.width(), 2);
        assert_eq!(fb.height(), 2);
        fb.clear([1.0, 0.0, 0.5, 1.0]);
        let bytes = fb.into_rgba_bytes();
        assert_eq!(bytes.len(), 2 * 2 * 4);
        (0..2)
            .for_each(|x| (0..2).for_each(|y| assert_eq!(px(&bytes, 2, x, y), [255, 0, 128, 255])));
    }

    #[test]
    fn set_pixel_writes_only_the_target_pixel() {
        let mut fb = SoftwareFramebuffer::new(3, 3);
        fb.clear([0.0, 0.0, 0.0, 1.0]);
        fb.set_pixel(1, 2, [1.0, 1.0, 1.0, 1.0]);
        let bytes = fb.into_rgba_bytes();
        assert_eq!(px(&bytes, 3, 1, 2), [255, 255, 255, 255]);
        (0..3).for_each(|x| {
            (0..3).for_each(|y| {
                let is_target = (x == 1) && (y == 2);
                let expected = if is_target { 255 } else { 0 };
                assert_eq!(px(&bytes, 3, x, y), [expected, expected, expected, 255]);
            })
        });
    }

    #[test]
    fn out_of_bounds_write_is_ignored() {
        let mut fb = SoftwareFramebuffer::new(2, 2);
        fb.clear([0.0, 0.0, 0.0, 1.0]);
        fb.set_pixel(2, 0, [1.0, 1.0, 1.0, 1.0]);
        fb.set_pixel(0, 2, [1.0, 1.0, 1.0, 1.0]);
        let bytes = fb.into_rgba_bytes();
        (0..2).for_each(|x| (0..2).for_each(|y| assert_eq!(px(&bytes, 2, x, y), [0, 0, 0, 255])));
    }

    #[test]
    fn channels_clamp_out_of_range_values() {
        let mut fb = SoftwareFramebuffer::new(1, 1);
        fb.set_pixel(0, 0, [2.0, -1.0, 0.0, 5.0]);
        assert_eq!(fb.into_rgba_bytes(), vec![255, 0, 0, 255]);
    }

    #[test]
    fn composite_over_opaque_blends_half_alpha_exactly() {
        let mut fb = SoftwareFramebuffer::new(1, 1);
        fb.clear([1.0, 0.0, 0.0, 1.0]);
        fb.composite_pixel(0, 0, [0.0, 0.0, 1.0, 0.5]);
        // out_rgb = blue·0.5 + red·0.5 = (0.5, 0, 0.5); out_a = 0.5 + 1·0.5 = 1.
        assert_eq!(fb.into_rgba_bytes(), vec![128, 0, 128, 255]);
    }

    #[test]
    fn composite_over_transparent_becomes_the_source() {
        let mut fb = SoftwareFramebuffer::new(1, 1);
        fb.composite_pixel(0, 0, [0.2, 0.4, 0.6, 1.0]);
        assert_eq!(fb.into_rgba_bytes(), vec![51, 102, 153, 255]);
    }

    #[test]
    fn composite_alpha_zero_is_a_no_op() {
        let mut fb = SoftwareFramebuffer::new(1, 1);
        fb.clear([0.1, 0.2, 0.3, 1.0]);
        fb.composite_pixel(0, 0, [1.0, 1.0, 1.0, 0.0]);
        assert_eq!(fb.into_rgba_bytes(), vec![26, 51, 77, 255]);
    }

    #[test]
    fn composite_out_of_bounds_is_ignored() {
        let mut fb = SoftwareFramebuffer::new(2, 2);
        fb.clear([0.0, 0.0, 0.0, 1.0]);
        fb.composite_pixel(2, 0, [1.0, 1.0, 1.0, 1.0]);
        fb.composite_pixel(0, 2, [1.0, 1.0, 1.0, 1.0]);
        let bytes = fb.into_rgba_bytes();
        (0..2).for_each(|x| (0..2).for_each(|y| assert_eq!(px(&bytes, 2, x, y), [0, 0, 0, 255])));
    }

    #[test]
    fn into_rgba_bytes_moves_the_buffer() {
        let mut fb = SoftwareFramebuffer::new(1, 1);
        fb.clear([0.0, 1.0, 0.0, 1.0]);
        let bytes = fb.into_rgba_bytes();
        assert_eq!(bytes, vec![0, 255, 0, 255]);
    }
}
