//! A named joint in a [`crate::humanoid_rig::HumanoidRigSpec`]: a stable name, an
//! optional parent, and a bind (rest) local transform.

use axiom_math::Transform;

use crate::ids::JointId;

/// One joint of a rig. Joints are stored in parent-before-child insertion order,
/// so every non-root joint's parent has a strictly smaller [`JointId`] — the
/// property that lets forward kinematics resolve in a single forward pass with no
/// sorting and no recursion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Joint {
    name: &'static str,
    parent: Option<JointId>,
    bind_local: Transform,
}

impl Joint {
    /// A root joint (no parent) with the given rest local transform.
    pub const fn root(name: &'static str, bind_local: Transform) -> Self {
        Joint {
            name,
            parent: None,
            bind_local,
        }
    }

    /// A child joint parented to `parent`.
    pub const fn child(name: &'static str, parent: JointId, bind_local: Transform) -> Self {
        Joint {
            name,
            parent: Some(parent),
            bind_local,
        }
    }

    /// The joint's stable name.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The joint's parent, or `None` for a root.
    pub const fn parent(&self) -> Option<JointId> {
        self.parent
    }

    /// The joint's bind (rest) local transform.
    pub const fn bind_local(&self) -> Transform {
        self.bind_local
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::Vec3;

    #[test]
    fn root_has_no_parent_and_keeps_its_bind() {
        let t = Transform::from_translation(Vec3::new(0.0, 1.0, 0.0));
        let j = Joint::root("root", t);
        assert_eq!(j.name(), "root");
        assert_eq!(j.parent(), None);
        assert_eq!(j.bind_local(), t);
    }

    #[test]
    fn child_records_its_parent() {
        let j = Joint::child("pelvis", JointId::from_raw(0), Transform::IDENTITY);
        assert_eq!(j.name(), "pelvis");
        assert_eq!(j.parent(), Some(JointId::from_raw(0)));
        assert_eq!(j.bind_local(), Transform::IDENTITY);
    }
}
