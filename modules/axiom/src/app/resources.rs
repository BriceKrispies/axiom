//! The live-backend resource exports on [`RunningApp`] — the registered meshes as
//! interleaved vertex streams and the materials as RGBA8 albedo sets the windowing
//! backend uploads.

use axiom_host::{MapPixels, MaterialTexture};

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

    /// Every registered material as the live backend's material set: one
    /// [`MaterialTexture`] per material, carrying its albedo pixels **and the
    /// sampling mode the material authored**. Resolution order per material: an
    /// app-authored raw-pixel texture (`with_custom_texture`, looked up in the
    /// custom-texture store); else the built-in procedural [`crate::texture::Texture`];
    /// else a 1×1 opaque-white albedo (so its sampled albedo is `(1,1,1,1)` and the
    /// draw colour reduces to base × per-vertex colour). The backend builds one
    /// albedo bind group per material.
    ///
    /// The four **non-albedo** maps — normal, `(occlusion, roughness, metalness,
    /// height)`, micro-detail and macro variation — resolve the same way, through
    /// the *same* custom-texture store, from the four ids
    /// [`crate::material::Material::with_normal_texture`] and its siblings name. A
    /// material that names none of them (id `0`, which no registration ever
    /// issues) carries `None` in every slot and the backend binds its documented
    /// neutrals, so it renders exactly as it did before those slots existed.
    ///
    /// The sampling mode rides here rather than on the frame packet because
    /// material pixels are **bind-time resident state**, not per-frame data: the
    /// backend builds one sampler per material once, and nothing about it changes
    /// from frame to frame. The maps ride here for the same reason, and one
    /// sampler covers all five: a filtering rule belongs to the material, not to
    /// each of its images.
    pub fn material_textures(&self) -> Vec<MaterialTexture> {
        self.materials
            .iter()
            .map(|(id, material)| {
                // The albedo keeps its own lookup rather than going through
                // `map_pixels`: it needs the pixels as a bare `Vec`, and routing
                // it through the map type would clone a multi-megabyte payload
                // twice per material at bind.
                let (w, h, pixels) = self
                    .custom_textures
                    .iter()
                    .find(|(tid, _, _, _)| *tid == material.custom_texture())
                    .map(|(_, w, h, px)| (*w, *h, px.clone()))
                    .or_else(|| material.texture().map(texture_rgba))
                    .unwrap_or_else(|| (1, 1, vec![255, 255, 255, 255]));
                MaterialTexture::new(*id, w, h, pixels)
                    .with_sampling(material.texture_sampling())
                    .with_normal(self.map_pixels(material.normal_texture()))
                    .with_orm_height(self.map_pixels(material.orm_texture()))
                    .with_detail(self.map_pixels(material.detail_texture()))
                    .with_macro_field(self.map_pixels(material.macro_texture()))
            })
            .collect()
    }

    /// The registered raw pixels behind one `add_texture_data` id, or `None` when
    /// the material named no texture for that slot (`0`) — or named one that was
    /// never registered, which resolves the same way rather than failing the
    /// frame: an unresolvable id is a missing map, and a missing map is exactly
    /// what the backend's neutral is for.
    fn map_pixels(&self, texture_id: u64) -> Option<MapPixels> {
        self.custom_textures
            .iter()
            .find(|(tid, _, _, _)| *tid == texture_id)
            .map(|(_, w, h, px)| MapPixels::new(*w, *h, px.clone()))
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

#[cfg(test)]
mod tests {
    use crate::app::App;
    use crate::color::Color;
    use crate::default_plugins::DefaultPlugins;
    use crate::material::Material;
    use crate::texture::Texture;
    use crate::window::Window;

    use super::RunningApp;

    /// A bare rendering app with empty mesh/material stores.
    fn app() -> RunningApp {
        App::new()
            .window(Window::new(64, 64))
            .add_plugins(DefaultPlugins)
            .build()
    }

    /// A 1x1 texture whose four bytes are all `tag`, so the slot a map landed in
    /// is readable straight off the pixels.
    fn tagged(app: &mut RunningApp, tag: u8) -> u64 {
        app.add_texture_data(1, 1, vec![tag; 4])
            .expect("a well-formed 1x1 registers")
            .id()
    }

    /// **The whole point of the change.** All four non-albedo maps resolve out of
    /// the one custom-texture store, into their own slots, alongside the albedo —
    /// and none of them crosses into another slot.
    #[test]
    fn every_authored_map_reaches_its_own_slot() {
        let mut app = app();
        let (albedo, normal, orm, detail, macro_field) = (
            tagged(&mut app, 10),
            tagged(&mut app, 20),
            tagged(&mut app, 30),
            tagged(&mut app, 40),
            tagged(&mut app, 50),
        );
        let handle = app.add_material(
            Material::lit(Color::WHITE)
                .with_custom_texture(albedo)
                .with_normal_texture(normal)
                .with_orm_texture(orm)
                .with_detail_texture(detail)
                .with_macro_texture(macro_field),
        );
        let set = app.material_textures();
        let entry = set
            .iter()
            .find(|t| t.material_id() == handle.id())
            .expect("the material is in the set");
        assert_eq!(entry.pixels(), &[10; 4], "the albedo slot");
        let slot = |m: Option<&axiom_host::MapPixels>| m.map(|m| m.pixels().to_vec());
        assert_eq!(slot(entry.normal()), Some(vec![20; 4]));
        assert_eq!(slot(entry.orm_height()), Some(vec![30; 4]));
        assert_eq!(slot(entry.detail()), Some(vec![40; 4]));
        assert_eq!(slot(entry.macro_field()), Some(vec![50; 4]));
        // The extents ride with the pixels rather than being inferred.
        assert_eq!(
            entry.normal().map(|m| (m.width(), m.height())),
            Some((1, 1))
        );
    }

    /// **The compatibility invariant.** A material authored the way every existing
    /// app authors one carries `None` in all four slots, so every backend binds
    /// its neutrals and the frame is the frame it was before these slots existed.
    /// Covers the built-in-procedural albedo and the 1x1-white fallback, which are
    /// the other two ways the albedo resolves.
    #[test]
    fn a_material_that_authors_no_maps_carries_none_in_every_slot() {
        let mut app = app();
        let plain = app.add_material(Material::lit(Color::WHITE));
        let procedural =
            app.add_material(Material::lit(Color::WHITE).with_texture(Texture::Checker));
        let set = app.material_textures();
        let entry = |id: u64| {
            set.iter()
                .find(|t| t.material_id() == id)
                .expect("material present")
        };
        [plain.id(), procedural.id()].iter().for_each(|id| {
            let t = entry(*id);
            assert_eq!(t.normal(), None, "material {id} authored no normal map");
            assert_eq!(t.orm_height(), None);
            assert_eq!(t.detail(), None);
            assert_eq!(t.macro_field(), None);
        });
        // And the albedo half is untouched: white 1x1 for the plain material, the
        // built-in checker's own pixels for the procedural one.
        assert_eq!(
            (
                entry(plain.id()).width(),
                entry(plain.id()).height(),
                entry(plain.id()).pixels()
            ),
            (1, 1, [255, 255, 255, 255].as_slice())
        );
        assert!(
            entry(procedural.id()).pixels().len() > 4,
            "the built-in procedural albedo still resolves to its own pixels"
        );
    }

    /// An id no registration ever issued resolves to "no map", not to a panic and
    /// not to another material's pixels. `0` is the documented cleared value; a
    /// stale non-zero id is the same missing map, and the backend's neutral is
    /// exactly what a missing map is for.
    #[test]
    fn an_unregistered_map_id_resolves_to_no_map() {
        let mut app = app();
        let real = tagged(&mut app, 77);
        let handle = app.add_material(
            Material::lit(Color::WHITE)
                .with_normal_texture(real)
                // 4096 is far past anything `add_texture_data` has issued.
                .with_orm_texture(4096)
                .with_detail_texture(0),
        );
        let set = app.material_textures();
        let entry = set
            .iter()
            .find(|t| t.material_id() == handle.id())
            .expect("the material is in the set");
        assert_eq!(entry.normal().map(|m| m.pixels().to_vec()), Some(vec![77; 4]));
        assert_eq!(entry.orm_height(), None, "a stale id is a missing map");
        assert_eq!(entry.detail(), None, "0 is the cleared value");
        assert_eq!(entry.macro_field(), None);
        // The albedo is still the untextured white default: a normal map is not
        // an albedo, and naming one must not change what the surface is coloured.
        assert_eq!(entry.pixels(), &[255, 255, 255, 255]);
    }
}
