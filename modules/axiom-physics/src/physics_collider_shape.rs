//! The deterministic collider shapes, as a flat *tagged value*.
//! A `PhysicsColliderShape` is **not** a payload-carrying enum. It is a tag
//! ([`PhysicsShapeKind`]) plus a uniform set of geometry fields, so the broad
//! phase, narrow phase, and queries read its parameters with plain field access
//! and dispatch on `kind().index()` — never a `match` on variant payloads, which
//! the Branchless Law forbids. The four constructors validate their inputs and
//! pack them into the flat representation:
//! | kind    | `half_extents`        | `radius` | `normal` | `offset`   |
//! |---------|-----------------------|----------|----------|------------|
//! | Sphere  | `(r, r, r)`           | `r`      | `ZERO`   | `0`        |
//! | Box     | the box half-extents  | `0`      | `ZERO`   | `0`        |
//! | Capsule | `(r, hh + r, r)`      | `r`      | `ZERO`   | `0`        |
//! | Plane   | `ZERO`                | `0`      | unit `n` | distance   |
//! `half_extents` is the shape's **local axis-aligned half-size** for the finite
//! kinds (the value the broad phase turns into a world AABB). Planes are infinite
//! and carry no finite extent; their `normal`/`offset` define the half-space.
//! Lengths enter through the kernel [`Meters`] quantity and directions/extents
//! through math [`Vec3`]; the packed fields are private `f32`/`Vec3`, never a
//! public naked float.

use axiom_kernel::Meters;
use axiom_math::Vec3;

use crate::physics_error::PhysicsError;
use crate::physics_error_code::PhysicsErrorCode;
use crate::physics_result::PhysicsResult;
use crate::physics_shape_kind::PhysicsShapeKind;

/// The geometric shape of a collider — a flat, branchless tagged value.
/// The four classical primitives are supported. They are validated at
/// construction and surfaced in snapshots. The broad phase and queries handle all
/// four; the narrow-phase contact generator handles the sphere/sphere,
/// sphere/plane, sphere/box, and box/plane pairings (capsule contacts are a
/// documented deferral — see `ROADMAP.md`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhysicsColliderShape {
    kind: PhysicsShapeKind,
    half_extents: Vec3,
    radius: f32,
    normal: Vec3,
    offset: f32,
}

impl PhysicsColliderShape {
    /// A sphere, rejecting a non-positive radius.
    pub(crate) fn sphere(radius: Meters) -> PhysicsResult<Self> {
        let r = radius.get();
        [
            Err(PhysicsError::invalid_collider_shape(
                "sphere radius must be greater than zero",
            )),
            Ok(PhysicsColliderShape {
                kind: PhysicsShapeKind::Sphere,
                half_extents: Vec3::new(r, r, r),
                radius: r,
                normal: Vec3::ZERO,
                offset: 0.0,
            }),
        ][(r > 0.0) as usize]
    }

    /// A box, rejecting any non-finite or non-positive half-extent.
    pub(crate) fn box_shape(half_extents: Vec3) -> PhysicsResult<Self> {
        let h = half_extents;
        let valid = h.x.is_finite()
            & h.y.is_finite()
            & h.z.is_finite()
            & (h.x > 0.0)
            & (h.y > 0.0)
            & (h.z > 0.0);
        [
            Err(PhysicsError::invalid_collider_shape(
                "box half-extents must be finite and greater than zero on every axis",
            )),
            Ok(PhysicsColliderShape {
                kind: PhysicsShapeKind::Box,
                half_extents,
                radius: 0.0,
                normal: Vec3::ZERO,
                offset: 0.0,
            }),
        ][valid as usize]
    }

    /// A capsule (local Y axis), rejecting a non-positive radius or a negative
    /// half-height. Its local AABB half-size is `(r, half_height + r, r)`.
    pub(crate) fn capsule(radius: Meters, half_height: Meters) -> PhysicsResult<Self> {
        let r = radius.get();
        let hh = half_height.get();
        let valid = (r > 0.0) & (hh >= 0.0);
        [
            Err(PhysicsError::invalid_collider_shape(
                "capsule radius must be > 0 and half-height must be >= 0",
            )),
            Ok(PhysicsColliderShape {
                kind: PhysicsShapeKind::Capsule,
                half_extents: Vec3::new(r, hh + r, r),
                radius: r,
                normal: Vec3::ZERO,
                offset: 0.0,
            }),
        ][valid as usize]
    }

    /// A plane, rejecting a non-finite or zero-length normal. The normal is
    /// stored **unit-length** (math `normalize` both validates and normalizes),
    /// so the narrow phase reads a ready-to-use half-space `n · x = offset`.
    pub(crate) fn plane(normal: Vec3, distance: Meters) -> PhysicsResult<Self> {
        normal
            .normalize()
            .map_err(|cause| {
                PhysicsError::with_math(
                    PhysicsErrorCode::InvalidColliderShape,
                    "plane normal must be finite and non-zero",
                    cause,
                )
            })
            .map(|unit| PhysicsColliderShape {
                kind: PhysicsShapeKind::Plane,
                half_extents: Vec3::ZERO,
                radius: 0.0,
                normal: unit,
                offset: distance.get(),
            })
    }

    /// A heightfield shape, carrying the grid's local bounding half-extents (the
    /// grid data itself lives on the collider). Rejects non-finite or non-positive
    /// extents on the footprint axes.
    pub(crate) fn heightfield_shape(half_extents: Vec3) -> PhysicsResult<Self> {
        let h = half_extents;
        let valid = h.x.is_finite() & h.y.is_finite() & h.z.is_finite() & (h.x > 0.0) & (h.z > 0.0);
        [
            Err(PhysicsError::invalid_collider_shape(
                "heightfield footprint half-extents must be finite and positive",
            )),
            Ok(PhysicsColliderShape {
                kind: PhysicsShapeKind::Heightfield,
                half_extents,
                radius: 0.0,
                normal: Vec3::ZERO,
                offset: 0.0,
            }),
        ][valid as usize]
    }

    /// The shape discriminant.
    pub(crate) fn kind(&self) -> PhysicsShapeKind {
        self.kind
    }

    /// The local axis-aligned half-size (meaningful for the finite kinds; `ZERO`
    /// for a plane).
    pub(crate) fn half_extents(&self) -> Vec3 {
        self.half_extents
    }

    /// The rounding radius (sphere/capsule; `0` otherwise).
    pub(crate) fn radius(&self) -> f32 {
        self.radius
    }

    /// The unit plane normal (`ZERO` for non-plane shapes).
    pub(crate) fn normal(&self) -> Vec3 {
        self.normal
    }

    /// The plane signed offset `n · x = offset` (`0` for non-plane shapes).
    pub(crate) fn offset(&self) -> f32 {
        self.offset
    }


    /// `true` iff this collider is a sphere.
    pub fn is_sphere(&self) -> bool {
        self.kind == PhysicsShapeKind::Sphere
    }

    /// `true` iff this collider is an axis-aligned box.
    pub fn is_box(&self) -> bool {
        self.kind == PhysicsShapeKind::Box
    }

    /// `true` iff this collider is a capsule.
    pub fn is_capsule(&self) -> bool {
        self.kind == PhysicsShapeKind::Capsule
    }

    /// `true` iff this collider is an infinite half-space plane.
    pub fn is_plane(&self) -> bool {
        self.kind == PhysicsShapeKind::Plane
    }

    /// `true` iff this collider is a static heightfield surface.
    pub fn is_heightfield(&self) -> bool {
        self.kind == PhysicsShapeKind::Heightfield
    }

    /// The sphere radius, or `None` if this is not a sphere.
    pub fn sphere_radius(&self) -> Option<Meters> {
        self.is_sphere()
            .then(|| Meters::new(self.radius).ok())
            .flatten()
    }

    /// The box half-extents, or `None` if this is not a box.
    pub fn box_half_extents(&self) -> Option<Vec3> {
        self.is_box().then_some(self.half_extents)
    }

    /// The capsule radius, or `None` if this is not a capsule.
    pub fn capsule_radius(&self) -> Option<Meters> {
        self.is_capsule()
            .then(|| Meters::new(self.radius).ok())
            .flatten()
    }

    /// The capsule half-height (the cylinder half-length, excluding the caps), or
    /// `None` if this is not a capsule. Recovered from the packed local extent
    /// `half_extents.y = half_height + radius`.
    pub fn capsule_half_height(&self) -> Option<Meters> {
        self.is_capsule()
            .then(|| Meters::new(self.half_extents.y - self.radius).ok())
            .flatten()
    }

    /// The unit plane normal, or `None` if this is not a plane.
    pub fn plane_normal(&self) -> Option<Vec3> {
        self.is_plane().then_some(self.normal)
    }

    /// The plane signed distance (`n · x = distance`), or `None` if this is not a
    /// plane.
    pub fn plane_distance(&self) -> Option<Meters> {
        self.is_plane()
            .then(|| Meters::new(self.offset).ok())
            .flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(v: f32) -> Meters {
        Meters::new(v).unwrap()
    }

    #[test]
    fn sphere_packs_radius_into_every_field_it_needs() {
        let s = PhysicsColliderShape::sphere(m(2.0)).unwrap();
        assert_eq!(s.kind(), PhysicsShapeKind::Sphere);
        assert_eq!(s.radius(), 2.0);
        assert_eq!(s.half_extents(), Vec3::new(2.0, 2.0, 2.0));
        let e = PhysicsColliderShape::sphere(m(0.0)).unwrap_err();
        assert_eq!(e.code(), PhysicsErrorCode::InvalidColliderShape);
        assert!(PhysicsColliderShape::sphere(m(-1.0)).is_err());
    }

    #[test]
    fn box_validates_every_extent_and_stores_half_extents() {
        let b = PhysicsColliderShape::box_shape(Vec3::new(1.0, 2.0, 3.0)).unwrap();
        assert_eq!(b.kind(), PhysicsShapeKind::Box);
        assert_eq!(b.half_extents(), Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(b.radius(), 0.0);
        assert!(PhysicsColliderShape::box_shape(Vec3::new(0.0, 1.0, 1.0)).is_err());
        assert!(PhysicsColliderShape::box_shape(Vec3::new(1.0, -1.0, 1.0)).is_err());
        assert!(PhysicsColliderShape::box_shape(Vec3::new(1.0, 1.0, f32::NAN)).is_err());
    }

    #[test]
    fn capsule_validates_and_bounds_include_the_caps() {
        let c = PhysicsColliderShape::capsule(m(1.0), m(2.0)).unwrap();
        assert_eq!(c.kind(), PhysicsShapeKind::Capsule);
        assert_eq!(c.radius(), 1.0);
        // local AABB half-size = (r, hh + r, r) = (1, 3, 1)
        assert_eq!(c.half_extents(), Vec3::new(1.0, 3.0, 1.0));
        assert!(PhysicsColliderShape::capsule(m(1.0), m(0.0)).is_ok());
        assert!(PhysicsColliderShape::capsule(m(0.0), m(1.0)).is_err());
        assert!(PhysicsColliderShape::capsule(m(1.0), m(-1.0)).is_err());
    }

    #[test]
    fn plane_stores_a_unit_normal_and_offset() {
        let p = PhysicsColliderShape::plane(Vec3::new(0.0, 2.0, 0.0), m(5.0)).unwrap();
        assert_eq!(p.kind(), PhysicsShapeKind::Plane);
        assert_eq!(p.normal(), Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(p.offset(), 5.0);
        assert!(PhysicsColliderShape::plane(Vec3::UNIT_Y, m(0.0)).is_ok());
        let e = PhysicsColliderShape::plane(Vec3::ZERO, m(0.0)).unwrap_err();
        assert_eq!(e.code(), PhysicsErrorCode::InvalidColliderShape);
        assert!(e.math().is_some(), "plane error wraps the math normalize cause");
        assert!(PhysicsColliderShape::plane(Vec3::new(f32::NAN, 1.0, 0.0), m(0.0)).is_err());
    }

    #[test]
    fn derives_are_exercised() {
        let s = PhysicsColliderShape::sphere(m(1.0)).unwrap();
        let c = s;
        assert_eq!(s, c);
        assert_ne!(s, PhysicsColliderShape::box_shape(Vec3::ONE).unwrap());
        assert!(format!("{s:?}").contains("Sphere"));
    }

    #[test]
    fn public_geometry_accessors_expose_each_shape_and_reject_others() {
        let sphere = PhysicsColliderShape::sphere(m(2.0)).unwrap();
        let boxes = PhysicsColliderShape::box_shape(Vec3::new(1.0, 2.0, 3.0)).unwrap();
        let capsule = PhysicsColliderShape::capsule(m(1.0), m(2.0)).unwrap();
        let plane = PhysicsColliderShape::plane(Vec3::UNIT_Y, m(5.0)).unwrap();

        assert!(sphere.is_sphere() && !sphere.is_box() && !sphere.is_capsule() && !sphere.is_plane());
        assert!(boxes.is_box() && !boxes.is_sphere());
        assert!(capsule.is_capsule() && !capsule.is_plane());
        assert!(plane.is_plane() && !plane.is_capsule());

        assert_eq!(sphere.sphere_radius().unwrap().get(), 2.0);
        assert!(boxes.sphere_radius().is_none());

        assert_eq!(boxes.box_half_extents().unwrap(), Vec3::new(1.0, 2.0, 3.0));
        assert!(sphere.box_half_extents().is_none());

        assert_eq!(capsule.capsule_radius().unwrap().get(), 1.0);
        assert_eq!(capsule.capsule_half_height().unwrap().get(), 2.0);
        assert!(sphere.capsule_radius().is_none());
        assert!(sphere.capsule_half_height().is_none());

        assert_eq!(plane.plane_normal().unwrap(), Vec3::UNIT_Y);
        assert_eq!(plane.plane_distance().unwrap().get(), 5.0);
        assert!(sphere.plane_normal().is_none());
        assert!(sphere.plane_distance().is_none());
    }

    #[test]
    fn heightfield_shape_stores_its_bounds_and_rejects_a_degenerate_footprint() {
        let hf = PhysicsColliderShape::heightfield_shape(Vec3::new(4.0, 1.0, 6.0)).unwrap();
        assert!(hf.is_heightfield() && !hf.is_box() && !hf.is_plane());
        assert_eq!(hf.half_extents(), Vec3::new(4.0, 1.0, 6.0));
        assert_eq!(hf.kind(), PhysicsShapeKind::Heightfield);
        // A zero / non-finite footprint axis is rejected (a flat y is allowed).
        assert!(PhysicsColliderShape::heightfield_shape(Vec3::new(0.0, 1.0, 6.0)).is_err());
        assert!(PhysicsColliderShape::heightfield_shape(Vec3::new(4.0, 1.0, 0.0)).is_err());
        assert!(PhysicsColliderShape::heightfield_shape(Vec3::new(4.0, 0.0, 6.0)).is_ok());
        assert!(!PhysicsColliderShape::sphere(m(1.0)).unwrap().is_heightfield());
    }
}
