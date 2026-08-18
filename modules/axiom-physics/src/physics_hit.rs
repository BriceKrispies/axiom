//! What a spatial query found: the app-facing record every cast returns.
//!
//! A `PhysicsHit` names the collider and body that were struck **and** describes
//! the touch itself — how far the caster travelled, where it landed, which way
//! the struck surface faces, and whether the caster began outside that surface.
//! It is a sealed value type: built by the query, returned by value, read through
//! accessors, never constructed by a caller. Like [`crate::contact_report`] it
//! exposes *physics* data only; translating it into decals, footstep sounds or
//! damage is an app's job.
//!
//! ## Why a handle alone is not enough
//! The queries used to answer with a bare `Option<PhysicsBodyHandle>` — *what*
//! was hit, and nothing else. Almost everything built on a cast needs more than
//! that: a projectile needs the impact point and the surface normal to spawn an
//! effect and deflect; a footstep needs the surface it landed on; a character
//! controller needs the ground normal to decide whether a slope is walkable and
//! the contact point to step onto a ledge; multi-layer penetration needs to tell
//! entering a wall from being inside one. Every one of those had to re-derive the
//! geometry the query already computed and threw away.
//!
//! ## `distance` means metres travelled, in both queries
//! For a ray cast it is the distance along the ray to the entry point. For a
//! shape cast it is the length of the portion of the motion vector travelled
//! before contact — the same physical quantity, so the two queries can be read
//! the same way. A cast that begins already overlapping reports `0` with
//! [`PhysicsHit::front_face`] `false`.

use axiom_kernel::Meters;
use axiom_math::Vec3;

use crate::physics_body_handle::PhysicsBodyHandle;
use crate::physics_collider_handle::PhysicsColliderHandle;

/// One hit from a ray cast or a shape cast.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhysicsHit {
    body: PhysicsBodyHandle,
    collider: PhysicsColliderHandle,
    distance: Meters,
    point: Vec3,
    normal: Vec3,
    front_face: bool,
}

impl PhysicsHit {
    pub(crate) fn new(
        body: PhysicsBodyHandle,
        collider: PhysicsColliderHandle,
        distance: Meters,
        point: Vec3,
        normal: Vec3,
        front_face: bool,
    ) -> Self {
        PhysicsHit {
            body,
            collider,
            distance,
            point,
            normal,
            front_face,
        }
    }

    /// The body that was struck.
    pub fn body(&self) -> PhysicsBodyHandle {
        self.body
    }

    /// The specific collider on that body that was struck — the one whose
    /// material and shape describe the surface.
    pub fn collider(&self) -> PhysicsColliderHandle {
        self.collider
    }

    /// How far the caster travelled before touching: distance along the ray, or
    /// length of the motion travelled by a shape cast.
    pub fn distance(&self) -> Meters {
        self.distance
    }

    /// The world contact point, on the struck surface.
    pub fn point(&self) -> Vec3 {
        self.point
    }

    /// The unit surface normal at the contact, facing back toward the caster.
    pub fn normal(&self) -> Vec3 {
        self.normal
    }

    /// `true` when the caster began **outside** this collider and met its
    /// outward-facing surface; `false` when it began already inside or already
    /// overlapping, in which case [`PhysicsHit::distance`] is zero and the
    /// surface reported is the nearest way out.
    pub fn front_face(&self) -> bool {
        self.front_face
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(front_face: bool) -> PhysicsHit {
        PhysicsHit::new(
            PhysicsBodyHandle::from_raw(7),
            PhysicsColliderHandle::from_raw(9),
            Meters::new(2.5).unwrap(),
            Vec3::new(1.0, 2.0, 3.0),
            Vec3::UNIT_Y,
            front_face,
        )
    }

    #[test]
    fn accessors_report_every_field_of_the_record() {
        let h = hit(true);
        assert_eq!(h.body(), PhysicsBodyHandle::from_raw(7));
        assert_eq!(h.collider(), PhysicsColliderHandle::from_raw(9));
        assert_eq!(h.distance().get(), 2.5);
        assert_eq!(h.point(), Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(h.normal(), Vec3::UNIT_Y);
        assert!(h.front_face());
        assert!(!hit(false).front_face());
    }

    #[test]
    fn derives_are_exercised() {
        let h = hit(true);
        let copy = h;
        assert_eq!(h, copy);
        assert_ne!(h, hit(false));
        assert!(format!("{h:?}").contains("PhysicsHit"));
    }
}
