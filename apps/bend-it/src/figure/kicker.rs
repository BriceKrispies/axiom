//! The kicker: a run-up, a plant, and a leg thrown at the ball.
//!
//! Nothing here schedules the contact. The leg is a driven pendulum
//! ([`super::strike`]) and the joints that put the boot where the swing says it
//! is are *solved* ([`super::ik`]) rather than posed. So the whole body is free
//! to change — a wider plant for a bent shot, a deeper lean for a lofted one, a
//! run-up that arrives faster for a hard one — and the boot still meets the ball,
//! because meeting the ball is a geometric fact about where the leg is rather
//! than a number that was tuned until it looked right.
//!
//! The one thing that has to be nailed down for that to work is the **frame the
//! swing is solved in**. The strike's root offsets (lean, roll, lift) are a
//! function of the drive alone and hold constant through the swing, so the hip is
//! a fixed point the arc can be measured from; everything that moves during the
//! strike moves *below* that root. The alternative — a lean that grows through
//! the follow-through — would move the hip out from under the arc mid-swing and
//! put the boot through the ball rather than on it.
//!
//! The striking foot is the part on the **world `+X` side** while the kicker
//! faces the goal.

use axiom::prelude::Vec3;
use axiom_math::{Quat, Transform};

use crate::tuning::{KickTuning, DT};

use super::ik;
use super::model::{
    hip_half_width, L_FOOT, L_FOREARM, L_SHIN, L_THIGH, L_UPPER_ARM, PARTS, PELVIS, R_FOOT,
    R_FOREARM, R_SHIN, R_THIGH, R_UPPER_ARM, TORSO,
};
use super::pose::{idle, qx, qy, run_gait, smooth, JointPose};
use super::rig::body_transform;
use super::strike::{KickDrive, Swing};

/// The index of the boot that strikes the ball.
pub const STRIKE_FOOT: usize = L_FOOT;
/// Metres per full stride in the run-up.
const STRIDE: f32 = 1.55;
/// Where the ankle has to be for the boot *box* to be on the ball: a little
/// above it and a little behind it, in the figure's own forward-`+Z` frame.
const BOOT_CONTACT: Vec3 = Vec3::new(0.0, 0.03, -0.11);
/// Ticks after the plant lands before the leg is released.
const RELEASE_LAG: f32 = 2.0;

/// Where the kicker starts, where it plants, and what the body is being asked
/// for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KickPlan {
    /// Where the run-up begins.
    pub start: Vec3,
    /// Where the body root stands at the moment of the strike.
    pub stand: Vec3,
    /// Where the plant foot is put, and stays.
    pub plant: Vec3,
    /// The ball being struck.
    pub ball: Vec3,
    /// What the drawing asked the body for.
    pub drive: KickDrive,
}

impl KickPlan {
    /// Build the run-up for a shot.
    pub fn for_shot(ball: Vec3, drive: KickDrive, tuning: &KickTuning) -> KickPlan {
        // The striking hip lines up on the ball, so the leg's own swing plane
        // passes through it and nothing has to reach across for the strike.
        let stand = Vec3::new(
            ball.x - hip_half_width(),
            0.0,
            ball.z + drive.plant_back + 0.14,
        );
        // The plant foot is placed in the world and stays there: the support leg
        // is solved onto it every tick, so the body leans and turns over a foot
        // that is genuinely planted rather than sliding about under it.
        let plant = Vec3::new(ball.x + drive.plant_side, 0.0, ball.z + drive.plant_back);
        let widen = tuning.approach_side + tuning.approach_bend_widen * drive.across.abs().min(1.0);
        KickPlan {
            start: Vec3::new(stand.x - widen, 0.0, stand.z + tuning.approach_back),
            stand,
            plant,
            ball,
            drive,
        }
    }

    /// Which way the kicker faces: down the run-up, at the ball.
    pub fn facing(&self) -> f32 {
        let d = self.ball.subtract(self.start);
        d.x.atan2(d.z)
    }

    /// Where the kicker waits while the player is still drawing.
    pub fn waiting(&self) -> (Vec3, f32, JointPose) {
        (self.start, self.facing(), idle())
    }

    /// How many ticks the run-up takes at this drive's approach speed.
    ///
    /// The run-up is a distance at a speed, so a hard shot is *arrived at*
    /// faster as well as struck harder — the whole kick is quicker, which is the
    /// most legible thing about a rushed penalty.
    pub fn run_up_ticks(&self, tuning: &KickTuning) -> u32 {
        let distance = self.start.subtract(self.stand).length();
        (((distance / self.drive.approach.max(0.5)) / DT) as u32).max(tuning.plant)
    }

    /// The tick the leg is released, counted from the start of the run-up.
    pub fn release_tick(&self, tuning: &KickTuning) -> u32 {
        self.run_up_ticks(tuning) + RELEASE_LAG as u32
    }

    /// The strike's root offsets — the frame the swing is solved in.
    ///
    /// Constant through the swing on purpose (see the module docs): the body's
    /// commitment is decided by the drawing before the leg moves, not accumulated
    /// as the leg comes through.
    pub fn strike_root(&self) -> JointPose {
        let mut pose = JointPose::neutral();
        pose.root_pitch = 0.13 - self.drive.lean;
        pose.root_roll = -self.drive.across * 0.22;
        pose.root_lift = -0.07;
        pose.root_lateral = -0.03;
        pose
    }

    /// The body transform the strike holds.
    fn strike_body(&self) -> Transform {
        body_transform(self.stand, self.facing(), &self.strike_root())
    }

    /// The swing arc, in the striking hip's own frame: how far to the side the
    /// ball sits, how far out along the swing plane it is, and the swing angle at
    /// which the boot is on it.
    ///
    /// This is measured off the planted body, so a shot struck from a wider or
    /// deeper plant simply has a different contact angle — and the physics that
    /// reaches that angle is the same physics either way.
    pub fn swing_arc(&self) -> (f32, f32, f32) {
        let to_ball = self
            .strike_body()
            .inverse()
            .map(|frame| frame.transform_point(self.ball))
            .unwrap_or(Vec3::ZERO)
            .subtract(PARTS[PELVIS].offset)
            .add(BOOT_CONTACT)
            .subtract(PARTS[L_THIGH].offset);
        (
            to_ball.x,
            (to_ball.y * to_ball.y + to_ball.z * to_ball.z).sqrt(),
            (-to_ball.z).atan2(-to_ball.y),
        )
    }

    /// The swing angle at which the ball is struck.
    pub fn contact_angle(&self) -> f32 {
        self.swing_arc().2
    }

    /// Where the ankle is at swing angle `angle`, `extension` of the way to full
    /// stretch — in the pelvis's frame, which is where the leg solve lives.
    ///
    /// At `extension == 1` and `angle == contact_angle` this is exactly the ball.
    fn boot_target(&self, angle: f32, extension: f32) -> Vec3 {
        let (lateral, radius, _) = self.swing_arc();
        PARTS[L_THIGH].offset.add(
            Vec3::new(lateral, -angle.cos() * radius, -angle.sin() * radius)
                .mul_scalar(extension),
        )
    }

    /// The plant foot, in the pelvis's frame.
    fn plant_target(&self) -> Vec3 {
        self.strike_body()
            .inverse()
            .map(|frame| frame.transform_point(self.plant))
            .unwrap_or(Vec3::new(0.2, -0.9, 0.0))
            .subtract(PARTS[PELVIS].offset)
            // The ankle sits a boot's thickness above the turf, not in it.
            .add(Vec3::new(0.0, 0.09, 0.0))
    }
}

/// The kicker's ground position, facing and pose, `tick` ticks into the kick and
/// with the leg wherever the swing has got to.
pub fn kick_frame(
    plan: &KickPlan,
    swing: &Swing,
    tick: u32,
    tuning: &KickTuning,
) -> (Vec3, f32, JointPose) {
    let run_up = plan.run_up_ticks(tuning).max(1) as f32;
    let arrived = smooth((tick as f32 / run_up).min(1.0));
    let ground = plan
        .start
        .add(plan.stand.subtract(plan.start).mul_scalar(arrived));
    let travelled = plan.start.subtract(ground).length();

    // Stage 1: the run-up, distance-driven so the legs keep step with the body,
    // and running harder the harder the shot is going to be hit.
    let effort = ((plan.drive.approach - 2.6) / 2.6).clamp(0.0, 1.0);
    let running = run_gait(travelled, STRIDE, effort);

    // Stage 2: the planted strike, built from where the leg actually is.
    let struck = strike_pose(plan, swing, tuning);

    // Ease from the run into the strike so the leg is fully cocked by the moment
    // it is released, then settle out of the follow-through into standing.
    let into_strike = smooth(((tick as f32 - (run_up - 8.0)) / (8.0 + RELEASE_LAG)).clamp(0.0, 1.0));
    let settle_from = swing
        .struck_at()
        .map(|t| t + tuning.follow_through)
        .unwrap_or(u32::MAX);
    let settle = smooth(((swing.ticks() as f32 - settle_from as f32) / 22.0).clamp(0.0, 1.0));
    let action = JointPose::blend(&running, &struck, into_strike);
    // The last thing that happens to any pose: it is made legal. A swing arc is
    // geometry and a blend is arithmetic; neither of them knows what a hip can do.
    (
        ground,
        plan.facing(),
        JointPose::blend(&action, &idle(), settle).human(),
    )
}

/// The body at one moment of the swing.
fn strike_pose(plan: &KickPlan, swing: &Swing, tuning: &KickTuning) -> JointPose {
    let mut pose = plan.strike_root();
    let drive = &plan.drive;
    let through = swing.progress(tuning, plan.contact_angle());

    // Where the boot is: on the arc about the hip, at the swing's own angle. The
    // radius grows as the knee snaps straight — the whip — and is exactly the
    // ball's at the ball, which is what makes contact geometric rather than
    // scheduled.
    let extension = (drive.whip + (1.0 - drive.whip) * through.min(1.0)).clamp(0.30, 1.0);
    let solved = ik::reach(
        PARTS[L_THIGH].offset,
        plan.boot_target(swing.angle(), extension),
        PARTS[L_SHIN].offset.y.abs(),
        PARTS[L_FOOT].offset.y.abs(),
        ik::KNEE_AXIS,
    );
    pose.joints[L_THIGH] = solved.upper;
    pose.joints[L_SHIN] = solved.lower;
    pose.joints[L_FOOT] = qx(-0.14 - 0.26 * through.min(1.0));

    // The support leg is solved onto the planted foot, so the body turns and
    // leans over a foot that stays where it was put.
    let support = ik::reach(
        PARTS[R_THIGH].offset,
        plan.plant_target(),
        PARTS[R_SHIN].offset.y.abs(),
        PARTS[R_FOOT].offset.y.abs(),
        ik::KNEE_AXIS,
    );
    pose.joints[R_THIGH] = support.upper;
    pose.joints[R_SHIN] = support.lower;
    pose.joints[R_FOOT] = qx(-0.16);

    // The upper body: the hips open through the ball, the arms counterbalance the
    // leg. All of it below the root, so none of it moves the hip.
    let opened = drive.turn * through.min(1.2);
    pose.joints[TORSO] = qy(0.26 - 0.55 * through.min(1.0) - opened);
    pose.joints[R_UPPER_ARM] = Quat::from_euler_xyz(
        -0.45 - 1.05 * through.min(1.0),
        0.0,
        0.80 + 0.45 * through.min(1.0),
    );
    pose.joints[R_FOREARM] = qx(-0.42);
    pose.joints[L_UPPER_ARM] = Quat::from_euler_xyz(0.25 + 0.60 * through.min(1.0), 0.0, -0.55);
    pose.joints[L_FOREARM] = qx(-0.30);
    pose
}

/// The signed lateral offset of the boot from the body root — exported so the
/// debug view can draw the swing plane.
pub fn strike_side() -> f32 {
    hip_half_width()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::figure::model::soccer_figure;
    use crate::figure::rig::world_parts;
    use crate::pitch::ball_spot;
    use crate::shot::{BendCurve, GoalTarget, ShotIntent};
    use crate::stroke::Pace;
    use crate::tuning::Tuning;

    fn drive(pace: f32, bend: f32, loft: f32) -> KickDrive {
        let tuning = Tuning::DEFAULT;
        KickDrive::for_shot(
            &ShotIntent::curved(GoalTarget::new(0.0, 0.5), BendCurve::through(0.5, bend, 0.14), BendCurve::through(0.5, loft, 0.14), Pace {
                    speed: pace,
                    easing: 0.0,
                }),
            &tuning,
        )
    }

    /// Run a whole kick and report `(contact tick, the boot's distance from the
    /// ball at every tick)`.
    fn play(plan: &KickPlan) -> (u32, Vec<f32>) {
        let tuning = Tuning::DEFAULT;
        let figure = soccer_figure();
        let release = plan.release_tick(&tuning.kick);
        let contact_angle = plan.contact_angle();
        let mut swing = Swing::cocked(&tuning.kick);
        let gaps = (0..200u32)
            .map(|tick| {
                (tick >= release).then(|| swing.step(&plan.drive, contact_angle, &tuning.kick));
                let (ground, facing, pose) = kick_frame(plan, &swing, tick, &tuning.kick);
                let parts = world_parts(&figure, body_transform(ground, facing, &pose), &pose);
                parts[STRIKE_FOOT]
                    .transform
                    .translation
                    .subtract(plan.ball)
                    .length()
            })
            .collect::<Vec<_>>();
        (
            release + swing.struck_at().expect("the leg reaches the ball"),
            gaps,
        )
    }

    #[test]
    fn the_boot_is_on_the_ball_on_the_tick_the_swing_says_it_is() {
        let tuning = Tuning::DEFAULT;
        let ball = ball_spot(tuning.flight.ball_radius);
        // Every tempo, because the swing gets shorter as it gets harder and the
        // contact tick has to stay exact when the whole downswing is four ticks.
        [0.0f32, 0.5, 1.0].into_iter().for_each(|pace| {
            let plan = KickPlan::for_shot(ball, drive(pace, 0.8, 0.6), &tuning.kick);
            let (contact, gaps) = play(&plan);
            let touching = tuning.flight.ball_radius + 0.12;
            assert!(
                gaps[contact as usize] < touching,
                "pace {pace}: the boot missed the ball by {:.3} m",
                gaps[contact as usize]
            );
            // And it is the closest the boot gets during the kick itself — the
            // window is bounded because a kicker STANDS next to the ball, so its
            // idle foot is naturally near it once the follow-through has settled.
            let nearest = gaps[..(contact as usize + 12).min(gaps.len())]
                .iter()
                .enumerate()
                .min_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(core::cmp::Ordering::Equal))
                .map(|(t, _)| t as u32)
                .expect("the kick has ticks");
            assert_eq!(
                nearest, contact,
                "pace {pace}: the boot was nearest the ball on {nearest}, not on {contact}"
            );
        });
    }

    #[test]
    fn the_leg_is_moving_as_fast_as_the_ball_it_sends_away() {
        // The join between the animation and the flight, asserted rather than
        // hoped for. The hip's torque is derived from the shot's launch speed, so
        // the boot has to *arrive* at that speed divided by the transfer — if the
        // two ever drift apart, this is what says so.
        let tuning = Tuning::DEFAULT;
        let ball = ball_spot(tuning.flight.ball_radius);
        for speed in [0.0f32, 0.5, 1.0] {
            let intent = ShotIntent::curved(GoalTarget::new(0.2, 0.5), BendCurve::through(0.5, 0.8, 0.14), BendCurve::through(0.5, 0.6, 0.14), Pace { speed, easing: 0.0 });
            let plan = KickPlan::for_shot(ball, KickDrive::for_shot(&intent, &tuning), &tuning.kick);
            let contact = plan.contact_angle();
            let mut swing = Swing::cocked(&tuning.kick);
            (0..200).for_each(|_| swing.step(&plan.drive, contact, &tuning.kick));
            let struck = crate::figure::strike::boot_speed(swing.impact_rate())
                * tuning.kick.ball_off_boot;
            let want = intent.launch_speed(&tuning);
            assert!(
                (struck - want).abs() < want * 0.15,
                "the shot leaves at {want:.1} m/s off a boot delivering {struck:.1}"
            );
            // And it is a real kick: a leg doing 20–35 m/s at the ball.
            let boot = crate::figure::strike::boot_speed(swing.impact_rate());
            assert!((18.0..36.0).contains(&boot), "the boot did {boot:.1} m/s");
        }
    }

    #[test]
    fn the_shape_of_the_drawing_changes_where_the_body_stands_and_how_it_leans() {
        let tuning = Tuning::DEFAULT;
        let ball = ball_spot(tuning.flight.ball_radius);
        let straight = KickPlan::for_shot(ball, drive(0.5, 0.0, 0.6), &tuning.kick);
        let bent = KickPlan::for_shot(ball, drive(0.5, tuning.bend.max_offset, 0.6), &tuning.kick);
        let lofted = KickPlan::for_shot(ball, drive(0.5, 0.0, tuning.loft.max_offset), &tuning.kick);
        assert!(
            bent.plant.x.abs() > straight.plant.x.abs(),
            "a bent shot is planted wider"
        );
        assert!(bent.start.x < straight.start.x, "and approached from wider");
        assert!(
            lofted.plant.z > straight.plant.z,
            "a lofted shot is planted further behind the ball"
        );
        assert!(
            lofted.strike_root().root_pitch < straight.strike_root().root_pitch,
            "and leant away from"
        );
        // The contact angle follows the body: a different plant is a different arc.
        assert_ne!(lofted.contact_angle(), straight.contact_angle());
        assert!(strike_side() > 0.0);
    }

    #[test]
    fn the_run_up_starts_away_from_the_ball_and_finishes_beside_it() {
        let tuning = Tuning::DEFAULT;
        let ball = ball_spot(tuning.flight.ball_radius);
        let plan = KickPlan::for_shot(ball, drive(0.5, 0.0, 0.6), &tuning.kick);
        assert!(plan.start.subtract(ball).length() > 2.0);
        assert!(plan.stand.subtract(ball).length() < 0.7);
        let swing = Swing::cocked(&tuning.kick);
        let (start_ground, _, _) = kick_frame(&plan, &swing, 0, &tuning.kick);
        assert_eq!(start_ground, plan.start);
        let (planted, _, _) = kick_frame(&plan, &swing, plan.run_up_ticks(&tuning.kick), &tuning.kick);
        assert!(planted.subtract(plan.stand).length() < 1.0e-4);
        // The kicker faces the goal, not the camera.
        assert!(plan.facing().abs() > 2.0, "facing {}", plan.facing());
    }

    #[test]
    fn the_kicker_waits_at_the_top_of_the_run_up_and_settles_afterwards() {
        let tuning = Tuning::DEFAULT;
        let ball = ball_spot(tuning.flight.ball_radius);
        let plan = KickPlan::for_shot(ball, drive(0.5, 0.0, 0.6), &tuning.kick);
        let (ground, facing, _) = plan.waiting();
        assert_eq!(ground, plan.start);
        assert!(facing.abs() > 2.0);
        // Long after the follow-through the pose has returned to a standing idle.
        let mut swing = Swing::cocked(&tuning.kick);
        (0..260).for_each(|_| swing.step(&plan.drive, plan.contact_angle(), &tuning.kick));
        let (_, _, late) = kick_frame(&plan, &swing, 300, &tuning.kick);
        assert!((late.root_pitch - idle().root_pitch).abs() < 1.0e-3);
    }
}
