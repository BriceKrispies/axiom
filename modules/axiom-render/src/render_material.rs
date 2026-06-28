//! Render-facing material data.

use axiom_kernel::Ratio;
use axiom_math::{Vec3, Vec4};

/// A const `Ratio` from a literal, built in const context. The `match` lives in a
/// macro expansion, so the branchless lint skips it and the fallible conversion
/// never runs at runtime.
macro_rules! ratio_lit {
    ($value:expr) => {{
        const R: Ratio = match Ratio::new($value) {
            Ok(r) => r,
            Err(_) => panic!("material ratio literal is finite"),
        };
        R
    }};
}

/// Render-facing material: an opaque id, a base colour, an albedo texture id
/// (`0` = untextured), and the catalog scalar fields the contract names —
/// `emissive` (self-illumination colour), `roughness` (`0` mirror-smooth … `1`
/// matte), and `opacity` (`1` opaque). The receipt fully describes each draw so
/// the deterministic `RenderCommandList` captures the material's full surface.
/// The texture id is neutral — the renderer never loads pixels; it carries the
/// binding so the command stream and its receipt capture which texture a draw
/// samples. `opacity` is carried now and blends visually only once SPEC-04 lands
/// the alpha-blend state (until then a translucent material renders REPLACE).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderMaterial {
    id: u64,
    base_color: Vec4,
    emissive: Vec3,
    roughness: Ratio,
    opacity: Ratio,
    texture_id: u64,
}

impl RenderMaterial {
    /// An untextured, non-emissive, fully-matte, fully-opaque material.
    pub const fn new(id: u64, base_color: Vec4) -> Self {
        RenderMaterial::new_textured(id, base_color, 0)
    }

    /// A textured material with default catalog fields (`texture_id` `0` =
    /// untextured).
    pub const fn new_textured(id: u64, base_color: Vec4, texture_id: u64) -> Self {
        RenderMaterial::new_lit(
            id,
            base_color,
            Vec3::ZERO,
            ratio_lit!(1.0),
            ratio_lit!(1.0),
            texture_id,
        )
    }

    /// A material with the full catalog surface specified.
    pub const fn new_lit(
        id: u64,
        base_color: Vec4,
        emissive: Vec3,
        roughness: Ratio,
        opacity: Ratio,
        texture_id: u64,
    ) -> Self {
        RenderMaterial {
            id,
            base_color,
            emissive,
            roughness,
            opacity,
            texture_id,
        }
    }

    pub const fn id(&self) -> u64 {
        self.id
    }

    pub const fn base_color(&self) -> Vec4 {
        self.base_color
    }

    /// The self-illumination colour added on top of the lit result.
    pub const fn emissive(&self) -> Vec3 {
        self.emissive
    }

    /// The surface roughness (`0` = mirror-smooth, `1` = matte).
    pub const fn roughness(&self) -> Ratio {
        self.roughness
    }

    /// The material opacity (`1` = opaque); blends only after SPEC-04.
    pub const fn opacity(&self) -> Ratio {
        self.opacity
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

    #[test]
    fn default_catalog_fields_and_lit_round_trip() {
        // The plain constructors default the catalog fields.
        let basic = RenderMaterial::new(1, Vec4::ONE);
        assert_eq!(basic.emissive(), Vec3::ZERO);
        assert_eq!(basic.roughness().get(), 1.0);
        assert_eq!(basic.opacity().get(), 1.0);

        // new_lit carries every field, read back distinct from the defaults.
        let half = Ratio::new(0.5).expect("finite");
        let lit = RenderMaterial::new_lit(2, Vec4::ONE, Vec3::new(0.0, 1.0, 0.0), half, half, 9);
        assert_eq!(lit.emissive(), Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(lit.roughness().get(), 0.5);
        assert_eq!(lit.opacity().get(), 0.5);
        assert_eq!(lit.texture_id(), 9);
        // Equality requires every new field: a differing emissive breaks it.
        let other = RenderMaterial::new_lit(2, Vec4::ONE, Vec3::ZERO, half, half, 9);
        assert_ne!(lit, other);
    }
}
