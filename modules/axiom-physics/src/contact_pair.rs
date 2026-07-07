//! The deterministic narrow phase: candidate pairs → contact manifolds.
//! `generate_contacts` takes the broad phase's candidate pairs and produces a
//! [`ContactManifold`] for each pair that is genuinely overlapping. The narrow
//! phase implements the four classical pairings — sphere/sphere, sphere/plane,
//! sphere/box, and box/plane — in both collider orderings; every other pairing
//! (box/box, any capsule, plane/plane) deterministically produces **no** contact
//! and is a documented deferral (see `ROADMAP.md`).
//! ## Branchless shape dispatch
//! There is no `match` on shape kinds. Each ordered `(kind_a, kind_b)` pairing
//! indexes a fixed 16-entry function table (`kind_a.index() * 4 + kind_b.index()`)
//! of contact generators; unimplemented pairings point at `no_contact`. The
//! reversed orderings (box/sphere, plane/sphere, plane/box) reuse the canonical
//! generator with arguments swapped and the resulting normal flipped, so the
//! geometry is written once.
//! ## Conventions (deterministic, documented)
//! - The contact **normal points from collider A to collider B** (A/B in
//!   ascending handle order — see [`ContactManifold`]).
//! - A plane is a one-sided **solid half-space**; its stored unit normal points to
//!   the empty side, so a body crossing to the solid side penetrates.
//! - Contact requires **strictly positive** penetration: a pair touching exactly
//!   at the boundary (`depth == 0`) produces no contact. Degenerate coincident
//!   configurations (sphere centers equal, or a sphere center inside a box) have
//!   no defined normal and likewise produce no contact.

use axiom_math::{Quat, Vec3};

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

/// A contact generator for one ordered shape pairing. Each collider is given its
/// owning body's world `center` **and** `rotation`, so an oriented box collides
/// on its true tilted faces. Sphere and plane generators ignore the rotation
/// arguments (a sphere is rotation-invariant; a plane's normal lives in its shape
/// and is treated as world-space, so a plane body's rotation does not steer it —
/// a documented choice, see the module docs).
type ContactFn =
    fn(PhysicsColliderShape, Vec3, Quat, PhysicsColliderShape, Vec3, Quat) -> Option<ContactGeom>;

/// The branchless dispatch table, indexed by `kind_a.index() * 5 + kind_b.index()`
/// with `Sphere = 0, Box = 1, Capsule = 2, Plane = 3, Heightfield = 4`. The
/// heightfield row/column are `no_contact` here: a heightfield's grid data is not
/// reachable through the flat `ContactFn` signature, so sphere↔heightfield contact
/// is generated alongside the table (which needs the whole collider) — see
/// [`heightfield_contact`].
const CONTACT_TABLE: [ContactFn; 25] = [
    sphere_sphere, // (Sphere, Sphere)
    sphere_box,    // (Sphere, Box)
    no_contact,    // (Sphere, Capsule)
    sphere_plane,  // (Sphere, Plane)
    no_contact,    // (Sphere, Heightfield) — see heightfield_contact
    box_sphere,    // (Box, Sphere)
    no_contact,    // (Box, Box)
    no_contact,    // (Box, Capsule)
    box_plane,     // (Box, Plane)
    no_contact,    // (Box, Heightfield)
    no_contact,    // (Capsule, Sphere)
    no_contact,    // (Capsule, Box)
    no_contact,    // (Capsule, Capsule)
    no_contact,    // (Capsule, Plane)
    no_contact,    // (Capsule, Heightfield)
    plane_sphere,  // (Plane, Sphere)
    plane_box,     // (Plane, Box)
    no_contact,    // (Plane, Capsule)
    no_contact,    // (Plane, Plane)
    no_contact,    // (Plane, Heightfield)
    no_contact,    // (Heightfield, Sphere) — see heightfield_contact
    no_contact,    // (Heightfield, Box)
    no_contact,    // (Heightfield, Capsule)
    no_contact,    // (Heightfield, Plane)
    no_contact,    // (Heightfield, Heightfield)
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
    _ra: Quat,
    _b: PhysicsColliderShape,
    _cb: Vec3,
    _rb: Quat,
) -> Option<ContactGeom> {
    None
}

/// Sphere (A) vs sphere (B). Rotation-invariant — the rotation args are unused.
fn sphere_sphere(
    a: PhysicsColliderShape,
    ca: Vec3,
    _ra: Quat,
    b: PhysicsColliderShape,
    cb: Vec3,
    _rb: Quat,
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

/// Sphere (A) vs plane (B). The plane center and rotation are irrelevant — the
/// plane is defined by its unit normal and signed offset `n · x = offset`.
fn sphere_plane(
    a: PhysicsColliderShape,
    ca: Vec3,
    _ra: Quat,
    b: PhysicsColliderShape,
    _cb: Vec3,
    _rb: Quat,
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
/// L1 combination of its half-extents with the plane normal expressed in the
/// box's **rotated** world axes, so a tilted box's half-width along the normal is
/// exact. At the identity box rotation `ra` this collapses to the axis-aligned
/// `|n.x|·hx + |n.y|·hy + |n.z|·hz`.
fn box_plane(
    a: PhysicsColliderShape,
    ca: Vec3,
    ra: Quat,
    b: PhysicsColliderShape,
    _cb: Vec3,
    _rb: Quat,
) -> Option<ContactGeom> {
    let he = a.half_extents();
    let n = b.normal();
    let signed = n.dot(ca) - b.offset();
    let radius = he.x * n.dot(ra.rotate(Vec3::UNIT_X)).abs()
        + he.y * n.dot(ra.rotate(Vec3::UNIT_Y)).abs()
        + he.z * n.dot(ra.rotate(Vec3::UNIT_Z)).abs();
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
/// point, computed in the **box's local frame** so an oriented (rotated) box
/// collides on its true tilted faces. The sphere center is brought into the box
/// frame with the box rotation's conjugate (its inverse for a unit body
/// quaternion), clamped to the half-extents, and the resulting local normal /
/// contact point are rotated back to world. A sphere whose center is inside the
/// box (`dist == 0`) has no defined normal and produces no contact. At the
/// identity box rotation `rb` this reduces exactly to the axis-aligned test.
fn sphere_box(
    a: PhysicsColliderShape,
    ca: Vec3,
    _ra: Quat,
    b: PhysicsColliderShape,
    cb: Vec3,
    rb: Quat,
) -> Option<ContactGeom> {
    let r = a.radius();
    let he = b.half_extents();
    let local = rb.conjugate().rotate(ca.subtract(cb));
    let closest_local = Vec3::new(
        local.x.clamp(-he.x, he.x),
        local.y.clamp(-he.y, he.y),
        local.z.clamp(-he.z, he.z),
    );
    let delta_local = local.subtract(closest_local);
    let dist = delta_local.length_squared().sqrt();
    let penetrating = (dist > 0.0) & (dist < r);
    let inv = 1.0 / dist.max(f32::MIN_POSITIVE);
    // A→B (sphere→box) points from the sphere centre toward the box surface, i.e.
    // opposite the local separation; rotate it (and the closest point) to world.
    let normal = rb.rotate(delta_local.mul_scalar(-inv));
    let depth = r - dist;
    penetrating.then_some(ContactGeom {
        normal,
        depth,
        point: cb.add(rb.rotate(closest_local)),
    })
}

/// Box (A) vs sphere (B) — the canonical sphere/box with roles (and rotations)
/// swapped.
fn box_sphere(
    a: PhysicsColliderShape,
    ca: Vec3,
    ra: Quat,
    b: PhysicsColliderShape,
    cb: Vec3,
    rb: Quat,
) -> Option<ContactGeom> {
    sphere_box(b, cb, rb, a, ca, ra).map(flip)
}

/// Plane (A) vs sphere (B) — the canonical sphere/plane with roles (and
/// rotations) swapped.
fn plane_sphere(
    a: PhysicsColliderShape,
    ca: Vec3,
    ra: Quat,
    b: PhysicsColliderShape,
    cb: Vec3,
    rb: Quat,
) -> Option<ContactGeom> {
    sphere_plane(b, cb, rb, a, ca, ra).map(flip)
}

/// Plane (A) vs box (B) — the canonical box/plane with roles (and rotations)
/// swapped.
fn plane_box(
    a: PhysicsColliderShape,
    ca: Vec3,
    ra: Quat,
    b: PhysicsColliderShape,
    cb: Vec3,
    rb: Quat,
) -> Option<ContactGeom> {
    box_plane(b, cb, rb, a, ca, ra).map(flip)
}

/// A sphere against a static heightfield, by the deterministic
/// **vertical-projection** contact: bring the sphere centre into the heightfield's
/// local frame, sample the surface height + central-difference normal directly
/// under it, and push the sphere out along that surface normal. Exact for the
/// gentle slopes a shallow track uses (steep, near-vertical walls are out of scope
/// and would need a closest-point-on-triangle test). Returns `None` unless the
/// first collider is a sphere and the second carries a heightfield they overlap.
/// The A→B normal points from the sphere into the surface.
fn sphere_vs_heightfield(
    sphere: PhysicsColliderShape,
    sphere_center: Vec3,
    heightfield: &PhysicsCollider,
    hf_pos: Vec3,
    hf_rot: Quat,
) -> Option<ContactGeom> {
    heightfield.heightfield().and_then(|grid| {
        let r = sphere.radius();
        let local = hf_rot.conjugate().rotate(sphere_center.subtract(hf_pos));
        let h = grid.sample(local.x, local.z);
        let n_local = grid.normal_at(local.x, local.z);
        // Perpendicular gap of the centre above the local tangent plane, and the
        // penetration of the sphere into it.
        let above = (local.y - h) * n_local.y;
        let depth = r - above;
        let hit = sphere.is_sphere() & grid.within(local.x, local.z) & (depth > 0.0) & (above > -r);
        let normal = hf_rot.rotate(n_local).mul_scalar(-1.0);
        let point = hf_pos.add(hf_rot.rotate(Vec3::new(local.x, h, local.z)));
        hit.then_some(ContactGeom { normal, depth, point })
    })
}

/// The sphere↔heightfield contact for a pair in **either** ordering (`None` for any
/// pair that is not a sphere against a heightfield). Complements [`CONTACT_TABLE`],
/// whose flat generators cannot reach a collider's grid data.
fn heightfield_contact(ca: &PhysicsCollider, ba_pos: Vec3, ba_rot: Quat, cb: &PhysicsCollider, cb_pos: Vec3, cb_rot: Quat) -> Option<ContactGeom> {
    // A = sphere, B = heightfield (normal already A→B).
    let a_sphere = sphere_vs_heightfield(ca.shape(), ba_pos, cb, cb_pos, cb_rot);
    // A = heightfield, B = sphere → sphere(B)↔heightfield(A), then flip B→A to A→B.
    let b_sphere = sphere_vs_heightfield(cb.shape(), cb_pos, ca, ba_pos, ba_rot).map(flip);
    a_sphere.or(b_sphere)
}

/// Generate a contact manifold for every candidate pair that is genuinely
/// overlapping, preserving the broad phase's sorted pair order. Each pair's
/// colliders and bodies are resolved by handle (always present, since the pair
/// came from these very slices), then dispatched through [`CONTACT_TABLE`] (and
/// the [`heightfield_contact`] path for sphere↔heightfield pairs).
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
                    let index = ca.shape().kind().index() * 5 + cb.shape().kind().index();
                    let (pa, ra) = (ba.transform().translation, ba.transform().rotation);
                    let (pb, rb) = (bb.transform().translation, bb.transform().rotation);
                    CONTACT_TABLE[index](ca.shape(), pa, ra, cb.shape(), pb, rb)
                        .or_else(|| heightfield_contact(ca, pa, ra, cb, pb, rb))
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

    /// The identity body rotation, for the axis-aligned generator tests.
    fn id() -> Quat {
        Quat::IDENTITY
    }


    #[test]
    fn sphere_sphere_hit_reports_axis_normal_and_depth() {
        let g = sphere_sphere(sphere(1.0), Vec3::ZERO, id(), sphere(1.0), Vec3::new(1.5, 0.0, 0.0), id())
            .expect("overlapping spheres are in contact");
        approx(g.normal, Vec3::new(1.0, 0.0, 0.0)); // A -> B is +X
        assert!((g.depth - 0.5).abs() < 1.0e-6);
        approx(g.point, Vec3::new(0.75, 0.0, 0.0));
    }

    #[test]
    fn sphere_sphere_miss_when_separated_or_coincident() {
        assert!(sphere_sphere(sphere(1.0), Vec3::ZERO, id(), sphere(1.0), Vec3::new(3.0, 0.0, 0.0), id()).is_none());
        // Exactly touching (dist == sum) is not a contact.
        assert!(sphere_sphere(sphere(1.0), Vec3::ZERO, id(), sphere(1.0), Vec3::new(2.0, 0.0, 0.0), id()).is_none());
        // Coincident centers have no defined normal.
        assert!(sphere_sphere(sphere(1.0), Vec3::ZERO, id(), sphere(1.0), Vec3::ZERO, id()).is_none());
    }


    #[test]
    fn sphere_plane_hit_pushes_along_plane_normal() {
        let g = sphere_plane(sphere(1.0), Vec3::new(0.0, 0.5, 0.0), id(), plane(), Vec3::ZERO, id())
            .expect("sphere crossing the plane is in contact");
        approx(g.normal, Vec3::new(0.0, -1.0, 0.0)); // sphere(A) -> plane(B) is downward
        assert!((g.depth - 0.5).abs() < 1.0e-6);
        approx(g.point, Vec3::new(0.0, -0.5, 0.0));
    }

    #[test]
    fn sphere_plane_miss_when_above_surface() {
        assert!(sphere_plane(sphere(1.0), Vec3::new(0.0, 2.0, 0.0), id(), plane(), Vec3::ZERO, id()).is_none());
    }


    #[test]
    fn box_plane_hit_uses_projection_radius() {
        let g = box_plane(box_shape(1.0, 1.0, 1.0), Vec3::new(0.0, 0.5, 0.0), id(), plane(), Vec3::ZERO, id())
            .expect("box crossing the plane is in contact");
        approx(g.normal, Vec3::new(0.0, -1.0, 0.0));
        assert!((g.depth - 0.5).abs() < 1.0e-6);
    }

    #[test]
    fn box_plane_miss_when_above_surface() {
        assert!(box_plane(box_shape(1.0, 1.0, 1.0), Vec3::new(0.0, 2.0, 0.0), id(), plane(), Vec3::ZERO, id()).is_none());
    }

    #[test]
    fn box_plane_uses_the_rotated_projection_radius_for_a_tilted_box() {
        use core::f32::consts::FRAC_PI_4;
        // A unit box on the plane y = 0, yawed 45° about Z so its half-height along
        // the plane normal (+Y) grows to sqrt(2)/2·(hx+hy) = sqrt(2). Its centre is
        // lifted to y = 1.3, so the projected underside dips 1.414 - 1.3 = 0.114
        // below the surface — a contact the axis-aligned radius (1.0 < 1.3) would
        // have missed.
        let tilt = Quat::from_axis_angle(Vec3::UNIT_Z, FRAC_PI_4).unwrap();
        let g = box_plane(box_shape(1.0, 1.0, 1.0), Vec3::new(0.0, 1.3, 0.0), tilt, plane(), Vec3::ZERO, id())
            .expect("tilted box dips below the plane");
        approx(g.normal, Vec3::new(0.0, -1.0, 0.0));
        assert!((g.depth - (2.0_f32.sqrt() - 1.3)).abs() < 1.0e-5, "rotated radius depth, got {}", g.depth);
        // Axis-aligned, the same box at y = 1.3 clears the plane (radius 1.0).
        assert!(box_plane(box_shape(1.0, 1.0, 1.0), Vec3::new(0.0, 1.3, 0.0), id(), plane(), Vec3::ZERO, id()).is_none());
    }


    #[test]
    fn sphere_box_hit_resolves_against_closest_face() {
        let g = sphere_box(sphere(1.0), Vec3::new(0.0, 1.5, 0.0), id(), box_shape(1.0, 1.0, 1.0), Vec3::ZERO, id())
            .expect("sphere resting above the box top is in contact");
        approx(g.normal, Vec3::new(0.0, -1.0, 0.0)); // sphere(A) -> box(B) is downward
        assert!((g.depth - 0.5).abs() < 1.0e-6);
        approx(g.point, Vec3::new(0.0, 1.0, 0.0));
    }

    #[test]
    fn sphere_box_miss_when_outside_or_center_inside() {
        // Far above the box.
        assert!(sphere_box(sphere(1.0), Vec3::new(0.0, 5.0, 0.0), id(), box_shape(1.0, 1.0, 1.0), Vec3::ZERO, id()).is_none());
        // Sphere center inside the box -> no defined normal -> no contact.
        assert!(sphere_box(sphere(1.0), Vec3::ZERO, id(), box_shape(1.0, 1.0, 1.0), Vec3::ZERO, id()).is_none());
    }

    #[test]
    fn sphere_box_collides_on_the_tilted_face_of_an_oriented_box() {
        use core::f32::consts::FRAC_PI_4;
        // A long ramp box (a thin slab, half-extents 3 × 0.25 × 1) pitched 45° about
        // Z. Its top face normal rotates to (−sin45, cos45, 0). A sphere placed just
        // off that face along the rotated normal must contact it, and the reported
        // world normal (sphere→box) must be the inward rotated face normal.
        let pitch = Quat::from_axis_angle(Vec3::UNIT_Z, FRAC_PI_4).unwrap();
        let he = box_shape(3.0, 0.25, 1.0);
        let face_normal = pitch.rotate(Vec3::UNIT_Y); // outward top-face normal
        // Sphere centre = face point (0.25 out along local +Y, rotated) + 0.4 along
        // the outward normal; radius 0.5 -> penetration depth 0.1.
        let face_point = pitch.rotate(Vec3::new(0.0, 0.25, 0.0));
        let sphere_center = face_point.add(face_normal.mul_scalar(0.4));
        let g = sphere_box(sphere(0.5), sphere_center, id(), he, Vec3::ZERO, pitch)
            .expect("sphere resting on the tilted face is in contact");
        // Sphere(A) -> box(B) normal points into the ramp, i.e. -outward.
        approx(g.normal, face_normal.mul_scalar(-1.0));
        assert!((g.depth - 0.1).abs() < 1.0e-5, "depth on tilted face, got {}", g.depth);
        // Slide the sphere out past the radius along the same normal -> a miss that
        // the axis-aligned test (which sees a wide flat slab) would have reported.
        let outside = face_point.add(face_normal.mul_scalar(0.6));
        assert!(sphere_box(sphere(0.5), outside, id(), he, Vec3::ZERO, pitch).is_none());
    }


    #[test]
    fn box_sphere_flips_the_canonical_normal() {
        let g = box_sphere(box_shape(1.0, 1.0, 1.0), Vec3::ZERO, id(), sphere(1.0), Vec3::new(0.0, 1.5, 0.0), id())
            .expect("box below sphere is in contact");
        approx(g.normal, Vec3::new(0.0, 1.0, 0.0)); // box(A) -> sphere(B) is upward
        assert!((g.depth - 0.5).abs() < 1.0e-6);
    }

    #[test]
    fn plane_sphere_flips_the_canonical_normal() {
        let g = plane_sphere(plane(), Vec3::ZERO, id(), sphere(1.0), Vec3::new(0.0, 0.5, 0.0), id())
            .expect("plane below sphere is in contact");
        approx(g.normal, Vec3::new(0.0, 1.0, 0.0)); // plane(A) -> sphere(B) is upward
    }

    #[test]
    fn plane_box_flips_the_canonical_normal() {
        let g = plane_box(plane(), Vec3::ZERO, id(), box_shape(1.0, 1.0, 1.0), Vec3::new(0.0, 0.5, 0.0), id())
            .expect("plane below box is in contact");
        approx(g.normal, Vec3::new(0.0, 1.0, 0.0));
    }

    #[test]
    fn swapped_miss_returns_none() {
        assert!(box_sphere(box_shape(1.0, 1.0, 1.0), Vec3::ZERO, id(), sphere(1.0), Vec3::new(0.0, 5.0, 0.0), id()).is_none());
    }


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

    fn heightfield_collider(collider_raw: u64, body_raw: u64, grid: crate::physics_heightfield::Heightfield) -> PhysicsCollider {
        let shape = PhysicsColliderShape::heightfield_shape(grid.half_extents()).unwrap();
        PhysicsCollider::new_heightfield(
            PhysicsColliderHandle::from_raw(collider_raw),
            PhysicsBodyHandle::from_raw(body_raw),
            shape,
            material(),
            false,
            grid,
        )
    }

    fn flat_grid() -> crate::physics_heightfield::Heightfield {
        crate::physics_heightfield::Heightfield::new(3, 3, 1.0, 1.0, vec![0.0; 9])
    }

    #[test]
    fn sphere_rests_in_a_flat_heightfield_and_misses_when_away() {
        let hf = heightfield_collider(20, 2, flat_grid());
        // Centre 0.5 above the surface → penetrates 0.5; A(sphere)→B(field) is down.
        let g = sphere_vs_heightfield(sphere(1.0), Vec3::new(0.0, 0.5, 0.0), &hf, Vec3::ZERO, id())
            .expect("sphere sits in the surface");
        approx(g.normal, Vec3::new(0.0, -1.0, 0.0));
        assert!((g.depth - 0.5).abs() < 1.0e-5);
        approx(g.point, Vec3::ZERO);
        // Above by more than r, outside the footprint, tunnelled far below, and a
        // non-sphere first shape all produce no contact.
        assert!(sphere_vs_heightfield(sphere(1.0), Vec3::new(0.0, 2.0, 0.0), &hf, Vec3::ZERO, id()).is_none());
        assert!(sphere_vs_heightfield(sphere(1.0), Vec3::new(10.0, 0.5, 0.0), &hf, Vec3::ZERO, id()).is_none());
        assert!(sphere_vs_heightfield(sphere(1.0), Vec3::new(0.0, -2.0, 0.0), &hf, Vec3::ZERO, id()).is_none());
        assert!(sphere_vs_heightfield(box_shape(1.0, 1.0, 1.0), Vec3::new(0.0, 0.5, 0.0), &hf, Vec3::ZERO, id()).is_none());
    }

    #[test]
    fn heightfield_contact_handles_both_orderings_and_ignores_other_pairs() {
        let hf = heightfield_collider(20, 2, flat_grid());
        let sph = collider(10, 1, sphere(1.0));
        // A = sphere over field: normal points down (sphere → field).
        let ab = heightfield_contact(&sph, Vec3::new(0.0, 0.5, 0.0), id(), &hf, Vec3::ZERO, id())
            .expect("sphere over heightfield");
        approx(ab.normal, Vec3::new(0.0, -1.0, 0.0));
        // A = field under sphere: the flip makes the field → sphere normal point up.
        let ba = heightfield_contact(&hf, Vec3::ZERO, id(), &sph, Vec3::new(0.0, 0.5, 0.0), id())
            .expect("heightfield under sphere");
        approx(ba.normal, Vec3::new(0.0, 1.0, 0.0));
        // Neither collider is a heightfield → no contact.
        let box_c = collider(30, 3, box_shape(1.0, 1.0, 1.0));
        assert!(heightfield_contact(&sph, Vec3::ZERO, id(), &box_c, Vec3::ZERO, id()).is_none());
    }

    #[test]
    fn generate_contacts_resolves_a_sphere_on_a_heightfield_pair() {
        let bodies = [body(1, 0.5), body(2, 0.0)];
        let colliders = [collider(10, 1, sphere(1.0)), heightfield_collider(20, 2, flat_grid())];
        let pairs = [BroadPhasePair::new(
            PhysicsColliderHandle::from_raw(10),
            PhysicsColliderHandle::from_raw(20),
        )];
        let contacts = generate_contacts(&pairs, &colliders, &bodies);
        assert_eq!(contacts.len(), 1);
        approx(contacts[0].normal(), Vec3::new(0.0, -1.0, 0.0));
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
