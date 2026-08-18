//! An oriented bounding box.

use crate::approx_eq::ApproxEq;
use crate::epsilon::Epsilon;
use crate::hit::Hit;
use crate::math_error::MathError;
use crate::math_result::MathResult;
use crate::quat::Quat;
use crate::ray::Ray;
use crate::vec3::Vec3;

/// Below this the direction component counts as parallel to a slab, matching
/// the tolerance [`Ray::intersect_aabb`] uses for the same decision.
const PARALLEL_DIRECTION: f32 = 1.0e-20;

/// The three local axes a slab index selects between.
const LOCAL_AXES: [Vec3; 3] = [Vec3::UNIT_X, Vec3::UNIT_Y, Vec3::UNIT_Z];

/// A box of `half_extents` about `center`, turned by `orientation`.
///
/// This is [`crate::Aabb`] freed from the world axes: the shape a per-bone
/// hitbox, a turned crate, or any oriented volume needs. `orientation` is stored
/// as a unit quaternion, so its conjugate is its inverse and the world-to-local
/// transform costs nothing to build.
#[derive(Debug, Clone, Copy)]
pub struct Obb {
    center: Vec3,
    half_extents: Vec3,
    orientation: Quat,
}

impl Obb {
    /// Construct from a finite center, finite non-negative half extents, and a
    /// non-zero orientation, which is normalized on the way in.
    pub fn new(center: Vec3, half_extents: Vec3, orientation: Quat) -> MathResult<Obb> {
        let finite = [center, half_extents]
            .into_iter()
            .flat_map(|v| [v.x, v.y, v.z])
            .all(|component| component.is_finite());
        let non_negative =
            (half_extents.x >= 0.0) & (half_extents.y >= 0.0) & (half_extents.z >= 0.0);
        (!finite)
            .then_some(Err(MathError::non_finite_scalar(
                "Obb center and half extents must be finite",
            )))
            .or_else(|| {
                (finite & !non_negative).then_some(Err(MathError::invalid_aabb_bounds(
                    "Obb half extents must be non-negative",
                )))
            })
            .unwrap_or_else(|| {
                orientation.normalize().map(|orientation| Obb {
                    center,
                    half_extents,
                    orientation,
                })
            })
    }

    /// The box's center.
    pub const fn center(&self) -> Vec3 {
        self.center
    }

    /// Half the box's size along each of its own axes.
    pub const fn half_extents(&self) -> Vec3 {
        self.half_extents
    }

    /// The unit rotation from the box's own axes to world axes.
    pub const fn orientation(&self) -> Quat {
        self.orientation
    }

    /// `p` expressed in the box's own frame, where the box is the axis-aligned
    /// span `[-half_extents, half_extents]`.
    pub fn to_local(&self, p: Vec3) -> Vec3 {
        self.orientation.conjugate().rotate(p.subtract(self.center))
    }

    /// Inclusive point containment.
    pub fn contains_point(&self, p: Vec3) -> bool {
        let local = self.to_local(p);
        (local.x.abs() <= self.half_extents.x)
            & (local.y.abs() <= self.half_extents.y)
            & (local.z.abs() <= self.half_extents.z)
    }

    /// Where `ray` first enters this box, with [`Hit::time`] a distance along
    /// the ray and [`Hit::normal`] the outward normal of the face it entered
    /// through.
    ///
    /// The ray is carried into the box's own frame, where the box is
    /// axis-aligned and the standard three-slab fold applies; the fold carries
    /// which slab last raised the entry distance, which is exactly the face that
    /// was struck. A ray that starts inside enters at distance `0` — the same
    /// convention as [`Ray::intersect_aabb_entry`] — and still names the face it
    /// came in through.
    pub fn raycast(&self, ray: &Ray) -> Option<Hit> {
        let local = self.orientation.conjugate();
        let origin = local.rotate(ray.origin().subtract(self.center));
        let direction = local.rotate(ray.direction());
        self.slab_fold(
            [origin.x, origin.y, origin.z],
            [direction.x, direction.y, direction.z],
        )
        .filter(|(_, far, _, _)| *far >= 0.0)
        .map(|(near, _, axis, facing)| {
            let t = near.max(0.0);
            Hit::new(
                t,
                ray.point_at(t),
                self.orientation.rotate(LOCAL_AXES[axis].mul_scalar(facing)),
            )
        })
    }

    /// Fold the three slabs of the local box, carrying the entry distance, the
    /// exit distance, and the axis and sign of the face that set the entry.
    /// A slab the ray runs parallel to cannot set the entry and cannot tighten
    /// the exit; it can only reject the cast outright, by lying outside it.
    fn slab_fold(&self, origins: [f32; 3], directions: [f32; 3]) -> Option<(f32, f32, usize, f32)> {
        let extents = [
            self.half_extents.x,
            self.half_extents.y,
            self.half_extents.z,
        ];
        (0..3)
            .try_fold(
                (f32::NEG_INFINITY, f32::INFINITY, 0_usize, 1.0_f32),
                |(near, far, axis, facing), i| {
                    let parallel = directions[i].abs() < PARALLEL_DIRECTION;
                    let outside = (origins[i] < -extents[i]) | (origins[i] > extents[i]);
                    let inverse = 1.0 / directions[i];
                    let low = (-extents[i] - origins[i]) * inverse;
                    let high = (extents[i] - origins[i]) * inverse;
                    let entering = !parallel & (low.min(high) > near);
                    let next = (
                        [near, low.min(high)][usize::from(entering)],
                        [far, far.min(low.max(high))][usize::from(!parallel)],
                        [axis, i][usize::from(entering)],
                        [facing, -directions[i].signum()][usize::from(entering)],
                    );
                    let miss = (parallel & outside) | (next.0 > next.1);
                    [Ok(next), Err(())][usize::from(miss)]
                },
            )
            .ok()
    }
}

impl ApproxEq for Obb {
    fn approx_eq(&self, other: &Self, epsilon: Epsilon) -> bool {
        self.center.approx_eq(&other.center, epsilon)
            & self.half_extents.approx_eq(&other.half_extents, epsilon)
            & self.orientation.approx_eq(&other.orientation, epsilon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math_error_code::MathErrorCode;
    use std::f32::consts::FRAC_PI_2;

    fn eps() -> Epsilon {
        Epsilon::new(1.0e-5).unwrap()
    }

    /// A 2x2x2 box at the origin, unrotated.
    fn cube() -> Obb {
        Obb::new(Vec3::ZERO, Vec3::ONE, Quat::IDENTITY).unwrap()
    }

    /// The same box turned a quarter turn about +Y, stretched so the turn shows.
    fn turned_slab() -> Obb {
        Obb::new(
            Vec3::ZERO,
            Vec3::new(4.0, 1.0, 1.0),
            Quat::from_axis_angle(Vec3::UNIT_Y, FRAC_PI_2).unwrap(),
        )
        .unwrap()
    }

    fn ray(origin: Vec3, direction: Vec3) -> Ray {
        Ray::new(origin, direction).unwrap()
    }

    #[test]
    fn new_rejects_non_finite_and_negative_and_unorientable_boxes() {
        assert_eq!(
            Obb::new(Vec3::new(f32::NAN, 0.0, 0.0), Vec3::ONE, Quat::IDENTITY)
                .unwrap_err()
                .code(),
            MathErrorCode::NonFiniteScalar
        );
        assert_eq!(
            Obb::new(Vec3::ZERO, Vec3::new(1.0, -1.0, 1.0), Quat::IDENTITY)
                .unwrap_err()
                .code(),
            MathErrorCode::InvalidAabbBounds
        );
        assert_eq!(
            Obb::new(Vec3::ZERO, Vec3::ONE, Quat::new(0.0, 0.0, 0.0, 0.0))
                .unwrap_err()
                .code(),
            MathErrorCode::NormalizeZeroLength
        );
    }

    #[test]
    fn accessors_report_center_extents_and_orientation() {
        let box_ = Obb::new(Vec3::new(1.0, 2.0, 3.0), Vec3::ONE, Quat::IDENTITY).unwrap();
        assert!(box_.center().approx_eq(&Vec3::new(1.0, 2.0, 3.0), eps()));
        assert!(box_.half_extents().approx_eq(&Vec3::ONE, eps()));
        assert!(box_.orientation().approx_eq(&Quat::IDENTITY, eps()));
    }

    #[test]
    fn to_local_undoes_the_center_and_the_turn() {
        // A quarter turn about +Y carries local +Z onto world +X, so world +X
        // reads back as local +Z.
        let local = turned_slab().to_local(Vec3::new(4.0, 0.0, 0.0));
        assert!(local.approx_eq(&Vec3::new(0.0, 0.0, 4.0), eps()));
    }

    #[test]
    fn contains_point_respects_the_turn() {
        let slab = turned_slab();
        // The long axis points along world -Z after the turn.
        assert!(slab.contains_point(Vec3::new(0.0, 0.0, 3.0)));
        assert!(!slab.contains_point(Vec3::new(3.0, 0.0, 0.0)));
        assert!(slab.contains_point(Vec3::new(0.5, 0.0, 0.0)));
        assert!(cube().contains_point(Vec3::ONE));
        assert!(!cube().contains_point(Vec3::new(1.0, 1.0, 1.01)));
    }

    #[test]
    fn ray_into_a_face_reports_distance_point_and_outward_normal() {
        let hit = cube()
            .raycast(&ray(Vec3::new(-5.0, 0.0, 0.0), Vec3::UNIT_X))
            .unwrap();
        assert!(hit.time().approx_eq(&4.0, eps()));
        assert!(hit.point().approx_eq(&Vec3::new(-1.0, 0.0, 0.0), eps()));
        assert!(hit.normal().approx_eq(&Vec3::new(-1.0, 0.0, 0.0), eps()));
    }

    #[test]
    fn ray_into_a_turned_face_reports_the_turned_normal() {
        // The slab is long along world Z, so a ray along -Z strikes its short
        // +Z end cap, whose outward normal is world +Z.
        let hit = turned_slab()
            .raycast(&ray(Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.0, 0.0, -1.0)))
            .unwrap();
        assert!(hit.time().approx_eq(&6.0, eps()));
        assert!(hit.point().approx_eq(&Vec3::new(0.0, 0.0, 4.0), eps()));
        assert!(hit.normal().approx_eq(&Vec3::UNIT_Z, eps()));
    }

    #[test]
    fn ray_striking_the_top_reports_the_top_normal() {
        let hit = cube()
            .raycast(&ray(Vec3::new(0.0, 5.0, 0.0), Vec3::new(0.0, -1.0, 0.0)))
            .unwrap();
        assert!(hit.time().approx_eq(&4.0, eps()));
        assert!(hit.normal().approx_eq(&Vec3::UNIT_Y, eps()));
    }

    #[test]
    fn ray_missing_the_box_returns_nothing() {
        let box_ = cube();
        assert!(box_
            .raycast(&ray(Vec3::new(-5.0, 5.0, 0.0), Vec3::UNIT_X))
            .is_none());
        assert!(box_
            .raycast(&ray(Vec3::new(-5.0, 0.0, 0.0), Vec3::new(-1.0, 0.0, 0.0)))
            .is_none());
    }

    #[test]
    fn ray_parallel_to_a_slab_hits_only_from_within_it() {
        let box_ = cube();
        // Parallel to X, inside the Y and Z slabs: a hit.
        assert!(box_
            .raycast(&ray(Vec3::new(0.0, 0.5, -5.0), Vec3::UNIT_Z))
            .is_some());
        // Parallel to X, outside the Y slab: a miss the parallel arm rejects.
        assert!(box_
            .raycast(&ray(Vec3::new(0.0, 5.0, -5.0), Vec3::UNIT_Z))
            .is_none());
        assert!(box_
            .raycast(&ray(Vec3::new(0.0, -5.0, -5.0), Vec3::UNIT_Z))
            .is_none());
    }

    #[test]
    fn ray_grazing_a_corner_still_hits() {
        let hit = cube()
            .raycast(&ray(Vec3::new(-3.0, -3.0, 0.0), Vec3::new(1.0, 1.0, 0.0)))
            .unwrap();
        assert!(hit.point().approx_eq(&Vec3::new(-1.0, -1.0, 0.0), eps()));
    }

    #[test]
    fn ray_starting_inside_enters_immediately_through_the_face_behind_it() {
        let hit = cube().raycast(&ray(Vec3::ZERO, Vec3::UNIT_X)).unwrap();
        assert_eq!(hit.time(), 0.0);
        assert!(hit.point().approx_eq(&Vec3::ZERO, eps()));
        assert!(hit.normal().approx_eq(&Vec3::new(-1.0, 0.0, 0.0), eps()));
    }

    #[test]
    fn a_flat_box_is_still_hit_on_its_plane() {
        let sheet = Obb::new(Vec3::ZERO, Vec3::new(2.0, 0.0, 2.0), Quat::IDENTITY).unwrap();
        let hit = sheet
            .raycast(&ray(Vec3::new(0.0, 3.0, 0.0), Vec3::new(0.0, -1.0, 0.0)))
            .unwrap();
        assert!(hit.time().approx_eq(&3.0, eps()));
        assert!(hit.normal().approx_eq(&Vec3::UNIT_Y, eps()));
    }

    #[test]
    fn approx_eq_compares_center_extents_and_orientation() {
        let box_ = cube();
        assert!(box_.approx_eq(&box_, eps()));
        assert!(!box_.approx_eq(
            &Obb::new(Vec3::UNIT_X, Vec3::ONE, Quat::IDENTITY).unwrap(),
            eps()
        ));
        assert!(!box_.approx_eq(
            &Obb::new(Vec3::ZERO, Vec3::new(2.0, 1.0, 1.0), Quat::IDENTITY).unwrap(),
            eps()
        ));
        assert!(!box_.approx_eq(&turned_slab(), eps()));
    }
}
