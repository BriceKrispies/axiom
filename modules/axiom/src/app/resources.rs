//! The live-backend resource exports on [`RunningApp`] — the registered meshes as
//! interleaved vertex streams and the materials as RGBA8 albedo sets the windowing
//! backend uploads.

use super::RunningApp;
use crate::mesh_geometry::MeshGeometry;
use crate::texture::texture_rgba;

impl RunningApp {
    /// The first mesh's geometry as the live backend's vertex stream (interleaved
    /// position+normal+uv+colour, 12 floats per vertex) plus its triangle-list
    /// indices. Empty when the app registered no mesh. Plain data the windowing
    /// backend uploads. The UV is the mesh's own texture coordinate; per-vertex
    /// colour is opaque **white** here: the live shader multiplies the sampled
    /// albedo by this and by the per-instance (material) colour, so white keeps
    /// the per-instance colour authoritative — the built-in cube renders exactly
    /// as before. An app that wants true per-vertex colours builds its own stream
    /// (see `axiom-growth`'s terrain).
    pub fn mesh_vertex_stream(&self) -> (Vec<f32>, Vec<u32>) {
        self.meshes.first().map_or_else(
            || (Vec::new(), Vec::new()),
            |(_, geom)| (interleave_vertices(geom), geom.indices.clone()),
        )
    }

    /// Every registered mesh's geometry as the multi-mesh live backend's upload
    /// set: `(mesh_id, interleaved position+normal+uv+colour vertices [12
    /// floats/vertex], triangle indices)`. UV is the mesh's own texture
    /// coordinate; per-vertex colour is opaque white (the live shader multiplies
    /// the sampled albedo by this and by the per-instance material colour, so
    /// white keeps the material colour authoritative). The backend uploads these
    /// once and draws each frame's per-mesh instance batches against them.
    pub fn mesh_set(&self) -> Vec<(u64, Vec<f32>, Vec<u32>)> {
        self.meshes
            .iter()
            .filter(|(_, geom)| geom.joints.is_empty())
            .map(|(id, geom)| (*id, interleave_vertices(geom), geom.indices.clone()))
            .collect()
    }

    /// Every registered **skinned** mesh as the backend's skinned upload set:
    /// `(mesh_id, interleaved 20-float vertices, triangle indices)`. Each vertex
    /// is `position(3) + normal(3) + uv(2) + colour(4) + joints(4) + weights(4)` —
    /// the standard 12-float stream plus the four bone indices (as floats) and four
    /// blend weights the skinning vertex shader reads. Skinned meshes are excluded
    /// from [`Self::mesh_set`] (they render only through the skinned pipeline with a
    /// per-draw joint palette). Uploaded once at bind, like the static set.
    pub fn skinned_mesh_set(&self) -> Vec<(u64, Vec<f32>, Vec<u32>)> {
        self.meshes
            .iter()
            .filter(|(_, geom)| !geom.joints.is_empty())
            .map(|(id, geom)| (*id, interleave_skinned_vertices(geom), geom.indices.clone()))
            .collect()
    }

    /// Every registered material as the live backend's material set: `(material_id,
    /// width, height, RGBA8 albedo pixels)`. Resolution order per material: an
    /// app-authored raw-pixel texture (`with_custom_texture`, looked up in the
    /// custom-texture store); else the built-in procedural [`crate::texture::Texture`];
    /// else a 1×1 opaque-white albedo (so its sampled albedo is `(1,1,1,1)` and the
    /// draw colour reduces to base × per-vertex colour). The backend builds one
    /// albedo bind group per material.
    pub fn material_textures(&self) -> Vec<(u64, u32, u32, Vec<u8>)> {
        self.materials
            .iter()
            .map(|(id, material)| {
                let custom = self
                    .custom_textures
                    .iter()
                    .find(|(tid, _, _, _)| *tid == material.custom_texture())
                    .map(|(_, w, h, px)| (*w, *h, px.clone()));
                let (w, h, pixels) = custom
                    .or_else(|| material.texture().map(texture_rgba))
                    .unwrap_or_else(|| (1, 1, vec![255, 255, 255, 255]));
                (*id, w, h, pixels)
            })
            .collect()
    }
}

/// Interleave one mesh's resolved geometry into the live backend's 12-float
/// vertex stream: position(3) + normal(3) + uv(2) + opaque-white colour(4) per
/// vertex. Shared by [`RunningApp::mesh_vertex_stream`] and
/// [`RunningApp::mesh_set`].
fn interleave_vertices(geom: &MeshGeometry) -> Vec<f32> {
    let mut vertices = Vec::with_capacity(geom.positions.len() * 12);
    geom.positions
        .iter()
        .zip(geom.normals.iter())
        .zip(geom.uvs.iter())
        .for_each(|((p, n), uv)| {
            vertices
                .extend_from_slice(&[p.x, p.y, p.z, n.x, n.y, n.z, uv.x, uv.y, 1.0, 1.0, 1.0, 1.0])
        });
    vertices
}

/// Interleave a **skinned** mesh into the 20-float skinning stream: the standard
/// 12 floats followed by four bone indices (as floats) and four blend weights.
/// Only called for meshes carrying skin streams (`skinned_mesh_set`).
fn interleave_skinned_vertices(geom: &MeshGeometry) -> Vec<f32> {
    let mut vertices = Vec::with_capacity(geom.positions.len() * 20);
    geom.positions
        .iter()
        .zip(geom.normals.iter())
        .zip(geom.uvs.iter())
        .zip(geom.joints.iter())
        .zip(geom.weights.iter())
        .for_each(|((((p, n), uv), j), w)| {
            vertices
                .extend_from_slice(&[p.x, p.y, p.z, n.x, n.y, n.z, uv.x, uv.y, 1.0, 1.0, 1.0, 1.0]);
            vertices.extend_from_slice(&[j[0] as f32, j[1] as f32, j[2] as f32, j[3] as f32]);
            vertices.extend_from_slice(&w[..]);
        });
    vertices
}
