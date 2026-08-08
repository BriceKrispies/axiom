//! The geometry that decides whether the ball touched something.
//!
//! Everything the ball can hit in this game — a post, the crossbar, a keeper's
//! outstretched arms, a keeper's body — is a **capsule**: a segment with a
//! radius. And the ball is not a point, it is a sphere moving a finite distance
//! each tick, so the honest test is capsule-against-capsule: the ball's own swept
//! segment against the obstacle's segment, with the two radii added.
//!
//! Doing it this way is what keeps the game's central promise. A save is not a
//! flag the keeper sets; it is this function returning a contact between the
//! keeper's actual reach and the ball's actual path. Nothing here ever moves the
//! ball — it only reports.

use axiom::prelude::Vec3;

/// A segment with a radius: the shape of a post, a crossbar, an arm, a torso, or
/// one tick of ball travel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Capsule {
    pub a: Vec3,
    pub b: Vec3,
    pub radius: f32,
}

impl Capsule {
    pub fn new(a: Vec3, b: Vec3, radius: f32) -> Self {
        Capsule { a, b, radius }
    }
}

/// A reported touch: where on the ball's swept segment it happened (`0..1`) and
/// the world point of closest approach on the obstacle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Contact {
    /// Fraction along the ball's swept segment.
    pub travel: f32,
    /// Closest point on the obstacle's axis.
    pub point: Vec3,
    /// Unit vector from the obstacle's axis toward the ball — the direction a
    /// deflection pushes.
    pub normal: Vec3,
}

/// Squared length of a vector.
fn len_sq(v: Vec3) -> f32 {
    v.dot(v)
}

/// The closest approach between two segments, as `(s, t, distance)` where `s`
/// and `t` are the clamped parameters along `p0→p1` and `q0→q1`.
///
/// The standard clamped-parametric solve, written without branches: the
/// degenerate cases (either segment a point, the two parallel) all fall out of
/// clamping a division whose denominator is floored away from zero, so a
/// zero-length segment simply resolves to its own endpoint instead of needing a
/// special case.
pub fn segment_closest(p0: Vec3, p1: Vec3, q0: Vec3, q1: Vec3) -> (f32, f32, f32) {
    let d1 = p1.subtract(p0);
    let d2 = q1.subtract(q0);
    let r = p0.subtract(q0);
    let a = len_sq(d1).max(1.0e-9);
    let e = len_sq(d2).max(1.0e-9);
    let f = d2.dot(r);
    let b = d1.dot(d2);
    let c = d1.dot(r);
    let denom = (a * e - b * b).max(1.0e-9);
    // First pass, then one re-clamp of each parameter against the other — two
    // rounds are enough for a clamped solve to land on the true minimum.
    let s0 = ((b * f - c * e) / denom).clamp(0.0, 1.0);
    let t = ((b * s0 + f) / e).clamp(0.0, 1.0);
    let s = ((b * t - c) / a).clamp(0.0, 1.0);
    let pa = p0.add(d1.mul_scalar(s));
    let pb = q0.add(d2.mul_scalar(t));
    (s, t, pa.subtract(pb).length())
}

/// Test one tick of ball travel (`from` → `to`, radius `ball_radius`) against a
/// capsule. `None` when they never come within the combined radii.
pub fn sweep(from: Vec3, to: Vec3, ball_radius: f32, obstacle: Capsule) -> Option<Contact> {
    let (s, _, distance) = segment_closest(from, to, obstacle.a, obstacle.b);
    let reach = ball_radius + obstacle.radius;
    (distance <= reach).then(|| {
        let travelled = from.add(to.subtract(from).mul_scalar(s));
        let (_, t, _) = segment_closest(travelled, travelled, obstacle.a, obstacle.b);
        let point = obstacle.a.add(obstacle.b.subtract(obstacle.a).mul_scalar(t));
        let away = travelled.subtract(point);
        let normal = away
            .normalize()
            .unwrap_or_else(|_| Vec3::new(0.0, 0.0, 1.0));
        Contact {
            travel: s,
            point,
            normal,
        }
    })
}

/// Reflect a velocity off a contact normal, keeping `restitution` of the
/// perpendicular component and `friction_keep` of the tangential one.
pub fn deflect(velocity: Vec3, normal: Vec3, restitution: f32, friction_keep: f32) -> Vec3 {
    let along = velocity.dot(normal);
    let perpendicular = normal.mul_scalar(along);
    let tangent = velocity.subtract(perpendicular);
    tangent
        .mul_scalar(friction_keep)
        .subtract(perpendicular.mul_scalar(restitution))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossing_segments_touch_at_zero_distance() {
        let (s, t, d) = segment_closest(
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        );
        assert!((s - 0.5).abs() < 1.0e-4);
        assert!((t - 0.5).abs() < 1.0e-4);
        assert!(d < 1.0e-4);
    }

    #[test]
    fn parallel_and_degenerate_segments_still_resolve() {
        let (_, _, d) = segment_closest(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 2.0, 0.0),
            Vec3::new(1.0, 2.0, 0.0),
        );
        assert!((d - 2.0).abs() < 1.0e-4);
        // A point against a point.
        let (_, _, d) = segment_closest(Vec3::ZERO, Vec3::ZERO, Vec3::UNIT_Y, Vec3::UNIT_Y);
        assert!((d - 1.0).abs() < 1.0e-4);
    }

    #[test]
    fn a_sweep_that_passes_the_capsule_reports_a_contact() {
        let post = Capsule::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 2.0, 0.0), 0.06);
        let hit = sweep(
            Vec3::new(-1.0, 1.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            0.11,
            post,
        )
        .expect("the ball crosses the post");
        assert!((hit.travel - 0.5).abs() < 1.0e-3);
        assert!(hit.point.y > 0.0);
        assert!(hit.normal.length() > 0.9);
    }

    #[test]
    fn a_sweep_that_clears_the_capsule_reports_nothing() {
        let post = Capsule::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 2.0, 0.0), 0.06);
        assert_eq!(
            sweep(
                Vec3::new(-1.0, 1.0, 0.5),
                Vec3::new(1.0, 1.0, 0.5),
                0.11,
                post
            ),
            None
        );
    }

    #[test]
    fn a_contact_dead_on_the_axis_still_yields_a_usable_normal() {
        let bar = Capsule::new(Vec3::new(-1.0, 1.0, 0.0), Vec3::new(1.0, 1.0, 0.0), 0.06);
        let hit = sweep(
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            0.11,
            bar,
        )
        .expect("a zero-length sweep sitting on the axis still contacts");
        assert!(hit.normal.length() > 0.9);
    }

    #[test]
    fn deflection_reverses_the_perpendicular_and_keeps_the_tangent() {
        let out = deflect(
            Vec3::new(1.0, 0.0, -4.0),
            Vec3::new(0.0, 0.0, 1.0),
            0.5,
            0.8,
        );
        assert!((out.z - 2.0).abs() < 1.0e-4);
        assert!((out.x - 0.8).abs() < 1.0e-4);
    }
}
