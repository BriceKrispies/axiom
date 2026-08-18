//! A capsule: every point within a radius of a segment.

use crate::approx_eq::ApproxEq;
use crate::epsilon::Epsilon;
use crate::math_error::MathError;
use crate::math_result::MathResult;
use crate::segment::Segment;
use crate::sphere::Sphere;
use crate::triangle::Triangle;
use crate::vec3::Vec3;

/// The set of points within `radius` of a [`Segment`] — a cylinder with a
/// hemisphere on each end.
///
/// This is the character-controller volume: a body, a limb, a projectile's
/// swept core. A capsule with a degenerate segment is a sphere, and a capsule
/// with a zero radius is a segment; both are legal, and every query here
/// answers correctly for them.
#[derive(Debug, Clone, Copy)]
pub struct Capsule {
    segment: Segment,
    radius: f32,
}

impl Capsule {
    /// Construct from a segment and a finite, non-negative radius.
    pub fn new(segment: Segment, radius: f32) -> MathResult<Capsule> {
        (!radius.is_finite())
            .then_some(Err(MathError::non_finite_scalar(
                "capsule radius must be finite",
            )))
            .or_else(|| {
                (radius.is_finite() & (radius < 0.0)).then_some(Err(
                    MathError::invalid_sphere_radius("capsule radius must be non-negative"),
                ))
            })
            .unwrap_or(Ok(Capsule { segment, radius }))
    }

    /// Construct from parts a caller has already validated — the internal
    /// counterpart of [`Segment::from_points`]. The swept tests build inflated
    /// capsules (one body's radius plus another's) per query, from radii that
    /// are already finite and non-negative by construction.
    pub(crate) const fn from_parts(segment: Segment, radius: f32) -> Capsule {
        Capsule { segment, radius }
    }

    /// The axis segment.
    pub const fn segment(&self) -> Segment {
        self.segment
    }

    /// The radius around the axis.
    pub const fn radius(&self) -> f32 {
        self.radius
    }

    /// Inclusive point containment.
    pub fn contains_point(&self, p: Vec3) -> bool {
        self.segment.distance_squared_to_point(p) <= self.radius * self.radius
    }

    /// Whether `self` and `other` share any point — the distance between their
    /// two axes against the sum of their radii.
    pub fn overlaps(&self, other: &Capsule) -> bool {
        let (mine, theirs) = self.segment.closest_points_to_segment(&other.segment);
        let reach = self.radius + other.radius;
        mine.subtract(theirs).length_squared() <= reach * reach
    }

    /// Whether this capsule and `sphere` share any point.
    pub fn overlaps_sphere(&self, sphere: &Sphere) -> bool {
        let reach = self.radius + sphere.radius();
        self.segment.distance_squared_to_point(sphere.center()) <= reach * reach
    }

    /// Whether this capsule and `triangle` share any point.
    ///
    /// Two terms, because the closest-feature solve alone is not enough: a
    /// capsule whose axis *pierces* the face is at distance zero from it, and
    /// the boundary features that solve reports are all further away than that.
    /// The crossing test names exactly that case.
    pub fn overlaps_triangle(&self, triangle: &Triangle) -> bool {
        let (mine, theirs) = self.segment.closest_points_to_triangle(triangle);
        let touching = mine.subtract(theirs).length_squared() <= self.radius * self.radius;
        touching | triangle.intersect_segment(&self.segment).is_some()
    }
}

impl ApproxEq for Capsule {
    fn approx_eq(&self, other: &Self, epsilon: Epsilon) -> bool {
        self.segment.approx_eq(&other.segment, epsilon)
            & self.radius.approx_eq(&other.radius, epsilon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math_error_code::MathErrorCode;

    fn eps() -> Epsilon {
        Epsilon::DEFAULT
    }

    fn segment(start: Vec3, end: Vec3) -> Segment {
        Segment::new(start, end).unwrap()
    }

    /// A one-radius capsule standing on the origin, two units of axis tall.
    fn body() -> Capsule {
        Capsule::new(
            segment(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 3.0, 0.0)),
            1.0,
        )
        .unwrap()
    }

    #[test]
    fn new_rejects_a_negative_or_non_finite_radius() {
        let axis = segment(Vec3::ZERO, Vec3::UNIT_Y);
        assert_eq!(
            Capsule::new(axis, -0.5).unwrap_err().code(),
            MathErrorCode::InvalidSphereRadius
        );
        assert_eq!(
            Capsule::new(axis, f32::NAN).unwrap_err().code(),
            MathErrorCode::NonFiniteScalar
        );
    }

    #[test]
    fn accessors_report_axis_and_radius() {
        let capsule = body();
        assert!(capsule
            .segment()
            .start()
            .approx_eq(&Vec3::new(0.0, 1.0, 0.0), eps()));
        assert_eq!(capsule.radius(), 1.0);
    }

    #[test]
    fn contains_point_covers_the_shaft_the_caps_and_the_outside() {
        let capsule = body();
        assert!(capsule.contains_point(Vec3::new(0.0, 2.0, 0.0)));
        assert!(capsule.contains_point(Vec3::new(0.99, 2.0, 0.0)));
        assert!(!capsule.contains_point(Vec3::new(1.01, 2.0, 0.0)));
        // The caps reach a full radius beyond each end of the axis.
        assert!(capsule.contains_point(Vec3::new(0.0, 0.0, 0.0)));
        assert!(capsule.contains_point(Vec3::new(0.0, 4.0, 0.0)));
        assert!(!capsule.contains_point(Vec3::new(0.0, 4.01, 0.0)));
    }

    #[test]
    fn a_degenerate_capsule_is_a_sphere() {
        let ball = Capsule::new(segment(Vec3::ZERO, Vec3::ZERO), 2.0).unwrap();
        assert!(ball.contains_point(Vec3::new(0.0, 1.9, 0.0)));
        assert!(!ball.contains_point(Vec3::new(0.0, 2.1, 0.0)));
    }

    #[test]
    fn overlaps_answers_touching_crossing_and_separated_capsules() {
        let capsule = body();
        let touching = Capsule::new(
            segment(Vec3::new(2.0, 1.0, 0.0), Vec3::new(2.0, 3.0, 0.0)),
            1.0,
        )
        .unwrap();
        let separated = Capsule::new(
            segment(Vec3::new(2.5, 1.0, 0.0), Vec3::new(2.5, 3.0, 0.0)),
            1.0,
        )
        .unwrap();
        let crossing = Capsule::new(
            segment(Vec3::new(-2.0, 2.0, 0.0), Vec3::new(2.0, 2.0, 0.0)),
            0.25,
        )
        .unwrap();
        assert!(capsule.overlaps(&touching));
        assert!(!capsule.overlaps(&separated));
        assert!(capsule.overlaps(&crossing));
    }

    #[test]
    fn overlaps_sphere_uses_the_summed_radii() {
        let capsule = body();
        assert!(capsule.overlaps_sphere(&Sphere::new(Vec3::new(2.0, 2.0, 0.0), 1.0).unwrap()));
        assert!(!capsule.overlaps_sphere(&Sphere::new(Vec3::new(2.5, 2.0, 0.0), 1.0).unwrap()));
        // Beyond the end of the axis, the cap still reaches.
        assert!(capsule.overlaps_sphere(&Sphere::new(Vec3::new(0.0, 5.0, 0.0), 1.0).unwrap()));
    }

    #[test]
    fn overlaps_triangle_answers_resting_separated_and_pierced() {
        let floor = Triangle::new(
            Vec3::new(-4.0, 0.0, -4.0),
            Vec3::new(-4.0, 0.0, 4.0),
            Vec3::new(4.0, 0.0, 0.0),
        )
        .unwrap();
        // Axis one radius above the face: touching.
        assert!(Capsule::new(
            segment(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 3.0, 0.0)),
            1.0
        )
        .unwrap()
        .overlaps_triangle(&floor));
        // Axis two radii above the face: clear.
        assert!(!Capsule::new(
            segment(Vec3::new(0.0, 2.0, 0.0), Vec3::new(0.0, 4.0, 0.0)),
            1.0
        )
        .unwrap()
        .overlaps_triangle(&floor));
        // Axis straight through the face, both endpoints far from every edge:
        // only the crossing term can see this one.
        assert!(Capsule::new(
            segment(Vec3::new(0.0, -3.0, 0.0), Vec3::new(0.0, 3.0, 0.0)),
            0.1
        )
        .unwrap()
        .overlaps_triangle(&floor));
        // Beside the triangle entirely.
        assert!(!Capsule::new(
            segment(Vec3::new(9.0, -3.0, 0.0), Vec3::new(9.0, 3.0, 0.0)),
            0.5
        )
        .unwrap()
        .overlaps_triangle(&floor));
    }

    #[test]
    fn approx_eq_compares_axis_and_radius() {
        let capsule = body();
        assert!(capsule.approx_eq(&capsule, eps()));
        let fatter = Capsule::new(capsule.segment(), 2.0).unwrap();
        let moved = Capsule::new(segment(Vec3::ZERO, Vec3::UNIT_Y), capsule.radius()).unwrap();
        assert!(!capsule.approx_eq(&fatter, eps()));
        assert!(!capsule.approx_eq(&moved, eps()));
    }
}
