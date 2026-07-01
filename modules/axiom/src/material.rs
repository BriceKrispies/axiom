//! A material description an app adds to an `Assets<Material>` collection.

use axiom_kernel::Ratio;

use crate::color::Color;
use crate::texture::Texture;

/// A const `Ratio` from a literal, built in const context. The `match` lives in a
/// macro expansion, so the branchless lint skips it and the fallible conversion
/// never runs at runtime — the same shape as `color::unit!`.
macro_rules! ratio_lit {
    ($value:expr) => {{
        const R: Ratio = match Ratio::new($value) {
            Ok(r) => r,
            Err(_) => panic!("material ratio literal is finite"),
        };
        R
    }};
}

/// A material an app registers with the engine.
/// The engine provides the built-in basic-lit material: a base [`Color`], an
/// optional albedo [`Texture`], and the catalog scalar fields the contract names
/// — `emissive` (self-illumination), `roughness` (`0` mirror-smooth … `1` matte),
/// and `opacity` (`1` opaque; blends only once SPEC-04 lands the alpha path). A
/// `Material` value is a *description*; the engine resolves it into real material
/// data when the app runs. The final surface colour is the sampled albedo × the
/// base colour × the per-vertex colour, plus the emissive term.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Material {
    base_color: Color,
    texture: Option<Texture>,
    emissive: Color,
    roughness: Ratio,
    opacity: Ratio,
}

impl Material {
    /// A basic-lit material with the given linear base colour, no texture, no
    /// emissive, fully matte, and fully opaque.
    pub const fn lit(base_color: Color) -> Self {
        Material {
            base_color,
            texture: None,
            emissive: Color::BLACK,
            roughness: ratio_lit!(1.0),
            opacity: ratio_lit!(1.0),
        }
    }

    /// This material with an albedo [`Texture`] attached (sampled × base colour).
    pub const fn with_texture(mut self, texture: Texture) -> Self {
        self.texture = Some(texture);
        self
    }

    /// This material with a self-illumination (emissive) colour added on top of
    /// the lit result.
    pub const fn with_emissive(mut self, emissive: Color) -> Self {
        self.emissive = emissive;
        self
    }

    /// This material with a surface roughness (`0` = mirror-smooth, `1` = matte).
    pub const fn with_roughness(mut self, roughness: Ratio) -> Self {
        self.roughness = roughness;
        self
    }

    /// This material with an opacity (`1` = opaque). Carried now; visually blends
    /// only after SPEC-04 lands the alpha-blend path.
    pub const fn with_opacity(mut self, opacity: Ratio) -> Self {
        self.opacity = opacity;
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

    /// The material's emissive (self-illumination) colour.
    pub const fn emissive(self) -> Color {
        self.emissive
    }

    /// The material's surface roughness.
    pub const fn roughness(self) -> Ratio {
        self.roughness
    }

    /// The material's opacity.
    pub const fn opacity(self) -> Ratio {
        self.opacity
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
        assert_eq!(m.base_color(), Color::WHITE);
    }

    #[test]
    fn lit_defaults_the_catalog_fields() {
        let m = Material::lit(Color::WHITE);
        assert_eq!(m.emissive(), Color::BLACK);
        assert_eq!(m.roughness().get(), 1.0);
        assert_eq!(m.opacity().get(), 1.0);
    }

    #[test]
    fn catalog_builders_round_trip_distinct_from_defaults() {
        let half = || Ratio::new(0.5).expect("finite");
        let m = Material::lit(Color::WHITE)
            .with_emissive(Color::WHITE)
            .with_roughness(half())
            .with_opacity(half());
        assert_eq!(m.emissive(), Color::WHITE);
        assert_eq!(m.roughness().get(), 0.5);
        assert_eq!(m.opacity().get(), 0.5);
        // Equality requires every field: a differing roughness breaks it.
        let other = Material::lit(Color::WHITE).with_emissive(Color::WHITE).with_opacity(half());
        assert_ne!(m, other);
    }
}
