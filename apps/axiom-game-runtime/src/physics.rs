//! Physics (SPEC-10) composed into the bridge: a deterministic rigid-body world
//! over [`axiom_physics::PhysicsApi`], a body table keyed by retained-world
//! entity, the per-tick step driven inside the fixed-step loop, and the
//! `#[wasm_bindgen]` boundary the TS `NativeBridge` physics methods bind.
//! ## What lives here
//! - [`PhysicsState`]: the `PhysicsApi` world plus the `entity -> body handle`
//!   table the bridge keeps. It is a private field of [`GameBridge`] (initialized
//!   in `GameBridge::new`), so all physics logic stays in this one file.
//! - An `impl GameBridge` block: the native, fully-testable physics methods the
//!   wasm shell marshals to (`physics_set_config`, `physics_add_body`,
//!   `physics_apply_*`, `physics_set_*velocity`).
//! - A `#[wasm_bindgen] impl WasmGame` block (wasm32 only): the camelCase exports
//!   the TS edge forwards verbatim.
//! ## Boundary convention (the established `(scalar/byte/string)` rule)
//! A physics vector crosses the wasm boundary as scalar `(x, y, z)` `f64` args,
//! never a structured object — exactly as entities cross as raw ids and
//! components as `(kind, bytes)`. The TS platform edge (`raf-loop.ts`)
//! destructures the `Vec3`/`Handle` of the `NativeBridge` shape into these scalar
//! calls, the physics analogue of its component codec.
//! ## Per-tick step + write-back
//! [`GameBridge::advance`] runs the fixed-step loop, then steps physics once per
//! fixed tick that ran and writes each bodied entity's resulting world transform
//! back to its [`Transform2D`] in the retained world — so an author reading
//! `Transform` through the world surface sees the simulated pose each tick. Both
//! the step loop and the write-back are branchless data transforms (iterator
//! folds / `find` / `map`), matching the engine's Branchless Law for app code.
//! ## Known facade gaps (root-cause fix belongs in `axiom-physics`, not here)
//! `PhysicsApi` exposes `apply_force`/`apply_impulse`/`apply_torque` but **no**
//! direct velocity setters and **no angular impulse**. So:
//! - `set_velocity` is implemented exactly for the unit-mass dynamic bodies this
//!   app creates: it applies the impulse `target - current_linear_velocity`
//!   (mass `1`, so `Δv == impulse`), reading the current velocity from the
//!   snapshot. Correct for these bodies; a true `set_linear_velocity` command in
//!   the physics module would make it mass-independent.
//! - `set_angular_velocity` has no faithful primitive (no angular impulse): it
//!   applies the torque `target - current_angular_velocity` as a best-effort
//!   nudge. This is a genuine physics-facade gap — see the final report.

use axiom::prelude::{Entity, Ratio, RunningApp, Transform, Vec3};
use axiom_kernel::{FrameIndex, Tick};
use axiom_physics::{PhysicsApi, PhysicsBodyHandle};
use axiom_runtime::RuntimeStep;

use crate::world::Transform2D;
use crate::GameBridge;

/// A body constructor selected by kind: it builds a body in the given world and
/// yields its handle, or `None` if the facade rejected the create.
type BodyBuilder<'a> = &'a dyn Fn(&mut PhysicsApi) -> Option<PhysicsBodyHandle>;

/// The physics state the bridge owns: the deterministic rigid-body world and the
/// `entity -> body handle` table mapping a retained-world entity to its body, so
/// the per-tick step can write each body's pose back to the right entity.
#[derive(Debug)]
pub(crate) struct PhysicsState {
    api: PhysicsApi,
    bodies: Vec<(u64, PhysicsBodyHandle)>,
    sequence: u64,
}

impl PhysicsState {
    /// A world with the deterministic default configuration and no bodies.
    pub(crate) fn new() -> Self {
        PhysicsState {
            api: PhysicsApi::new(),
            bodies: Vec::new(),
            sequence: 0,
        }
    }

    /// Set the physics world config (gravity + linear/angular damping). The
    /// facade builds config at construction, so this rebuilds the world and
    /// clears the body table — call it before attaching bodies (the one
    /// construction point, exactly as the TS `setConfig` documents). Invalid
    /// values (non-finite gravity) leave the existing world untouched.
    fn set_config(
        &mut self,
        gravity_x: f64,
        gravity_y: f64,
        gravity_z: f64,
        linear_damping: f64,
        angular_damping: f64,
    ) {
        let gravity = Vec3::new(gravity_x as f32, gravity_y as f32, gravity_z as f32);
        let linear = Ratio::new((linear_damping as f32).clamp(0.0, 1.0)).ok();
        let angular = Ratio::new((angular_damping as f32).clamp(0.0, 1.0)).ok();
        linear
            .zip(angular)
            .and_then(|(l, a)| {
                PhysicsApi::with_config(gravity, 8, 4096, 4096, 1, true, l, a).ok()
            })
            .into_iter()
            .for_each(|api| {
                self.api = api;
                self.bodies.clear();
                self.sequence = 0;
            });
    }

    /// Attach a `kind` body (`"dynamic"`/`"kinematic"`/`"static"`) to `entity`,
    /// placing it at the entity's current `Transform2D` (identity if absent), and
    /// record it in the body table. Returns the raw body handle, or `0`
    /// (the invalid sentinel) for an unknown kind or a rejected create.
    fn add_body(&mut self, app: &RunningApp, entity: u64, kind: &str) -> u64 {
        let transform = app
            .get_dynamic::<Transform2D>(Entity::from_raw(entity))
            .map(|t| Transform::from_translation(Vec3::new(t.x, t.y, 0.0)))
            .unwrap_or(Transform::IDENTITY);
        let mass = Ratio::new(1.0).expect("unit mass is finite");
        let dynamic = move |api: &mut PhysicsApi| api.create_dynamic_body(transform, mass).ok();
        let kinematic = move |api: &mut PhysicsApi| api.create_kinematic_body(transform).ok();
        let fixed = move |api: &mut PhysicsApi| api.create_static_body(transform).ok();
        let builders: [BodyBuilder; 3] = [&dynamic, &kinematic, &fixed];
        ["dynamic", "kinematic", "static"]
            .iter()
            .position(|name| *name == kind)
            .and_then(|index| builders[index](&mut self.api))
            .map(|handle| {
                self.bodies.push((entity, handle));
                handle.raw()
            })
            .unwrap_or(0)
    }

    /// Queue an instantaneous impulse on `body` (a stale handle / non-dynamic body
    /// is a clean no-op the facade rejects).
    fn apply_impulse(&mut self, body: u64, x: f64, y: f64, z: f64) {
        let _ = self
            .api
            .apply_impulse(PhysicsBodyHandle::from_raw(body), vec3(x, y, z));
    }

    /// Queue a continuous force on `body`.
    fn apply_force(&mut self, body: u64, x: f64, y: f64, z: f64) {
        let _ = self
            .api
            .apply_force(PhysicsBodyHandle::from_raw(body), vec3(x, y, z));
    }

    /// Queue a continuous torque on `body`.
    fn apply_torque(&mut self, body: u64, x: f64, y: f64, z: f64) {
        let _ = self
            .api
            .apply_torque(PhysicsBodyHandle::from_raw(body), vec3(x, y, z));
    }

    /// Set `body`'s linear velocity. Implemented as the impulse
    /// `target - current_linear_velocity`; exact for the unit-mass dynamic bodies
    /// this app creates (see the module's facade-gap note).
    fn set_velocity(&mut self, body: u64, x: f64, y: f64, z: f64) {
        let handle = PhysicsBodyHandle::from_raw(body);
        let (current, _) = self.body_velocity(handle);
        let _ = self
            .api
            .apply_impulse(handle, vec3(x, y, z).subtract(current));
    }

    /// Set `body`'s angular velocity. Best-effort torque nudge toward the target
    /// (the facade has no angular impulse; see the module's facade-gap note).
    fn set_angular_velocity(&mut self, body: u64, x: f64, y: f64, z: f64) {
        let handle = PhysicsBodyHandle::from_raw(body);
        let (_, current) = self.body_velocity(handle);
        let _ = self
            .api
            .apply_torque(handle, vec3(x, y, z).subtract(current));
    }

    /// The current `(linear, angular)` velocity of `body` from the latest
    /// snapshot — `(ZERO, ZERO)` for an unknown handle.
    fn body_velocity(&self, body: PhysicsBodyHandle) -> (Vec3, Vec3) {
        self.api
            .snapshot()
            .bodies()
            .iter()
            .find(|snapshot| snapshot.handle() == body)
            .map(|snapshot| (snapshot.linear_velocity(), snapshot.angular_velocity()))
            .unwrap_or((Vec3::ZERO, Vec3::ZERO))
    }

    /// Step the world once per fixed tick that ran this frame, writing each
    /// body's resulting world transform back to its entity's `Transform2D`. A
    /// branchless fold over the step count drives the loop; the write-back is a
    /// branchless scan of the snapshot against the body table.
    pub(crate) fn step_and_writeback(
        &mut self,
        app: &mut RunningApp,
        steps: u32,
        fixed_step_nanos: u64,
    ) {
        (0..steps).for_each(|_step| {
            let sequence = self.sequence;
            let step = RuntimeStep::new(
                FrameIndex::new(sequence),
                Tick::new(sequence),
                fixed_step_nanos,
                sequence,
            );
            let _ = self.api.step(step);
            self.sequence = sequence + 1;
            let snapshot = self.api.snapshot();
            let bodies = &self.bodies;
            snapshot.bodies().iter().for_each(|body| {
                bodies
                    .iter()
                    .find(|&&(_, handle)| handle == body.handle())
                    .map(|&(entity, _)| entity)
                    .into_iter()
                    .for_each(|entity| write_back(app, entity, body.transform()));
            });
        });
    }
}

/// A physics vector from scalar boundary args.
fn vec3(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3::new(x as f32, y as f32, z as f32)
}

/// Write a body's world `transform` back to `entity`'s `Transform2D`, preserving
/// the author's scale (physics owns only position/orientation) and projecting the
/// orientation to a 2D rotation about Z (`2 * atan2(z, w)`).
fn write_back(app: &mut RunningApp, entity: u64, transform: Transform) {
    let handle = Entity::from_raw(entity);
    let (scale_x, scale_y) = app
        .get_dynamic::<Transform2D>(handle)
        .map(|prev| (prev.scale_x, prev.scale_y))
        .unwrap_or((1.0, 1.0));
    app.set_dynamic(
        handle,
        Transform2D {
            x: transform.translation.x,
            y: transform.translation.y,
            rotation: 2.0 * transform.rotation.z.atan2(transform.rotation.w),
            scale_x,
            scale_y,
        },
    );
}

impl GameBridge {
    /// Set the physics world config (`physicsSetConfig`): gravity vector plus
    /// linear/angular damping ratios. Rebuilds the world — call before adding
    /// bodies.
    pub fn physics_set_config(
        &mut self,
        gravity_x: f64,
        gravity_y: f64,
        gravity_z: f64,
        linear_damping: f64,
        angular_damping: f64,
    ) {
        self.physics.set_config(
            gravity_x,
            gravity_y,
            gravity_z,
            linear_damping,
            angular_damping,
        );
    }

    /// Attach a `kind` body to `entity` and return its raw handle
    /// (`physicsAddBody`); `0` for an unknown kind / rejected create.
    pub fn physics_add_body(&mut self, entity: u64, kind: &str) -> u64 {
        self.physics.add_body(self.runtime.app(), entity, kind)
    }

    /// Apply an instantaneous impulse to `body` (`physicsApplyImpulse`).
    pub fn physics_apply_impulse(&mut self, body: u64, x: f64, y: f64, z: f64) {
        self.physics.apply_impulse(body, x, y, z);
    }

    /// Apply a continuous force to `body` (`physicsApplyForce`).
    pub fn physics_apply_force(&mut self, body: u64, x: f64, y: f64, z: f64) {
        self.physics.apply_force(body, x, y, z);
    }

    /// Apply a torque to `body` (`physicsApplyTorque`).
    pub fn physics_apply_torque(&mut self, body: u64, x: f64, y: f64, z: f64) {
        self.physics.apply_torque(body, x, y, z);
    }

    /// Set `body`'s linear velocity (`physicsSetVelocity`).
    pub fn physics_set_velocity(&mut self, body: u64, x: f64, y: f64, z: f64) {
        self.physics.set_velocity(body, x, y, z);
    }

    /// Set `body`'s angular velocity (`physicsSetAngularVelocity`).
    pub fn physics_set_angular_velocity(&mut self, body: u64, x: f64, y: f64, z: f64) {
        self.physics.set_angular_velocity(body, x, y, z);
    }
}

#[cfg(target_arch = "wasm32")]
mod wasm_exports {
    use wasm_bindgen::prelude::*;

    use crate::wasm::WasmGame;

    #[wasm_bindgen]
    impl WasmGame {
        /// Set the physics world config (`physicsSetConfig`). The gravity vector
        /// crosses as three scalar args per the boundary convention; the TS edge
        /// destructures the `Vec3`.
        #[wasm_bindgen(js_name = physicsSetConfig)]
        pub fn physics_set_config(
            &mut self,
            gravity_x: f64,
            gravity_y: f64,
            gravity_z: f64,
            linear_damping: f64,
            angular_damping: f64,
        ) {
            self.bridge.physics_set_config(
                gravity_x,
                gravity_y,
                gravity_z,
                linear_damping,
                angular_damping,
            );
        }

        /// Attach a `kind` body to `entity`, returning the body handle as a JS
        /// number (`physicsAddBody`).
        #[wasm_bindgen(js_name = physicsAddBody)]
        pub fn physics_add_body(&mut self, entity: f64, kind: String) -> f64 {
            self.bridge.physics_add_body(entity as u64, &kind) as f64
        }

        /// Apply an instantaneous impulse to `body` (`physicsApplyImpulse`).
        #[wasm_bindgen(js_name = physicsApplyImpulse)]
        pub fn physics_apply_impulse(&mut self, body: f64, x: f64, y: f64, z: f64) {
            self.bridge.physics_apply_impulse(body as u64, x, y, z);
        }

        /// Apply a continuous force to `body` (`physicsApplyForce`).
        #[wasm_bindgen(js_name = physicsApplyForce)]
        pub fn physics_apply_force(&mut self, body: f64, x: f64, y: f64, z: f64) {
            self.bridge.physics_apply_force(body as u64, x, y, z);
        }

        /// Apply a torque to `body` (`physicsApplyTorque`).
        #[wasm_bindgen(js_name = physicsApplyTorque)]
        pub fn physics_apply_torque(&mut self, body: f64, x: f64, y: f64, z: f64) {
            self.bridge.physics_apply_torque(body as u64, x, y, z);
        }

        /// Set `body`'s linear velocity (`physicsSetVelocity`).
        #[wasm_bindgen(js_name = physicsSetVelocity)]
        pub fn physics_set_velocity(&mut self, body: f64, x: f64, y: f64, z: f64) {
            self.bridge.physics_set_velocity(body as u64, x, y, z);
        }

        /// Set `body`'s angular velocity (`physicsSetAngularVelocity`).
        #[wasm_bindgen(js_name = physicsSetAngularVelocity)]
        pub fn physics_set_angular_velocity(&mut self, body: f64, x: f64, y: f64, z: f64) {
            self.bridge.physics_set_angular_velocity(body as u64, x, y, z);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::world::{self, Transform2D};
    use crate::{demo_app, GameBridge};
    use axiom::prelude::{BinaryReader, Reflect};

    /// 1 ms fixed step (matches the other slice tests).
    const STEP: u64 = 1_000_000;

    fn bridge() -> GameBridge {
        GameBridge::new(demo_app().build(), 0, STEP, 1)
    }

    /// A `Transform` component's bytes at `(x, y)` (defaults for the rest).
    fn transform_bytes(x: f32, y: f32) -> Vec<u8> {
        world::encode(&Transform2D {
            x,
            y,
            rotation: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
        })
    }

    /// Decode an entity's current `Transform2D` from the retained world.
    fn read_transform(b: &GameBridge, entity: u64) -> Transform2D {
        Transform2D::reflect_read(&mut BinaryReader::new(&b.world_get(entity, "Transform")))
            .expect("a bodied entity has a Transform written back")
    }

    /// Deterministic FNV-1a over a byte buffer.
    fn fnv1a(bytes: &[u8]) -> u64 {
        bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    }

    /// Script a gravity-off world with one dynamic body given an +X impulse, then
    /// return the per-tick fingerprint of the body entity's written-back transform.
    fn scripted_run() -> Vec<u64> {
        let mut b = bridge();
        let entity = b.world_spawn();
        b.world_set(entity, "Transform", &transform_bytes(0.0, 0.0));
        // Gravity off (top-down): the body should never gain Y from gravity.
        b.physics_set_config(0.0, 0.0, 0.0, 0.0, 0.0);
        let body = b.physics_add_body(entity, "dynamic");
        b.physics_apply_impulse(body, 1.0, 0.0, 0.0);
        (0..30u32)
            .map(|_tick| {
                b.advance(STEP);
                fnv1a(&b.world_get(entity, "Transform"))
            })
            .collect()
    }

    #[test]
    fn two_worlds_replay_to_a_byte_identical_transform_hash_sequence() {
        // Same scripted body creates + impulses over N ticks ⇒ byte-identical
        // per-tick transform-hash sequences across two independent bridges.
        let first = scripted_run();
        assert_eq!(first, scripted_run());
        // The body genuinely moves (the impulse drives it), so the fingerprint is
        // not constant — the ticks did real physics work.
        assert!(first.iter().any(|&hash| hash != first[0]));
    }

    #[test]
    fn an_impulse_moves_the_body_along_its_direction_and_gravity_off_leaves_y_fixed() {
        let mut b = bridge();
        let entity = b.world_spawn();
        b.world_set(entity, "Transform", &transform_bytes(0.0, 0.0));
        b.physics_set_config(0.0, 0.0, 0.0, 0.0, 0.0);
        let body = b.physics_add_body(entity, "dynamic");
        b.physics_apply_impulse(body, 1.0, 0.0, 0.0);
        (0..30u32).for_each(|_tick| {
            b.advance(STEP);
        });
        let transform = read_transform(&b, entity);
        // The +X impulse carried the body in +X...
        assert!(transform.x > 0.0);
        // ...and with gravity off, Y never changed.
        assert_eq!(transform.y, 0.0);
    }

    #[test]
    fn set_velocity_reaches_the_target_and_drives_the_body() {
        let mut b = bridge();
        let entity = b.world_spawn();
        b.world_set(entity, "Transform", &transform_bytes(0.0, 0.0));
        b.physics_set_config(0.0, 0.0, 0.0, 0.0, 0.0);
        let body = b.physics_add_body(entity, "dynamic");
        // Setting velocity from rest (mass 1) is exact: the body then coasts +X.
        b.physics_set_velocity(body, 2.0, 0.0, 0.0);
        (0..10u32).for_each(|_tick| {
            b.advance(STEP);
        });
        assert!(read_transform(&b, entity).x > 0.0);
    }

    #[test]
    fn an_unknown_body_kind_is_the_invalid_handle_and_a_no_op() {
        let mut b = bridge();
        let entity = b.world_spawn();
        // An unknown kind never creates a body (the invalid sentinel handle 0).
        assert_eq!(b.physics_add_body(entity, "ghost"), 0);
        // The other body kinds create real (non-zero) handles.
        assert_ne!(b.physics_add_body(entity, "static"), 0);
        assert_ne!(b.physics_add_body(entity, "kinematic"), 0);
    }

    #[test]
    fn the_apply_and_angular_verbs_are_deterministic_no_panic_paths() {
        // Exercise every remaining verb on a torque-capable dynamic body; the
        // point is they route through the facade without panicking and stay
        // deterministic (a second identical run agrees).
        let run = || -> Vec<u8> {
            let mut b = bridge();
            let entity = b.world_spawn();
            b.world_set(entity, "Transform", &transform_bytes(0.0, 0.0));
            b.physics_set_config(0.0, -9.8, 0.0, 0.1, 0.1);
            let body = b.physics_add_body(entity, "dynamic");
            b.physics_apply_force(body, 0.0, 1.0, 0.0);
            b.physics_apply_torque(body, 0.0, 0.0, 1.0);
            b.physics_set_angular_velocity(body, 0.0, 0.0, 1.0);
            (0..5u32).for_each(|_tick| {
                b.advance(STEP);
            });
            b.world_get(entity, "Transform")
        };
        assert_eq!(run(), run());
    }
}
