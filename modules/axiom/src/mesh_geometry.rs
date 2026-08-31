//! Resolving a [`Mesh`] description into renderable geometry.
//!
//! This mapping lives in the umbrella because it bridges the umbrella's `Mesh`
//! enum to an `axiom-resources` primitive — neither module can name the other's
//! contract types, so the composition is the feature module's job. The resources
//! table/resolve types are not nameable across the module boundary, so the
//! read-back is repeated per primitive (kept as inferred locals) rather than
//! factored into a shared helper.

use axiom_math::{Vec2, Vec3, Vec4};
use axiom_resources::ResourcesApi;

use crate::mesh::Mesh;
use crate::mesh_data::{MeshData, MeshDataError};

/// One mesh's resolved geometry: the vertex streams the render pipeline uploads.
/// `joints`/`weights` are empty for static meshes and one four-influence entry
/// per vertex for a skinned mesh (deformed by a skeleton on the GPU).
#[derive(Debug, PartialEq)]
pub(crate) struct MeshGeometry {
    pub(crate) positions: Vec<Vec3>,
    pub(crate) normals: Vec<Vec3>,
    pub(crate) uvs: Vec<Vec2>,
    pub(crate) indices: Vec<u32>,
    pub(crate) joints: Vec<[u16; 4]>,
    pub(crate) weights: Vec<[f32; 4]>,
    /// Per-vertex colour. **Empty means opaque white for every vertex** — the
    /// value the interleave substituted before authors could supply one — so a
    /// primitive or an author mesh that names no colour uploads exactly the
    /// bytes it always did.
    pub(crate) colors: Vec<[f32; 4]>,
}

/// Resolve a mesh description into renderable geometry by its kind.
pub(crate) fn mesh_geometry(mesh: &Mesh) -> MeshGeometry {
    // Table order must match the `Mesh` variant order (Cube=0, Plane=1, Sphere=2,
    // Cylinder=3); adding a variant requires adding its generator at the same index.
    let generators: [fn() -> MeshGeometry; 4] = [
        cube_geometry,
        plane_geometry,
        sphere_geometry,
        cylinder_geometry,
    ];
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
        joints: Vec::new(),
        weights: Vec::new(),
        colors: Vec::new(),
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
        joints: Vec::new(),
        weights: Vec::new(),
        colors: Vec::new(),
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
        joints: Vec::new(),
        weights: Vec::new(),
        colors: Vec::new(),
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
        joints: Vec::new(),
        weights: Vec::new(),
        colors: Vec::new(),
    }
}

/// Resolve author-supplied [`MeshData`] into the same renderable geometry shape
/// the catalog primitives produce, or report the first reason it is invalid. On
/// success the neutral geometry is registered and resolved through
/// `axiom-resources` exactly like a primitive (see [`resolve_author_geometry`]),
/// so an author mesh rides the identical pipeline — no special render path.
pub(crate) fn mesh_data_geometry(data: &MeshData) -> Result<MeshGeometry, MeshDataError> {
    validate(data).map_or_else(
        || {
            Ok(data
                .is_skinned()
                .then(|| skinned_author_geometry(data))
                .unwrap_or_else(|| resolve_author_geometry(data)))
        },
        Err,
    )
}

/// Build renderable geometry for a **skinned** author mesh directly from its
/// streams. Skinned meshes bypass the `axiom-resources` round-trip (which carries
/// only position/normal/uv) so the per-vertex skin streams stay aligned 1:1 with
/// the vertices. UVs default to the origin when the author supplied none, matching
/// [`resolve_author_geometry`]. Only called after [`validate`] passes.
fn skinned_author_geometry(data: &MeshData) -> MeshGeometry {
    let uvs = (0..data.positions().len())
        .map(|i| data.uvs().get(i).copied().unwrap_or(Vec2::ZERO))
        .collect();
    MeshGeometry {
        positions: data.positions().to_vec(),
        normals: data.normals().to_vec(),
        uvs,
        indices: data.indices().to_vec(),
        joints: data.joints().to_vec(),
        weights: data.weights().to_vec(),
        colors: author_colors(data),
    }
}

/// The first failing validation check, in [`MeshDataError`] declaration priority,
/// or `None` when the author geometry is well-formed.
fn validate(data: &MeshData) -> Option<MeshDataError> {
    let positions = data.positions();
    let normals = data.normals();
    let uvs = data.uvs();
    let indices = data.indices();
    let empty = positions
        .is_empty()
        .then_some(MeshDataError::EmptyPositions);
    let non_finite = (!all_finite(positions, normals, uvs)).then_some(MeshDataError::NonFinite);
    let normal_mismatch =
        (normals.len() != positions.len()).then_some(MeshDataError::NormalCountMismatch);
    let uv_mismatch = ((!uvs.is_empty()) & (uvs.len() != positions.len()))
        .then_some(MeshDataError::UvCountMismatch);
    let no_indices = indices.is_empty().then_some(MeshDataError::NoIndices);
    let not_triangles = (indices.len() % 3 != 0).then_some(MeshDataError::IndicesNotTriangles);
    let out_of_range = indices
        .iter()
        .any(|&i| i as usize >= positions.len())
        .then_some(MeshDataError::IndexOutOfRange);
    // Skin streams: if either is present, both must carry one four-influence entry
    // per vertex, and every weight must be finite.
    let skin_present = (!data.joints().is_empty()) | (!data.weights().is_empty());
    let skin_mismatch = (skin_present
        & ((data.joints().len() != positions.len()) | (data.weights().len() != positions.len())))
    .then_some(MeshDataError::SkinCountMismatch);
    let skin_non_finite = data
        .weights()
        .iter()
        .flatten()
        .any(|w| !w.is_finite())
        .then_some(MeshDataError::SkinWeightsNonFinite);
    // Colours: present or absent, never partial. Absent is the opaque-white
    // sentinel; a partial stream would silently white out whichever vertices ran
    // past the end of it, which is the failure this rejects rather than pads.
    let color_mismatch = ((!data.colors().is_empty())
        & (data.colors().len() != positions.len()))
    .then_some(MeshDataError::ColorCountMismatch);
    let color_non_finite = data
        .colors()
        .iter()
        .any(|c| !(c.x.is_finite() & c.y.is_finite() & c.z.is_finite() & c.w.is_finite()))
        .then_some(MeshDataError::ColorsNonFinite);
    empty
        .or(non_finite)
        .or(normal_mismatch)
        .or(uv_mismatch)
        .or(no_indices)
        .or(not_triangles)
        .or(out_of_range)
        .or(skin_mismatch)
        .or(skin_non_finite)
        .or(color_mismatch)
        .or(color_non_finite)
}

/// The author's per-vertex colours as the interleave's lane shape. Empty stays
/// empty — that is the "opaque white" sentinel, not a missing value to fill in.
fn author_colors(data: &MeshData) -> Vec<[f32; 4]> {
    data.colors()
        .iter()
        .map(|c| [c.x, c.y, c.z, c.w])
        .collect()
}

/// Whether every position / normal / UV coordinate is finite (no NaN, no ∞).
fn all_finite(positions: &[Vec3], normals: &[Vec3], uvs: &[Vec2]) -> bool {
    let vec3_finite = |v: &Vec3| v.x.is_finite() & v.y.is_finite() & v.z.is_finite();
    positions.iter().chain(normals.iter()).all(vec3_finite)
        & uvs.iter().all(|v| v.x.is_finite() & v.y.is_finite())
}

/// Register pre-validated author geometry through `axiom-resources` and read the
/// resolved vertex streams back, mirroring [`cube_geometry`] (the per-primitive
/// read-back cannot be factored — see the module docs). UVs default to the origin
/// when the author supplied none; per-vertex colour is opaque white (the live
/// shader multiplies it, so white keeps the material colour authoritative). Only
/// called after [`validate`] passes, so the registration and read-back never see
/// malformed data.
fn resolve_author_geometry(data: &MeshData) -> MeshGeometry {
    let resources = ResourcesApi::new();
    let mut table = resources.empty_table();
    let vertices: Vec<([f32; 3], [f32; 3], [f32; 2], [f32; 4])> = data
        .positions()
        .iter()
        .zip(data.normals().iter())
        .enumerate()
        .map(|(i, (p, n))| {
            let uv = data.uvs().get(i).copied().unwrap_or(Vec2::ZERO);
            (
                [p.x, p.y, p.z],
                [n.x, n.y, n.z],
                [uv.x, uv.y],
                [1.0, 1.0, 1.0, 1.0],
            )
        })
        .collect();
    let id = resources
        .register_mesh(&mut table, "axiom.author.mesh", &vertices, data.indices())
        .raw();
    let resolved = resources.resolve(&table);
    let vertex_count = resources
        .resolved_mesh_vertex_count(&resolved, id)
        .expect("author mesh present");
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
        .expect("author mesh present")
        .to_vec();
    MeshGeometry {
        positions,
        normals,
        uvs,
        indices,
        joints: Vec::new(),
        weights: Vec::new(),
        // Straight off the author, NOT through the `axiom-resources` round-trip
        // above: that trip carries position/normal/uv only, so a colour put into
        // it would come back white. The same reason `skinned_author_geometry`
        // bypasses it for the skin streams.
        colors: author_colors(data),
    }
}

#[cfg(test)]
mod tests {
    use super::*;


    /// A tri with one colour per vertex — the shape a geometry-baked mask has.
    fn colored_tri(colors: Vec<Vec4>) -> MeshData {
        MeshData::new(
            vec![Vec3::ZERO, Vec3::UNIT_X, Vec3::UNIT_Y],
            vec![Vec3::UNIT_Z; 3],
            vec![],
            vec![0, 1, 2],
        )
        .with_colors(colors)
    }

    /// **The defect this stream exists to remove.** An author's per-vertex
    /// colour has to survive registration and reach the resolved geometry; it
    /// used to be overwritten with opaque white on the way through, so a
    /// geometry-baked wear/grime mask arrived at the shader as a constant.
    #[test]
    fn author_colors_survive_into_the_resolved_geometry() {
        let authored = vec![
            Vec4::new(0.1, 0.2, 0.3, 1.0),
            Vec4::new(0.4, 0.5, 0.6, 1.0),
            Vec4::new(0.7, 0.8, 0.9, 1.0),
        ];
        let geom = mesh_data_geometry(&colored_tri(authored.clone())).expect("well-formed");
        assert_eq!(
            geom.colors,
            authored
                .iter()
                .map(|c| [c.x, c.y, c.z, c.w])
                .collect::<Vec<_>>(),
            "the round-trip whitened the mask again"
        );
    }

    /// The sentinel: no colours means an EMPTY stream, never a filled-in white
    /// one. The interleave reads the emptiness, so padding here would cost every
    /// mesh in the engine four floats a vertex to say nothing.
    #[test]
    fn a_mesh_that_names_no_colour_resolves_an_empty_stream() {
        let geom = mesh_data_geometry(&colored_tri(vec![])).expect("well-formed");
        assert!(geom.colors.is_empty());
        // And a primitive is the same — the generators name no colour either.
        assert!(mesh_geometry(&Mesh::Cube).colors.is_empty());
    }

    /// Present or absent, never partial: two colours for three vertices would
    /// leave the third silently white.
    #[test]
    fn a_partial_colour_stream_is_rejected() {
        let short = colored_tri(vec![Vec4::new(1.0, 0.0, 0.0, 1.0); 2]);
        assert_eq!(
            mesh_data_geometry(&short).unwrap_err(),
            MeshDataError::ColorCountMismatch
        );
    }

    /// A non-finite channel is caught here rather than reaching the GPU, exactly
    /// as a non-finite skin weight is.
    #[test]
    fn a_non_finite_colour_is_rejected() {
        let bad = colored_tri(vec![
            Vec4::new(f32::NAN, 0.0, 0.0, 1.0),
            Vec4::new(0.0, 0.0, 0.0, 1.0),
            Vec4::new(0.0, 0.0, 0.0, 1.0),
        ]);
        assert_eq!(
            mesh_data_geometry(&bad).unwrap_err(),
            MeshDataError::ColorsNonFinite
        );
        let inf = colored_tri(vec![
            Vec4::new(0.0, 0.0, 0.0, f32::INFINITY),
            Vec4::new(0.0, 0.0, 0.0, 1.0),
            Vec4::new(0.0, 0.0, 0.0, 1.0),
        ]);
        assert_eq!(
            mesh_data_geometry(&inf).unwrap_err(),
            MeshDataError::ColorsNonFinite
        );
    }

    /// Colours compose with the skin streams rather than excluding them: a
    /// skinned mesh takes the same builder, and `skinned_author_geometry` is a
    /// different code path from the static one.
    #[test]
    fn a_skinned_mesh_carries_colours_too() {
        let colors = vec![Vec4::new(0.25, 0.5, 0.75, 1.0); 3];
        let data = MeshData::new_skinned(
            vec![Vec3::ZERO, Vec3::UNIT_X, Vec3::UNIT_Y],
            vec![Vec3::UNIT_Z; 3],
            vec![],
            vec![[0, 0, 0, 0]; 3],
            vec![[1.0, 0.0, 0.0, 0.0]; 3],
            vec![0, 1, 2],
        )
        .with_colors(colors);
        let geom = mesh_data_geometry(&data).expect("well-formed");
        assert_eq!(geom.colors, vec![[0.25, 0.5, 0.75, 1.0]; 3]);
        assert_eq!(geom.joints.len(), 3, "the skin streams still arrive");
    }

    /// The accessor is part of the public surface, and the two new error
    /// variants carry the derives the rest of the enum does.
    #[test]
    fn the_colour_accessor_and_error_variants_are_reachable() {
        let c = Vec4::new(1.0, 1.0, 1.0, 1.0);
        assert_eq!(colored_tri(vec![c; 3]).colors().len(), 3);
        assert!(colored_tri(vec![]).colors().is_empty());
        assert!(format!("{:?}", MeshDataError::ColorCountMismatch).contains("ColorCount"));
        assert_ne!(
            MeshDataError::ColorsNonFinite,
            MeshDataError::ColorCountMismatch
        );
    }


    #[test]
    fn mesh_geometry_resolves_every_primitive() {
        let cube = mesh_geometry(&Mesh::Cube);
        let plane = mesh_geometry(&Mesh::Plane);
        let sphere = mesh_geometry(&Mesh::Sphere);
        let cylinder = mesh_geometry(&Mesh::Cylinder);
        assert_eq!(cube.positions.len(), 24);
        assert_eq!(plane.positions.len(), 4);
        assert!(sphere.positions.len() > 100);
        // Cylinder: 4 rings of 17 + 2 cap centers = 70 vertices.
        assert_eq!(cylinder.positions.len(), 70);
        [&cube, &plane, &sphere, &cylinder]
            .into_iter()
            .for_each(|g| {
                assert_eq!(g.positions.len(), g.normals.len());
                assert_eq!(g.positions.len(), g.uvs.len());
                assert!(!g.indices.is_empty());
            });
    }

    /// A well-formed author triangle with explicit UVs.
    fn triangle_with_uvs() -> MeshData {
        MeshData::new(
            vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(2.0, 0.0, 0.0),
                Vec3::new(0.0, 3.0, 0.0),
            ],
            vec![Vec3::UNIT_Z, Vec3::UNIT_Z, Vec3::UNIT_Z],
            vec![Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0)],
            vec![0, 1, 2],
        )
    }

    #[test]
    fn author_geometry_resolves_explicit_vertices_and_uvs() {
        let geom = mesh_data_geometry(&triangle_with_uvs()).expect("valid author mesh");
        assert_eq!(geom.positions.len(), 3);
        assert_eq!(geom.normals.len(), 3);
        assert_eq!(geom.uvs.len(), 3);
        assert_eq!(geom.indices, vec![0, 1, 2]);
        assert_eq!(geom.positions[1], Vec3::new(2.0, 0.0, 0.0));
        assert_eq!(geom.normals[0], Vec3::UNIT_Z);
        assert_eq!(geom.uvs[2], Vec2::new(0.0, 1.0));
    }

    #[test]
    fn author_geometry_defaults_omitted_uvs_to_the_origin() {
        let data = MeshData::new(
            vec![Vec3::ZERO, Vec3::UNIT_X, Vec3::UNIT_Y],
            vec![Vec3::UNIT_Z, Vec3::UNIT_Z, Vec3::UNIT_Z],
            vec![],
            vec![0, 1, 2],
        );
        let geom = mesh_data_geometry(&data).expect("valid author mesh");
        assert_eq!(geom.uvs, vec![Vec2::ZERO, Vec2::ZERO, Vec2::ZERO]);
        assert_eq!(geom.positions.len(), 3);
    }

    #[test]
    fn empty_positions_are_rejected() {
        let data = MeshData::new(vec![], vec![], vec![], vec![]);
        assert_eq!(
            mesh_data_geometry(&data),
            Err(MeshDataError::EmptyPositions)
        );
    }

    #[test]
    fn non_finite_coordinates_are_rejected() {
        // A NaN position and a separate ∞ UV each exercise a different arm of
        // `all_finite`.
        let nan_pos = MeshData::new(
            vec![Vec3::new(f32::NAN, 0.0, 0.0), Vec3::UNIT_X, Vec3::UNIT_Y],
            vec![Vec3::UNIT_Z, Vec3::UNIT_Z, Vec3::UNIT_Z],
            vec![],
            vec![0, 1, 2],
        );
        assert_eq!(mesh_data_geometry(&nan_pos), Err(MeshDataError::NonFinite));
        let inf_uv = MeshData::new(
            vec![Vec3::ZERO, Vec3::UNIT_X, Vec3::UNIT_Y],
            vec![Vec3::UNIT_Z, Vec3::UNIT_Z, Vec3::UNIT_Z],
            vec![Vec2::ZERO, Vec2::ZERO, Vec2::new(f32::INFINITY, 0.0)],
            vec![0, 1, 2],
        );
        assert_eq!(mesh_data_geometry(&inf_uv), Err(MeshDataError::NonFinite));
    }

    #[test]
    fn mismatched_normal_count_is_rejected() {
        let data = MeshData::new(
            vec![Vec3::ZERO, Vec3::UNIT_X, Vec3::UNIT_Y],
            vec![Vec3::UNIT_Z, Vec3::UNIT_Z],
            vec![],
            vec![0, 1, 2],
        );
        assert_eq!(
            mesh_data_geometry(&data),
            Err(MeshDataError::NormalCountMismatch)
        );
    }

    #[test]
    fn mismatched_uv_count_is_rejected() {
        let data = MeshData::new(
            vec![Vec3::ZERO, Vec3::UNIT_X, Vec3::UNIT_Y],
            vec![Vec3::UNIT_Z, Vec3::UNIT_Z, Vec3::UNIT_Z],
            vec![Vec2::ZERO],
            vec![0, 1, 2],
        );
        assert_eq!(
            mesh_data_geometry(&data),
            Err(MeshDataError::UvCountMismatch)
        );
    }

    #[test]
    fn missing_indices_are_rejected() {
        let data = MeshData::new(
            vec![Vec3::ZERO, Vec3::UNIT_X, Vec3::UNIT_Y],
            vec![Vec3::UNIT_Z, Vec3::UNIT_Z, Vec3::UNIT_Z],
            vec![],
            vec![],
        );
        assert_eq!(mesh_data_geometry(&data), Err(MeshDataError::NoIndices));
    }

    #[test]
    fn non_triangle_index_count_is_rejected() {
        let data = MeshData::new(
            vec![Vec3::ZERO, Vec3::UNIT_X, Vec3::UNIT_Y],
            vec![Vec3::UNIT_Z, Vec3::UNIT_Z, Vec3::UNIT_Z],
            vec![],
            vec![0, 1],
        );
        assert_eq!(
            mesh_data_geometry(&data),
            Err(MeshDataError::IndicesNotTriangles)
        );
    }

    #[test]
    fn out_of_range_index_is_rejected() {
        let data = MeshData::new(
            vec![Vec3::ZERO, Vec3::UNIT_X, Vec3::UNIT_Y],
            vec![Vec3::UNIT_Z, Vec3::UNIT_Z, Vec3::UNIT_Z],
            vec![],
            vec![0, 1, 9],
        );
        assert_eq!(
            mesh_data_geometry(&data),
            Err(MeshDataError::IndexOutOfRange)
        );
    }

    fn skinned_triangle(joints: Vec<[u16; 4]>, weights: Vec<[f32; 4]>, uvs: Vec<Vec2>) -> MeshData {
        MeshData::new_skinned(
            vec![Vec3::ZERO, Vec3::UNIT_X, Vec3::UNIT_Y],
            vec![Vec3::UNIT_Z; 3],
            uvs,
            joints,
            weights,
            vec![0, 1, 2],
        )
    }

    #[test]
    fn a_static_author_mesh_has_empty_skin_streams() {
        let geom = mesh_data_geometry(&triangle_with_uvs()).expect("valid");
        assert!(geom.joints.is_empty());
        assert!(geom.weights.is_empty());
    }

    #[test]
    fn skinned_author_geometry_carries_aligned_skin_streams() {
        let geom = mesh_data_geometry(&skinned_triangle(
            vec![[0, 1, 0, 0]; 3],
            vec![[0.6, 0.4, 0.0, 0.0]; 3],
            vec![Vec2::ZERO; 3],
        ))
        .expect("valid skinned mesh");
        assert_eq!(geom.positions.len(), 3);
        assert_eq!(geom.joints, vec![[0, 1, 0, 0]; 3]);
        assert_eq!(geom.weights[0], [0.6, 0.4, 0.0, 0.0]);
    }

    #[test]
    fn skinned_author_geometry_defaults_omitted_uvs_to_the_origin() {
        let geom = mesh_data_geometry(&skinned_triangle(
            vec![[0, 0, 0, 0]; 3],
            vec![[1.0, 0.0, 0.0, 0.0]; 3],
            vec![],
        ))
        .expect("valid skinned mesh");
        assert_eq!(geom.uvs, vec![Vec2::ZERO; 3]);
    }

    #[test]
    fn mismatched_skin_count_is_rejected() {
        // Joints present but shorter than the vertex count.
        let data = skinned_triangle(vec![[0, 0, 0, 0]; 2], vec![[1.0, 0.0, 0.0, 0.0]; 3], vec![]);
        assert_eq!(
            mesh_data_geometry(&data),
            Err(MeshDataError::SkinCountMismatch)
        );
    }

    #[test]
    fn non_finite_skin_weights_are_rejected() {
        let data = skinned_triangle(
            vec![[0, 0, 0, 0]; 3],
            vec![
                [f32::NAN, 0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0, 0.0],
            ],
            vec![],
        );
        assert_eq!(
            mesh_data_geometry(&data),
            Err(MeshDataError::SkinWeightsNonFinite)
        );
    }
}
