//! The mutable CPU-side resource table.

use std::collections::BTreeMap;

use crate::material_data::MaterialData;
use crate::mesh_data::MeshData;
use crate::resource_id::ResourceId;
use crate::texture_data::TextureData;

/// A mutable, deterministic CPU-side resource table.
///
/// IDs are monotonic across the whole table — mesh ids, material ids,
/// and texture ids all draw from the same monotonic counter so a
/// resource ID is globally unique inside one table.
#[derive(Debug, Clone, Default)]
pub struct ResourceTable {
    meshes: BTreeMap<ResourceId, MeshData>,
    materials: BTreeMap<ResourceId, MaterialData>,
    textures: BTreeMap<ResourceId, TextureData>,
    next_id: u64,
}

impl ResourceTable {
    pub fn new() -> Self {
        ResourceTable {
            meshes: BTreeMap::new(),
            materials: BTreeMap::new(),
            textures: BTreeMap::new(),
            next_id: 1,
        }
    }

    pub(crate) fn next_id(&mut self) -> ResourceId {
        let id = ResourceId::from_raw(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    pub(crate) fn insert_mesh(&mut self, mesh: MeshData) -> ResourceId {
        let id = mesh.id();
        self.meshes.insert(id, mesh);
        id
    }

    pub(crate) fn insert_material(&mut self, material: MaterialData) -> ResourceId {
        let id = material.id();
        self.materials.insert(id, material);
        id
    }

    pub(crate) fn insert_texture(&mut self, texture: TextureData) -> ResourceId {
        let id = texture.id();
        self.textures.insert(id, texture);
        id
    }

    pub fn mesh(&self, id: ResourceId) -> Option<&MeshData> {
        self.meshes.get(&id)
    }

    pub fn material(&self, id: ResourceId) -> Option<&MaterialData> {
        self.materials.get(&id)
    }

    pub fn texture(&self, id: ResourceId) -> Option<&TextureData> {
        self.textures.get(&id)
    }

    pub fn meshes_in_order(&self) -> impl Iterator<Item = (ResourceId, &MeshData)> {
        self.meshes.iter().map(|(id, m)| (*id, m))
    }

    pub fn materials_in_order(&self) -> impl Iterator<Item = (ResourceId, &MaterialData)> {
        self.materials.iter().map(|(id, m)| (*id, m))
    }

    pub fn textures_in_order(&self) -> impl Iterator<Item = (ResourceId, &TextureData)> {
        self.textures.iter().map(|(id, t)| (*id, t))
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::basic_lit_material::build_basic_lit_material;
    use crate::mesh_data::test_mesh;
    use axiom_math::Vec4;

    #[test]
    fn new_table_is_empty() {
        let t = ResourceTable::new();
        assert_eq!(t.mesh_count(), 0);
        assert_eq!(t.material_count(), 0);
        assert_eq!(t.texture_count(), 0);
    }

    #[test]
    fn next_id_is_monotonic_across_kinds() {
        let mut t = ResourceTable::new();
        let a = t.next_id();
        let b = t.next_id();
        let c = t.next_id();
        assert!(a.raw() < b.raw());
        assert!(b.raw() < c.raw());
    }

    #[test]
    fn insert_and_lookup_round_trip() {
        let mut t = ResourceTable::new();
        let mesh_id = t.next_id();
        let mat_id = t.next_id();
        t.insert_mesh(test_mesh(mesh_id));
        t.insert_material(build_basic_lit_material(mat_id, Vec4::ONE));
        assert!(t.mesh(mesh_id).is_some());
        assert!(t.material(mat_id).is_some());
    }

    #[test]
    fn iteration_is_in_ascending_id_order() {
        let mut t = ResourceTable::new();
        let a = t.next_id();
        let b = t.next_id();
        t.insert_mesh(test_mesh(b));
        t.insert_mesh(test_mesh(a));
        let ids: Vec<u64> = t.meshes_in_order().map(|(id, _)| id.raw()).collect();
        assert_eq!(ids, vec![a.raw(), b.raw()]);
    }
}

#[cfg(test)]
mod cov {
    use super::*;
    use crate::solid_color_texture::build_solid_color_texture;

    #[test]
    fn texture_insert_lookup_and_order() {
        let mut t = ResourceTable::new();
        let a = t.next_id();
        let b = t.next_id();
        t.insert_texture(build_solid_color_texture(b, "b", [0, 0, 0, 0]));
        t.insert_texture(build_solid_color_texture(a, "a", [1, 1, 1, 1]));
        assert!(t.texture(a).is_some());
        assert!(t.texture(b).is_some());
        assert!(t.texture(ResourceId::from_raw(9999)).is_none());
        assert_eq!(t.texture_count(), 2);
        let ids: Vec<u64> = t.textures_in_order().map(|(id, _)| id.raw()).collect();
        assert_eq!(ids, vec![a.raw(), b.raw()]);
    }

    #[test]
    fn material_missing_lookup_is_none() {
        let t = ResourceTable::new();
        assert!(t.material(ResourceId::from_raw(1)).is_none());
        assert!(t.mesh(ResourceId::from_raw(1)).is_none());
        let _ = t.materials_in_order().count();
    }
}
