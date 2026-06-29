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
    Angle, Camera, Color, DirectionalLight, Material, Mesh, Meters, PerspectiveProjection, Ratio,
    Transform, Vec3,
};

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

/// A lit material colour from a 3-element linear `[r, g, b]` slice.
fn color(s: &[f64]) -> Color {
    Color::linear_rgb(
        channel(*s.first().unwrap_or(&0.0)),
        channel(*s.get(1).unwrap_or(&0.0)),
        channel(*s.get(2).unwrap_or(&0.0)),
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

    /// Register a lit material of linear colour `[r, g, b]` and return its handle
    /// id (`createMaterial`).
    pub fn create_material(&mut self, rgb: &[f64]) -> u64 {
        self.runtime.app_mut().add_material(Material::lit(color(rgb))).id()
    }

    /// Set (replacing any existing) the active perspective camera at `position`
    /// (`setCamera3D`): vertical FOV in degrees, near/far clip in metres.
    pub fn set_camera_3d(&mut self, position: &[f64], fov_deg: f64, near: f64, far: f64) {
        let projection = PerspectiveProjection {
            fov_y: Angle::degrees(fov_deg as f32),
            near: meters(near),
            far: meters(far),
        };
        self.runtime.app_mut().set_camera(
            Camera::perspective(projection),
            Transform::from_translation(v3(position)),
        );
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

        /// Register a lit material of linear colour `[r, g, b]` (`createMaterial`).
        #[wasm_bindgen(js_name = createMaterial)]
        pub fn create_material(&mut self, rgb: &[f64]) -> f64 {
            self.bridge.create_material(rgb) as f64
        }

        /// Set the active perspective camera (`setCamera3D`).
        #[wasm_bindgen(js_name = setCamera3D)]
        pub fn set_camera_3d(&mut self, position: &[f64], fov_deg: f64, near: f64, far: f64) {
            self.bridge.set_camera_3d(position, fov_deg, near, far);
        }

        /// Spawn a directional light, returning its node id (`addLight`).
        #[wasm_bindgen(js_name = addLight)]
        pub fn add_light(&mut self, direction: &[f64], rgb: &[f64], intensity: f64) -> f64 {
            self.bridge.add_light(direction, rgb, intensity) as f64
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
        let red = b.create_material(&[0.9, 0.1, 0.1]);
        let blue = b.create_material(&[0.1, 0.1, 0.9]);
        b.set_camera_3d(&[0.0, 0.0, 8.0], 60.0, 0.1, 100.0);
        let light = b.add_light(&[0.3, -1.0, 0.4], &[1.0, 1.0, 1.0], 1.0);
        vec![cube, sphere, ghost, red, blue, light]
    }

    #[test]
    fn authoring_mints_stable_distinct_handles_and_replays() {
        let ids = authoring_ids();
        // Mesh handles are 1-based and distinct; the unknown kind still mints a
        // fresh (third) handle rather than colliding.
        assert_eq!(ids[0], 1);
        assert_eq!(ids[1], 2);
        assert_eq!(ids[2], 3);
        // Materials are their own 1-based store.
        assert_eq!(ids[3], 1);
        assert_eq!(ids[4], 2);
        // The light node is a real, non-zero entity, and the whole script replays
        // byte-identically on a second bridge.
        assert_ne!(ids[5], 0);
        assert_eq!(ids, authoring_ids());
    }

    #[test]
    fn an_added_light_is_a_live_addressable_node() {
        let mut b = bridge();
        let light = b.add_light(&[0.0, -1.0, 0.0], &[1.0, 1.0, 1.0], 1.0);
        // The returned id names a live scene node (the bridge's world-liveness read).
        assert!(b.world_alive(light));
    }
}
