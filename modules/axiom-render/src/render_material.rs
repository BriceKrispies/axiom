//! Render-facing material data.

use axiom_math::Vec4;

/// Render-facing material: an opaque id, a base colour, and an opaque albedo
/// texture id (`0` = untextured). The vertical slice only supports the "basic
/// lit" pipeline; richer material models live in future iterations. The texture
/// id is neutral — the renderer never loads pixels; it carries the binding so
/// the command stream and its receipt capture which texture a draw samples.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderMaterial {
    id: u64,
    base_color: Vec4,
    texture_id: u64,
}

impl RenderMaterial {
    /// An untextured material (texture id `0`).
    pub const fn new(id: u64, base_color: Vec4) -> Self {
        RenderMaterial::new_textured(id, base_color, 0)
    }

    /// A material that samples albedo texture `texture_id` (`0` = untextured).
    pub const fn new_textured(id: u64, base_color: Vec4, texture_id: u64) -> Self {
        RenderMaterial {
            id,
            base_color,
            texture_id,
        }
    }

    pub const fn id(&self) -> u64 {
        self.id
    }

    pub const fn base_color(&self) -> Vec4 {
        self.base_color
    }

    /// The albedo texture id this material samples; `0` means untextured.
    pub const fn texture_id(&self) -> u64 {
        self.texture_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_round_trip() {
        let m = RenderMaterial::new(3, Vec4::new(0.5, 0.5, 0.5, 1.0));
        assert_eq!(m.id(), 3);
        assert_eq!(m.base_color(), Vec4::new(0.5, 0.5, 0.5, 1.0));
        // The plain constructor is untextured.
        assert_eq!(m.texture_id(), 0);
    }

    #[test]
    fn textured_constructor_carries_its_texture_id() {
        let m = RenderMaterial::new_textured(3, Vec4::ONE, 42);
        assert_eq!(m.texture_id(), 42);
    }

    #[test]
    fn equality_requires_all_fields() {
        let a = RenderMaterial::new(1, Vec4::ONE);
        let b = RenderMaterial::new(1, Vec4::ONE);
        let c = RenderMaterial::new(1, Vec4::ZERO);
        // A differing texture id alone breaks equality.
        let d = RenderMaterial::new_textured(1, Vec4::ONE, 7);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }
}
