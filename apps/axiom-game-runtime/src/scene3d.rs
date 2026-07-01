//! 3D scene authoring composed into the bridge: the `createMesh` / `createMaterial`
//! / `setCamera3D` / `addLight` verbs the TS `HostBridge` 3D surface projects,
//! every one forwarding to the engine's **runtime scene authoring** on
//! [`RunningApp`](axiom::prelude::RunningApp) (`add_mesh` / `add_material` /
//! `set_camera` / `add_light`). These grow the already-running scene a piece at a
//! time; the resolved-geometry / material stores and the node lifecycle are the
//! engine's, never duplicated here.
//!
//! ## Boundary convention (the established slice / scalar / handle rule)
//! A mesh kind crosses as its `string` name (`"cube"` / `"sphere"` / `"plane"` /
//! `"cylinder"`; an unknown kind falls back to a cube — a table select, never a
//! branch). A colour crosses as a 3-element linear `&[f64]` slice, a position /
//! direction likewise; a camera adds its `(fovDeg, near, far)` scalars, a light
//! its `intensity` scalar (one slice per vector keeps every method within the
//! engine's argument-count budget). Every authoring call returns the engine
//! handle it minted as a raw `u64` (`f64` at the JS edge): a mesh / material
//! `Handle` id, or a light node [`Entity`](axiom::prelude::Entity) id.

use axiom::prelude::{
    Angle, Bounds, Camera, Color, DirectionalLight, Entity, FirstPersonInput, Handle, Material,
    Mesh, MeshData, Meters, PerspectiveProjection, Ratio, Spawn, Transform, Vec2, Vec3,
};
use axiom_math::Quat;

use crate::GameBridge;

/// A finite [`Meters`] from a boundary scalar (non-finite ⇒ zero).
fn meters(value: f64) -> Meters {
    Meters::new(value as f32).unwrap_or_else(|_| Meters::new(0.0).expect("0.0 is finite"))
}

/// A linear colour channel / intensity from a boundary scalar, sanitized to a
/// finite [`Ratio`] (non-finite ⇒ zero) without a branch.
fn channel(value: f64) -> Ratio {
    Ratio::finite_or_zero(value as f32)
}

/// A `Vec3` from a 3-element boundary slice (missing entries read `0`).
fn v3(s: &[f64]) -> Vec3 {
    let [x, y, z]: [f32; 3] = core::array::from_fn(|i| *s.get(i).unwrap_or(&0.0) as f32);
    Vec3::new(x, y, z)
}

/// A flat `[x,y,z, …]` boundary slice as a `Vec<Vec3>` (a trailing partial triple
/// is dropped — the SDK always sends whole triples).
fn v3_list(s: &[f64]) -> Vec<Vec3> {
    s.chunks_exact(3)
        .map(|c| Vec3::new(c[0] as f32, c[1] as f32, c[2] as f32))
        .collect()
}

/// A flat `[u,v, …]` boundary slice as a `Vec<Vec2>` (a trailing partial pair is
/// dropped — the SDK always sends whole pairs).
fn v2_list(s: &[f64]) -> Vec<Vec2> {
    s.chunks_exact(2)
        .map(|c| Vec2::new(c[0] as f32, c[1] as f32))
        .collect()
}

/// A lit material colour from a 3-element linear `[r, g, b]` slice.
fn color(s: &[f64]) -> Color {
    Color::linear_rgb(
        channel(*s.first().unwrap_or(&0.0)),
        channel(*s.get(1).unwrap_or(&0.0)),
        channel(*s.get(2).unwrap_or(&0.0)),
    )
}

/// A [`Transform`] from the flat 10-tuple `[tx, ty, tz, qx, qy, qz, qw, sx, sy,
/// sz]` the boundary uses — the exact shape `worldWorldTransform` reads back, so
/// an author can round-trip a node's pose. Missing entries read `0` (a table read,
/// never a branch); the SDK always sends a valid (identity-by-default) quaternion,
/// so no normalisation guard is needed here.
fn transform_from_tuple(t: &[f64]) -> Transform {
    let g = |i: usize| *t.get(i).unwrap_or(&0.0) as f32;
    Transform::new(
        Vec3::new(g(0), g(1), g(2)),
        Quat::new(g(3), g(4), g(5), g(6)),
        Vec3::new(g(7), g(8), g(9)),
    )
}

impl GameBridge {
    /// Register a primitive mesh of `kind` and return its handle id
    /// (`createMesh`); an unknown kind is the cube fallback (a table select).
    pub fn create_mesh(&mut self, kind: &str) -> u64 {
        let index = ["cube", "sphere", "plane", "cylinder"]
            .iter()
            .position(|name| *name == kind)
            .unwrap_or(0);
        let mesh = [Mesh::cube(), Mesh::sphere(), Mesh::plane(), Mesh::cylinder()]
            .into_iter()
            .nth(index)
            .unwrap_or_else(Mesh::cube);
        self.runtime.app_mut().add_mesh(mesh).id()
    }

    /// Register an author-supplied mesh from flat vertex arrays (`createMeshData`):
    /// `positions` / `normals` as flat `[x,y,z, …]` triples, `uvs` as flat
    /// `[u,v, …]` pairs (an empty slice ⇒ each vertex's UV defaults to the
    /// origin), and `indices` a triangle list into the vertices. The engine
    /// validates the geometry and threads it through the same resolved-geometry
    /// store the primitive `create_mesh` uses, so the returned handle spawns
    /// exactly like a catalog mesh. Malformed geometry yields `0` — the null
    /// handle the SDK reads as "no mesh" — never a panic at the boundary.
    pub fn create_mesh_data(
        &mut self,
        positions: &[f64],
        normals: &[f64],
        uvs: &[f64],
        indices: &[u32],
    ) -> u64 {
        let data = MeshData::new(v3_list(positions), v3_list(normals), v2_list(uvs), indices.to_vec());
        self.runtime
            .app_mut()
            .add_mesh_data(data)
            .map_or(0, |handle| handle.id())
    }

    /// Register a fully-specified lit material and return its handle id
    /// (`createMaterial`): linear base colour `[r, g, b]`, linear `emissive`
    /// `[r, g, b]` self-illumination, `roughness` (`0` smooth … `1` matte), and
    /// `opacity` (`1` opaque; a translucent material folds into the per-draw
    /// alpha). The slice/scalar boundary mirrors `set_camera_3d`: one slice per
    /// colour vector, the catalog ratios as trailing scalars. Each scalar is
    /// sanitized to a finite [`Ratio`] (non-finite ⇒ zero) without a branch.
    pub fn create_material(&mut self, rgb: &[f64], emissive: &[f64], roughness: f64, opacity: f64) -> u64 {
        let material = Material::lit(color(rgb))
            .with_emissive(color(emissive))
            .with_roughness(channel(roughness))
            .with_opacity(channel(opacity));
        self.runtime.app_mut().add_material(material).id()
    }

    /// Set (replacing any existing) the active perspective camera at `position`
    /// aimed at `target` (`setCamera3D`): vertical FOV in degrees, near/far clip
    /// in metres. The camera looks from `position` toward `target` with world
    /// up = +Y; a degenerate aim (eye coincident with `target`, or the look
    /// direction parallel to up) falls back to translation-only — never panics.
    pub fn set_camera_3d(&mut self, position: &[f64], target: &[f64], fov_deg: f64, near: f64, far: f64) {
        let projection = PerspectiveProjection {
            fov_y: Angle::degrees(fov_deg as f32),
            near: meters(near),
            far: meters(far),
        };
        let eye = v3(position);
        let transform = Transform::from_translation(eye)
            .looking_at(v3(target), Vec3::new(0.0, 1.0, 0.0))
            .unwrap_or_else(|_| Transform::from_translation(eye));
        self.runtime
            .app_mut()
            .set_camera(Camera::perspective(projection), transform);
    }

    /// Spawn a directional light (`addLight`): world-space `direction`, linear
    /// `[r, g, b]` colour, and `intensity`. Returns the light node's entity id.
    pub fn add_light(&mut self, direction: &[f64], rgb: &[f64], intensity: f64) -> u64 {
        let light = DirectionalLight {
            direction: v3(direction),
            color: color(rgb),
            intensity: channel(intensity),
        };
        self.runtime
            .app_mut()
            .add_light(light, Transform::IDENTITY)
            .raw()
    }

    /// Spawn a renderable node from a `(mesh, material)` handle pair at the
    /// flat-10-tuple `transform` (`spawnRenderable`), returning its entity id. The
    /// handles are the ones [`Self::create_mesh`] / [`Self::create_material`]
    /// minted; the node draws every frame and can be moved with
    /// [`Self::set_node_transform`] or bounded with [`Self::set_node_bounds`].
    pub fn spawn_renderable(&mut self, mesh_id: u64, material_id: u64, transform: &[f64]) -> u64 {
        self.runtime
            .app_mut()
            .spawn(Spawn::new(
                transform_from_tuple(transform),
                Handle::from_raw(mesh_id),
                Handle::from_raw(material_id),
            ))
            .raw()
    }

    /// Overwrite `entity`'s local transform from the flat 10-tuple
    /// (`setNodeTransform`) — the per-frame move/rotate/scale a game applies to a
    /// renderable (e.g. an enemy walking, the player camera following). A stale
    /// handle is a clean `false`.
    pub fn set_node_transform(&mut self, entity: u64, transform: &[f64]) -> bool {
        let app = self.runtime.app_mut();
        let moved = app.set::<Transform>(Entity::from_raw(entity), transform_from_tuple(transform));
        // A bare `set::<Transform>` defers world-transform propagation to the next
        // tick; commit it now so a query (or the present render) this frame sees the
        // node at its new pose, the move-then-look-then-shoot loop a game runs.
        app.update_world_transforms();
        moved
    }

    /// Set `entity`'s collision bounds to an axis-aligned box of `half_extents`
    /// (`setNodeBounds`), so it participates in the `overlapBox` / `raycast`
    /// queries. A stale handle is a clean `false`.
    pub fn set_node_bounds(&mut self, entity: u64, half_extents: &[f64]) -> bool {
        self.runtime
            .app_mut()
            .set::<Bounds>(Entity::from_raw(entity), Bounds::new(v3(half_extents)))
    }

    /// Clear the whole 3D scene (`clearScene`): drop every renderable, light, and
    /// camera and reset the mesh/material stores, leaving a blank scene to author
    /// from. A 3D game calls this once at startup before building its own scene, so
    /// the runtime's default demo content does not bleed through. After it,
    /// [`Self::create_mesh`] / [`Self::create_material`] mint 1-based handles again.
    pub fn clear_scene(&mut self) {
        self.runtime.app_mut().reauthor(|_scene, _meshes, _materials| {});
    }

    /// Spawn the active camera as a first-person **controller** at `position` with
    /// the given perspective intrinsics, marked controller `index`
    /// (`createController`). Returns the controller node's entity id. Unlike
    /// [`Self::set_camera_3d`], its orientation is not a look-at target — the engine
    /// drives it from per-frame [`Self::control_first_person`] inputs.
    pub fn spawn_controller(&mut self, position: &[f64], fov_deg: f64, near: f64, far: f64, index: u32) -> u64 {
        let projection = PerspectiveProjection {
            fov_y: Angle::degrees(fov_deg as f32),
            near: meters(near),
            far: meters(far),
        };
        self.runtime
            .app_mut()
            .spawn_controller(
                Camera::perspective(projection),
                Transform::from_translation(v3(position)),
                index,
            )
            .raw()
    }

    /// Apply one first-person input to controller `index` **immediately**
    /// (`controlFirstPerson`): move by `move_local` in the node's own frame (-Z
    /// forward, +X right) and yaw/pitch by the given radian deltas. The engine yaws,
    /// pitches (clamped), and moves the camera node now — zero lag, no camera
    /// re-authoring.
    pub fn control_first_person(&mut self, index: u32, move_local: &[f64], yaw: f64, pitch: f64) {
        self.runtime.app_mut().control(FirstPersonInput::new(
            index,
            v3(move_local),
            Angle::radians(yaw as f32),
            Angle::radians(pitch as f32),
        ));
    }
}

#[cfg(target_arch = "wasm32")]
mod wasm_exports {
    use wasm_bindgen::prelude::*;

    use crate::wasm::WasmGame;

    #[wasm_bindgen]
    impl WasmGame {
        /// Register a primitive mesh by kind name (`createMesh`).
        #[wasm_bindgen(js_name = createMesh)]
        pub fn create_mesh(&mut self, kind: String) -> f64 {
            self.bridge.create_mesh(&kind) as f64
        }

        /// Register an author-supplied mesh from flat vertex arrays
        /// (`createMeshData`): `positions` / `normals` flat `[x,y,z,…]` triples,
        /// `uvs` flat `[u,v,…]` pairs (empty ⇒ origin per vertex), `indices` a
        /// triangle list. Returns the mesh handle id, or `0` on malformed geometry.
        #[wasm_bindgen(js_name = createMeshData)]
        pub fn create_mesh_data(
            &mut self,
            positions: &[f64],
            normals: &[f64],
            uvs: &[f64],
            indices: &[u32],
        ) -> f64 {
            self.bridge
                .create_mesh_data(positions, normals, uvs, indices) as f64
        }

        /// Register a fully-specified lit material (`createMaterial`): linear base
        /// colour `[r, g, b]`, linear `emissive` `[r, g, b]`, `roughness`, and
        /// `opacity` (`1` opaque).
        #[wasm_bindgen(js_name = createMaterial)]
        pub fn create_material(&mut self, rgb: &[f64], emissive: &[f64], roughness: f64, opacity: f64) -> f64 {
            self.bridge.create_material(rgb, emissive, roughness, opacity) as f64
        }

        /// Set the active perspective camera, aimed from `position` at `target`
        /// (`setCamera3D`).
        #[wasm_bindgen(js_name = setCamera3D)]
        pub fn set_camera_3d(
            &mut self,
            position: &[f64],
            target: &[f64],
            fov_deg: f64,
            near: f64,
            far: f64,
        ) {
            self.bridge.set_camera_3d(position, target, fov_deg, near, far);
        }

        /// Spawn a directional light, returning its node id (`addLight`).
        #[wasm_bindgen(js_name = addLight)]
        pub fn add_light(&mut self, direction: &[f64], rgb: &[f64], intensity: f64) -> f64 {
            self.bridge.add_light(direction, rgb, intensity) as f64
        }

        /// Spawn a renderable node from a `(mesh, material)` handle pair at the
        /// flat 10-tuple `transform`, returning its entity id (`spawnRenderable`).
        #[wasm_bindgen(js_name = spawnRenderable)]
        pub fn spawn_renderable(&mut self, mesh_id: f64, material_id: f64, transform: &[f64]) -> f64 {
            self.bridge
                .spawn_renderable(mesh_id as u64, material_id as u64, transform) as f64
        }

        /// Overwrite a node's local transform from the flat 10-tuple
        /// (`setNodeTransform`). A stale handle is a clean no-op.
        #[wasm_bindgen(js_name = setNodeTransform)]
        pub fn set_node_transform(&mut self, entity: f64, transform: &[f64]) {
            self.bridge.set_node_transform(entity as u64, transform);
        }

        /// Set a node's collision bounds to a box of `half_extents`
        /// (`setNodeBounds`). A stale handle is a clean no-op.
        #[wasm_bindgen(js_name = setNodeBounds)]
        pub fn set_node_bounds(&mut self, entity: f64, half_extents: &[f64]) {
            self.bridge.set_node_bounds(entity as u64, half_extents);
        }

        /// Clear the whole 3D scene, leaving a blank scene to author (`clearScene`).
        #[wasm_bindgen(js_name = clearScene)]
        pub fn clear_scene(&mut self) {
            self.bridge.clear_scene();
        }

        /// Spawn the active camera as a first-person controller, returning its
        /// entity id (`createController`).
        #[wasm_bindgen(js_name = spawnController)]
        pub fn spawn_controller(
            &mut self,
            position: &[f64],
            fov_deg: f64,
            near: f64,
            far: f64,
            index: u32,
        ) -> f64 {
            self.bridge
                .spawn_controller(position, fov_deg, near, far, index) as f64
        }

        /// Apply one first-person input to a controller immediately
        /// (`controlFirstPerson`): `move_local` plus yaw/pitch radian deltas.
        #[wasm_bindgen(js_name = controlFirstPerson)]
        pub fn control_first_person(&mut self, index: u32, move_local: &[f64], yaw: f64, pitch: f64) {
            self.bridge.control_first_person(index, move_local, yaw, pitch);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::GameBridge;
    use axiom::prelude::{App, DefaultPlugins, Window};

    const STEP: u64 = 1_000_000;

    /// A bridge over a bare scene (empty mesh/material stores), so authored handle
    /// ids start at 1 and are easy to reason about.
    fn bridge() -> GameBridge {
        let app = App::new()
            .window(Window::new(64, 64))
            .add_plugins(DefaultPlugins)
            .build();
        GameBridge::new(app, 0, STEP, 1)
    }

    /// Replay the same authoring script and return the minted handle/entity ids.
    fn authoring_ids() -> Vec<u64> {
        let mut b = bridge();
        let cube = b.create_mesh("cube");
        let sphere = b.create_mesh("sphere");
        let ghost = b.create_mesh("ghost"); // unknown ⇒ cube fallback, fresh handle
        let red = b.create_material(&[0.9, 0.1, 0.1], &[0.0, 0.0, 0.0], 1.0, 1.0);
        let blue = b.create_material(&[0.1, 0.1, 0.9], &[0.0, 0.0, 0.0], 1.0, 1.0);
        b.set_camera_3d(&[0.0, 0.0, 8.0], &[0.0, 0.0, 0.0], 60.0, 0.1, 100.0);
        let light = b.add_light(&[0.3, -1.0, 0.4], &[1.0, 1.0, 1.0], 1.0);
        vec![cube, sphere, ghost, red, blue, light]
    }

    #[test]
    fn authoring_mints_stable_distinct_handles_and_replays() {
        let ids = authoring_ids();
        assert_eq!(ids[0], 1);
        assert_eq!(ids[1], 2);
        assert_eq!(ids[2], 3);
        assert_eq!(ids[3], 1);
        assert_eq!(ids[4], 2);
        assert_ne!(ids[5], 0);
        assert_eq!(ids, authoring_ids());
    }

    #[test]
    fn create_mesh_data_authors_a_renderable_custom_mesh() {
        // SPEC-11 §9: author geometry must ride the same spawn/draw rails as a
        // catalog primitive.
        let mut b = bridge();
        b.set_camera_3d(&[0.0, 0.0, 8.0], &[0.0, 0.0, 0.0], 60.0, 0.1, 100.0);
        let positions = [-0.5, -0.5, 0.0, 0.5, -0.5, 0.0, 0.0, 0.5, 0.0];
        let normals = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0];
        let uvs = [0.0, 0.0, 1.0, 0.0, 0.5, 1.0];
        let mesh = b.create_mesh_data(&positions, &normals, &uvs, &[0, 1, 2]);
        assert_eq!(mesh, 1, "the authored mesh mints a 1-based handle");
        let white = b.create_material(&[1.0, 1.0, 1.0], &[0.0, 0.0, 0.0], 1.0, 1.0);
        let node = b.spawn_renderable(mesh, white, &pose(0.0, 0.0, 0.0, 1.0));
        assert!(b.world_alive(node));
        assert_eq!(b.runtime.app_mut().tick(0).draws().len(), 1, "the custom mesh renders");
    }

    #[test]
    fn create_mesh_data_with_empty_uvs_is_accepted() {
        let mut b = bridge();
        let positions = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let normals = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0];
        let mesh = b.create_mesh_data(&positions, &normals, &[], &[0, 1, 2]);
        assert_eq!(mesh, 1, "a UV-less authored mesh is still registered");
    }

    #[test]
    fn create_mesh_data_rejects_malformed_geometry_with_the_null_handle() {
        let mut b = bridge();
        let positions = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let normals = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0];
        let mesh = b.create_mesh_data(&positions, &normals, &[], &[0, 1, 9]);
        assert_eq!(mesh, 0, "malformed author geometry is the null handle");
    }

    #[test]
    fn set_camera_3d_aims_at_the_target_not_just_the_position() {
        // From a single fixed eye, two distinct look targets must yield two
        // distinct view-projections, proving the aim flows through and not
        // merely the translation.
        let mut b = bridge();
        b.set_camera_3d(&[0.0, 0.0, 8.0], &[0.0, 0.0, 0.0], 60.0, 0.1, 100.0);
        let toward_origin = b.runtime.app_mut().tick(0).camera_view_proj();
        b.set_camera_3d(&[0.0, 0.0, 8.0], &[5.0, 0.0, 0.0], 60.0, 0.1, 100.0);
        let toward_side = b.runtime.app_mut().tick(1).camera_view_proj();
        assert_ne!(
            toward_origin, toward_side,
            "the camera orientation tracks the look target"
        );
    }

    #[test]
    fn set_camera_3d_with_a_degenerate_aim_falls_back_without_panicking() {
        let mut b = bridge();
        b.set_camera_3d(&[1.0, 2.0, 3.0], &[1.0, 2.0, 3.0], 60.0, 0.1, 100.0);
        let view = b.runtime.app_mut().tick(0).camera_view_proj();
        let no_camera = bridge().runtime.app_mut().tick(0).camera_view_proj();
        assert_ne!(view, no_camera, "a camera is installed despite the degenerate aim");
    }

    #[test]
    fn an_added_light_is_a_live_addressable_node() {
        let mut b = bridge();
        let light = b.add_light(&[0.0, -1.0, 0.0], &[1.0, 1.0, 1.0], 1.0);
        assert!(b.world_alive(light));
    }

    /// The identity-rotation flat 10-tuple for a node at `(x, y, z)` with uniform
    /// `scale` — the boundary shape `spawnRenderable` / `setNodeTransform` take.
    fn pose(x: f64, y: f64, z: f64, scale: f64) -> [f64; 10] {
        [x, y, z, 0.0, 0.0, 0.0, 1.0, scale, scale, scale]
    }

    #[test]
    fn a_spawned_renderable_draws_and_can_be_moved_and_cleared() {
        let mut b = bridge();
        b.set_camera_3d(&[0.0, 0.0, 8.0], &[0.0, 0.0, 0.0], 60.0, 0.1, 100.0);
        let cube = b.create_mesh("cube");
        let red = b.create_material(&[0.9, 0.1, 0.1], &[0.0, 0.0, 0.0], 1.0, 1.0);
        let node = b.spawn_renderable(cube, red, &pose(0.0, 0.0, 0.0, 1.0));
        assert!(b.world_alive(node));
        assert_eq!(b.runtime.app_mut().tick(0).draws().len(), 1);
        assert!(b.set_node_bounds(node, &[0.5, 0.5, 0.5]));
        assert!(b.set_node_transform(node, &pose(3.0, 0.0, -2.0, 1.0)));
        let world = b.world_world_transform(node);
        assert_eq!((world[0], world[1], world[2]), (3.0, 0.0, -2.0));
        b.clear_scene();
        assert!(!b.world_alive(node));
        assert!(b.runtime.app_mut().tick(1).draws().is_empty());
    }

    #[test]
    fn a_spawned_controller_is_driven_immediately_by_first_person_input() {
        let mut b = bridge();
        let cam = b.spawn_controller(&[0.0, 1.0, 5.0], 70.0, 0.1, 100.0, 0);
        assert!(b.world_alive(cam));
        // Forward (local -Z) at yaw 0 moves the camera from z=5 to z=4 immediately,
        // with no tick.
        b.control_first_person(0, &[0.0, 0.0, -1.0], 0.0, 0.0);
        let world = b.world_world_transform(cam);
        assert_eq!((world[0], world[1], world[2]), (0.0, 1.0, 4.0));
    }

    #[test]
    fn a_create_material_authored_translucent_material_renders_translucent() {
        // SPEC-11 §3.4: opacity authored through create_material must reach the
        // rendered draw alpha (base alpha 1.0 x opacity 0.5 = 0.5), RGB unchanged.
        let mut b = bridge();
        b.set_camera_3d(&[0.0, 0.0, 8.0], &[0.0, 0.0, 0.0], 60.0, 0.1, 100.0);
        let cube = b.create_mesh("cube");
        let glass = b.create_material(&[0.1, 0.2, 0.9], &[0.4, 0.0, 0.0], 0.3, 0.5);
        b.spawn_renderable(cube, glass, &pose(0.0, 0.0, 0.0, 1.0));
        let translucent = b.runtime.app_mut().tick(0).draws()[0].color();
        assert_eq!(
            translucent,
            [0.1, 0.2, 0.9, 0.5],
            "opacity 0.5 folds into the draw alpha; base RGB is preserved"
        );

        // Control: the same base colour authored fully opaque must render alpha
        // 1.0, so the translucency above is the authored opacity, not a constant.
        let mut o = bridge();
        o.set_camera_3d(&[0.0, 0.0, 8.0], &[0.0, 0.0, 0.0], 60.0, 0.1, 100.0);
        let mesh = o.create_mesh("cube");
        let solid = o.create_material(&[0.1, 0.2, 0.9], &[0.0, 0.0, 0.0], 1.0, 1.0);
        o.spawn_renderable(mesh, solid, &pose(0.0, 0.0, 0.0, 1.0));
        let opaque = o.runtime.app_mut().tick(0).draws()[0].color();
        assert_eq!(opaque, [0.1, 0.2, 0.9, 1.0], "an opaque material keeps full alpha");
        assert_ne!(translucent[3], opaque[3], "the authored opacity changed the rendered alpha");
    }

    #[test]
    fn set_node_transform_and_bounds_on_a_stale_handle_are_clean_no_ops() {
        let mut b = bridge();
        assert!(!b.set_node_transform(999, &pose(1.0, 1.0, 1.0, 1.0)));
        assert!(!b.set_node_bounds(999, &[1.0, 1.0, 1.0]));
    }
}
