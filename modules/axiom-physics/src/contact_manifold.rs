//! A single deterministic contact between two colliders.
//!
//! A `ContactManifold` is the narrow phase's output for one colliding pair: the
//! two colliders (and their owning bodies), a unit contact **normal**, the
//! penetration **depth**, and a world contact **point**. It is a flat value the
//! contact solver reads to compute impulses.
//!
//! ## Normal orientation (deterministic, documented)
//! The normal points **from collider A toward collider B**, where `A`/`B` are the
//! pair's colliders in ascending handle order (the broad phase guarantees
//! `collider_a < collider_b`). To separate the pair, B moves along `+normal` and
//! A along `-normal`, each scaled by its inverse mass. Because the A/B roles are
//! fixed by handle order, the normal direction is a stable function of world
//! state, never of discovery order.

use axiom_math::Vec3;

use crate::physics_body_handle::PhysicsBodyHandle;
use crate::physics_collider_handle::PhysicsColliderHandle;

/// One contact between collider `a` and collider `b` (in ascending handle order).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ContactManifold {
    collider_a: PhysicsColliderHandle,
    collider_b: PhysicsColliderHandle,
    body_a: PhysicsBodyHandle,
    body_b: PhysicsBodyHandle,
    normal: Vec3,
    depth: f32,
    point: Vec3,
}

impl ContactManifold {
    /// Build a manifold. `normal` must be unit-length and oriented from A to B;
    /// `depth` is the (positive) penetration; `point` is the world contact point.
    pub(crate) fn new(
        collider_a: PhysicsColliderHandle,
        collider_b: PhysicsColliderHandle,
        body_a: PhysicsBodyHandle,
        body_b: PhysicsBodyHandle,
        normal: Vec3,
        depth: f32,
        point: Vec3,
    ) -> Self {
        ContactManifold {
            collider_a,
            collider_b,
            body_a,
            body_b,
            normal,
            depth,
            point,
        }
    }

    pub(crate) fn collider_a(&self) -> PhysicsColliderHandle {
        self.collider_a
    }

    pub(crate) fn collider_b(&self) -> PhysicsColliderHandle {
        self.collider_b
    }

    pub(crate) fn body_a(&self) -> PhysicsBodyHandle {
        self.body_a
    }

    pub(crate) fn body_b(&self) -> PhysicsBodyHandle {
        self.body_b
    }

    /// The unit contact normal, oriented from collider A toward collider B.
    pub(crate) fn normal(&self) -> Vec3 {
        self.normal
    }

    /// The penetration depth (always `> 0` for a real contact).
    pub(crate) fn depth(&self) -> f32 {
        self.depth
    }

    /// The world contact point.
    pub(crate) fn point(&self) -> Vec3 {
        self.point
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifold(depth: f32) -> ContactManifold {
        ContactManifold::new(
            PhysicsColliderHandle::from_raw(1),
            PhysicsColliderHandle::from_raw(2),
            PhysicsBodyHandle::from_raw(3),
            PhysicsBodyHandle::from_raw(4),
            Vec3::UNIT_Y,
            depth,
            Vec3::new(1.0, 0.0, 0.0),
        )
    }

    #[test]
    fn exposes_every_field() {
        let m = manifold(0.5);
        assert_eq!(m.collider_a(), PhysicsColliderHandle::from_raw(1));
        assert_eq!(m.collider_b(), PhysicsColliderHandle::from_raw(2));
        assert_eq!(m.body_a(), PhysicsBodyHandle::from_raw(3));
        assert_eq!(m.body_b(), PhysicsBodyHandle::from_raw(4));
        assert_eq!(m.normal(), Vec3::UNIT_Y);
        assert_eq!(m.depth(), 0.5);
        assert_eq!(m.point(), Vec3::new(1.0, 0.0, 0.0));
    }

    #[test]
    fn derives_are_exercised() {
        let m = manifold(0.5);
        let c = m;
        let cloned = m.clone();
        assert_eq!(m, c);
        assert_eq!(m, cloned);
        assert_ne!(m, manifold(0.9));
        assert!(format!("{m:?}").contains("ContactManifold"));
    }
}
