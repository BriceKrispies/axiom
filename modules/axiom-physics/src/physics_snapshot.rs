//! Deterministic, replay-friendly world snapshots.

use axiom_math::{Transform, Vec3};

use crate::physics_body_handle::PhysicsBodyHandle;
use crate::physics_body_kind::PhysicsBodyKind;
use crate::physics_collider_handle::PhysicsColliderHandle;
use crate::physics_collider_shape::PhysicsColliderShape;
use crate::physics_material::PhysicsMaterial;

/// A deterministic snapshot of one rigid body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodySnapshot {
    handle: PhysicsBodyHandle,
    kind: PhysicsBodyKind,
    transform: Transform,
    linear_velocity: Vec3,
    angular_velocity: Vec3,
    enabled: bool,
}

impl BodySnapshot {
    pub(crate) fn new(
        handle: PhysicsBodyHandle,
        kind: PhysicsBodyKind,
        transform: Transform,
        linear_velocity: Vec3,
        angular_velocity: Vec3,
        enabled: bool,
    ) -> Self {
        BodySnapshot {
            handle,
            kind,
            transform,
            linear_velocity,
            angular_velocity,
            enabled,
        }
    }

    pub fn handle(&self) -> PhysicsBodyHandle {
        self.handle
    }

    pub fn kind(&self) -> PhysicsBodyKind {
        self.kind
    }

    pub fn transform(&self) -> Transform {
        self.transform
    }

    pub fn linear_velocity(&self) -> Vec3 {
        self.linear_velocity
    }

    pub fn angular_velocity(&self) -> Vec3 {
        self.angular_velocity
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }
}

/// A deterministic snapshot of one collider.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColliderSnapshot {
    handle: PhysicsColliderHandle,
    body: PhysicsBodyHandle,
    shape: PhysicsColliderShape,
    material: PhysicsMaterial,
    is_trigger: bool,
    enabled: bool,
}

impl ColliderSnapshot {
    pub(crate) fn new(
        handle: PhysicsColliderHandle,
        body: PhysicsBodyHandle,
        shape: PhysicsColliderShape,
        material: PhysicsMaterial,
        is_trigger: bool,
        enabled: bool,
    ) -> Self {
        ColliderSnapshot {
            handle,
            body,
            shape,
            material,
            is_trigger,
            enabled,
        }
    }

    pub fn handle(&self) -> PhysicsColliderHandle {
        self.handle
    }

    pub fn body(&self) -> PhysicsBodyHandle {
        self.body
    }

    pub fn shape(&self) -> PhysicsColliderShape {
        self.shape
    }

    pub fn material(&self) -> PhysicsMaterial {
        self.material
    }

    pub fn is_trigger(&self) -> bool {
        self.is_trigger
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }
}

/// A deterministic, replay-friendly snapshot of the whole world at a step.
///
/// Bodies and colliders are listed in **insertion order** (the order they were
/// created / attached), which is stable across runs. Two snapshots taken from
/// worlds that received identical inputs compare equal.
#[derive(Debug, Clone, PartialEq)]
pub struct PhysicsSnapshot {
    step_index: u64,
    bodies: Vec<BodySnapshot>,
    colliders: Vec<ColliderSnapshot>,
}

impl PhysicsSnapshot {
    pub(crate) fn new(
        step_index: u64,
        bodies: Vec<BodySnapshot>,
        colliders: Vec<ColliderSnapshot>,
    ) -> Self {
        PhysicsSnapshot {
            step_index,
            bodies,
            colliders,
        }
    }

    /// The world's step index at the time of the snapshot.
    pub fn step_index(&self) -> u64 {
        self.step_index
    }

    /// The body snapshots, in insertion order.
    pub fn bodies(&self) -> &[BodySnapshot] {
        &self.bodies
    }

    /// The collider snapshots, in insertion order.
    pub fn colliders(&self) -> &[ColliderSnapshot] {
        &self.colliders
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::{Meters, Ratio};

    fn body_snapshot() -> BodySnapshot {
        BodySnapshot::new(
            PhysicsBodyHandle::from_raw(1),
            PhysicsBodyKind::Dynamic,
            Transform::IDENTITY,
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::ZERO,
            true,
        )
    }

    fn collider_snapshot() -> ColliderSnapshot {
        ColliderSnapshot::new(
            PhysicsColliderHandle::from_raw(2),
            PhysicsBodyHandle::from_raw(1),
            PhysicsColliderShape::sphere(Meters::new(1.0).unwrap()).unwrap(),
            PhysicsMaterial::new(
                Ratio::new(0.5).unwrap(),
                Ratio::new(0.5).unwrap(),
                Ratio::new(1.0).unwrap(),
            )
            .unwrap(),
            false,
            true,
        )
    }

    #[test]
    fn body_snapshot_exposes_all_fields() {
        let b = body_snapshot();
        assert_eq!(b.handle(), PhysicsBodyHandle::from_raw(1));
        assert_eq!(b.kind(), PhysicsBodyKind::Dynamic);
        assert_eq!(b.transform(), Transform::IDENTITY);
        assert_eq!(b.linear_velocity(), Vec3::new(0.0, -1.0, 0.0));
        assert_eq!(b.angular_velocity(), Vec3::ZERO);
        assert!(b.enabled());
    }

    #[test]
    fn collider_snapshot_exposes_all_fields() {
        let c = collider_snapshot();
        assert_eq!(c.handle(), PhysicsColliderHandle::from_raw(2));
        assert_eq!(c.body(), PhysicsBodyHandle::from_raw(1));
        assert_eq!(c.material().friction().get(), 0.5);
        assert!(!c.is_trigger());
        assert!(c.enabled());
        assert!(format!("{:?}", c.shape()).contains("Sphere"));
    }

    #[test]
    fn snapshot_exposes_ordered_contents() {
        let snap = PhysicsSnapshot::new(3, vec![body_snapshot()], vec![collider_snapshot()]);
        assert_eq!(snap.step_index(), 3);
        assert_eq!(snap.bodies().len(), 1);
        assert_eq!(snap.colliders().len(), 1);
    }

    #[test]
    fn derives_are_exercised() {
        let snap = PhysicsSnapshot::new(1, vec![body_snapshot()], vec![collider_snapshot()]);
        let cloned = snap.clone();
        assert_eq!(snap, cloned);
        assert_ne!(snap, PhysicsSnapshot::new(2, Vec::new(), Vec::new()));
        let b = body_snapshot();
        let bc = b;
        assert_eq!(b, bc);
        assert!(format!("{snap:?}").contains("PhysicsSnapshot"));
        assert!(format!("{b:?}").contains("BodySnapshot"));
        assert!(format!("{:?}", collider_snapshot()).contains("ColliderSnapshot"));
    }
}
