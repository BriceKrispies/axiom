//! The built-in `soccer_penalty_kick_v0` motion — a worked example authored
//! entirely with the module's own vocabulary.
//!
//! It is *not* new engine machinery: it is one facade method that registers the
//! standard humanoid rig and an authored [`MotionSpec`] of six phases (approach,
//! plant, backswing, strike, follow-through, recover) with four targets (`ball`,
//! `net_center`, `left_plant_spot`, `approach_start`) and a `ball_contact` event
//! at the strike tick. A game or editor would author its own motions the same way.
//!
//! The spec is built with the internal, infallible builders (all ids are
//! constructed here, so nothing can go "not found"), then registered — which is
//! why this file needs no fallible per-call plumbing.

use axiom_kernel::{Ratio, Tick};
use axiom_math::Vec3;

use crate::authoring_api::AnimationAuthoringApi;
use crate::constraint::Constraint;
use crate::contact::ContactDeclaration;
use crate::ease::EaseCurve;
use crate::ids::{MotionId, RigId};
use crate::motion_event::MotionEvent;
use crate::motion_phase::MotionPhase;
use crate::motion_spec::MotionSpec;
use crate::pose_goal::PoseGoal;
use crate::root_motion::RootMotion;

/// The total motion length, in ticks.
pub const DURATION: u64 = 60;
/// The tick at which the strike connects and the `ball_contact` event fires.
pub const STRIKE_CONTACT_TICK: u64 = 38;

/// A full backswing / balance magnitude.
const FULL: f32 = 1.0;
/// The torso counter-rotation / reach magnitude.
const COUNTER: f32 = 0.6;
/// The trailing-arm reach magnitude on the follow-through.
const REACH: f32 = 0.5;

impl AnimationAuthoringApi {
    /// Author the built-in deterministic soccer penalty kick, returning its
    /// [`MotionId`]. `power` sets the motion's `power` style scalar and the
    /// emitted `ball_contact` power, so changing it changes the strike power
    /// deterministically. A fresh standard humanoid rig is registered for the
    /// motion; the returned motion is ready to [`AnimationAuthoringApi::compile`].
    pub fn soccer_penalty_kick_v0(&mut self, power: Ratio) -> MotionId {
        let rig = self.standard_humanoid();
        self.push_motion(build_penalty_spec(rig, power.get()))
    }
}

/// Build the penalty-kick spec against `rig` at `power` (all builders infallible).
fn build_penalty_spec(rig: RigId, power: f32) -> MotionSpec {
    let mut m = MotionSpec::new("soccer_penalty_kick_v0", Tick::new(DURATION), rig);
    m.add_target("approach_start", Vec3::new(0.0, 0.0, -3.0));
    m.add_target("ball", Vec3::new(0.0, 0.0, 0.0));
    m.add_target("net_center", Vec3::new(0.0, 0.8, 8.0));
    m.add_target("left_plant_spot", Vec3::new(0.25, 0.0, -0.1));
    m.set_style("power", power);

    // approach: a short burst run toward the ball.
    let approach = m.add_phase(MotionPhase::new("approach", Tick::new(0), Tick::new(12)));
    m.phase_mut(approach).into_iter().for_each(|p| {
        p.set_root(RootMotion::move_toward("approach_start", "ball"));
        p.set_ease(EaseCurve::SmoothStep);
    });

    // plant: the left foot pins beside the ball, weight over it.
    let plant = m.add_phase(MotionPhase::new("plant", Tick::new(12), Tick::new(20)));
    m.phase_mut(plant).into_iter().for_each(|p| {
        p.set_root(RootMotion::hold());
        p.push_contact(ContactDeclaration::new("left_foot_sole", "left_plant_spot"));
        p.push_constraint(Constraint::keep_center_of_mass_over_support("left_foot_sole"));
    });

    // backswing: the right leg draws back, arms rise, the torso counter-rotates.
    let backswing = m.add_phase(MotionPhase::new("backswing", Tick::new(20), Tick::new(32)));
    m.phase_mut(backswing).into_iter().for_each(|p| {
        p.set_root(RootMotion::hold());
        p.set_ease(EaseCurve::EaseIn);
        p.set_layer_weight(0.6); // the wind-up drives less hard than the strike
        p.push_goal(PoseGoal::leg_backswing(true, FULL));
        p.push_goal(PoseGoal::raise_arm_for_balance(true));
        p.push_goal(PoseGoal::raise_arm_for_balance(false));
        p.push_goal(PoseGoal::torso_twist_toward_target("approach_start", COUNTER));
        p.push_constraint(Constraint::preserve_foot_contact("left_foot_sole", "left_plant_spot"));
    });

    // strike: the right instep contacts the ball, hip leads knee, torso rotates through.
    let strike = m.add_phase(MotionPhase::new("strike", Tick::new(32), Tick::new(44)));
    m.phase_mut(strike).into_iter().for_each(|p| {
        p.set_root(RootMotion::hold());
        p.set_ease(EaseCurve::EaseOut);
        p.push_goal(PoseGoal::set_joint_rotation("right_hip", Vec3::new(-0.5, 0.0, 0.0)));
        p.push_goal(PoseGoal::leg_strike(true, "ball"));
        p.push_goal(PoseGoal::aim_effector_at_target("right_foot_instep", "ball"));
        p.push_goal(PoseGoal::torso_twist_toward_target("net_center", COUNTER));
        p.push_constraint(Constraint::keep_gaze_on_target("ball"));
    });

    // follow_through: the right leg continues toward the net, arms counterbalance.
    let follow = m.add_phase(MotionPhase::new("follow_through", Tick::new(44), Tick::new(54)));
    m.phase_mut(follow).into_iter().for_each(|p| {
        p.set_root(RootMotion::hold());
        p.set_ease(EaseCurve::EaseOut);
        p.set_layer_weight(0.8); // easing off the strike
        p.push_goal(PoseGoal::follow_through(true, "net_center"));
        p.push_goal(PoseGoal::move_effector_toward_target("left_hand", "net_center", REACH));
    });

    // recover: the body settles.
    let recover = m.add_phase(MotionPhase::new("recover", Tick::new(54), Tick::new(60)));
    m.phase_mut(recover).into_iter().for_each(|p| {
        p.set_root(RootMotion::settle());
        p.set_layer_weight(0.3); // the body settles — the least aggressive drive
    });

    // The strike connects: instep on ball, aimed at the net, at the given power.
    m.add_event(MotionEvent::ball_contact(
        Tick::new(STRIKE_CONTACT_TICK),
        "right_foot_instep",
        "ball",
        "net_center",
        power,
    ));

    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pose_frame::PoseFrame;

    fn ratio(v: f32) -> Ratio {
        Ratio::new(v).unwrap()
    }

    #[test]
    fn the_penalty_kick_authors_and_compiles() {
        let mut api = AnimationAuthoringApi::new();
        let m = api.soccer_penalty_kick_v0(ratio(0.7));
        // Six phases were authored and the whole thing compiles.
        assert!(api.compile(m).is_ok());
    }

    #[test]
    fn the_strike_tick_emits_the_ball_contact_at_the_authored_power() {
        let mut api = AnimationAuthoringApi::new();
        let m = api.soccer_penalty_kick_v0(ratio(0.7));
        let plan = api.compile(m).unwrap();
        let frame = api.sample(plan, Tick::new(STRIKE_CONTACT_TICK)).unwrap();
        let (_, _, _, power) = api.frame_ball_contact(&frame).unwrap();
        assert!((power.get() - 0.7).abs() < 1.0e-6);
        // No ball contact one tick earlier.
        let before = api.sample(plan, Tick::new(STRIKE_CONTACT_TICK - 1)).unwrap();
        assert_eq!(api.frame_ball_contact(&before), None);
    }

    #[test]
    fn style_power_changes_the_ball_contact_power_deterministically() {
        let power_at = |p: f32| {
            let mut api = AnimationAuthoringApi::new();
            let m = api.soccer_penalty_kick_v0(ratio(p));
            let plan = api.compile(m).unwrap();
            let frame: PoseFrame = api.sample(plan, Tick::new(STRIKE_CONTACT_TICK)).unwrap();
            api.frame_ball_contact(&frame).unwrap().3.get()
        };
        assert!((power_at(0.2) - 0.2).abs() < 1.0e-6);
        assert!((power_at(0.9) - 0.9).abs() < 1.0e-6);
        assert!(power_at(0.2) < power_at(0.9));
    }
}
