//! Pass 3 — the style pass: the deterministic bundle of light model + visual
//! style the app renders with.
//!
//! `PenaltyStylePass` is the one object that carries "how the diorama looks":
//! the [`PenaltyLightModel`] used to flat-shade world materials and the
//! [`PenaltyVisualStyle`] retro 32-bit descriptor. It owns no ordering (that is the
//! render plan's job) and no scene data — only style. It is built from fixed
//! constants and is identical on every build.

use crate::soccer_penalty::low_poly_assets::Rgba;
use crate::soccer_penalty::penalty_light::PenaltyLightModel;
use crate::soccer_penalty::penalty_materials::{material, PenaltyMaterial};
use crate::soccer_penalty::penalty_style::PenaltyVisualStyle;
use axiom_math::Vec3;

/// The deterministic visual style pass: light model + retro 32-bit style descriptor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyStylePass {
    pub light: PenaltyLightModel,
    pub style: PenaltyVisualStyle,
}

impl PenaltyStylePass {
    /// The fixed Stage 1 / Pass 3 style pass.
    pub const fn stage1() -> Self {
        Self {
            light: PenaltyLightModel::stage1(),
            style: PenaltyVisualStyle::stage1(),
        }
    }

    /// Resolve a material and flat-shade it for a face normal. Unlit materials
    /// (HUD, blob shadows) are returned at their base color unchanged; lit
    /// materials are shaded by the light model.
    pub fn shade(&self, mat: &PenaltyMaterial, normal: Vec3) -> Rgba {
        if mat.unlit { mat.base_color } else { self.light.shade(mat.base_color, normal) }
    }

    /// Convenience: resolve by id, then shade.
    pub fn shade_id(
        &self,
        id: crate::soccer_penalty::penalty_materials::PenaltyMaterialId,
        normal: Vec3,
    ) -> Rgba {
        self.shade(&material(id), normal)
    }
}
