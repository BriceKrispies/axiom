//! A deterministic snapshot of a [`crate::ResourceTable`].

use crate::material_data::MaterialData;
use crate::mesh_data::MeshData;
use crate::resource_id::ResourceId;
use crate::resource_table::ResourceTable;
use crate::texture_data::TextureData;

/// A deterministic, value-typed snapshot of a [`ResourceTable`]
/// captured at one point in time.
///
/// Lists are ordered by ascending [`ResourceId`]. The snapshot is
/// plain CPU-side data — no GPU buffers, no async, no file IO.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedResources {
    meshes: Vec<MeshData>,
    materials: Vec<MaterialData>,
    textures: Vec<TextureData>,
}

impl ResolvedResources {
    pub fn from_table(table: &ResourceTable) -> Self {
        ResolvedResources {
            meshes: table.meshes_in_order().map(|(_, m)| m.clone()).collect(),
            materials: table
                .materials_in_order()
                .map(|(_, m)| *m)
                .collect(),
            textures: table.textures_in_order().map(|(_, t)| t.clone()).collect(),
        }
    }

    pub fn mesh_count(&self) -> usize {
        self.meshes.len()
    }

    pub fn material_count(&self) -> usize {
        self.materials.len()
    }

    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }

    pub fn mesh_at(&self, idx: usize) -> Option<&MeshData> {
        self.meshes.get(idx)
    }

    pub fn material_at(&self, idx: usize) -> Option<&MaterialData> {
        self.materials.get(idx)
    }

    pub fn texture_at(&self, idx: usize) -> Option<&TextureData> {
        self.textures.get(idx)
    }

    pub fn mesh_by_id(&self, id: ResourceId) -> Option<&MeshData> {
        self.meshes.iter().find(|m| m.id() == id)
    }

    pub fn material_by_id(&self, id: ResourceId) -> Option<&MaterialData> {
        self.materials.iter().find(|m| m.id() == id)
    }

    pub fn meshes(&self) -> &[MeshData] {
        &self.meshes
    }

    pub fn materials(&self) -> &[MaterialData] {
        &self.materials
    }

    pub fn textures(&self) -> &[TextureData] {
        &self.textures
    }

    pub fn is_empty(&self) -> bool {
        self.meshes.is_empty() && self.materials.is_empty() && self.textures.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::basic_lit_material::build_basic_lit_material;
    use crate::cube_mesh::build_cube_mesh;
    use axiom_math::Vec4;

    fn populated_table() -> ResourceTable {
        let mut t = ResourceTable::new();
        let mesh_id = t.next_id();
        let mat_id = t.next_id();
        t.insert_mesh(build_cube_mesh(mesh_id));
        t.insert_material(build_basic_lit_material(
            mat_id,
            Vec4::new(0.8, 0.4, 0.2, 1.0),
        ));
        t
    }

    #[test]
    fn snapshot_of_empty_table_is_empty() {
        let r = ResolvedResources::from_table(&ResourceTable::new());
        assert!(r.is_empty());
    }

    #[test]
    fn snapshot_records_inserted_resources() {
        let r = ResolvedResources::from_table(&populated_table());
        assert_eq!(r.mesh_count(), 1);
        assert_eq!(r.material_count(), 1);
    }

    #[test]
    fn snapshot_is_deterministic_across_runs() {
        let a = ResolvedResources::from_table(&populated_table());
        let b = ResolvedResources::from_table(&populated_table());
        assert_eq!(a, b);
    }

    #[test]
    fn lookup_by_id_works() {
        let r = ResolvedResources::from_table(&populated_table());
        let cube_id = r.mesh_at(0).unwrap().id();
        assert!(r.mesh_by_id(cube_id).is_some());
    }
}
