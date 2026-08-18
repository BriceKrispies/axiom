//! The internal per-shape result of a query, before it is tagged with the
//! collider and body it belongs to.
//!
//! Every per-shape query function — the ray casts in [`crate::query_ray`], the
//! sweeps in [`crate::query_sweep`] — answers with this: the math layer's
//! [`Hit`] (time of impact, contact point on the struck surface, unit normal
//! facing the caster), plus the one fact `Hit` cannot carry, because it is about
//! the *caster* rather than the surface.
//!
//! ## `front_face`
//! `true` when the caster began **outside** the collider and met its
//! outward-facing surface; `false` when it began already inside or already
//! overlapping, in which case the time of impact is `0` and the reported surface
//! is the nearest way out rather than a genuine entry.
//!
//! This distinction is not decoration. A bullet tracing through stacked geometry
//! has to tell "I have entered a new surface" from "I started inside this wall",
//! and a character controller has to tell "I will touch the floor part-way
//! through this step" from "I am standing in the floor and must be pushed out".
//! Both are the same `t = 0` hit without it.

use axiom_math::Hit;

/// One shape's answer to a query: where it was struck, and whether the caster
/// started outside it.
pub(crate) struct QueryHit {
    hit: Hit,
    front_face: bool,
}

impl QueryHit {
    /// Build a hit whose caster started outside the collider (`front_face`) or
    /// inside it.
    pub(crate) fn new(hit: Hit, front_face: bool) -> Self {
        QueryHit { hit, front_face }
    }

    /// The geometric contact: time, point, normal.
    pub(crate) fn hit(&self) -> Hit {
        self.hit
    }

    /// Whether the caster started outside this collider.
    pub(crate) fn front_face(&self) -> bool {
        self.front_face
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::Vec3;

    #[test]
    fn a_query_hit_carries_its_contact_and_its_facing() {
        let entering = QueryHit::new(Hit::new(3.0, Vec3::UNIT_X, Vec3::UNIT_Y), true);
        assert_eq!(entering.hit().time(), 3.0);
        assert_eq!(entering.hit().point(), Vec3::UNIT_X);
        assert_eq!(entering.hit().normal(), Vec3::UNIT_Y);
        assert!(entering.front_face());

        let started_inside = QueryHit::new(Hit::new(0.0, Vec3::ZERO, Vec3::UNIT_Y), false);
        assert!(!started_inside.front_face());
        assert_eq!(started_inside.hit().time(), 0.0);
    }
}
