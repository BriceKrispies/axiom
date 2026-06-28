//! The single public facade of the `axiom-resources` module.

use axiom_math::{Vec2, Vec3, Vec4};

use crate::basic_lit_material::{build_basic_lit_material, build_textured_lit_material};
use crate::biome_atlas_texture::{biome_cell_origin, build_biome_atlas_texture};
use crate::checker_texture::build_checker_texture;
use crate::cube_mesh::build_cube_mesh;
use crate::cylinder_mesh::build_cylinder_mesh;
use crate::mesh_data::{MeshData, MeshInputVertex};
use crate::plane_mesh::build_plane_mesh;
use crate::resolved_resources::ResolvedResources;
use crate::resource_id::ResourceId;
use crate::resource_table::ResourceTable;
use crate::solid_color_texture::build_solid_color_texture;
use crate::sphere_mesh::build_sphere_mesh;
use crate::uv_grid_texture::build_uv_grid_texture;
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

    /// Register an arbitrary triangle mesh from neutral geometry and return its
    /// [`ResourceId`]. This is the engine's general, shape-agnostic mesh
    /// capability: `vertices` is a slice of `(position, normal, uv, color)`
    /// float tuples and `indices` is a triangle-list into them. `name` is debug
    /// metadata. Built-in primitives such as [`Self::register_cube_mesh`] are
    /// thin generators layered on this one path — the resource module knows no
    /// shapes of its own.
    pub fn register_mesh(
        &self,
        table: &mut ResourceTable,
        name: &'static str,
        vertices: &[MeshInputVertex],
        indices: &[u32],
    ) -> ResourceId {
        let id = table.next_id();
        let verts: Vec<Vertex> = vertices
            .iter()
            .map(|&(p, n, uv, c)| {
                Vertex::new(
                    Vec3::new(p[0], p[1], p[2]),
                    Vec3::new(n[0], n[1], n[2]),
                    Vec2::new(uv[0], uv[1]),
                    Vec4::new(c[0], c[1], c[2], c[3]),
                )
            })
            .collect();
        table.insert_mesh(MeshData::new(id, name, verts, indices.to_vec()))
    }

    /// Register the built-in unit cube mesh and return its [`ResourceId`]. A
    /// thin generator over [`Self::register_mesh`]: the cube is a primitive, not
    /// a special case the resource table bakes in.
    pub fn register_cube_mesh(&self, table: &mut ResourceTable) -> ResourceId {
        let (vertices, indices) = build_cube_mesh();
        self.register_mesh(table, "axiom.builtin.cube", &vertices, &indices)
    }

    /// Register the built-in unit plane (quad) mesh and return its [`ResourceId`].
    /// A 1x1 quad in the XZ plane facing +Y — scale it into a ground plane.
    pub fn register_plane_mesh(&self, table: &mut ResourceTable) -> ResourceId {
        let (vertices, indices) = build_plane_mesh();
        self.register_mesh(table, "axiom.builtin.plane", &vertices, &indices)
    }

    /// Register the built-in unit UV-sphere mesh and return its [`ResourceId`].
    /// Radius 0.5 (diameter 1, matching the unit cube), smooth normals.
    pub fn register_sphere_mesh(&self, table: &mut ResourceTable) -> ResourceId {
        let (vertices, indices) = build_sphere_mesh();
        self.register_mesh(table, "axiom.builtin.sphere", &vertices, &indices)
    }

    /// Register the built-in unit cylinder mesh and return its [`ResourceId`].
    /// Radius 0.5, height 1 (±0.5 on Y), radial side wall + two end caps.
    pub fn register_cylinder_mesh(&self, table: &mut ResourceTable) -> ResourceId {
        let (vertices, indices) = build_cylinder_mesh();
        self.register_mesh(table, "axiom.builtin.cylinder", &vertices, &indices)
    }

    /// Register the built-in basic-lit material with the given base
    /// colour and no texture.
    pub fn register_basic_lit_material(
        &self,
        table: &mut ResourceTable,
        base_color: Vec4,
    ) -> ResourceId {
        let id = table.next_id();
        table.insert_material(build_basic_lit_material(id, base_color))
    }

    /// Register a basic-lit material with the given base colour and an
    /// optional albedo texture id (sampled × base colour × vertex colour).
    pub fn register_textured_lit_material(
        &self,
        table: &mut ResourceTable,
        base_color: Vec4,
        texture: Option<ResourceId>,
    ) -> ResourceId {
        let id = table.next_id();
        table.insert_material(build_textured_lit_material(id, base_color, texture))
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

    /// Register the built-in checkerboard texture (`a` is the origin cell
    /// colour, `b` alternates with it). Tinted by the material base colour.
    pub fn register_checker_texture(
        &self,
        table: &mut ResourceTable,
        name: &'static str,
        a: [u8; 4],
        b: [u8; 4],
    ) -> ResourceId {
        let id = table.next_id();
        table.insert_texture(build_checker_texture(id, name, a, b))
    }

    /// Register the built-in UV-grid texture (gradient + white grid lines).
    pub fn register_uv_grid_texture(
        &self,
        table: &mut ResourceTable,
        name: &'static str,
    ) -> ResourceId {
        let id = table.next_id();
        table.insert_texture(build_uv_grid_texture(id, name))
    }

    /// Register the built-in 2×2 biome atlas texture (sand/grass/rock/snow).
    pub fn register_biome_atlas_texture(
        &self,
        table: &mut ResourceTable,
        name: &'static str,
    ) -> ResourceId {
        let id = table.next_id();
        table.insert_texture(build_biome_atlas_texture(id, name))
    }

    /// The top-left UV of biome `biome`'s cell in the built-in biome atlas (a 2×2
    /// packing of sand/grass/rock/snow). A terrain vertex tagged with `biome`
    /// samples that biome by offsetting a fractional position within the
    /// `0.5 × 0.5` cell starting here. Out-of-range biome ids wrap into the grid.
    pub fn biome_atlas_cell_origin(&self, biome: u32) -> (f32, f32) {
        biome_cell_origin(biome)
    }

    // --- Snapshot ---

    pub fn resolve(&self, table: &ResourceTable) -> ResolvedResources {
        ResolvedResources::from_table(table)
    }
}

/// Inspection of a resolved snapshot: boundary primitives the app reads back
/// without naming any resource-internal type. Split into its own `impl` block (one
/// facade, grouped by responsibility) so neither block grows unwieldy.
impl ResourcesApi {
    // --- Inspection methods on ResolvedResources (boundary primitives only) ---

    pub fn resolved_mesh_count(&self, resolved: &ResolvedResources) -> usize {
        resolved.mesh_count()
    }

    pub fn resolved_material_count(&self, resolved: &ResolvedResources) -> usize {
        resolved.material_count()
    }

    pub fn resolved_mesh_id_at(&self, resolved: &ResolvedResources, idx: usize) -> Option<u64> {
        resolved.mesh_at(idx).map(|m| m.id().raw())
    }

    pub fn resolved_material_id_at(&self, resolved: &ResolvedResources, idx: usize) -> Option<u64> {
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

    /// The albedo texture id a material samples, if any. `Some(None)` is
    /// flattened to `None` — a missing material and an untextured material
    /// both report "no texture".
    pub fn resolved_material_texture_id(
        &self,
        resolved: &ResolvedResources,
        material_id: u64,
    ) -> Option<u64> {
        resolved
            .material_by_id(ResourceId::from_raw(material_id))
            .and_then(|m| m.texture())
            .map(|t| t.raw())
    }

    pub fn resolved_texture_count(&self, resolved: &ResolvedResources) -> usize {
        resolved.texture_count()
    }

    pub fn resolved_texture_id_at(&self, resolved: &ResolvedResources, idx: usize) -> Option<u64> {
        resolved.texture_at(idx).map(|t| t.id().raw())
    }

    pub fn resolved_texture_width(
        &self,
        resolved: &ResolvedResources,
        texture_id: u64,
    ) -> Option<u32> {
        resolved
            .texture_by_id(ResourceId::from_raw(texture_id))
            .map(|t| t.width())
    }

    pub fn resolved_texture_height(
        &self,
        resolved: &ResolvedResources,
        texture_id: u64,
    ) -> Option<u32> {
        resolved
            .texture_by_id(ResourceId::from_raw(texture_id))
            .map(|t| t.height())
    }

    pub fn resolved_texture_pixels<'a>(
        &self,
        resolved: &'a ResolvedResources,
        texture_id: u64,
    ) -> Option<&'a [u8]> {
        resolved
            .texture_by_id(ResourceId::from_raw(texture_id))
            .map(|t| t.rgba8_pixels())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::Vec4;

    fn api() -> ResourcesApi {
        ResourcesApi::new()
    }

    #[test]
    fn new_and_default_facades_are_equivalent() {
        // Both construction paths produce an equivalent empty table.
        assert_eq!(
            ResourcesApi::new().empty_table().mesh_count(),
            ResourcesApi::default().empty_table().mesh_count(),
        );
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
    fn plane_and_sphere_primitives_can_be_registered_and_resolve() {
        let api = api();
        let mut t = api.empty_table();
        let plane = api.register_plane_mesh(&mut t);
        let sphere = api.register_sphere_mesh(&mut t);
        assert!(plane.is_valid());
        assert!(sphere.is_valid());
        assert_eq!(t.mesh_count(), 2);
        let resolved = api.resolve(&t);
        // The plane is a 4-vertex quad; the sphere has many more vertices.
        assert_eq!(
            api.resolved_mesh_vertex_count(&resolved, plane.raw()),
            Some(4)
        );
        assert!(
            api.resolved_mesh_vertex_count(&resolved, sphere.raw())
                .unwrap()
                > 100
        );
    }

    #[test]
    fn arbitrary_mesh_can_be_registered_through_the_general_path() {
        // A non-cube shape (a single triangle) proves the resource module is
        // shape-agnostic: any neutral geometry round-trips through the snapshot.
        let mut t = api().empty_table();
        let tri = [
            (
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                [0.0, 0.0],
                [1.0, 0.0, 0.0, 1.0],
            ),
            (
                [2.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                [1.0, 0.0],
                [0.0, 1.0, 0.0, 1.0],
            ),
            (
                [0.0, 3.0, 0.0],
                [0.0, 0.0, 1.0],
                [0.0, 1.0],
                [0.0, 0.0, 1.0, 1.0],
            ),
        ];
        let id = api().register_mesh(&mut t, "triangle", &tri, &[0, 1, 2]);
        assert!(id.is_valid());

        let r = api().resolve(&t);
        // Not the cube's 24/36 — the registered shape's own counts.
        assert_eq!(api().resolved_mesh_vertex_count(&r, id.raw()), Some(3));
        assert_eq!(api().resolved_mesh_index_count(&r, id.raw()), Some(3));
        // Position, normal, and uv all thread through unchanged.
        assert_eq!(
            api().resolved_mesh_position_at(&r, id.raw(), 1),
            Some([2.0, 0.0, 0.0])
        );
        assert_eq!(
            api().resolved_mesh_normal_at(&r, id.raw(), 0),
            Some([0.0, 0.0, 1.0])
        );
        assert_eq!(api().resolved_mesh_uv_at(&r, id.raw(), 2), Some([0.0, 1.0]));
        assert_eq!(
            api().resolved_mesh_indices(&r, id.raw()),
            Some(&[0, 1, 2][..])
        );
    }

    #[test]
    fn register_mesh_is_deterministic() {
        let tri = [
            (
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                [0.0, 0.0],
                [1.0, 1.0, 1.0, 1.0],
            ),
            (
                [1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                [1.0, 0.0],
                [1.0, 1.0, 1.0, 1.0],
            ),
            (
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
                [0.0, 1.0],
                [1.0, 1.0, 1.0, 1.0],
            ),
        ];
        let build = || {
            let mut t = api().empty_table();
            api().register_mesh(&mut t, "triangle", &tri, &[0, 1, 2]);
            api().resolve(&t)
        };
        assert_eq!(build(), build());
    }

    #[test]
    fn cube_is_the_general_path_with_cube_geometry() {
        // register_cube_mesh is a thin generator over register_mesh: the cube
        // produces the same 24/36 it always did, now through the general path.
        let mut t = api().empty_table();
        let id = api().register_cube_mesh(&mut t);
        let r = api().resolve(&t);
        assert_eq!(api().resolved_mesh_vertex_count(&r, id.raw()), Some(24));
        assert_eq!(api().resolved_mesh_index_count(&r, id.raw()), Some(36));
    }

    #[test]
    fn basic_lit_material_can_be_registered() {
        let mut t = api().empty_table();
        let id = api().register_basic_lit_material(&mut t, Vec4::new(0.8, 0.4, 0.2, 1.0));
        assert!(id.is_valid());
        assert_eq!(t.material_count(), 1);
    }

    #[test]
    fn solid_color_texture_can_be_registered() {
        let mut t = api().empty_table();
        let id = api().register_solid_color_texture(&mut t, "white", [255, 255, 255, 255]);
        assert!(id.is_valid());
        assert_eq!(t.texture_count(), 1);
    }

    #[test]
    fn resolve_produces_a_complete_snapshot() {
        let mut t = api().empty_table();
        let mesh = api().register_cube_mesh(&mut t);
        let mat = api().register_basic_lit_material(&mut t, Vec4::ONE);
        let _tex = api().register_solid_color_texture(&mut t, "white", [255, 255, 255, 255]);
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
        let id = api().register_basic_lit_material(&mut t, Vec4::new(0.1, 0.2, 0.3, 1.0));
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
        assert!(api()
            .resolved_mesh_normal_at(&r, mesh.raw(), 9999)
            .is_none());
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

    #[test]
    fn procedural_textures_can_be_registered() {
        let api = api();
        let mut t = api.empty_table();
        let checker = api.register_checker_texture(&mut t, "checker", [255; 4], [60, 60, 60, 255]);
        let uv = api.register_uv_grid_texture(&mut t, "uv");
        let biomes = api.register_biome_atlas_texture(&mut t, "biomes");
        assert!(checker.is_valid() & uv.is_valid() & biomes.is_valid());
        assert_eq!(t.texture_count(), 3);
    }

    #[test]
    fn textured_material_threads_its_texture_id() {
        let api = api();
        let mut t = api.empty_table();
        let tex = api.register_checker_texture(&mut t, "checker", [255; 4], [0, 0, 0, 255]);
        let mat = api.register_textured_lit_material(&mut t, Vec4::ONE, Some(tex));
        let untextured = api.register_textured_lit_material(&mut t, Vec4::ONE, None);
        let r = api.resolve(&t);
        assert_eq!(
            api.resolved_material_texture_id(&r, mat.raw()),
            Some(tex.raw())
        );
        // An untextured material and a missing material both report no texture.
        assert_eq!(api.resolved_material_texture_id(&r, untextured.raw()), None);
        assert_eq!(api.resolved_material_texture_id(&r, 9999), None);
    }

    #[test]
    fn resolved_texture_inspection_returns_pixel_facts() {
        let api = api();
        let mut t = api.empty_table();
        let tex = api.register_checker_texture(&mut t, "checker", [255; 4], [0, 0, 0, 255]);
        let r = api.resolve(&t);
        assert_eq!(api.resolved_texture_count(&r), 1);
        assert_eq!(api.resolved_texture_id_at(&r, 0), Some(tex.raw()));
        let w = api.resolved_texture_width(&r, tex.raw()).unwrap();
        let h = api.resolved_texture_height(&r, tex.raw()).unwrap();
        assert_eq!(
            api.resolved_texture_pixels(&r, tex.raw()).unwrap().len(),
            (w * h * 4) as usize
        );
    }

    #[test]
    fn biome_atlas_cell_origins_cover_the_grid() {
        let api = api();
        assert_eq!(api.biome_atlas_cell_origin(0), (0.0, 0.0));
        assert_eq!(api.biome_atlas_cell_origin(3), (0.5, 0.5));
        // Out-of-range biome ids wrap into the 4-cell grid.
        assert_eq!(
            api.biome_atlas_cell_origin(4),
            api.biome_atlas_cell_origin(0)
        );
    }

    #[test]
    fn resolved_texture_accessors_handle_missing_ids() {
        let r = api().resolve(&api().empty_table());
        assert!(api().resolved_texture_id_at(&r, 0).is_none());
        assert!(api().resolved_texture_width(&r, 1).is_none());
        assert!(api().resolved_texture_height(&r, 1).is_none());
        assert!(api().resolved_texture_pixels(&r, 1).is_none());
        assert_eq!(api().resolved_texture_count(&r), 0);
    }
}
