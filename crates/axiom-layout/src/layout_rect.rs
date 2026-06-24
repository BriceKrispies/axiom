//! A placed rectangle in logical pixels.

use axiom_host::Pixels;

/// A solved layout rectangle in **logical** (device-independent) pixels, with a
/// top-left origin (`+x` right, `+y` down). Produced by [`crate::solve`]; an app
/// reads its edges to size and position a surface — a `<canvas>` backing store, a
/// DOM element, a HUD quad. Every edge is a host [`Pixels`], so a placed
/// coordinate can never be non-finite.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutRect {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
}

impl LayoutRect {
    /// Construct from finite logical-pixel edges. Crate-internal: only the solver
    /// mints rects, and it guarantees finite, non-negative extents (extents are
    /// floored at zero, divisors guarded), so the [`Pixels`] accessors never fail.
    pub(crate) const fn from_edges(left: f32, top: f32, width: f32, height: f32) -> Self {
        LayoutRect {
            left,
            top,
            width,
            height,
        }
    }

    /// The left edge (distance from the viewport's left).
    pub fn left(&self) -> Pixels {
        Pixels::new(self.left).expect("a solved edge is finite")
    }

    /// The top edge (distance from the viewport's top).
    pub fn top(&self) -> Pixels {
        Pixels::new(self.top).expect("a solved edge is finite")
    }

    /// The width.
    pub fn width(&self) -> Pixels {
        Pixels::new(self.width).expect("a solved extent is finite")
    }

    /// The height.
    pub fn height(&self) -> Pixels {
        Pixels::new(self.height).expect("a solved extent is finite")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_its_edges_as_pixels() {
        let r = LayoutRect::from_edges(10.0, 20.0, 300.0, 150.0);
        assert_eq!(r.left().get(), 10.0);
        assert_eq!(r.top().get(), 20.0);
        assert_eq!(r.width().get(), 300.0);
        assert_eq!(r.height().get(), 150.0);
    }

    #[test]
    fn is_copy_and_equal() {
        let a = LayoutRect::from_edges(0.0, 0.0, 1.0, 1.0);
        let b = a;
        assert_eq!(a, b);
    }
}
