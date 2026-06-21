//! A material description an app adds to an `Assets<Material>` collection.

use crate::color::Color;
use crate::texture::Texture;

/// A material an app registers with the engine.
///
/// Today the engine provides the built-in basic-lit material, parameterised by a
/// base [`Color`] and an optional albedo [`Texture`]. A `Material` value is a
/// *description*; the engine resolves it into real material data (via
/// `axiom-resources`) when the app runs. The final surface colour is the sampled
/// albedo × the base colour × the per-vertex colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Material {
    base_color: Color,
    texture: Option<Texture>,
}

impl Material {
    /// A basic-lit material with the given linear base colour and no texture.
    pub const fn lit(base_color: Color) -> Self {
        Material {
            base_color,
            texture: None,
        }
    }

    /// This material with an albedo [`Texture`] attached (sampled × base colour).
    pub const fn with_texture(mut self, texture: Texture) -> Self {
        self.texture = Some(texture);
        self
    }

    /// The material's base colour.
    pub const fn base_color(self) -> Color {
        self.base_color
    }

    /// The material's albedo texture, if any.
    pub const fn texture(self) -> Option<Texture> {
        self.texture
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lit_carries_its_base_color() {
        use axiom_kernel::Ratio;
        let red = || {
            Color::linear_rgb(
                Ratio::new(0.8).expect("authored colour channel is finite"),
                Ratio::new(0.2).expect("authored colour channel is finite"),
                Ratio::new(0.2).expect("authored colour channel is finite"),
            )
        };
        let m = Material::lit(red());
        assert_eq!(m.base_color(), red());
        assert_eq!(m.texture(), None);
    }

    #[test]
    fn with_texture_attaches_an_albedo() {
        let m = Material::lit(Color::WHITE).with_texture(Texture::Checker);
        assert_eq!(m.texture(), Some(Texture::Checker));
        // The base colour is preserved.
        assert_eq!(m.base_color(), Color::WHITE);
    }
}
