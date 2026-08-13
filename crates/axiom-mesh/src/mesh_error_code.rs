//! Machine-readable mesh-error code.

/// The reason a mesh construction, validation, or geometry operation failed.
///
/// The kernel error model's enums are closed, so this layer defines its own
/// identity in the same shape as [`axiom_math::MathErrorCode`]: a stable `u16`
/// discriminant that two errors compare on regardless of their human message.
/// The discriminants are part of the layer's contract — append, never reorder.
///
/// The `axiom-mesh-ops` layer reuses these codes rather than defining a second
/// error vocabulary, so the operator-facing codes (`InvalidTessellation`,
/// `InvalidProfile`, ...) live here alongside the representation-facing ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum MeshErrorCode {
    /// A mesh was built with no positions. The empty mesh is not representable:
    /// a `Mesh` always describes at least one vertex.
    EmptyPositions = 1,
    /// A vertex position component was `NaN` or `±Inf`.
    NonFinitePosition = 2,
    /// The index count was not a whole number of triangles (not divisible by 3).
    IndexCountNotTriangular = 3,
    /// An index addressed a vertex at or beyond the position count.
    IndexOutOfRange = 4,
    /// An attribute stream was neither empty (absent) nor exactly as long as the
    /// position stream.
    AttributeLengthMismatch = 5,
    /// A normal, uv, tangent, or colour component was `NaN` or `±Inf`.
    NonFiniteAttribute = 6,
    /// The skin joint and weight streams disagreed: one was present without the
    /// other, or their lengths differed.
    SkinStreamMismatch = 7,
    /// A skin weight row was negative, non-finite, or did not sum to one within
    /// the layer's normalization tolerance.
    SkinWeightsNotNormalized = 8,
    /// A triangle had zero area (two or more coincident corners) where the
    /// operation requires non-degenerate topology.
    DegenerateTriangle = 9,
    /// A numeric parameter was outside its documented domain (negative radius,
    /// zero extent, inverted range).
    InvalidParameter = 10,
    /// A tessellation/detail quantity was outside its documented bounds.
    InvalidTessellation = 11,
    /// A 2D profile polygon was unusable: too few points, duplicate consecutive
    /// points, or zero signed area.
    InvalidProfile = 12,
    /// A sweep path was unusable: fewer than two distinct points, or zero total
    /// arc length.
    InvalidPath = 13,
    /// A sampled grid's declared dimensions did not match its sample count, or a
    /// dimension was below the operator's minimum.
    InvalidGridDimensions = 14,
    /// Two loft sections could not be corresponded: differing point counts or
    /// incompatible open/closed policy.
    IncompatibleProfiles = 15,
    /// Ear-clipping could not triangulate the polygon — it is self-intersecting,
    /// or otherwise not a simple polygon.
    TriangulationFailed = 16,
    /// Binary deserialization could not be completed; the wrapped `KernelError`
    /// preserves the kernel binary-reader cause.
    DeserializationFailed = 17,
    /// The operation would have produced more triangles than the caller's
    /// declared budget allows.
    BudgetExceeded = 18,
    /// A revolution or framing axis was zero-length or collinear with the input
    /// it must be independent of.
    DegenerateAxis = 19,
    /// Tangents could not be generated: the mesh carries no UVs, or every
    /// triangle touching a vertex has a degenerate UV parameterization.
    TangentGenerationFailed = 20,
}

impl MeshErrorCode {
    /// The stable numeric discriminant.
    pub const fn raw(self) -> u16 {
        self as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discriminants_are_stable() {
        assert_eq!(MeshErrorCode::EmptyPositions.raw(), 1);
        assert_eq!(MeshErrorCode::IndexOutOfRange.raw(), 4);
        assert_eq!(MeshErrorCode::DegenerateTriangle.raw(), 9);
        assert_eq!(MeshErrorCode::TriangulationFailed.raw(), 16);
        assert_eq!(MeshErrorCode::TangentGenerationFailed.raw(), 20);
    }

    #[test]
    fn codes_order_and_compare_by_discriminant() {
        assert!(MeshErrorCode::EmptyPositions < MeshErrorCode::IndexOutOfRange);
        assert_eq!(MeshErrorCode::InvalidProfile, MeshErrorCode::InvalidProfile);
        assert_ne!(MeshErrorCode::InvalidProfile, MeshErrorCode::InvalidPath);
    }
}
