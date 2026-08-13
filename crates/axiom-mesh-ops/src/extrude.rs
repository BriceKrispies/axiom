//! Linear extrusion: sweep a 2D profile along `+Z` into a real solid.
//!
//! # What it produces
//!
//! A **side wall per profile edge** (one quad, two triangles), plus optional
//! caps at the two ends. Walls are mandatory and are what makes this an
//! extrusion rather than two offset copies of the same outline: an extruded
//! square is a box with six faces, not two floating quads.
//!
//! # The two planes
//!
//! The profile lives at `z = 0` (the **start** plane); its sweep lands at
//! `z = distance` (the **end** plane). A negative distance is legal and sweeps
//! toward `-Z`; the geometry is still a solid with outward-facing triangles,
//! because the wall winding and the two cap normals are all selected from the
//! sign of the distance rather than assumed positive.
//!
//! [`CapPolicy::Start`] closes the profile plane, [`CapPolicy::End`] closes the
//! swept plane. Outward for the start cap is `-sign(distance) * Z`, outward for
//! the end cap is `+sign(distance) * Z` — for the ordinary positive sweep that
//! reads as "the back cap faces `-Z`, the front cap faces `+Z`".
//!
//! # Open profiles have no caps
//!
//! An open profile is a strip, not an outline: it encloses no area, so there is
//! nothing for a cap to close. **A cap request on an open profile is ignored**
//! rather than rejected — the caller asking for a capped sweep of a polyline
//! wants the ribbon, and refusing it would push a `is_closed()` test into every
//! call site for no gain.
//!
//! # Shading and parameterization
//!
//! Wall vertices are **duplicated per edge** and carry that edge's normal, so a
//! box's corners stay hard creases instead of being smoothed into a rounded
//! blob. Wall `u` is the cumulative perimeter distance normalised to `0..1` and
//! `v` runs `0` at the start plane to `1` at the end plane. Cap vertices carry
//! `±Z` and are parameterized by their XY position within the profile's
//! bounding box.

use axiom_kernel::Meters;
use axiom_math::{Vec2, Vec3};
use axiom_mesh::{Mesh, MeshError, MeshErrorCode, MeshResult, MeshStreams};

use crate::cap_policy::CapPolicy;
use crate::polygon_triangulation::triangulate_profile;
use crate::profile::{Profile, ProfileWinding, PROFILE_EPSILON};

/// Local vertex indices of a wall quad, ordered so each triple is
/// counter-clockwise seen from outside. Row 0 is a negative sweep, row 1 a
/// positive one; the quad's own corners are `0 = start-i`, `1 = start-j`,
/// `2 = end-j`, `3 = end-i`.
const WALL_WINDING: [[u32; 6]; 2] = [[0, 2, 1, 0, 3, 2], [0, 1, 2, 0, 2, 3]];

/// Whether a cap keeps the triangulator's counter-clockwise order (row 1) or
/// reverses it to face the other way (row 0).
const CAP_WINDING: [[usize; 3]; 2] = [[0, 2, 1], [0, 1, 2]];

/// One generated vertex, kept together so the three attribute streams cannot
/// drift out of correspondence while they are being built.
#[derive(Debug, Clone, Copy)]
struct Vertex {
    position: Vec3,
    normal: Vec3,
    uv: Vec2,
}

/// The min and max XY corner of a profile's bounding box.
#[derive(Debug, Clone, Copy)]
struct Bounds {
    min: Vec2,
    max: Vec2,
}

/// Extrude `profile` along `+Z` by `distance`, closing the ends per `caps`.
///
/// The result carries positions, indices, normals, and UVs. Walls are
/// flat-shaded (per-edge vertices, per-edge normals); caps carry `±Z`. See the
/// module documentation for the winding, cap, and UV conventions.
///
/// # Errors
///
/// - [`MeshErrorCode::InvalidParameter`] — `distance` is zero. A zero sweep has
///   no walls to build and would collapse the solid onto its own outline.
/// - [`MeshErrorCode::TriangulationFailed`] — a cap was requested on a closed
///   profile whose outline is self-intersecting, so it cannot be triangulated.
///   Requesting no caps on such a profile still yields the wall shell.
pub fn extrude(profile: &Profile, distance: Meters, caps: CapPolicy) -> MeshResult<Mesh> {
    validated_distance(distance)
        .and_then(|d| cap_triangles(profile, caps).map(|triangles| (d, triangles)))
        .and_then(|(d, triangles)| assemble(profile, d, caps, &triangles))
}

/// A sweep must have extent. `Meters` is finite by construction, so the only
/// rejectable value is zero.
fn validated_distance(distance: Meters) -> MeshResult<f32> {
    let d = distance.get();
    (d.is_finite() & (d != 0.0)).then_some(d).ok_or_else(|| {
        MeshError::new(
            MeshErrorCode::InvalidParameter,
            "an extrusion distance must be finite and non-zero",
        )
    })
}

/// `-1.0` for a sweep toward `-Z`, `+1.0` toward `+Z`.
fn sweep_sign(d: f32) -> f32 {
    [-1.0, 1.0][usize::from(d > 0.0)]
}

/// The cap decomposition, or an empty one when no cap is reachable.
///
/// Triangulating only when a cap will actually be used is what lets an
/// un-triangulatable outline still be extruded as an open shell.
fn cap_triangles(profile: &Profile, caps: CapPolicy) -> MeshResult<Vec<[u32; 3]>> {
    (profile.is_closed() & (caps.cap_count() > 0))
        .then(|| triangulate_profile(profile))
        .transpose()
        .map(Option::unwrap_or_default)
}

/// The profile's point indices in the order the walls travel.
///
/// A closed profile is normalised to counter-clockwise so "outward" is the
/// right-hand side of every edge. An open profile has no inside, so its given
/// order is its own definition of which side the walls face.
fn extrusion_ring(profile: &Profile) -> Vec<u32> {
    let n = profile.point_count() as u32;
    let reverse = profile.is_closed() & matches!(profile.winding(), ProfileWinding::Clockwise);
    reverse
        .then_some(())
        .map_or_else(|| (0..n).collect(), |()| (0..n).rev().collect())
}

/// Build every stream and hand them to the mesh contract for validation.
fn assemble(profile: &Profile, d: f32, caps: CapPolicy, cap_tris: &[[u32; 3]]) -> MeshResult<Mesh> {
    let points = profile.points();
    let ring = extrusion_ring(profile);
    let edges = ring.len() - usize::from(!profile.is_closed());
    let walls = wall_vertices(points, &ring, edges, d);

    let start_wanted = caps.caps_start() & profile.is_closed();
    let end_wanted = caps.caps_end() & profile.is_closed();
    let bounds = profile_bounds(points);
    let sign = sweep_sign(d);
    let start_cap = cap_vertices(points, 0.0, -sign, bounds, start_wanted);
    let end_cap = cap_vertices(points, d, sign, bounds, end_wanted);

    let start_base = walls.len() as u32;
    let end_base = start_base + profile.point_count() as u32 * u32::from(start_wanted);
    let indices: Vec<u32> = wall_indices(edges, d)
        .into_iter()
        .chain(cap_indices(cap_tris, start_base, d < 0.0, start_wanted))
        .chain(cap_indices(cap_tris, end_base, d > 0.0, end_wanted))
        .collect();

    let vertices: Vec<Vertex> = walls
        .into_iter()
        .chain(start_cap)
        .chain(end_cap)
        .collect();
    Mesh::from_streams(MeshStreams {
        normals: vertices.iter().map(|v| v.normal).collect(),
        uvs: vertices.iter().map(|v| v.uv).collect(),
        ..MeshStreams::new(vertices.iter().map(|v| v.position).collect(), indices)
    })
}

/// Four vertices per edge — no sharing, so adjacent walls crease.
fn wall_vertices(points: &[Vec2], ring: &[u32], edges: usize, d: f32) -> Vec<Vertex> {
    let lengths: Vec<f32> = (0..edges)
        .map(|k| {
            let (a, b) = edge_points(points, ring, k);
            b.subtract(a).length()
        })
        .collect();
    let total = lengths.iter().sum::<f32>().max(PROFILE_EPSILON);
    let offsets: Vec<f32> = lengths
        .iter()
        .scan(0.0_f32, |travelled, length| {
            let start = *travelled;
            *travelled += *length;
            Some(start)
        })
        .collect();
    (0..edges)
        .flat_map(|k| {
            edge_quad(
                points,
                ring,
                k,
                (offsets[k] / total, (offsets[k] + lengths[k]) / total),
                d,
            )
        })
        .collect()
}

/// The two profile points spanning wall edge `k`, in ring order.
fn edge_points(points: &[Vec2], ring: &[u32], k: usize) -> (Vec2, Vec2) {
    (
        points[ring[k] as usize],
        points[ring[(k + 1) % ring.len()] as usize],
    )
}

/// One wall quad's four vertices: start plane first, then end plane, all
/// carrying the edge's own outward normal.
fn edge_quad(points: &[Vec2], ring: &[u32], k: usize, u: (f32, f32), d: f32) -> [Vertex; 4] {
    let (a, b) = edge_points(points, ring, k);
    let edge = b.subtract(a);
    // Outward is the right-hand side of a counter-clockwise ring. Consecutive
    // profile points are validated distinct, so the clamp only guards against
    // an f32 underflow, never a real zero-length edge.
    let inverse = 1.0 / edge.length().max(PROFILE_EPSILON);
    let normal = Vec3::new(edge.y * inverse, -edge.x * inverse, 0.0);
    [
        Vertex {
            position: Vec3::new(a.x, a.y, 0.0),
            normal,
            uv: Vec2::new(u.0, 0.0),
        },
        Vertex {
            position: Vec3::new(b.x, b.y, 0.0),
            normal,
            uv: Vec2::new(u.1, 0.0),
        },
        Vertex {
            position: Vec3::new(b.x, b.y, d),
            normal,
            uv: Vec2::new(u.1, 1.0),
        },
        Vertex {
            position: Vec3::new(a.x, a.y, d),
            normal,
            uv: Vec2::new(u.0, 1.0),
        },
    ]
}

/// Two triangles per wall quad, wound outward for the sweep's direction.
fn wall_indices(edges: usize, d: f32) -> Vec<u32> {
    let order = WALL_WINDING[usize::from(d > 0.0)];
    (0..edges as u32)
        .flat_map(|k| order.map(|corner| k * 4 + corner))
        .collect()
}

/// The profile's XY bounding box, used to parameterize cap vertices.
fn profile_bounds(points: &[Vec2]) -> Bounds {
    points.iter().fold(
        Bounds {
            min: Vec2::new(f32::INFINITY, f32::INFINITY),
            max: Vec2::new(f32::NEG_INFINITY, f32::NEG_INFINITY),
        },
        |bounds, p| Bounds {
            min: Vec2::new(bounds.min.x.min(p.x), bounds.min.y.min(p.y)),
            max: Vec2::new(bounds.max.x.max(p.x), bounds.max.y.max(p.y)),
        },
    )
}

/// One cap's vertices at plane `z`, facing `nz`, or none when unwanted.
fn cap_vertices(points: &[Vec2], z: f32, nz: f32, bounds: Bounds, wanted: bool) -> Vec<Vertex> {
    // A closed profile encloses area, so both spans are strictly positive; the
    // clamp only keeps the division total.
    let span = Vec2::new(
        (bounds.max.x - bounds.min.x).max(PROFILE_EPSILON),
        (bounds.max.y - bounds.min.y).max(PROFILE_EPSILON),
    );
    points
        .iter()
        .take(points.len() * usize::from(wanted))
        .map(|p| Vertex {
            position: Vec3::new(p.x, p.y, z),
            normal: Vec3::new(0.0, 0.0, nz),
            uv: Vec2::new((p.x - bounds.min.x) / span.x, (p.y - bounds.min.y) / span.y),
        })
        .collect()
}

/// One cap's indices, offset into its own vertex block and wound to face out.
fn cap_indices(triangles: &[[u32; 3]], base: u32, keep: bool, wanted: bool) -> Vec<u32> {
    let order = CAP_WINDING[usize::from(keep)];
    triangles
        .iter()
        .take(triangles.len() * usize::from(wanted))
        .flat_map(|t| order.map(|corner| base + t[corner]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meters(value: f32) -> Meters {
        Meters::new(value).unwrap()
    }

    fn unit_square() -> Profile {
        Profile::closed(vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(0.0, 1.0),
        ])
        .unwrap()
    }

    fn open_strip() -> Profile {
        Profile::open(vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(1.0, 1.0),
        ])
        .unwrap()
    }

    /// Every triangle's geometric normal and centroid.
    fn faces(mesh: &Mesh) -> Vec<(Vec3, Vec3)> {
        mesh.indices()
            .chunks(3)
            .map(|t| {
                let (a, b, c) = (
                    mesh.positions()[t[0] as usize],
                    mesh.positions()[t[1] as usize],
                    mesh.positions()[t[2] as usize],
                );
                (
                    b.subtract(a).cross(c.subtract(a)),
                    a.add(b).add(c).mul_scalar(1.0 / 3.0),
                )
            })
            .collect()
    }

    fn assert_all_faces_point_outward(mesh: &Mesh, centre: Vec3) {
        for (index, (normal, centroid)) in faces(mesh).into_iter().enumerate() {
            let outward = centroid.subtract(centre);
            assert!(
                normal.dot(outward) > 0.0,
                "face {index} normal {normal:?} at {centroid:?} faces inward"
            );
            assert!(normal.length() > 1.0e-6, "face {index} is degenerate");
        }
    }

    #[test]
    fn a_capped_square_extrusion_is_a_closed_box() {
        let mesh = extrude(&unit_square(), meters(2.0), CapPolicy::Both).unwrap();

        // 4 walls * 2 + 2 caps * 2 = 12 triangles: six quads' worth.
        assert_eq!(mesh.triangle_count(), 12);
        // 4 edges * 4 wall vertices + 4 + 4 cap vertices.
        assert_eq!(mesh.vertex_count(), 24);
        assert!(mesh.has_normals() & mesh.has_uvs());
        assert_all_faces_point_outward(&mesh, Vec3::new(0.5, 0.5, 1.0));
    }

    #[test]
    fn the_box_spans_exactly_the_profile_and_the_sweep() {
        let mesh = extrude(&unit_square(), meters(2.0), CapPolicy::Both).unwrap();
        let zs: Vec<f32> = mesh.positions().iter().map(|p| p.z).collect();
        assert!(zs.iter().all(|z| (*z == 0.0) | (*z == 2.0)));
        assert!(zs.contains(&0.0));
        assert!(zs.contains(&2.0));
        assert!(mesh
            .positions()
            .iter()
            .all(|p| (0.0..=1.0).contains(&p.x) & (0.0..=1.0).contains(&p.y)));
    }

    #[test]
    fn walls_are_flat_shaded_with_one_normal_per_edge() {
        let mesh = extrude(&unit_square(), meters(1.0), CapPolicy::None).unwrap();
        // The four wall normals are the four axis directions, each on exactly
        // four vertices — no corner smoothing.
        for expected in [
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(-1.0, 0.0, 0.0),
        ] {
            let count = mesh
                .normals()
                .iter()
                .filter(|n| n.subtract(expected).length() < 1.0e-6)
                .count();
            assert_eq!(count, 4, "expected four vertices with normal {expected:?}");
        }
    }

    #[test]
    fn cap_vertices_carry_the_plus_and_minus_z_normals() {
        let mesh = extrude(&unit_square(), meters(2.0), CapPolicy::Both).unwrap();
        assert_eq!(
            mesh.normals()
                .iter()
                .filter(|n| **n == Vec3::new(0.0, 0.0, -1.0))
                .count(),
            4
        );
        assert_eq!(
            mesh.normals()
                .iter()
                .filter(|n| **n == Vec3::new(0.0, 0.0, 1.0))
                .count(),
            4
        );
    }

    #[test]
    fn wall_uvs_run_the_perimeter_horizontally_and_the_sweep_vertically() {
        let mesh = extrude(&unit_square(), meters(2.0), CapPolicy::None).unwrap();
        let uvs = mesh.uvs();
        assert_eq!(uvs.len(), 16);
        // v is 0 on the start plane and 1 on the end plane.
        for (uv, position) in uvs.iter().zip(mesh.positions()) {
            let expected_v = position.z / 2.0;
            assert!((uv.y - expected_v).abs() < 1.0e-6);
            assert!((0.0..=1.0).contains(&uv.x));
        }
        // The unit square's perimeter is 4, so each edge advances u by 0.25.
        assert!((uvs[0].x - 0.0).abs() < 1.0e-6);
        assert!((uvs[1].x - 0.25).abs() < 1.0e-6);
        assert!((uvs[4].x - 0.25).abs() < 1.0e-6);
        assert!((uvs[15].x - 0.75).abs() < 1.0e-6);
        // The final edge closes the loop at u = 1.
        assert!((uvs[13].x - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn cap_uvs_normalise_the_profile_bounding_box() {
        let profile = Profile::closed(vec![
            Vec2::new(-2.0, -1.0),
            Vec2::new(2.0, -1.0),
            Vec2::new(2.0, 1.0),
            Vec2::new(-2.0, 1.0),
        ])
        .unwrap();
        let mesh = extrude(&profile, meters(1.0), CapPolicy::Start).unwrap();
        // Walls first (4 edges * 4), then the single start cap's 4 vertices.
        let cap_uvs = &mesh.uvs()[16..20];
        assert_eq!(cap_uvs[0], Vec2::new(0.0, 0.0));
        assert_eq!(cap_uvs[1], Vec2::new(1.0, 0.0));
        assert_eq!(cap_uvs[2], Vec2::new(1.0, 1.0));
        assert_eq!(cap_uvs[3], Vec2::new(0.0, 1.0));
    }

    #[test]
    fn no_caps_leaves_only_the_side_walls() {
        let mesh = extrude(&unit_square(), meters(2.0), CapPolicy::None).unwrap();
        assert_eq!(mesh.triangle_count(), 8);
        assert_eq!(mesh.vertex_count(), 16);
        assert!(mesh.normals().iter().all(|n| n.z == 0.0));
    }

    #[test]
    fn one_cap_adds_exactly_that_cap() {
        let start = extrude(&unit_square(), meters(2.0), CapPolicy::Start).unwrap();
        assert_eq!(start.triangle_count(), 10);
        assert_eq!(start.vertex_count(), 20);
        assert!(start.positions()[16..].iter().all(|p| p.z == 0.0));

        let end = extrude(&unit_square(), meters(2.0), CapPolicy::End).unwrap();
        assert_eq!(end.triangle_count(), 10);
        assert_eq!(end.vertex_count(), 20);
        assert!(end.positions()[16..].iter().all(|p| p.z == 2.0));
    }

    #[test]
    fn a_clockwise_profile_still_extrudes_outward() {
        let profile = unit_square().reversed();
        assert_eq!(profile.winding(), ProfileWinding::Clockwise);
        let mesh = extrude(&profile, meters(2.0), CapPolicy::Both).unwrap();
        assert_eq!(mesh.triangle_count(), 12);
        assert_all_faces_point_outward(&mesh, Vec3::new(0.5, 0.5, 1.0));
    }

    #[test]
    fn a_negative_distance_sweeps_backward_and_still_faces_outward() {
        let mesh = extrude(&unit_square(), meters(-2.0), CapPolicy::Both).unwrap();
        assert_eq!(mesh.triangle_count(), 12);
        assert!(mesh.positions().iter().all(|p| p.z <= 0.0));
        assert_all_faces_point_outward(&mesh, Vec3::new(0.5, 0.5, -1.0));
        // The cap on the profile plane now faces +Z, the swept one -Z.
        assert_eq!(mesh.normals()[16], Vec3::new(0.0, 0.0, 1.0));
        assert_eq!(mesh.normals()[20], Vec3::new(0.0, 0.0, -1.0));
    }

    #[test]
    fn a_negative_clockwise_extrusion_faces_outward_too() {
        let mesh = extrude(&unit_square().reversed(), meters(-1.5), CapPolicy::Both).unwrap();
        assert_all_faces_point_outward(&mesh, Vec3::new(0.5, 0.5, -0.75));
    }

    #[test]
    fn an_open_profile_extrudes_to_walls_and_ignores_cap_requests() {
        let capped = extrude(&open_strip(), meters(1.0), CapPolicy::Both).unwrap();
        let uncapped = extrude(&open_strip(), meters(1.0), CapPolicy::None).unwrap();
        // Two edges, two quads, four triangles — the cap request changed nothing.
        assert_eq!(capped.triangle_count(), 4);
        assert_eq!(capped.vertex_count(), 8);
        assert_eq!(capped, uncapped);
        assert!(capped.normals().iter().all(|n| n.z == 0.0));
    }

    #[test]
    fn a_concave_profile_extrudes_into_a_solid_with_matching_caps() {
        let profile = Profile::closed(vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(2.0, 0.0),
            Vec2::new(2.0, 1.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(1.0, 2.0),
            Vec2::new(0.0, 2.0),
        ])
        .unwrap();
        let mesh = extrude(&profile, meters(1.0), CapPolicy::Both).unwrap();
        // 6 walls * 2 + 2 caps * 4 triangles.
        assert_eq!(mesh.triangle_count(), 20);
        assert_eq!(mesh.vertex_count(), 6 * 4 + 6 + 6);
        // Both caps face along their own axis, and the L's area is 3 per cap.
        let cap_area: f32 = faces(&mesh)
            .iter()
            .filter(|(normal, _)| normal.x == 0.0 && normal.y == 0.0)
            .map(|(normal, _)| normal.length() * 0.5)
            .sum();
        assert!((cap_area - 6.0).abs() < 1.0e-5);
    }

    #[test]
    fn extruding_is_deterministic() {
        let a = extrude(&unit_square(), meters(2.0), CapPolicy::Both).unwrap();
        let b = extrude(&unit_square(), meters(2.0), CapPolicy::Both).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn a_zero_distance_is_rejected() {
        assert_eq!(
            extrude(&unit_square(), meters(0.0), CapPolicy::Both)
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidParameter
        );
    }

    #[test]
    fn a_self_intersecting_outline_fails_only_when_a_cap_needs_it() {
        let bowtie = Profile::closed(vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(4.0, 4.0),
            Vec2::new(4.0, 0.0),
            Vec2::new(0.0, 1.0),
        ])
        .unwrap();
        assert_eq!(
            extrude(&bowtie, meters(1.0), CapPolicy::Both)
                .unwrap_err()
                .code(),
            MeshErrorCode::TriangulationFailed
        );
        // Without caps the outline never has to be decomposed.
        let shell = extrude(&bowtie, meters(1.0), CapPolicy::None).unwrap();
        assert_eq!(shell.triangle_count(), 8);
    }
}
