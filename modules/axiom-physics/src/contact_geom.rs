//! The raw geometric result of one narrow-phase test, shared by every contact
//! generator.
//!
//! A `ContactGeom` is a [`crate::contact_manifold::ContactManifold`] minus its
//! identity: the unit **normal** oriented from collider A toward collider B, the
//! strictly positive penetration **depth**, and the world contact **point**.
//! `generate_contacts` tags it with the pair's collider/body handles.
//!
//! It lives in its own file because every pairing file
//! (`contact_capsule_*.rs`, `contact_box_box.rs`, `contact_pair.rs`) produces
//! one, and the sign convention it carries is the single thing they must all
//! agree on.

use axiom_math::Vec3;

/// One contact's geometry: normal (A→B), positive depth, world point.
pub(crate) struct ContactGeom {
    /// The unit contact normal, pointing from collider A toward collider B.
    pub(crate) normal: Vec3,
    /// The penetration depth, always strictly positive for a real contact.
    pub(crate) depth: f32,
    /// The world contact point, on or within the overlap region.
    pub(crate) point: Vec3,
}

/// Reverse a contact: swapping the A/B roles flips the A→B normal, and leaves
/// the depth and the world point (which belong to no particular collider)
/// untouched.
pub(crate) fn flip(geom: ContactGeom) -> ContactGeom {
    ContactGeom {
        normal: geom.normal.mul_scalar(-1.0),
        depth: geom.depth,
        point: geom.point,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flip_reverses_only_the_normal() {
        let flipped = flip(ContactGeom {
            normal: Vec3::UNIT_Y,
            depth: 0.25,
            point: Vec3::new(1.0, 2.0, 3.0),
        });
        assert_eq!(flipped.normal, Vec3::new(0.0, -1.0, 0.0));
        assert_eq!(flipped.depth, 0.25);
        assert_eq!(flipped.point, Vec3::new(1.0, 2.0, 3.0));
    }
}
