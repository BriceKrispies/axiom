//! The single public facade for the physics module.

use axiom_kernel::{Meters, Ratio};
use axiom_math::{Transform, Vec3};
use axiom_runtime::RuntimeStep;

use crate::physics_body_desc::PhysicsBodyDesc;
use crate::physics_body_handle::PhysicsBodyHandle;
use crate::physics_collider_handle::PhysicsColliderHandle;
use crate::physics_collider_shape::PhysicsColliderShape;
use crate::physics_config::PhysicsConfig;
use crate::physics_error::PhysicsError;
use crate::physics_heightfield::Heightfield;
use crate::contact_report::ContactReport;
use crate::physics_event::PhysicsEvent;
use crate::physics_material::PhysicsMaterial;
use crate::physics_query::PhysicsQuery;
use crate::physics_result::PhysicsResult;
use crate::physics_snapshot::PhysicsSnapshot;
use crate::physics_step_record::PhysicsStepRecord;
use crate::physics_world::PhysicsWorld;

/// The deterministic rigid-body physics facade — the only public type in the
/// module. Everything (bodies, colliders, commands, stepping, snapshots,
/// queries) flows through this handle; the internal world state is never exposed
/// directly. All scalar quantities cross the boundary as kernel/math value types
/// (`Ratio`, `Meters`, `Vec3`, `Transform`) — never naked floats.
#[derive(Debug)]
pub struct PhysicsApi {
    world: PhysicsWorld,
}

impl PhysicsApi {
    /// Create a world with the deterministic default configuration.
    pub fn new() -> Self {
        PhysicsApi {
            world: PhysicsWorld::new(PhysicsConfig::default_config()),
        }
    }

    /// Create a world from an explicit configuration, rejecting invalid values.
    /// The configuration is passed as primitive value types (the internal
    /// `PhysicsConfig` is not part of the public surface). `linear_damping` and
    /// `angular_damping` are per-step velocity-decay fractions in `[0, 1]`
    /// (`Ratio`): `0` means no decay (a free body coasts forever, the engine's
    /// prior behaviour) and `1` brings the corresponding velocity to rest in a
    /// single step.
    #[allow(clippy::too_many_arguments)]
    pub fn with_config(
        gravity: Vec3,
        solver_iterations: u32,
        max_bodies: u32,
        max_colliders: u32,
        max_substeps: u32,
        sleeping_disabled: bool,
        linear_damping: Ratio,
        angular_damping: Ratio,
    ) -> PhysicsResult<Self> {
        PhysicsConfig::new(
            gravity,
            solver_iterations,
            max_bodies,
            max_colliders,
            max_substeps,
            sleeping_disabled,
            linear_damping,
            angular_damping,
        )
        .map(|config| PhysicsApi {
            world: PhysicsWorld::new(config),
        })
    }

    /// Create a static body at `transform`.
    pub fn create_static_body(&mut self, transform: Transform) -> PhysicsResult<PhysicsBodyHandle> {
        PhysicsBodyDesc::static_body(transform).and_then(|desc| self.world.create_body(desc))
    }

    /// Create a dynamic body at `transform` with the given mass (finite, `> 0`).
    pub fn create_dynamic_body(
        &mut self,
        transform: Transform,
        mass: Ratio,
    ) -> PhysicsResult<PhysicsBodyHandle> {
        PhysicsBodyDesc::dynamic_body(transform, mass).and_then(|desc| self.world.create_body(desc))
    }

    /// Create a kinematic body at `transform`.
    pub fn create_kinematic_body(
        &mut self,
        transform: Transform,
    ) -> PhysicsResult<PhysicsBodyHandle> {
        PhysicsBodyDesc::kinematic_body(transform).and_then(|desc| self.world.create_body(desc))
    }

    /// Build a validated surface material (`friction >= 0`, `restitution` in
    /// `[0, 1]`, `density > 0`). The returned material is an opaque value the
    /// caller hands back to an `attach_*_collider` call — keeping the collider
    /// methods' argument lists small and the material validated up front.
    pub fn material(
        friction: Ratio,
        restitution: Ratio,
        density: Ratio,
    ) -> PhysicsResult<PhysicsMaterial> {
        PhysicsMaterial::new(friction, restitution, density)
    }

    /// Attach a sphere collider to `body`.
    pub fn attach_sphere_collider(
        &mut self,
        body: PhysicsBodyHandle,
        radius: Meters,
        material: PhysicsMaterial,
        is_trigger: bool,
    ) -> PhysicsResult<PhysicsColliderHandle> {
        PhysicsColliderShape::sphere(radius)
            .and_then(|shape| self.world.attach_collider(body, shape, material, is_trigger))
    }

    /// Attach an axis-aligned box collider to `body`.
    pub fn attach_box_collider(
        &mut self,
        body: PhysicsBodyHandle,
        half_extents: Vec3,
        material: PhysicsMaterial,
        is_trigger: bool,
    ) -> PhysicsResult<PhysicsColliderHandle> {
        PhysicsColliderShape::box_shape(half_extents)
            .and_then(|shape| self.world.attach_collider(body, shape, material, is_trigger))
    }

    /// Attach a capsule collider to `body`.
    pub fn attach_capsule_collider(
        &mut self,
        body: PhysicsBodyHandle,
        radius: Meters,
        half_height: Meters,
        material: PhysicsMaterial,
        is_trigger: bool,
    ) -> PhysicsResult<PhysicsColliderHandle> {
        PhysicsColliderShape::capsule(radius, half_height)
            .and_then(|shape| self.world.attach_collider(body, shape, material, is_trigger))
    }

    /// Attach a plane collider to `body`.
    pub fn attach_plane_collider(
        &mut self,
        body: PhysicsBodyHandle,
        normal: Vec3,
        distance: Meters,
        material: PhysicsMaterial,
        is_trigger: bool,
    ) -> PhysicsResult<PhysicsColliderHandle> {
        PhysicsColliderShape::plane(normal, distance)
            .and_then(|shape| self.world.attach_collider(body, shape, material, is_trigger))
    }

    /// Attach a **static heightfield** collider to `body`: an `nx × nz` grid of
    /// surface heights (row-major, `heights[iz*nx + ix]`), spaced `spacing_x` /
    /// `spacing_z` metres and centred on the body origin in the local XZ plane. A
    /// sphere collides it by the deterministic vertical-projection contact — ideal
    /// for a shallow curved track surface. Rejects `nx`/`nz < 2`, non-positive
    /// spacing, or a `heights` length that is not `nx·nz`.
    pub fn attach_heightfield_collider(
        &mut self,
        body: PhysicsBodyHandle,
        nx: u32,
        nz: u32,
        spacing_x: Meters,
        spacing_z: Meters,
        heights: &[Meters],
        material: PhysicsMaterial,
        is_trigger: bool,
    ) -> PhysicsResult<PhysicsColliderHandle> {
        let (sx, sz) = (spacing_x.get(), spacing_z.get());
        let valid = (nx >= 2) & (nz >= 2) & (sx > 0.0) & (sz > 0.0) & (heights.len() as u64 == u64::from(nx) * u64::from(nz));
        [
            Err(PhysicsError::invalid_collider_shape(
                "heightfield needs nx,nz >= 2, positive spacing, and heights.len() == nx*nz",
            )),
            Ok(()),
        ][valid as usize]
        .and_then(|()| {
            let grid = Heightfield::new(nx, nz, sx, sz, heights.iter().map(|m| m.get()).collect());
            PhysicsColliderShape::heightfield_shape(grid.half_extents())
                .and_then(|shape| self.world.attach_heightfield_collider(body, shape, material, is_trigger, grid))
        })
    }

    /// Queue a continuous force on a dynamic body (applied at the next step).
    pub fn apply_force(&mut self, body: PhysicsBodyHandle, force: Vec3) -> PhysicsResult<()> {
        self.world.enqueue_apply_force(body, force)
    }

    /// Queue an instantaneous impulse on a dynamic body.
    pub fn apply_impulse(&mut self, body: PhysicsBodyHandle, impulse: Vec3) -> PhysicsResult<()> {
        self.world.enqueue_apply_impulse(body, impulse)
    }

    /// Queue a continuous torque on a dynamic body (the angular analogue of
    /// [`PhysicsApi::apply_force`]; applied over the next step and drained FIFO).
    /// Rejected on a non-dynamic or disabled body, exactly like a force. The
    /// resulting angular acceleration is `inverse_inertia ⊙ torque`, where the
    /// body's inverse inertia is derived from its collider's shape and mass.
    pub fn apply_torque(&mut self, body: PhysicsBodyHandle, torque: Vec3) -> PhysicsResult<()> {
        self.world.enqueue_apply_torque(body, torque)
    }

    /// Queue enabling a body.
    pub fn enable_body(&mut self, body: PhysicsBodyHandle) -> PhysicsResult<()> {
        self.world.enqueue_enable(body)
    }

    /// Queue disabling a body.
    pub fn disable_body(&mut self, body: PhysicsBodyHandle) -> PhysicsResult<()> {
        self.world.enqueue_disable(body)
    }

    /// Immediately teleport a body to `transform` (finite, existing body), outside
    /// the normal integration — e.g. to spawn or respawn a player at a fixed
    /// point. Unlike the queued force/impulse commands this applies at once and is
    /// visible in the next [`PhysicsApi::snapshot`].
    pub fn set_body_transform(
        &mut self,
        body: PhysicsBodyHandle,
        transform: Transform,
    ) -> PhysicsResult<()> {
        self.world.set_body_transform(body, transform)
    }

    /// Immediately set a body's linear and angular velocity (finite, existing
    /// body) — to bring it to rest on respawn or to brake it. Velocity on a
    /// non-dynamic body is stored but never integrated.
    pub fn set_body_velocity(
        &mut self,
        body: PhysicsBodyHandle,
        linear: Vec3,
        angular: Vec3,
    ) -> PhysicsResult<()> {
        self.world.set_body_velocity(body, linear, angular)
    }

    /// Advance the world by one explicit, deterministic fixed step (taken from
    /// the runtime layer). Drains queued commands FIFO, then integrates.
    pub fn step(&mut self, step: RuntimeStep) -> PhysicsResult<()> {
        self.world.step(step)
    }

    /// A deterministic snapshot of the world's current state.
    pub fn snapshot(&self) -> PhysicsSnapshot {
        self.world.snapshot()
    }

    /// The record of the most recent step (empty before the first step).
    pub fn latest_step_record(&self) -> PhysicsStepRecord {
        self.world.latest_record()
    }

    /// The contacts resolved during the most recent step, as neutral read-only
    /// reports (body/collider handles, contact normal, penetration depth, and
    /// world contact point), in deterministic order. Empty before any contact.
    /// An app translates these into debug-draw or gameplay state; physics never
    /// does.
    pub fn latest_contacts(&self) -> Vec<ContactReport> {
        self.world.latest_contacts()
    }

    /// The deterministic event log, in emission order. This is a read-only view
    /// of events not yet drained; use [`PhysicsApi::drain_events`] to consume
    /// them and keep the log bounded.
    pub fn events(&self) -> &[PhysicsEvent] {
        self.world.events()
    }

    /// Drain and return the deterministic event log, in emission order, clearing
    /// the internal queue. An app should drain once per step (or periodically)
    /// after consuming events; otherwise the log — which gains a `StepCompleted`
    /// every step — grows without bound. Draining an empty log returns an empty
    /// `Vec` deterministically.
    pub fn drain_events(&mut self) -> Vec<PhysicsEvent> {
        self.world.drain_events()
    }

    /// Cast a ray and return the nearest solid body hit within `max_distance`
    /// (ties broken by the smaller body handle; triggers excluded). Returns
    /// `None` for a miss, a zero-length/non-finite ray, or a non-finite origin.
    pub fn raycast(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: Meters,
    ) -> Option<PhysicsBodyHandle> {
        PhysicsQuery::new(&self.world).raycast(origin, direction, max_distance)
    }

    /// Find the bodies overlapping a query sphere, as a sorted, de-duplicated
    /// handle list (triggers included). Returns an empty list for a non-finite
    /// query centre.
    pub fn overlap_sphere(&self, center: Vec3, radius: Meters) -> Vec<PhysicsBodyHandle> {
        PhysicsQuery::new(&self.world).overlap_sphere(center, radius)
    }
}

impl Default for PhysicsApi {
    fn default() -> Self {
        PhysicsApi::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::{FrameIndex, Tick};

    fn zero() -> Ratio {
        Ratio::new(0.0).unwrap()
    }

    fn step() -> RuntimeStep {
        RuntimeStep::new(FrameIndex::new(0), Tick::new(0), 100_000_000, 0)
    }

    #[test]
    fn default_and_new_agree_and_are_debuggable() {
        let api = PhysicsApi::default();
        assert!(api.events().is_empty());
        assert_eq!(api.latest_step_record().step_index(), 0);
        assert!(format!("{api:?}").contains("PhysicsApi"));
    }

    #[test]
    fn with_config_rejects_invalid_configuration() {
        assert!(PhysicsApi::with_config(Vec3::ZERO, 0, 1, 1, 1, true, zero(), zero()).is_err());
        assert!(
            PhysicsApi::with_config(Vec3::new(0.0, -9.8, 0.0), 8, 16, 16, 1, true, zero(), zero())
                .is_ok()
        );
        let bad = Ratio::new(2.0).unwrap();
        assert!(PhysicsApi::with_config(Vec3::ZERO, 8, 16, 16, 1, true, bad, zero()).is_err());
    }

    #[test]
    fn apply_torque_is_accepted_on_a_dynamic_body_and_rejected_otherwise() {
        let mut api = PhysicsApi::new();
        let dynamic = api
            .create_dynamic_body(Transform::IDENTITY, Ratio::new(1.0).unwrap())
            .unwrap();
        assert!(api.apply_torque(dynamic, Vec3::new(0.0, 1.0, 0.0)).is_ok());
        let ground = api.create_static_body(Transform::IDENTITY).unwrap();
        assert!(api.apply_torque(ground, Vec3::new(0.0, 1.0, 0.0)).is_err());
        api.step(step()).unwrap();
        assert_eq!(api.latest_step_record().command_count(), 1);
    }

    #[test]
    fn set_body_transform_and_velocity_apply_immediately_and_validate() {
        let mut api = PhysicsApi::new();
        let body = api
            .create_dynamic_body(Transform::IDENTITY, Ratio::new(1.0).unwrap())
            .unwrap();

        // Teleport applies at once and is visible in the snapshot without a step.
        let target = Transform::from_translation(Vec3::new(3.0, 4.0, 5.0));
        assert!(api.set_body_transform(body, target).is_ok());
        let snap = api.snapshot();
        let placed = snap.bodies().iter().find(|b| b.handle() == body).unwrap();
        assert_eq!(placed.transform().translation, Vec3::new(3.0, 4.0, 5.0));

        // Velocity (linear + angular) is set immediately and read back.
        assert!(api
            .set_body_velocity(body, Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 2.0, 0.0))
            .is_ok());
        let snap = api.snapshot();
        let moved = snap.bodies().iter().find(|b| b.handle() == body).unwrap();
        assert_eq!(moved.linear_velocity(), Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(moved.angular_velocity(), Vec3::new(0.0, 2.0, 0.0));

        // Rejections: non-finite transform, non-finite velocity, and unknown body.
        let nan = Transform::from_translation(Vec3::new(f32::NAN, 0.0, 0.0));
        assert!(api.set_body_transform(body, nan).is_err());
        assert!(api
            .set_body_velocity(body, Vec3::new(f32::INFINITY, 0.0, 0.0), Vec3::ZERO)
            .is_err());
        let missing = PhysicsBodyHandle::from_raw(999);
        assert!(api.set_body_transform(missing, target).is_err());
        assert!(api.set_body_velocity(missing, Vec3::ZERO, Vec3::ZERO).is_err());
    }
}
