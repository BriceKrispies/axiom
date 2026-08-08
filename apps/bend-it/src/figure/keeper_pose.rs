//! The keeper's body, as both a pose and a reach.
//!
//! This module produces one value per tick — a [`KeeperFrame`] — and that value
//! is used for two things that must never disagree: the boxes the renderer
//! draws, and the capsules the ball is tested against. The hands the player
//! watches fly past the ball *are* the hands that failed to save it.
//!
//! The keeper is the same 17-box figure as the kicker, under a different
//! palette. A dive is a lean, a lift and a reach — the arms extend along the
//! dive direction and the body banks into it, so a full-stretch save reads as a
//! full-stretch save rather than as a person sliding sideways.

use axiom::prelude::Vec3;
use axiom_math::Quat;

use crate::contact::Capsule;
use crate::tuning::KeeperTuning;

use super::model::{
    L_FOREARM, L_SHIN, L_THIGH, L_UPPER_ARM, R_FOREARM, R_SHIN, R_THIGH, R_UPPER_ARM, TORSO,
};
use super::pose::{crouch, qx, qz, smooth, JointPose};

/// The keeper's whole physical state for one tick.
#[derive(Debug, Clone, PartialEq)]
pub struct KeeperFrame {
    /// Where the feet (or, mid-dive, the hips' ground projection) are.
    pub ground: Vec3,
    /// Yaw — the keeper always faces the shot.
    pub facing: f32,
    pub pose: JointPose,
    /// The reach capsule: hand to hand through the chest. This is what the ball
    /// is tested against.
    pub reach: Capsule,
    /// The torso capsule: hips to shoulders.
    pub body: Capsule,
}

impl KeeperFrame {
    /// The two capsules a shot can be stopped by, nearest-first.
    pub fn obstacles(&self) -> [Capsule; 2] {
        [self.reach, self.body]
    }
}

/// Where the keeper's hips are, and how committed the dive is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeeperMotion {
    /// Hip position.
    pub hips: Vec3,
    /// Signed lateral direction of the dive, `-1..1` (`0` = standing).
    pub lean: f32,
    /// How far into the dive, `0..1`.
    pub extend: f32,
    /// Where the hands are being thrown: `-1` low, `+1` high.
    pub height_bias: f32,
}

/// Height of the hips above the soles when the keeper is stood up, metres.
const HIP_HEIGHT: f32 = 0.92;
/// Height of the shoulders above the hips, metres.
const SHOULDER_RISE: f32 = 0.52;

/// Resolve the keeper's body for one tick of motion.
pub fn keeper_frame(motion: KeeperMotion, tuning: &KeeperTuning) -> KeeperFrame {
    let extend = motion.extend.clamp(0.0, 1.0);
    let lean = motion.lean.clamp(-1.0, 1.0);
    let bias = motion.height_bias.clamp(-1.0, 1.0);
    let eased = smooth(extend);

    // A dive lays the body over: the bank is what turns a sideways shuffle into
    // a dive, and it is the same number the arms are thrown along.
    let bank = lean * eased * 1.15;
    let mut pose = JointPose::neutral();
    crouch(&mut pose, 0.55 - 0.35 * eased);
    pose.root_roll = bank;
    pose.root_pitch = 0.10 + 0.10 * eased * (1.0 - bias.max(0.0));
    // Trailing leg drives, leading leg tucks — the shape of a push off one foot.
    pose.joints[L_THIGH] = qz(-0.30 * bank);
    pose.joints[R_THIGH] = qz(-0.30 * bank);
    pose.joints[L_SHIN] = qx(0.55 - 0.35 * eased);
    pose.joints[R_SHIN] = qx(0.55 - 0.15 * eased);
    pose.joints[TORSO] = Quat::from_euler_xyz(0.0, 0.0, 0.22 * bank);

    // Arms: at rest a set stance with the hands up and out; in a dive both arms
    // are thrown along the bank, high or low depending on where the read said the
    // ball was going.
    let spread = 0.85 + 0.75 * eased;
    let lift = -0.55 - 1.35 * eased * bias.max(0.0) + 0.85 * eased * (-bias).max(0.0);
    pose.joints[L_UPPER_ARM] = Quat::from_euler_xyz(lift, 0.0, -spread);
    pose.joints[R_UPPER_ARM] = Quat::from_euler_xyz(lift, 0.0, spread);
    pose.joints[L_FOREARM] = qx(-0.30 + 0.25 * eased);
    pose.joints[R_FOREARM] = qx(-0.30 + 0.25 * eased);

    // The reach: a capsule from fingertip to fingertip through the chest. It
    // banks with the body and rises with the jump, so its extent is exactly the
    // wingspan the pose above is drawing.
    let hips = motion.hips;
    let chest = Vec3::new(
        hips.x + bank.sin() * SHOULDER_RISE * 0.55,
        hips.y + SHOULDER_RISE * bank.cos().abs().max(0.35),
        hips.z,
    );
    let span = tuning.arm_span * (0.72 + 0.28 * eased);
    // The arms extend ALONG the dive, not along the body's bank. A keeper at
    // full stretch has its torso laid over and its hands pointing sideways at
    // the ball; taking the reach straight off the bank angle instead would throw
    // the fingertips upward and cost most of the lateral reach that makes a dive
    // worth making.
    let along = Vec3::new((bank * 0.42).cos(), (bank * 0.42).sin(), 0.0);
    // The hands follow the read DOWN as well as up. Offsetting only upward left
    // a keeper that had read a low ball reaching at chest height, which opened a
    // band across the bottom of the goal that nothing could cover.
    let vertical = Vec3::new(0.0, 0.55 * bias, 0.0);
    let hand_a = chest.subtract(along.mul_scalar(span)).add(vertical);
    let hand_b = chest.add(along.mul_scalar(span)).add(vertical);

    // The body is a capsule from the boots to the chest, laid over by the bank —
    // a diving keeper's legs trail *behind* it rather than hanging under it. That
    // shape matters twice: standing, it is a column that guards the bottom of the
    // goal, so a ball arced into the near corner is not an unconditional goal;
    // diving, it tilts away, so the far corner genuinely opens up.
    let body_length = HIP_HEIGHT + SHOULDER_RISE;
    let up_body = Vec3::new(bank.sin(), bank.cos(), 0.0);
    let feet = chest.subtract(up_body.mul_scalar(body_length));
    let boots = Vec3::new(feet.x, feet.y.max(0.0), feet.z);
    KeeperFrame {
        ground: Vec3::new(hips.x, (hips.y - HIP_HEIGHT).max(0.0), hips.z),
        facing: core::f32::consts::PI,
        pose,
        reach: Capsule::new(hand_a, hand_b, tuning.reach_radius),
        body: Capsule::new(boots, chest, tuning.body_radius),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tuning::Tuning;

    fn standing() -> KeeperMotion {
        KeeperMotion {
            hips: Vec3::new(0.0, HIP_HEIGHT, 0.42),
            lean: 0.0,
            extend: 0.0,
            height_bias: 0.0,
        }
    }

    #[test]
    fn a_standing_keeper_reaches_level_and_stands_on_the_ground() {
        let tuning = Tuning::DEFAULT;
        let frame = keeper_frame(standing(), &tuning.keeper);
        assert!(frame.ground.y.abs() < 1.0e-5);
        assert!((frame.reach.a.y - frame.reach.b.y).abs() < 1.0e-4, "level");
        assert!(frame.reach.b.x > 0.4 && frame.reach.a.x < -0.4, "arms out");
        assert_eq!(frame.obstacles().len(), 2);
        assert!(frame.body.b.y > frame.body.a.y, "the torso stands up");
    }

    #[test]
    fn a_dive_banks_the_body_and_throws_the_reach_along_it() {
        let tuning = Tuning::DEFAULT;
        let right = keeper_frame(
            KeeperMotion {
                hips: Vec3::new(1.8, HIP_HEIGHT + 0.5, 0.42),
                lean: 1.0,
                extend: 1.0,
                height_bias: 0.6,
            },
            &tuning.keeper,
        );
        assert!(right.pose.root_roll > 0.8, "the body lays over");
        // The leading hand is above the trailing one: the reach is diagonal.
        assert!(right.reach.b.y > right.reach.a.y + 0.5);
        // A full-stretch dive is wider than a set stance.
        let set = keeper_frame(standing(), &tuning.keeper);
        assert!(
            right.reach.b.subtract(right.reach.a).length()
                > set.reach.b.subtract(set.reach.a).length()
        );
        // Diving the other way mirrors it.
        let left = keeper_frame(
            KeeperMotion {
                lean: -1.0,
                ..KeeperMotion {
                    hips: Vec3::new(-1.8, HIP_HEIGHT + 0.5, 0.42),
                    lean: -1.0,
                    extend: 1.0,
                    height_bias: 0.6,
                }
            },
            &tuning.keeper,
        );
        assert!(left.pose.root_roll < -0.8);
    }

    #[test]
    fn a_low_dive_keeps_the_hands_down() {
        let tuning = Tuning::DEFAULT;
        let low = keeper_frame(
            KeeperMotion {
                hips: Vec3::new(1.5, HIP_HEIGHT * 0.6, 0.42),
                lean: 1.0,
                extend: 1.0,
                height_bias: -1.0,
            },
            &tuning.keeper,
        );
        let high = keeper_frame(
            KeeperMotion {
                hips: Vec3::new(1.5, HIP_HEIGHT * 0.6, 0.42),
                lean: 1.0,
                extend: 1.0,
                height_bias: 1.0,
            },
            &tuning.keeper,
        );
        assert!(low.reach.b.y < high.reach.b.y);
        // Out-of-range inputs clamp instead of producing a broken body.
        let wild = keeper_frame(
            KeeperMotion {
                hips: Vec3::new(0.0, 0.2, 0.42),
                lean: 9.0,
                extend: 9.0,
                height_bias: 9.0,
            },
            &tuning.keeper,
        );
        assert!(wild.pose.root_roll.is_finite() && wild.ground.y >= 0.0);
    }
}
