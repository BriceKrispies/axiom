//! The kicker: a short run-up, a plant, and a strike whose boot is on the ball
//! at exactly the tick the ball leaves.
//!
//! The whole sequence is a pure function of `(plan, tick)` — no state, no clock
//! — so the launch tick and the animation cannot drift apart. That the boot
//! actually arrives at the ball on the launch tick is not a hope about the
//! numbers: it is asserted here, by resolving the figure and measuring where the
//! boot is.
//!
//! The striking foot is the part on the **world `+X` side** while the kicker
//! faces the goal, and the kicker stands with that hip lined up on the ball, so
//! the leg's natural fore-and-aft swing plane passes through it. Nothing has to
//! reach sideways for the ball, which is what keeps the strike readable rather
//! than rubbery.

use axiom::prelude::Vec3;
use axiom_math::Quat;

use crate::tuning::KickTuning;

use super::model::{
    hip_half_width, L_FOOT, L_FOREARM, L_SHIN, L_THIGH, L_UPPER_ARM, R_FOOT, R_FOREARM, R_SHIN,
    R_THIGH, R_UPPER_ARM, TORSO,
};
use super::pose::{crouch, idle, qx, qy, run_gait, smooth, JointPose};

/// The index of the boot that strikes the ball.
pub const STRIKE_FOOT: usize = L_FOOT;

/// Tick the striking leg starts coming through, and how many ticks the swing
/// spans. Chosen so the boot crosses the ball exactly on
/// [`KickTuning::contact`] — see `the_boot_is_on_the_ball_on_the_contact_tick`.
const SWING_START: f32 = 17.0;
const SWING_SPAN: f32 = 14.0;
/// Where the leg is cocked to, and where it finishes, radians about the hip.
/// Positive is behind the body.
const COCK_ANGLE: f32 = 0.90;
const THROUGH_ANGLE: f32 = -1.35;
/// Metres per full stride in the run-up.
const STRIDE: f32 = 1.55;

/// Where the kicker starts, where it plants, and which way it runs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KickPlan {
    /// Where the run-up begins.
    pub start: Vec3,
    /// Where the body root stands at the moment of the strike.
    pub stand: Vec3,
    /// The ball being struck.
    pub ball: Vec3,
}

impl KickPlan {
    /// Build the run-up for a shot. `bend` is the signed bend effort in `-1..1`;
    /// a shot the player has bent hard is approached from a wider angle, which
    /// is the only place the authored shape shows up before the ball moves.
    pub fn for_shot(ball: Vec3, bend: f32, tuning: &KickTuning) -> KickPlan {
        // The striking hip sits half a pelvis to the kicker's own left, which
        // maps to world `-X` of the body root while it faces the goal. Standing
        // that far the other way puts the boot's swing plane over the ball.
        let stand = Vec3::new(ball.x - hip_half_width(), 0.0, ball.z + 0.20);
        let widen = tuning.approach_side + tuning.approach_bend_widen * bend.abs().min(1.0);
        KickPlan {
            start: Vec3::new(stand.x - widen, 0.0, stand.z + tuning.approach_back),
            stand,
            ball,
        }
    }

    /// Where the kicker waits while the player is still drawing the shot.
    pub fn waiting(&self) -> (Vec3, f32, JointPose) {
        let facing = facing_toward(self.start, self.ball);
        (self.start, facing, idle())
    }
}

/// Yaw that points a figure from `from` toward `to`.
fn facing_toward(from: Vec3, to: Vec3) -> f32 {
    let d = to.subtract(from);
    d.x.atan2(d.z)
}

/// The kicker's ground position, facing and pose at `tick` ticks into the kick.
///
/// Valid for every tick, including after the strike: the follow-through simply
/// keeps playing while the ball is in the air.
pub fn kick_frame(plan: &KickPlan, tick: u32, tuning: &KickTuning) -> (Vec3, f32, JointPose) {
    let t = tick as f32;
    let plant = tuning.plant.max(1) as f32;

    // Travel: an ease into the plant, then held.
    let approach = smooth((t / plant).min(1.0));
    let ground = plan
        .start
        .add(plan.stand.subtract(plan.start).mul_scalar(approach));
    let travelled = plan.start.subtract(ground).length();
    let facing = facing_toward(plan.start, plan.ball);

    // Stage 1: the run-up, distance-driven so the legs slow as the body does.
    let running = run_gait(travelled, STRIDE, 0.72);

    // Stage 2: the planted strike. The support leg takes the weight and the
    // striking leg sweeps from cocked to through.
    let swing = smooth(((t - SWING_START) / SWING_SPAN).clamp(0.0, 1.0));
    let leg = COCK_ANGLE + (THROUGH_ANGLE - COCK_ANGLE) * swing;
    let knee_open = smooth(((t - SWING_START) / (SWING_SPAN * 0.55)).clamp(0.0, 1.0));
    let mut struck = JointPose::neutral();
    crouch(&mut struck, 0.34);
    // Striking leg (the world +X side while facing the goal).
    struck.joints[L_THIGH] = qx(leg);
    struck.joints[L_SHIN] = qx(0.95 * (1.0 - knee_open));
    struck.joints[L_FOOT] = qx(-0.16 - 0.24 * swing);
    // Support leg: planted, knee soft, foot flat.
    struck.joints[R_THIGH] = qx(-0.10);
    struck.joints[R_SHIN] = qx(0.34);
    struck.joints[R_FOOT] = qx(-0.22);
    // The upper body counter-rotates into the strike; the off arm flies out for
    // balance, which is the single most legible thing about a struck ball.
    struck.joints[TORSO] = qy(0.30 - 0.62 * swing);
    struck.joints[R_UPPER_ARM] =
        Quat::from_euler_xyz(-0.55 - 0.95 * swing, 0.0, 0.85 + 0.40 * swing);
    struck.joints[R_FOREARM] = qx(-0.42);
    struck.joints[L_UPPER_ARM] = Quat::from_euler_xyz(0.30 + 0.55 * swing, 0.0, -0.55);
    struck.joints[L_FOREARM] = qx(-0.30);
    struck.root_pitch = 0.14 + 0.22 * swing;
    struck.root_roll = -0.10 * swing;

    // Stage 3: the settle after the follow-through, back toward a standing idle.
    let settle_start = tuning.contact as f32 + tuning.follow_through as f32;
    let settle = smooth(((t - settle_start) / 22.0).clamp(0.0, 1.0));

    // Blend run-up → strike over the last few ticks before the plant, so neither
    // stage has to know the other exists.
    let into_strike = smooth(((t - (SWING_START - 4.0)) / 7.0).clamp(0.0, 1.0));
    let action = JointPose::blend(&running, &struck, into_strike);
    (
        ground,
        facing,
        JointPose::blend(&action, &idle(), settle),
    )
}

/// The signed lateral offset of the boot from the body root at full extension —
/// exported so the debug view can draw the swing plane.
pub fn strike_side() -> f32 {
    hip_half_width()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::figure::model::soccer_figure;
    use crate::figure::rig::{body_transform, world_parts};
    use crate::pitch::ball_spot;
    use crate::tuning::Tuning;

    fn boot_at(plan: &KickPlan, tick: u32, tuning: &KickTuning) -> Vec3 {
        let figure = soccer_figure();
        let (ground, facing, pose) = kick_frame(plan, tick, tuning);
        let parts = world_parts(&figure, body_transform(ground, facing, &pose), &pose);
        parts[STRIKE_FOOT].transform.translation
    }

    #[test]
    fn the_boot_is_on_the_ball_on_the_contact_tick() {
        let tuning = Tuning::DEFAULT;
        let ball = ball_spot(tuning.flight.ball_radius);
        let plan = KickPlan::for_shot(ball, 0.0, &tuning.kick);
        // Of every tick in the kick, the one where the boot is nearest the ball
        // is the tick the ball is launched on.
        let nearest = (0..=(tuning.kick.contact + 10))
            .map(|t| (t, boot_at(&plan, t, &tuning.kick).subtract(ball).length()))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(core::cmp::Ordering::Equal))
            .expect("the kick has ticks");
        assert!(
            nearest.0.abs_diff(tuning.kick.contact) <= 1,
            "the boot is nearest the ball on tick {} but the ball leaves on {}",
            nearest.0,
            tuning.kick.contact
        );
        // ... and it is genuinely touching, not merely closest.
        let gap = boot_at(&plan, tuning.kick.contact, &tuning.kick)
            .subtract(ball)
            .length();
        assert!(
            gap < tuning.flight.ball_radius + 0.14,
            "the boot misses the ball by {gap} m"
        );
    }

    #[test]
    fn the_run_up_starts_away_from_the_ball_and_finishes_beside_it() {
        let tuning = Tuning::DEFAULT;
        let ball = ball_spot(tuning.flight.ball_radius);
        let plan = KickPlan::for_shot(ball, 0.0, &tuning.kick);
        assert!(plan.start.subtract(ball).length() > 2.5);
        assert!(plan.stand.subtract(ball).length() < 0.6);
        let (start_ground, _, _) = kick_frame(&plan, 0, &tuning.kick);
        assert_eq!(start_ground, plan.start);
        let (plant_ground, _, _) = kick_frame(&plan, tuning.kick.plant, &tuning.kick);
        assert!(plant_ground.subtract(plan.stand).length() < 1.0e-4);
        // The kicker faces the goal, not the camera.
        let (_, facing, _) = kick_frame(&plan, 0, &tuning.kick);
        assert!(facing.abs() > 2.0, "facing {facing} should be near ±π");
    }

    #[test]
    fn a_bent_shot_is_approached_from_a_wider_angle() {
        let tuning = Tuning::DEFAULT;
        let ball = ball_spot(tuning.flight.ball_radius);
        let straight = KickPlan::for_shot(ball, 0.0, &tuning.kick);
        let curled = KickPlan::for_shot(ball, 1.0, &tuning.kick);
        assert!(curled.start.x < straight.start.x);
        // Bending the other way widens it just the same.
        assert_eq!(
            KickPlan::for_shot(ball, -1.0, &tuning.kick).start,
            curled.start
        );
        assert!(strike_side() > 0.0);
    }

    #[test]
    fn the_kicker_waits_at_the_top_of_the_run_up_and_settles_afterwards() {
        let tuning = Tuning::DEFAULT;
        let ball = ball_spot(tuning.flight.ball_radius);
        let plan = KickPlan::for_shot(ball, 0.0, &tuning.kick);
        let (ground, facing, _) = plan.waiting();
        assert_eq!(ground, plan.start);
        assert!(facing.abs() > 2.0);
        // Long after the follow-through the pose has returned to a standing idle.
        let (_, _, late) = kick_frame(&plan, 200, &tuning.kick);
        assert!((late.root_pitch - idle().root_pitch).abs() < 1.0e-3);
    }
}
