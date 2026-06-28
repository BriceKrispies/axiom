//! A single rigid body's mutable simulation state.

use axiom_math::{Quat, Transform, Vec3};

use crate::force_accumulator::ForceAccumulator;
use crate::mass_properties::MassProperties;
use crate::physics_body_desc::PhysicsBodyDesc;
use crate::physics_body_handle::PhysicsBodyHandle;
use crate::physics_body_kind::PhysicsBodyKind;

/// The mutable state of one rigid body, owned by the [`crate::PhysicsApi`]
/// world. A body knows nothing about scenes, renderables, meshes, assets, or
/// gameplay — only its own deterministic transform, velocities, accumulated
/// forces, and mass.
///
/// Angular state (`angular_velocity`) is integrated from accumulated torque and
/// the body's inverse inertia, and the integrator advances the transform's
/// orientation from it each step (see `integrator.rs`); both are surfaced in
/// snapshots.
#[derive(Debug, Clone)]
pub(crate) struct PhysicsBody {
    handle: PhysicsBodyHandle,
    kind: PhysicsBodyKind,
    transform: Transform,
    linear_velocity: Vec3,
    angular_velocity: Vec3,
    forces: ForceAccumulator,
    mass_properties: MassProperties,
    enabled: bool,
}

impl PhysicsBody {
    /// Create a body from a validated description, at rest and enabled.
    pub(crate) fn from_desc(handle: PhysicsBodyHandle, desc: PhysicsBodyDesc) -> Self {
        PhysicsBody {
            handle,
            kind: desc.kind(),
            transform: desc.transform(),
            linear_velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
            forces: ForceAccumulator::new(),
            mass_properties: desc.mass_properties(),
            enabled: true,
        }
    }

    pub(crate) fn handle(&self) -> PhysicsBodyHandle {
        self.handle
    }

    pub(crate) fn kind(&self) -> PhysicsBodyKind {
        self.kind
    }

    pub(crate) fn transform(&self) -> Transform {
        self.transform
    }

    pub(crate) fn set_transform(&mut self, transform: Transform) {
        self.transform = transform;
    }

    pub(crate) fn linear_velocity(&self) -> Vec3 {
        self.linear_velocity
    }

    pub(crate) fn set_linear_velocity(&mut self, velocity: Vec3) {
        self.linear_velocity = velocity;
    }

    pub(crate) fn angular_velocity(&self) -> Vec3 {
        self.angular_velocity
    }

    pub(crate) fn set_angular_velocity(&mut self, velocity: Vec3) {
        self.angular_velocity = velocity;
    }

    pub(crate) fn mass_properties(&self) -> MassProperties {
        self.mass_properties
    }

    /// Replace the body's mass properties. Used when a collider is attached to a
    /// dynamic body so the body's inverse inertia reflects the collider's shape.
    pub(crate) fn set_mass_properties(&mut self, mass_properties: MassProperties) {
        self.mass_properties = mass_properties;
    }

    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub(crate) fn forces(&self) -> &ForceAccumulator {
        &self.forces
    }

    pub(crate) fn forces_mut(&mut self) -> &mut ForceAccumulator {
        &mut self.forces
    }

    /// `true` iff every component of this body's stored simulation state
    /// (translation, scale, orientation, and linear and angular velocity) is
    /// finite. The integrator now writes translation, linear velocity, the
    /// orientation quaternion, and angular velocity, so all five are screened —
    /// guaranteeing a committed body, and therefore every snapshot, can never
    /// carry a `NaN`/`±∞` (the orientation integrate clamps its normalize divide,
    /// but checking it here keeps the atomic-rollback guarantee total).
    pub(crate) fn is_finite_state(&self) -> bool {
        let t = self.transform;
        vec3_is_finite(t.translation)
            & vec3_is_finite(t.scale)
            & quat_is_finite(t.rotation)
            & vec3_is_finite(self.linear_velocity)
            & vec3_is_finite(self.angular_velocity)
    }
}

/// `true` iff every component of `v` is finite.
fn vec3_is_finite(v: Vec3) -> bool {
    v.x.is_finite() & v.y.is_finite() & v.z.is_finite()
}

/// `true` iff every component of orientation quaternion `q` is finite.
fn quat_is_finite(q: Quat) -> bool {
    q.x.is_finite() & q.y.is_finite() & q.z.is_finite() & q.w.is_finite()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::Ratio;

    fn dynamic_body() -> PhysicsBody {
        let desc =
            PhysicsBodyDesc::dynamic_body(Transform::IDENTITY, Ratio::new(2.0).unwrap()).unwrap();
        PhysicsBody::from_desc(PhysicsBodyHandle::from_raw(1), desc)
    }

    #[test]
    fn from_desc_starts_at_rest_and_enabled() {
        let b = dynamic_body();
        assert_eq!(b.handle(), PhysicsBodyHandle::from_raw(1));
        assert_eq!(b.kind(), PhysicsBodyKind::Dynamic);
        assert_eq!(b.transform(), Transform::IDENTITY);
        assert_eq!(b.linear_velocity(), Vec3::ZERO);
        assert_eq!(b.angular_velocity(), Vec3::ZERO);
        assert_eq!(b.mass_properties().inverse_mass().get(), 0.5);
        assert!(b.enabled());
        assert_eq!(b.forces().force(), Vec3::ZERO);
    }

    #[test]
    fn mutators_update_state() {
        let mut b = dynamic_body();
        b.set_transform(Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)));
        assert_eq!(b.transform().translation, Vec3::new(1.0, 2.0, 3.0));
        b.set_linear_velocity(Vec3::new(4.0, 0.0, 0.0));
        assert_eq!(b.linear_velocity(), Vec3::new(4.0, 0.0, 0.0));
        b.set_angular_velocity(Vec3::new(0.0, 5.0, 0.0));
        assert_eq!(b.angular_velocity(), Vec3::new(0.0, 5.0, 0.0));
        b.set_enabled(false);
        assert!(!b.enabled());
        b.forces_mut().apply_torque(Vec3::ONE);
        assert_eq!(b.forces().torque(), Vec3::ONE);
    }

    #[test]
    fn set_mass_properties_updates_inverse_inertia() {
        use crate::physics_collider_shape::PhysicsColliderShape;
        use axiom_kernel::Meters;
        let mut b = dynamic_body();
        assert_eq!(b.mass_properties().inverse_inertia(), Vec3::ZERO);
        let sphere = PhysicsColliderShape::sphere(Meters::new(1.0).unwrap()).unwrap();
        let mp = b.mass_properties().with_inertia_for(sphere);
        b.set_mass_properties(mp);
        // mass 2, radius 1: I = 0.4*2 = 0.8 -> inverse 1.25 on every axis.
        let inv = b.mass_properties().inverse_inertia();
        assert!((inv.x - 1.25).abs() < 1.0e-6);
        assert!((inv.y - 1.25).abs() < 1.0e-6);
        assert!((inv.z - 1.25).abs() < 1.0e-6);
    }

    #[test]
    fn is_finite_state_detects_a_non_finite_velocity_transform_or_rotation() {
        let mut b = dynamic_body();
        assert!(b.is_finite_state());
        b.set_linear_velocity(Vec3::new(f32::INFINITY, 0.0, 0.0));
        assert!(!b.is_finite_state());
        let mut c = dynamic_body();
        c.set_transform(Transform::from_translation(Vec3::new(f32::NAN, 0.0, 0.0)));
        assert!(!c.is_finite_state());
        // A non-finite angular velocity is caught.
        let mut d = dynamic_body();
        d.set_angular_velocity(Vec3::new(0.0, f32::NAN, 0.0));
        assert!(!d.is_finite_state());
        // A non-finite orientation quaternion is caught.
        let mut e = dynamic_body();
        e.set_transform(Transform::from_rotation(Quat::new(f32::NAN, 0.0, 0.0, 1.0)));
        assert!(!e.is_finite_state());
    }

    #[test]
    fn debug_is_exercised() {
        assert!(format!("{:?}", dynamic_body()).contains("PhysicsBody"));
    }
}
