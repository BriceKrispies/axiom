//! A single rigid body's mutable simulation state.

use axiom_math::{Transform, Vec3};

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
/// Angular state (`angular_velocity`) is stored and surfaced in snapshots, but
/// the integrator never changes it and never rotates the body — rotational
/// dynamics are a documented deferral (see `ARCHITECTURE.md` / `ROADMAP.md`).
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

    pub(crate) fn mass_properties(&self) -> MassProperties {
        self.mass_properties
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
    /// (translation, scale, linear and angular velocity) is finite. The
    /// integrator only ever writes translation and linear velocity, but checking
    /// all four guarantees a committed body — and therefore every snapshot — can
    /// never carry a `NaN`/`±∞`. Orientation is never mutated, so it stays the
    /// validated finite value it was created with.
    pub(crate) fn is_finite_state(&self) -> bool {
        let t = self.transform;
        vec3_is_finite(t.translation)
            & vec3_is_finite(t.scale)
            & vec3_is_finite(self.linear_velocity)
            & vec3_is_finite(self.angular_velocity)
    }
}

/// `true` iff every component of `v` is finite.
fn vec3_is_finite(v: Vec3) -> bool {
    v.x.is_finite() & v.y.is_finite() & v.z.is_finite()
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
        b.set_enabled(false);
        assert!(!b.enabled());
        b.forces_mut().apply_force(Vec3::ONE);
        assert_eq!(b.forces().force(), Vec3::ONE);
    }

    #[test]
    fn is_finite_state_detects_a_non_finite_velocity_or_transform() {
        let mut b = dynamic_body();
        assert!(b.is_finite_state());
        b.set_linear_velocity(Vec3::new(f32::INFINITY, 0.0, 0.0));
        assert!(!b.is_finite_state());
        let mut c = dynamic_body();
        c.set_transform(Transform::from_translation(Vec3::new(f32::NAN, 0.0, 0.0)));
        assert!(!c.is_finite_state());
    }

    #[test]
    fn debug_is_exercised() {
        assert!(format!("{:?}", dynamic_body()).contains("PhysicsBody"));
    }
}
