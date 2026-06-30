//! Render-facing SDF shape: a raymarched primitive's neutral inputs.

use axiom_math::{Mat4, Vec3, Vec4};

/// One render-facing signed-distance shape: a `kind` discriminant (sphere / box
/// / plane, matching [`axiom_host::SdfPrimitive`]'s kinds), the full `world`
/// transform that places it, its **local** `dims` (sphere radius in `x`; box
/// half-extents; plane unused), and its linear-RGBA `color`.
///
/// This is scene-independent neutral data — no `SceneNodeId`s, no scene types.
/// [`crate::RenderApi::build_frame_packet`] inverts `world` into the backend's
/// world→local matrix and extracts the transform's uniform scale, producing the
/// backend-neutral SDF primitive the render backends march.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderSdf {
    kind: u32,
    world: Mat4,
    dims: Vec3,
    color: Vec4,
}

impl RenderSdf {
    pub const fn new(kind: u32, world: Mat4, dims: Vec3, color: Vec4) -> Self {
        RenderSdf {
            kind,
            world,
            dims,
            color,
        }
    }

    pub const fn kind(&self) -> u32 {
        self.kind
    }

    pub const fn world(&self) -> Mat4 {
        self.world
    }

    pub const fn dims(&self) -> Vec3 {
        self.dims
    }

    pub const fn color(&self) -> Vec4 {
        self.color
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_round_trip() {
        let s = RenderSdf::new(1, Mat4::IDENTITY, Vec3::new(1.0, 2.0, 3.0), Vec4::ONE);
        assert_eq!(s.kind(), 1);
        assert_eq!(s.world(), Mat4::IDENTITY);
        assert_eq!(s.dims(), Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(s.color(), Vec4::ONE);
    }

    #[test]
    fn equality_requires_all_fields() {
        let a = RenderSdf::new(0, Mat4::IDENTITY, Vec3::ONE, Vec4::ONE);
        let b = RenderSdf::new(0, Mat4::IDENTITY, Vec3::ONE, Vec4::ONE);
        assert_eq!(a, b);
        assert_ne!(a, RenderSdf::new(2, Mat4::IDENTITY, Vec3::ONE, Vec4::ONE));
        assert_ne!(a, RenderSdf::new(0, Mat4::ZERO, Vec3::ONE, Vec4::ONE));
        assert_ne!(a, RenderSdf::new(0, Mat4::IDENTITY, Vec3::ZERO, Vec4::ONE));
        assert_ne!(a, RenderSdf::new(0, Mat4::IDENTITY, Vec3::ONE, Vec4::ZERO));
    }
}
