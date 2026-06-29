//! Incremental **runtime scene authoring** on [`RunningApp`] — adding meshes,
//! materials, lights, and the active camera *after* the app is already running,
//! the write-side complement to authoring once in [`crate::prelude::App::setup`].
//!
//! Where `setup` (and the live-rebuild [`RunningApp::reauthor`]) realize a whole
//! scene from a closure at a tick boundary, these methods grow the existing world
//! a piece at a time without rebuilding it: register one more mesh/material into
//! the same stores the initial build filled, spawn a light node, or set/replace
//! the active camera. They reuse the existing resolved-geometry / material stores
//! and the scene's node lifecycle — there is no parallel store. A child module of
//! `app` so it reaches [`RunningApp`]'s private scene + resource tables while
//! keeping `app.rs` within the per-file size budget.
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

use super::RunningApp;
use crate::camera::Camera;
use crate::directional_light::DirectionalLight;
use crate::handle::Handle;
use crate::material::Material;
use crate::mesh::Mesh;
use crate::mesh_geometry::mesh_geometry;

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

    /// Register `material` into the running app's material store and return a
    /// stable [`Handle<Material>`] addressing it — the runtime counterpart to
    /// `materials.add(..)` in `setup`. The stored entry is the material's base
    /// colour plus its optional albedo texture, exactly as the initial build
    /// records it.
    pub fn add_material(&mut self, material: Material) -> Handle<Material> {
        let id = self.materials.len() as u64 + 1;
        self.materials
            .push((id, material.base_color().to_array(), material.texture()));
        Handle::new(id)
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
                Vec3::new(light.color.r.get(), light.color.g.get(), light.color.b.get()),
                light.intensity,
            )
            .expect("authored light parameters are valid");
        self.light_direction = light.direction;
        self.scene.update_world_transforms();
        node
    }

    /// Set (replacing any existing one) the active camera: drop every camera
    /// already in the scene, then spawn a fresh camera node carrying `camera` at
    /// `transform`. The render path uses the scene's first camera, so removing the
    /// others guarantees the new one is the camera every subsequent frame renders
    /// through. The projection resolves against the current viewport aspect.
    pub fn set_camera(&mut self, camera: Camera, transform: Transform) {
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

        let aspect =
            self.viewport.physical_width() as f32 / self.viewport.physical_height() as f32;
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
        // A bare app draws nothing.
        assert!(app.tick(0).draws().is_empty());

        // Runtime-register two meshes and two materials; the handles are distinct.
        let cube = app.add_mesh(Mesh::cube());
        let sphere = app.add_mesh(Mesh::sphere());
        assert_ne!(cube, sphere);
        let red = app.add_material(Material::lit(Color::linear_rgb(ch(0.9), ch(0.1), ch(0.1))));
        let blue = app.add_material(Material::lit(Color::linear_rgb(ch(0.1), ch(0.1), ch(0.9))));
        assert_ne!(red, blue);

        // Spawn renderables from the runtime handles (reusing RunningApp::spawn):
        // each handle resolves, so both objects draw with their distinct colours.
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
        // The two distinct meshes resolve to distinct geometry ids in the upload set.
        let mesh_set = app.mesh_set();
        assert_eq!(mesh_set.len(), 2);
        assert_ne!(mesh_set[0].0, mesh_set[1].0);
    }

    #[test]
    fn add_light_adds_a_light_visible_to_the_renderer() {
        let mut app = empty_render_app();
        // No light yet: the renderer resolves zero lights.
        assert!(app.tick(0).lights().is_empty());

        let entity = app.add_light(
            DirectionalLight {
                direction: Vec3::new(0.3, -1.0, 0.4),
                color: Color::WHITE,
                intensity: ch(1.0),
            },
            Transform::from_translation(Vec3::new(0.0, 5.0, 0.0)),
        );

        // The renderer now resolves exactly one light.
        assert_eq!(app.tick(1).lights().len(), 1);
        // The returned entity is a live, addressable node carrying the transform.
        assert_eq!(
            app.get::<Transform>(entity).map(|t| t.translation),
            Some(Vec3::new(0.0, 5.0, 0.0))
        );
    }

    #[test]
    fn set_camera_sets_and_then_replaces_the_active_camera() {
        let mut app = empty_render_app();
        // With no camera the view-projection is identity.
        let identity = app.tick(0).camera_view_proj();

        // Setting a camera makes the view-projection non-identity.
        app.set_camera(camera(), Transform::from_translation(Vec3::new(0.0, 0.0, 8.0)));
        let near = app.tick(1).camera_view_proj();
        assert_ne!(near, identity, "a camera replaces the identity view");

        // Replacing the camera (a different position) changes the view again —
        // proving the old camera was dropped, not merely shadowed by a second one.
        app.set_camera(
            camera(),
            Transform::from_translation(Vec3::new(0.0, 0.0, 20.0)),
        );
        let far = app.tick(2).camera_view_proj();
        assert_ne!(far, near, "set_camera replaces the active camera");
        // Exactly one camera remains in the scene after the replacement.
        assert_eq!(app.tick(3).camera_view_proj(), far);
    }
}
