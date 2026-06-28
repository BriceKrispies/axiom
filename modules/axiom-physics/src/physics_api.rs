//! The single public facade for the physics module.

use axiom_kernel::{Meters, Ratio};
use axiom_math::{Transform, Vec3};
use axiom_runtime::RuntimeStep;

use crate::physics_body_desc::PhysicsBodyDesc;
use crate::physics_body_handle::PhysicsBodyHandle;
use crate::physics_collider_handle::PhysicsColliderHandle;
use crate::physics_collider_shape::PhysicsColliderShape;
use crate::physics_config::PhysicsConfig;
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
    /// `PhysicsConfig` is not part of the public surface).
    pub fn with_config(
        gravity: Vec3,
        solver_iterations: u32,
        max_bodies: u32,
        max_colliders: u32,
        max_substeps: u32,
        sleeping_disabled: bool,
    ) -> PhysicsResult<Self> {
        PhysicsConfig::new(
            gravity,
            solver_iterations,
            max_bodies,
            max_colliders,
            max_substeps,
            sleeping_disabled,
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

    /// Queue a continuous force on a dynamic body (applied at the next step).
    pub fn apply_force(&mut self, body: PhysicsBodyHandle, force: Vec3) -> PhysicsResult<()> {
        self.world.enqueue_apply_force(body, force)
    }

    /// Queue an instantaneous impulse on a dynamic body.
    pub fn apply_impulse(&mut self, body: PhysicsBodyHandle, impulse: Vec3) -> PhysicsResult<()> {
        self.world.enqueue_apply_impulse(body, impulse)
    }

    /// Queue enabling a body.
    pub fn enable_body(&mut self, body: PhysicsBodyHandle) -> PhysicsResult<()> {
        self.world.enqueue_enable(body)
    }

    /// Queue disabling a body.
    pub fn disable_body(&mut self, body: PhysicsBodyHandle) -> PhysicsResult<()> {
        self.world.enqueue_disable(body)
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

    #[test]
    fn default_and_new_agree_and_are_debuggable() {
        let api = PhysicsApi::default();
        assert!(api.events().is_empty());
        assert_eq!(api.latest_step_record().step_index(), 0);
        assert!(format!("{api:?}").contains("PhysicsApi"));
    }

    #[test]
    fn with_config_rejects_invalid_configuration() {
        assert!(PhysicsApi::with_config(Vec3::ZERO, 0, 1, 1, 1, true).is_err());
        assert!(PhysicsApi::with_config(Vec3::new(0.0, -9.8, 0.0), 8, 16, 16, 1, true).is_ok());
    }
}
