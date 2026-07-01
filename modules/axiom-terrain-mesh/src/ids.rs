//! The module's pure value-type vocabulary: the geometry the facade hands back.

use axiom_math::Vec3;

/// A square grid mesh: the neutral geometry a heightfield mesher produces, before
/// any app-side decoration (colour, UVs, world semantics).
///
/// The three arrays are the standard triangle-mesh triple:
/// - [`positions`](GridMesh::positions) — one world-space vertex per grid site, in
///   row-major order (`z` outer, `x` inner);
/// - [`normals`](GridMesh::normals) — the unit central-difference surface normal at
///   each vertex, index-aligned with `positions`;
/// - [`indices`](GridMesh::indices) — the grid triangulation, two triangles per
///   cell, referencing into `positions`.
///
/// It carries geometry data, not engine state: a caller reads the three slices and
/// turns them into whatever renderable vertex layout it needs.
#[derive(Debug, Clone, PartialEq)]
pub struct GridMesh {
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    indices: Vec<u32>,
}

impl GridMesh {
    /// Assemble a mesh from its three index-aligned arrays. Crate-internal: only
    /// the facade builds a `GridMesh`, guaranteeing `positions` and `normals` stay
    /// the same length and every index is in range.
    pub(crate) fn new(positions: Vec<Vec3>, normals: Vec<Vec3>, indices: Vec<u32>) -> Self {
        Self {
            positions,
            normals,
            indices,
        }
    }

    /// The world-space vertex positions, row-major (`z` outer, `x` inner).
    pub fn positions(&self) -> &[Vec3] {
        &self.positions
    }

    /// The unit surface normals, index-aligned with [`positions`](GridMesh::positions).
    pub fn normals(&self) -> &[Vec3] {
        &self.normals
    }

    /// The triangle indices (two triangles per grid cell) into
    /// [`positions`](GridMesh::positions).
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }
}
