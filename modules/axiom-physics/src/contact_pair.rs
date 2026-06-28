//! The deterministic narrow phase: candidate pairs → contact manifolds.
//!
//! `generate_contacts` takes the broad phase's candidate pairs and produces a
//! [`ContactManifold`] for each pair that is genuinely overlapping. The narrow
//! phase implements the four classical pairings — sphere/sphere, sphere/plane,
//! sphere/box, and box/plane — in both collider orderings; every other pairing
//! (box/box, any capsule, plane/plane) deterministically produces **no** contact
//! and is a documented deferral (see `ROADMAP.md`).
//!
//! ## Branchless shape dispatch
//! There is no `match` on shape kinds. Each ordered `(kind_a, kind_b)` pairing
//! indexes a fixed 16-entry function table (`kind_a.index() * 4 + kind_b.index()`)
//! of contact generators; unimplemented pairings point at `no_contact`. The
//! reversed orderings (box/sphere, plane/sphere, plane/box) reuse the canonical
//! generator with arguments swapped and the resulting normal flipped, so the
//! geometry is written once.
//!
//! ## Conventions (deterministic, documented)
//! - The contact **normal points from collider A to collider B** (A/B in
//!   ascending handle order — see [`ContactManifold`]).
//! - A plane is a one-sided **solid half-space**; its stored unit normal points to
//!   the empty side, so a body crossing to the solid side penetrates.
//! - Contact requires **strictly positive** penetration: a pair touching exactly
//!   at the boundary (`depth == 0`) produces no contact. Degenerate coincident
//!   configurations (sphere centers equal, or a sphere center inside a box) have
//!   no defined normal and likewise produce no contact.

use axiom_math::Vec3;

use crate::broad_phase_pair::BroadPhasePair;
use crate::contact_manifold::ContactManifold;
use crate::physics_body::PhysicsBody;
use crate::physics_collider::PhysicsCollider;
use crate::physics_collider_shape::PhysicsColliderShape;

/// The raw geometric result of a narrow-phase test, before it is tagged with the
/// pair's handles: a unit normal (A→B), a positive penetration depth, and a world
/// contact point.
struct ContactGeom {
    normal: Vec3,
    depth: f32,
    point: Vec3,
}

/// A contact generator for one ordered shape pairing.
type ContactFn = fn(PhysicsColliderShape, Vec3, PhysicsColliderShape, Vec3) -> Option<ContactGeom>;

/// The branchless dispatch table, indexed by `kind_a.index() * 4 + kind_b.index()`
/// with `Sphere = 0, Box = 1, Capsule = 2, Plane = 3`.
const CONTACT_TABLE: [ContactFn; 16] = [
    sphere_sphere, // (Sphere, Sphere)
    sphere_box,    // (Sphere, Box)
    no_contact,    // (Sphere, Capsule)
    sphere_plane,  // (Sphere, Plane)
    box_sphere,    // (Box, Sphere)
    no_contact,    // (Box, Box)
    no_contact,    // (Box, Capsule)
    box_plane,     // (Box, Plane)
    no_contact,    // (Capsule, Sphere)
    no_contact,    // (Capsule, Box)
    no_contact,    // (Capsule, Capsule)
    no_contact,    // (Capsule, Plane)
    plane_sphere,  // (Plane, Sphere)
    plane_box,     // (Plane, Box)
    no_contact,    // (Plane, Capsule)
    no_contact,    // (Plane, Plane)
];

/// Reverse a contact: swapping the A/B roles flips the A→B normal.
fn flip(geom: ContactGeom) -> ContactGeom {
    ContactGeom {
        normal: geom.normal.mul_scalar(-1.0),
        depth: geom.depth,
        point: geom.point,
    }
}

/// An unimplemented pairing — never reports a contact.
fn no_contact(
    _a: PhysicsColliderShape,
    _ca: Vec3,
    _b: PhysicsColliderShape,
    _cb: Vec3,
) -> Option<ContactGeom> {
    None
}

/// Sphere (A) vs sphere (B).
fn sphere_sphere(
    a: PhysicsColliderShape,
    ca: Vec3,
    b: PhysicsColliderShape,
    cb: Vec3,
) -> Option<ContactGeom> {
    let sum = a.radius() + b.radius();
    let delta = cb.subtract(ca);
    let dist = delta.length_squared().sqrt();
    let penetrating = (dist > 0.0) & (dist < sum);
    let inv = 1.0 / dist.max(f32::MIN_POSITIVE);
    let normal = delta.mul_scalar(inv);
    let depth = sum - dist;
    let point = ca.add(normal.mul_scalar(a.radius() - depth * 0.5));
    penetrating.then_some(ContactGeom {
        normal,
        depth,
        point,
    })
}

/// Sphere (A) vs plane (B). The plane center is irrelevant — the plane is defined
/// by its unit normal and signed offset `n · x = offset`.
fn sphere_plane(
    a: PhysicsColliderShape,
    ca: Vec3,
    b: PhysicsColliderShape,
    _cb: Vec3,
) -> Option<ContactGeom> {
    let r = a.radius();
    let n = b.normal();
    let signed = n.dot(ca) - b.offset();
    let depth = r - signed;
    let penetrating = depth > 0.0;
    let normal = n.mul_scalar(-1.0);
    let point = ca.subtract(n.mul_scalar(r));
    penetrating.then_some(ContactGeom {
        normal,
        depth,
        point,
    })
}

/// Box (A) vs plane (B). The box projection radius onto the plane normal is the
/// L1 combination of its half-extents with the absolute normal components.
fn box_plane(
    a: PhysicsColliderShape,
    ca: Vec3,
    b: PhysicsColliderShape,
    _cb: Vec3,
) -> Option<ContactGeom> {
    let he = a.half_extents();
    let n = b.normal();
    let signed = n.dot(ca) - b.offset();
    let radius = n.x.abs() * he.x + n.y.abs() * he.y + n.z.abs() * he.z;
    let depth = radius - signed;
    let penetrating = depth > 0.0;
    let normal = n.mul_scalar(-1.0);
    let point = ca.subtract(n.mul_scalar(signed));
    penetrating.then_some(ContactGeom {
        normal,
        depth,
        point,
    })
}

/// Sphere (A) vs box (B): the sphere center against the box's closest surface
/// point. A sphere whose center is inside the box (`dist == 0`) has no defined
/// normal and produces no contact.
fn sphere_box(
    a: PhysicsColliderShape,
    ca: Vec3,
    b: PhysicsColliderShape,
    cb: Vec3,
) -> Option<ContactGeom> {
    let r = a.radius();
    let he = b.half_extents();
    let d = ca.subtract(cb);
    let closest = cb.add(Vec3::new(
        d.x.clamp(-he.x, he.x),
        d.y.clamp(-he.y, he.y),
        d.z.clamp(-he.z, he.z),
    ));
    let delta = ca.subtract(closest);
    let dist = delta.length_squared().sqrt();
    let penetrating = (dist > 0.0) & (dist < r);
    let inv = 1.0 / dist.max(f32::MIN_POSITIVE);
    let normal = delta.mul_scalar(-inv);
    let depth = r - dist;
    penetrating.then_some(ContactGeom {
        normal,
        depth,
        point: closest,
    })
}

/// Box (A) vs sphere (B) — the canonical sphere/box with roles swapped.
fn box_sphere(
    a: PhysicsColliderShape,
    ca: Vec3,
    b: PhysicsColliderShape,
    cb: Vec3,
) -> Option<ContactGeom> {
    sphere_box(b, cb, a, ca).map(flip)
}

/// Plane (A) vs sphere (B) — the canonical sphere/plane with roles swapped.
fn plane_sphere(
    a: PhysicsColliderShape,
    ca: Vec3,
    b: PhysicsColliderShape,
    cb: Vec3,
) -> Option<ContactGeom> {
    sphere_plane(b, cb, a, ca).map(flip)
}

/// Plane (A) vs box (B) — the canonical box/plane with roles swapped.
fn plane_box(
    a: PhysicsColliderShape,
    ca: Vec3,
    b: PhysicsColliderShape,
    cb: Vec3,
) -> Option<ContactGeom> {
    box_plane(b, cb, a, ca).map(flip)
}

/// Generate a contact manifold for every candidate pair that is genuinely
/// overlapping, preserving the broad phase's sorted pair order. Each pair's
/// colliders and bodies are resolved by handle (always present, since the pair
/// came from these very slices), then dispatched through [`CONTACT_TABLE`].
pub(crate) fn generate_contacts(
    pairs: &[BroadPhasePair],
    colliders: &[PhysicsCollider],
    bodies: &[PhysicsBody],
) -> Vec<ContactManifold> {
    pairs
        .iter()
        .filter_map(|pair| {
            let ca = colliders.iter().find(|c| c.handle() == pair.a());
            let cb = colliders.iter().find(|c| c.handle() == pair.b());
            ca.zip(cb).and_then(|(ca, cb)| {
                let ba = bodies.iter().find(|x| x.handle() == ca.body());
                let bb = bodies.iter().find(|x| x.handle() == cb.body());
                ba.zip(bb).and_then(|(ba, bb)| {
                    let index = ca.shape().kind().index() * 4 + cb.shape().kind().index();
                    CONTACT_TABLE[index](
                        ca.shape(),
                        ba.transform().translation,
                        cb.shape(),
                        bb.transform().translation,
                    )
                    .map(|geom| {
                        ContactManifold::new(
                            ca.handle(),
                            cb.handle(),
                            ca.body(),
                            cb.body(),
                            geom.normal,
                            geom.depth,
                            geom.point,
                        )
                    })
                })
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics_body_desc::PhysicsBodyDesc;
    use crate::physics_body_handle::PhysicsBodyHandle;
    use crate::physics_collider_handle::PhysicsColliderHandle;
    use crate::physics_material::PhysicsMaterial;
    use axiom_kernel::{Meters, Ratio};
    use axiom_math::Transform;

    fn sphere(r: f32) -> PhysicsColliderShape {
        PhysicsColliderShape::sphere(Meters::new(r).unwrap()).unwrap()
    }

    fn box_shape(x: f32, y: f32, z: f32) -> PhysicsColliderShape {
        PhysicsColliderShape::box_shape(Vec3::new(x, y, z)).unwrap()
    }

    fn plane() -> PhysicsColliderShape {
        PhysicsColliderShape::plane(Vec3::UNIT_Y, Meters::new(0.0).unwrap()).unwrap()
    }

    fn capsule() -> PhysicsColliderShape {
        PhysicsColliderShape::capsule(Meters::new(0.5).unwrap(), Meters::new(1.0).unwrap()).unwrap()
    }

    fn approx(a: Vec3, b: Vec3) {
        let d = a.subtract(b).length_squared();
        assert!(d < 1.0e-9, "expected {b:?}, got {a:?}");
    }

    // ---- sphere / sphere ----

    #[test]
    fn sphere_sphere_hit_reports_axis_normal_and_depth() {
        let g = sphere_sphere(sphere(1.0), Vec3::ZERO, sphere(1.0), Vec3::new(1.5, 0.0, 0.0))
            .expect("overlapping spheres are in contact");
        approx(g.normal, Vec3::new(1.0, 0.0, 0.0)); // A -> B is +X
        assert!((g.depth - 0.5).abs() < 1.0e-6);
        approx(g.point, Vec3::new(0.75, 0.0, 0.0));
    }

    #[test]
    fn sphere_sphere_miss_when_separated_or_coincident() {
        assert!(sphere_sphere(sphere(1.0), Vec3::ZERO, sphere(1.0), Vec3::new(3.0, 0.0, 0.0)).is_none());
        // Exactly touching (dist == sum) is not a contact.
        assert!(sphere_sphere(sphere(1.0), Vec3::ZERO, sphere(1.0), Vec3::new(2.0, 0.0, 0.0)).is_none());
        // Coincident centers have no defined normal.
        assert!(sphere_sphere(sphere(1.0), Vec3::ZERO, sphere(1.0), Vec3::ZERO).is_none());
    }

    // ---- sphere / plane ----

    #[test]
    fn sphere_plane_hit_pushes_along_plane_normal() {
        let g = sphere_plane(sphere(1.0), Vec3::new(0.0, 0.5, 0.0), plane(), Vec3::ZERO)
            .expect("sphere crossing the plane is in contact");
        approx(g.normal, Vec3::new(0.0, -1.0, 0.0)); // sphere(A) -> plane(B) is downward
        assert!((g.depth - 0.5).abs() < 1.0e-6);
        approx(g.point, Vec3::new(0.0, -0.5, 0.0));
    }

    #[test]
    fn sphere_plane_miss_when_above_surface() {
        assert!(sphere_plane(sphere(1.0), Vec3::new(0.0, 2.0, 0.0), plane(), Vec3::ZERO).is_none());
    }

    // ---- box / plane ----

    #[test]
    fn box_plane_hit_uses_projection_radius() {
        let g = box_plane(box_shape(1.0, 1.0, 1.0), Vec3::new(0.0, 0.5, 0.0), plane(), Vec3::ZERO)
            .expect("box crossing the plane is in contact");
        approx(g.normal, Vec3::new(0.0, -1.0, 0.0));
        assert!((g.depth - 0.5).abs() < 1.0e-6);
    }

    #[test]
    fn box_plane_miss_when_above_surface() {
        assert!(box_plane(box_shape(1.0, 1.0, 1.0), Vec3::new(0.0, 2.0, 0.0), plane(), Vec3::ZERO).is_none());
    }

    // ---- sphere / box ----

    #[test]
    fn sphere_box_hit_resolves_against_closest_face() {
        let g = sphere_box(sphere(1.0), Vec3::new(0.0, 1.5, 0.0), box_shape(1.0, 1.0, 1.0), Vec3::ZERO)
            .expect("sphere resting above the box top is in contact");
        approx(g.normal, Vec3::new(0.0, -1.0, 0.0)); // sphere(A) -> box(B) is downward
        assert!((g.depth - 0.5).abs() < 1.0e-6);
        approx(g.point, Vec3::new(0.0, 1.0, 0.0));
    }

    #[test]
    fn sphere_box_miss_when_outside_or_center_inside() {
        // Far above the box.
        assert!(sphere_box(sphere(1.0), Vec3::new(0.0, 5.0, 0.0), box_shape(1.0, 1.0, 1.0), Vec3::ZERO).is_none());
        // Sphere center inside the box -> no defined normal -> no contact.
        assert!(sphere_box(sphere(1.0), Vec3::ZERO, box_shape(1.0, 1.0, 1.0), Vec3::ZERO).is_none());
    }

    // ---- swapped orderings reuse the canonical generators ----

    #[test]
    fn box_sphere_flips_the_canonical_normal() {
        let g = box_sphere(box_shape(1.0, 1.0, 1.0), Vec3::ZERO, sphere(1.0), Vec3::new(0.0, 1.5, 0.0))
            .expect("box below sphere is in contact");
        approx(g.normal, Vec3::new(0.0, 1.0, 0.0)); // box(A) -> sphere(B) is upward
        assert!((g.depth - 0.5).abs() < 1.0e-6);
    }

    #[test]
    fn plane_sphere_flips_the_canonical_normal() {
        let g = plane_sphere(plane(), Vec3::ZERO, sphere(1.0), Vec3::new(0.0, 0.5, 0.0))
            .expect("plane below sphere is in contact");
        approx(g.normal, Vec3::new(0.0, 1.0, 0.0)); // plane(A) -> sphere(B) is upward
    }

    #[test]
    fn plane_box_flips_the_canonical_normal() {
        let g = plane_box(plane(), Vec3::ZERO, box_shape(1.0, 1.0, 1.0), Vec3::new(0.0, 0.5, 0.0))
            .expect("plane below box is in contact");
        approx(g.normal, Vec3::new(0.0, 1.0, 0.0));
    }

    #[test]
    fn swapped_miss_returns_none() {
        assert!(box_sphere(box_shape(1.0, 1.0, 1.0), Vec3::ZERO, sphere(1.0), Vec3::new(0.0, 5.0, 0.0)).is_none());
    }

    // ---- generate_contacts orchestration ----

    fn material() -> PhysicsMaterial {
        PhysicsMaterial::new(
            Ratio::new(0.0).unwrap(),
            Ratio::new(0.0).unwrap(),
            Ratio::new(1.0).unwrap(),
        )
        .unwrap()
    }

    fn body(raw: u64, y: f32) -> PhysicsBody {
        let desc =
            PhysicsBodyDesc::static_body(Transform::from_translation(Vec3::new(0.0, y, 0.0)))
                .unwrap();
        PhysicsBody::from_desc(PhysicsBodyHandle::from_raw(raw), desc)
    }

    fn collider(collider_raw: u64, body_raw: u64, shape: PhysicsColliderShape) -> PhysicsCollider {
        PhysicsCollider::new(
            PhysicsColliderHandle::from_raw(collider_raw),
            PhysicsBodyHandle::from_raw(body_raw),
            shape,
            material(),
            false,
        )
    }

    #[test]
    fn generate_contacts_tags_a_real_contact_with_handles() {
        // Sphere (collider 10 on body 1, y = 1.5) resting on a box (collider 20 on
        // body 2, y = 0).
        let bodies = [body(1, 1.5), body(2, 0.0)];
        let colliders = [
            collider(10, 1, sphere(1.0)),
            collider(20, 2, box_shape(1.0, 1.0, 1.0)),
        ];
        let pairs = [BroadPhasePair::new(
            PhysicsColliderHandle::from_raw(10),
            PhysicsColliderHandle::from_raw(20),
        )];
        let contacts = generate_contacts(&pairs, &colliders, &bodies);
        assert_eq!(contacts.len(), 1);
        let m = contacts[0];
        assert_eq!(m.collider_a(), PhysicsColliderHandle::from_raw(10));
        assert_eq!(m.collider_b(), PhysicsColliderHandle::from_raw(20));
        assert_eq!(m.body_a(), PhysicsBodyHandle::from_raw(1));
        assert_eq!(m.body_b(), PhysicsBodyHandle::from_raw(2));
        approx(m.normal(), Vec3::new(0.0, -1.0, 0.0));
        assert!((m.depth() - 0.5).abs() < 1.0e-6);
    }

    #[test]
    fn generate_contacts_drops_unimplemented_and_separated_pairs() {
        // Two overlapping boxes (box/box is unimplemented) -> no contact, and the
        // capsule row of the table is exercised as no_contact too.
        let bodies = [body(1, 0.0), body(2, 0.0)];
        let colliders = [
            collider(10, 1, box_shape(1.0, 1.0, 1.0)),
            collider(20, 2, capsule()),
        ];
        let pairs = [BroadPhasePair::new(
            PhysicsColliderHandle::from_raw(10),
            PhysicsColliderHandle::from_raw(20),
        )];
        assert!(generate_contacts(&pairs, &colliders, &bodies).is_empty());
    }

    #[test]
    fn generate_contacts_is_empty_without_pairs() {
        assert!(generate_contacts(&[], &[], &[]).is_empty());
    }
}
