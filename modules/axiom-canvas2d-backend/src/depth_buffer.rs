//! A per-pixel f32 depth buffer for the software rasterizer.
//!
//! ## Depth convention (explicit, tested in the rasterizer)
//! Depth is NDC z as produced by `projection::project_vertex`: **smaller =
//! nearer**. The buffer clears to `f32::INFINITY` (the far value). The rasterizer
//! owns the hot per-pixel depth test inline against [`DepthBuffer::slice_mut`]
//! (no per-pixel method call): a fragment passes — and overwrites colour +
//! depth — iff its depth is **strictly less than** the stored depth. Therefore a
//! nearer fragment overwrites a farther one, a farther never overwrites a
//! nearer, and on **exactly equal** depth the earlier-drawn fragment wins (the
//! strict `<` rejects the later one), deterministically. Those behaviours are
//! covered by the rasterizer's tests.

/// A `width`×`height` grid of f32 depths, row-major. The rasterizer writes it
/// directly through [`Self::slice_mut`]; the depth-buffer overlay reads it
/// through [`Self::depth_at`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DepthBuffer {
    width: u32,
    height: u32,
    depth: Vec<f32>,
}

impl DepthBuffer {
    /// A depth buffer of `width`×`height` pixels, initialized to the far value.
    pub(crate) fn new(width: u32, height: u32) -> Self {
        let len = width as usize * height as usize;
        DepthBuffer {
            width,
            height,
            depth: vec![f32::INFINITY; len],
        }
    }

    /// Reset every pixel to the far value (`f32::INFINITY`).
    pub(crate) fn clear_far(&mut self) {
        self.depth.iter_mut().for_each(|d| *d = f32::INFINITY);
    }

    /// Read the stored depth at `(x, y)`, or the far value when out of bounds
    /// (used by the depth-buffer debug overlay).
    pub(crate) fn depth_at(&self, x: u32, y: u32) -> f32 {
        let inside = (x < self.width) & (y < self.height);
        let idx = y as usize * self.width as usize + x as usize;
        inside
            .then(|| self.depth.get(idx).copied())
            .flatten()
            .unwrap_or(f32::INFINITY)
    }

    /// The raw depth slice (row-major) for the rasterizer's inline depth test —
    /// preallocated, written by indexed offset with no per-pixel bounds ceremony.
    pub(crate) fn slice_mut(&mut self) -> &mut [f32] {
        &mut self.depth
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_far_initializes_every_pixel_to_far() {
        let mut b = DepthBuffer::new(3, 2);
        b.slice_mut()
            .iter_mut()
            .enumerate()
            .for_each(|(i, d)| *d = i as f32);
        b.clear_far();
        (0..3).for_each(|x| (0..2).for_each(|y| assert_eq!(b.depth_at(x, y), f32::INFINITY)));
    }

    #[test]
    fn slice_mut_writes_are_visible_through_depth_at() {
        let mut b = DepthBuffer::new(2, 2);
        b.slice_mut()[3] = 0.25;
        assert_eq!(b.depth_at(1, 1), 0.25);
        assert_eq!(b.depth_at(0, 0), f32::INFINITY);
    }

    #[test]
    fn depth_at_out_of_bounds_reads_far() {
        let b = DepthBuffer::new(2, 2);
        assert_eq!(b.depth_at(2, 0), f32::INFINITY);
        assert_eq!(b.depth_at(0, 2), f32::INFINITY);
    }
}
