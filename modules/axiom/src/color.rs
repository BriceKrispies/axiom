//! A linear-RGBA colour value carried across the engine prelude.

use axiom_kernel::Ratio;
use axiom_math::Vec4;

/// Build a [`Ratio`] from a known-finite literal, evaluated entirely in const
/// context.
///
/// The colour constants and the opaque-alpha default need a `const` way to wrap
/// a literal channel; `Ratio::new` is `const` but fallible. This macro inlines
/// the unwrap as a `const` block (never an instrumented runtime function), so the
/// `Err` arm is a const-eval-only path — unreachable for the finite literals we
/// pass, and never a runtime code region.
macro_rules! unit {
    ($value:expr) => {{
        const RATIO: Ratio = match Ratio::new($value) {
            Ok(r) => r,
            Err(_) => panic!("colour channel literal is finite"),
        };
        RATIO
    }};
}

/// Opaque alpha (`1.0`) as a `Ratio`, built once in const context so the runtime
/// constructors never re-run the fallible conversion.
const OPAQUE: Ratio = unit!(1.0);

/// A linear-space RGBA colour.
///
/// Components are linear (not sRGB-encoded) dimensionless ratios: callers pass
/// the same linear values the engine's materials and clear colours consume, so a
/// colour flows straight through to a `Vec4` base colour or a `[f32; 4]` clear
/// value (the extraction point back to raw floats).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: Ratio,
    pub g: Ratio,
    pub b: Ratio,
    pub a: Ratio,
}

impl Color {
    /// Opaque white.
    pub const WHITE: Color = Color::linear_rgb(unit!(1.0), unit!(1.0), unit!(1.0));
    /// Opaque black.
    pub const BLACK: Color = Color::linear_rgb(unit!(0.0), unit!(0.0), unit!(0.0));

    /// An opaque colour from linear RGB (alpha = 1).
    pub const fn linear_rgb(r: Ratio, g: Ratio, b: Ratio) -> Self {
        Color { r, g, b, a: OPAQUE }
    }

    /// A colour from linear RGBA.
    pub const fn linear_rgba(r: Ratio, g: Ratio, b: Ratio, a: Ratio) -> Self {
        Color { r, g, b, a }
    }

    /// As a math `Vec4` `(r, g, b, a)` — the form a material base colour takes.
    pub fn to_vec4(self) -> Vec4 {
        Vec4::new(self.r.get(), self.g.get(), self.b.get(), self.a.get())
    }

    /// As a plain `[r, g, b, a]` array — the form a clear colour takes (the
    /// extraction point back to raw floats).
    pub const fn to_array(self) -> [f32; 4] {
        [self.r.get(), self.g.get(), self.b.get(), self.a.get()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A finite channel ratio for tests — the runtime analogue of [`unit`],
    /// kept out of `unit` itself so that helper stays a purely const-evaluated
    /// path (its unreachable panic arm is never a runtime region).
    fn r(value: f32) -> Ratio {
        Ratio::new(value).unwrap()
    }

    #[test]
    fn linear_rgb_is_opaque() {
        let c = Color::linear_rgb(r(0.1), r(0.2), r(0.3));
        assert_eq!(
            c,
            Color {
                r: r(0.1),
                g: r(0.2),
                b: r(0.3),
                a: r(1.0),
            }
        );
    }

    #[test]
    fn linear_rgba_keeps_alpha() {
        let c = Color::linear_rgba(r(0.1), r(0.2), r(0.3), r(0.4));
        assert_eq!(c.a, r(0.4));
    }

    #[test]
    fn white_and_black_constants() {
        assert_eq!(Color::WHITE, Color::linear_rgb(r(1.0), r(1.0), r(1.0)));
        assert_eq!(Color::BLACK, Color::linear_rgb(r(0.0), r(0.0), r(0.0)));
    }

    #[test]
    fn converts_to_vec4_and_array() {
        let c = Color::linear_rgba(r(0.2), r(0.4), r(0.6), r(0.8));
        let v = c.to_vec4();
        assert_eq!((v.x, v.y, v.z, v.w), (0.2, 0.4, 0.6, 0.8));
        assert_eq!(c.to_array(), [0.2, 0.4, 0.6, 0.8]);
    }
}
