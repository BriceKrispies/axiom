//! A material description an app adds to an `Assets<Material>` collection.

use crate::color::Color;

/// A material an app registers with the engine.
///
/// Today the engine provides the built-in basic-lit material, parameterised by
/// a base [`Color`]. A `Material` value is a *description*; the engine resolves
/// it into real material data (via `axiom-resources`) when the app runs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Material {
    base_color: Color,
}

impl Material {
    /// A basic-lit material with the given linear base colour.
    pub const fn lit(base_color: Color) -> Self {
        Material { base_color }
    }

    /// The material's base colour.
    pub const fn base_color(self) -> Color {
        self.base_color
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lit_carries_its_base_color() {
        let m = Material::lit(Color::linear_rgb(0.8, 0.2, 0.2));
        assert_eq!(m.base_color(), Color::linear_rgb(0.8, 0.2, 0.2));
    }
}
