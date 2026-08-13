//! Generated vertex normals: area-weighted smooth, and per-face flat.
//!
//! Both generators derive normals from positions and winding alone, replacing
//! whatever normal stream the mesh arrived with and preserving every other
//! attribute. Winding is the layer's convention: for triangle `(a, b, c)` the
//! face normal is `(p[b] - p[a]) x (p[c] - p[a])`, so counter-clockwise
//! triangles face outward.

use axiom_math::Vec3;

use crate::mesh::Mesh;
use crate::mesh_result::MeshResult;
use crate::mesh_streams::MeshStreams;

/// The **un-normalized** face normal of one triangle.
///
/// The cross product's magnitude is twice the triangle's area, and that is
/// deliberately kept: accumulating the raw vector weights each face's
/// contribution by its area for free, so a large face bends a shared vertex's
/// normal more than a sliver does. Normalizing first would throw the weighting
/// away and make the result depend on how finely the surface happens to be cut.
fn face_cross(positions: &[Vec3], triangle: &[u32]) -> Vec3 {
    let a = positions[triangle[0] as usize];
    positions[triangle[1] as usize]
        .subtract(a)
        .cross(positions[triangle[2] as usize].subtract(a))
}

/// Unit-length `v`, or `+Y` when `v` has no direction to report.
///
/// A zero-length accumulation means the vertex has no defined normal: it is
/// referenced by no triangle, every triangle touching it is degenerate, or its
/// faces' contributions cancel exactly (a fin folded back on itself). `+Y` is
/// the engine's up axis and is chosen as a **deterministic, documented
/// fallback** — the alternative, failing the whole mesh, would make a single
/// stray vertex unrenderable geometry.
fn unit_or_up(v: Vec3) -> Vec3 {
    v.normalize().unwrap_or(Vec3::UNIT_Y)
}

/// Sum every triangle's area-weighted face normal into its three corners.
fn accumulate_face_normals(positions: &[Vec3], indices: &[u32], accumulator: &mut [Vec3]) {
    indices.chunks_exact(3).for_each(|triangle| {
        let face = face_cross(positions, triangle);
        triangle.iter().for_each(|&corner| {
            let i = corner as usize;
            accumulator[i] = accumulator[i].add(face);
        });
    });
}

/// Copy one attribute stream through a corner list, unwelding it.
///
/// An **absent** stream is empty, so every lookup misses and the result is
/// empty too: absence carries across without a special case, which is exactly
/// what the empty-stream-means-absent contract buys.
fn gather<T: Copy>(stream: &[T], corners: &[usize]) -> Vec<T> {
    corners
        .iter()
        .filter_map(|&i| stream.get(i).copied())
        .collect()
}

/// Smooth per-vertex normals, area-weighted across every triangle sharing the
/// vertex.
///
/// Vertices are shared, so this is the generator for surfaces meant to read as
/// curved. A vertex with no defined normal receives `+Y` (see [`unit_or_up`]).
/// Topology, positions, and every other attribute stream are untouched.
pub fn generate_normals(mesh: &Mesh) -> MeshResult<Mesh> {
    let mut accumulator = vec![Vec3::ZERO; mesh.vertex_count()];
    accumulate_face_normals(mesh.positions(), mesh.indices(), &mut accumulator);
    let normals: Vec<Vec3> = accumulator.into_iter().map(unit_or_up).collect();
    Mesh::from_streams(MeshStreams {
        normals,
        ..mesh.clone().into_streams()
    })
}

/// Flat per-face normals, produced by **unwelding**: three fresh vertices per
/// triangle, each carrying that triangle's own face normal.
///
/// Faceted shading needs a hard normal discontinuity at every edge, which a
/// shared vertex cannot express — so the vertex count becomes `3 * triangles`
/// and the index buffer becomes `0..3n`. Every present attribute (uvs,
/// tangents, colours, skin joints and weights) is copied from the source vertex
/// each corner came from.
///
/// A mesh with no triangles unwelds to no vertices, and the empty mesh is not
/// representable, so that input fails with
/// [`crate::MeshErrorCode::EmptyPositions`] rather than silently returning
/// something a renderer cannot draw.
pub fn generate_flat_normals(mesh: &Mesh) -> MeshResult<Mesh> {
    let positions = mesh.positions();
    let corners: Vec<usize> = mesh.indices().iter().map(|&i| i as usize).collect();
    let normals: Vec<Vec3> = mesh
        .indices()
        .chunks_exact(3)
        .flat_map(|triangle| [unit_or_up(face_cross(positions, triangle)); 3])
        .collect();
    Mesh::from_streams(MeshStreams {
        normals,
        uvs: gather(mesh.uvs(), &corners),
        tangents: gather(mesh.tangents(), &corners),
        colors: gather(mesh.colors(), &corners),
        joints: gather(mesh.joints(), &corners),
        weights: gather(mesh.weights(), &corners),
        ..MeshStreams::new(
            gather(positions, &corners),
            (0..corners.len() as u32).collect(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh_error_code::MeshErrorCode;
    use axiom_math::{ApproxEq, Epsilon, Vec2, Vec4};

    fn eps() -> Epsilon {
        Epsilon::DEFAULT
    }

    /// A unit quad in the XZ plane, wound counter-clockwise seen from `+Y`.
    fn quad() -> Mesh {
        Mesh::from_streams(MeshStreams::new(
            vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, -1.0),
                Vec3::new(0.0, 0.0, -1.0),
            ],
            vec![0, 1, 2, 0, 2, 3],
        ))
        .unwrap()
    }

    #[test]
    fn a_flat_quads_smooth_normals_all_point_up() {
        let m = generate_normals(&quad()).unwrap();
        assert_eq!(m.vertex_count(), 4);
        assert!(m
            .normals()
            .iter()
            .all(|n| n.approx_eq(&Vec3::UNIT_Y, eps())));
        // Winding, not position order, decides the sign.
        assert_eq!(m.indices(), quad().indices());
    }

    #[test]
    fn reversing_the_winding_flips_the_generated_normal() {
        let flipped = Mesh::from_streams(MeshStreams::new(
            vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(0.0, 0.0, -1.0),
                Vec3::new(1.0, 0.0, 0.0),
            ],
            vec![0, 1, 2],
        ))
        .unwrap();
        let n = generate_normals(&flipped).unwrap();
        assert!(n.normals()[0].approx_eq(&Vec3::new(0.0, -1.0, 0.0), eps()));
    }

    #[test]
    fn a_shared_vertex_is_weighted_by_face_area_not_face_count() {
        // Vertex 0 is shared by a large `+Y` face (twice-area 100) and a tiny
        // `+X` face (twice-area 1). Equal weighting would give a 45-degree
        // blend; area weighting must land the result almost exactly on `+Y`.
        let m = Mesh::from_streams(MeshStreams::new(
            vec![
                Vec3::ZERO,
                Vec3::new(10.0, 0.0, 0.0),
                Vec3::new(0.0, 0.0, -10.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(0.0, 0.0, 1.0),
            ],
            vec![0, 1, 2, 0, 3, 4],
        ))
        .unwrap();
        let n = generate_normals(&m).unwrap().normals()[0];
        // Accumulated (1, 100, 0), normalized.
        assert!(n.y > 0.999);
        assert!(n.x > 0.0);
        assert!(n.x < 0.02);
        // The equal-weight answer would be ~0.707 on both axes.
        assert!(n.x < 0.5);
    }

    #[test]
    fn a_vertex_no_triangle_references_falls_back_to_up() {
        let m = Mesh::from_streams(MeshStreams::new(
            vec![
                Vec3::ZERO,
                Vec3::new(0.0, 0.0, -1.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(5.0, 5.0, 5.0),
            ],
            vec![0, 1, 2],
        ))
        .unwrap();
        assert_eq!(generate_normals(&m).unwrap().normals()[3], Vec3::UNIT_Y);
    }

    #[test]
    fn exactly_cancelling_faces_fall_back_to_up() {
        // The same triangle wound both ways: every accumulation sums to zero.
        let m = Mesh::from_streams(MeshStreams::new(
            vec![Vec3::ZERO, Vec3::UNIT_X, Vec3::new(0.0, 0.0, -1.0)],
            vec![0, 1, 2, 0, 2, 1],
        ))
        .unwrap();
        assert!(generate_normals(&m)
            .unwrap()
            .normals()
            .iter()
            .all(|&n| n == Vec3::UNIT_Y));
    }

    #[test]
    fn smooth_generation_replaces_old_normals_and_preserves_other_streams() {
        let source = Mesh::from_streams(MeshStreams {
            normals: vec![Vec3::new(0.0, -1.0, 0.0); 4],
            uvs: vec![Vec2::new(0.25, 0.75); 4],
            colors: vec![Vec4::new(0.1, 0.2, 0.3, 1.0); 4],
            ..quad().into_streams()
        })
        .unwrap();
        let m = generate_normals(&source).unwrap();
        assert!(m.normals()[0].approx_eq(&Vec3::UNIT_Y, eps()));
        assert_eq!(m.uvs(), &[Vec2::new(0.25, 0.75); 4]);
        assert_eq!(m.colors()[3], Vec4::new(0.1, 0.2, 0.3, 1.0));
        assert_eq!(m.positions(), quad().positions());
    }

    #[test]
    fn flat_generation_unwelds_to_three_vertices_per_triangle() {
        let m = generate_flat_normals(&quad()).unwrap();
        assert_eq!(m.vertex_count(), 6);
        assert_eq!(m.triangle_count(), 2);
        assert_eq!(m.indices(), &[0, 1, 2, 3, 4, 5]);
        assert!(m
            .normals()
            .iter()
            .all(|n| n.approx_eq(&Vec3::UNIT_Y, eps())));
        // The unwelded corners are the source positions in index order.
        assert_eq!(m.positions()[2], Vec3::new(1.0, 0.0, -1.0));
        assert_eq!(m.positions()[3], Vec3::ZERO);
    }

    #[test]
    fn flat_normals_differ_per_face_across_a_crease() {
        // A roof: one face up-and-forward, one up-and-back. Smoothing would
        // average them at the ridge; unwelding must keep them distinct.
        let m = Mesh::from_streams(MeshStreams::new(
            vec![
                Vec3::new(0.0, 0.0, 1.0),
                Vec3::new(1.0, 0.0, 1.0),
                Vec3::new(1.0, 1.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(1.0, 0.0, -1.0),
                Vec3::new(0.0, 0.0, -1.0),
            ],
            vec![0, 1, 2, 0, 2, 3, 3, 2, 4, 3, 4, 5],
        ))
        .unwrap();
        let flat = generate_flat_normals(&m).unwrap();
        let front = flat.normals()[0];
        let back = flat.normals()[6];
        assert!(front.z > 0.0);
        assert!(back.z < 0.0);
        assert!(front.y > 0.0);
        assert!(back.y > 0.0);
        // The two faces of the front slope agree with each other.
        assert!(flat.normals()[0].approx_eq(&flat.normals()[3], eps()));
    }

    #[test]
    fn flat_generation_carries_every_present_attribute_through_the_unweld() {
        let source = Mesh::from_streams(MeshStreams {
            uvs: vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(1.0, 0.0),
                Vec2::new(1.0, 1.0),
                Vec2::new(0.0, 1.0),
            ],
            tangents: vec![Vec4::new(1.0, 0.0, 0.0, -1.0); 4],
            colors: vec![
                Vec4::new(1.0, 0.0, 0.0, 1.0),
                Vec4::new(0.0, 1.0, 0.0, 1.0),
                Vec4::new(0.0, 0.0, 1.0, 1.0),
                Vec4::new(1.0, 1.0, 0.0, 1.0),
            ],
            joints: vec![[0, 0, 0, 0], [1, 0, 0, 0], [2, 0, 0, 0], [3, 0, 0, 0]],
            weights: vec![[1.0, 0.0, 0.0, 0.0]; 4],
            ..quad().into_streams()
        })
        .unwrap();
        let m = generate_flat_normals(&source).unwrap();
        // Corner 4 of the unwelded mesh came from source vertex 2 (indices
        // `[0,1,2, 0,2,3]`).
        assert_eq!(m.uvs()[4], Vec2::new(1.0, 1.0));
        assert_eq!(m.colors()[4], Vec4::new(0.0, 0.0, 1.0, 1.0));
        assert_eq!(m.joints()[4], [2, 0, 0, 0]);
        assert_eq!(m.tangents()[4], Vec4::new(1.0, 0.0, 0.0, -1.0));
        assert_eq!(m.weights()[4], [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(m.uvs().len(), 6);
    }

    #[test]
    fn flat_generation_leaves_absent_attributes_absent() {
        let m = generate_flat_normals(&quad()).unwrap();
        assert!(!m.has_uvs());
        assert!(!m.has_colors());
        assert!(!m.has_tangents());
        assert!(!m.is_skinned());
        assert!(m.has_normals());
    }

    #[test]
    fn a_degenerate_triangle_gets_the_up_fallback_normal() {
        let m = Mesh::from_streams(MeshStreams::new(
            vec![Vec3::ZERO, Vec3::UNIT_X, Vec3::new(2.0, 0.0, 0.0)],
            vec![0, 1, 2],
        ))
        .unwrap();
        assert!(generate_flat_normals(&m)
            .unwrap()
            .normals()
            .iter()
            .all(|&n| n == Vec3::UNIT_Y));
    }

    #[test]
    fn unwelding_a_mesh_with_no_triangles_reports_an_empty_mesh() {
        let m = Mesh::from_streams(MeshStreams::new(vec![Vec3::ZERO; 3], Vec::new())).unwrap();
        assert_eq!(
            generate_flat_normals(&m).unwrap_err().code(),
            MeshErrorCode::EmptyPositions
        );
    }

    #[test]
    fn both_generators_are_deterministic() {
        assert_eq!(
            generate_normals(&quad()).unwrap(),
            generate_normals(&quad()).unwrap()
        );
        assert_eq!(
            generate_flat_normals(&quad()).unwrap(),
            generate_flat_normals(&quad()).unwrap()
        );
    }
}
