//! Exact per-shape overlap against a **capsule-shaped query volume**, and the
//! branchless table that dispatches it.
//!
//! There is one overlap relation here, not two. A query sphere is a capsule whose
//! axis has zero length, so `overlap_sphere` and `overlap_capsule` are the same
//! test asked with a different axis — collapsing them means one implementation
//! per collider kind, one set of edge cases, and no way for the sphere and
//! capsule answers to disagree.
//!
//! | kind        | test                                                          |
//! |-------------|---------------------------------------------------------------|
//! | Sphere      | [`axiom_math::Capsule::overlaps_sphere`]                       |
//! | Box         | the query capsule against the box's twelve surface triangles, plus containment |
//! | Capsule     | [`axiom_math::Capsule::overlaps`] — axis distance vs summed radii |
//! | Plane       | the deepest end of the query axis against the solid half-space |
//! | Heightfield | **unsupported** — never reported                               |
//!
//! ## A plane is solid, so being *behind* it counts
//! The half-space's stored normal points to the empty side, and the narrow phase
//! treats everything on the other side as solid material. Overlap therefore asks
//! whether the query volume reaches *into or past* the surface —
//! `signed_distance <= radius` — not whether it straddles it. A query buried well
//! inside the ground overlaps the ground, which the earlier absolute-distance
//! form denied while the contact generator was busy pushing bodies out of it.
//!
//! ## Heightfield is explicitly unsupported, not approximated
//! The grid lives on the collider, not on the shape, so it is out of reach of
//! this flat per-shape signature — the same boundary the narrow phase draws (see
//! [`crate::query_ray`]). Falling back to its bounding box would report overlaps
//! in the open air above a valley, so a heightfield is excluded outright.

use axiom_math::{Capsule, Quat, Sphere, Vec3};

use crate::collider_capsule::world_capsule;
use crate::collider_obb::{obb_triangles, world_obb};
use crate::physics_collider_shape::PhysicsColliderShape;
use crate::physics_shape_kind::PhysicsShapeKind;

/// The exact overlap function for one shape kind, against a query capsule.
type OverlapFn = fn(PhysicsColliderShape, Vec3, Quat, &Capsule) -> bool;

/// Exact per-kind overlap functions, indexed by `kind().index()`. Sized by
/// [`PhysicsShapeKind::COUNT`] so it cannot fall behind the enum.
const OVERLAP_TABLE: [OverlapFn; PhysicsShapeKind::COUNT] = [
    overlap_sphere_shape,
    overlap_box_shape,
    overlap_capsule_shape,
    overlap_plane_shape,
    overlap_heightfield_shape,
    overlap_triangle_soup_shape,
];

/// Whether a collider exactly overlaps `query`, dispatched branchlessly on the
/// shape kind.
pub(crate) fn overlaps_capsule(
    shape: PhysicsColliderShape,
    center: Vec3,
    rotation: Quat,
    query: &Capsule,
) -> bool {
    OVERLAP_TABLE[shape.kind().index()](shape, center, rotation, query)
}

/// Exact sphere overlap: the query axis within the summed radii of the sphere's
/// centre (inclusive of touching).
fn overlap_sphere_shape(
    shape: PhysicsColliderShape,
    center: Vec3,
    _rotation: Quat,
    query: &Capsule,
) -> bool {
    Sphere::new(center, shape.radius()).map_or(false, |sphere| query.overlaps_sphere(&sphere))
}

/// Exact oriented-box overlap. Two terms, because the surface solve alone cannot
/// see a query volume swallowed whole: the query against each of the box's
/// twelve surface triangles catches every crossing and grazing contact, and the
/// containment of the query's own endpoints catches a capsule entirely inside the
/// box (which touches no triangle at all).
fn overlap_box_shape(
    shape: PhysicsColliderShape,
    center: Vec3,
    rotation: Quat,
    query: &Capsule,
) -> bool {
    world_obb(shape, center, rotation).map_or(false, |obb| {
        let axis = query.segment();
        obb.contains_point(axis.start())
            | obb.contains_point(axis.end())
            | obb_triangles(&obb)
                .iter()
                .any(|triangle| query.overlaps_triangle(triangle))
    })
}

/// Exact capsule overlap: the distance between the two axes against the sum of
/// the two radii, on the collider's true rotated axis.
fn overlap_capsule_shape(
    shape: PhysicsColliderShape,
    center: Vec3,
    rotation: Quat,
    query: &Capsule,
) -> bool {
    world_capsule(shape, center, rotation).map_or(false, |capsule| query.overlaps(&capsule))
}

/// Exact half-space overlap: the deeper end of the query axis is within one
/// query radius of the surface, or already past it into the solid.
fn overlap_plane_shape(
    shape: PhysicsColliderShape,
    _center: Vec3,
    _rotation: Quat,
    query: &Capsule,
) -> bool {
    let normal = shape.normal();
    let axis = query.segment();
    let deepest = (normal.dot(axis.start()) - shape.offset())
        .min(normal.dot(axis.end()) - shape.offset());
    deepest <= query.radius()
}

/// A heightfield is explicitly unsupported by overlap — never reported.
/// As [`overlap_heightfield_shape`]: the soup is not reachable through the flat
/// signature, so the entry keeps the table exhaustive and states the gap.
fn overlap_triangle_soup_shape(
    _shape: PhysicsColliderShape,
    _center: Vec3,
    _rotation: Quat,
    _query: &Capsule,
) -> bool {
    false
}

fn overlap_heightfield_shape(
    _shape: PhysicsColliderShape,
    _center: Vec3,
    _rotation: Quat,
    _query: &Capsule,
) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::Meters;
    use axiom_math::Segment;
    use core::f32::consts::FRAC_PI_2;

    fn sphere(r: f32) -> PhysicsColliderShape {
        PhysicsColliderShape::sphere(Meters::new(r).unwrap()).unwrap()
    }

    fn box_shape(x: f32, y: f32, z: f32) -> PhysicsColliderShape {
        PhysicsColliderShape::box_shape(Vec3::new(x, y, z)).unwrap()
    }

    fn capsule_shape(r: f32, hh: f32) -> PhysicsColliderShape {
        PhysicsColliderShape::capsule(Meters::new(r).unwrap(), Meters::new(hh).unwrap()).unwrap()
    }

    fn plane(normal: Vec3, distance: f32) -> PhysicsColliderShape {
        PhysicsColliderShape::plane(normal, Meters::new(distance).unwrap()).unwrap()
    }

    fn heightfield() -> PhysicsColliderShape {
        PhysicsColliderShape::heightfield_shape(Vec3::new(4.0, 1.0, 4.0)).unwrap()
    }

    /// A query volume that is a plain sphere — the degenerate capsule.
    fn ball(center: Vec3, radius: f32) -> Capsule {
        Capsule::new(Segment::new(center, center).unwrap(), radius).unwrap()
    }

    /// A query volume with a real axis.
    fn shaft(start: Vec3, end: Vec3, radius: f32) -> Capsule {
        Capsule::new(Segment::new(start, end).unwrap(), radius).unwrap()
    }

    fn id() -> Quat {
        Quat::IDENTITY
    }

    #[test]
    fn a_sphere_collider_overlaps_by_summed_radii() {
        let unit = sphere(1.0);
        // Centres 1.5 apart, radii sum 2.0 -> overlap.
        assert!(overlaps_capsule(
            unit,
            Vec3::new(1.5, 0.0, 0.0),
            id(),
            &ball(Vec3::ZERO, 1.0)
        ));
        // Centres 2.5 apart -> no overlap. A bounding sphere inflated by sqrt(3)
        // would have falsely reported this one.
        assert!(!overlaps_capsule(
            unit,
            Vec3::new(2.5, 0.0, 0.0),
            id(),
            &ball(Vec3::ZERO, 1.0)
        ));
        // A query with a real axis reaches further along it.
        assert!(overlaps_capsule(
            unit,
            Vec3::new(4.0, 0.0, 0.0),
            id(),
            &shaft(Vec3::ZERO, Vec3::new(3.0, 0.0, 0.0), 1.0)
        ));
    }

    #[test]
    fn a_box_collider_overlaps_by_closest_point_not_bounding_sphere() {
        let cube = box_shape(1.0, 1.0, 1.0);
        // sqrt(3 * 1.2^2) ~ 2.08 from the (1,1,1) corner: clear of a unit query.
        assert!(!overlaps_capsule(
            cube,
            Vec3::ZERO,
            id(),
            &ball(Vec3::new(2.2, 2.2, 2.2), 1.0)
        ));
        // sqrt(3 * 0.5^2) ~ 0.87 from the same corner: overlapping.
        assert!(overlaps_capsule(
            cube,
            Vec3::ZERO,
            id(),
            &ball(Vec3::new(1.5, 1.5, 1.5), 1.0)
        ));
    }

    #[test]
    fn a_query_capsule_swallowed_by_a_box_still_overlaps_it() {
        // Entirely inside a large box, touching none of its twelve triangles:
        // only the containment term can see this one.
        assert!(overlaps_capsule(
            box_shape(10.0, 10.0, 10.0),
            Vec3::ZERO,
            id(),
            &shaft(
                Vec3::new(0.0, -1.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                0.25
            )
        ));
    }

    #[test]
    fn a_query_capsule_laid_across_a_box_overlaps_it() {
        // Both endpoints outside the box, the shaft crossing straight through:
        // only the triangle term can see this one.
        assert!(overlaps_capsule(
            box_shape(1.0, 1.0, 1.0),
            Vec3::ZERO,
            id(),
            &shaft(
                Vec3::new(-9.0, 0.0, 0.0),
                Vec3::new(9.0, 0.0, 0.0),
                0.25
            )
        ));
    }

    #[test]
    fn a_turned_box_overlaps_on_its_real_extent() {
        // A slab yawed a quarter turn about Y reaches |z| = 4 and only |x| = 1.
        let yaw = Quat::from_axis_angle(Vec3::UNIT_Y, FRAC_PI_2).unwrap();
        let slab = box_shape(4.0, 1.0, 1.0);
        assert!(overlaps_capsule(
            slab,
            Vec3::ZERO,
            yaw,
            &ball(Vec3::new(0.0, 0.0, 3.5), 0.25)
        ));
        assert!(!overlaps_capsule(
            slab,
            Vec3::ZERO,
            yaw,
            &ball(Vec3::new(3.5, 0.0, 0.0), 0.25)
        ));
    }

    #[test]
    fn an_unliftable_box_never_overlaps() {
        assert!(!overlaps_capsule(
            box_shape(1.0, 1.0, 1.0),
            Vec3::new(f32::NAN, 0.0, 0.0),
            id(),
            &ball(Vec3::ZERO, 100.0)
        ));
    }

    #[test]
    fn a_capsule_collider_overlaps_by_axis_distance() {
        // Collider r = 1 upright about the origin; the axis spans y in [-1, 1].
        let post = capsule_shape(1.0, 1.0);
        assert!(overlaps_capsule(
            post,
            Vec3::ZERO,
            id(),
            &ball(Vec3::new(1.5, 0.0, 0.0), 1.0)
        ));
        assert!(!overlaps_capsule(
            post,
            Vec3::ZERO,
            id(),
            &ball(Vec3::new(2.5, 0.0, 0.0), 1.0)
        ));
        // The collider's rotation is genuinely used: tipped along X it now
        // reaches x = 3 and no longer reaches y = 3.
        let tipped = Quat::from_axis_angle(Vec3::UNIT_Z, FRAC_PI_2).unwrap();
        let long = capsule_shape(1.0, 3.0);
        assert!(overlaps_capsule(
            long,
            Vec3::ZERO,
            tipped,
            &ball(Vec3::new(3.5, 0.0, 0.0), 0.75)
        ));
        assert!(!overlaps_capsule(
            long,
            Vec3::ZERO,
            tipped,
            &ball(Vec3::new(0.0, 3.5, 0.0), 0.75)
        ));
    }

    #[test]
    fn an_unliftable_capsule_never_overlaps() {
        assert!(!overlaps_capsule(
            capsule_shape(1.0, 1.0),
            Vec3::new(0.0, f32::INFINITY, 0.0),
            id(),
            &ball(Vec3::ZERO, 100.0)
        ));
    }

    #[test]
    fn a_plane_is_solid_so_behind_it_overlaps_and_above_it_does_not() {
        let ground = plane(Vec3::UNIT_Y, 0.0);
        // Well above the surface: no overlap.
        assert!(!overlaps_capsule(
            ground,
            Vec3::ZERO,
            id(),
            &ball(Vec3::new(0.0, 5.0, 0.0), 1.0)
        ));
        // Reaching down to it: overlap.
        assert!(overlaps_capsule(
            ground,
            Vec3::ZERO,
            id(),
            &ball(Vec3::new(0.0, 0.5, 0.0), 1.0)
        ));
        // Buried inside the solid: overlap. An absolute-distance test would have
        // denied this while the narrow phase was pushing bodies out of it.
        assert!(overlaps_capsule(
            ground,
            Vec3::ZERO,
            id(),
            &ball(Vec3::new(0.0, -5.0, 0.0), 1.0)
        ));
    }

    #[test]
    fn a_query_capsule_dips_into_a_plane_with_its_lower_end() {
        // The upper end is far clear of the surface; the lower one is not.
        assert!(overlaps_capsule(
            plane(Vec3::UNIT_Y, 0.0),
            Vec3::ZERO,
            id(),
            &shaft(
                Vec3::new(0.0, 0.2, 0.0),
                Vec3::new(0.0, 9.0, 0.0),
                0.25
            )
        ));
        assert!(!overlaps_capsule(
            plane(Vec3::UNIT_Y, 0.0),
            Vec3::ZERO,
            id(),
            &shaft(
                Vec3::new(0.0, 2.0, 0.0),
                Vec3::new(0.0, 9.0, 0.0),
                0.25
            )
        ));
    }

    #[test]
    fn a_heightfield_is_never_reported() {
        assert!(!overlaps_capsule(
            heightfield(),
            Vec3::ZERO,
            id(),
            &ball(Vec3::ZERO, 100.0)
        ));
    }

    #[test]
    fn every_shape_kind_has_a_table_entry() {
        assert_eq!(OVERLAP_TABLE.len(), PhysicsShapeKind::COUNT);
        let shapes = [
            sphere(1.0),
            box_shape(1.0, 1.0, 1.0),
            capsule_shape(1.0, 1.0),
            plane(Vec3::UNIT_Y, 0.0),
            heightfield(),
        ];
        let reported = shapes
            .into_iter()
            .filter(|s| overlaps_capsule(*s, Vec3::ZERO, id(), &ball(Vec3::ZERO, 0.5)))
            .count();
        assert_eq!(reported, 4, "only the heightfield is excluded");
    }
}
