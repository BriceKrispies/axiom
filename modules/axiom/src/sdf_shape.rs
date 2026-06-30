//! `SdfShape`: a raymarched signed-distance primitive (sphere / box / plane) an
//! app attaches to a node. The engine marches it and composites the result with
//! the rasterized meshes — on the GPU backend and, preserving the engine's
//! software-fallback property, on the Canvas 2D backend too.
//!
//! An authoring value type, like [`crate::prelude::Renderable`] or
//! [`crate::prelude::Bounds`]: spawned in a bundle and realized into the scene's
//! SDF-shape component. The node's transform places (and uniformly scales) it.

use axiom_kernel::Meters;
use axiom_math::Vec3;

use crate::color::Color;

/// A raymarched SDF shape authored on a node. Build one with [`Self::sphere`],
/// [`Self::cuboid`], or [`Self::plane`]; the node's world transform places and
/// uniformly scales it. `dims` carries the local size the kind needs (a sphere's
/// radius in `x`, a box's half-extents, nothing for a plane).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SdfShape {
    kind: u32,
    dims: Vec3,
    color: Color,
}

impl SdfShape {
    /// Sphere / box / plane discriminants — they match the scene and backend SDF
    /// primitive kinds, so the value flows through unchanged.
    pub(crate) const SPHERE: u32 = 0;
    pub(crate) const BOX: u32 = 1;
    pub(crate) const PLANE: u32 = 2;

    /// A raymarched sphere of the given `radius` and `color`.
    pub fn sphere(radius: Meters, color: Color) -> Self {
        let r = radius.get();
        SdfShape {
            kind: Self::SPHERE,
            dims: Vec3::new(r, r, r),
            color,
        }
    }

    /// A raymarched axis-aligned box of the given local `half_extents` and `color`.
    pub const fn cuboid(half_extents: Vec3, color: Color) -> Self {
        SdfShape {
            kind: Self::BOX,
            dims: half_extents,
            color,
        }
    }

    /// A raymarched ground plane (`y = 0` in the node's local frame) of `color`.
    pub const fn plane(color: Color) -> Self {
        SdfShape {
            kind: Self::PLANE,
            dims: Vec3::ZERO,
            color,
        }
    }

    /// The kind discriminant (sphere / box / plane).
    pub(crate) const fn kind(&self) -> u32 {
        self.kind
    }

    /// The local dimensions (sphere radius in `x`; box half-extents; plane unused).
    pub(crate) const fn dims(&self) -> Vec3 {
        self.dims
    }

    /// The linear-RGB surface colour.
    pub(crate) const fn color(&self) -> Color {
        self.color
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col() -> Color {
        Color::linear_rgb(
            axiom_kernel::Ratio::new(0.2).unwrap(),
            axiom_kernel::Ratio::new(0.4).unwrap(),
            axiom_kernel::Ratio::new(0.6).unwrap(),
        )
    }

    #[test]
    fn sphere_stores_radius_in_each_dim() {
        let s = SdfShape::sphere(Meters::new(2.0).unwrap(), col());
        assert_eq!(s.kind(), SdfShape::SPHERE);
        assert_eq!(s.dims(), Vec3::new(2.0, 2.0, 2.0));
        assert_eq!(s.color(), col());
    }

    #[test]
    fn cuboid_keeps_half_extents() {
        let s = SdfShape::cuboid(Vec3::new(1.0, 2.0, 3.0), col());
        assert_eq!(s.kind(), SdfShape::BOX);
        assert_eq!(s.dims(), Vec3::new(1.0, 2.0, 3.0));
    }

    #[test]
    fn plane_has_no_dimensions() {
        let s = SdfShape::plane(col());
        assert_eq!(s.kind(), SdfShape::PLANE);
        assert_eq!(s.dims(), Vec3::ZERO);
        assert_eq!(s.color(), col());
        // Copy + Debug.
        let c = s;
        assert_eq!(s, c);
        assert!(format!("{s:?}").contains("SdfShape"));
    }
}
