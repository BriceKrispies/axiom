//! The backend-neutral SDF (signed-distance-field) scene contract for a
//! raymarch presentation pass.
//! `SdfScene` is the raymarch peer of [`crate::FramePacket`]'s triangle draws:
//! it carries everything a backend needs to *march* a frame's SDF shapes, and
//! only primitives — no GPU, browser, DOM, render-module, or scene types — so
//! both render backends (the GPU backend and the Canvas 2D software backend)
//! can name, store, and evaluate it. Host owns the *contract*; it marches
//! nothing. A backend evaluates the same primitives two ways (a WGSL shader on
//! the GPU, a branchless CPU fold in the software backend), but the data they
//! read is identical, so the two backends stay in parity.
//! Each primitive is evaluated in its own local frame: a sample point is
//! transformed by `inv_transform` (column-major world→local) into local space,
//! the canonical local SDF is evaluated there, and the distance is rescaled by
//! the uniform scale the transform carries. This supports translation, rotation
//! and uniform scale exactly; non-uniform scale is intentionally out of scope
//! (it makes the field non-metric).

/// One SDF primitive, evaluated in its own local frame.
/// `kind` selects the canonical local distance function ([`Self::SPHERE`],
/// [`Self::BOX`], [`Self::PLANE`]); `inv_transform` is the column-major
/// world→local matrix; `params` carries the primitive's local dimensions plus
/// the transform's uniform scale (so the backend can rescale the local distance
/// back to world units); `color` is the linear RGBA surface colour.
/// Parameter layout by kind:
/// - sphere: `[radius, _, _, uniform_scale]`
/// - box:    `[half_x, half_y, half_z, uniform_scale]`
/// - plane:  `[_, _, _, uniform_scale]` (the plane is `y = 0` in local space)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SdfPrimitive {
    kind: u32,
    inv_transform: [f32; 16],
    params: [f32; 4],
    color: [f32; 4],
}

impl SdfPrimitive {
    /// `kind` discriminant for a sphere of radius `params[0]`.
    pub const SPHERE: u32 = 0;
    /// `kind` discriminant for an axis-aligned box of half-extents `params[0..3]`.
    pub const BOX: u32 = 1;
    /// `kind` discriminant for the local `y = 0` plane.
    pub const PLANE: u32 = 2;

    /// A primitive with its `kind`, column-major world→local `inv_transform`,
    /// `params` (local dimensions in `[0..3]`, uniform scale in `[3]`), and
    /// linear RGBA `color`.
    pub const fn new(
        kind: u32,
        inv_transform: [f32; 16],
        params: [f32; 4],
        color: [f32; 4],
    ) -> Self {
        SdfPrimitive {
            kind,
            inv_transform,
            params,
            color,
        }
    }

    /// The primitive's kind discriminant.
    pub const fn kind(&self) -> u32 {
        self.kind
    }

    /// The column-major world→local transform.
    pub const fn inv_transform(&self) -> [f32; 16] {
        self.inv_transform
    }

    /// The local dimensions in `[0..3]` and the transform's uniform scale in `[3]`.
    pub const fn params(&self) -> [f32; 4] {
        self.params
    }

    /// The linear RGBA surface colour.
    pub const fn color(&self) -> [f32; 4] {
        self.color
    }
}

/// The frame's SDF scene: the primitives to march, the camera's forward and
/// inverse view-projection, the camera world position, and the march tunables.
/// The scene is **self-contained**: it carries everything a backend needs to
/// march *and* depth-composite a frame's SDF shapes against the meshes, without
/// reaching for a [`crate::FrameCamera`]. That is required, not merely tidy — a
/// backend may evaluate an `SdfScene` with no `FramePacket` in hand (the GPU
/// backend's live/offscreen path takes raw instance batches, never a camera), so
/// the contract itself must hold the camera math both backends consume.
/// `view_proj` (world→clip) projects each world hit to the **same** NDC z the
/// triangle pass writes, so SDF depth composites with the meshes; `inv_view_proj`
/// (clip→world) *un*projects each pixel into a world ray; `camera_world_pos` is
/// the ray origin. `march` is `[max_dist, hit_epsilon, _, _]`; the per-pixel step
/// *count* is a backend constant (a branchless CPU marcher iterates a fixed
/// number of steps), not data.
#[derive(Debug, Clone, PartialEq)]
pub struct SdfScene {
    primitives: Vec<SdfPrimitive>,
    view_proj: [f32; 16],
    inv_view_proj: [f32; 16],
    camera_world_pos: [f32; 3],
    march: [f32; 4],
}

impl SdfScene {
    /// An SDF scene from its `primitives`, the column-major `view_proj`
    /// (clip from world, for depth projection), the `inv_view_proj` (world from
    /// clip, for ray reconstruction), the `camera_world_pos` ray origin, and the
    /// `march` tunables `[max_dist, hit_epsilon, _, _]`.
    pub fn new(
        primitives: Vec<SdfPrimitive>,
        view_proj: [f32; 16],
        inv_view_proj: [f32; 16],
        camera_world_pos: [f32; 3],
        march: [f32; 4],
    ) -> Self {
        SdfScene {
            primitives,
            view_proj,
            inv_view_proj,
            camera_world_pos,
            march,
        }
    }

    /// The primitives to march, in input order.
    pub fn primitives(&self) -> &[SdfPrimitive] {
        &self.primitives
    }

    /// The column-major forward view-projection (clip from world), used to
    /// depth-project each world hit to the mesh pass's NDC z.
    pub const fn view_proj(&self) -> [f32; 16] {
        self.view_proj
    }

    /// The column-major inverse view-projection (world from clip).
    pub const fn inv_view_proj(&self) -> [f32; 16] {
        self.inv_view_proj
    }

    /// The camera's world position — the per-pixel ray origin.
    pub const fn camera_world_pos(&self) -> [f32; 3] {
        self.camera_world_pos
    }

    /// The march tunables `[max_dist, hit_epsilon, _, _]`.
    pub const fn march(&self) -> [f32; 4] {
        self.march
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mat(seed: f32) -> [f32; 16] {
        [seed; 16]
    }

    #[test]
    fn primitive_accessors_round_trip() {
        let p = SdfPrimitive::new(
            SdfPrimitive::BOX,
            mat(2.0),
            [1.0, 2.0, 3.0, 0.5],
            [0.1, 0.2, 0.3, 1.0],
        );
        assert_eq!(p.kind(), SdfPrimitive::BOX);
        assert_eq!(p.inv_transform(), mat(2.0));
        assert_eq!(p.params(), [1.0, 2.0, 3.0, 0.5]);
        assert_eq!(p.color(), [0.1, 0.2, 0.3, 1.0]);
    }

    #[test]
    fn primitive_kind_discriminants_are_distinct() {
        assert_eq!(SdfPrimitive::SPHERE, 0);
        assert_eq!(SdfPrimitive::BOX, 1);
        assert_eq!(SdfPrimitive::PLANE, 2);
    }

    #[test]
    fn primitive_derives_are_exercised() {
        let p = SdfPrimitive::new(SdfPrimitive::SPHERE, mat(1.0), [1.0; 4], [1.0; 4]);
        assert_eq!(p, p);
        assert_eq!(
            p,
            SdfPrimitive::new(SdfPrimitive::SPHERE, mat(1.0), [1.0; 4], [1.0; 4])
        );
        assert_ne!(
            p,
            SdfPrimitive::new(SdfPrimitive::BOX, mat(1.0), [1.0; 4], [1.0; 4])
        );
        assert!(format!("{p:?}").contains("SdfPrimitive"));
    }

    #[test]
    fn scene_accessors_round_trip() {
        let p = SdfPrimitive::new(
            SdfPrimitive::SPHERE,
            mat(1.0),
            [0.75, 0.0, 0.0, 1.0],
            [1.0; 4],
        );
        let scene = SdfScene::new(
            vec![p],
            mat(2.0),
            mat(3.0),
            [4.0, 5.0, 6.0],
            [100.0, 0.001, 0.0, 0.0],
        );
        assert_eq!(scene.primitives(), &[p]);
        assert_eq!(scene.view_proj(), mat(2.0));
        assert_eq!(scene.inv_view_proj(), mat(3.0));
        assert_eq!(scene.camera_world_pos(), [4.0, 5.0, 6.0]);
        assert_eq!(scene.march(), [100.0, 0.001, 0.0, 0.0]);
    }

    #[test]
    fn scene_derives_are_exercised() {
        let scene = SdfScene::new(Vec::new(), mat(0.0), mat(0.0), [0.0; 3], [0.0; 4]);
        assert_eq!(scene, scene.clone());
        assert_ne!(
            scene,
            SdfScene::new(Vec::new(), mat(0.0), mat(1.0), [0.0; 3], [0.0; 4])
        );
        assert!(format!("{scene:?}").contains("SdfScene"));
        assert!(scene.primitives().is_empty());
    }
}
