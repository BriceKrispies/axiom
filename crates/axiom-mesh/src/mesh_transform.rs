//! Rigid/affine placement of a mesh, and the winding flip that turns it inside
//! out.
//!
//! # Why positions and normals need different matrices
//!
//! A position is a point: it rides the full 4×4. A normal is *not* a direction
//! that rides along with the surface — it is the covector perpendicular to it,
//! and under a non-uniform scale the perpendicular of the scaled surface is not
//! the scaled perpendicular. Scaling a 45° face by `(2, 1, 1)` shears it toward
//! horizontal, so its normal must tilt toward *vertical* — the opposite way.
//! The transform that does this is the **inverse transpose of the upper-left
//! 3×3** linear part. Feeding normals through the position matrix is the single
//! most common mesh-transform bug, and it only shows up once someone scales
//! non-uniformly, which is why this module never offers that shortcut: the
//! normal matrix is derived here, once, and both the normal and the tangent
//! direction go through it.
//!
//! # Why a mirror also rewrites the index buffer
//!
//! A matrix whose linear part has a **negative determinant** mirrors space. A
//! mirror reverses the orientation of every triangle, so a mesh that was
//! counter-clockwise-front before the transform is clockwise-front after it —
//! its faces would cull backwards. [`transform`] therefore reverses the index
//! order of every triangle when the determinant is negative, restoring the
//! engine's CCW-front convention, and flips each tangent's `w` handedness for
//! the same reason (the bitangent basis is mirrored too).
//!
//! Colours, UVs, joints, and weights are geometry-independent and pass through
//! untouched.

use axiom_math::{Mat4, Vec3, Vec4};

use crate::mesh::Mesh;
use crate::mesh_error::MeshError;
use crate::mesh_error_code::MeshErrorCode;
use crate::mesh_result::MeshResult;
use crate::mesh_streams::MeshStreams;

/// Triangle corner orders: identity, then the mirrored order that restores
/// CCW-front winding. Indexed by "is this triangle being reversed".
const WINDING: [[usize; 3]; 2] = [[0, 1, 2], [0, 2, 1]];

/// Place a mesh by an affine matrix.
///
/// Positions ride the full matrix. Normals and tangent directions ride the
/// **inverse transpose of the linear part**, so a non-uniform scale leaves them
/// perpendicular to the surface they describe; both are re-normalized
/// afterwards. Tangent `w` handedness is preserved, except under a mirror
/// (negative determinant), where it is negated along with the triangle winding.
/// UVs, colours, joints, and weights are copied unchanged.
///
/// # Errors
///
/// [`MeshErrorCode::InvalidParameter`] when `matrix` is singular: a mesh cannot
/// be placed by a transform that collapses space, and no normal matrix exists.
///
/// The normal matrix is read from the upper-left 3×3 of the 4×4 inverse, which
/// is exactly the inverse of the linear part for the affine matrices a mesh
/// placement is built from (translation, rotation, scale, and their products).
pub fn transform(mesh: &Mesh, matrix: Mat4) -> MeshResult<Mesh> {
    matrix
        .inverse()
        .ok_or_else(|| {
            MeshError::new(
                MeshErrorCode::InvalidParameter,
                "a mesh transform matrix must be invertible (a singular matrix collapses the mesh and has no normal matrix)",
            )
        })
        .map(|inverse| transformed_streams(mesh, matrix, inverse))
        .and_then(Mesh::from_streams)
}

/// Turn a mesh inside out: reverse every triangle's winding and negate every
/// normal and tangent direction.
///
/// This is the operation that makes a solid into its own interior — a box
/// becomes a room, a sphere becomes a skydome. It is *not* a repair for a mesh
/// whose faces were authored backwards in only some places; it flips all of
/// them. Tangent `w` handedness is left alone: reversing the surface negates
/// both the normal and the tangent, so their cross product — and therefore the
/// bitangent convention `w` records — is unchanged.
///
/// # Errors
///
/// Propagates [`Mesh::from_streams`] validation, which cannot fail here: the
/// operation permutes and negates already-valid finite streams.
pub fn reverse_winding(mesh: &Mesh) -> MeshResult<Mesh> {
    Mesh::from_streams(MeshStreams {
        normals: mesh.normals().iter().map(|n| n.mul_scalar(-1.0)).collect(),
        tangents: mesh
            .tangents()
            .iter()
            .map(|t| Vec4::new(-t.x, -t.y, -t.z, t.w))
            .collect(),
        uvs: mesh.uvs().to_vec(),
        colors: mesh.colors().to_vec(),
        joints: mesh.joints().to_vec(),
        weights: mesh.weights().to_vec(),
        ..MeshStreams::new(mesh.positions().to_vec(), wound(mesh.indices(), true))
    })
}

/// Build every transformed stream in one pass, given the matrix and its inverse.
fn transformed_streams(mesh: &Mesh, matrix: Mat4, inverse: Mat4) -> MeshStreams {
    let normal_matrix = transpose_linear(inverse);
    let mirrored = linear_determinant(matrix) < 0.0;
    let handedness = [1.0_f32, -1.0][usize::from(mirrored)];
    MeshStreams {
        normals: mesh
            .normals()
            .iter()
            .map(|&n| renormalize(normal_matrix.transform_vector(n)))
            .collect(),
        tangents: mesh
            .tangents()
            .iter()
            .map(|&t| transformed_tangent(normal_matrix, t, handedness))
            .collect(),
        uvs: mesh.uvs().to_vec(),
        colors: mesh.colors().to_vec(),
        joints: mesh.joints().to_vec(),
        weights: mesh.weights().to_vec(),
        ..MeshStreams::new(
            mesh.positions()
                .iter()
                .map(|&p| matrix.transform_point(p))
                .collect(),
            wound(mesh.indices(), mirrored),
        )
    }
}

/// Transpose the upper-left 3×3 of `m`, discarding translation.
///
/// Applied to an inverse, this yields the inverse-transpose normal matrix.
fn transpose_linear(m: Mat4) -> Mat4 {
    let a = m.as_cols_array();
    Mat4::from_cols_array([
        a[0], a[4], a[8], 0.0, //
        a[1], a[5], a[9], 0.0, //
        a[2], a[6], a[10], 0.0, //
        0.0, 0.0, 0.0, 1.0, //
    ])
}

/// The determinant of the upper-left 3×3 linear part. Negative means the
/// transform mirrors space and reverses triangle orientation.
fn linear_determinant(m: Mat4) -> f32 {
    let a = m.as_cols_array();
    a[0] * (a[5] * a[10] - a[9] * a[6]) - a[4] * (a[1] * a[10] - a[9] * a[2])
        + a[8] * (a[1] * a[6] - a[5] * a[2])
}

/// Restore unit length, leaving a degenerate (zero-length) input alone rather
/// than inventing a direction for it.
fn renormalize(v: Vec3) -> Vec3 {
    v.normalize().unwrap_or(v)
}

/// Transform a tangent: direction through the normal matrix, re-normalized;
/// handedness preserved, or negated under a mirror.
fn transformed_tangent(normal_matrix: Mat4, t: Vec4, handedness: f32) -> Vec4 {
    let d = renormalize(normal_matrix.transform_vector(Vec3::new(t.x, t.y, t.z)));
    Vec4::new(d.x, d.y, d.z, t.w * handedness)
}

/// Re-emit the index buffer, optionally swapping each triangle's second and
/// third corner.
fn wound(indices: &[u32], reversed: bool) -> Vec<u32> {
    let order = WINDING[usize::from(reversed)];
    indices
        .chunks_exact(3)
        .flat_map(|t| order.map(|k| t[k]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::Vec2;

    /// One CCW triangle in the XZ plane facing +Y, with every attribute stream
    /// populated so each transform path is exercised.
    fn decorated() -> Mesh {
        Mesh::from_streams(MeshStreams {
            normals: vec![Vec3::UNIT_Y; 3],
            uvs: vec![Vec2::new(0.25, 0.75); 3],
            tangents: vec![Vec4::new(1.0, 0.0, 0.0, 1.0); 3],
            colors: vec![Vec4::new(0.1, 0.2, 0.3, 1.0); 3],
            joints: vec![[2, 0, 0, 0]; 3],
            weights: vec![[1.0, 0.0, 0.0, 0.0]; 3],
            ..MeshStreams::new(
                vec![
                    Vec3::ZERO,
                    Vec3::new(0.0, 0.0, 1.0),
                    Vec3::new(1.0, 0.0, 0.0),
                ],
                vec![0, 1, 2],
            )
        })
        .unwrap()
    }

    /// A 45° face in the XY plane whose normal is the diagonal (1,1,0)/√2 — the
    /// case that exposes a wrong normal matrix under non-uniform scale.
    fn slanted() -> Mesh {
        let n = Vec3::new(1.0, 1.0, 0.0).normalize().unwrap();
        Mesh::from_streams(MeshStreams {
            normals: vec![n; 3],
            ..MeshStreams::new(
                vec![
                    Vec3::new(1.0, 0.0, 0.0),
                    Vec3::new(0.0, 1.0, 0.0),
                    Vec3::new(1.0, 0.0, 1.0),
                ],
                vec![0, 1, 2],
            )
        })
        .unwrap()
    }

    fn geometric_normal(mesh: &Mesh) -> Vec3 {
        let p = mesh.positions();
        let i = mesh.indices();
        let a = p[i[0] as usize];
        p[i[1] as usize]
            .subtract(a)
            .cross(p[i[2] as usize].subtract(a))
            .normalize()
            .unwrap()
    }

    #[test]
    fn translation_moves_positions_and_leaves_normals_alone() {
        let out = transform(&decorated(), Mat4::translation(Vec3::new(3.0, -2.0, 0.5))).unwrap();
        assert_eq!(out.positions()[0], Vec3::new(3.0, -2.0, 0.5));
        assert_eq!(out.positions()[2], Vec3::new(4.0, -2.0, 0.5));
        assert_eq!(out.normals()[0], Vec3::UNIT_Y);
        assert_eq!(out.indices(), &[0, 1, 2]);
    }

    #[test]
    fn non_uniform_scale_keeps_the_normal_perpendicular_to_its_face() {
        // Squashing X by 4 makes the 45° face steeper, so its normal must tilt
        // toward the horizontal (+X). Running the normal through the POSITION
        // matrix instead would tilt it the other way, toward +Y.
        let scaled = transform(&slanted(), Mat4::scale(Vec3::new(0.25, 1.0, 1.0))).unwrap();
        let n = scaled.normals()[0];

        // Unit length is restored.
        assert!((n.length() - 1.0).abs() < 1.0e-5);
        // And it is genuinely perpendicular to the transformed face.
        let face = geometric_normal(&scaled);
        assert!(
            n.dot(face).abs() > 0.999_9,
            "normal {n:?} is not parallel to the face normal {face:?}"
        );
        // The tell-tale: the normal tilted toward the squashed axis (+X), the
        // opposite of what the position matrix would have produced.
        assert!(
            n.x > n.y,
            "normal {n:?} tilted the way the POSITION matrix would have moved it"
        );
    }

    #[test]
    fn uniform_scale_and_rotation_leave_the_normal_direction_untouched() {
        let out = transform(&decorated(), Mat4::scale(Vec3::new(2.0, 2.0, 2.0))).unwrap();
        assert_eq!(out.positions()[2], Vec3::new(2.0, 0.0, 0.0));
        assert_eq!(out.normals()[0], Vec3::UNIT_Y);
        assert_eq!(out.tangents()[0], Vec4::new(1.0, 0.0, 0.0, 1.0));
    }

    #[test]
    fn a_mirror_reverses_winding_and_flips_tangent_handedness() {
        let out = transform(&decorated(), Mat4::scale(Vec3::new(-1.0, 1.0, 1.0))).unwrap();

        assert_eq!(out.indices(), &[0, 2, 1]);
        assert_eq!(out.positions()[2], Vec3::new(-1.0, 0.0, 0.0));
        // w flipped, direction mirrored.
        assert_eq!(out.tangents()[0], Vec4::new(-1.0, 0.0, 0.0, -1.0));
        // The normal still agrees with the (re-wound) face orientation.
        assert!(out.normals()[0].dot(geometric_normal(&out)) > 0.999_9);
    }

    #[test]
    fn attributes_that_do_not_depend_on_geometry_pass_through() {
        let out = transform(&decorated(), Mat4::scale(Vec3::new(-2.0, 3.0, 1.0))).unwrap();
        assert_eq!(out.uvs(), decorated().uvs());
        assert_eq!(out.colors(), decorated().colors());
        assert_eq!(out.joints(), decorated().joints());
        assert_eq!(out.weights(), decorated().weights());
    }

    #[test]
    fn a_mesh_without_optional_streams_transforms_to_one_without_them() {
        let bare = Mesh::from_streams(MeshStreams::new(
            vec![Vec3::ZERO, Vec3::UNIT_X, Vec3::UNIT_Z],
            vec![0, 1, 2],
        ))
        .unwrap();
        let out = transform(&bare, Mat4::translation(Vec3::UNIT_Y)).unwrap();
        assert!(!out.has_normals());
        assert!(!out.has_tangents());
        assert_eq!(out.positions()[0], Vec3::UNIT_Y);
    }

    #[test]
    fn a_degenerate_zero_normal_survives_as_zero_rather_than_becoming_nan() {
        let m = Mesh::from_streams(MeshStreams {
            normals: vec![Vec3::ZERO; 3],
            ..MeshStreams::new(
                vec![Vec3::ZERO, Vec3::UNIT_X, Vec3::UNIT_Z],
                vec![0, 1, 2],
            )
        })
        .unwrap();
        let out = transform(&m, Mat4::scale(Vec3::new(2.0, 2.0, 2.0))).unwrap();
        assert_eq!(out.normals()[0], Vec3::ZERO);
    }

    #[test]
    fn a_singular_matrix_is_rejected() {
        // Flattening Z collapses space: no inverse, no normal matrix.
        let flatten = Mat4::scale(Vec3::new(1.0, 1.0, 0.0));
        let err = transform(&decorated(), flatten).unwrap_err();
        assert_eq!(err.code(), MeshErrorCode::InvalidParameter);
    }

    #[test]
    fn reverse_winding_swaps_corners_and_negates_normals_and_tangents() {
        let out = reverse_winding(&decorated()).unwrap();
        assert_eq!(out.indices(), &[0, 2, 1]);
        assert_eq!(out.positions(), decorated().positions());
        assert_eq!(out.normals()[0], Vec3::new(0.0, -1.0, 0.0));
        // Direction negated, handedness preserved.
        assert_eq!(out.tangents()[0], Vec4::new(-1.0, 0.0, 0.0, 1.0));
        assert_eq!(out.uvs(), decorated().uvs());
        assert_eq!(out.colors(), decorated().colors());
        assert_eq!(out.joints(), decorated().joints());
        assert_eq!(out.weights(), decorated().weights());
    }

    #[test]
    fn reverse_winding_inverts_the_face_orientation() {
        let before = geometric_normal(&decorated());
        let after = geometric_normal(&reverse_winding(&decorated()).unwrap());
        assert!(before.dot(after) < -0.999_9, "{before:?} vs {after:?}");
    }

    #[test]
    fn reversing_twice_returns_the_original_mesh() {
        let twice = reverse_winding(&reverse_winding(&decorated()).unwrap()).unwrap();
        assert_eq!(twice, decorated());
    }

    #[test]
    fn the_linear_determinant_reads_the_upper_left_block_only() {
        assert_eq!(linear_determinant(Mat4::IDENTITY), 1.0);
        assert_eq!(
            linear_determinant(Mat4::translation(Vec3::new(9.0, 9.0, 9.0))),
            1.0
        );
        assert_eq!(linear_determinant(Mat4::scale(Vec3::new(2.0, 3.0, -1.0))), -6.0);
    }

    #[test]
    fn transposing_the_linear_part_swaps_rows_and_columns() {
        let m = Mat4::from_cols_array([
            1.0, 2.0, 3.0, 0.0, //
            4.0, 5.0, 6.0, 0.0, //
            7.0, 8.0, 9.0, 0.0, //
            10.0, 11.0, 12.0, 1.0,
        ]);
        assert_eq!(
            transpose_linear(m).as_cols_array(),
            [
                1.0, 4.0, 7.0, 0.0, //
                2.0, 5.0, 8.0, 0.0, //
                3.0, 6.0, 9.0, 0.0, //
                0.0, 0.0, 0.0, 1.0,
            ]
        );
    }

    #[test]
    fn winding_selection_covers_both_orders() {
        assert_eq!(wound(&[0, 1, 2, 3, 4, 5], false), vec![0, 1, 2, 3, 4, 5]);
        assert_eq!(wound(&[0, 1, 2, 3, 4, 5], true), vec![0, 2, 1, 3, 5, 4]);
    }
}
