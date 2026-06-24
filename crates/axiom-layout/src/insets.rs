//! Edge insets in logical pixels (a node's padding).

use axiom_host::Pixels;

/// Padding insets in logical pixels: the gap between a node's outer box and the
/// content box its children are placed within. Each edge is a non-negative
/// [`Pixels`] in practice (the solver only subtracts them), but the type does not
/// enforce non-negativity — only finiteness, via `Pixels`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Insets {
    /// Inset from the top edge.
    pub top: Pixels,
    /// Inset from the right edge.
    pub right: Pixels,
    /// Inset from the bottom edge.
    pub bottom: Pixels,
    /// Inset from the left edge.
    pub left: Pixels,
}

impl Insets {
    /// Insets from four explicit edges.
    pub const fn new(top: Pixels, right: Pixels, bottom: Pixels, left: Pixels) -> Self {
        Insets {
            top,
            right,
            bottom,
            left,
        }
    }

    /// The same inset on every edge.
    pub const fn uniform(all: Pixels) -> Self {
        Insets {
            top: all,
            right: all,
            bottom: all,
            left: all,
        }
    }

    /// Zero on every edge.
    pub fn zero() -> Self {
        Insets::uniform(Pixels::new(0.0).expect("zero is a finite pixel length"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(v: f32) -> Pixels {
        Pixels::new(v).unwrap()
    }

    #[test]
    fn new_carries_each_edge() {
        let i = Insets::new(px(1.0), px(2.0), px(3.0), px(4.0));
        assert_eq!(i.top.get(), 1.0);
        assert_eq!(i.right.get(), 2.0);
        assert_eq!(i.bottom.get(), 3.0);
        assert_eq!(i.left.get(), 4.0);
    }

    #[test]
    fn uniform_sets_all_edges_equal() {
        let i = Insets::uniform(px(8.0));
        assert_eq!(i.top.get(), 8.0);
        assert_eq!(i.right.get(), 8.0);
        assert_eq!(i.bottom.get(), 8.0);
        assert_eq!(i.left.get(), 8.0);
    }

    #[test]
    fn zero_is_all_zero_and_copy() {
        let z = Insets::zero();
        assert_eq!(z.top.get(), 0.0);
        assert_eq!(z, z);
    }
}
