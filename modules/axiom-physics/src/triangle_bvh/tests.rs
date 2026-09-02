//! Tests for [`super::TriangleBvh`].
//!
//! A child module rather than an inline one: the tree and its tests together
//! ran past the repo's 1000-line file budget, and this is the seam that splits
//! them without moving a line of production code. `super` still resolves to
//! `triangle_bvh`, so nothing about what the tests can reach has changed.

use super::{BvhHit, TriangleBvh, LEAF_SIZE};
use axiom_math::{DSegment, DVec3};

/// Deterministic pseudo-random `f32`s in `[-1, 1)`.
fn noise(n: usize) -> Vec<f32> {
    let mut s = 0x1234_5678_9abc_def0_u64;
    (0..n)
        .map(|_| {
            s = s
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((s >> 40) as f32 / (1u32 << 24) as f32) * 2.0 - 1.0
        })
        .collect()
}

/// `rows * cols` quads on the XZ plane, two triangles each, with a little
/// vertical jitter so the centroids actually spread on all three axes.
fn terrain(rows: usize, cols: usize) -> Vec<f32> {
    let jitter = noise(rows * cols);
    let mut out = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            let (x0, z0) = (c as f32, r as f32);
            let (x1, z1) = (x0 + 1.0, z0 + 1.0);
            let y = jitter[r * cols + c] * 0.25;
            out.extend([x0, y, z0, x1, y, z0, x1, y, z1]);
            out.extend([x0, y, z0, x1, y, z1, x0, y, z1]);
        }
    }
    out
}

/// The answer, computed the slow way: test every triangle, keep the nearest.
fn brute_force(
    tree: &TriangleBvh,
    origin: DVec3,
    direction: DVec3,
    max_distance: f64,
    accept: &impl Fn(u32) -> bool,
) -> Option<BvhHit> {
    (0..tree.triangle_count() as u32)
        .filter(|t| accept(*t))
        .filter_map(|t| tree.intersect(t, origin, direction))
        .filter(|h| h.distance < max_distance)
        .fold(None, |best: Option<BvhHit>, h| {
            match best {
                Some(b) if b.distance <= h.distance => Some(b),
                _ => Some(h),
            }
        })
}

// =================================================================
// Structure
// =================================================================

#[test]
fn an_empty_soup_builds_an_empty_tree_and_hits_nothing() {
    let tree = TriangleBvh::build(&[]);
    assert_eq!(tree.triangle_count(), 0);
    assert_eq!(tree.node_count, 0);
    assert_eq!(tree.max_depth, 0);
    assert_eq!(
        tree.raycast(DVec3::ZERO, DVec3::new(0.0, 0.0, 1.0), 100.0, |_| true),
        None
    );
}

/// A buffer that is not a whole number of triangles keeps the whole ones.
#[test]
fn a_truncated_buffer_keeps_the_triangles_that_are_complete() {
    let mut soup = terrain(1, 1);
    soup.truncate(soup.len() - 4);
    assert_eq!(TriangleBvh::build(&soup).triangle_count(), 1);
}

#[test]
fn a_soup_that_fits_one_leaf_is_a_single_node() {
    let tree = TriangleBvh::build(&terrain(1, 2));
    assert!(tree.triangle_count() <= LEAF_SIZE);
    assert_eq!(tree.node_count, 1);
    assert_eq!(tree.max_depth, 0);
}

#[test]
fn a_larger_soup_actually_splits() {
    let tree = TriangleBvh::build(&terrain(8, 8));
    assert_eq!(tree.triangle_count(), 128);
    assert!(tree.node_count > 1, "{} node(s)", tree.node_count);
    assert!(tree.max_depth > 0);
}

/// **The invariant a broken partition breaks.** Every triangle must appear
/// in exactly one leaf, and the leaves must tile the whole index range with
/// no gap and no overlap.
#[test]
fn the_leaves_tile_every_triangle_exactly_once() {
    let tree = TriangleBvh::build(&terrain(9, 7));
    let total = tree.triangle_count();

    let mut seen = vec![0u32; total];
    let mut covered = 0usize;
    (0..tree.node_count).for_each(|n| {
        let count = tree.node_meta[n * 2 + 1];
        let first = tree.node_meta[n * 2] as usize;
        if count > 0 {
            covered += count as usize;
            tree.order[first..first + count as usize]
                .iter()
                .for_each(|&t| seen[t as usize] += 1);
        }
    });

    assert_eq!(covered, total, "leaf runs do not cover every slot");
    assert!(
        seen.iter().all(|&n| n == 1),
        "every triangle should appear once; got {seen:?}"
    );
}

#[test]
fn the_order_is_a_permutation_of_the_soup() {
    let tree = TriangleBvh::build(&terrain(6, 6));
    let mut sorted = (&tree.order).to_vec();
    sorted.sort_unstable();
    assert_eq!(sorted, (0..tree.triangle_count() as u32).collect::<Vec<_>>());
}

/// A node's bounds must contain its triangles, or traversal will cull a
/// triangle the ray genuinely hits.
#[test]
fn every_node_encloses_the_triangles_beneath_it() {
    let tree = TriangleBvh::build(&terrain(7, 5));
    (0..tree.node_count).for_each(|n| {
        let count = tree.node_meta[n * 2 + 1];
        if count > 0 {
            let first = tree.node_meta[n * 2] as usize;
            let o = n * 6;
            tree.order[first..first + count as usize].iter().for_each(|&t| {
                let b = tree.triangle_box(t);
                (0..3).for_each(|k| {
                    assert!(
                        f64::from(tree.node_bounds[o + k]) <= b[k],
                        "node {n} min[{k}] excludes triangle {t}"
                    );
                    assert!(
                        f64::from(tree.node_bounds[o + 3 + k]) >= b[k + 3],
                        "node {n} max[{k}] excludes triangle {t}"
                    );
                });
            });
        }
    });
}

#[test]
fn the_bounds_enclose_the_whole_soup() {
    let tree = TriangleBvh::build(&terrain(4, 4));
    let b = tree.bounds();
    assert!(b.min.x <= 0.0 && b.max.x >= 4.0, "{b:?}");
    assert!(b.min.z <= 0.0 && b.max.z >= 4.0, "{b:?}");
}

#[test]
fn building_the_same_soup_twice_gives_the_same_tree() {
    let soup = terrain(6, 5);
    assert_eq!(TriangleBvh::build(&soup), TriangleBvh::build(&soup));
}

// =================================================================
// Queries — checked against the slow answer
// =================================================================

/// **The test that matters.** A tree can be wrong in ways no structural
/// assertion catches — bounds a hair too tight, a partition that loses a
/// triangle to the wrong side, a traversal that prunes too eagerly — and
/// every one of them shows up as a ray that misses something it should hit.
/// So: many rays, from many angles, compared against testing every triangle.
#[test]
fn every_ray_agrees_with_testing_every_triangle() {
    let tree = TriangleBvh::build(&terrain(9, 9));
    let r = noise(6 * 400);
    let mut checked = 0usize;
    let mut hits = 0usize;

    for c in r.chunks_exact(6) {
        let origin = DVec3::new(
            f64::from(c[0]) * 6.0 + 4.0,
            f64::from(c[1]) * 4.0 + 3.0,
            f64::from(c[2]) * 6.0 + 4.0,
        );
        // `normalize_or_zero` handles a degenerate direction by returning
        // zero, and a zero direction is a legitimate thing to ask about — so
        // there is no guard here to skip one. The sampler does not produce
        // one anyway, which is exactly why a guard would be dead code.
        let direction =
            DVec3::new(f64::from(c[3]), f64::from(c[4]) - 0.5, f64::from(c[5]))
                .normalize_or_zero();

        let fast = tree.raycast(origin, direction, 50.0, |_| true);
        let slow = brute_force(&tree, origin, direction, 50.0, &|_| true);
        assert_eq!(fast, slow, "origin {origin:?} direction {direction:?}");
        checked += 1;
        hits += usize::from(fast.is_some());
    }

    assert!(checked > 300, "only checked {checked} rays");
    assert!(hits > 30, "only {hits} of {checked} rays hit anything — test is not exercising the tree");
}

/// The predicate has to be applied during traversal, not after: a rejected
/// triangle must not shorten the ray for whatever is behind it.
#[test]
fn a_filtered_ray_finds_what_is_behind_the_rejected_triangle() {
    // Two stacked triangles; the ray points down through both.
    let soup = [
        0.0, 2.0, 0.0, 2.0, 2.0, 0.0, 0.0, 2.0, 2.0, //
        0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 2.0,
    ];
    let tree = TriangleBvh::build(&soup);
    let origin = DVec3::new(0.4, 5.0, 0.4);
    let down = DVec3::new(0.0, -1.0, 0.0);

    let all = tree.raycast(origin, down, 20.0, |_| true).expect("hits the top");
    assert!((all.distance - 3.0).abs() < 1e-9, "{all:?}");

    let lower = tree
        .raycast(origin, down, 20.0, |t| t != all.triangle)
        .expect("hits the one below");
    assert!((lower.distance - 5.0).abs() < 1e-9, "{lower:?}");
    assert_ne!(lower.triangle, all.triangle);
}

#[test]
fn rejecting_everything_finds_nothing() {
    let tree = TriangleBvh::build(&terrain(4, 4));
    let hit = tree.raycast(
        DVec3::new(1.5, 5.0, 1.5),
        DVec3::new(0.0, -1.0, 0.0),
        20.0,
        |_| false,
    );
    assert_eq!(hit, None);
}

#[test]
fn a_ray_pointing_away_from_the_soup_misses() {
    let tree = TriangleBvh::build(&terrain(4, 4));
    let hit = tree.raycast(
        DVec3::new(1.5, 5.0, 1.5),
        DVec3::new(0.0, 1.0, 0.0),
        20.0,
        |_| true,
    );
    assert_eq!(hit, None);
}

#[test]
fn a_ray_that_stops_short_of_the_surface_misses() {
    let tree = TriangleBvh::build(&terrain(4, 4));
    let origin = DVec3::new(1.5, 5.0, 1.5);
    let down = DVec3::new(0.0, -1.0, 0.0);
    assert!(tree.raycast(origin, down, 20.0, |_| true).is_some());
    assert_eq!(tree.raycast(origin, down, 1.0, |_| true), None);
}

/// A zero component in the direction must not produce a NaN in the slab
/// test — an axis-aligned ray is the common case, not an edge case.
#[test]
fn an_axis_aligned_ray_is_not_defeated_by_a_zero_component() {
    let tree = TriangleBvh::build(&terrain(5, 5));
    let hit = tree.raycast(
        DVec3::new(2.5, 8.0, 2.5),
        DVec3::new(0.0, -1.0, 0.0),
        50.0,
        |_| true,
    );
    assert!(hit.is_some(), "straight down should hit the ground");
}

#[test]
fn front_and_back_faces_are_distinguished() {
    let soup = [0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 2.0];
    let tree = TriangleBvh::build(&soup);
    let above = tree
        .raycast(DVec3::new(0.4, 1.0, 0.4), DVec3::new(0.0, -1.0, 0.0), 5.0, |_| true)
        .expect("from above");
    let below = tree
        .raycast(DVec3::new(0.4, -1.0, 0.4), DVec3::new(0.0, 1.0, 0.0), 5.0, |_| true)
        .expect("from below");
    assert_ne!(above.front_face, below.front_face);
}

#[test]
fn a_ray_parallel_to_a_triangle_does_not_hit_it() {
    let soup = [0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 2.0];
    let tree = TriangleBvh::build(&soup);
    let hit = tree.raycast(
        DVec3::new(-1.0, 0.0, 0.5),
        DVec3::new(1.0, 0.0, 0.0),
        10.0,
        |_| true,
    );
    assert_eq!(hit, None, "coplanar ray");
}

// =================================================================
// Degenerate geometry
// =================================================================

/// Every centroid at one point: no split separates anything, so the node
/// stays a leaf however many triangles it holds.
#[test]
fn a_cluster_with_no_centroid_spread_stays_one_leaf() {
    let one = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
    let soup: Vec<f32> = (0..20).flat_map(|_| one).collect();
    let tree = TriangleBvh::build(&soup);
    assert_eq!(tree.triangle_count(), 20);
    assert_eq!(tree.node_count, 1, "identical centroids cannot be split");
}

/// A zero-area triangle has no normal to normalise; it gets a unit stand-in
/// rather than a NaN, so a caller reading it back gets something usable.
// =================================================================
// Capsule overlap
// =================================================================

fn segment(a: [f64; 3], b: [f64; 3]) -> DSegment {
    DSegment {
        start: DVec3::from_array(a),
        end: DVec3::from_array(b),
    }
}

/// One triangle on the XZ plane, wound so its normal points **up**.
///
/// The winding is the point: `contact_normal` falls back to the face normal
/// for a deep contact, so a floor wound the other way reports contacts that
/// push a capsule down through it. Getting this backwards in an early draft
/// of these tests is what surfaced that.
fn up_facing_floor(size: f32) -> [f32; 9] {
    [0.0, 0.0, 0.0, 0.0, 0.0, size, size, 0.0, 0.0]
}

#[test]
fn a_capsule_clear_of_the_soup_does_not_overlap_it() {
    let tree = TriangleBvh::build(&terrain(4, 4));
    // Well above the ground.
    assert!(!tree.overlaps_capsule(segment([2.0, 5.0, 2.0], [2.0, 6.0, 2.0]), 0.5));
    // Beside it.
    assert!(!tree.overlaps_capsule(segment([-9.0, 0.0, -9.0], [-9.0, 1.0, -9.0]), 0.5));
}

#[test]
fn a_capsule_resting_on_the_ground_overlaps_it() {
    let tree = TriangleBvh::build(&terrain(4, 4));
    assert!(tree.overlaps_capsule(segment([2.0, 0.4, 2.0], [2.0, 1.4, 2.0]), 0.5));
}

/// The contact a character controller pushes out along: a normal pointing
/// out of the surface towards the capsule, a point on the triangle, and a
/// depth equal to how far the capsule has sunk.
#[test]
fn a_contact_reports_depth_point_and_an_outward_normal() {
    let tree = TriangleBvh::build(&up_facing_floor(4.0));

    // A vertical capsule of radius 1 whose axis runs 0.25 above the face, so
    // its surface has sunk 0.75 into it.
    let contact = tree
        .contacts_capsule(segment([0.5, 0.25, 0.5], [0.5, 2.25, 0.5]), 1.0)
        .expect("the capsule reaches the face");

    assert!((contact.depth - 0.75).abs() < 1e-9, "{contact:?}");
    assert!((contact.point.y).abs() < 1e-9, "point should be on the face: {contact:?}");
    assert!(contact.normal.y > 0.99, "normal should point up: {contact:?}");
}

/// A capsule that has sunk past the face must still be pushed *out*, not
/// further in. The closest-point direction flips once the axis crosses the
/// surface, which is why the face normal takes over.
#[test]
fn a_capsule_sunk_past_the_face_still_gets_an_outward_normal() {
    let tree = TriangleBvh::build(&up_facing_floor(4.0));

    // The axis straddles the face, so the closest point on it is below.
    let contact = tree
        .contacts_capsule(segment([0.5, -0.5, 0.5], [0.5, 1.5, 0.5]), 1.0)
        .expect("deeply overlapping");
    assert!(
        contact.normal.y > 0.99,
        "a deep contact must still push up, got {contact:?}"
    );
}

#[test]
fn the_deepest_contact_is_the_one_reported() {
    // Two floors, one 0.5 below the other; a tall capsule reaches both.
    let soup = [
        0.0, 0.0, 0.0, 0.0, 0.0, 4.0, 4.0, 0.0, 0.0, //
        0.0, -0.5, 0.0, 0.0, -0.5, 4.0, 4.0, -0.5, 0.0,
    ];
    let tree = TriangleBvh::build(&soup);
    let contact = tree
        .contacts_capsule(segment([0.5, 0.6, 0.5], [0.5, 2.6, 0.5]), 1.0)
        .expect("reaches both");
    // The lower face is further away, so the capsule is *less* deep in it;
    // the upper face is the deeper contact.
    assert!((contact.depth - 0.4).abs() < 1e-9, "{contact:?}");
}

// =================================================================
// Capsule sweep
// =================================================================

#[test]
fn a_capsule_driven_into_the_ground_stops_on_it() {
    let tree = TriangleBvh::build(&up_facing_floor(4.0));

    // Lower end 2 above the face, radius 1, so it has 1 to travel.
    let hit = tree
        .sweep_capsule(
            segment([0.5, 3.0, 0.5], [0.5, 5.0, 0.5]),
            1.0,
            DVec3::new(0.0, -4.0, 0.0),
        )
        .expect("it should land");
    assert!((hit.distance - 2.0).abs() < 1e-3, "{hit:?}");
    assert!(hit.normal.y > 0.9, "{hit:?}");
}

/// **The one that stalls a controller if it is wrong.** A capsule already
/// resting on the floor, moved sideways, is not blocked by the floor: it
/// slides. A sweep that reported a zero-distance hit here would freeze
/// anything standing on anything.
#[test]
fn a_capsule_resting_on_the_floor_slides_along_it_rather_than_being_blocked() {
    let soup = [-4.0, 0.0, -4.0, -4.0, 0.0, 4.0, 4.0, 0.0, -4.0];
    let tree = TriangleBvh::build(&soup);

    // Exactly touching: the axis's lower end sits one radius above the face.
    let resting = segment([1.0, 1.0, -1.0], [1.0, 3.0, -1.0]);
    assert!(tree.overlaps_capsule(resting, 1.0 + 1e-9), "it is in contact");
    assert_eq!(
        tree.sweep_capsule(resting, 1.0, DVec3::new(1.0, 0.0, 0.0)),
        None,
        "sliding along a surface is not blocked by it"
    );
}

#[test]
fn a_capsule_moving_away_from_the_soup_is_not_blocked() {
    let tree = TriangleBvh::build(&up_facing_floor(4.0));
    assert_eq!(
        tree.sweep_capsule(
            segment([0.5, 2.0, 0.5], [0.5, 4.0, 0.5]),
            1.0,
            DVec3::new(0.0, 5.0, 0.0),
        ),
        None
    );
}

#[test]
fn a_capsule_stopping_short_of_the_soup_is_not_blocked() {
    let tree = TriangleBvh::build(&up_facing_floor(4.0));
    let axis = segment([0.5, 8.0, 0.5], [0.5, 10.0, 0.5]);
    assert!(tree.sweep_capsule(axis, 1.0, DVec3::new(0.0, -9.0, 0.0)).is_some());
    assert_eq!(tree.sweep_capsule(axis, 1.0, DVec3::new(0.0, -2.0, 0.0)), None);
}

#[test]
fn a_capsule_that_does_not_move_is_never_blocked() {
    let tree = TriangleBvh::build(&terrain(3, 3));
    assert_eq!(
        tree.sweep_capsule(segment([1.0, 0.0, 1.0], [1.0, 2.0, 1.0]), 1.0, DVec3::ZERO),
        None,
        "a zero motion cannot meet anything on the way"
    );
}

#[test]
fn the_nearest_triangle_stops_the_sweep() {
    // Two parallel walls; the capsule travels towards both.
    // Wound so each wall's normal points back down -X, towards the capsule.
    let soup = [
        2.0, -2.0, -2.0, 2.0, -2.0, 2.0, 2.0, 2.0, -2.0, //
        5.0, -2.0, -2.0, 5.0, -2.0, 2.0, 5.0, 2.0, -2.0,
    ];
    let tree = TriangleBvh::build(&soup);
    let hit = tree
        .sweep_capsule(
            segment([-2.0, 0.0, 0.0], [-2.0, 0.5, 0.0]),
            0.5,
            DVec3::new(10.0, 0.0, 0.0),
        )
        .expect("it meets the nearer wall");
    // 2 - 0.5(radius) - (-2)(start) = 3.5
    assert!((hit.distance - 3.5).abs() < 1e-3, "{hit:?}");
}

#[test]
fn an_empty_soup_blocks_nothing_and_overlaps_nothing() {
    let tree = TriangleBvh::build(&[]);
    let axis = segment([0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    assert!(!tree.overlaps_capsule(axis, 1.0));
    assert_eq!(tree.sweep_capsule(axis, 1.0, DVec3::new(0.0, -1.0, 0.0)), None);
    assert_eq!(tree.contacts_capsule(axis, 1.0), None);
}

#[test]
fn a_degenerate_triangle_gets_a_unit_normal_rather_than_a_nan() {
    let soup = [1.0, 2.0, 3.0, 1.0, 2.0, 3.0, 1.0, 2.0, 3.0];
    let tree = TriangleBvh::build(&soup);
    let n = tree.normal(0);
    assert!(n.x.is_finite() && n.y.is_finite() && n.z.is_finite(), "{n:?}");
    assert!((n.length() - 1.0).abs() < 1e-9, "{n:?}");
}

#[test]
fn a_triangle_reads_back_the_corners_it_was_given() {
    let soup = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0];
    let [a, b, c] = TriangleBvh::build(&soup).triangle(0);
    assert_eq!((a.x, a.y, a.z), (1.0, 2.0, 3.0));
    assert_eq!((b.x, b.y, b.z), (4.0, 5.0, 6.0));
    assert_eq!((c.x, c.y, c.z), (7.0, 8.0, 9.0));
}
