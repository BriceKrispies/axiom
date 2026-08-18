//! The box↔box narrow-phase pairing, by the separating-axis theorem.
//!
//! Two oriented boxes are disjoint exactly when some axis separates their
//! projections, and for boxes it suffices to test fifteen: each box's three face
//! normals, and the nine cross products of one box's axes with the other's (the
//! candidate edge-edge directions). If none separates them, the axis of
//! **minimum overlap** is the shallowest way out, so it is the contact normal and
//! its overlap is the penetration depth.
//!
//! ## Branchless, in one fold
//! The fifteen candidates are folded once, carrying three things: the smallest
//! positive overlap seen so far, the axis that produced it (already oriented A→B),
//! and whether any axis has separated the pair. A cross product of two parallel
//! axes is the zero vector — it names no direction and must not be allowed to
//! claim a separation — so each candidate is normalized and the degenerate ones
//! are excluded arithmetically, never by an early exit.
//!
//! ## The contact point, and why the support function ignores zero
//! A single contact point has to stand in for what is often a whole face. The
//! point reported is the midpoint of the two boxes' **support points** along the
//! contact normal — the extreme point of A in the direction of B, and of B back
//! toward A. The support uses a three-valued sign (`-1`, `0`, `+1`) rather than
//! `signum`: an axis exactly perpendicular to the normal contributes **nothing**,
//! so a box resting squarely on another reports its face *centre* rather than an
//! arbitrarily chosen corner, and the solver separates the two without spinning
//! them. `f32::signum` would answer `+1` for a perpendicular axis and pick a
//! corner — the sign convention is load-bearing here.

use axiom_math::{Quat, Vec3};

use crate::contact_geom::ContactGeom;
use crate::physics_collider_shape::PhysicsColliderShape;

/// A normalized candidate axis shorter than this is the degenerate zero vector a
/// parallel-edge cross product produced, not a direction.
const DEGENERATE_AXIS: f32 = 0.5;

/// A box's three world axes, in local X/Y/Z order.
fn box_axes(rotation: Quat) -> [Vec3; 3] {
    [
        rotation.rotate(Vec3::UNIT_X),
        rotation.rotate(Vec3::UNIT_Y),
        rotation.rotate(Vec3::UNIT_Z),
    ]
}

/// Half the box's width measured along `axis` — the L1 combination of its
/// half-extents with the axis expressed in the box's own frame.
fn projected_radius(half: Vec3, axes: [Vec3; 3], axis: Vec3) -> f32 {
    half.x * axis.dot(axes[0]).abs()
        + half.y * axis.dot(axes[1]).abs()
        + half.z * axis.dot(axes[2]).abs()
}

/// `-1`, `0` or `+1`. Unlike [`f32::signum`], zero maps to zero — which is what
/// makes a support point collapse to a face centre instead of a corner when the
/// direction is perpendicular to an axis.
fn axis_sign(x: f32) -> f32 {
    f32::from(x > 0.0) - f32::from(x < 0.0)
}

/// The extreme point of a box in direction `dir`, with perpendicular axes
/// contributing nothing (see the module docs).
fn support(center: Vec3, half: Vec3, axes: [Vec3; 3], dir: Vec3) -> Vec3 {
    center
        .add(axes[0].mul_scalar(half.x * axis_sign(dir.dot(axes[0]))))
        .add(axes[1].mul_scalar(half.y * axis_sign(dir.dot(axes[1]))))
        .add(axes[2].mul_scalar(half.z * axis_sign(dir.dot(axes[2]))))
}

/// The fifteen separating-axis candidates: six face normals then nine edge-edge
/// cross products.
fn candidate_axes(a: [Vec3; 3], b: [Vec3; 3]) -> [Vec3; 15] {
    [
        a[0],
        a[1],
        a[2],
        b[0],
        b[1],
        b[2],
        a[0].cross(b[0]),
        a[0].cross(b[1]),
        a[0].cross(b[2]),
        a[1].cross(b[0]),
        a[1].cross(b[1]),
        a[1].cross(b[2]),
        a[2].cross(b[0]),
        a[2].cross(b[1]),
        a[2].cross(b[2]),
    ]
}

/// Box (A) vs box (B). Both rotations are genuinely used — this is an oriented
/// test, exact for any pair of orientations.
///
/// Two coincident boxes are **not** a degenerate case here, unlike two
/// coincident spheres: they still have a well-defined shallowest escape axis
/// (the one whose summed half-widths are smallest), which the fold selects
/// deterministically, so the solver can push them apart instead of leaving them
/// interpenetrating forever.
pub(crate) fn box_box(
    a: PhysicsColliderShape,
    ca: Vec3,
    ra: Quat,
    b: PhysicsColliderShape,
    cb: Vec3,
    rb: Quat,
) -> Option<ContactGeom> {
    let (ha, hb) = (a.half_extents(), b.half_extents());
    let (ua, ub) = (box_axes(ra), box_axes(rb));
    let delta = cb.subtract(ca);
    let (depth, normal, separated) = candidate_axes(ua, ub).into_iter().fold(
        (f32::INFINITY, Vec3::ZERO, false),
        |(best, best_normal, separating), candidate| {
            let unit = candidate.normalize().unwrap_or(Vec3::ZERO);
            let real = unit.length_squared() > DEGENERATE_AXIS;
            let signed = unit.dot(delta);
            let overlap =
                projected_radius(ha, ua, unit) + projected_radius(hb, ub, unit) - signed.abs();
            let better = real & (overlap > 0.0) & (overlap < best);
            // Orient the winner from A toward B; a zero projection (concentric on
            // this axis) takes the positive sense, deterministically.
            let oriented = unit.mul_scalar([1.0, -1.0][usize::from(signed < 0.0)]);
            (
                [best, overlap][usize::from(better)],
                [best_normal, oriented][usize::from(better)],
                separating | (real & (overlap <= 0.0)),
            )
        },
    );
    (!separated).then_some(ContactGeom {
        normal,
        depth,
        point: support(ca, ha, ua, normal)
            .add(support(cb, hb, ub, normal.mul_scalar(-1.0)))
            .mul_scalar(0.5),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f32::consts::FRAC_PI_4;

    fn box_shape(x: f32, y: f32, z: f32) -> PhysicsColliderShape {
        PhysicsColliderShape::box_shape(Vec3::new(x, y, z)).unwrap()
    }

    fn id() -> Quat {
        Quat::IDENTITY
    }

    fn approx(a: Vec3, b: Vec3) {
        assert!(
            a.subtract(b).length_squared() < 1.0e-7,
            "expected {b:?}, got {a:?}"
        );
    }

    #[test]
    fn a_box_resting_on_a_box_pushes_along_the_face_normal_from_the_face_centre() {
        // Unit box at the origin, second unit box centred at y = 1.5: they
        // overlap by 0.5 in Y and by a full 2.0 in X and Z, so Y is the
        // shallowest axis.
        let g = box_box(
            box_shape(1.0, 1.0, 1.0),
            Vec3::ZERO,
            id(),
            box_shape(1.0, 1.0, 1.0),
            Vec3::new(0.0, 1.5, 0.0),
            id(),
        )
        .expect("overlapping boxes are in contact");
        approx(g.normal, Vec3::UNIT_Y);
        assert!((g.depth - 0.5).abs() < 1.0e-6, "depth was {}", g.depth);
        // Both supports are face centres, so the contact is on the axis of the
        // stack, not at a corner.
        approx(g.point, Vec3::new(0.0, 0.75, 0.0));
    }

    #[test]
    fn the_shallowest_axis_wins_even_when_it_is_not_the_first_tested() {
        // Deep overlap in Y (1.8) and X (1.5), shallow in Z (0.2).
        let g = box_box(
            box_shape(1.0, 1.0, 1.0),
            Vec3::ZERO,
            id(),
            box_shape(1.0, 1.0, 1.0),
            Vec3::new(0.5, 0.2, 1.8),
            id(),
        )
        .expect("overlapping boxes are in contact");
        approx(g.normal, Vec3::UNIT_Z);
        assert!((g.depth - 0.2).abs() < 1.0e-5, "depth was {}", g.depth);
    }

    #[test]
    fn separated_and_exactly_touching_boxes_report_no_contact() {
        let cube = box_shape(1.0, 1.0, 1.0);
        assert!(box_box(cube, Vec3::ZERO, id(), cube, Vec3::new(9.0, 0.0, 0.0), id()).is_none());
        // Face-to-face at exactly zero overlap is not a contact.
        assert!(box_box(cube, Vec3::ZERO, id(), cube, Vec3::new(2.0, 0.0, 0.0), id()).is_none());
        // Separated on a diagonal, where no face axis alone is the witness.
        assert!(box_box(
            cube,
            Vec3::ZERO,
            id(),
            cube,
            Vec3::new(2.5, 2.5, 2.5),
            id()
        )
        .is_none());
    }

    #[test]
    fn a_turned_box_is_measured_on_its_rotated_projection() {
        // A box yawed 45 degrees about Y reaches sqrt(2) along X and Z. Placed at
        // x = 2.5 it clears a unit box (1 + 1.414 = 2.414 < 2.5); the same pair
        // pushed to x = 2.3 overlaps, and the reported depth is the true 0.114 —
        // a number only the *rotated* projection radius produces.
        let yaw = Quat::from_axis_angle(Vec3::UNIT_Y, FRAC_PI_4).unwrap();
        let cube = box_shape(1.0, 1.0, 1.0);
        assert!(box_box(cube, Vec3::ZERO, id(), cube, Vec3::new(2.5, 0.0, 0.0), yaw).is_none());
        let g = box_box(cube, Vec3::ZERO, id(), cube, Vec3::new(2.3, 0.0, 0.0), yaw)
            .expect("the turned box's corner is inside");
        approx(g.normal, Vec3::UNIT_X);
        assert!(
            (g.depth - (1.0 + 2.0_f32.sqrt() - 2.3)).abs() < 1.0e-5,
            "depth was {}",
            g.depth
        );
    }

    #[test]
    fn two_crossed_slabs_escape_along_the_tilted_slab_s_own_face_normal() {
        // A long slab rolled 45 degrees about X crossing a slab lying along X.
        // The shallowest escape is the rolled slab's own thin-face normal
        // (0, 1, 1)/sqrt(2) — an axis that exists only because the test projects
        // both boxes onto *both* boxes' axes, not just the first one's.
        let roll = Quat::from_axis_angle(Vec3::UNIT_X, FRAC_PI_4).unwrap();
        let g = box_box(
            box_shape(4.0, 0.5, 0.5),
            Vec3::ZERO,
            id(),
            box_shape(0.5, 0.5, 4.0),
            Vec3::new(0.0, 0.9, 0.0),
            roll,
        )
        .expect("the crossed slabs overlap");
        let root_half = 0.5_f32.sqrt();
        approx(g.normal, Vec3::new(0.0, root_half, root_half));
        // 0.7071 (A's half-width on that axis) + 0.5 (B's) - 0.6364 (the gap).
        assert!(
            (g.depth - (root_half + 0.5 - 0.9 * root_half)).abs() < 1.0e-5,
            "depth was {}",
            g.depth
        );
    }

    #[test]
    fn the_candidate_axes_are_the_six_faces_then_the_nine_edge_crosses_in_order() {
        // The nine cross products are easy to transpose silently, and a
        // transposed pair would only surface as a rare missed contact. Pin the
        // exact set and order instead.
        let a = box_axes(Quat::from_axis_angle(Vec3::UNIT_Y, FRAC_PI_4).unwrap());
        let b = box_axes(Quat::from_axis_angle(Vec3::UNIT_X, FRAC_PI_4).unwrap());
        let axes = candidate_axes(a, b);
        assert_eq!(axes.len(), 15);
        (0..3).for_each(|i| {
            approx(axes[i], a[i]);
            approx(axes[3 + i], b[i]);
        });
        (0..3).for_each(|i| {
            (0..3).for_each(|j| {
                approx(axes[6 + i * 3 + j], a[i].cross(b[j]));
            });
        });
    }

    #[test]
    fn parallel_edge_axes_are_degenerate_and_never_claim_a_separation() {
        // Two identically-oriented boxes make each axis's cross with its own
        // counterpart the zero vector — X x X, Y x Y, Z x Z, the diagonal of the
        // 3x3 block. If a zero axis were allowed to vote, its overlap of exactly
        // 0 would mark the pair separated and no two axis-aligned boxes would
        // ever collide.
        let cube = box_shape(1.0, 1.0, 1.0);
        let axes = candidate_axes(box_axes(id()), box_axes(id()));
        let degenerate: Vec<usize> = (6..15)
            .filter(|i| axes[*i].length_squared() == 0.0)
            .collect();
        assert_eq!(
            degenerate,
            vec![6, 10, 14],
            "exactly the self-paired crosses collapse"
        );
        assert!(
            box_box(cube, Vec3::ZERO, id(), cube, Vec3::new(0.0, 1.5, 0.0), id()).is_some(),
            "the degenerate axes must be excluded, not counted as separating"
        );
    }

    #[test]
    fn coincident_boxes_escape_along_their_shortest_axis() {
        // A flat slab exactly on top of itself: no separating axis exists, and
        // the shallowest way out is the thin Y axis.
        let slab = box_shape(3.0, 0.5, 3.0);
        let g = box_box(slab, Vec3::ZERO, id(), slab, Vec3::ZERO, id())
            .expect("coincident boxes are in contact");
        assert!((g.depth - 1.0).abs() < 1.0e-6, "depth was {}", g.depth);
        approx(g.normal, Vec3::UNIT_Y);
        approx(g.point, Vec3::ZERO);
    }

    #[test]
    fn the_normal_always_points_from_a_toward_b() {
        let cube = box_shape(1.0, 1.0, 1.0);
        let above = box_box(cube, Vec3::ZERO, id(), cube, Vec3::new(0.0, 1.5, 0.0), id())
            .expect("in contact");
        let below = box_box(cube, Vec3::ZERO, id(), cube, Vec3::new(0.0, -1.5, 0.0), id())
            .expect("in contact");
        approx(above.normal, Vec3::UNIT_Y);
        approx(below.normal, Vec3::new(0.0, -1.0, 0.0));
    }

    #[test]
    fn axis_sign_is_three_valued_unlike_signum() {
        assert_eq!(axis_sign(2.0), 1.0);
        assert_eq!(axis_sign(-2.0), -1.0);
        assert_eq!(axis_sign(0.0), 0.0);
        assert_eq!(axis_sign(-0.0), 0.0);
        // The distinction that matters: `signum` would answer 1.0 here.
        assert_ne!(axis_sign(0.0), 0.0_f32.signum());
    }
}
