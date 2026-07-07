//! [`PoseGoal`] — the authored pose-shaping vocabulary, and [`ResolvedGoal`], its
//! name-resolved form.
//!
//! A pose goal is a small, declarative statement of intent ("swing the right leg
//! back", "aim the right foot at the ball"). It is authored against *names*
//! (joints, effectors, targets); the compiler resolves those names to ids and
//! positions, and the sampler applies the resolved goal to a working pose. The
//! per-kind resolution and application live in the compiler and sampler
//! respectively (each a table indexed by [`GoalKind`]); this file owns only the
//! data.

use axiom_math::Vec3;

use crate::ids::JointId;

/// The pose-goal vocabulary. The discriminant is the dispatch index used by the
/// compiler's resolver table and the sampler's applier table, so the order is
/// fixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalKind {
    /// Rotate a named joint toward an explicit Euler orientation.
    SetJointRotation,
    /// Aim an effector's joint so the effector points at a target.
    AimEffectorAtTarget,
    /// Translate an effector's joint a fraction of the way toward a target.
    MoveEffectorTowardTarget,
    /// Raise an arm outward for balance.
    RaiseArmForBalance,
    /// Twist the torso to face a target.
    TorsoTwistTowardTarget,
    /// Draw a leg back (a wind-up).
    LegBackswing,
    /// Swing a leg forward through a target (the strike).
    LegStrike,
    /// Continue a leg's forward swing past a target (the follow-through).
    FollowThrough,
    /// Oscillate a joint's fore/aft (X) angle over the phase's raw progress — the
    /// per-joint building block of a locomotion cycle (a walk/run). The joint's
    /// angle is `bias + amplitude·sin(TAU·steps·progress + phase_offset)`, so a
    /// thigh swings fore/aft, a shin bends on the lift, and an arm pumps — each with
    /// its own phase offset. Unlike every other goal this cycles on **raw progress**,
    /// not the eased, weight-scaled strength, so a step never shrinks over the phase.
    RunCycle,
}

/// An authored pose goal: a kind plus the (kind-specific) names and parameters.
/// Unused fields for a given kind are `None`/default and ignored.
#[derive(Debug, Clone, PartialEq)]
pub struct PoseGoal {
    kind: GoalKind,
    joint_name: Option<String>,
    effector_name: Option<String>,
    target_name: Option<String>,
    side_right: bool,
    amount: f32,
    euler: Vec3,
    /// The rotation axis a [`GoalKind::RunCycle`] oscillates about (a canonical unit
    /// axis: `+X` = fore/aft swing, `+Z` = lateral abduction). Ignored by every other
    /// kind (left `ZERO`).
    axis: Vec3,
}

impl PoseGoal {
    /// Rotate `joint` toward `euler` (radians, XYZ).
    pub(crate) fn set_joint_rotation(joint: &str, euler: Vec3) -> Self {
        PoseGoal {
            kind: GoalKind::SetJointRotation,
            joint_name: Some(joint.to_string()),
            effector_name: None,
            target_name: None,
            side_right: false,
            amount: 0.0,
            euler,
            axis: Vec3::ZERO,
        }
    }

    /// Aim `effector` at `target`.
    pub(crate) fn aim_effector_at_target(effector: &str, target: &str) -> Self {
        PoseGoal {
            kind: GoalKind::AimEffectorAtTarget,
            joint_name: None,
            effector_name: Some(effector.to_string()),
            target_name: Some(target.to_string()),
            side_right: false,
            amount: 0.0,
            euler: Vec3::ZERO,
            axis: Vec3::ZERO,
        }
    }

    /// Move `effector` a fraction `amount` toward `target`.
    pub(crate) fn move_effector_toward_target(effector: &str, target: &str, amount: f32) -> Self {
        PoseGoal {
            kind: GoalKind::MoveEffectorTowardTarget,
            joint_name: None,
            effector_name: Some(effector.to_string()),
            target_name: Some(target.to_string()),
            side_right: false,
            amount,
            euler: Vec3::ZERO,
            axis: Vec3::ZERO,
        }
    }

    /// Raise the right (`right = true`) or left arm for balance.
    pub(crate) fn raise_arm_for_balance(right: bool) -> Self {
        PoseGoal {
            kind: GoalKind::RaiseArmForBalance,
            joint_name: None,
            effector_name: None,
            target_name: None,
            side_right: right,
            amount: 0.0,
            euler: Vec3::ZERO,
            axis: Vec3::ZERO,
        }
    }

    /// Twist the torso toward `target` by `amount`.
    pub(crate) fn torso_twist_toward_target(target: &str, amount: f32) -> Self {
        PoseGoal {
            kind: GoalKind::TorsoTwistTowardTarget,
            joint_name: None,
            effector_name: None,
            target_name: Some(target.to_string()),
            side_right: false,
            amount,
            euler: Vec3::ZERO,
            axis: Vec3::ZERO,
        }
    }

    /// Draw the right/left leg back by `amount`.
    pub(crate) fn leg_backswing(right: bool, amount: f32) -> Self {
        PoseGoal {
            kind: GoalKind::LegBackswing,
            joint_name: None,
            effector_name: None,
            target_name: None,
            side_right: right,
            amount,
            euler: Vec3::ZERO,
            axis: Vec3::ZERO,
        }
    }

    /// Strike with the right/left leg toward `target`.
    pub(crate) fn leg_strike(right: bool, target: &str) -> Self {
        PoseGoal {
            kind: GoalKind::LegStrike,
            joint_name: None,
            effector_name: None,
            target_name: Some(target.to_string()),
            side_right: right,
            amount: 0.0,
            euler: Vec3::ZERO,
            axis: Vec3::ZERO,
        }
    }

    /// Follow through with the right/left leg toward `target`.
    pub(crate) fn follow_through(right: bool, target: &str) -> Self {
        PoseGoal {
            kind: GoalKind::FollowThrough,
            joint_name: None,
            effector_name: None,
            target_name: Some(target.to_string()),
            side_right: right,
            amount: 0.0,
            euler: Vec3::ZERO,
            axis: Vec3::ZERO,
        }
    }

    /// Oscillate `joint` about `axis` over the phase: angle `bias + amplitude·sin(
    /// TAU·steps·progress + phase_offset)`. The three cycle parameters ride in
    /// `euler = (phase_offset, steps, bias)`; `amount` carries the `amplitude`; `axis`
    /// is the canonical unit axis (`+X` fore/aft swing, `+Z` lateral abduction).
    pub(crate) fn run_cycle(joint: &str, amplitude: f32, phase_offset: f32, steps: f32, bias: f32, axis: Vec3) -> Self {
        PoseGoal {
            kind: GoalKind::RunCycle,
            joint_name: Some(joint.to_string()),
            effector_name: None,
            target_name: None,
            side_right: false,
            amount: amplitude,
            euler: Vec3::new(phase_offset, steps, bias),
            axis,
        }
    }

    /// The goal kind.
    pub(crate) fn kind(&self) -> GoalKind {
        self.kind
    }

    /// The referenced joint name, if any.
    pub(crate) fn joint_name(&self) -> Option<&str> {
        self.joint_name.as_deref()
    }

    /// The referenced effector name, if any.
    pub(crate) fn effector_name(&self) -> Option<&str> {
        self.effector_name.as_deref()
    }

    /// The referenced target name, if any.
    pub(crate) fn target_name(&self) -> Option<&str> {
        self.target_name.as_deref()
    }

    /// Whether the goal applies to the right side.
    pub(crate) fn side_right(&self) -> bool {
        self.side_right
    }

    /// The scalar magnitude parameter.
    pub(crate) fn amount(&self) -> f32 {
        self.amount
    }

    /// The explicit Euler orientation (only `SetJointRotation` uses it).
    pub(crate) fn euler(&self) -> Vec3 {
        self.euler
    }

    /// The oscillation axis (only `RunCycle` uses it).
    pub(crate) fn axis(&self) -> Vec3 {
        self.axis
    }
}

/// A resolved pose goal: names replaced by the primary joint id the goal drives
/// (effector goals resolve to their effector's joint) and a resolved target
/// position. Fields not meaningful for the kind are set to a harmless default.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedGoal {
    kind: GoalKind,
    joint: JointId,
    target: Vec3,
    amount: f32,
    euler: Vec3,
}

impl ResolvedGoal {
    /// Construct a resolved goal.
    pub(crate) fn new(kind: GoalKind, joint: JointId, target: Vec3, amount: f32, euler: Vec3) -> Self {
        ResolvedGoal {
            kind,
            joint,
            target,
            amount,
            euler,
        }
    }

    /// The goal kind.
    pub(crate) fn kind(&self) -> GoalKind {
        self.kind
    }

    /// The primary joint the goal drives.
    pub(crate) fn joint(&self) -> JointId {
        self.joint
    }

    /// The resolved target position.
    pub(crate) fn target(&self) -> Vec3 {
        self.target
    }

    /// The scalar magnitude parameter.
    pub(crate) fn amount(&self) -> f32 {
        self.amount
    }

    /// The explicit Euler orientation.
    pub(crate) fn euler(&self) -> Vec3 {
        self.euler
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_set_the_right_kind_and_fields() {
        let s = PoseGoal::set_joint_rotation("chest", Vec3::new(0.1, 0.2, 0.3));
        assert_eq!(s.kind(), GoalKind::SetJointRotation);
        assert_eq!(s.joint_name(), Some("chest"));
        assert_eq!(s.euler(), Vec3::new(0.1, 0.2, 0.3));

        let a = PoseGoal::aim_effector_at_target("right_foot_instep", "ball");
        assert_eq!(a.kind(), GoalKind::AimEffectorAtTarget);
        assert_eq!(a.effector_name(), Some("right_foot_instep"));
        assert_eq!(a.target_name(), Some("ball"));

        let m = PoseGoal::move_effector_toward_target("left_hand", "ball", 0.5);
        assert_eq!(m.kind(), GoalKind::MoveEffectorTowardTarget);
        assert_eq!(m.amount(), 0.5);

        let r = PoseGoal::raise_arm_for_balance(true);
        assert_eq!(r.kind(), GoalKind::RaiseArmForBalance);
        assert!(r.side_right());

        let t = PoseGoal::torso_twist_toward_target("net_center", 0.7);
        assert_eq!(t.kind(), GoalKind::TorsoTwistTowardTarget);
        assert_eq!(t.target_name(), Some("net_center"));
        assert_eq!(t.amount(), 0.7);

        let b = PoseGoal::leg_backswing(true, 0.8);
        assert_eq!(b.kind(), GoalKind::LegBackswing);
        assert!(b.side_right());
        assert_eq!(b.amount(), 0.8);

        let k = PoseGoal::leg_strike(true, "ball");
        assert_eq!(k.kind(), GoalKind::LegStrike);
        assert_eq!(k.target_name(), Some("ball"));

        let f = PoseGoal::follow_through(false, "net_center");
        assert_eq!(f.kind(), GoalKind::FollowThrough);
        assert!(!f.side_right());
        assert_eq!(f.target_name(), Some("net_center"));

        let rc = PoseGoal::run_cycle("left_thigh", 0.5, 1.0, 3.0, 0.2, Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(rc.kind(), GoalKind::RunCycle);
        assert_eq!(rc.joint_name(), Some("left_thigh"));
        assert_eq!(rc.amount(), 0.5);
        assert_eq!(rc.euler(), Vec3::new(1.0, 3.0, 0.2));
        assert_eq!(rc.axis(), Vec3::new(1.0, 0.0, 0.0));
    }

    #[test]
    fn resolved_goal_round_trips_its_fields() {
        let g = ResolvedGoal::new(
            GoalKind::LegStrike,
            JointId::from_raw(21),
            Vec3::new(0.0, 0.0, 1.0),
            0.4,
            Vec3::new(0.0, 0.0, 0.0),
        );
        assert_eq!(g.kind(), GoalKind::LegStrike);
        assert_eq!(g.joint(), JointId::from_raw(21));
        assert_eq!(g.target(), Vec3::new(0.0, 0.0, 1.0));
        assert_eq!(g.amount(), 0.4);
        assert_eq!(g.euler(), Vec3::ZERO);
    }
}
