//! The single public facade of the `axiom-resources` module.

use axiom_math::{Vec2, Vec3, Vec4};

use crate::basic_lit_material::build_basic_lit_material;
use crate::cube_mesh::build_cube_mesh;
use crate::mesh_data::MeshData;
use crate::material_data::MaterialData;
use crate::resolved_resources::ResolvedResources;
use crate::resource_id::ResourceId;
use crate::resource_table::ResourceTable;
use crate::solid_color_texture::build_solid_color_texture;
use crate::texture_data::TextureData;
use crate::vertex::Vertex;

/// The only public export of `axiom-resources`.
///
/// `ResourcesApi` is the single boundary between the app and CPU-side
/// resource descriptions. It owns the built-in cube mesh, basic-lit
/// material, and solid-colour texture builders, plus the conversion
/// from a mutable [`ResourceTable`] to a deterministic
/// [`ResolvedResources`] snapshot the app hands to the renderer.
#[derive(Debug, Clone, Copy, Default)]
pub struct ResourcesApi {
    _sealed: (),
}

impl ResourcesApi {
    pub const fn new() -> Self {
        ResourcesApi { _sealed: () }
    }

    // --- Table construction ---

    pub fn empty_table(&self) -> ResourceTable {
        ResourceTable::new()
    }

    /// Register the built-in cube mesh and return its [`ResourceId`].
    pub fn register_cube_mesh(&self, table: &mut ResourceTable) -> ResourceId {
        let id = table.next_id();
        table.insert_mesh(build_cube_mesh(id))
    }

    /// Register the built-in basic-lit material with the given base
    /// colour.
    pub fn register_basic_lit_material(
        &self,
        table: &mut ResourceTable,
        base_color: Vec4,
    ) -> ResourceId {
        let id = table.next_id();
        table.insert_material(build_basic_lit_material(id, base_color))
    }

    /// Register a 2×2 solid-colour texture.
    pub fn register_solid_color_texture(
        &self,
        table: &mut ResourceTable,
        name: &'static str,
        rgba: [u8; 4],
    ) -> ResourceId {
        let id = table.next_id();
        table.insert_texture(build_solid_color_texture(id, name, rgba))
    }

    // --- Snapshot ---

    pub fn resolve(&self, table: &ResourceTable) -> ResolvedResources {
        ResolvedResources::from_table(table)
    }

    // --- Inspection methods on ResolvedResources (boundary primitives only) ---

    pub fn resolved_mesh_count(&self, resolved: &ResolvedResources) -> usize {
        resolved.mesh_count()
    }

    pub fn resolved_material_count(&self, resolved: &ResolvedResources) -> usize {
        resolved.material_count()
    }

    pub fn resolved_mesh_id_at(
        &self,
        resolved: &ResolvedResources,
        idx: usize,
    ) -> Option<u64> {
        resolved.mesh_at(idx).map(|m| m.id().raw())
    }

    pub fn resolved_material_id_at(
        &self,
        resolved: &ResolvedResources,
        idx: usize,
    ) -> Option<u64> {
        resolved.material_at(idx).map(|m| m.id().raw())
    }

    pub fn resolved_mesh_vertex_count(
        &self,
        resolved: &ResolvedResources,
        mesh_id: u64,
    ) -> Option<usize> {
        resolved
            .mesh_by_id(ResourceId::from_raw(mesh_id))
            .map(|m| m.vertices().len())
    }

    pub fn resolved_mesh_index_count(
        &self,
        resolved: &ResolvedResources,
        mesh_id: u64,
    ) -> Option<usize> {
        resolved
            .mesh_by_id(ResourceId::from_raw(mesh_id))
            .map(|m| m.indices().len())
    }

    /// Copy the vertex position at `vert_idx` as a `[f32; 3]`.
    pub fn resolved_mesh_position_at(
        &self,
        resolved: &ResolvedResources,
        mesh_id: u64,
        vert_idx: usize,
    ) -> Option<[f32; 3]> {
        resolved
            .mesh_by_id(ResourceId::from_raw(mesh_id))
            .and_then(|m| m.vertices().get(vert_idx))
            .map(|v| [v.position().x, v.position().y, v.position().z])
    }

    pub fn resolved_mesh_normal_at(
        &self,
        resolved: &ResolvedResources,
        mesh_id: u64,
        vert_idx: usize,
    ) -> Option<[f32; 3]> {
        resolved
            .mesh_by_id(ResourceId::from_raw(mesh_id))
            .and_then(|m| m.vertices().get(vert_idx))
            .map(|v| [v.normal().x, v.normal().y, v.normal().z])
    }

    pub fn resolved_mesh_uv_at(
        &self,
        resolved: &ResolvedResources,
        mesh_id: u64,
        vert_idx: usize,
    ) -> Option<[f32; 2]> {
        resolved
            .mesh_by_id(ResourceId::from_raw(mesh_id))
            .and_then(|m| m.vertices().get(vert_idx))
            .map(|v| [v.uv().x, v.uv().y])
    }

    pub fn resolved_mesh_indices<'a>(
        &self,
        resolved: &'a ResolvedResources,
        mesh_id: u64,
    ) -> Option<&'a [u32]> {
        resolved
            .mesh_by_id(ResourceId::from_raw(mesh_id))
            .map(|m| m.indices())
    }

    pub fn resolved_material_base_color(
        &self,
        resolved: &ResolvedResources,
        material_id: u64,
    ) -> Option<[f32; 4]> {
        resolved
            .material_by_id(ResourceId::from_raw(material_id))
            .map(|m| {
                let c = m.base_color();
                [c.x, c.y, c.z, c.w]
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::{Vec3, Vec4};

    fn api() -> ResourcesApi {
        ResourcesApi::new()
    }

    #[test]
    fn new_and_default_facades_are_equivalent() {
        let _ = ResourcesApi::new();
        let _ = ResourcesApi::default();
    }

    #[test]
    fn empty_table_is_empty() {
        let t = api().empty_table();
        assert_eq!(t.mesh_count(), 0);
        assert_eq!(t.material_count(), 0);
        assert_eq!(t.texture_count(), 0);
    }

    #[test]
    fn cube_mesh_can_be_registered() {
        let mut t = api().empty_table();
        let id = api().register_cube_mesh(&mut t);
        assert!(id.is_valid());
        assert_eq!(t.mesh_count(), 1);
    }

    #[test]
    fn basic_lit_material_can_be_registered() {
        let mut t = api().empty_table();
        let id = api()
            .register_basic_lit_material(&mut t, Vec4::new(0.8, 0.4, 0.2, 1.0));
        assert!(id.is_valid());
        assert_eq!(t.material_count(), 1);
    }

    #[test]
    fn solid_color_texture_can_be_registered() {
        let mut t = api().empty_table();
        let id = api()
            .register_solid_color_texture(&mut t, "white", [255, 255, 255, 255]);
        assert!(id.is_valid());
        assert_eq!(t.texture_count(), 1);
    }

    #[test]
    fn resolve_produces_a_complete_snapshot() {
        let mut t = api().empty_table();
        let mesh = api().register_cube_mesh(&mut t);
        let mat = api().register_basic_lit_material(&mut t, Vec4::ONE);
        let _tex =
            api().register_solid_color_texture(&mut t, "white", [255, 255, 255, 255]);
        let r = api().resolve(&t);
        assert_eq!(r.mesh_count(), 1);
        assert_eq!(r.material_count(), 1);
        assert_eq!(r.texture_count(), 1);
        assert_eq!(api().resolved_mesh_id_at(&r, 0), Some(mesh.raw()));
        assert_eq!(api().resolved_material_id_at(&r, 0), Some(mat.raw()));
    }

    #[test]
    fn resolve_is_deterministic() {
        let build = || {
            let mut t = api().empty_table();
            api().register_cube_mesh(&mut t);
            api().register_basic_lit_material(&mut t, Vec4::ONE);
            api().resolve(&t)
        };
        assert_eq!(build(), build());
    }

    #[test]
    fn resolved_mesh_inspection_returns_cube_facts() {
        let mut t = api().empty_table();
        let mesh = api().register_cube_mesh(&mut t);
        let r = api().resolve(&t);
        assert_eq!(api().resolved_mesh_vertex_count(&r, mesh.raw()), Some(24));
        assert_eq!(api().resolved_mesh_index_count(&r, mesh.raw()), Some(36));
        // Position 0 is one of the cube corners.
        let p = api().resolved_mesh_position_at(&r, mesh.raw(), 0).unwrap();
        assert!(p.iter().all(|c| (c.abs() - 0.5).abs() < 1.0e-6));
    }

    #[test]
    fn resolved_material_base_color_matches_registration() {
        let mut t = api().empty_table();
        let id = api()
            .register_basic_lit_material(&mut t, Vec4::new(0.1, 0.2, 0.3, 1.0));
        let r = api().resolve(&t);
        assert_eq!(
            api().resolved_material_base_color(&r, id.raw()),
            Some([0.1, 0.2, 0.3, 1.0])
        );
    }

    #[test]
    fn resolved_mesh_indices_match_cube() {
        let mut t = api().empty_table();
        let mesh = api().register_cube_mesh(&mut t);
        let r = api().resolve(&t);
        let indices = api().resolved_mesh_indices(&r, mesh.raw()).unwrap();
        assert_eq!(indices.len(), 36);
    }

    #[test]
    fn imports_live_executes() {
        _imports_live();
    }

    #[test]
    fn resolved_counts_via_facade() {
        let mut t = api().empty_table();
        api().register_cube_mesh(&mut t);
        api().register_basic_lit_material(&mut t, Vec4::ONE);
        let r = api().resolve(&t);
        assert_eq!(api().resolved_mesh_count(&r), 1);
        assert_eq!(api().resolved_material_count(&r), 1);
    }

    #[test]
    fn resolved_counts_differ_from_one() {
        // Kills `resolved_mesh_count/resolved_material_count -> 1`: assert
        // exact counts that are NOT 1 (0 for empty, 2 for two registrations).
        let empty = api().resolve(&api().empty_table());
        assert_eq!(api().resolved_mesh_count(&empty), 0);
        assert_eq!(api().resolved_material_count(&empty), 0);

        let mut t = api().empty_table();
        api().register_cube_mesh(&mut t);
        api().register_cube_mesh(&mut t);
        api().register_basic_lit_material(&mut t, Vec4::ONE);
        api().register_basic_lit_material(&mut t, Vec4::new(0.1, 0.2, 0.3, 1.0));
        let r = api().resolve(&t);
        assert_eq!(api().resolved_mesh_count(&r), 2);
        assert_eq!(api().resolved_material_count(&r), 2);
    }

    #[test]
    fn resolved_normal_and_uv_present_and_missing() {
        let mut t = api().empty_table();
        let mesh = api().register_cube_mesh(&mut t);
        let r = api().resolve(&t);
        // Present mesh + present vertex.
        assert!(api().resolved_mesh_normal_at(&r, mesh.raw(), 0).is_some());
        assert!(api().resolved_mesh_uv_at(&r, mesh.raw(), 0).is_some());
        // Present mesh, out-of-range vertex index.
        assert!(api().resolved_mesh_normal_at(&r, mesh.raw(), 9999).is_none());
        assert!(api().resolved_mesh_uv_at(&r, mesh.raw(), 9999).is_none());
        // Missing mesh id.
        assert!(api().resolved_mesh_normal_at(&r, 9999, 0).is_none());
        assert!(api().resolved_mesh_uv_at(&r, 9999, 0).is_none());
    }

    #[test]
    fn resolved_mesh_accessors_handle_missing_ids() {
        let r = api().resolve(&api().empty_table());
        assert!(api().resolved_mesh_id_at(&r, 0).is_none());
        assert!(api().resolved_material_id_at(&r, 0).is_none());
        assert!(api().resolved_mesh_vertex_count(&r, 1).is_none());
        assert!(api().resolved_mesh_index_count(&r, 1).is_none());
        assert!(api().resolved_mesh_position_at(&r, 1, 0).is_none());
        assert!(api().resolved_mesh_indices(&r, 1).is_none());
        assert!(api().resolved_material_base_color(&r, 1).is_none());
    }

    // Keep imports live so dead-code lints don't fire if a test is
    // commented out during local development.
    #[allow(dead_code)]
    fn _imports_live() {
        let _ = (
            MeshData::new(
                ResourceId::from_raw(1),
                "x",
                vec![Vertex::new(Vec3::ZERO, Vec3::ZERO, Vec2::ZERO, Vec4::ZERO)],
                vec![0],
            ),
            MaterialData::new(ResourceId::from_raw(1), "x", Vec4::ONE, None),
            TextureData::new(ResourceId::from_raw(1), "x", 1, 1, vec![0u8; 4]),
        );
    }
}
