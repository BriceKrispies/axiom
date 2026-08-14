//! [`SceneObject`]: one generated thing in the scene, and the record of which
//! operator generated it.
//!
//! The `operator` field is not decoration. This app exists to prove that every
//! visible shape came out of `axiom-mesh-ops`, and the only way a reader (or a
//! test, or the on-page legend) can check that claim without re-reading the
//! builders is if each object carries the name of the operator chain that
//! produced it. An object with no operator string would be an object nobody can
//! audit.
//!
//! Geometry is authored in the object's own local space and placed by
//! `placement`, so the mesh a test digests is the mesh the operator produced —
//! not the mesh plus wherever the scene happens to put it this week.

use axiom_math::Transform;
use axiom_mesh::Mesh;

/// One generated object: its geometry, the operator chain that built it, where
/// it sits, and the linear colour that makes it read apart from its neighbours.
#[derive(Debug, Clone)]
pub struct SceneObject {
    /// A stable identifier, unique within the scene. Tests address objects by it.
    pub name: &'static str,
    /// The `axiom-mesh-ops` / `axiom-mesh` chain that produced `mesh`.
    pub operator: &'static str,
    /// The generated geometry, in the object's own local space.
    pub mesh: Mesh,
    /// Where the object sits in the world.
    pub placement: Transform,
    /// Linear RGB base colour.
    pub color: [f32; 3],
}

impl SceneObject {
    /// Record a generated mesh with its provenance, placement and colour.
    pub fn new(
        name: &'static str,
        operator: &'static str,
        mesh: Mesh,
        placement: Transform,
        color: [f32; 3],
    ) -> Self {
        SceneObject {
            name,
            operator,
            mesh,
            placement,
            color,
        }
    }
}
