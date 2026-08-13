//! The structural + finite-value contract every [`crate::Mesh`] satisfies.
//!
//! This is the single gate through which streams become a mesh. It is exposed
//! publicly so a caller can check candidate streams without constructing, and
//! so the invariants are testable in isolation from the type.

use axiom_math::{Vec2, Vec3, Vec4};

use crate::mesh_error::MeshError;
use crate::mesh_error_code::MeshErrorCode;
use crate::mesh_result::MeshResult;
use crate::mesh_streams::MeshStreams;

/// How far a skin-weight row's sum may stray from `1.0` and still count as
/// normalized. Generous enough for `f32` accumulation over four weights,
/// tight enough that an unnormalized row (e.g. summing to `0.5`) is rejected.
pub const SKIN_WEIGHT_TOLERANCE: f32 = 1.0e-3;

/// Whether an optional attribute stream's length is legal for `vertex_count`:
/// either absent (empty) or exactly one entry per vertex.
const fn aligned(len: usize, vertex_count: usize) -> bool {
    (len == 0) | (len == vertex_count)
}

const fn vec2_finite(v: Vec2) -> bool {
    v.x.is_finite() & v.y.is_finite()
}

const fn vec3_finite(v: Vec3) -> bool {
    v.x.is_finite() & v.y.is_finite() & v.z.is_finite()
}

const fn vec4_finite(v: Vec4) -> bool {
    v.x.is_finite() & v.y.is_finite() & v.z.is_finite() & v.w.is_finite()
}

fn length_mismatch() -> MeshError {
    MeshError::new(
        MeshErrorCode::AttributeLengthMismatch,
        "an attribute stream must be empty (absent) or exactly one entry per vertex",
    )
}

fn non_finite_attribute() -> MeshError {
    MeshError::new(
        MeshErrorCode::NonFiniteAttribute,
        "every attribute component must be finite (no NaN, no Inf)",
    )
}

/// Check one optional stream: aligned length, then finite entries.
fn check_stream<T: Copy>(
    stream: &[T],
    vertex_count: usize,
    finite: fn(T) -> bool,
) -> MeshResult<()> {
    aligned(stream.len(), vertex_count)
        .then_some(())
        .ok_or_else(length_mismatch)
        .and_then(|()| {
            stream
                .iter()
                .copied()
                .all(finite)
                .then_some(())
                .ok_or_else(non_finite_attribute)
        })
}

/// Positions must exist: a `Mesh` always describes at least one vertex.
fn check_positions(positions: &[Vec3]) -> MeshResult<()> {
    (!positions.is_empty())
        .then_some(())
        .ok_or_else(|| {
            MeshError::new(
                MeshErrorCode::EmptyPositions,
                "a mesh must have at least one position",
            )
        })
        .and_then(|()| {
            positions
                .iter()
                .copied()
                .all(vec3_finite)
                .then_some(())
                .ok_or_else(|| {
                    MeshError::new(
                        MeshErrorCode::NonFinitePosition,
                        "every vertex position component must be finite (no NaN, no Inf)",
                    )
                })
        })
}

/// The index buffer must be a whole number of triangles, every index in range.
fn check_indices(indices: &[u32], vertex_count: usize) -> MeshResult<()> {
    indices
        .len()
        .is_multiple_of(3)
        .then_some(())
        .ok_or_else(|| {
            MeshError::new(
                MeshErrorCode::IndexCountNotTriangular,
                "the index count must be a whole number of triangles (divisible by 3)",
            )
        })
        .and_then(|()| {
            indices
                .iter()
                .all(|&i| (i as usize) < vertex_count)
                .then_some(())
                .ok_or_else(|| {
                    MeshError::new(
                        MeshErrorCode::IndexOutOfRange,
                        "every index must address a vertex below the position count",
                    )
                })
        })
}

/// Whether one skin-weight row is usable: finite, non-negative, sums to one.
fn weight_row_normalized(row: [f32; 4]) -> bool {
    let finite = row.iter().all(|w| w.is_finite());
    let non_negative = row.iter().all(|w| *w >= 0.0);
    let sum: f32 = row.iter().sum();
    finite & non_negative & ((sum - 1.0).abs() <= SKIN_WEIGHT_TOLERANCE)
}

/// The joint and weight streams are present together or absent together, and
/// every present weight row is normalized.
fn check_skin(joints: &[[u16; 4]], weights: &[[f32; 4]], vertex_count: usize) -> MeshResult<()> {
    let both_absent = joints.is_empty() & weights.is_empty();
    let both_present = (joints.len() == vertex_count) & (weights.len() == vertex_count);
    (both_absent | both_present)
        .then_some(())
        .ok_or_else(|| {
            MeshError::new(
                MeshErrorCode::SkinStreamMismatch,
                "skin joints and weights must both be absent or both be one entry per vertex",
            )
        })
        .and_then(|()| {
            weights
                .iter()
                .copied()
                .all(weight_row_normalized)
                .then_some(())
                .ok_or_else(|| {
                    MeshError::new(
                        MeshErrorCode::SkinWeightsNotNormalized,
                        "every skin weight row must be finite, non-negative, and sum to one",
                    )
                })
        })
}

/// Validate candidate streams against the full mesh contract.
///
/// Checks, in order: positions non-empty and finite; index count triangular and
/// every index in range; each optional stream aligned and finite; the skin
/// streams paired and normalized. The first failure wins, so the reported code
/// is stable for a given input.
pub fn validate_streams(streams: &MeshStreams) -> MeshResult<()> {
    let n = streams.positions.len();
    check_positions(&streams.positions)
        .and_then(|()| check_indices(&streams.indices, n))
        .and_then(|()| check_stream(&streams.normals, n, vec3_finite))
        .and_then(|()| check_stream(&streams.uvs, n, vec2_finite))
        .and_then(|()| check_stream(&streams.tangents, n, vec4_finite))
        .and_then(|()| check_stream(&streams.colors, n, vec4_finite))
        .and_then(|()| check_skin(&streams.joints, &streams.weights, n))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tri() -> MeshStreams {
        MeshStreams::new(
            vec![Vec3::ZERO, Vec3::UNIT_X, Vec3::UNIT_Y],
            vec![0, 1, 2],
        )
    }

    #[test]
    fn a_minimal_triangle_validates() {
        assert_eq!(validate_streams(&tri()), Ok(()));
    }

    #[test]
    fn an_index_buffer_may_be_empty() {
        // Zero triangles is a whole number of triangles. A mesh with vertices
        // and no faces is structurally legal (a point cloud awaiting topology).
        let s = MeshStreams::new(vec![Vec3::ZERO], Vec::new());
        assert_eq!(validate_streams(&s), Ok(()));
    }

    #[test]
    fn empty_positions_are_rejected() {
        let s = MeshStreams::new(Vec::new(), Vec::new());
        assert_eq!(
            validate_streams(&s).unwrap_err().code(),
            MeshErrorCode::EmptyPositions
        );
    }

    #[test]
    fn a_non_finite_position_is_rejected() {
        let s = MeshStreams::new(vec![Vec3::new(f32::NAN, 0.0, 0.0)], Vec::new());
        assert_eq!(
            validate_streams(&s).unwrap_err().code(),
            MeshErrorCode::NonFinitePosition
        );
        let s = MeshStreams::new(vec![Vec3::new(0.0, f32::INFINITY, 0.0)], Vec::new());
        assert_eq!(
            validate_streams(&s).unwrap_err().code(),
            MeshErrorCode::NonFinitePosition
        );
    }

    #[test]
    fn a_non_triangular_index_count_is_rejected() {
        let s = MeshStreams::new(vec![Vec3::ZERO; 3], vec![0, 1]);
        assert_eq!(
            validate_streams(&s).unwrap_err().code(),
            MeshErrorCode::IndexCountNotTriangular
        );
    }

    #[test]
    fn an_out_of_range_index_is_rejected() {
        let s = MeshStreams::new(vec![Vec3::ZERO; 3], vec![0, 1, 3]);
        assert_eq!(
            validate_streams(&s).unwrap_err().code(),
            MeshErrorCode::IndexOutOfRange
        );
    }

    #[test]
    fn a_misaligned_attribute_stream_is_rejected() {
        let s = MeshStreams {
            normals: vec![Vec3::UNIT_Y; 2],
            ..tri()
        };
        assert_eq!(
            validate_streams(&s).unwrap_err().code(),
            MeshErrorCode::AttributeLengthMismatch
        );
    }

    #[test]
    fn a_non_finite_normal_uv_tangent_or_colour_is_rejected() {
        let bad_normal = MeshStreams {
            normals: vec![Vec3::new(f32::NAN, 0.0, 0.0), Vec3::UNIT_Y, Vec3::UNIT_Y],
            ..tri()
        };
        assert_eq!(
            validate_streams(&bad_normal).unwrap_err().code(),
            MeshErrorCode::NonFiniteAttribute
        );

        let bad_uv = MeshStreams {
            uvs: vec![Vec2::new(0.0, f32::INFINITY), Vec2::ZERO, Vec2::ZERO],
            ..tri()
        };
        assert_eq!(
            validate_streams(&bad_uv).unwrap_err().code(),
            MeshErrorCode::NonFiniteAttribute
        );

        let bad_tangent = MeshStreams {
            tangents: vec![Vec4::new(0.0, 0.0, 0.0, f32::NAN), Vec4::ZERO, Vec4::ZERO],
            ..tri()
        };
        assert_eq!(
            validate_streams(&bad_tangent).unwrap_err().code(),
            MeshErrorCode::NonFiniteAttribute
        );

        let bad_color = MeshStreams {
            colors: vec![Vec4::new(f32::NEG_INFINITY, 0.0, 0.0, 1.0), Vec4::ZERO, Vec4::ZERO],
            ..tri()
        };
        assert_eq!(
            validate_streams(&bad_color).unwrap_err().code(),
            MeshErrorCode::NonFiniteAttribute
        );
    }

    #[test]
    fn aligned_streams_of_every_kind_validate() {
        let s = MeshStreams {
            normals: vec![Vec3::UNIT_Y; 3],
            uvs: vec![Vec2::ZERO; 3],
            tangents: vec![Vec4::new(1.0, 0.0, 0.0, 1.0); 3],
            colors: vec![Vec4::ONE; 3],
            joints: vec![[0, 1, 2, 3]; 3],
            weights: vec![[0.5, 0.25, 0.25, 0.0]; 3],
            ..tri()
        };
        assert_eq!(validate_streams(&s), Ok(()));
    }

    #[test]
    fn skin_streams_must_be_paired() {
        let joints_only = MeshStreams {
            joints: vec![[0, 0, 0, 0]; 3],
            ..tri()
        };
        assert_eq!(
            validate_streams(&joints_only).unwrap_err().code(),
            MeshErrorCode::SkinStreamMismatch
        );

        let weights_only = MeshStreams {
            weights: vec![[1.0, 0.0, 0.0, 0.0]; 3],
            ..tri()
        };
        assert_eq!(
            validate_streams(&weights_only).unwrap_err().code(),
            MeshErrorCode::SkinStreamMismatch
        );

        let short = MeshStreams {
            joints: vec![[0, 0, 0, 0]; 2],
            weights: vec![[1.0, 0.0, 0.0, 0.0]; 2],
            ..tri()
        };
        assert_eq!(
            validate_streams(&short).unwrap_err().code(),
            MeshErrorCode::SkinStreamMismatch
        );
    }

    #[test]
    fn unnormalized_negative_or_non_finite_weights_are_rejected() {
        let sums_to_half = MeshStreams {
            joints: vec![[0, 0, 0, 0]; 3],
            weights: vec![[0.5, 0.0, 0.0, 0.0]; 3],
            ..tri()
        };
        assert_eq!(
            validate_streams(&sums_to_half).unwrap_err().code(),
            MeshErrorCode::SkinWeightsNotNormalized
        );

        let negative = MeshStreams {
            joints: vec![[0, 0, 0, 0]; 3],
            weights: vec![[1.5, -0.5, 0.0, 0.0]; 3],
            ..tri()
        };
        assert_eq!(
            validate_streams(&negative).unwrap_err().code(),
            MeshErrorCode::SkinWeightsNotNormalized
        );

        let nan = MeshStreams {
            joints: vec![[0, 0, 0, 0]; 3],
            weights: vec![[f32::NAN, 0.0, 0.0, 0.0]; 3],
            ..tri()
        };
        assert_eq!(
            validate_streams(&nan).unwrap_err().code(),
            MeshErrorCode::SkinWeightsNotNormalized
        );
    }

    #[test]
    fn a_weight_row_within_tolerance_is_accepted() {
        let s = MeshStreams {
            joints: vec![[0, 0, 0, 0]; 3],
            weights: vec![[1.0 - SKIN_WEIGHT_TOLERANCE * 0.5, 0.0, 0.0, 0.0]; 3],
            ..tri()
        };
        assert_eq!(validate_streams(&s), Ok(()));
    }
}
