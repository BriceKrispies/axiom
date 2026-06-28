//! A neutral, read-only report of one contact from the most recent step.
//!
//! `ContactReport` is the app-facing projection of an internal contact manifold:
//! it names the two bodies and colliders in contact (by their public handle
//! vocabulary), and carries the contact's unit `normal`, penetration `depth`, and
//! world `point`. It is a sealed value type — built by the world, returned
//! by-value from [`crate::PhysicsApi::latest_contacts`], read through accessors,
//! and never constructed by a caller. It exposes *physics* data only: an app
//! translates it into scene/render/debug-draw state; physics never does.

use axiom_kernel::Meters;
use axiom_math::Vec3;

use crate::physics_body_handle::PhysicsBodyHandle;
use crate::physics_collider_handle::PhysicsColliderHandle;

/// One resolved contact from the most recent step.
///
/// The `normal` is the unit contact normal oriented from collider A toward
/// collider B (A/B are the pair in ascending handle order — a deterministic
/// function of world state). `depth` is the positive penetration; `point` is the
/// world contact point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContactReport {
    body_a: PhysicsBodyHandle,
    body_b: PhysicsBodyHandle,
    collider_a: PhysicsColliderHandle,
    collider_b: PhysicsColliderHandle,
    normal: Vec3,
    depth: Meters,
    point: Vec3,
}

impl ContactReport {
    pub(crate) fn new(
        body_a: PhysicsBodyHandle,
        body_b: PhysicsBodyHandle,
        collider_a: PhysicsColliderHandle,
        collider_b: PhysicsColliderHandle,
        normal: Vec3,
        depth: Meters,
        point: Vec3,
    ) -> Self {
        ContactReport {
            body_a,
            body_b,
            collider_a,
            collider_b,
            normal,
            depth,
            point,
        }
    }

    /// The first body in contact (the body owning collider A).
    pub fn body_a(&self) -> PhysicsBodyHandle {
        self.body_a
    }

    /// The second body in contact (the body owning collider B).
    pub fn body_b(&self) -> PhysicsBodyHandle {
        self.body_b
    }

    /// The first collider in contact (the lower handle of the pair).
    pub fn collider_a(&self) -> PhysicsColliderHandle {
        self.collider_a
    }

    /// The second collider in contact (the higher handle of the pair).
    pub fn collider_b(&self) -> PhysicsColliderHandle {
        self.collider_b
    }

    /// The unit contact normal, oriented from collider A toward collider B.
    pub fn normal(&self) -> Vec3 {
        self.normal
    }

    /// The penetration depth (positive).
    pub fn depth(&self) -> Meters {
        self.depth
    }

    /// The world contact point.
    pub fn point(&self) -> Vec3 {
        self.point
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> ContactReport {
        ContactReport::new(
            PhysicsBodyHandle::from_raw(1),
            PhysicsBodyHandle::from_raw(2),
            PhysicsColliderHandle::from_raw(3),
            PhysicsColliderHandle::from_raw(4),
            Vec3::new(0.0, -1.0, 0.0),
            Meters::new(0.25).unwrap(),
            Vec3::new(1.0, 2.0, 3.0),
        )
    }

    #[test]
    fn exposes_every_field() {
        let r = report();
        assert_eq!(r.body_a(), PhysicsBodyHandle::from_raw(1));
        assert_eq!(r.body_b(), PhysicsBodyHandle::from_raw(2));
        assert_eq!(r.collider_a(), PhysicsColliderHandle::from_raw(3));
        assert_eq!(r.collider_b(), PhysicsColliderHandle::from_raw(4));
        assert_eq!(r.normal(), Vec3::new(0.0, -1.0, 0.0));
        assert_eq!(r.depth().get(), 0.25);
        assert_eq!(r.point(), Vec3::new(1.0, 2.0, 3.0));
    }

    #[test]
    fn derives_are_exercised() {
        let r = report();
        let c = r;
        assert_eq!(r, c);
        assert_ne!(
            r,
            ContactReport::new(
                PhysicsBodyHandle::from_raw(9),
                PhysicsBodyHandle::from_raw(2),
                PhysicsColliderHandle::from_raw(3),
                PhysicsColliderHandle::from_raw(4),
                Vec3::new(0.0, -1.0, 0.0),
                Meters::new(0.25).unwrap(),
                Vec3::new(1.0, 2.0, 3.0),
            )
        );
        assert!(format!("{r:?}").contains("ContactReport"));
    }
}
