//! A named effector in a [`crate::humanoid_rig::HumanoidRigSpec`]: a stable name,
//! the joint it rides, and a local offset from that joint.
//!
//! An effector is an *interaction point* — a foot sole, a hand, a gaze direction
//! — computed in world space by composing its joint's world transform with its
//! local offset. Pose goals and constraints target effectors by name (e.g.
//! "pin `left_foot_sole` to the plant spot").

use axiom_math::Transform;

use crate::ids::JointId;

/// One effector of a rig: a named point offset from a joint.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Effector {
    name: &'static str,
    joint: JointId,
    offset: Transform,
}

impl Effector {
    /// Construct an effector `name` attached to `joint` at local `offset`.
    pub const fn new(name: &'static str, joint: JointId, offset: Transform) -> Self {
        Effector {
            name,
            joint,
            offset,
        }
    }

    /// The effector's stable name.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The joint the effector rides.
    pub const fn joint(&self) -> JointId {
        self.joint
    }

    /// The effector's local offset from its joint.
    pub const fn offset(&self) -> Transform {
        self.offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::Vec3;

    #[test]
    fn effector_records_its_joint_and_offset() {
        let off = Transform::from_translation(Vec3::new(0.0, -0.05, 0.0));
        let e = Effector::new("left_foot_sole", JointId::from_raw(18), off);
        assert_eq!(e.name(), "left_foot_sole");
        assert_eq!(e.joint(), JointId::from_raw(18));
        assert_eq!(e.offset(), off);
    }
}
