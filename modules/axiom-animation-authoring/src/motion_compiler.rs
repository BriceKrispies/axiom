//! [`MotionCompiler`] — validates a [`MotionSpec`] against its rig and resolves it
//! into a sample-ready [`MotionPlan`].
//!
//! Validation rejects, in order: empty/inverted/out-of-range phase or event tick
//! ranges (`InvalidTickRange`), overlapping phases (`OverlappingPhases`), and
//! non-finite authored scalars/positions (`NonFiniteValue`). Resolution then
//! replaces every joint/effector/target *name* with its id/position, rejecting an
//! unknown reference (`UnknownJoint` / `UnknownEffector` / `UnknownTarget`). The
//! per-kind resolvers are dispatched through `const` fn-pointer tables indexed by
//! the goal/event discriminant — a table lookup, never a `match`.

use axiom_math::Vec3;

use crate::authoring_error::AuthoringError;
use crate::authoring_result::AuthoringResult;
use crate::constraint::{Constraint, ResolvedConstraint};
use crate::contact::{ContactDeclaration, ResolvedContact};
use crate::humanoid_rig::HumanoidRigSpec;
use crate::ids::{EffectorId, JointId};
use crate::motion_event::{
    EventKind, MotionEvent, ResolvedEvent, UNUSED_EFFECTOR, UNUSED_TARGET,
};
use crate::motion_phase::{MotionPhase, ResolvedPhase};
use crate::motion_plan::MotionPlan;
use crate::motion_spec::MotionSpec;
use crate::pose_goal::{GoalKind, PoseGoal, ResolvedGoal};
use crate::root_motion::{ResolvedRootMotion, RootMotion};

/// The deterministic motion compiler.
#[derive(Debug)]
pub struct MotionCompiler;

impl MotionCompiler {
    /// Validate `spec` against `rig` and resolve it into a [`MotionPlan`], or
    /// return the first deterministic [`AuthoringError`].
    pub(crate) fn compile(spec: &MotionSpec, rig: &HumanoidRigSpec) -> AuthoringResult<MotionPlan> {
        validate_ranges(spec)
            .and_then(|()| validate_no_overlap(spec))
            .and_then(|()| validate_finite(spec))
            .and_then(|()| resolve(spec, rig))
    }
}

/// Whether all three components of a vector are finite.
fn finite3(v: Vec3) -> bool {
    v.x.is_finite() & v.y.is_finite() & v.z.is_finite()
}

/// Reject empty/inverted phase spans, phase ends past the duration, and events at
/// or beyond the duration.
fn validate_ranges(spec: &MotionSpec) -> AuthoringResult<()> {
    let duration = spec.duration().raw();
    let phases_ok = spec
        .phases()
        .iter()
        .all(|p| (p.start().raw() < p.end().raw()) & (p.end().raw() <= duration));
    let events_ok = spec.events().iter().all(|e| e.tick().raw() < duration);
    (phases_ok & events_ok)
        .then_some(())
        .ok_or_else(|| AuthoringError::invalid_tick_range("phase or event tick range out of bounds"))
}

/// Reject any pair of phases whose `[start, end)` spans overlap.
fn validate_no_overlap(spec: &MotionSpec) -> AuthoringResult<()> {
    let phases = spec.phases();
    let disjoint = phases.iter().enumerate().all(|(i, a)| {
        phases
            .iter()
            .skip(i + 1)
            .all(|b| (a.end().raw() <= b.start().raw()) | (b.end().raw() <= a.start().raw()))
    });
    disjoint
        .then_some(())
        .ok_or_else(|| AuthoringError::overlapping_phases("two phases cover overlapping ticks"))
}

/// Reject any non-finite authored position or scalar.
fn validate_finite(spec: &MotionSpec) -> AuthoringResult<()> {
    let targets_ok = spec.targets().iter().all(|(_, p)| finite3(*p));
    let styles_ok = spec.styles().iter().all(|(_, v)| v.is_finite());
    let phases_ok = spec.phases().iter().all(|ph| {
        ph.layer_weight().is_finite()
            & ph.goals().iter().all(|g| finite3(g.euler()) & g.amount().is_finite())
    });
    let events_ok = spec.events().iter().all(|e| e.power().is_finite());
    (targets_ok & styles_ok & phases_ok & events_ok)
        .then_some(())
        .ok_or_else(|| AuthoringError::non_finite_value("authored value is not finite"))
}

/// Resolve an optional name through `lookup`, propagating `err` when a present
/// name does not resolve. An absent name is `Ok(None)`.
fn resolve_opt<T>(
    name: Option<&str>,
    lookup: impl Fn(&str) -> Option<T>,
    err: AuthoringError,
) -> AuthoringResult<Option<T>> {
    name.map(|n| lookup(n).ok_or(err)).transpose()
}

/// Resolve an effector name to its `(id, joint)`.
fn resolve_effector_joint(
    rig: &HumanoidRigSpec,
    name: Option<&str>,
) -> AuthoringResult<(EffectorId, JointId)> {
    name.and_then(|n| rig.effector_id(n).and_then(|eid| rig.effector(eid).map(|e| (eid, e.joint()))))
        .ok_or_else(|| AuthoringError::unknown_effector("effector name absent from rig"))
}

/// Resolve a target name to its position.
fn resolve_target_pos(spec: &MotionSpec, name: Option<&str>) -> AuthoringResult<Vec3> {
    name.and_then(|n| spec.target_position(n))
        .ok_or_else(|| AuthoringError::unknown_target("target name never declared"))
}

/// Resolve a joint name to its id.
fn resolve_joint(rig: &HumanoidRigSpec, name: &str) -> AuthoringResult<JointId> {
    rig.joint_id(name)
        .ok_or_else(|| AuthoringError::unknown_joint("joint name absent from rig"))
}

/// The joint name a side-driven leg goal targets.
fn thigh_name(right: bool) -> &'static str {
    ["left_thigh", "right_thigh"][right as usize]
}

/// The joint name a side-driven arm goal targets.
fn upper_arm_name(right: bool) -> &'static str {
    ["left_upper_arm", "right_upper_arm"][right as usize]
}

type GoalResolver = fn(&PoseGoal, &HumanoidRigSpec, &MotionSpec) -> AuthoringResult<ResolvedGoal>;

fn resolve_set_joint(g: &PoseGoal, rig: &HumanoidRigSpec, _spec: &MotionSpec) -> AuthoringResult<ResolvedGoal> {
    // Collapse "no joint name" (unreachable — the constructor always sets one) and
    // "joint name absent from rig" into one reachable error arm.
    g.joint_name()
        .and_then(|n| rig.joint_id(n))
        .ok_or_else(|| AuthoringError::unknown_joint("set_joint_rotation joint absent from rig"))
        .map(|joint| ResolvedGoal::new(GoalKind::SetJointRotation, joint, Vec3::ZERO, 0.0, g.euler()))
}

fn resolve_aim(g: &PoseGoal, rig: &HumanoidRigSpec, spec: &MotionSpec) -> AuthoringResult<ResolvedGoal> {
    resolve_effector_joint(rig, g.effector_name()).and_then(|(_eff, joint)| {
        resolve_target_pos(spec, g.target_name())
            .map(|target| ResolvedGoal::new(GoalKind::AimEffectorAtTarget, joint, target, 0.0, Vec3::ZERO))
    })
}

fn resolve_move(g: &PoseGoal, rig: &HumanoidRigSpec, spec: &MotionSpec) -> AuthoringResult<ResolvedGoal> {
    resolve_effector_joint(rig, g.effector_name()).and_then(|(_eff, joint)| {
        resolve_target_pos(spec, g.target_name())
            .map(|target| ResolvedGoal::new(GoalKind::MoveEffectorTowardTarget, joint, target, g.amount(), Vec3::ZERO))
    })
}

fn resolve_raise_arm(g: &PoseGoal, rig: &HumanoidRigSpec, _spec: &MotionSpec) -> AuthoringResult<ResolvedGoal> {
    resolve_joint(rig, upper_arm_name(g.side_right()))
        .map(|joint| ResolvedGoal::new(GoalKind::RaiseArmForBalance, joint, Vec3::ZERO, 0.0, Vec3::ZERO))
}

fn resolve_torso_twist(g: &PoseGoal, rig: &HumanoidRigSpec, spec: &MotionSpec) -> AuthoringResult<ResolvedGoal> {
    resolve_joint(rig, "chest").and_then(|joint| {
        resolve_target_pos(spec, g.target_name())
            .map(|target| ResolvedGoal::new(GoalKind::TorsoTwistTowardTarget, joint, target, g.amount(), Vec3::ZERO))
    })
}

fn resolve_leg_backswing(g: &PoseGoal, rig: &HumanoidRigSpec, _spec: &MotionSpec) -> AuthoringResult<ResolvedGoal> {
    resolve_joint(rig, thigh_name(g.side_right()))
        .map(|joint| ResolvedGoal::new(GoalKind::LegBackswing, joint, Vec3::ZERO, g.amount(), Vec3::ZERO))
}

fn resolve_leg_strike(g: &PoseGoal, rig: &HumanoidRigSpec, spec: &MotionSpec) -> AuthoringResult<ResolvedGoal> {
    resolve_joint(rig, thigh_name(g.side_right())).and_then(|joint| {
        resolve_target_pos(spec, g.target_name())
            .map(|target| ResolvedGoal::new(GoalKind::LegStrike, joint, target, 0.0, Vec3::ZERO))
    })
}

fn resolve_follow_through(g: &PoseGoal, rig: &HumanoidRigSpec, spec: &MotionSpec) -> AuthoringResult<ResolvedGoal> {
    resolve_joint(rig, thigh_name(g.side_right())).and_then(|joint| {
        resolve_target_pos(spec, g.target_name())
            .map(|target| ResolvedGoal::new(GoalKind::FollowThrough, joint, target, 0.0, Vec3::ZERO))
    })
}

/// Per-kind goal resolvers, indexed by [`GoalKind`] discriminant.
const GOAL_RESOLVERS: [GoalResolver; 8] = [
    resolve_set_joint,
    resolve_aim,
    resolve_move,
    resolve_raise_arm,
    resolve_torso_twist,
    resolve_leg_backswing,
    resolve_leg_strike,
    resolve_follow_through,
];

fn resolve_goal(g: &PoseGoal, rig: &HumanoidRigSpec, spec: &MotionSpec) -> AuthoringResult<ResolvedGoal> {
    GOAL_RESOLVERS[g.kind() as usize](g, rig, spec)
}

fn resolve_constraint(c: &Constraint, rig: &HumanoidRigSpec, spec: &MotionSpec) -> AuthoringResult<ResolvedConstraint> {
    resolve_opt(
        c.effector_name(),
        |n| rig.effector_id(n),
        AuthoringError::unknown_effector("constraint effector absent from rig"),
    )
    .and_then(|eff| {
        resolve_opt(
            c.target_name(),
            |n| spec.target_position(n),
            AuthoringError::unknown_target("constraint target never declared"),
        )
        .map(|target| ResolvedConstraint::new(c.kind(), eff, target))
    })
}

fn resolve_contact(c: &ContactDeclaration, rig: &HumanoidRigSpec, spec: &MotionSpec) -> AuthoringResult<ResolvedContact> {
    resolve_effector_joint(rig, Some(c.effector_name())).and_then(|(eff, _joint)| {
        resolve_target_pos(spec, Some(c.target_name())).map(|target| ResolvedContact::new(eff, target))
    })
}

fn resolve_root(root: &RootMotion, spec: &MotionSpec) -> AuthoringResult<ResolvedRootMotion> {
    resolve_opt(
        root.source_name(),
        |n| spec.target_position(n),
        AuthoringError::unknown_target("root-motion `from` target never declared"),
    )
    .and_then(|from| {
        resolve_opt(
            root.dest_name(),
            |n| spec.target_position(n),
            AuthoringError::unknown_target("root-motion `to` target never declared"),
        )
        .map(|to| ResolvedRootMotion::new(root.kind(), from.unwrap_or(Vec3::ZERO), to.unwrap_or(Vec3::ZERO)))
    })
}

fn resolve_phase(ph: &MotionPhase, rig: &HumanoidRigSpec, spec: &MotionSpec) -> AuthoringResult<ResolvedPhase> {
    resolve_root(ph.root(), spec).and_then(|root| {
        ph.goals()
            .iter()
            .map(|g| resolve_goal(g, rig, spec))
            .collect::<AuthoringResult<Vec<_>>>()
            .and_then(|goals| {
                ph.constraints()
                    .iter()
                    .map(|c| resolve_constraint(c, rig, spec))
                    .collect::<AuthoringResult<Vec<_>>>()
                    .and_then(|constraints| {
                        ph.contacts()
                            .iter()
                            .map(|c| resolve_contact(c, rig, spec))
                            .collect::<AuthoringResult<Vec<_>>>()
                            .map(|contacts| {
                                ResolvedPhase::new(
                                    (ph.name().to_string(), ph.start().raw(), ph.end().raw()),
                                    root,
                                    goals,
                                    constraints,
                                    contacts,
                                    ph.ease(),
                                    ph.layer_weight(),
                                )
                            })
                    })
            })
    })
}

type EventResolver = fn(&MotionEvent, &HumanoidRigSpec, &MotionSpec) -> AuthoringResult<ResolvedEvent>;

fn resolve_named(e: &MotionEvent, _rig: &HumanoidRigSpec, _spec: &MotionSpec) -> AuthoringResult<ResolvedEvent> {
    Ok(ResolvedEvent::new(
        EventKind::Named,
        e.tick(),
        e.name().to_string(),
        UNUSED_EFFECTOR,
        UNUSED_TARGET,
        UNUSED_TARGET,
        0.0,
    ))
}

fn resolve_ball_contact(e: &MotionEvent, rig: &HumanoidRigSpec, spec: &MotionSpec) -> AuthoringResult<ResolvedEvent> {
    resolve_effector_joint(rig, e.contact_surface()).and_then(|(surface, _joint)| {
        e.target_name()
            .and_then(|n| spec.target_id(n))
            .ok_or_else(|| AuthoringError::unknown_target("ball-contact target never declared"))
            .and_then(|target| {
                e.direction_target_name()
                    .and_then(|n| spec.target_id(n))
                    .ok_or_else(|| AuthoringError::unknown_target("ball-contact direction target never declared"))
                    .map(|direction| {
                        ResolvedEvent::new(
                            EventKind::BallContact,
                            e.tick(),
                            e.name().to_string(),
                            surface,
                            target,
                            direction,
                            e.power(),
                        )
                    })
            })
    })
}

/// Per-kind event resolvers, indexed by [`EventKind`] discriminant.
const EVENT_RESOLVERS: [EventResolver; 2] = [resolve_named, resolve_ball_contact];

fn resolve_event(e: &MotionEvent, rig: &HumanoidRigSpec, spec: &MotionSpec) -> AuthoringResult<ResolvedEvent> {
    EVENT_RESOLVERS[e.kind() as usize](e, rig, spec)
}

fn resolve(spec: &MotionSpec, rig: &HumanoidRigSpec) -> AuthoringResult<MotionPlan> {
    spec.phases()
        .iter()
        .map(|ph| resolve_phase(ph, rig, spec))
        .collect::<AuthoringResult<Vec<_>>>()
        .and_then(|phases| {
            spec.events()
                .iter()
                .map(|e| resolve_event(e, rig, spec))
                .collect::<AuthoringResult<Vec<_>>>()
                .map(|events| {
                    MotionPlan::new(
                        rig.clone(),
                        spec.duration().raw(),
                        phases,
                        events,
                        spec.targets().to_vec(),
                    )
                })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authoring_error_code::AuthoringErrorCode;
    use crate::ease::EaseCurve;
    use crate::ids::RigId;
    use crate::motion_phase::MotionPhase;
    use axiom_kernel::Tick;

    fn rig() -> HumanoidRigSpec {
        HumanoidRigSpec::standard_humanoid()
    }

    /// A minimal well-formed spec: one target, two disjoint phases, one event.
    fn base_spec() -> MotionSpec {
        let mut m = MotionSpec::new("m", Tick::new(30), RigId::from_raw(0));
        m.add_target("ball", Vec3::new(0.0, 0.0, 0.0));
        m.add_target("net_center", Vec3::new(0.0, 0.8, 8.0));
        m.add_phase(MotionPhase::new("a", Tick::new(0), Tick::new(10)));
        m.add_phase(MotionPhase::new("b", Tick::new(10), Tick::new(30)));
        m
    }

    fn err(spec: &MotionSpec) -> AuthoringErrorCode {
        MotionCompiler::compile(spec, &rig()).unwrap_err().code()
    }

    #[test]
    fn a_well_formed_spec_compiles() {
        let plan = MotionCompiler::compile(&base_spec(), &rig()).unwrap();
        assert_eq!(plan.duration(), 30);
        assert_eq!(plan.phases().len(), 2);
        assert_eq!(plan.rig().joints().len(), 25);
    }

    #[test]
    fn a_named_event_resolves_into_the_plan() {
        let mut m = base_spec();
        m.add_event(MotionEvent::named(Tick::new(5), "whistle"));
        let plan = MotionCompiler::compile(&m, &rig()).unwrap();
        assert_eq!(plan.events().len(), 1);
        assert_eq!(plan.events_at(5)[0].name(), "whistle");
    }

    #[test]
    fn a_valid_ball_contact_event_resolves_into_the_plan() {
        let mut m = base_spec();
        m.add_event(MotionEvent::ball_contact(Tick::new(5), "right_foot_instep", "ball", "net_center", 0.7));
        let plan = MotionCompiler::compile(&m, &rig()).unwrap();
        assert!(plan.events_at(5)[0].is_ball_contact());
        assert!((plan.events_at(5)[0].power() - 0.7).abs() < 1.0e-6);
    }

    #[test]
    fn an_inverted_or_empty_phase_range_is_rejected() {
        let mut m = MotionSpec::new("m", Tick::new(30), RigId::from_raw(0));
        m.add_phase(MotionPhase::new("bad", Tick::new(10), Tick::new(10))); // empty
        assert_eq!(err(&m), AuthoringErrorCode::InvalidTickRange);
        let mut n = MotionSpec::new("n", Tick::new(30), RigId::from_raw(0));
        n.add_phase(MotionPhase::new("bad", Tick::new(20), Tick::new(5))); // inverted
        assert_eq!(err(&n), AuthoringErrorCode::InvalidTickRange);
    }

    #[test]
    fn a_phase_or_event_past_the_duration_is_rejected() {
        let mut m = MotionSpec::new("m", Tick::new(30), RigId::from_raw(0));
        m.add_phase(MotionPhase::new("over", Tick::new(0), Tick::new(31)));
        assert_eq!(err(&m), AuthoringErrorCode::InvalidTickRange);
        let mut n = base_spec();
        n.add_event(MotionEvent::named(Tick::new(30), "late"));
        assert_eq!(err(&n), AuthoringErrorCode::InvalidTickRange);
    }

    #[test]
    fn overlapping_phases_are_rejected() {
        let mut m = MotionSpec::new("m", Tick::new(30), RigId::from_raw(0));
        m.add_phase(MotionPhase::new("a", Tick::new(0), Tick::new(15)));
        m.add_phase(MotionPhase::new("b", Tick::new(10), Tick::new(20)));
        assert_eq!(err(&m), AuthoringErrorCode::OverlappingPhases);
    }

    #[test]
    fn a_non_finite_position_or_scalar_is_rejected() {
        let mut m = base_spec();
        m.add_target("bad", Vec3::new(f32::NAN, 0.0, 0.0));
        assert_eq!(err(&m), AuthoringErrorCode::NonFiniteValue);

        let mut s = base_spec();
        s.set_style("power", f32::INFINITY);
        assert_eq!(err(&s), AuthoringErrorCode::NonFiniteValue);

        let mut g = base_spec();
        g.phase_mut(0).unwrap().push_goal(PoseGoal::leg_backswing(true, f32::NAN));
        assert_eq!(err(&g), AuthoringErrorCode::NonFiniteValue);

        let mut w = base_spec();
        w.phase_mut(0).unwrap().set_layer_weight(f32::NAN);
        assert_eq!(err(&w), AuthoringErrorCode::NonFiniteValue);

        let mut e = base_spec();
        e.add_event(MotionEvent::ball_contact(Tick::new(5), "right_foot_instep", "ball", "net_center", f32::NAN));
        assert_eq!(err(&e), AuthoringErrorCode::NonFiniteValue);
    }

    #[test]
    fn an_unknown_joint_is_rejected() {
        let mut m = base_spec();
        m.phase_mut(0).unwrap().push_goal(PoseGoal::set_joint_rotation("no_such_joint", Vec3::ZERO));
        assert_eq!(err(&m), AuthoringErrorCode::UnknownJoint);
    }

    #[test]
    fn an_unknown_effector_is_rejected_across_goals_constraints_contacts_and_events() {
        let mut g = base_spec();
        g.phase_mut(0).unwrap().push_goal(PoseGoal::aim_effector_at_target("ghost", "ball"));
        assert_eq!(err(&g), AuthoringErrorCode::UnknownEffector);

        let mut mv = base_spec();
        mv.phase_mut(0).unwrap().push_goal(PoseGoal::move_effector_toward_target("ghost", "ball", 0.5));
        assert_eq!(err(&mv), AuthoringErrorCode::UnknownEffector);

        let mut c = base_spec();
        c.phase_mut(0).unwrap().push_constraint(Constraint::pin_effector_to_target("ghost", "ball"));
        assert_eq!(err(&c), AuthoringErrorCode::UnknownEffector);

        let mut ct = base_spec();
        ct.phase_mut(0).unwrap().push_contact(ContactDeclaration::new("ghost", "ball"));
        assert_eq!(err(&ct), AuthoringErrorCode::UnknownEffector);

        let mut ev = base_spec();
        ev.add_event(MotionEvent::ball_contact(Tick::new(5), "ghost", "ball", "net_center", 0.5));
        assert_eq!(err(&ev), AuthoringErrorCode::UnknownEffector);
    }

    #[test]
    fn an_unknown_target_is_rejected_across_every_reference_site() {
        let mut aim = base_spec();
        aim.phase_mut(0).unwrap().push_goal(PoseGoal::aim_effector_at_target("right_foot_instep", "ghost"));
        assert_eq!(err(&aim), AuthoringErrorCode::UnknownTarget);

        let mut torso = base_spec();
        torso.phase_mut(0).unwrap().push_goal(PoseGoal::torso_twist_toward_target("ghost", 0.5));
        assert_eq!(err(&torso), AuthoringErrorCode::UnknownTarget);

        let mut strike = base_spec();
        strike.phase_mut(0).unwrap().push_goal(PoseGoal::leg_strike(true, "ghost"));
        assert_eq!(err(&strike), AuthoringErrorCode::UnknownTarget);

        let mut follow = base_spec();
        follow.phase_mut(0).unwrap().push_goal(PoseGoal::follow_through(true, "ghost"));
        assert_eq!(err(&follow), AuthoringErrorCode::UnknownTarget);

        let mut con = base_spec();
        con.phase_mut(0).unwrap().push_constraint(Constraint::keep_gaze_on_target("ghost"));
        assert_eq!(err(&con), AuthoringErrorCode::UnknownTarget);

        let mut ct = base_spec();
        ct.phase_mut(0).unwrap().push_contact(ContactDeclaration::new("left_foot_sole", "ghost"));
        assert_eq!(err(&ct), AuthoringErrorCode::UnknownTarget);

        let mut root = base_spec();
        root.phase_mut(0).unwrap().set_root(RootMotion::move_toward("approach_start", "ball"));
        assert_eq!(err(&root), AuthoringErrorCode::UnknownTarget); // approach_start not declared

        let mut evt = base_spec();
        evt.add_event(MotionEvent::ball_contact(Tick::new(5), "right_foot_instep", "ghost", "net_center", 0.5));
        assert_eq!(err(&evt), AuthoringErrorCode::UnknownTarget);

        let mut evd = base_spec();
        evd.add_event(MotionEvent::ball_contact(Tick::new(5), "right_foot_instep", "ball", "ghost", 0.5));
        assert_eq!(err(&evd), AuthoringErrorCode::UnknownTarget);
    }

    #[test]
    fn side_driven_joint_goals_resolve_against_a_deficient_rig_to_unknown_joint() {
        // A rig missing the derived joints exercises the internal joint-lookup
        // error arms of the raise-arm / torso / leg resolvers.
        let bare = HumanoidRigSpec::empty();
        let mut arm = base_spec();
        arm.phase_mut(0).unwrap().push_goal(PoseGoal::raise_arm_for_balance(true));
        assert_eq!(
            MotionCompiler::compile(&arm, &bare).unwrap_err().code(),
            AuthoringErrorCode::UnknownJoint
        );

        let mut back = base_spec();
        back.phase_mut(0).unwrap().push_goal(PoseGoal::leg_backswing(false, 0.5));
        assert_eq!(
            MotionCompiler::compile(&back, &bare).unwrap_err().code(),
            AuthoringErrorCode::UnknownJoint
        );

        let mut torso = base_spec();
        torso.phase_mut(0).unwrap().push_goal(PoseGoal::torso_twist_toward_target("ball", 0.5));
        assert_eq!(
            MotionCompiler::compile(&torso, &bare).unwrap_err().code(),
            AuthoringErrorCode::UnknownJoint
        );
    }

    #[test]
    fn every_goal_kind_and_constraint_kind_resolves_in_a_full_spec() {
        let mut m = base_spec();
        m.add_target("left_plant_spot", Vec3::new(0.25, 0.0, -0.1));
        {
            let p = m.phase_mut(0).unwrap();
            p.set_ease(EaseCurve::SmoothStep);
            p.set_root(RootMotion::hold());
            p.push_goal(PoseGoal::set_joint_rotation("chest", Vec3::new(0.0, 0.2, 0.0)));
            p.push_goal(PoseGoal::aim_effector_at_target("right_foot_instep", "ball"));
            p.push_goal(PoseGoal::move_effector_toward_target("left_hand", "ball", 0.5));
            p.push_goal(PoseGoal::raise_arm_for_balance(true));
            p.push_goal(PoseGoal::torso_twist_toward_target("net_center", 0.5));
            p.push_goal(PoseGoal::leg_backswing(true, 0.8));
            p.push_goal(PoseGoal::leg_strike(true, "ball"));
            p.push_goal(PoseGoal::follow_through(true, "net_center"));
            p.push_constraint(Constraint::pin_effector_to_target("left_foot_sole", "left_plant_spot"));
            p.push_constraint(Constraint::keep_gaze_on_target("ball"));
            p.push_constraint(Constraint::keep_center_of_mass_over_support("left_foot_sole"));
            p.push_constraint(Constraint::orient_surface_toward_target("right_foot_instep", "net_center"));
            p.push_constraint(Constraint::preserve_foot_contact("left_foot_sole", "left_plant_spot"));
            p.push_contact(ContactDeclaration::new("left_foot_sole", "left_plant_spot"));
        }
        let plan = MotionCompiler::compile(&m, &rig()).unwrap();
        assert_eq!(plan.phases()[0].goals().len(), 8);
        assert_eq!(plan.phases()[0].constraints().len(), 5);
        assert_eq!(plan.phases()[0].contacts().len(), 1);
    }
}
