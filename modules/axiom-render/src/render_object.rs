//! Render-facing object: the per-object binding the command builder draws.

use axiom_math::Mat4;

use crate::render_pipeline_kind::RenderPipelineKind;

/// One render-facing object: a stable identity (`id`) + world transform
/// (`Mat4`) + mesh index (into [`crate::RenderInput::meshes`]) + material
/// index + the per-object binding channels the object-contract carries —
/// `texture_id` (a per-object albedo override; `0` = inherit the material's
/// texture), `pipeline` (which shader/pipeline draws it; defaults to
/// [`RenderPipelineKind::BASIC_LIT`]), and `tag` (the semantic kind rolled in
/// from the scene, `0` = untagged) — plus a visibility flag. The id, tag and the
/// resolved texture/pipeline ride through to the object's `DrawIndexed` /
/// `SetPipeline` / `SetMaterial` commands so a backend can preserve identity,
/// select a pipeline, and sample a texture from the command list alone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderObject {
    id: u64,
    world: Mat4,
    mesh_idx: u32,
    material_idx: u32,
    texture_id: u64,
    pipeline: u32,
    tag: u32,
    visible: bool,
}

impl RenderObject {
    /// An object drawn with the default basic-lit pipeline, no per-object texture
    /// override, and no tag. The common case; richer bindings use
    /// [`Self::bound`].
    pub const fn new(
        id: u64,
        world: Mat4,
        mesh_idx: u32,
        material_idx: u32,
        visible: bool,
    ) -> Self {
        RenderObject::bound(
            id,
            world,
            mesh_idx,
            material_idx,
            0,
            RenderPipelineKind::BASIC_LIT,
            0,
            visible,
        )
    }

    /// A fully-bound object: identity + transform + mesh + material + the
    /// per-object `texture_id` / `pipeline` / `tag` channels + visibility.
    #[allow(clippy::too_many_arguments)]
    pub const fn bound(
        id: u64,
        world: Mat4,
        mesh_idx: u32,
        material_idx: u32,
        texture_id: u64,
        pipeline: u32,
        tag: u32,
        visible: bool,
    ) -> Self {
        RenderObject {
            id,
            world,
            mesh_idx,
            material_idx,
            texture_id,
            pipeline,
            tag,
            visible,
        }
    }

    /// The object's stable identity, threaded into its `DrawIndexed` command.
    pub const fn id(&self) -> u64 {
        self.id
    }

    pub const fn world(&self) -> Mat4 {
        self.world
    }

    pub const fn mesh_idx(&self) -> u32 {
        self.mesh_idx
    }

    pub const fn material_idx(&self) -> u32 {
        self.material_idx
    }

    /// The per-object albedo texture override (`0` = inherit the material's
    /// texture id).
    pub const fn texture_id(&self) -> u64 {
        self.texture_id
    }

    /// The pipeline id this object selects (defaults to
    /// [`RenderPipelineKind::BASIC_LIT`]).
    pub const fn pipeline(&self) -> u32 {
        self.pipeline
    }

    /// The semantic kind rolled in from the scene (`0` = untagged).
    pub const fn tag(&self) -> u32 {
        self.tag
    }

    pub const fn visible(&self) -> bool {
        self.visible
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_defaults_the_binding_channels() {
        let o = RenderObject::new(42, Mat4::IDENTITY, 1, 2, true);
        assert_eq!(o.id(), 42);
        assert_eq!(o.world(), Mat4::IDENTITY);
        assert_eq!(o.mesh_idx(), 1);
        assert_eq!(o.material_idx(), 2);
        assert_eq!(o.texture_id(), 0);
        assert_eq!(o.pipeline(), RenderPipelineKind::BASIC_LIT);
        assert_eq!(o.tag(), 0);
        assert!(o.visible());
    }

    #[test]
    fn bound_carries_every_channel() {
        let o = RenderObject::bound(7, Mat4::IDENTITY, 3, 4, 55, 2, 9, false);
        assert_eq!(o.texture_id(), 55);
        assert_eq!(o.pipeline(), 2);
        assert_eq!(o.tag(), 9);
        assert!(!o.visible());
    }

    #[test]
    fn equality_requires_all_fields() {
        let a = RenderObject::new(1, Mat4::IDENTITY, 0, 0, true);
        let b = RenderObject::new(1, Mat4::IDENTITY, 0, 0, true);
        let c = RenderObject::new(1, Mat4::IDENTITY, 0, 0, false);
        let d = RenderObject::new(2, Mat4::IDENTITY, 0, 0, true);
        let e = RenderObject::bound(
            1,
            Mat4::IDENTITY,
            0,
            0,
            1,
            RenderPipelineKind::BASIC_LIT,
            0,
            true,
        );
        let f = RenderObject::bound(1, Mat4::IDENTITY, 0, 0, 0, 2, 0, true);
        let g = RenderObject::bound(
            1,
            Mat4::IDENTITY,
            0,
            0,
            0,
            RenderPipelineKind::BASIC_LIT,
            3,
            true,
        );
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
        assert_ne!(a, e);
        assert_ne!(a, f);
        assert_ne!(a, g);
    }
}
