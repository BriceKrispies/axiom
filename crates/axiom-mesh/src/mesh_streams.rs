//! The attribute streams a [`crate::Mesh`] is built from and taken apart into.

use axiom_math::{Vec2, Vec3, Vec4};

/// The parallel attribute streams of an indexed triangle mesh, before
/// validation.
///
/// This is the *only* way to build a [`crate::Mesh`], and the shape
/// [`crate::Mesh::into_streams`] hands back. It is a plain value struct with
/// public fields so construction stays immutable and machine-authorable: an
/// agent (or a generator) fills the streams it has and leaves the rest empty.
///
/// **An empty stream means the attribute is absent.** There is no `Option` and
/// no presence bitmask — `normals: vec![]` and "this mesh has no normals" are
/// the same statement. A present stream must be exactly as long as `positions`.
///
/// Use struct-update syntax to name only what you have:
///
/// ```
/// use axiom_math::{Vec2, Vec3};
/// use axiom_mesh::{Mesh, MeshStreams};
///
/// let positions = vec![Vec3::ZERO, Vec3::UNIT_X, Vec3::UNIT_Y];
/// let uvs = vec![Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0)];
/// let mesh = Mesh::from_streams(MeshStreams {
///     uvs,
///     ..MeshStreams::new(positions, vec![0, 1, 2])
/// })
/// .unwrap();
/// assert!(mesh.has_uvs());
/// assert!(!mesh.has_normals());
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeshStreams {
    /// Vertex positions. Required, non-empty, every component finite.
    pub positions: Vec<Vec3>,
    /// Triangle-list indices into `positions`. Required, length divisible by 3.
    pub indices: Vec<u32>,
    /// Per-vertex normals. Empty (absent) or `positions.len()` long.
    pub normals: Vec<Vec3>,
    /// Per-vertex texture coordinates, `(0,0)` at the lower-left. Empty or
    /// `positions.len()` long.
    pub uvs: Vec<Vec2>,
    /// Per-vertex tangents: `xyz` is the tangent direction, `w` is the
    /// bitangent handedness (`+1` or `-1`). Empty or `positions.len()` long.
    pub tangents: Vec<Vec4>,
    /// Per-vertex linear RGBA colour. Empty or `positions.len()` long.
    pub colors: Vec<Vec4>,
    /// Per-vertex skin bone indices (four per vertex). Empty, or
    /// `positions.len()` long together with `weights`.
    pub joints: Vec<[u16; 4]>,
    /// Per-vertex skin blend weights (four per vertex, summing to one). Empty,
    /// or `positions.len()` long together with `joints`.
    pub weights: Vec<[f32; 4]>,
}

impl MeshStreams {
    /// The minimum viable streams: positions and a triangle-list index buffer,
    /// with every optional attribute absent.
    pub fn new(positions: Vec<Vec3>, indices: Vec<u32>) -> Self {
        MeshStreams {
            positions,
            indices,
            ..MeshStreams::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_populates_only_positions_and_indices() {
        let s = MeshStreams::new(vec![Vec3::ZERO; 3], vec![0, 1, 2]);
        assert_eq!(s.positions.len(), 3);
        assert_eq!(s.indices, vec![0, 1, 2]);
        assert!(s.normals.is_empty());
        assert!(s.uvs.is_empty());
        assert!(s.tangents.is_empty());
        assert!(s.colors.is_empty());
        assert!(s.joints.is_empty());
        assert!(s.weights.is_empty());
    }

    #[test]
    fn default_is_entirely_empty() {
        let s = MeshStreams::default();
        assert!(s.positions.is_empty());
        assert!(s.indices.is_empty());
        assert_eq!(s, MeshStreams::new(Vec::new(), Vec::new()));
    }

    #[test]
    fn struct_update_syntax_names_only_the_present_attributes() {
        let s = MeshStreams {
            normals: vec![Vec3::UNIT_Y; 3],
            ..MeshStreams::new(vec![Vec3::ZERO; 3], vec![0, 1, 2])
        };
        assert_eq!(s.normals, vec![Vec3::UNIT_Y; 3]);
        assert!(s.uvs.is_empty());
    }
}
