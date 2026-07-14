//! Incremental **runtime scene authoring** on [`RunningApp`] — adding meshes,
//! materials, lights, and the active camera *after* the app is already running,
//! the write-side complement to authoring once in [`crate::prelude::App::setup`].
//!
//! Where `setup` (and the live-rebuild [`RunningApp::reauthor`]) realize a whole
//! scene from a closure at a tick boundary, these methods grow the existing world
//! a piece at a time without rebuilding it: register one more mesh/material into
//! the same stores the initial build filled, spawn a light node, or set/replace
//! the active camera. They reuse the existing resolved-geometry / material stores
//! and the scene's node lifecycle — there is no parallel store.
//!
//! A renderable node is *spawned* from a `(Handle<Mesh>, Handle<Material>)` pair
//! through the existing [`RunningApp::spawn`] (a [`crate::prelude::Spawn`]
//! request) — the handles these methods return are exactly the ones it consumes,
//! so no new spawn entry point is needed.
//!
//! Headless [`RunningApp::tick`] re-reads the mesh/material stores every frame, so
//! a runtime-added mesh/material is uploaded into the next frame's render input
//! immediately. The **live windowing backend**, by contrast, sizes its vertex and
//! instance buffers once at startup (see [`RunningApp::reauthor`]); a mesh added
//! after `run` is therefore visible to the deterministic headless path and to a
//! freshly built live backend, not retroactively to an already-running one.

use axiom_kernel::{Radians, Ratio};
use axiom_math::{MathApi, Transform, Vec3};
use axiom_scene::SceneNodeId as Entity;

use axiom_math::Mat4;

use super::{PendingSkinned, RunningApp};
use crate::camera::Camera;
use crate::controller::FirstPersonInput;
use crate::directional_light::DirectionalLight;
use crate::handle::Handle;
use crate::material::Material;
use crate::mesh::Mesh;
use crate::mesh_data::{MeshData, MeshDataError};
use crate::mesh_geometry::{mesh_data_geometry, mesh_geometry};
use crate::point_light::PointLight;
use crate::texture::Texture;

/// Why app-supplied texture pixels are not valid renderable data. Returned by
/// [`RunningApp::add_texture_data`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureDataError {
    /// Zero width/height, or the pixel buffer length is not exactly
    /// `width * height * 4` (RGBA8, row-major).
    Malformed,
}

impl RunningApp {
    /// Register `mesh` into the running app's resolved-geometry store and return a
    /// stable [`Handle<Mesh>`] addressing it — the runtime counterpart to
    /// `meshes.add(..)` in `setup`. The handle's 1-based id matches the store's
    /// registration order, so it is interchangeable with a `setup`-minted handle
    /// in a [`crate::prelude::Spawn`].
    pub fn add_mesh(&mut self, mesh: Mesh) -> Handle<Mesh> {
        let id = self.meshes.len() as u64 + 1;
        self.meshes.push((id, mesh_geometry(&mesh)));
        Handle::new(id)
    }

    /// Register author-supplied [`MeshData`] (explicit positions / normals /
    /// optional UVs / triangle indices) into the running app's resolved-geometry
    /// store and return a stable [`Handle<Mesh>`] addressing it — the non-catalog
    /// counterpart to [`Self::add_mesh`]. The geometry is validated (finite
    /// coordinates, one normal per vertex, optional UVs matching the vertex count,
    /// a non-empty in-range triangle-list) and threaded through the SAME
    /// `axiom-resources` resolution the built-in primitives use, so the returned
    /// handle is interchangeable with a primitive's in a [`crate::prelude::Spawn`].
    /// Malformed geometry returns the offending [`MeshDataError`] and registers
    /// nothing (the store is left untouched).
    pub fn add_mesh_data(&mut self, data: MeshData) -> Result<Handle<Mesh>, MeshDataError> {
        mesh_data_geometry(&data).map(|geometry| {
            let id = self.meshes.len() as u64 + 1;
            self.meshes.push((id, geometry));
            Handle::new(id)
        })
    }

    /// Queue a **skinned** draw for this frame: `mesh` (registered via
    /// [`Self::add_mesh_data`] with skin streams) deformed by `palette` — the
    /// per-bone joint matrices from `AnimationApi::joint_matrices` — at `transform`,
    /// tinted by `material`'s colour. Unlike a spawned node this is **not retained**:
    /// it is drawn once, this frame, and must be re-submitted every frame with the
    /// current pose. This is how an app renders a bake-once, pose-per-frame
    /// character (skeletal skinning) without re-baking geometry.
    pub fn submit_skinned_draw(
        &mut self,
        mesh: Handle<Mesh>,
        material: Handle<Material>,
        transform: Transform,
        palette: &[Mat4],
    ) {
        let color = self
            .materials
            .iter()
            .find(|(id, _)| *id == material.id())
            .map(|(_, m)| m.base_color().to_array())
            .expect("skinned draw references a registered material");
        self.pending_skinned.push(PendingSkinned {
            mesh_id: mesh.id(),
            material_id: material.id(),
            color,
            world: transform.to_matrix().as_cols_array(),
            palette: palette.iter().map(|m| m.as_cols_array()).collect(),
        });
    }

    /// Register `material` into the running app's material store and return a
    /// stable [`Handle<Material>`] addressing it — the runtime counterpart to
    /// `materials.add(..)` in `setup`. The store keeps the full [`Material`] — its
    /// base colour, optional albedo texture, AND its catalog surface (emissive /
    /// roughness / opacity) — so an authored translucent / emissive / rough
    /// material reaches the renderer intact, exactly as the initial build records it.
    pub fn add_material(&mut self, material: Material) -> Handle<Material> {
        let id = self.materials.len() as u64 + 1;
        self.materials.push((id, material));
        Handle::new(id)
    }

    /// Register an app-authored raw-pixel albedo texture — `width * height` RGBA8
    /// pixels (row-major, 4 bytes/pixel) — into the running app's custom-texture
    /// store and return a stable [`Handle<Texture>`] whose 1-based id a
    /// [`Material::with_custom_texture`] references. This is the raw-pixel
    /// counterpart to [`Self::add_mesh_data`] for geometry: it lets an app supply
    /// hand-authored textures the built-in [`Texture`] enum cannot express. The
    /// pixels are resolved by `material_textures` and reach every backend (the same
    /// upload lane the built-in textures use). Malformed input (zero dimensions or
    /// a pixel buffer that is not exactly `width * height * 4` bytes) returns
    /// [`TextureDataError::Malformed`] and registers nothing.
    pub fn add_texture_data(
        &mut self,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    ) -> Result<Handle<Texture>, TextureDataError> {
        let expected = width as usize * height as usize * 4;
        let ok = (width > 0) & (height > 0) & (pixels.len() == expected);
        ok.then_some(())
            .map(|()| {
                let id = self.custom_textures.len() as u64 + 1;
                self.custom_textures.push((id, width, height, pixels));
                Handle::new(id)
            })
            .ok_or(TextureDataError::Malformed)
    }

    /// Spawn a directional-light node carrying `transform` and return its
    /// [`Entity`]. The light's colour + intensity attach to the new node, and its
    /// world-space `direction` becomes the frame's sun direction (matching the
    /// `setup` semantics, where the last directional light wins). World transforms
    /// refresh so the node is immediately queryable.
    pub fn add_light(&mut self, light: DirectionalLight, transform: Transform) -> Entity {
        let math = MathApi::new();
        let node = self.scene.create_node_with_transform(transform);
        self.scene
            .add_directional_light(
                &math,
                node,
                Vec3::new(
                    light.color.r.get(),
                    light.color.g.get(),
                    light.color.b.get(),
                ),
                light.intensity,
            )
            .expect("authored light parameters are valid");
        self.light_direction = light.direction;
        self.scene.update_world_transforms();
        node
    }

    /// Spawn a point-light node carrying `transform` and return its [`Entity`] —
    /// the positional counterpart to [`Self::add_light`]. The light's colour +
    /// intensity attach to the new node and radiate from its world position, so a
    /// scene can place focal lights (a lamp, a fixture, a glowing prop) exactly
    /// where they belong. World transforms refresh so the node is immediately
    /// queryable.
    pub fn add_point_light(&mut self, light: PointLight, transform: Transform) -> Entity {
        let math = MathApi::new();
        let node = self.scene.create_node_with_transform(transform);
        self.scene
            .add_point_light(
                &math,
                node,
                Vec3::new(
                    light.color.r.get(),
                    light.color.g.get(),
                    light.color.b.get(),
                ),
                light.intensity,
            )
            .expect("authored point-light parameters are valid");
        self.scene.update_world_transforms();
        node
    }

    /// Set (replacing any existing one) the active camera: drop every camera
    /// already in the scene, then spawn a fresh camera node carrying `camera` at
    /// `transform`, and return its [`Entity`]. The render path uses the scene's
    /// first camera, so removing the others guarantees the new one is the camera
    /// every subsequent frame renders through. The projection resolves against the
    /// current viewport aspect.
    pub fn set_camera(&mut self, camera: Camera, transform: Transform) -> Entity {
        let math = MathApi::new();
        let existing: Vec<Entity> = self
            .scene
            .snapshot()
            .cameras()
            .iter()
            .map(|cam| cam.node())
            .collect();
        existing.into_iter().for_each(|node| {
            let _ = self.scene.remove_camera(node);
        });

        let aspect = self.viewport.physical_width() as f32 / self.viewport.physical_height() as f32;
        let projection = camera.projection();
        let node = self.scene.create_node_with_transform(transform);
        self.scene
            .add_perspective_camera(
                &math,
                node,
                Radians::new(projection.fov_y.as_radians()).expect("authored fov is finite"),
                Ratio::new(aspect).expect("authored aspect is finite"),
                projection.near,
                projection.far,
            )
            .expect("authored camera intrinsics are valid");
        self.scene.update_world_transforms();
        node
    }

    /// Spawn the active camera as a **first-person controller** for `index`: set
    /// the camera at `transform` (dropping any existing one) and mark its node a
    /// [`crate::prelude::Controller`], returning the node. The returned controller
    /// is then driven each frame with [`Self::control`] — the engine yaws, pitches,
    /// and moves the camera node itself, so a game never re-authors the camera
    /// transform; it just hands the engine a per-frame [`FirstPersonInput`].
    pub fn spawn_controller(&mut self, camera: Camera, transform: Transform, index: u32) -> Entity {
        let node = self.set_camera(camera, transform);
        self.scene
            .add_controller(node, index)
            .expect("the controller node was just created");
        node
    }

    /// Apply one first-person input to the controller **immediately** (zero-lag):
    /// yaw and pitch the camera node (pitch clamped by the engine) and move it
    /// along its yaw-only frame, recomputing world transforms now. The per-frame
    /// drive a host that owns its own loop calls instead of re-authoring the
    /// camera — the engine owns the camera; the game owns only the
    /// [`FirstPersonInput`] intent.
    pub fn control(&mut self, input: FirstPersonInput) {
        let yaw = Radians::new(input.yaw.as_radians()).expect("authored yaw is finite");
        let pitch = Radians::new(input.pitch.as_radians()).expect("authored pitch is finite");
        self.scene
            .control_now(input.index, input.move_local, yaw, pitch, input.seat_y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::angle::Angle;
    use crate::app::App;
    use crate::camera::PerspectiveProjection;
    use crate::color::Color;
    use crate::default_plugins::DefaultPlugins;
    use crate::spawn::Spawn;
    use crate::window::Window;
    use axiom_kernel::Meters;
    use axiom_math::Vec2;

    /// A linear colour channel from a known-finite authored literal.
    fn ch(value: f32) -> Ratio {
        Ratio::new(value).expect("authored colour channel is finite")
    }

    /// A perspective camera looking down -Z.
    fn camera() -> Camera {
        Camera::perspective(PerspectiveProjection {
            fov_y: Angle::degrees(60.0),
            near: Meters::new(0.1).expect("near plane is finite"),
            far: Meters::new(100.0).expect("far plane is finite"),
        })
    }

    /// A bare rendering app with empty mesh/material stores and no scene content —
    /// the starting point for incremental runtime authoring.
    fn empty_render_app() -> RunningApp {
        App::new()
            .window(Window::new(64, 64))
            .add_plugins(DefaultPlugins)
            .build()
    }

    #[test]
    fn add_mesh_and_add_material_yield_distinct_usable_handles() {
        let mut app = empty_render_app();
        assert!(app.tick(0).draws().is_empty());

        let cube = app.add_mesh(Mesh::cube());
        let sphere = app.add_mesh(Mesh::sphere());
        assert_ne!(cube, sphere);
        let red = app.add_material(Material::lit(Color::linear_rgb(ch(0.9), ch(0.1), ch(0.1))));
        let blue = app.add_material(Material::lit(Color::linear_rgb(ch(0.1), ch(0.1), ch(0.9))));
        assert_ne!(red, blue);

        app.spawn(Spawn::new(
            Transform::from_translation(Vec3::new(-1.0, 0.0, 0.0)),
            cube,
            red,
        ));
        app.spawn(Spawn::new(
            Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)),
            sphere,
            blue,
        ));

        let outcome = app.tick(1);
        assert_eq!(outcome.draws().len(), 2, "both runtime nodes render");
        let colors: Vec<[f32; 4]> = outcome.draws().iter().map(|d| d.color()).collect();
        assert_ne!(colors[0], colors[1], "the two materials are distinct");
        let mesh_set = app.mesh_set();
        assert_eq!(mesh_set.len(), 2);
        assert_ne!(mesh_set[0].0, mesh_set[1].0);
    }

    /// A unit quad authored from explicit vertex data (4 verts, 2 triangles).
    fn quad_mesh_data() -> MeshData {
        MeshData::new(
            vec![
                Vec3::new(-0.5, -0.5, 0.0),
                Vec3::new(0.5, -0.5, 0.0),
                Vec3::new(0.5, 0.5, 0.0),
                Vec3::new(-0.5, 0.5, 0.0),
            ],
            vec![Vec3::new(0.0, 0.0, 1.0); 4],
            vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(1.0, 0.0),
                Vec2::new(1.0, 1.0),
                Vec2::new(0.0, 1.0),
            ],
            vec![0, 1, 2, 0, 2, 3],
        )
    }

    #[test]
    fn add_mesh_data_registers_author_geometry_that_spawns_and_renders() {
        let mut app = empty_render_app();
        let quad = app
            .add_mesh_data(quad_mesh_data())
            .expect("the authored quad is valid geometry");
        let white = app.add_material(Material::lit(Color::linear_rgb(ch(1.0), ch(1.0), ch(1.0))));

        app.spawn(Spawn::new(Transform::IDENTITY, quad, white));
        assert_eq!(app.tick(0).draws().len(), 1, "the authored mesh renders");

        let mesh_set = app.mesh_set();
        assert_eq!(mesh_set.len(), 1);
        assert_eq!(mesh_set[0].1.len(), 4 * 12, "4 authored vertices uploaded");
        assert_eq!(
            mesh_set[0].2,
            vec![0, 1, 2, 0, 2, 3],
            "authored indices intact"
        );
    }

    #[test]
    fn add_mesh_data_handle_interleaves_with_primitive_handles() {
        let mut app = empty_render_app();
        let cube = app.add_mesh(Mesh::cube());
        let quad = app
            .add_mesh_data(quad_mesh_data())
            .expect("the authored quad is valid geometry");
        assert_eq!(cube.id(), 1);
        assert_eq!(quad.id(), 2);
        assert_eq!(app.mesh_set().len(), 2);
    }

    #[test]
    fn add_mesh_data_rejects_malformed_geometry_and_registers_nothing() {
        let mut app = empty_render_app();
        let bad = MeshData::new(
            vec![Vec3::ZERO, Vec3::UNIT_X, Vec3::UNIT_Y],
            vec![Vec3::UNIT_Z, Vec3::UNIT_Z, Vec3::UNIT_Z],
            vec![],
            vec![0, 1, 7],
        );
        assert_eq!(app.add_mesh_data(bad), Err(MeshDataError::IndexOutOfRange));
        assert!(
            app.mesh_set().is_empty(),
            "a rejected mesh registers nothing"
        );
    }

    /// A unit quad with skin streams (every vertex split 50/50 between bones 0, 1).
    fn skinned_quad_mesh_data() -> MeshData {
        MeshData::new_skinned(
            vec![
                Vec3::new(-0.5, -0.5, 0.0),
                Vec3::new(0.5, -0.5, 0.0),
                Vec3::new(0.5, 0.5, 0.0),
                Vec3::new(-0.5, 0.5, 0.0),
            ],
            vec![Vec3::new(0.0, 0.0, 1.0); 4],
            vec![],
            vec![[0, 1, 0, 0]; 4],
            vec![[0.5, 0.5, 0.0, 0.0]; 4],
            vec![0, 1, 2, 0, 2, 3],
        )
    }

    #[test]
    fn skinned_mesh_uploads_through_the_skinned_set_not_the_static_set() {
        let mut app = empty_render_app();
        app.add_mesh_data(skinned_quad_mesh_data())
            .expect("valid skinned mesh");
        // The static set excludes skinned meshes...
        assert!(app.mesh_set().is_empty());
        // ...and the skinned set carries the 20-float stream.
        let skinned = app.skinned_mesh_set();
        assert_eq!(skinned.len(), 1);
        assert_eq!(
            skinned[0].1.len(),
            4 * 20,
            "4 skinned vertices at 20 floats each"
        );
        assert_eq!(skinned[0].2, vec![0, 1, 2, 0, 2, 3]);
        // The joints then weights ride at floats [12..20] of the first vertex.
        assert_eq!(
            &skinned[0].1[12..20],
            &[0.0, 1.0, 0.0, 0.0, 0.5, 0.5, 0.0, 0.0]
        );
    }

    #[test]
    fn submit_skinned_draw_appears_in_the_outcome_and_drains_each_frame() {
        let mut app = empty_render_app();
        let mesh = app
            .add_mesh_data(skinned_quad_mesh_data())
            .expect("valid skinned mesh");
        let mat = app.add_material(Material::lit(Color::linear_rgb(ch(0.2), ch(0.4), ch(0.8))));
        let palette = [Mat4::IDENTITY, Mat4::IDENTITY];
        app.submit_skinned_draw(mesh, mat, Transform::IDENTITY, &palette);

        let outcome = app.tick(0);
        assert_eq!(
            outcome.skinned_draws().len(),
            1,
            "the submitted skinned draw is in the frame"
        );
        let d = &outcome.skinned_draws()[0];
        assert_eq!(d.mesh_id(), mesh.id());
        assert_eq!(d.material_id(), mat.id());
        assert_eq!(d.joints().len(), 2, "the two-bone palette is carried");
        assert_eq!(d.color()[3], 1.0, "opaque tint from the material");
        // World is IDENTITY, so mvp == view_projection and world round-trips.
        assert_eq!(d.world(), Mat4::IDENTITY.as_cols_array());
        assert_eq!(d.mvp().len(), 16);
        // The queue drains: a frame with no fresh submission has no skinned draws.
        assert_eq!(app.tick(1).skinned_draws().len(), 0);
    }

    #[test]
    fn add_texture_data_registers_pixels_surfaced_by_material_textures() {
        let mut app = empty_render_app();
        // A 1x2 texture: one red texel, one green texel (8 RGBA8 bytes).
        let tex = app
            .add_texture_data(1, 2, vec![255, 0, 0, 255, 0, 255, 0, 255])
            .expect("well-formed pixels register");
        let material = app.add_material(
            Material::lit(Color::linear_rgb(ch(1.0), ch(1.0), ch(1.0)))
                .with_custom_texture(tex.id()),
        );
        // Spawn + tick a renderable with the custom-textured material, so the
        // per-frame material build (which reports the texture id) runs it.
        let cube = app.add_mesh(Mesh::cube());
        app.spawn(Spawn::new(Transform::IDENTITY, cube, material));
        assert_eq!(
            app.tick(0).draws().len(),
            1,
            "the custom-textured mesh renders"
        );
        // The material set carries the app's custom pixels for that material id,
        // not the untextured 1x1 white default.
        let set = app.material_textures();
        let entry = set
            .iter()
            .find(|(id, _, _, _)| *id == material.id())
            .expect("the material is in the set");
        assert_eq!((entry.1, entry.2), (1, 2), "authored dimensions surface");
        assert_eq!(
            entry.3,
            vec![255, 0, 0, 255, 0, 255, 0, 255],
            "authored pixels surface"
        );
        // An untextured material still gets the 1x1 white fallback.
        let plain = app.add_material(Material::lit(Color::linear_rgb(ch(0.5), ch(0.5), ch(0.5))));
        let plain_entry = app
            .material_textures()
            .into_iter()
            .find(|(id, _, _, _)| *id == plain.id())
            .expect("plain material present");
        assert_eq!(
            (plain_entry.1, plain_entry.2, plain_entry.3),
            (1, 1, vec![255, 255, 255, 255])
        );
    }

    #[test]
    fn add_texture_data_rejects_malformed_and_registers_nothing() {
        let mut app = empty_render_app();
        // Zero dimension.
        assert_eq!(
            app.add_texture_data(0, 4, vec![]),
            Err(TextureDataError::Malformed)
        );
        // Pixel buffer length != width*height*4.
        assert_eq!(
            app.add_texture_data(2, 2, vec![0; 8]),
            Err(TextureDataError::Malformed)
        );
        // A rejected texture registers nothing: the next valid one still gets id 1,
        // and a material referencing it resolves the authored pixels.
        let tex = app
            .add_texture_data(1, 1, vec![9, 8, 7, 255])
            .expect("valid pixels register");
        assert_eq!(tex.id(), 1, "no phantom ids from rejected textures");
        let material = app.add_material(Material::lit(Color::WHITE).with_custom_texture(tex.id()));
        let entry = app
            .material_textures()
            .into_iter()
            .find(|(id, _, _, _)| *id == material.id())
            .expect("material present");
        assert_eq!(entry.3, vec![9, 8, 7, 255]);
    }

    #[test]
    fn add_light_adds_a_light_visible_to_the_renderer() {
        let mut app = empty_render_app();
        assert!(app.tick(0).lights().is_empty());

        let entity = app.add_light(
            DirectionalLight {
                direction: Vec3::new(0.3, -1.0, 0.4),
                color: Color::WHITE,
                intensity: ch(1.0),
            },
            Transform::from_translation(Vec3::new(0.0, 5.0, 0.0)),
        );

        assert_eq!(app.tick(1).lights().len(), 1);
        assert_eq!(
            app.get::<Transform>(entity).map(|t| t.translation),
            Some(Vec3::new(0.0, 5.0, 0.0))
        );
    }

    #[test]
    fn add_point_light_adds_a_positional_light() {
        let mut app = empty_render_app();
        assert!(app.tick(0).lights().is_empty());

        let entity = app.add_point_light(
            PointLight {
                color: Color::WHITE,
                intensity: ch(1.0),
            },
            Transform::from_translation(Vec3::new(2.0, 3.0, -1.0)),
        );

        assert_eq!(app.tick(1).lights().len(), 1);
        assert_eq!(
            app.get::<Transform>(entity).map(|t| t.translation),
            Some(Vec3::new(2.0, 3.0, -1.0))
        );
    }

    #[test]
    fn set_camera_sets_and_then_replaces_the_active_camera() {
        let mut app = empty_render_app();
        let identity = app.tick(0).camera_view_proj();

        app.set_camera(
            camera(),
            Transform::from_translation(Vec3::new(0.0, 0.0, 8.0)),
        );
        let near = app.tick(1).camera_view_proj();
        assert_ne!(near, identity, "a camera replaces the identity view");

        // A second `set_camera` must drop the old one, not merely shadow it with
        // a second camera in the scene.
        app.set_camera(
            camera(),
            Transform::from_translation(Vec3::new(0.0, 0.0, 20.0)),
        );
        let far = app.tick(2).camera_view_proj();
        assert_ne!(far, near, "set_camera replaces the active camera");
        assert_eq!(app.tick(3).camera_view_proj(), far);
    }

    #[test]
    fn spawn_controller_drives_the_camera_immediately_via_first_person_input() {
        let mut app = empty_render_app();
        let cam = app.spawn_controller(
            camera(),
            Transform::from_translation(Vec3::new(0.0, 1.0, 5.0)),
            0,
        );
        assert_ne!(app.tick(0).camera_view_proj(), [0.0; 16]);

        // `control` applies immediately — no tick, no re-authoring the camera
        // transform.
        app.control(FirstPersonInput::new(
            0,
            Vec3::new(0.0, 0.0, -1.0),
            Angle::radians(0.0),
            Angle::radians(0.0),
        ));
        assert_eq!(
            app.world_transform(cam).map(|t| t.translation),
            Some(Vec3::new(0.0, 1.0, 4.0)),
            "forward at yaw 0 moves the camera node along -Z, applied now"
        );

        // Yaw 90° then move forward again: the move frame rotates with the yaw,
        // so the engine (not the game) rotates the movement.
        app.control(FirstPersonInput::new(
            0,
            Vec3::new(0.0, 0.0, -1.0),
            Angle::radians(std::f32::consts::FRAC_PI_2),
            Angle::radians(0.0),
        ));
        let turned = app
            .world_transform(cam)
            .expect("controller node is live")
            .translation;
        assert!(
            turned.x.abs() > 0.5,
            "yaw rotated the move frame off the z axis"
        );

        let before = app.world_transform(cam).map(|t| t.translation);
        app.control(FirstPersonInput::new(
            9,
            Vec3::new(0.0, 0.0, -1.0),
            Angle::radians(0.0),
            Angle::radians(0.0),
        ));
        assert_eq!(app.world_transform(cam).map(|t| t.translation), before);
    }
}
