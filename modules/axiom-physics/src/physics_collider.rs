//! A collider attached to a rigid body.

use crate::physics_body_handle::PhysicsBodyHandle;
use crate::physics_collider_handle::PhysicsColliderHandle;
use crate::physics_collider_shape::PhysicsColliderShape;
use crate::physics_heightfield::Heightfield;
use crate::physics_material::PhysicsMaterial;

/// A collider: a shape + material attached to an owning body.
///
/// Colliders participate in the live collision pipeline — the broad phase, narrow
/// phase, and contact solver — and are surfaced in snapshots and queries.
/// `is_trigger` marks a collider as a (future) overlap-only sensor; it is stored
/// and reported but carries no event behavior yet (a documented deferral — see
/// `ROADMAP.md`). A [`PhysicsShapeKind::Heightfield`] collider additionally owns
/// its grid data (the fixed shape POD only carries the bounding half-extents).
#[derive(Debug)]
pub(crate) struct PhysicsCollider {
    handle: PhysicsColliderHandle,
    body: PhysicsBodyHandle,
    shape: PhysicsColliderShape,
    material: PhysicsMaterial,
    is_trigger: bool,
    enabled: bool,
    heightfield: Option<Heightfield>,
}

impl PhysicsCollider {
    /// Attach a collider to `body`. Colliders are created enabled.
    pub(crate) fn new(
        handle: PhysicsColliderHandle,
        body: PhysicsBodyHandle,
        shape: PhysicsColliderShape,
        material: PhysicsMaterial,
        is_trigger: bool,
    ) -> Self {
        PhysicsCollider {
            handle,
            body,
            shape,
            material,
            is_trigger,
            enabled: true,
            heightfield: None,
        }
    }

    /// Attach a heightfield collider carrying its grid data.
    pub(crate) fn new_heightfield(
        handle: PhysicsColliderHandle,
        body: PhysicsBodyHandle,
        shape: PhysicsColliderShape,
        material: PhysicsMaterial,
        is_trigger: bool,
        heightfield: Heightfield,
    ) -> Self {
        PhysicsCollider {
            handle,
            body,
            shape,
            material,
            is_trigger,
            enabled: true,
            heightfield: Some(heightfield),
        }
    }

    /// The heightfield grid this collider carries, if it is a heightfield.
    pub(crate) fn heightfield(&self) -> Option<&Heightfield> {
        self.heightfield.as_ref()
    }

    pub(crate) fn handle(&self) -> PhysicsColliderHandle {
        self.handle
    }

    pub(crate) fn body(&self) -> PhysicsBodyHandle {
        self.body
    }

    pub(crate) fn shape(&self) -> PhysicsColliderShape {
        self.shape
    }

    pub(crate) fn material(&self) -> PhysicsMaterial {
        self.material
    }

    pub(crate) fn is_trigger(&self) -> bool {
        self.is_trigger
    }

    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::{Meters, Ratio};

    fn collider() -> PhysicsCollider {
        let shape = PhysicsColliderShape::sphere(Meters::new(1.0).unwrap()).unwrap();
        let material = PhysicsMaterial::new(
            Ratio::new(0.5).unwrap(),
            Ratio::new(0.5).unwrap(),
            Ratio::new(1.0).unwrap(),
        )
        .unwrap();
        PhysicsCollider::new(
            PhysicsColliderHandle::from_raw(1),
            PhysicsBodyHandle::from_raw(2),
            shape,
            material,
            true,
        )
    }

    #[test]
    fn new_exposes_all_parts() {
        let c = collider();
        assert_eq!(c.handle(), PhysicsColliderHandle::from_raw(1));
        assert_eq!(c.body(), PhysicsBodyHandle::from_raw(2));
        assert_eq!(
            c.shape(),
            PhysicsColliderShape::sphere(Meters::new(1.0).unwrap()).unwrap()
        );
        assert_eq!(c.material().friction().get(), 0.5);
        assert!(c.is_trigger());
        assert!(c.enabled());
    }

    #[test]
    fn debug_is_exercised() {
        assert!(format!("{:?}", collider()).contains("PhysicsCollider"));
    }

    #[test]
    fn a_plain_collider_has_no_heightfield_but_a_heightfield_one_does() {
        assert!(collider().heightfield().is_none());
        let grid = crate::physics_heightfield::Heightfield::new(3, 3, 1.0, 1.0, vec![0.0; 9]);
        let shape = PhysicsColliderShape::heightfield_shape(grid.half_extents()).unwrap();
        let material = PhysicsMaterial::new(
            Ratio::new(0.5).unwrap(),
            Ratio::new(0.5).unwrap(),
            Ratio::new(1.0).unwrap(),
        )
        .unwrap();
        let hf = PhysicsCollider::new_heightfield(
            PhysicsColliderHandle::from_raw(3),
            PhysicsBodyHandle::from_raw(4),
            shape,
            material,
            false,
            grid,
        );
        assert!(hf.heightfield().is_some());
        assert_eq!(hf.shape().kind(), crate::physics_shape_kind::PhysicsShapeKind::Heightfield);
    }
}
