//! [`ContactDeclaration`] — an authored declaration that an effector is in contact
//! with (planted on) a target during a phase, and [`ResolvedContact`], its
//! name-resolved form.
//!
//! A contact is the softer sibling of a pin constraint: it states that a foot is
//! planted this phase. The sampler both records it as active in the pose frame
//! and pins the effector's world position to the contact target, so a planted
//! foot stays put.

use axiom_math::Vec3;

use crate::ids::EffectorId;

/// An authored contact: an effector name planted on a target name.
#[derive(Debug, Clone, PartialEq)]
pub struct ContactDeclaration {
    effector_name: String,
    target_name: String,
}

impl ContactDeclaration {
    /// Declare `effector` planted on `target`.
    pub(crate) fn new(effector: &str, target: &str) -> Self {
        ContactDeclaration {
            effector_name: effector.to_string(),
            target_name: target.to_string(),
        }
    }

    /// The planted effector name.
    pub(crate) fn effector_name(&self) -> &str {
        &self.effector_name
    }

    /// The contact target name.
    pub(crate) fn target_name(&self) -> &str {
        &self.target_name
    }
}

/// A resolved contact: names replaced by an effector id and a target position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedContact {
    effector: EffectorId,
    target: Vec3,
}

impl ResolvedContact {
    /// Construct a resolved contact.
    pub(crate) fn new(effector: EffectorId, target: Vec3) -> Self {
        ResolvedContact { effector, target }
    }

    /// The `(effector, target)` pin a contact always contributes.
    pub(crate) fn pin(&self) -> (EffectorId, Vec3) {
        (self.effector, self.target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_contact_carries_its_names() {
        let c = ContactDeclaration::new("left_foot_sole", "left_plant_spot");
        assert_eq!(c.effector_name(), "left_foot_sole");
        assert_eq!(c.target_name(), "left_plant_spot");
    }

    #[test]
    fn resolved_contact_yields_its_pin() {
        let c = ResolvedContact::new(EffectorId::from_raw(2), Vec3::new(0.25, 0.0, -0.1));
        assert_eq!(
            c.pin(),
            (EffectorId::from_raw(2), Vec3::new(0.25, 0.0, -0.1))
        );
    }
}
