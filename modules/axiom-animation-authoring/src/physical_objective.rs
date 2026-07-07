//! Physical-control objectives derived from a compiled [`MotionPlan`].
//!
//! These are the **neutral control data** the physics bridge consumes: each is a
//! pure, deterministic function of the plan and the tick (no physics types, no
//! world mutation). The [`crate::AnimationAuthoringApi`] facade exposes them as
//! typed readers; a bridge (feature module) translates them into physics commands.
//!
//! The objective *values* are math/kernel value types (`Vec3`, `Ratio`) and the
//! module's ids, so the bridge can name them even though it cannot name a
//! `MotionPlan`.

use axiom_kernel::Ratio;
use axiom_math::Vec3;

use crate::ids::{EffectorId, JointId};
use crate::motion_plan::MotionPlan;

/// The root's per-tick velocity target: a `MoveToward` phase advances the root
/// from its `from` target to its `to` target, so the target velocity is that
/// displacement spread over the phase's ticks. `Hold`/`Settle` phases (and ticks
/// in no phase) have no root-velocity objective.
pub(crate) fn root_velocity(plan: &MotionPlan, tick: u64) -> Option<Vec3> {
    plan.active_phase(tick).and_then(|p| {
        let from = p.root().start(Vec3::ZERO);
        let to = p.root().end(Vec3::ZERO);
        let delta = to.subtract(from);
        let span = p.end().saturating_sub(p.start()).max(1) as f32;
        (delta.length() > 0.0).then(|| delta.mul_scalar(1.0 / span))
    })
}

/// The active foot-plant objective: the first pinned effector this phase (a
/// contact declaration, else a pinning constraint) and its world target. The
/// bridge holds that effector's body at the target.
pub(crate) fn foot_plant(plan: &MotionPlan, tick: u64) -> Option<(EffectorId, Vec3)> {
    plan.active_phase(tick).and_then(|p| {
        p.contacts()
            .iter()
            .map(|c| c.pin())
            .chain(p.constraints().iter().filter_map(|c| c.pin()))
            .next()
    })
}

/// The active joint-motor objectives: for each joint the active phase drives, its
/// authored Euler target (the explicit orientation for `set_joint_rotation`,
/// `ZERO` for the procedural swings the pose path shapes) and the phase's **drive**
/// scalar (its clamped layer weight), so a `strike` phase outdrives a `backswing`.
pub(crate) fn joint_motors(plan: &MotionPlan, tick: u64) -> Vec<(JointId, Vec3, Ratio)> {
    plan.active_phase(tick)
        .map(|p| {
            let drive = Ratio::finite_or_zero(p.layer_weight());
            p.goals()
                .iter()
                .map(|g| (g.joint(), g.euler(), drive))
                .collect()
        })
        .unwrap_or_default()
}

/// The ball-impulse objective at a `ball_contact` tick: the contact-surface
/// effector, the unit direction from the aim target toward the direction target
/// (`net_center − ball`), and the impulse magnitude (the event's power). The
/// bridge applies this as a real impulse on the ball body — never a teleport.
pub(crate) fn ball_impulse(plan: &MotionPlan, tick: u64) -> Option<(EffectorId, Vec3, Ratio)> {
    plan.events_at(tick)
        .into_iter()
        .find(|e| e.is_ball_contact())
        .and_then(|e| {
            plan.target_position(e.target())
                .zip(plan.target_position(e.direction_target()))
                .map(|(ball, net)| {
                    let delta = net.subtract(ball);
                    let len = delta.length().max(1.0e-6);
                    (e.contact_surface(), delta.mul_scalar(1.0 / len), Ratio::finite_or_zero(e.power()))
                })
        })
}

/// The active gaze objective: the world position a `keep_gaze_on_target`
/// constraint holds the gaze on this phase, if any. A kinematic/pose objective.
pub(crate) fn gaze(plan: &MotionPlan, tick: u64) -> Option<Vec3> {
    plan.active_phase(tick)
        .and_then(|p| p.constraints().iter().find_map(|c| c.gaze_target()))
}

#[cfg(test)]
mod tests {
    //! The objective functions are covered end-to-end through the public facade
    //! readers (`objective_*`), which is exactly how the physics bridge consumes
    //! them.
    use crate::authoring_api::AnimationAuthoringApi;
    use crate::ids::PlanId;
    use axiom_kernel::{Ratio, Tick};
    use axiom_math::Vec3;

    fn penalty(power: f32) -> (AnimationAuthoringApi, PlanId) {
        let mut api = AnimationAuthoringApi::new();
        let m = api.soccer_penalty_kick_v0(Ratio::new(power).unwrap());
        let plan = api.compile(m).unwrap();
        (api, plan)
    }

    #[test]
    fn root_velocity_points_toward_the_ball_during_approach_and_is_none_when_holding() {
        let (api, plan) = penalty(0.7);
        // Approach ([0,12)) is a MoveToward approach_start(z=-3) -> ball(z=0): the
        // root-velocity objective points +Z (toward the ball).
        let v = api.objective_root_velocity(plan, Tick::new(4)).unwrap().unwrap();
        assert!(v.z > 0.0);
        // Plant ([12,20)) holds — no root-velocity objective.
        assert_eq!(api.objective_root_velocity(plan, Tick::new(16)).unwrap(), None);
    }

    #[test]
    fn foot_plant_pins_the_left_foot_during_plant_only() {
        let (api, plan) = penalty(0.7);
        let (_effector, target) = api.objective_foot_plant(plan, Tick::new(16)).unwrap().unwrap();
        assert_eq!(target, Vec3::new(0.25, 0.0, -0.1)); // left_plant_spot
        assert_eq!(api.objective_foot_plant(plan, Tick::new(4)).unwrap(), None);
    }

    #[test]
    fn joint_motors_drive_strike_harder_than_backswing() {
        let (api, plan) = penalty(0.7);
        let backswing = api.objective_joint_motors(plan, Tick::new(26)).unwrap(); // weight 0.6
        let strike = api.objective_joint_motors(plan, Tick::new(38)).unwrap(); // weight 1.0
        assert!(!backswing.is_empty() && !strike.is_empty());
        assert!(strike[0].2.get() > backswing[0].2.get());
        // A tick in no phase yields no motors.
        assert!(api.objective_joint_motors(plan, Tick::new(99)).unwrap().is_empty());
    }

    #[test]
    fn ball_impulse_fires_at_strike_toward_the_net_scaled_by_power() {
        let (api, plan) = penalty(0.7);
        let (_surface, dir, power) = api.objective_ball_impulse(plan, Tick::new(38)).unwrap().unwrap();
        assert!(dir.z > 0.0);
        assert!((dir.length() - 1.0).abs() < 1.0e-4);
        assert!((power.get() - 0.7).abs() < 1.0e-6);
        assert_eq!(api.objective_ball_impulse(plan, Tick::new(10)).unwrap(), None);
    }

    #[test]
    fn active_phase_name_and_target_position_read_through_the_facade() {
        let (api, plan) = penalty(0.7);
        assert_eq!(api.active_phase_name(plan, Tick::new(4)).unwrap().as_deref(), Some("approach"));
        assert_eq!(api.active_phase_name(plan, Tick::new(38)).unwrap().as_deref(), Some("strike"));
        assert_eq!(api.active_phase_name(plan, Tick::new(999)).unwrap(), None);
        assert_eq!(api.plan_target_position(plan, "ball").unwrap(), Some(Vec3::new(0.0, 0.0, 0.0)));
        assert_eq!(api.plan_target_position(plan, "net_center").unwrap(), Some(Vec3::new(0.0, 0.8, 8.0)));
        assert_eq!(api.plan_target_position(plan, "nope").unwrap(), None);
        assert!(api.plan_joint_id(plan, "pelvis").unwrap().is_some());
        assert!(api.plan_joint_id(plan, "nope").unwrap().is_none());
        assert!(api.plan_effector_id(plan, "left_foot_sole").unwrap().is_some());
        assert!(api.plan_effector_id(plan, "nope").unwrap().is_none());
        // Missing-plan errors propagate.
        let ghost = PlanId::from_raw(9);
        assert!(api.plan_joint_id(ghost, "pelvis").is_err());
        assert!(api.plan_effector_id(ghost, "x").is_err());
        assert!(api.active_phase_name(ghost, Tick::new(0)).is_err());
        assert!(api.plan_target_position(ghost, "ball").is_err());
        assert!(api.objective_root_velocity(ghost, Tick::new(0)).is_err());
        assert!(api.objective_foot_plant(ghost, Tick::new(0)).is_err());
        assert!(api.objective_joint_motors(ghost, Tick::new(0)).is_err());
        assert!(api.objective_ball_impulse(ghost, Tick::new(0)).is_err());
        assert!(api.objective_gaze(ghost, Tick::new(0)).is_err());
    }

    #[test]
    fn gaze_targets_the_ball_during_strike_only() {
        let (api, plan) = penalty(0.7);
        assert_eq!(api.objective_gaze(plan, Tick::new(38)).unwrap(), Some(Vec3::new(0.0, 0.0, 0.0)));
        assert_eq!(api.objective_gaze(plan, Tick::new(4)).unwrap(), None);
    }

    #[test]
    fn ball_impulse_direction_is_zero_when_target_and_direction_coincide() {
        // Exercise the degenerate (net == ball) branch of the direction math.
        let mut api = AnimationAuthoringApi::new();
        let rig = api.standard_humanoid();
        let m = api.create_motion("k", Tick::new(10), rig).unwrap();
        api.add_target(m, "spot", Vec3::new(1.0, 2.0, 3.0)).unwrap();
        api.add_phase(m, "p", Tick::new(0), Tick::new(10)).unwrap();
        api.add_ball_contact(m, Tick::new(5), "right_foot_instep", "spot", "spot", Ratio::new(0.5).unwrap())
            .unwrap();
        let plan = api.compile(m).unwrap();
        let (_s, dir, _power) = api.objective_ball_impulse(plan, Tick::new(5)).unwrap().unwrap();
        assert!(dir.length() < 1.0e-3);
    }
}
