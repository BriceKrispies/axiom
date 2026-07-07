//! The authoring module's identity vocabulary — the value-type ids the
//! [`crate::AnimationAuthoringApi`] facade hands back and accepts.
//!
//! All are opaque, deterministic newtypes; none depend on pointer addresses or
//! randomness, so the same sequence of authoring calls always yields the same
//! ids. [`RigId`], [`MotionId`] and [`PlanId`] are monotonic handles into the
//! facade's registries. [`JointId`] / [`EffectorId`] are stable indices *within*
//! a rig (index `0` is the first joint/effector added). [`TargetId`] is a stable
//! index within a motion's target list. [`PhaseId`] names a phase within a
//! specific motion, so a caller can attach goals to it without re-naming the
//! motion.

/// A handle to a humanoid rig registered in an [`crate::AnimationAuthoringApi`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RigId(u64);

impl RigId {
    /// Construct from a raw index.
    pub const fn from_raw(raw: u64) -> Self {
        RigId(raw)
    }

    /// The underlying raw index.
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// A handle to a motion spec registered in an [`crate::AnimationAuthoringApi`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MotionId(u64);

impl MotionId {
    /// Construct from a raw index.
    pub const fn from_raw(raw: u64) -> Self {
        MotionId(raw)
    }

    /// The underlying raw index.
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// A handle to a compiled motion plan registered in an
/// [`crate::AnimationAuthoringApi`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlanId(u64);

impl PlanId {
    /// Construct from a raw index.
    pub const fn from_raw(raw: u64) -> Self {
        PlanId(raw)
    }

    /// The underlying raw index.
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// A stable joint index within a single rig. Joint `0` is the first joint added;
/// a child joint always has a larger index than its parent, which is what makes
/// forward-kinematics resolution a single forward pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JointId(u64);

impl JointId {
    /// Construct from a raw index.
    pub const fn from_raw(raw: u64) -> Self {
        JointId(raw)
    }

    /// The underlying raw index.
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// A stable effector index within a single rig.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EffectorId(u64);

impl EffectorId {
    /// Construct from a raw index.
    pub const fn from_raw(raw: u64) -> Self {
        EffectorId(raw)
    }

    /// The underlying raw index.
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// A stable target index within a single motion's target list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TargetId(u64);

impl TargetId {
    /// Construct from a raw index.
    pub const fn from_raw(raw: u64) -> Self {
        TargetId(raw)
    }

    /// The underlying raw index.
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// A handle to a phase within a specific motion: the owning [`MotionId`] plus the
/// phase's insertion index. Returned by `add_phase` so pose goals, constraints
/// and contacts can be attached to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PhaseId {
    motion: MotionId,
    index: u64,
}

impl PhaseId {
    /// Construct from an owning motion and a phase index.
    pub const fn new(motion: MotionId, index: u64) -> Self {
        PhaseId { motion, index }
    }

    /// The motion this phase belongs to.
    pub const fn motion(self) -> MotionId {
        self.motion
    }

    /// The phase's insertion index within its motion.
    pub const fn index(self) -> u64 {
        self.index
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_ids_round_trip_and_order_numerically() {
        assert_eq!(RigId::from_raw(3).raw(), 3);
        assert_eq!(MotionId::from_raw(9).raw(), 9);
        assert_eq!(PlanId::from_raw(1).raw(), 1);
        assert!(RigId::from_raw(1) < RigId::from_raw(2));
        assert_eq!(MotionId::from_raw(4), MotionId::from_raw(4));
        assert_ne!(PlanId::from_raw(1), PlanId::from_raw(2));
    }

    #[test]
    fn index_ids_round_trip_and_order() {
        assert_eq!(JointId::from_raw(0).raw(), 0);
        assert_eq!(EffectorId::from_raw(5).raw(), 5);
        assert_eq!(TargetId::from_raw(2).raw(), 2);
        assert!(JointId::from_raw(1) < JointId::from_raw(2));
        assert_ne!(EffectorId::from_raw(0), EffectorId::from_raw(1));
        assert_eq!(TargetId::from_raw(7), TargetId::from_raw(7));
    }

    #[test]
    fn phase_id_carries_motion_and_index() {
        let p = PhaseId::new(MotionId::from_raw(2), 3);
        assert_eq!(p.motion(), MotionId::from_raw(2));
        assert_eq!(p.index(), 3);
        assert_eq!(p, PhaseId::new(MotionId::from_raw(2), 3));
        assert_ne!(p, PhaseId::new(MotionId::from_raw(2), 4));
        assert_ne!(p, PhaseId::new(MotionId::from_raw(1), 3));
    }
}
