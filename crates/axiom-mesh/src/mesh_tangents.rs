//! Per-vertex tangents derived from the UV parameterization.
//!
//! A tangent frame is what turns a tangent-space normal map into a world-space
//! perturbation, so it is defined by the *texture* mapping, not by the geometry
//! alone: the tangent points along increasing `u` and the bitangent along
//! increasing `v`. This layer stores the frame compactly — `xyz` is the
//! orthonormalized tangent and `w` is the bitangent's handedness (`±1`), which
//! is all a shader needs to rebuild the third axis as `w * (n x t)`.

use axiom_math::{tangent_basis, Vec2, Vec3, Vec4};

use crate::mesh::Mesh;
use crate::mesh_error::MeshError;
use crate::mesh_error_code::MeshErrorCode;
use crate::mesh_result::MeshResult;
use crate::mesh_streams::MeshStreams;

fn generation_failed(message: &'static str) -> MeshError {
    MeshError::new(MeshErrorCode::TangentGenerationFailed, message)
}

/// Add one triangle's UV-space tangent and bitangent into each of its corners.
///
/// The per-triangle solve is the standard one: the two edge vectors are the
/// image of the two UV delta vectors under the (linear) tangent frame, so
/// inverting the 2x2 UV matrix recovers the frame. The determinant of that
/// matrix is zero exactly when the triangle's UVs are collinear or coincident —
/// a degenerate parameterization that names no direction. Such a triangle
/// contributes a **zero** vector (selected without a branch, and without ever
/// multiplying by an infinite reciprocal), so its neighbours still decide the
/// shared vertices' frames.
fn accumulate_triangle(
    triangle: &[u32],
    positions: &[Vec3],
    uvs: &[Vec2],
    tangents: &mut [Vec3],
    bitangents: &mut [Vec3],
) {
    let (ia, ib, ic) = (
        triangle[0] as usize,
        triangle[1] as usize,
        triangle[2] as usize,
    );
    let edge1 = positions[ib].subtract(positions[ia]);
    let edge2 = positions[ic].subtract(positions[ia]);
    let delta1 = uvs[ib].subtract(uvs[ia]);
    let delta2 = uvs[ic].subtract(uvs[ia]);
    let determinant = delta1.x * delta2.y - delta2.x * delta1.y;
    let scale = [0.0, determinant.recip()][usize::from(determinant != 0.0)];
    let tangent = edge1
        .mul_scalar(delta2.y)
        .subtract(edge2.mul_scalar(delta1.y))
        .mul_scalar(scale);
    let bitangent = edge2
        .mul_scalar(delta1.x)
        .subtract(edge1.mul_scalar(delta2.x))
        .mul_scalar(scale);
    triangle.iter().for_each(|&corner| {
        let i = corner as usize;
        tangents[i] = tangents[i].add(tangent);
        bitangents[i] = bitangents[i].add(bitangent);
    });
}

/// Orthonormalize one accumulated frame against the vertex normal and pack it.
///
/// Gram-Schmidt removes the normal-aligned component the averaging introduced,
/// so the stored tangent is exactly perpendicular to the shading normal. The
/// handedness `w` records whether the accumulated bitangent agrees with
/// `n x t`; a mirrored UV island flips it, which is precisely the information a
/// shader would otherwise have to guess.
///
/// A vertex whose accumulated tangent is zero (no triangle references it, or
/// every triangle touching it is UV-degenerate) or whose tangent is parallel to
/// its normal has no recoverable direction. It receives a **deterministic
/// orthonormal companion of the normal** — the same east axis
/// [`axiom_math::tangent_basis`] builds, pole-hardened — so the frame stays
/// valid and unit-length instead of poisoning the stream with a zero vector.
fn vertex_tangent(normal: Vec3, tangent: Vec3, bitangent: Vec3) -> Vec4 {
    let orthogonal = tangent.subtract(normal.mul_scalar(normal.dot(tangent)));
    let unit = orthogonal
        .normalize()
        .unwrap_or_else(|_| tangent_basis(normal).0);
    let handedness = [1.0, -1.0][usize::from(normal.cross(unit).dot(bitangent) < 0.0)];
    Vec4::new(unit.x, unit.y, unit.z, handedness)
}

/// Accumulate, check that the parameterization said something, and rebuild.
fn build_tangents(mesh: &Mesh) -> MeshResult<Mesh> {
    let mut tangents = vec![Vec3::ZERO; mesh.vertex_count()];
    let mut bitangents = vec![Vec3::ZERO; mesh.vertex_count()];
    mesh.indices().chunks_exact(3).for_each(|triangle| {
        accumulate_triangle(
            triangle,
            mesh.positions(),
            mesh.uvs(),
            &mut tangents,
            &mut bitangents,
        );
    });
    tangents
        .iter()
        .any(|t| t.length_squared() > 0.0)
        .then_some(())
        .ok_or_else(|| {
            generation_failed(
                "no triangle carried a usable uv parameterization, so no tangent direction exists",
            )
        })
        .and_then(|()| {
            let resolved: Vec<Vec4> = mesh
                .normals()
                .iter()
                .copied()
                .zip(tangents.iter().copied())
                .zip(bitangents.iter().copied())
                .map(|((normal, tangent), bitangent)| vertex_tangent(normal, tangent, bitangent))
                .collect();
            Mesh::from_streams(MeshStreams {
                tangents: resolved,
                ..mesh.clone().into_streams()
            })
        })
}

/// Generate the per-vertex tangent frame from the mesh's UVs and normals.
///
/// Requires both streams: UVs define the tangent direction and the normals
/// define the plane it is orthonormalized into, so neither is substitutable.
/// A mesh missing either — or whose every triangle has a degenerate UV
/// parameterization, leaving no direction anywhere — fails with
/// [`crate::MeshErrorCode::TangentGenerationFailed`] rather than inventing a
/// frame the texture does not support.
///
/// Topology, positions, and every other attribute stream are preserved; an
/// existing tangent stream is replaced.
pub fn generate_tangents(mesh: &Mesh) -> MeshResult<Mesh> {
    (mesh.has_uvs() & mesh.has_normals())
        .then_some(())
        .ok_or_else(|| {
            generation_failed("tangent generation requires both a uv and a normal stream")
        })
        .and_then(|()| build_tangents(mesh))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::{ApproxEq, Epsilon};

    fn eps() -> Epsilon {
        Epsilon::DEFAULT
    }

    /// A unit quad in the XZ plane facing `+Y`, with `u` along `+X` and `v`
    /// along `-Z` — the canonical mapping, so the tangent must come out `+X`.
    fn quad_streams() -> MeshStreams {
        MeshStreams {
            normals: vec![Vec3::UNIT_Y; 4],
            uvs: vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(1.0, 0.0),
                Vec2::new(1.0, 1.0),
                Vec2::new(0.0, 1.0),
            ],
            ..MeshStreams::new(
                vec![
                    Vec3::new(0.0, 0.0, 0.0),
                    Vec3::new(1.0, 0.0, 0.0),
                    Vec3::new(1.0, 0.0, -1.0),
                    Vec3::new(0.0, 0.0, -1.0),
                ],
                vec![0, 1, 2, 0, 2, 3],
            )
        }
    }

    fn quad() -> Mesh {
        Mesh::from_streams(quad_streams()).unwrap()
    }

    #[test]
    fn the_tangent_follows_increasing_u() {
        let m = generate_tangents(&quad()).unwrap();
        assert_eq!(m.tangents().len(), 4);
        m.tangents().iter().for_each(|t| {
            assert!(Vec3::new(t.x, t.y, t.z).approx_eq(&Vec3::UNIT_X, eps()));
        });
    }

    #[test]
    fn the_stored_tangent_is_unit_length_and_perpendicular_to_the_normal() {
        // Lean the shading normals into the `+X` the raw UV solve produces, so
        // the two are *not* already perpendicular and Gram-Schmidt must remove a
        // real component rather than a rounding error.
        let leaning = Mesh::from_streams(MeshStreams {
            normals: vec![Vec3::new(1.0, 1.0, 0.0).normalize().unwrap(); 4],
            ..quad_streams()
        })
        .unwrap();
        let m = generate_tangents(&leaning).unwrap();
        m.normals()
            .iter()
            .zip(m.tangents().iter())
            .for_each(|(n, t)| {
                let dir = Vec3::new(t.x, t.y, t.z);
                assert!(dir.length().approx_eq(&1.0, eps()));
                assert!(n.dot(dir).abs() < 1.0e-5);
            });
        // The raw `+X` tangent, with its normal-aligned half removed, rotates
        // into the surface: `(1,0,0) - n*(n.x)` normalized is `(1,-1,0)/sqrt(2)`.
        let t = m.tangents()[0];
        let expected = Vec3::new(1.0, -1.0, 0.0).normalize().unwrap();
        assert!(Vec3::new(t.x, t.y, t.z).approx_eq(&expected, eps()));
    }

    #[test]
    fn a_right_handed_mapping_reports_positive_handedness() {
        let m = generate_tangents(&quad()).unwrap();
        assert!(m.tangents().iter().all(|t| t.w == 1.0));
    }

    #[test]
    fn a_mirrored_v_axis_reports_negative_handedness() {
        // Same geometry, `v` running the other way: the bitangent flips while
        // the tangent does not, which is exactly what `w` exists to record.
        let mirrored = Mesh::from_streams(MeshStreams {
            uvs: vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(1.0, 0.0),
                Vec2::new(1.0, -1.0),
                Vec2::new(0.0, -1.0),
            ],
            ..quad_streams()
        })
        .unwrap();
        let m = generate_tangents(&mirrored).unwrap();
        assert!(m.tangents().iter().all(|t| t.w == -1.0));
        // The tangent direction itself is unchanged.
        let t = m.tangents()[0];
        assert!(Vec3::new(t.x, t.y, t.z).approx_eq(&Vec3::UNIT_X, eps()));
    }

    #[test]
    fn a_rotated_mapping_rotates_the_tangent() {
        // Swap the roles of the axes: `u` now runs along `-Z`, so the tangent
        // must follow it rather than staying pinned to `+X`.
        let rotated = Mesh::from_streams(MeshStreams {
            uvs: vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(0.0, 1.0),
                Vec2::new(1.0, 1.0),
                Vec2::new(1.0, 0.0),
            ],
            ..quad_streams()
        })
        .unwrap();
        let t = generate_tangents(&rotated).unwrap().tangents()[0];
        assert!(Vec3::new(t.x, t.y, t.z).approx_eq(&Vec3::new(0.0, 0.0, -1.0), eps()));
    }

    #[test]
    fn generation_preserves_every_other_stream_and_replaces_old_tangents() {
        let source = Mesh::from_streams(MeshStreams {
            tangents: vec![Vec4::new(0.0, 0.0, 1.0, -1.0); 4],
            colors: vec![Vec4::new(0.5, 0.25, 0.125, 1.0); 4],
            ..quad_streams()
        })
        .unwrap();
        let m = generate_tangents(&source).unwrap();
        let t = m.tangents()[0];
        assert!(Vec3::new(t.x, t.y, t.z).approx_eq(&Vec3::UNIT_X, eps()));
        assert_eq!(m.colors()[2], Vec4::new(0.5, 0.25, 0.125, 1.0));
        assert_eq!(m.uvs(), quad().uvs());
        assert_eq!(m.positions(), quad().positions());
        assert_eq!(m.indices(), quad().indices());
    }

    #[test]
    fn a_vertex_outside_every_triangle_gets_a_companion_of_its_normal() {
        // Vertex 4 is orphaned: nothing accumulates into it, so its frame is
        // built from its normal alone. `tangent_basis` of `+Y` is the `+X` east
        // axis, and a zero bitangent leaves the handedness positive.
        let orphaned = Mesh::from_streams(MeshStreams {
            normals: vec![Vec3::UNIT_Y; 5],
            uvs: vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(1.0, 0.0),
                Vec2::new(1.0, 1.0),
                Vec2::new(0.0, 1.0),
                Vec2::new(0.5, 0.5),
            ],
            ..MeshStreams::new(
                vec![
                    Vec3::new(0.0, 0.0, 0.0),
                    Vec3::new(1.0, 0.0, 0.0),
                    Vec3::new(1.0, 0.0, -1.0),
                    Vec3::new(0.0, 0.0, -1.0),
                    Vec3::new(9.0, 9.0, 9.0),
                ],
                vec![0, 1, 2, 0, 2, 3],
            )
        })
        .unwrap();
        let m = generate_tangents(&orphaned).unwrap();
        assert_eq!(m.tangents()[4], Vec4::new(1.0, 0.0, 0.0, 1.0));
        // The triangulated vertices are unaffected by the orphan.
        let t = m.tangents()[0];
        assert!(Vec3::new(t.x, t.y, t.z).approx_eq(&Vec3::UNIT_X, eps()));
    }

    #[test]
    fn one_degenerate_triangle_does_not_spoil_its_neighbours() {
        // The second triangle collapses all three UVs onto one point; the first
        // still parameterizes the shared vertices.
        let partly_degenerate = Mesh::from_streams(MeshStreams {
            uvs: vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(1.0, 0.0),
                Vec2::new(0.5, 0.5),
                Vec2::new(0.5, 0.5),
            ],
            ..quad_streams()
        })
        .unwrap();
        let m = generate_tangents(&partly_degenerate).unwrap();
        let t = m.tangents()[1];
        assert!(Vec3::new(t.x, t.y, t.z).approx_eq(&Vec3::UNIT_X, eps()));
        // Vertex 3 is touched only by the degenerate face, so it falls back to
        // the normal's companion rather than to a zero vector.
        assert_eq!(m.tangents()[3], Vec4::new(1.0, 0.0, 0.0, 1.0));
    }

    #[test]
    fn a_mesh_without_uvs_cannot_produce_tangents() {
        let no_uvs = Mesh::from_streams(MeshStreams {
            uvs: Vec::new(),
            ..quad_streams()
        })
        .unwrap();
        assert_eq!(
            generate_tangents(&no_uvs).unwrap_err().code(),
            MeshErrorCode::TangentGenerationFailed
        );
    }

    #[test]
    fn a_mesh_without_normals_cannot_produce_tangents() {
        let no_normals = Mesh::from_streams(MeshStreams {
            normals: Vec::new(),
            ..quad_streams()
        })
        .unwrap();
        assert_eq!(
            generate_tangents(&no_normals).unwrap_err().code(),
            MeshErrorCode::TangentGenerationFailed
        );
    }

    #[test]
    fn a_wholly_degenerate_parameterization_is_reported_not_invented() {
        let collapsed = Mesh::from_streams(MeshStreams {
            uvs: vec![Vec2::new(0.5, 0.5); 4],
            ..quad_streams()
        })
        .unwrap();
        assert_eq!(
            generate_tangents(&collapsed).unwrap_err().code(),
            MeshErrorCode::TangentGenerationFailed
        );
    }

    #[test]
    fn generation_is_deterministic() {
        assert_eq!(
            generate_tangents(&quad()).unwrap(),
            generate_tangents(&quad()).unwrap()
        );
    }
}
