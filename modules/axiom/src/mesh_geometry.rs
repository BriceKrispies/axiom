//! Resolving a [`Mesh`] description into renderable geometry.
//!
//! This mapping lives in the umbrella because it bridges the umbrella's `Mesh`
//! enum to an `axiom-resources` primitive — neither module can name the other's
//! contract types, so the composition is the feature module's job. The resources
//! table/resolve types are not nameable across the module boundary, so the
//! read-back is repeated per primitive (kept as inferred locals) rather than
//! factored into a shared helper.

use axiom_math::{Vec2, Vec3};
use axiom_resources::ResourcesApi;

use crate::mesh::Mesh;

/// One mesh's resolved geometry: the vertex streams the render pipeline uploads.
#[derive(Debug)]
pub(crate) struct MeshGeometry {
    pub(crate) positions: Vec<Vec3>,
    pub(crate) normals: Vec<Vec3>,
    pub(crate) uvs: Vec<Vec2>,
    pub(crate) indices: Vec<u32>,
}

/// Resolve a mesh description into renderable geometry by its kind.
pub(crate) fn mesh_geometry(mesh: &Mesh) -> MeshGeometry {
    // `Mesh` is a fieldless enum, so `*mesh as usize` is its discriminant: index
    // a generator table instead of `match`ing (branchless). The table order must
    // match the variant order (Cube=0, Plane=1, Sphere=2, Cylinder=3); adding a
    // `Mesh` variant requires adding its generator at the same index, or this
    // panics.
    let generators: [fn() -> MeshGeometry; 4] =
        [cube_geometry, plane_geometry, sphere_geometry, cylinder_geometry];
    generators[*mesh as usize]()
}

/// The engine's built-in cube primitive. `axiom-resources` owns the cube mesh
/// data; this only threads it into plain vertex streams the renderer uploads
/// (the resources table is a local, so its un-nameable type never escapes here).
fn cube_geometry() -> MeshGeometry {
    let resources = ResourcesApi::new();
    let mut table = resources.empty_table();
    let id = resources.register_cube_mesh(&mut table).raw();
    let resolved = resources.resolve(&table);
    let vertex_count = resources
        .resolved_mesh_vertex_count(&resolved, id)
        .expect("cube mesh present");
    let mut positions = Vec::with_capacity(vertex_count);
    let mut normals = Vec::with_capacity(vertex_count);
    let mut uvs = Vec::with_capacity(vertex_count);
    (0..vertex_count).for_each(|v| {
        let p = resources
            .resolved_mesh_position_at(&resolved, id, v)
            .expect("vertex in range");
        let n = resources
            .resolved_mesh_normal_at(&resolved, id, v)
            .expect("vertex in range");
        let u = resources
            .resolved_mesh_uv_at(&resolved, id, v)
            .expect("vertex in range");
        positions.push(Vec3::new(p[0], p[1], p[2]));
        normals.push(Vec3::new(n[0], n[1], n[2]));
        uvs.push(Vec2::new(u[0], u[1]));
    });
    let indices = resources
        .resolved_mesh_indices(&resolved, id)
        .expect("cube mesh present")
        .to_vec();
    MeshGeometry {
        positions,
        normals,
        uvs,
        indices,
    }
}

/// The engine's built-in plane primitive (a ground quad). Mirrors
/// [`cube_geometry`] (the per-primitive read-back cannot be factored — see the
/// module docs).
fn plane_geometry() -> MeshGeometry {
    let resources = ResourcesApi::new();
    let mut table = resources.empty_table();
    let id = resources.register_plane_mesh(&mut table).raw();
    let resolved = resources.resolve(&table);
    let vertex_count = resources
        .resolved_mesh_vertex_count(&resolved, id)
        .expect("plane mesh present");
    let mut positions = Vec::with_capacity(vertex_count);
    let mut normals = Vec::with_capacity(vertex_count);
    let mut uvs = Vec::with_capacity(vertex_count);
    (0..vertex_count).for_each(|v| {
        let p = resources
            .resolved_mesh_position_at(&resolved, id, v)
            .expect("vertex in range");
        let n = resources
            .resolved_mesh_normal_at(&resolved, id, v)
            .expect("vertex in range");
        let u = resources
            .resolved_mesh_uv_at(&resolved, id, v)
            .expect("vertex in range");
        positions.push(Vec3::new(p[0], p[1], p[2]));
        normals.push(Vec3::new(n[0], n[1], n[2]));
        uvs.push(Vec2::new(u[0], u[1]));
    });
    let indices = resources
        .resolved_mesh_indices(&resolved, id)
        .expect("plane mesh present")
        .to_vec();
    MeshGeometry {
        positions,
        normals,
        uvs,
        indices,
    }
}

/// The engine's built-in UV-sphere primitive. Mirrors [`cube_geometry`].
fn sphere_geometry() -> MeshGeometry {
    let resources = ResourcesApi::new();
    let mut table = resources.empty_table();
    let id = resources.register_sphere_mesh(&mut table).raw();
    let resolved = resources.resolve(&table);
    let vertex_count = resources
        .resolved_mesh_vertex_count(&resolved, id)
        .expect("sphere mesh present");
    let mut positions = Vec::with_capacity(vertex_count);
    let mut normals = Vec::with_capacity(vertex_count);
    let mut uvs = Vec::with_capacity(vertex_count);
    (0..vertex_count).for_each(|v| {
        let p = resources
            .resolved_mesh_position_at(&resolved, id, v)
            .expect("vertex in range");
        let n = resources
            .resolved_mesh_normal_at(&resolved, id, v)
            .expect("vertex in range");
        let u = resources
            .resolved_mesh_uv_at(&resolved, id, v)
            .expect("vertex in range");
        positions.push(Vec3::new(p[0], p[1], p[2]));
        normals.push(Vec3::new(n[0], n[1], n[2]));
        uvs.push(Vec2::new(u[0], u[1]));
    });
    let indices = resources
        .resolved_mesh_indices(&resolved, id)
        .expect("sphere mesh present")
        .to_vec();
    MeshGeometry {
        positions,
        normals,
        uvs,
        indices,
    }
}

/// The engine's built-in cylinder primitive. Mirrors [`sphere_geometry`].
fn cylinder_geometry() -> MeshGeometry {
    let resources = ResourcesApi::new();
    let mut table = resources.empty_table();
    let id = resources.register_cylinder_mesh(&mut table).raw();
    let resolved = resources.resolve(&table);
    let vertex_count = resources
        .resolved_mesh_vertex_count(&resolved, id)
        .expect("cylinder mesh present");
    let mut positions = Vec::with_capacity(vertex_count);
    let mut normals = Vec::with_capacity(vertex_count);
    let mut uvs = Vec::with_capacity(vertex_count);
    (0..vertex_count).for_each(|v| {
        let p = resources
            .resolved_mesh_position_at(&resolved, id, v)
            .expect("vertex in range");
        let n = resources
            .resolved_mesh_normal_at(&resolved, id, v)
            .expect("vertex in range");
        let u = resources
            .resolved_mesh_uv_at(&resolved, id, v)
            .expect("vertex in range");
        positions.push(Vec3::new(p[0], p[1], p[2]));
        normals.push(Vec3::new(n[0], n[1], n[2]));
        uvs.push(Vec2::new(u[0], u[1]));
    });
    let indices = resources
        .resolved_mesh_indices(&resolved, id)
        .expect("cylinder mesh present")
        .to_vec();
    MeshGeometry {
        positions,
        normals,
        uvs,
        indices,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_geometry_resolves_every_primitive() {
        // Covers the generator table + each primitive resolver.
        let cube = mesh_geometry(&Mesh::Cube);
        let plane = mesh_geometry(&Mesh::Plane);
        let sphere = mesh_geometry(&Mesh::Sphere);
        let cylinder = mesh_geometry(&Mesh::Cylinder);
        assert_eq!(cube.positions.len(), 24);
        assert_eq!(plane.positions.len(), 4);
        assert!(sphere.positions.len() > 100);
        // Cylinder: 4 rings of 17 + 2 cap centers = 70 vertices.
        assert_eq!(cylinder.positions.len(), 70);
        [&cube, &plane, &sphere, &cylinder].into_iter().for_each(|g| {
            assert_eq!(g.positions.len(), g.normals.len());
            assert_eq!(g.positions.len(), g.uvs.len());
            assert!(!g.indices.is_empty());
        });
    }
}
