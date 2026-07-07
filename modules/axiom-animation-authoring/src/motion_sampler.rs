//! [`MotionSampler`] — samples a [`MotionPlan`] at a tick into a [`PoseFrame`].
//!
//! Sampling is a pure function of the plan and the tick — forward kinematics plus
//! arithmetic goal application — so the same plan sampled at the same tick yields
//! an identical frame. The pipeline at tick `t`:
//!
//! 1. **Root** — fold the completed phases to find where the root has travelled,
//!    then interpolate the active phase's root motion.
//! 2. **Locals** — start from the rig bind pose and apply the active phase's pose
//!    goals at their eased, weight-scaled strength.
//! 3. **Forward kinematics** — compose local transforms down the hierarchy in one
//!    forward pass to world transforms.
//! 4. **Effectors** — offset each effector from its joint's world, then override
//!    any effector pinned by an active pin constraint or contact.
//! 5. **Records** — attach the active constraints, contacts, and firing events.
//!
//! Per-kind goal application is dispatched through a `const` fn-pointer table
//! indexed by the goal discriminant — a table lookup, never a `match`.

use axiom_kernel::Tick;
use axiom_math::{Quat, Transform, Vec3};

use crate::humanoid_rig::HumanoidRigSpec;
use crate::ids::EffectorId;
use crate::motion_plan::MotionPlan;
use crate::motion_phase::ResolvedPhase;
use crate::pose_frame::PoseFrame;
use crate::pose_goal::ResolvedGoal;

/// Maximum thigh angle (radians) a full-strength backswing draws the leg back.
const BACKSWING_MAX: f32 = 0.9;
/// Thigh angle a full-strength strike swings the leg forward.
const STRIKE_ANGLE: f32 = 0.9;
/// Thigh angle a full-strength follow-through carries the leg past the strike.
const FOLLOW_ANGLE: f32 = 1.3;
/// Shoulder angle a full-strength balance raise lifts the arm.
const RAISE_ANGLE: f32 = 0.6;
/// Maximum torso yaw a full-strength twist applies.
const TWIST_MAX: f32 = 0.6;
/// How far, per unit amount/strength, a move-toward goal shifts a joint.
const MOVE_SCALE: f32 = 0.5;

/// The deterministic motion sampler.
#[derive(Debug)]
pub struct MotionSampler;

impl MotionSampler {
    /// Sample `plan` at `tick`.
    pub(crate) fn sample(plan: &MotionPlan, tick: Tick) -> PoseFrame {
        let t = tick.raw();
        let rig = plan.rig();
        let active = plan.active_phase(t);

        let root_pos = root_position(plan, t);
        let root = Transform::from_translation(root_pos);

        let bind: Vec<Transform> = rig.joints().iter().map(|j| j.bind_local()).collect();
        let locals = active
            .map(|p| apply_goals(&bind, p, t, root_pos))
            .unwrap_or(bind);

        let worlds = forward_kinematics(rig, &locals, root);
        let pins = active_pins(active);
        let effector_worlds = effector_worlds(rig, &worlds, &pins);

        let constraints = active.map(|p| p.constraints().to_vec()).unwrap_or_default();
        let contacts = active.map(|p| p.contacts().to_vec()).unwrap_or_default();
        let events = plan.events_at(t);

        PoseFrame::new(root, locals, worlds, effector_worlds, constraints, contacts, events)
    }
}

/// Linear interpolation between two positions.
fn lerp(a: Vec3, b: Vec3, u: f32) -> Vec3 {
    a.add(b.subtract(a).mul_scalar(u))
}

/// The root position at tick `t`: fold the phases already completed to carry the
/// running root forward, then interpolate the active phase (or hold the carried
/// position when `t` sits in no phase).
fn root_position(plan: &MotionPlan, t: u64) -> Vec3 {
    let running = plan
        .phases()
        .iter()
        .filter(|p| p.end() <= t)
        .fold(Vec3::ZERO, |carry, p| p.root().end(carry));
    plan.active_phase(t)
        .map(|p| {
            let u = p.eased_progress(t);
            lerp(p.root().start(running), p.root().end(running), u)
        })
        .unwrap_or(running)
}

/// A per-kind goal applier. Receives the goal, the working locals, the phase's
/// eased+weighted `strength` (the magnitude most goals scale by), the root world
/// position, and the phase's **raw** linear `progress` (the even clock a gait
/// cycles on — see [`apply_run_cycle`]).
type GoalApplier = fn(&ResolvedGoal, &mut [Transform], f32, Vec3, f32);

fn apply_set_joint(g: &ResolvedGoal, locals: &mut [Transform], s: f32, _root: Vec3, _p: f32) {
    let e = g.euler();
    locals[g.joint().raw() as usize].rotation = Quat::from_euler_xyz(e.x * s, e.y * s, e.z * s);
}

fn apply_aim(g: &ResolvedGoal, locals: &mut [Transform], s: f32, root: Vec3, _p: f32) {
    let d = g.target().subtract(root);
    let yaw = d.x.atan2(d.z);
    locals[g.joint().raw() as usize].rotation = Quat::from_euler_xyz(0.0, yaw * s, 0.0);
}

fn apply_move(g: &ResolvedGoal, locals: &mut [Transform], s: f32, root: Vec3, _p: f32) {
    let j = g.joint().raw() as usize;
    let shift = g.target().subtract(root).mul_scalar(g.amount() * s * MOVE_SCALE);
    locals[j].translation = locals[j].translation.add(shift);
}

fn apply_raise_arm(g: &ResolvedGoal, locals: &mut [Transform], s: f32, _root: Vec3, _p: f32) {
    locals[g.joint().raw() as usize].rotation = Quat::from_euler_xyz(0.0, 0.0, RAISE_ANGLE * s);
}

fn apply_torso_twist(g: &ResolvedGoal, locals: &mut [Transform], s: f32, _root: Vec3, _p: f32) {
    locals[g.joint().raw() as usize].rotation =
        Quat::from_euler_xyz(0.0, TWIST_MAX * g.amount() * s, 0.0);
}

fn apply_leg_backswing(g: &ResolvedGoal, locals: &mut [Transform], s: f32, _root: Vec3, _p: f32) {
    // Positive X thigh rotation swings the foot to -Z: behind the body.
    locals[g.joint().raw() as usize].rotation =
        Quat::from_euler_xyz(BACKSWING_MAX * g.amount() * s, 0.0, 0.0);
}

fn apply_leg_strike(g: &ResolvedGoal, locals: &mut [Transform], s: f32, _root: Vec3, _p: f32) {
    // Negative X thigh rotation swings the foot to +Z: forward through the ball.
    locals[g.joint().raw() as usize].rotation = Quat::from_euler_xyz(-STRIKE_ANGLE * s, 0.0, 0.0);
}

fn apply_follow_through(g: &ResolvedGoal, locals: &mut [Transform], s: f32, _root: Vec3, _p: f32) {
    locals[g.joint().raw() as usize].rotation = Quat::from_euler_xyz(-FOLLOW_ANGLE * s, 0.0, 0.0);
}

/// A locomotion oscillator: the joint's fore/aft (X) angle is
/// `bias + amplitude·sin(TAU·steps·progress + phase_offset)`, cycling on the phase's
/// **raw** progress (not `strength`), so a stride is uniform across the phase. The
/// three cycle parameters ride in `euler = (phase_offset, steps, bias)`; `amount`
/// carries the amplitude.
fn apply_run_cycle(g: &ResolvedGoal, locals: &mut [Transform], _s: f32, _root: Vec3, progress: f32) {
    let e = g.euler();
    let angle = e.z + g.amount() * (core::f32::consts::TAU * e.y * progress + e.x).sin();
    // Rotate `angle` about the canonical unit axis carried in the resolved `target`
    // (`+X` fore/aft swing, `+Z` lateral abduction).
    let a = g.target();
    locals[g.joint().raw() as usize].rotation = Quat::from_euler_xyz(a.x * angle, a.y * angle, a.z * angle);
}

/// Per-kind goal appliers, indexed by the goal discriminant.
const GOAL_APPLIERS: [GoalApplier; 9] = [
    apply_set_joint,
    apply_aim,
    apply_move,
    apply_raise_arm,
    apply_torso_twist,
    apply_leg_backswing,
    apply_leg_strike,
    apply_follow_through,
    apply_run_cycle,
];

/// Apply an active phase's goals to a copy of the bind pose. Each goal is applied at
/// the phase's eased+weighted `strength`; a gait goal instead reads the raw
/// `progress` its applier is also handed.
fn apply_goals(bind: &[Transform], phase: &ResolvedPhase, t: u64, root: Vec3) -> Vec<Transform> {
    let s = phase.strength(t);
    let progress = phase.progress(t);
    phase.goals().iter().fold(bind.to_vec(), |mut locals, g| {
        GOAL_APPLIERS[g.kind() as usize](g, &mut locals, s, root, progress);
        locals
    })
}

/// Compose local transforms down the joint hierarchy into world transforms in one
/// forward pass (each parent has a smaller index, guaranteed by the rig).
fn forward_kinematics(rig: &HumanoidRigSpec, locals: &[Transform], root: Transform) -> Vec<Transform> {
    let joints = rig.joints();
    (0..joints.len()).fold(Vec::with_capacity(joints.len()), |mut worlds, i| {
        let parent = joints[i]
            .parent()
            .map(|p| worlds[p.raw() as usize])
            .unwrap_or(root);
        worlds.push(Transform::combine(parent, locals[i]));
        worlds
    })
}

/// The `(effector, target)` pins an active phase imposes: pinning constraints plus
/// every contact.
fn active_pins(active: Option<&ResolvedPhase>) -> Vec<(EffectorId, Vec3)> {
    active
        .map(|p| {
            p.constraints()
                .iter()
                .filter_map(|c| c.pin())
                .chain(p.contacts().iter().map(|c| c.pin()))
                .collect()
        })
        .unwrap_or_default()
}

/// The world transform of every effector: its joint world composed with its
/// offset, then overridden to a pin target if pinned.
fn effector_worlds(
    rig: &HumanoidRigSpec,
    worlds: &[Transform],
    pins: &[(EffectorId, Vec3)],
) -> Vec<Transform> {
    rig.effectors()
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let base = Transform::combine(worlds[e.joint().raw() as usize], e.offset());
            let id = EffectorId::from_raw(i as u64);
            pins.iter()
                .find(|(pe, _)| *pe == id)
                .map(|(_, target)| Transform {
                    translation: *target,
                    ..base
                })
                .unwrap_or(base)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraint::Constraint;
    use crate::contact::ContactDeclaration;
    use crate::ids::RigId;
    use crate::motion_compiler::MotionCompiler;
    use crate::motion_event::MotionEvent;
    use crate::motion_phase::MotionPhase;
    use crate::motion_spec::MotionSpec;
    use crate::pose_goal::PoseGoal;
    use crate::root_motion::RootMotion;

    fn rig() -> HumanoidRigSpec {
        HumanoidRigSpec::standard_humanoid()
    }

    /// A plan whose single phase authors one of every goal kind plus a pin and a
    /// contact, with a MoveToward root over `[0, 10)`.
    fn full_plan() -> MotionPlan {
        let mut m = MotionSpec::new("m", Tick::new(10), RigId::from_raw(0));
        m.add_target("approach_start", Vec3::new(0.0, 0.0, -3.0));
        m.add_target("ball", Vec3::new(0.0, 0.0, 0.0));
        m.add_target("net_center", Vec3::new(0.0, 0.8, 8.0));
        m.add_target("left_plant_spot", Vec3::new(0.25, 0.0, -0.1));
        m.add_phase(MotionPhase::new("p", Tick::new(0), Tick::new(10)));
        {
            let p = m.phase_mut(0).unwrap();
            p.set_root(RootMotion::move_toward("approach_start", "ball"));
            p.push_goal(PoseGoal::set_joint_rotation("chest", Vec3::new(0.0, 0.2, 0.0)));
            p.push_goal(PoseGoal::aim_effector_at_target("right_foot_instep", "ball"));
            p.push_goal(PoseGoal::move_effector_toward_target("left_hand", "ball", 0.5));
            p.push_goal(PoseGoal::raise_arm_for_balance(true));
            p.push_goal(PoseGoal::torso_twist_toward_target("net_center", 0.5));
            p.push_goal(PoseGoal::leg_backswing(true, 0.8));
            p.push_goal(PoseGoal::leg_strike(false, "ball"));
            p.push_goal(PoseGoal::follow_through(false, "net_center"));
            p.push_goal(PoseGoal::run_cycle("left_shin", 0.5, 0.0, 2.0, 0.1, Vec3::new(1.0, 0.0, 0.0)));
            p.push_constraint(Constraint::pin_effector_to_target("left_foot_sole", "left_plant_spot"));
            p.push_contact(ContactDeclaration::new("right_foot_sole", "ball"));
        }
        m.add_event(MotionEvent::named(Tick::new(5), "cue"));
        MotionCompiler::compile(&m, &rig()).unwrap()
    }

    #[test]
    fn sampling_is_replayable() {
        let plan = full_plan();
        assert_eq!(MotionSampler::sample(&plan, Tick::new(5)), MotionSampler::sample(&plan, Tick::new(5)));
    }

    #[test]
    fn every_goal_applier_runs_and_shifts_the_pose_off_bind() {
        let plan = full_plan();
        let frame = MotionSampler::sample(&plan, Tick::new(6));
        // Representative joints each moved off the bind pose: chest (twist/set), a thigh (strike),
        // the other thigh (backswing), an upper arm (raise), a hand (move).
        let joint = |name: &str| plan.rig().joint_id(name).unwrap();
        assert_ne!(frame.joint_local(joint("chest")).unwrap(), Transform::IDENTITY);
        assert_ne!(
            frame.joint_local(joint("right_thigh")).unwrap().rotation,
            Quat::IDENTITY
        );
        assert_ne!(
            frame.joint_local(joint("left_thigh")).unwrap().rotation,
            Quat::IDENTITY
        );
        assert_ne!(
            frame.joint_local(joint("right_upper_arm")).unwrap().rotation,
            Quat::IDENTITY
        );
        let left_hand_bind = plan.rig().joints()[joint("left_hand").raw() as usize].bind_local().translation;
        assert_ne!(frame.joint_local(joint("left_hand")).unwrap().translation, left_hand_bind);
    }

    #[test]
    fn a_run_cycle_oscillates_its_joint_over_raw_progress() {
        // The left shin carries a RunCycle (steps=2 over [0,10)); its fore/aft angle
        // is a sine of raw progress, so it reads different rotations at progresses
        // that map to different points on the cycle — proof the gait actually moves,
        // unlike a single interpolated pose.
        let plan = full_plan();
        let shin = plan.rig().joint_id("left_shin").unwrap();
        let at = |t| MotionSampler::sample(&plan, Tick::new(t)).joint_local(shin).unwrap().rotation;
        // progress 0.1 vs 0.3 -> sin(0.4π) vs sin(1.2π): opposite signs, distinct poses.
        assert_ne!(at(1), at(3));
        // The oscillation is off the bind identity at a non-zero point of the cycle.
        assert_ne!(at(1), Quat::IDENTITY);
    }

    #[test]
    fn root_moves_toward_the_target_over_a_move_phase() {
        let plan = full_plan();
        let ball = Vec3::new(0.0, 0.0, 0.0);
        let start = Vec3::new(0.0, 0.0, -3.0);
        let early = MotionSampler::sample(&plan, Tick::new(1)).root().translation;
        let late = MotionSampler::sample(&plan, Tick::new(9)).root().translation;
        // Progress moves the root from approach_start toward the ball.
        assert!(late.distance(ball) < early.distance(ball));
        assert!(early.distance(ball) < start.distance(ball));
    }

    #[test]
    fn a_pin_constraint_and_a_contact_override_their_effector_worlds() {
        let plan = full_plan();
        let frame = MotionSampler::sample(&plan, Tick::new(5));
        let eff = |name: &str| plan.rig().effector_id(name).unwrap();
        // The pinned left foot sits exactly on the plant spot.
        assert_eq!(
            frame.effector_world(eff("left_foot_sole")).unwrap().translation,
            Vec3::new(0.25, 0.0, -0.1)
        );
        // The contact right foot sits exactly on the ball.
        assert_eq!(
            frame.effector_world(eff("right_foot_sole")).unwrap().translation,
            Vec3::new(0.0, 0.0, 0.0)
        );
        // An un-pinned effector is not at either target.
        assert_ne!(
            frame.effector_world(eff("head_gaze")).unwrap().translation,
            Vec3::new(0.25, 0.0, -0.1)
        );
    }

    #[test]
    fn a_tick_in_no_phase_holds_the_carried_root_and_empty_records() {
        // A plan with a single [0, 5) phase; sampling at 8 sits past every phase.
        let mut m = MotionSpec::new("m", Tick::new(10), RigId::from_raw(0));
        m.add_target("approach_start", Vec3::new(0.0, 0.0, -3.0));
        m.add_target("ball", Vec3::new(0.0, 0.0, 2.0));
        m.add_phase(MotionPhase::new("p", Tick::new(0), Tick::new(5)));
        m.phase_mut(0).unwrap().set_root(RootMotion::move_toward("approach_start", "ball"));
        let plan = MotionCompiler::compile(&m, &rig()).unwrap();
        let frame = MotionSampler::sample(&plan, Tick::new(8));
        // Root holds at the completed move's destination (the ball).
        assert_eq!(frame.root().translation, Vec3::new(0.0, 0.0, 2.0));
        assert!(frame.active_constraints().is_empty());
        assert!(frame.active_contacts().is_empty());
        // Locals are the bind pose (no active goals): a representative joint matches
        // its rig bind local.
        let pelvis = plan.rig().joint_id("pelvis").unwrap();
        let bind_pelvis = plan.rig().joints()[pelvis.raw() as usize].bind_local();
        assert_eq!(frame.joint_local(pelvis).unwrap(), bind_pelvis);
    }

    #[test]
    fn events_fire_only_on_their_exact_tick() {
        let plan = full_plan();
        assert_eq!(MotionSampler::sample(&plan, Tick::new(5)).event_names(), vec!["cue"]);
        assert!(MotionSampler::sample(&plan, Tick::new(4)).event_names().is_empty());
    }
}
