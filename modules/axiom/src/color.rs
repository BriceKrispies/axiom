//! A linear-RGBA colour value carried across the engine prelude.

use axiom_math::Vec4;

/// A linear-space RGBA colour.
///
/// Components are linear (not sRGB-encoded) and unclamped: callers pass the same
/// linear values the engine's materials and clear colours consume, so a colour
/// flows straight through to a `Vec4` base colour or a `[f32; 4]` clear value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    /// Opaque white.
    pub const WHITE: Color = Color::linear_rgb(1.0, 1.0, 1.0);
    /// Opaque black.
    pub const BLACK: Color = Color::linear_rgb(0.0, 0.0, 0.0);

    /// An opaque colour from linear RGB (alpha = 1).
    pub const fn linear_rgb(r: f32, g: f32, b: f32) -> Self {
        Color { r, g, b, a: 1.0 }
    }

    /// A colour from linear RGBA.
    pub const fn linear_rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Color { r, g, b, a }
    }

    /// As a math `Vec4` `(r, g, b, a)` — the form a material base colour takes.
    pub fn to_vec4(self) -> Vec4 {
        Vec4::new(self.r, self.g, self.b, self.a)
    }

    /// As a plain `[r, g, b, a]` array — the form a clear colour takes.
    pub const fn to_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_rgb_is_opaque() {
        let c = Color::linear_rgb(0.1, 0.2, 0.3);
        assert_eq!(c, Color { r: 0.1, g: 0.2, b: 0.3, a: 1.0 });
    }

    #[test]
    fn linear_rgba_keeps_alpha() {
        let c = Color::linear_rgba(0.1, 0.2, 0.3, 0.4);
        assert_eq!(c.a, 0.4);
    }

    #[test]
    fn white_and_black_constants() {
        assert_eq!(Color::WHITE, Color::linear_rgb(1.0, 1.0, 1.0));
        assert_eq!(Color::BLACK, Color::linear_rgb(0.0, 0.0, 0.0));
    }

    #[test]
    fn converts_to_vec4_and_array() {
        let c = Color::linear_rgba(0.2, 0.4, 0.6, 0.8);
        let v = c.to_vec4();
        assert_eq!((v.x, v.y, v.z, v.w), (0.2, 0.4, 0.6, 0.8));
        assert_eq!(c.to_array(), [0.2, 0.4, 0.6, 0.8]);
    }
}
