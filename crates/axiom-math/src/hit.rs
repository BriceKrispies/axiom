//! What a cast or a sweep found when it touched a surface.

use crate::approx_eq::ApproxEq;
use crate::epsilon::Epsilon;
use crate::vec3::Vec3;

/// The record every cast and swept test in this layer returns: *when* the
/// moving geometry first touched, *where* it touched, and which way the touched
/// surface faces.
///
/// * `time` is the parameter of the query that produced it — a distance along
///   the ray for the [`crate::Ray`] casts (whose direction is a unit vector),
///   and a fraction of the motion vector in `[0, 1]` for the swept tests.
/// * `point` lies on the **struck** surface — the triangle, the static capsule,
///   the box face — never on the mover.
/// * `normal` is a unit vector of that surface pointing back at the mover, so a
///   character controller can project its remaining motion onto it without
///   re-deriving a sign.
///
/// "Whether it hit" is the `Option` that wraps this record: a query which found
/// nothing returns `None`, so a `Hit` value always describes a real touch and
/// never has to be interrogated for validity.
#[derive(Debug, Clone, Copy)]
pub struct Hit {
    time: f32,
    point: Vec3,
    normal: Vec3,
}

impl Hit {
    /// Construct from an already-solved contact. The queries in this layer are
    /// the intended producers; the constructor is public so a caller can build
    /// the same record for a surface this layer does not model yet.
    pub const fn new(time: f32, point: Vec3, normal: Vec3) -> Hit {
        Hit {
            time,
            point,
            normal,
        }
    }

    /// Time of impact: a distance for a ray cast, a fraction of the motion for
    /// a sweep.
    pub const fn time(&self) -> f32 {
        self.time
    }

    /// The contact point, on the struck surface.
    pub const fn point(&self) -> Vec3 {
        self.point
    }

    /// The unit surface normal at the contact, facing the mover.
    pub const fn normal(&self) -> Vec3 {
        self.normal
    }
}

impl ApproxEq for Hit {
    fn approx_eq(&self, other: &Self, epsilon: Epsilon) -> bool {
        self.time.approx_eq(&other.time, epsilon)
            & self.point.approx_eq(&other.point, epsilon)
            & self.normal.approx_eq(&other.normal, epsilon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eps() -> Epsilon {
        Epsilon::DEFAULT
    }

    #[test]
    fn accessors_return_the_constructed_contact() {
        let hit = Hit::new(0.25, Vec3::new(1.0, 2.0, 3.0), Vec3::UNIT_Y);
        assert_eq!(hit.time(), 0.25);
        assert!(hit.point().approx_eq(&Vec3::new(1.0, 2.0, 3.0), eps()));
        assert!(hit.normal().approx_eq(&Vec3::UNIT_Y, eps()));
    }

    #[test]
    fn approx_eq_compares_every_field() {
        let hit = Hit::new(0.5, Vec3::ZERO, Vec3::UNIT_Y);
        assert!(hit.approx_eq(&hit, eps()));
        assert!(!hit.approx_eq(&Hit::new(0.6, Vec3::ZERO, Vec3::UNIT_Y), eps()));
        assert!(!hit.approx_eq(&Hit::new(0.5, Vec3::UNIT_X, Vec3::UNIT_Y), eps()));
        assert!(!hit.approx_eq(&Hit::new(0.5, Vec3::ZERO, Vec3::UNIT_X), eps()));
    }
}
