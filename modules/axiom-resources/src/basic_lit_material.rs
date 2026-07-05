//! The built-in deterministic basic-lit material.

use axiom_kernel::Ratio;
use axiom_math::{Vec3, Vec4};

use crate::material_data::MaterialData;
use crate::resource_id::ResourceId;

/// Build the canonical "basic lit" material with the given base
/// colour and no texture (per-vertex colour only). A thin wrapper over
/// [`build_textured_lit_material`] with no texture.
pub fn build_basic_lit_material(id: ResourceId, base_color: Vec4) -> MaterialData {
    build_textured_lit_material(id, base_color, None)
}

/// Build the canonical "basic lit" material with a base colour and an
/// optional albedo texture. The texture (when present) is sampled and
/// multiplied by the base colour and per-vertex colour at draw time.
pub fn build_textured_lit_material(
    id: ResourceId,
    base_color: Vec4,
    texture: Option<ResourceId>,
) -> MaterialData {
    MaterialData::new(id, "axiom.builtin.basic_lit", base_color, texture)
}

/// Build a full-catalog lit material: base colour + optional texture + the
/// `emissive` / `roughness` / `opacity` catalog fields the render tier accepts,
/// so a complete material is authorable through the resource table (audit M9).
pub fn build_lit_material(
    id: ResourceId,
    base_color: Vec4,
    texture: Option<ResourceId>,
    emissive: Vec3,
    roughness: Ratio,
    opacity: Ratio,
) -> MaterialData {
    MaterialData::new_lit(
        id,
        "axiom.builtin.lit",
        base_color,
        texture,
        emissive,
        roughness,
        opacity,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_is_built_with_supplied_color() {
        let m = build_basic_lit_material(ResourceId::from_raw(1), Vec4::new(1.0, 0.5, 0.0, 1.0));
        assert_eq!(m.base_color(), Vec4::new(1.0, 0.5, 0.0, 1.0));
        assert!(m.texture().is_none());
    }

    #[test]
    fn material_is_deterministic_across_runs() {
        let a = build_basic_lit_material(ResourceId::from_raw(1), Vec4::ONE);
        let b = build_basic_lit_material(ResourceId::from_raw(1), Vec4::ONE);
        assert_eq!(a, b);
    }

    #[test]
    fn textured_material_carries_its_texture_id() {
        let tex = ResourceId::from_raw(7);
        let m = build_textured_lit_material(ResourceId::from_raw(1), Vec4::ONE, Some(tex));
        assert_eq!(m.texture(), Some(tex));
        assert_eq!(m.base_color(), Vec4::ONE);
    }

    #[test]
    fn lit_material_carries_the_full_catalog() {
        let half = Ratio::new(0.5).expect("finite");
        let m = build_lit_material(
            ResourceId::from_raw(1),
            Vec4::ONE,
            Some(ResourceId::from_raw(2)),
            Vec3::new(0.0, 1.0, 0.0),
            half,
            half,
        );
        assert_eq!(m.emissive(), Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(m.roughness().get(), 0.5);
        assert_eq!(m.opacity().get(), 0.5);
        assert_eq!(m.texture(), Some(ResourceId::from_raw(2)));
    }
}
