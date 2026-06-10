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
            materials: table.materials_in_order().map(|(_, m)| *m).collect(),
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

#[cfg(test)]
mod cov {
    use super::*;
    use crate::basic_lit_material::build_basic_lit_material;
    use crate::cube_mesh::build_cube_mesh;
    use crate::solid_color_texture::build_solid_color_texture;
    use axiom_math::Vec4;

    fn table_with_texture() -> ResourceTable {
        let mut t = ResourceTable::new();
        let mesh_id = t.next_id();
        let mat_id = t.next_id();
        let tex_id = t.next_id();
        t.insert_mesh(build_cube_mesh(mesh_id));
        t.insert_material(build_basic_lit_material(mat_id, Vec4::ONE));
        t.insert_texture(build_solid_color_texture(
            tex_id,
            "white",
            [255, 255, 255, 255],
        ));
        t
    }

    #[test]
    fn texture_accessors_and_slices() {
        let r = ResolvedResources::from_table(&table_with_texture());
        assert_eq!(r.texture_count(), 1);
        assert!(r.texture_at(0).is_some());
        assert!(r.texture_at(99).is_none());
        assert_eq!(r.meshes().len(), 1);
        assert_eq!(r.materials().len(), 1);
        assert_eq!(r.textures().len(), 1);
    }

    #[test]
    fn material_by_id_present_and_missing() {
        let r = ResolvedResources::from_table(&table_with_texture());
        let mat_id = r.material_at(0).unwrap().id();
        assert!(r.material_by_id(mat_id).is_some());
        assert!(r.material_by_id(ResourceId::from_raw(9999)).is_none());
        assert!(r.mesh_by_id(ResourceId::from_raw(9999)).is_none());
    }

    #[test]
    fn counts_differ_from_one_for_empty_and_for_two() {
        // Kills `mesh_count/material_count/texture_count -> 1`: assert exact
        // counts that are NOT 1 (0 for empty, 2 for a doubly-populated table).
        let empty = ResolvedResources::from_table(&ResourceTable::new());
        assert_eq!(empty.mesh_count(), 0);
        assert_eq!(empty.material_count(), 0);
        assert_eq!(empty.texture_count(), 0);

        let mut t = ResourceTable::new();
        let m1 = t.next_id();
        let m2 = t.next_id();
        t.insert_mesh(build_cube_mesh(m1));
        t.insert_mesh(build_cube_mesh(m2));
        let mat1 = t.next_id();
        let mat2 = t.next_id();
        t.insert_material(build_basic_lit_material(mat1, Vec4::ONE));
        t.insert_material(build_basic_lit_material(mat2, Vec4::ONE));
        let tex1 = t.next_id();
        let tex2 = t.next_id();
        t.insert_texture(build_solid_color_texture(tex1, "a", [1, 2, 3, 4]));
        t.insert_texture(build_solid_color_texture(tex2, "b", [5, 6, 7, 8]));
        let r = ResolvedResources::from_table(&t);
        assert_eq!(r.mesh_count(), 2);
        assert_eq!(r.material_count(), 2);
        assert_eq!(r.texture_count(), 2);
    }

    #[test]
    fn is_empty_false_when_populated() {
        let r = ResolvedResources::from_table(&table_with_texture());
        assert!(!r.is_empty());
        let empty = ResolvedResources::from_table(&ResourceTable::new());
        assert!(empty.is_empty());
    }

    // `is_empty` is `meshes.is_empty() && materials.is_empty() &&
    // textures.is_empty()`. To exercise both arms of every `&&` operand we
    // need cases where each operand is the first non-empty collection.
    #[test]
    fn is_empty_short_circuits_on_each_operand() {
        // meshes-only: first operand false -> short-circuits.
        let mut tm = ResourceTable::new();
        let id = tm.next_id();
        tm.insert_mesh(build_cube_mesh(id));
        assert!(!ResolvedResources::from_table(&tm).is_empty());

        // materials-only: meshes empty (true), materials non-empty (false).
        let mut tmat = ResourceTable::new();
        let id = tmat.next_id();
        tmat.insert_material(build_basic_lit_material(id, Vec4::ONE));
        assert!(!ResolvedResources::from_table(&tmat).is_empty());

        // textures-only: meshes+materials empty (true), textures non-empty.
        let mut ttex = ResourceTable::new();
        let id = ttex.next_id();
        ttex.insert_texture(build_solid_color_texture(id, "x", [0, 0, 0, 0]));
        assert!(!ResolvedResources::from_table(&ttex).is_empty());
    }
}
