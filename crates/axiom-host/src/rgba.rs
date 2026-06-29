//! Linear RGBA colour, channels carried as kernel [`Ratio`] so the public
//! surface never exposes a naked `f32`.

use axiom_kernel::Ratio;

/// A linear RGBA colour.
///
/// Each channel is a dimensionless [`Ratio`] (normally `0.0..=1.0`, but values
/// above `1.0` are permitted for HDR / additive glow — `Ratio` constrains
/// finiteness, not range). The colour carries no behaviour; it is the value the
/// 2D draw commands tint, fill, stroke, and shadow with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: Ratio,
    pub g: Ratio,
    pub b: Ratio,
    pub a: Ratio,
}

impl Rgba {
    /// Construct a colour from four pre-validated channel ratios.
    pub const fn new(r: Ratio, g: Ratio, b: Ratio, a: Ratio) -> Self {
        Rgba { r, g, b, a }
    }

    /// The four channels as raw `f32`s in `[r, g, b, a]` order — the form a
    /// backend feeds a blend pipeline.
    pub fn channels(self) -> [f32; 4] {
        [self.r.get(), self.g.get(), self.b.get(), self.a.get()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ratio(v: f32) -> Ratio {
        Ratio::new(v).unwrap()
    }

    #[test]
    fn channels_preserve_order_and_values() {
        let c = Rgba::new(ratio(0.1), ratio(0.2), ratio(0.3), ratio(0.4));
        assert_eq!(c.channels(), [0.1, 0.2, 0.3, 0.4]);
    }

    #[test]
    fn equality_is_component_wise() {
        let a = Rgba::new(ratio(1.0), ratio(0.0), ratio(0.0), ratio(1.0));
        let b = Rgba::new(ratio(1.0), ratio(0.0), ratio(0.0), ratio(1.0));
        let c = Rgba::new(ratio(0.0), ratio(0.0), ratio(0.0), ratio(1.0));
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn hdr_channels_above_one_are_permitted() {
        let c = Rgba::new(ratio(2.5), ratio(0.0), ratio(0.0), ratio(1.0));
        assert_eq!(c.channels()[0], 2.5);
    }
}
