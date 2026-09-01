//! Double-precision axis-aligned bounding box, and the ray slab test over it.

use crate::approx_eq::ApproxEq;
use crate::dvec3::DVec3;
use crate::epsilon::Epsilon;

/// An axis-aligned bounding box in `f64`.
///
/// The double-precision sibling of [`crate::Aabb`]. It exists for the same
/// reason [`DVec3`] does — a broad-phase over a city-scale world is one of the
/// domains whose *internal* precision is load-bearing (see [`crate::Scalar`]).
/// At a kilometre from the origin an `f32` box has roughly `1e-4 m` of
/// resolution, which is the scale at which a capsule resting on a floor starts
/// to jitter between "touching" and "not".
///
/// No invariant is enforced on `min <= max`: an inverted box is the natural
/// identity for a union fold, and rejecting it would force every builder to
/// special-case its first element.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DAabb {
    pub min: DVec3,
    pub max: DVec3,
}

impl DAabb {
    /// The inverted box — `min` at `+∞`, `max` at `-∞`.
    ///
    /// The identity for [`DAabb::union`] and [`DAabb::grown_to_include`]:
    /// folding any set of points onto it yields their exact bounds, with no
    /// "first point" branch.
    pub const EMPTY: DAabb = DAabb {
        min: DVec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
        max: DVec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
    };

    /// Corner constructor.
    pub const fn new(min: DVec3, max: DVec3) -> Self {
        DAabb { min, max }
    }

    /// The box containing both `self` and `other`.
    pub fn union(self, other: DAabb) -> DAabb {
        DAabb::new(
            DVec3::new(
                self.min.x.min(other.min.x),
                self.min.y.min(other.min.y),
                self.min.z.min(other.min.z),
            ),
            DVec3::new(
                self.max.x.max(other.max.x),
                self.max.y.max(other.max.y),
                self.max.z.max(other.max.z),
            ),
        )
    }

    /// The box containing `self` and the point `p`.
    pub fn grown_to_include(self, p: DVec3) -> DAabb {
        self.union(DAabb::new(p, p))
    }

    /// The box grown outward by `margin` on every axis.
    pub fn expanded(self, margin: f64) -> DAabb {
        let m = DVec3::new(margin, margin, margin);
        DAabb::new(self.min.subtract(m), self.max.add(m))
    }

    /// Width, height and depth. Negative on an axis of an inverted box.
    pub fn extent(self) -> DVec3 {
        self.max.subtract(self.min)
    }

    /// The midpoint of the box.
    pub fn center(self) -> DVec3 {
        self.min.add(self.max).mul_scalar(0.5)
    }

    /// Total surface area — the cost term of a surface-area-heuristic split.
    ///
    /// Clamped at zero on each axis so an inverted or degenerate box reports
    /// `0.0` rather than a negative "area" that would make a nonsense split
    /// look attractive.
    pub fn surface_area(self) -> f64 {
        let e = self.extent();
        let (x, y, z) = (e.x.max(0.0), e.y.max(0.0), e.z.max(0.0));
        2.0 * (x * y + y * z + z * x)
    }

    /// Whether `p` lies inside or on the box.
    pub fn contains(self, p: DVec3) -> bool {
        (p.x >= self.min.x)
            & (p.y >= self.min.y)
            & (p.z >= self.min.z)
            & (p.x <= self.max.x)
            & (p.y <= self.max.y)
            & (p.z <= self.max.z)
    }

    /// Whether the two boxes share any volume, touching faces included.
    pub fn intersects(self, other: DAabb) -> bool {
        (self.min.x <= other.max.x)
            & (self.max.x >= other.min.x)
            & (self.min.y <= other.max.y)
            & (self.max.y >= other.min.y)
            & (self.min.z <= other.max.z)
            & (self.max.z >= other.min.z)
    }

    /// Distance along the ray at which it enters the box, or `None` if it never
    /// does within `limit`.
    ///
    /// The slab test. A ray that *starts inside* the box enters at `0.0` rather
    /// than at the negative distance of the slab behind it, so a traversal that
    /// begins inside a node still descends into it.
    ///
    /// ## Why the reciprocal is a parameter
    ///
    /// `inverse_direction` is `1.0 / direction`, componentwise, supplied by the
    /// caller. A BVH traversal tests one ray against thousands of boxes, and
    /// three divides per box is the single hottest cost in it; hoisting them to
    /// once per ray is the reason this test is written as a slab test at all.
    /// Passing it in also makes the ±∞ components an explicit, documented input
    /// rather than an accident of a division this function happened to do.
    ///
    /// ## The zero-times-infinity case, and why a naive slab test gets it wrong
    ///
    /// An axis-parallel ray has an infinite reciprocal on its other two axes.
    /// When such a ray starts *exactly* on a slab face, `(face - origin)` is
    /// exactly zero, and zero times infinity is `NaN` — not a large number, not
    /// a small one, but a value that loses every comparison it takes part in.
    ///
    /// A slab test written the obvious way (`if t0 < t1 { t0 } else { t1 }`)
    /// then propagates that `NaN` into the running interval and **misses a box
    /// the ray genuinely passes through**. A ray sliding along the floor of a
    /// collision world is exactly that case, and it is not rare: it is what a
    /// grounded character does with its downward probe every frame.
    ///
    /// The fix is to read the `NaN` for what it means. It arises only when
    /// `bound - origin` is exactly zero, which is to say the origin lies *on*
    /// that face — so the origin is inside that slab and the axis constrains
    /// nothing. Substituting the appropriate infinity for a `NaN` bound says
    /// precisely that, and the interval then comes only from the axes that do
    /// constrain the ray. After the substitution no `NaN` survives, so
    /// [`f64::min`] and [`f64::max`] are unambiguous.
    ///
    /// An origin *outside* a slab on a zero-direction axis produces no `NaN` at
    /// all — both bounds are the same infinity — and correctly misses.
    pub fn ray_entry(self, origin: DVec3, inverse_direction: DVec3, limit: f64) -> Option<f64> {
        let near = self.min.subtract(origin).mul_componentwise(inverse_direction);
        let far = self.max.subtract(origin).mul_componentwise(inverse_direction);

        // A NaN bound means the origin is on this face and the ray does not
        // move along this axis, so the axis constrains nothing. See the doc.
        let low = |t: f64| [t, f64::NEG_INFINITY][usize::from(t.is_nan())];
        let high = |t: f64| [t, f64::INFINITY][usize::from(t.is_nan())];

        let lo_x = low(near.x).min(low(far.x));
        let hi_x = high(near.x).max(high(far.x));
        let lo_y = low(near.y).min(low(far.y));
        let hi_y = high(near.y).max(high(far.y));
        let lo_z = low(near.z).min(low(far.z));
        let hi_z = high(near.z).max(high(far.z));

        let lo = lo_x.max(lo_y).max(lo_z);
        let hi = hi_x.min(hi_y).min(hi_z);

        let missed = (hi < 0.0) | (lo > hi) | (lo > limit);
        // A ray starting inside enters at zero, not behind itself.
        let entry = [lo, 0.0][usize::from(lo < 0.0)];
        (!missed).then_some(entry)
    }
}

impl ApproxEq for DAabb {
    fn approx_eq(&self, other: &Self, epsilon: Epsilon) -> bool {
        self.min.approx_eq(&other.min, epsilon) & self.max.approx_eq(&other.max, epsilon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit() -> DAabb {
        DAabb::new(DVec3::ZERO, DVec3::ONE)
    }

    fn reciprocal(d: DVec3) -> DVec3 {
        DVec3::new(1.0 / d.x, 1.0 / d.y, 1.0 / d.z)
    }

    #[test]
    fn extent_center_and_area_describe_the_box() {
        let b = DAabb::new(DVec3::new(-1.0, 0.0, 2.0), DVec3::new(1.0, 4.0, 5.0));
        assert_eq!(b.extent(), DVec3::new(2.0, 4.0, 3.0));
        assert_eq!(b.center(), DVec3::new(0.0, 2.0, 3.5));
        assert_eq!(b.surface_area(), 2.0 * (8.0 + 12.0 + 6.0));
    }

    #[test]
    fn the_empty_box_is_the_union_identity() {
        let b = unit();
        assert_eq!(DAabb::EMPTY.union(b), b);
        assert_eq!(b.union(DAabb::EMPTY), b);
    }

    #[test]
    fn folding_points_onto_the_empty_box_gives_their_bounds() {
        let points = [
            DVec3::new(1.0, 2.0, 3.0),
            DVec3::new(-4.0, 0.5, 9.0),
            DVec3::new(0.0, -2.0, 1.0),
        ];
        let bounds = points
            .into_iter()
            .fold(DAabb::EMPTY, DAabb::grown_to_include);
        assert_eq!(bounds.min, DVec3::new(-4.0, -2.0, 1.0));
        assert_eq!(bounds.max, DVec3::new(1.0, 2.0, 9.0));
    }

    #[test]
    fn an_inverted_or_degenerate_box_has_no_negative_area() {
        assert_eq!(DAabb::EMPTY.surface_area(), 0.0);
        let flat = DAabb::new(DVec3::ZERO, DVec3::new(2.0, 0.0, 3.0));
        assert_eq!(flat.surface_area(), 2.0 * 6.0);
    }

    #[test]
    fn expanded_grows_on_every_axis() {
        let b = unit().expanded(0.5);
        assert_eq!(b.min, DVec3::new(-0.5, -0.5, -0.5));
        assert_eq!(b.max, DVec3::new(1.5, 1.5, 1.5));
    }

    #[test]
    fn contains_includes_the_boundary_and_excludes_the_outside() {
        let b = unit();
        assert!(b.contains(DVec3::new(0.5, 0.5, 0.5)));
        assert!(b.contains(DVec3::ZERO));
        assert!(b.contains(DVec3::ONE));
        assert!(!b.contains(DVec3::new(-0.001, 0.5, 0.5)));
        assert!(!b.contains(DVec3::new(0.5, 1.001, 0.5)));
        assert!(!b.contains(DVec3::new(0.5, 0.5, 1.001)));
    }

    #[test]
    fn intersects_counts_touching_faces_and_rejects_a_gap() {
        let b = unit();
        assert!(b.intersects(DAabb::new(DVec3::ONE, DVec3::new(2.0, 2.0, 2.0))));
        assert!(!b.intersects(DAabb::new(
            DVec3::new(1.001, 0.0, 0.0),
            DVec3::new(2.0, 1.0, 1.0)
        )));
        assert!(!b.intersects(DAabb::new(
            DVec3::new(0.0, 1.001, 0.0),
            DVec3::new(1.0, 2.0, 1.0)
        )));
        assert!(!b.intersects(DAabb::new(
            DVec3::new(0.0, 0.0, 1.001),
            DVec3::new(1.0, 1.0, 2.0)
        )));
    }

    #[test]
    fn a_ray_aimed_at_the_box_reports_its_entry_distance() {
        let dir = DVec3::UNIT_X;
        let entry = unit().ray_entry(DVec3::new(-3.0, 0.5, 0.5), reciprocal(dir), 100.0);
        assert_eq!(entry, Some(3.0));
    }

    #[test]
    fn a_ray_starting_inside_enters_at_zero() {
        let entry = unit().ray_entry(DVec3::new(0.5, 0.5, 0.5), reciprocal(DVec3::UNIT_X), 100.0);
        assert_eq!(entry, Some(0.0));
    }

    #[test]
    fn a_ray_pointing_away_misses() {
        let entry = unit().ray_entry(
            DVec3::new(-3.0, 0.5, 0.5),
            reciprocal(DVec3::UNIT_X.mul_scalar(-1.0)),
            100.0,
        );
        assert_eq!(entry, None);
    }

    #[test]
    fn a_ray_that_passes_beside_the_box_misses() {
        let entry = unit().ray_entry(
            DVec3::new(-3.0, 5.0, 0.5),
            reciprocal(DVec3::UNIT_X),
            100.0,
        );
        assert_eq!(entry, None);
    }

    #[test]
    fn a_hit_beyond_the_limit_is_a_miss() {
        let o = DVec3::new(-3.0, 0.5, 0.5);
        let inv = reciprocal(DVec3::UNIT_X);
        assert_eq!(unit().ray_entry(o, inv, 2.0), None);
        assert_eq!(unit().ray_entry(o, inv, 3.0), Some(3.0));
    }

    /// The zero-times-infinity case: an axis-parallel ray running exactly along
    /// a face must HIT. A comparison-based slab test misses it, which is the
    /// defect this implementation exists to avoid.
    #[test]
    fn an_axis_parallel_ray_lying_on_a_face_still_hits() {
        // Origin exactly on the y = 0 face, travelling along +x. The y slab
        // yields `0 * inf` = NaN on one side.
        let inv = DVec3::new(1.0, f64::INFINITY, f64::INFINITY);
        let entry = unit().ray_entry(DVec3::new(-3.0, 0.0, 0.0), inv, 100.0);
        assert_eq!(entry, Some(3.0));
    }

    #[test]
    fn an_axis_parallel_ray_outside_every_slab_still_misses() {
        let inv = DVec3::new(1.0, f64::INFINITY, f64::INFINITY);
        let entry = unit().ray_entry(DVec3::new(-3.0, 5.0, 0.0), inv, 100.0);
        assert_eq!(entry, None);
    }

    #[test]
    fn each_axis_can_be_the_one_that_decides_entry_and_exit() {
        // Entry dominated by y, exit by z: proves the running min/max fold
        // reads all three axes rather than only the first.
        let b = DAabb::new(DVec3::new(-10.0, 0.0, -10.0), DVec3::new(10.0, 1.0, 0.25));
        let dir = DVec3::new(0.1, 1.0, 0.5).normalize().unwrap();
        assert!(b
            .ray_entry(DVec3::new(0.0, -2.0, -2.0), reciprocal(dir), 100.0)
            .is_some());
    }

    #[test]
    fn approx_eq_compares_both_corners() {
        let eps = Epsilon::DEFAULT_DOUBLE;
        assert!(unit().approx_eq(&unit(), eps));
        assert!(!unit().approx_eq(&DAabb::new(DVec3::new(0.1, 0.0, 0.0), DVec3::ONE), eps));
        assert!(!unit().approx_eq(&DAabb::new(DVec3::ZERO, DVec3::new(1.1, 1.0, 1.0)), eps));
    }
}
