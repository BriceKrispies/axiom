//! A 2D polygon in the XY plane — the cross-section every constructive
//! operator consumes.

use axiom_kernel::{Meters, Radians};
use axiom_math::Vec2;
use axiom_mesh::{MeshError, MeshErrorCode, MeshResult};

use crate::tessellation::Segments;

/// How close two consecutive profile points may be before they count as
/// duplicates. Duplicate points break ear clipping and produce zero-area
/// side quads, so they are rejected at the boundary rather than silently
/// tolerated.
pub const PROFILE_EPSILON: f32 = 1.0e-6;

/// Which way a closed profile's points wind in the XY plane.
///
/// Reported rather than enforced: extrusion and revolution need to know the
/// winding to emit front-facing triangles, and forcing every caller to
/// pre-orient its polygon would be a worse contract than measuring it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileWinding {
    /// Positive signed area — the standard orientation, front face toward `+Z`.
    CounterClockwise,
    /// Negative signed area.
    Clockwise,
}

/// A polyline or polygon in the XY plane.
///
/// A **closed** profile implies a final edge from the last point back to the
/// first and must enclose area; an **open** profile is a strip with two
/// distinct ends. Both are validated on construction: finite points, no
/// duplicate neighbours, enough points to be meaningful.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    points: Vec<Vec2>,
    closed: bool,
}

impl Profile {
    /// A closed polygon. Needs at least 3 points and non-zero area.
    pub fn closed(points: Vec<Vec2>) -> MeshResult<Profile> {
        validate(&points, true).map(|()| Profile {
            points,
            closed: true,
        })
    }

    /// An open polyline. Needs at least 2 points.
    pub fn open(points: Vec<Vec2>) -> MeshResult<Profile> {
        validate(&points, false).map(|()| Profile {
            points,
            closed: false,
        })
    }

    /// A closed regular polygon inscribed in `radius`, starting at angle zero
    /// and winding counter-clockwise.
    ///
    /// This is the profile behind cylinders, cones, capsules, tori, and discs,
    /// so it lives here rather than being re-derived in each generator.
    pub fn circle(radius: Meters, segments: Segments) -> MeshResult<Profile> {
        (radius.get() > 0.0)
            .then_some(())
            .ok_or_else(|| {
                MeshError::new(
                    MeshErrorCode::InvalidParameter,
                    "a circle profile needs a strictly positive radius",
                )
            })
            .and_then(|()| {
                let n = segments.get();
                let step = core::f32::consts::TAU / n as f32;
                Profile::closed(
                    (0..n)
                        .map(|i| {
                            let a = step * i as f32;
                            Vec2::new(radius.get() * a.cos(), radius.get() * a.sin())
                        })
                        .collect(),
                )
            })
    }

    /// A closed axis-aligned rectangle centred on the origin, wound
    /// counter-clockwise.
    pub fn rectangle(half_width: Meters, half_height: Meters) -> MeshResult<Profile> {
        let (w, h) = (half_width.get(), half_height.get());
        ((w > 0.0) & (h > 0.0))
            .then_some(())
            .ok_or_else(|| {
                MeshError::new(
                    MeshErrorCode::InvalidParameter,
                    "a rectangle profile needs strictly positive half-extents",
                )
            })
            .and_then(|()| {
                Profile::closed(vec![
                    Vec2::new(-w, -h),
                    Vec2::new(w, -h),
                    Vec2::new(w, h),
                    Vec2::new(-w, h),
                ])
            })
    }

    /// The profile points, in order.
    pub fn points(&self) -> &[Vec2] {
        &self.points
    }

    /// Whether a closing edge joins the last point to the first.
    pub const fn is_closed(&self) -> bool {
        self.closed
    }

    /// The number of points.
    pub fn point_count(&self) -> usize {
        self.points.len()
    }

    /// The number of edges: one per point when closed, one fewer when open.
    pub fn edge_count(&self) -> usize {
        self.points.len() - usize::from(!self.closed)
    }

    /// Which way the points wind. An open profile is reported by the winding of
    /// its implied closing edge, which is what a sweep cap needs.
    pub fn winding(&self) -> ProfileWinding {
        [ProfileWinding::Clockwise, ProfileWinding::CounterClockwise]
            [usize::from(self.signed_area() >= 0.0)]
    }

    /// Twice-the-shoelace signed area. Positive is counter-clockwise.
    ///
    /// Crate-private: the *value* is an implementation detail of triangulation
    /// and cap orientation, while the *orientation* is the public fact.
    pub(crate) fn signed_area(&self) -> f32 {
        let n = self.points.len();
        self.points
            .iter()
            .zip(self.points.iter().cycle().skip(1))
            .take(n)
            .map(|(a, b)| a.x * b.y - b.x * a.y)
            .sum::<f32>()
            * 0.5
    }

    /// The same profile with its point order reversed, flipping its winding.
    pub fn reversed(&self) -> Profile {
        Profile {
            points: self.points.iter().rev().copied().collect(),
            closed: self.closed,
        }
    }

    /// The same profile rotated about the origin in its own plane.
    pub fn rotated(&self, angle: Radians) -> Profile {
        let (s, c) = (angle.get().sin(), angle.get().cos());
        Profile {
            points: self
                .points
                .iter()
                .map(|p| Vec2::new(p.x * c - p.y * s, p.x * s + p.y * c))
                .collect(),
            closed: self.closed,
        }
    }

    /// The same profile scaled uniformly about the origin.
    pub fn scaled(&self, factor: Meters) -> Profile {
        Profile {
            points: self.points.iter().map(|p| p.mul_scalar(factor.get())).collect(),
            closed: self.closed,
        }
    }
}

fn invalid(message: &'static str) -> MeshError {
    MeshError::new(MeshErrorCode::InvalidProfile, message)
}

/// Enough points, all finite, no duplicate neighbours, and (closed only)
/// non-zero enclosed area.
fn validate(points: &[Vec2], closed: bool) -> MeshResult<()> {
    let minimum = 2 + usize::from(closed);
    (points.len() >= minimum)
        .then_some(())
        .ok_or_else(|| invalid("a closed profile needs >= 3 points, an open profile >= 2"))
        .and_then(|()| {
            points
                .iter()
                .all(|p| p.x.is_finite() & p.y.is_finite())
                .then_some(())
                .ok_or_else(|| invalid("every profile point must be finite"))
        })
        .and_then(|()| {
            let neighbours_distinct = points
                .windows(2)
                .all(|w| w[0].distance(w[1]) > PROFILE_EPSILON);
            // A closed profile additionally must not have its last point sitting
            // on its first — that would make the closing edge degenerate.
            let wrap_distinct = (!closed)
                | points
                    .first()
                    .zip(points.last())
                    .map_or(false, |(a, b)| a.distance(*b) > PROFILE_EPSILON);
            (neighbours_distinct & wrap_distinct)
                .then_some(())
                .ok_or_else(|| invalid("profile points must not repeat consecutively"))
        })
        .and_then(|()| {
            let area = shoelace(points);
            (!closed | (area.abs() > PROFILE_EPSILON))
                .then_some(())
                .ok_or_else(|| invalid("a closed profile must enclose a non-zero area"))
        })
}

fn shoelace(points: &[Vec2]) -> f32 {
    let n = points.len();
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(n)
        .map(|(a, b)| a.x * b.y - b.x * a.y)
        .sum::<f32>()
        * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Vec<Vec2> {
        vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(0.0, 1.0),
        ]
    }

    #[test]
    fn a_closed_square_validates_and_reports_its_shape() {
        let p = Profile::closed(square()).unwrap();
        assert!(p.is_closed());
        assert_eq!(p.point_count(), 4);
        assert_eq!(p.edge_count(), 4);
        assert_eq!(p.winding(), ProfileWinding::CounterClockwise);
        assert!((p.signed_area() - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn an_open_polyline_has_one_fewer_edge_than_points() {
        let p = Profile::open(vec![Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(2.0, 1.0)]).unwrap();
        assert!(!p.is_closed());
        assert_eq!(p.point_count(), 3);
        assert_eq!(p.edge_count(), 2);
    }

    #[test]
    fn reversing_flips_the_winding() {
        let p = Profile::closed(square()).unwrap();
        let r = p.reversed();
        assert_eq!(r.winding(), ProfileWinding::Clockwise);
        assert!((r.signed_area() + 1.0).abs() < 1.0e-6);
        assert_eq!(r.reversed().points(), p.points());
    }

    #[test]
    fn too_few_points_are_rejected() {
        assert_eq!(
            Profile::closed(vec![Vec2::ZERO, Vec2::new(1.0, 0.0)])
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidProfile
        );
        assert_eq!(
            Profile::open(vec![Vec2::ZERO]).unwrap_err().code(),
            MeshErrorCode::InvalidProfile
        );
    }

    #[test]
    fn non_finite_points_are_rejected() {
        assert_eq!(
            Profile::open(vec![Vec2::new(f32::NAN, 0.0), Vec2::new(1.0, 0.0)])
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidProfile
        );
    }

    #[test]
    fn duplicate_consecutive_points_are_rejected() {
        assert_eq!(
            Profile::open(vec![Vec2::ZERO, Vec2::ZERO, Vec2::new(1.0, 0.0)])
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidProfile
        );
    }

    #[test]
    fn a_closed_profile_whose_last_point_repeats_its_first_is_rejected() {
        assert_eq!(
            Profile::closed(vec![
                Vec2::ZERO,
                Vec2::new(1.0, 0.0),
                Vec2::new(1.0, 1.0),
                Vec2::ZERO,
            ])
            .unwrap_err()
            .code(),
            MeshErrorCode::InvalidProfile
        );
    }

    #[test]
    fn a_zero_area_closed_profile_is_rejected() {
        // Three collinear points enclose nothing.
        assert_eq!(
            Profile::closed(vec![
                Vec2::ZERO,
                Vec2::new(1.0, 0.0),
                Vec2::new(2.0, 0.0)
            ])
            .unwrap_err()
            .code(),
            MeshErrorCode::InvalidProfile
        );
    }

    #[test]
    fn an_open_profile_may_be_collinear() {
        let p = Profile::open(vec![Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(2.0, 0.0)]);
        assert!(p.is_ok());
    }

    #[test]
    fn a_circle_profile_is_counter_clockwise_and_inscribed() {
        let p = Profile::circle(Meters::new(2.0).unwrap(), Segments::new(8).unwrap()).unwrap();
        assert_eq!(p.point_count(), 8);
        assert_eq!(p.winding(), ProfileWinding::CounterClockwise);
        assert!(p.points().iter().all(|q| (q.length() - 2.0).abs() < 1.0e-5));
        assert_eq!(p.points()[0], Vec2::new(2.0, 0.0));
    }

    #[test]
    fn a_circle_needs_a_positive_radius() {
        assert_eq!(
            Profile::circle(Meters::new(0.0).unwrap(), Segments::new(8).unwrap())
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidParameter
        );
    }

    #[test]
    fn a_rectangle_profile_is_centred_and_counter_clockwise() {
        let p =
            Profile::rectangle(Meters::new(2.0).unwrap(), Meters::new(1.0).unwrap()).unwrap();
        assert_eq!(p.point_count(), 4);
        assert_eq!(p.winding(), ProfileWinding::CounterClockwise);
        assert!((p.signed_area() - 8.0).abs() < 1.0e-5);
    }

    #[test]
    fn a_rectangle_needs_positive_half_extents() {
        assert_eq!(
            Profile::rectangle(Meters::new(0.0).unwrap(), Meters::new(1.0).unwrap())
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidParameter
        );
        assert_eq!(
            Profile::rectangle(Meters::new(1.0).unwrap(), Meters::new(-1.0).unwrap())
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidParameter
        );
    }

    #[test]
    fn rotating_preserves_area_and_moves_points() {
        let p = Profile::rectangle(Meters::new(2.0).unwrap(), Meters::new(1.0).unwrap()).unwrap();
        let r = p.rotated(Radians::new(core::f32::consts::FRAC_PI_2).unwrap());
        assert!((r.signed_area() - p.signed_area()).abs() < 1.0e-4);
        assert!((r.points()[0].x - 1.0).abs() < 1.0e-5);
        assert!((r.points()[0].y + 2.0).abs() < 1.0e-5);
    }

    #[test]
    fn scaling_scales_area_quadratically() {
        let p = Profile::closed(square()).unwrap();
        let s = p.scaled(Meters::new(3.0).unwrap());
        assert!((s.signed_area() - 9.0).abs() < 1.0e-5);
        assert!(s.is_closed());
    }
}
