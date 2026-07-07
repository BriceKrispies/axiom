//! [`HumanoidRigSpec`] — a named joint hierarchy plus named effectors, and the
//! standard PS1/low-poly sports-character constructor.
//!
//! The rig is deliberately schematic (a low-poly athlete, not an anatomical
//! model): the joint chain and bind offsets are chosen so forward kinematics
//! produces sensible, testable interaction points. Convention: `+X` is the
//! character's left, `+Y` is up, `+Z` is forward (the direction it faces and
//! kicks).

use axiom_math::{Transform, Vec3};

use crate::effector::Effector;
use crate::ids::{EffectorId, JointId};
use crate::joint::Joint;

/// A humanoid rig: joints in parent-before-child order plus named effectors.
/// Built once (usually via [`HumanoidRigSpec::standard_humanoid`]) and thereafter
/// read-only; it is the actor a [`crate::motion_spec::MotionSpec`] references.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HumanoidRigSpec {
    joints: Vec<Joint>,
    effectors: Vec<Effector>,
}

/// A translation-only bind local, the common case for a schematic rig.
const fn offset(x: f32, y: f32, z: f32) -> Transform {
    Transform::from_translation(Vec3::new(x, y, z))
}

impl HumanoidRigSpec {
    /// An empty rig.
    pub(crate) fn empty() -> Self {
        HumanoidRigSpec {
            joints: Vec::new(),
            effectors: Vec::new(),
        }
    }

    /// Append a root joint, returning its id.
    pub(crate) fn push_root(&mut self, name: &'static str, bind: Transform) -> JointId {
        let id = JointId::from_raw(self.joints.len() as u64);
        self.joints.push(Joint::root(name, bind));
        id
    }

    /// Append a child joint parented to `parent`, returning its id.
    pub(crate) fn push_child(
        &mut self,
        name: &'static str,
        parent: JointId,
        bind: Transform,
    ) -> JointId {
        let id = JointId::from_raw(self.joints.len() as u64);
        self.joints.push(Joint::child(name, parent, bind));
        id
    }

    /// Append an effector on `joint` at local `off`, returning its id.
    pub(crate) fn push_effector(
        &mut self,
        name: &'static str,
        joint: JointId,
        off: Transform,
    ) -> EffectorId {
        let id = EffectorId::from_raw(self.effectors.len() as u64);
        self.effectors.push(Effector::new(name, joint, off));
        id
    }

    /// The joints in insertion order.
    pub(crate) fn joints(&self) -> &[Joint] {
        &self.joints
    }

    /// The effectors in insertion order.
    pub(crate) fn effectors(&self) -> &[Effector] {
        &self.effectors
    }

    /// The id of the joint named `name`, if present.
    pub(crate) fn joint_id(&self, name: &str) -> Option<JointId> {
        self.joints
            .iter()
            .position(|j| j.name() == name)
            .map(|i| JointId::from_raw(i as u64))
    }

    /// The id of the effector named `name`, if present.
    pub(crate) fn effector_id(&self, name: &str) -> Option<EffectorId> {
        self.effectors
            .iter()
            .position(|e| e.name() == name)
            .map(|i| EffectorId::from_raw(i as u64))
    }

    /// The effector at `id`, if in range.
    pub(crate) fn effector(&self, id: EffectorId) -> Option<Effector> {
        self.effectors.get(id.raw() as usize).copied()
    }

    /// Every joint name, in order.
    pub(crate) fn joint_names(&self) -> Vec<&'static str> {
        self.joints.iter().map(Joint::name).collect()
    }

    /// Every effector name, in order.
    pub(crate) fn effector_names(&self) -> Vec<&'static str> {
        self.effectors.iter().map(Effector::name).collect()
    }

    /// Whether the hierarchy is valid: the rig is non-empty and every joint's
    /// parent (if any) has a strictly smaller index than the joint itself (which
    /// also forces joint `0` to be a root). This is the single invariant forward
    /// kinematics relies on.
    pub(crate) fn is_valid(&self) -> bool {
        (!self.joints.is_empty())
            & self.joints.iter().enumerate().all(|(i, j)| {
                j.parent().map(|p| p.raw() < i as u64).unwrap_or(true)
            })
    }

    /// The standard PS1/low-poly sports-character rig: a spine chain
    /// (root→pelvis→spine_lower→spine_upper→chest→neck→head), two arms
    /// (shoulder→upper_arm→forearm→hand) off the chest, and two legs
    /// (hip→thigh→shin→foot→toe) off the pelvis, plus the eight named effectors.
    pub(crate) fn standard_humanoid() -> Self {
        let mut r = HumanoidRigSpec::empty();

        // Spine chain.
        let root = r.push_root("root", Transform::IDENTITY);
        let pelvis = r.push_child("pelvis", root, offset(0.0, 0.9, 0.0));
        let spine_lower = r.push_child("spine_lower", pelvis, offset(0.0, 0.15, 0.0));
        let spine_upper = r.push_child("spine_upper", spine_lower, offset(0.0, 0.15, 0.0));
        let chest = r.push_child("chest", spine_upper, offset(0.0, 0.15, 0.0));
        let neck = r.push_child("neck", chest, offset(0.0, 0.12, 0.0));
        let head = r.push_child("head", neck, offset(0.0, 0.12, 0.0));

        // Left arm.
        let l_shoulder = r.push_child("left_shoulder", chest, offset(0.16, 0.05, 0.0));
        let l_upper_arm = r.push_child("left_upper_arm", l_shoulder, offset(0.02, -0.25, 0.0));
        let l_forearm = r.push_child("left_forearm", l_upper_arm, offset(0.0, -0.25, 0.0));
        let l_hand = r.push_child("left_hand", l_forearm, offset(0.0, -0.08, 0.0));

        // Right arm.
        let r_shoulder = r.push_child("right_shoulder", chest, offset(-0.16, 0.05, 0.0));
        let r_upper_arm = r.push_child("right_upper_arm", r_shoulder, offset(-0.02, -0.25, 0.0));
        let r_forearm = r.push_child("right_forearm", r_upper_arm, offset(0.0, -0.25, 0.0));
        let r_hand = r.push_child("right_hand", r_forearm, offset(0.0, -0.08, 0.0));

        // Left leg.
        let l_hip = r.push_child("left_hip", pelvis, offset(0.10, 0.0, 0.0));
        let l_thigh = r.push_child("left_thigh", l_hip, offset(0.0, -0.45, 0.0));
        let l_shin = r.push_child("left_shin", l_thigh, offset(0.0, -0.45, 0.0));
        let l_foot = r.push_child("left_foot", l_shin, offset(0.0, -0.05, 0.12));
        r.push_child("left_toe", l_foot, offset(0.0, 0.0, 0.10));

        // Right leg.
        let r_hip = r.push_child("right_hip", pelvis, offset(-0.10, 0.0, 0.0));
        let r_thigh = r.push_child("right_thigh", r_hip, offset(0.0, -0.45, 0.0));
        let r_shin = r.push_child("right_shin", r_thigh, offset(0.0, -0.45, 0.0));
        let r_foot = r.push_child("right_foot", r_shin, offset(0.0, -0.05, 0.12));
        r.push_child("right_toe", r_foot, offset(0.0, 0.0, 0.10));

        // Effectors.
        r.push_effector("left_foot_sole", l_foot, offset(0.0, -0.05, 0.0));
        r.push_effector("right_foot_sole", r_foot, offset(0.0, -0.05, 0.0));
        r.push_effector("right_foot_instep", r_foot, offset(0.0, 0.02, 0.06));
        r.push_effector("left_hand", l_hand, offset(0.0, -0.05, 0.0));
        r.push_effector("right_hand", r_hand, offset(0.0, -0.05, 0.0));
        r.push_effector("head_gaze", head, offset(0.0, 0.0, 0.10));
        r.push_effector("chest_forward", chest, offset(0.0, 0.0, 0.10));
        r.push_effector("pelvis_forward", pelvis, offset(0.0, 0.0, 0.10));

        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_humanoid_has_the_expected_joints_and_effectors() {
        let rig = HumanoidRigSpec::standard_humanoid();
        assert_eq!(rig.joints().len(), 25);
        assert_eq!(rig.effectors().len(), 8);
        assert!(rig.is_valid());
        // A few representative names resolve.
        assert_eq!(rig.joint_id("root"), Some(JointId::from_raw(0)));
        assert!(rig.joint_id("right_thigh").is_some());
        assert!(rig.joint_id("head").is_some());
        assert!(rig.effector_id("right_foot_instep").is_some());
        assert!(rig.effector_id("pelvis_forward").is_some());
    }

    #[test]
    fn every_named_joint_and_effector_from_the_spec_is_present() {
        let rig = HumanoidRigSpec::standard_humanoid();
        let joints = rig.joint_names();
        [
            "root", "pelvis", "spine_lower", "spine_upper", "chest", "neck", "head",
            "left_shoulder", "left_upper_arm", "left_forearm", "left_hand",
            "right_shoulder", "right_upper_arm", "right_forearm", "right_hand",
            "left_hip", "left_thigh", "left_shin", "left_foot", "left_toe",
            "right_hip", "right_thigh", "right_shin", "right_foot", "right_toe",
        ]
        .iter()
        .for_each(|n| assert!(joints.contains(n), "missing joint {n}"));
        let effectors = rig.effector_names();
        [
            "left_foot_sole", "right_foot_sole", "right_foot_instep", "left_hand",
            "right_hand", "head_gaze", "chest_forward", "pelvis_forward",
        ]
        .iter()
        .for_each(|n| assert!(effectors.contains(n), "missing effector {n}"));
    }

    #[test]
    fn parent_indices_are_always_smaller_than_the_child() {
        let rig = HumanoidRigSpec::standard_humanoid();
        rig.joints().iter().enumerate().for_each(|(i, j)| {
            j.parent()
                .into_iter()
                .for_each(|p| assert!(p.raw() < i as u64, "joint {i} parents forward"));
        });
    }

    #[test]
    fn unknown_names_resolve_to_none_and_out_of_range_ids_are_none() {
        let rig = HumanoidRigSpec::standard_humanoid();
        assert_eq!(rig.joint_id("no_such_joint"), None);
        assert_eq!(rig.effector_id("no_such_effector"), None);
        assert_eq!(rig.effector(EffectorId::from_raw(999)), None);
        assert!(rig.effector(EffectorId::from_raw(0)).is_some());
    }

    #[test]
    fn an_empty_rig_is_invalid() {
        assert!(!HumanoidRigSpec::empty().is_valid());
    }

    #[test]
    fn a_forward_referencing_parent_is_invalid() {
        let mut r = HumanoidRigSpec::empty();
        r.push_root("root", Transform::IDENTITY);
        // A child whose parent index is not smaller than its own index.
        r.push_child("bad", JointId::from_raw(9), Transform::IDENTITY);
        assert!(!r.is_valid());
    }

    #[test]
    fn default_rig_is_empty() {
        assert_eq!(HumanoidRigSpec::default(), HumanoidRigSpec::empty());
    }
}
