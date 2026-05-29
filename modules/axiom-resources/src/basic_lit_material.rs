//! The built-in deterministic basic-lit material.

use axiom_math::Vec4;

use crate::material_data::MaterialData;
use crate::resource_id::ResourceId;

/// Build the canonical "basic lit" material with the given base
/// colour. The material has no texture; the vertical slice uses
/// per-vertex colour only.
pub fn build_basic_lit_material(id: ResourceId, base_color: Vec4) -> MaterialData {
    MaterialData::new(id, "axiom.builtin.basic_lit", base_color, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_is_built_with_supplied_color() {
        let m = build_basic_lit_material(
            ResourceId::from_raw(1),
            Vec4::new(1.0, 0.5, 0.0, 1.0),
        );
        assert_eq!(m.base_color(), Vec4::new(1.0, 0.5, 0.0, 1.0));
        assert!(m.texture().is_none());
    }

    #[test]
    fn material_is_deterministic_across_runs() {
        let a = build_basic_lit_material(ResourceId::from_raw(1), Vec4::ONE);
        let b = build_basic_lit_material(ResourceId::from_raw(1), Vec4::ONE);
        assert_eq!(a, b);
    }
}
