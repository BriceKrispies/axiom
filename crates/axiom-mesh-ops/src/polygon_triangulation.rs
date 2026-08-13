//! Ear-clipping triangulation of a closed 2D profile polygon.
//!
//! The one place in the layer that turns an outline into faces. Caps, lofted
//! end sections, and revolved lids all reduce to "triangulate this polygon",
//! so the algorithm lives here once instead of being re-derived per operator.
//!
//! # What it handles
//!
//! Any **simple** polygon — convex or concave, wound either way. The input
//! winding is normalised internally, so the emitted triangles are always
//! counter-clockwise in the XY plane (front-facing toward `+Z`) whatever the
//! caller handed in. Holes are *not* supported: a hole needs a bridged outline,
//! which is a different data contract than [`Profile`].
//!
//! # Why the clip is bounded rather than iterative
//!
//! A simple polygon of `n` points clips in exactly `n - 2` steps — one ear per
//! step, one triangle per ear. That makes the whole algorithm a `try_fold` over
//! a fixed range rather than a `while` over a shrinking list: the remaining
//! vertex ring is the fold accumulator, and a step that cannot find an ear
//! yields the error that ends the fold. The polygon being non-simple
//! (self-intersecting) is exactly the condition "a full pass found no ear", so
//! the failure is detected rather than looped on forever.

use axiom_math::Vec2;
use axiom_mesh::{MeshError, MeshErrorCode, MeshResult};

use crate::profile::{Profile, ProfileWinding};

/// Triangulate a closed profile polygon by ear clipping.
///
/// Returns `n - 2` triangles for an `n`-point polygon. Each triangle names
/// **original** profile point indices (the internal re-orientation is not
/// visible in the output) and is wound **counter-clockwise in XY**, so a cap
/// built from these indices at a constant `z` faces `+Z`.
///
/// # Errors
///
/// - [`MeshErrorCode::InvalidProfile`] — the profile is open. An open polyline
///   encloses no area, so there is nothing to triangulate.
/// - [`MeshErrorCode::TriangulationFailed`] — a full pass over the remaining
///   vertices found no ear, which for a polygon with at least three remaining
///   points means the outline is self-intersecting (not simple).
pub fn triangulate_profile(profile: &Profile) -> MeshResult<Vec<[u32; 3]>> {
    profile
        .is_closed()
        .then_some(())
        .ok_or_else(|| {
            MeshError::new(
                MeshErrorCode::InvalidProfile,
                "triangulation needs a closed profile; an open polyline encloses no area",
            )
        })
        .and_then(|()| clip_ears(profile.points(), counter_clockwise_order(profile)))
}

/// The profile's point indices in counter-clockwise order.
///
/// Ear clipping's convexity test is orientation-dependent, so the ring is
/// normalised once here instead of every predicate carrying a sign. Reversing
/// the *index* ring (rather than the points) is what keeps the emitted
/// triangles addressing the caller's original point order.
fn counter_clockwise_order(profile: &Profile) -> Vec<u32> {
    let n = profile.point_count() as u32;
    matches!(profile.winding(), ProfileWinding::CounterClockwise)
        .then_some(())
        .map_or_else(|| (0..n).rev().collect(), |()| (0..n).collect())
}

/// Clip `order.len() - 2` ears, one per step, or report the step that stalled.
fn clip_ears(points: &[Vec2], order: Vec<u32>) -> MeshResult<Vec<[u32; 3]>> {
    let steps = order.len() - 2;
    (0..steps)
        .try_fold(
            (order, Vec::with_capacity(steps)),
            |(remaining, triangles), _| clip_one_ear(points, remaining, triangles),
        )
        .map(|(_, triangles)| triangles)
}

/// One clipping step: find an ear, emit its triangle, drop its tip.
fn clip_one_ear(
    points: &[Vec2],
    remaining: Vec<u32>,
    mut triangles: Vec<[u32; 3]>,
) -> MeshResult<(Vec<u32>, Vec<[u32; 3]>)> {
    find_ear(points, &remaining).map(|tip| {
        let n = remaining.len();
        triangles.push([
            remaining[(tip + n - 1) % n],
            remaining[tip],
            remaining[(tip + 1) % n],
        ]);
        let mut rest = remaining;
        rest.remove(tip);
        (rest, triangles)
    })
}

/// The position (within `remaining`) of the first ear, scanning from zero.
///
/// Scanning in ring order rather than picking a "best" ear is what makes the
/// output deterministic: the same polygon always decomposes the same way.
///
/// Only **reflex** vertices are tested for containment. That is both cheaper
/// and more correct than testing every vertex: a convex vertex of a simple
/// polygon can never lie inside a candidate ear without a reflex vertex lying
/// there too, and testing convex vertices would reject legal ears whose edge
/// merely touches one.
fn find_ear(points: &[Vec2], remaining: &[u32]) -> MeshResult<usize> {
    let reflex: Vec<usize> = (0..remaining.len())
        .filter(|&i| corner_cross(points, remaining, i) < 0.0)
        .collect();
    (0..remaining.len())
        .find(|&i| is_ear(points, remaining, i, &reflex))
        .ok_or_else(|| {
            MeshError::new(
                MeshErrorCode::TriangulationFailed,
                "no ear remained: the profile outline is self-intersecting, not a simple polygon",
            )
        })
}

/// Whether the corner at `tip` is an ear: convex, and enclosing no other
/// reflex vertex of the remaining ring.
fn is_ear(points: &[Vec2], remaining: &[u32], tip: usize, reflex: &[usize]) -> bool {
    let n = remaining.len();
    let previous = (tip + n - 1) % n;
    let next = (tip + 1) % n;
    let (a, b, c) = corner(points, remaining, tip);
    let convex = cross(b.subtract(a), c.subtract(b)) > 0.0;
    let clear = reflex.iter().all(|&other| {
        ((other == previous) | (other == tip) | (other == next))
            | !point_in_triangle(points[remaining[other] as usize], a, b, c)
    });
    convex & clear
}

/// The three points of the corner at `tip`: previous, tip, next.
fn corner(points: &[Vec2], remaining: &[u32], tip: usize) -> (Vec2, Vec2, Vec2) {
    let n = remaining.len();
    (
        points[remaining[(tip + n - 1) % n] as usize],
        points[remaining[tip] as usize],
        points[remaining[(tip + 1) % n] as usize],
    )
}

/// The turn at a corner. Positive turns left (convex in a counter-clockwise
/// ring), negative turns right (reflex), zero is collinear — neither, so a
/// collinear vertex is never clipped as an ear and never blocks one.
fn corner_cross(points: &[Vec2], remaining: &[u32], tip: usize) -> f32 {
    let (a, b, c) = corner(points, remaining, tip);
    cross(b.subtract(a), c.subtract(b))
}

/// The 2D cross product (the z of the 3D cross of two XY vectors).
const fn cross(u: Vec2, v: Vec2) -> f32 {
    u.x * v.y - u.y * v.x
}

/// Whether `p` lies in triangle `(a, b, c)`, boundary included.
///
/// Barycentric sign consistency: `p` is inside exactly when it sits on the same
/// side of all three directed edges. A point on an edge produces a zero and is
/// counted as contained, which is the conservative answer for ear clipping —
/// an ear whose edge grazes a reflex vertex is rejected rather than emitted as
/// an overlapping sliver.
fn point_in_triangle(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> bool {
    let ab = cross(b.subtract(a), p.subtract(a));
    let bc = cross(c.subtract(b), p.subtract(b));
    let ca = cross(a.subtract(c), p.subtract(c));
    let any_negative = (ab < 0.0) | (bc < 0.0) | (ca < 0.0);
    let any_positive = (ab > 0.0) | (bc > 0.0) | (ca > 0.0);
    !(any_negative & any_positive)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Twice the signed area of a triangle of profile points.
    fn double_area(points: &[Vec2], t: [u32; 3]) -> f32 {
        let (a, b, c) = (
            points[t[0] as usize],
            points[t[1] as usize],
            points[t[2] as usize],
        );
        cross(b.subtract(a), c.subtract(a))
    }

    fn total_area(points: &[Vec2], triangles: &[[u32; 3]]) -> f32 {
        triangles
            .iter()
            .map(|t| double_area(points, *t) * 0.5)
            .sum()
    }

    fn ccw_square() -> Vec<Vec2> {
        vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(0.0, 1.0),
        ]
    }

    /// An L: a 2x2 square with its top-right quadrant removed. Six points, one
    /// reflex corner at (1,1), enclosed area 3.
    fn l_shape() -> Vec<Vec2> {
        vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(2.0, 0.0),
            Vec2::new(2.0, 1.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(1.0, 2.0),
            Vec2::new(0.0, 2.0),
        ]
    }

    #[test]
    fn a_counter_clockwise_square_becomes_two_triangles_covering_its_area() {
        let points = ccw_square();
        let profile = Profile::closed(points.clone()).unwrap();
        let triangles = triangulate_profile(&profile).unwrap();

        assert_eq!(triangles.len(), 2);
        assert!((total_area(&points, &triangles) - 1.0).abs() < 1.0e-6);
        for t in &triangles {
            assert!(double_area(&points, *t) > 0.0, "triangle {t:?} is not CCW");
        }
    }

    #[test]
    fn a_clockwise_square_still_yields_counter_clockwise_triangles() {
        let points: Vec<Vec2> = ccw_square().into_iter().rev().collect();
        let profile = Profile::closed(points.clone()).unwrap();
        assert_eq!(profile.winding(), ProfileWinding::Clockwise);

        let triangles = triangulate_profile(&profile).unwrap();
        assert_eq!(triangles.len(), 2);
        assert!((total_area(&points, &triangles) - 1.0).abs() < 1.0e-6);
        for t in &triangles {
            assert!(double_area(&points, *t) > 0.0, "triangle {t:?} is not CCW");
        }
    }

    #[test]
    fn indices_address_the_original_point_order() {
        let profile = Profile::closed(ccw_square()).unwrap();
        let triangles = triangulate_profile(&profile).unwrap();
        let mut used: Vec<u32> = triangles.iter().flat_map(|t| t.iter().copied()).collect();
        used.sort_unstable();
        used.dedup();
        // Every corner of the square participates, and no index is invented.
        assert_eq!(used, vec![0, 1, 2, 3]);
    }

    #[test]
    fn a_concave_l_shape_yields_four_positive_area_triangles() {
        let points = l_shape();
        let profile = Profile::closed(points.clone()).unwrap();
        let triangles = triangulate_profile(&profile).unwrap();

        assert_eq!(triangles.len(), 4);
        assert!((total_area(&points, &triangles) - 3.0).abs() < 1.0e-6);
        for t in &triangles {
            assert!(
                double_area(&points, *t) > 0.0,
                "triangle {t:?} has non-positive area"
            );
        }
        // The removed quadrant must not be covered: no triangle may contain a
        // point in the notch.
        let notch = Vec2::new(1.5, 1.5);
        for t in &triangles {
            let (a, b, c) = (
                points[t[0] as usize],
                points[t[1] as usize],
                points[t[2] as usize],
            );
            assert!(
                !point_in_triangle(notch, a, b, c),
                "triangle {t:?} covers the notch"
            );
        }
    }

    #[test]
    fn a_clockwise_l_shape_is_normalised_before_clipping() {
        let points: Vec<Vec2> = l_shape().into_iter().rev().collect();
        let profile = Profile::closed(points.clone()).unwrap();
        let triangles = triangulate_profile(&profile).unwrap();

        assert_eq!(triangles.len(), 4);
        assert!((total_area(&points, &triangles) - 3.0).abs() < 1.0e-6);
        for t in &triangles {
            assert!(double_area(&points, *t) > 0.0);
        }
    }

    #[test]
    fn triangulation_is_deterministic() {
        let profile = Profile::closed(l_shape()).unwrap();
        assert_eq!(
            triangulate_profile(&profile).unwrap(),
            triangulate_profile(&profile).unwrap()
        );
    }

    #[test]
    fn a_self_intersecting_bowtie_fails_as_untriangulatable() {
        // Edge (0,0)->(4,4) crosses edge (4,0)->(0,1) at (0.8, 0.8).
        let profile = Profile::closed(vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(4.0, 4.0),
            Vec2::new(4.0, 0.0),
            Vec2::new(0.0, 1.0),
        ])
        .unwrap();
        assert_eq!(
            triangulate_profile(&profile).unwrap_err().code(),
            MeshErrorCode::TriangulationFailed
        );
    }

    #[test]
    fn an_open_profile_cannot_be_triangulated() {
        let profile = Profile::open(vec![
            Vec2::ZERO,
            Vec2::new(1.0, 0.0),
            Vec2::new(1.0, 1.0),
        ])
        .unwrap();
        assert_eq!(
            triangulate_profile(&profile).unwrap_err().code(),
            MeshErrorCode::InvalidProfile
        );
    }

    #[test]
    fn a_many_sided_convex_polygon_yields_n_minus_two_triangles() {
        use axiom_kernel::Meters;

        use crate::tessellation::Segments;

        let profile =
            Profile::circle(Meters::new(1.0).unwrap(), Segments::new(12).unwrap()).unwrap();
        let triangles = triangulate_profile(&profile).unwrap();
        assert_eq!(triangles.len(), profile.point_count() - 2);
        // A regular 12-gon inscribed in radius 1 has area
        // (1/2) * 12 * sin(30 degrees) = 3.0.
        assert!((total_area(profile.points(), &triangles) - 3.0).abs() < 1.0e-4);
    }

    #[test]
    fn point_containment_uses_barycentric_sign_consistency() {
        let (a, b, c) = (Vec2::ZERO, Vec2::new(2.0, 0.0), Vec2::new(0.0, 2.0));
        assert!(point_in_triangle(Vec2::new(0.5, 0.5), a, b, c));
        // On an edge counts as contained.
        assert!(point_in_triangle(Vec2::new(1.0, 0.0), a, b, c));
        // A corner counts as contained.
        assert!(point_in_triangle(a, a, b, c));
        assert!(!point_in_triangle(Vec2::new(2.0, 2.0), a, b, c));
        assert!(!point_in_triangle(Vec2::new(-0.5, 0.5), a, b, c));
    }

    #[test]
    fn a_collinear_vertex_neither_clips_as_an_ear_nor_blocks_one() {
        // A square with a redundant midpoint on its bottom edge: 5 points,
        // 3 triangles, still exactly the square's area.
        let points = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(2.0, 0.0),
            Vec2::new(2.0, 2.0),
            Vec2::new(0.0, 2.0),
        ];
        let profile = Profile::closed(points.clone()).unwrap();
        let triangles = triangulate_profile(&profile).unwrap();
        assert_eq!(triangles.len(), 3);
        assert!((total_area(&points, &triangles) - 4.0).abs() < 1.0e-6);
    }
}
