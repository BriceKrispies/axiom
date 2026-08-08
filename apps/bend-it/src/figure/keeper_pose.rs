//! The keeper's body, as both a pose and a reach.
//!
//! This module produces one value per tick — a [`KeeperFrame`] — and that value
//! is used for two things that must never disagree: the boxes the renderer
//! draws, and the capsules the ball is tested against. The hands the player
//! watches fly past the ball *are* the hands that failed to save it.
//!
//! The keeper is the same 17-box figure as the kicker, under a different
//! palette. A dive is a lean, a lift and a reach — the body banks into it and
//! **the arms are solved onto the point the keeper is throwing its hands at**,
//! so a full-stretch save reads as a full-stretch save rather than as a person
//! sliding sideways with its arms held out at a fixed angle.
//!
//! The arms used to be two Euler angles thrown along the bank, and the reach
//! capsule was a separate analytic line drawn near where they seemed to be. Two
//! descriptions of one thing is one description too many: the hands could point
//! somewhere the capsule was not, which is a save that looks like a miss (or the
//! reverse). Now there is one description. The arms are solved by
//! [`super::ik`] onto the target, and the capsule is built from **the solved
//! fingertips** — so the hands the player watches fly past the ball really are
//! the hands that failed to save it.

use axiom::prelude::Vec3;
use axiom_math::{Quat, Transform};

use crate::contact::Capsule;
use crate::tuning::KeeperTuning;

use super::ik;
use super::model::{
    L_FOREARM, L_HAND, L_SHIN, L_THIGH, L_UPPER_ARM, PARTS, PELVIS, R_FOREARM, R_SHIN,
    R_THIGH, R_UPPER_ARM, SHOULDERS, TORSO,
};
use super::pose::{crouch, qx, qz, smooth, JointPose};
use super::rig::body_transform;

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
    /// The world point the keeper is throwing its hands at — where it believes
    /// the ball is going.
    ///
    /// The *leading* hand is solved onto this; the trailing one counterbalances.
    /// A keeper cannot reach what it did not read, so this is its prediction and
    /// never the ball: a wrong read puts the hands confidently in the wrong place,
    /// which is the whole point of the keeper.
    pub hands: Vec3,
}

/// Height of the hips above the soles when the keeper is stood up, metres.
const HIP_HEIGHT: f32 = 0.92;
/// Height of the shoulders above the hips, metres.
const SHOULDER_RISE: f32 = 0.52;

/// How far the keeper's hand reaches past its shoulder, metres — the figure's
/// own arm, measured off the model rather than written down a second time.
///
/// The dive aims the **hips** this far short of the ball and lets the arm cover
/// the rest, which is what a keeper does: you do not put your hips on the ball,
/// you put a hand on it.
pub fn arm_reach() -> f32 {
    let (upper, fore, glove) = arm_bones();
    upper + fore + glove
}

/// How far sideways the keeper's fingertips get past its **hips** at full
/// stretch, metres.
///
/// **Measured, not asserted.** It used to be a sum — the shoulder's carry over a
/// laid-over body, plus the arm — and a sum goes stale the moment anything
/// upstream changes. The arms are now bounded by the figure's own ranges of
/// motion, so how far the fingertips genuinely get is a fact about the skeleton
/// that only the skeleton can answer. So it is asked: pose a keeper at full
/// stretch, reaching for something it cannot possibly get to, and see where its
/// hand ends up.
///
/// It matters because the dive aims the **hips** this much short of the ball and
/// lets the arm cover the rest — which is what a keeper does, and which only
/// works if the number is true.
pub fn stretch_from_hips(tuning: &KeeperTuning) -> f32 {
    let hips = Vec3::new(0.0, HIP_HEIGHT * 0.72, 0.0);
    let frame = keeper_frame(
        KeeperMotion {
            hips,
            lean: 1.0,
            extend: 1.0,
            height_bias: 0.0,
            hands: Vec3::new(20.0, hips.y, 0.30),
            },
        tuning,
    );
    frame.reach.b.x - hips.x
}

/// The two bones of an arm, and how far past the wrist the glove reaches.
fn arm_bones() -> (f32, f32, f32) {
    (
        PARTS[L_FOREARM].offset.y.abs(),
        PARTS[L_HAND].offset.y.abs(),
        PARTS[L_HAND].box_size.y * 0.5 + PARTS[L_HAND].box_offset.y.abs(),
    )
}

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

    // The frame the arms hang in: pelvis → torso → shoulder yoke, composed from
    // the pose just built and from where the hips actually are, so the solve
    // happens in the same body the renderer is about to draw.
    let hips = motion.hips;
    let ground = Vec3::new(hips.x, (hips.y - HIP_HEIGHT).max(0.0), hips.z);
    let facing = core::f32::consts::PI;
    let yoke = Transform::combine(
        body_transform(ground, facing, &pose),
        Transform::combine(
            Transform::combine(
                Transform::new(PARTS[PELVIS].offset, pose.joints[PELVIS], Vec3::ONE),
                Transform::new(PARTS[TORSO].offset, pose.joints[TORSO], Vec3::ONE),
            ),
            Transform::new(PARTS[SHOULDERS].offset, pose.joints[SHOULDERS], Vec3::ONE),
        ),
    );
    let into_yoke = yoke.inverse().unwrap_or(Transform::IDENTITY);
    let (upper, fore, glove) = arm_bones();

    let target = into_yoke.transform_point(motion.hands);

    // How far into the dive this is. A keeper stood on its line is not reaching
    // for anything yet and both hands belong out in front of it; once it has
    // committed, the leading hand abandons that stance and goes for the target.
    //
    // Deliberately NOT scaled by how big the dive is. Scaling it by `lean` looks
    // reasonable and is wrong: a keeper a metre off centre leans very little, so
    // its hand would only travel a fraction of the way to a ball it could
    // comfortably have reached — which put a cliff in the save rate about a
    // metre either side of the middle and made the whole inner goal free.
    let commit = eased;
    let span = upper + fore;
    // The set stance, per side: hands out, a little down, a little forward.
    let set = |side: f32| Vec3::new(side * span * 0.62, -0.30, 0.34 + 0.18 * eased);
    let toward = |side: f32| {
        let rest = set(side);
        rest.add(target.subtract(rest).mul_scalar(commit))
    };
    // The trailing arm keeps its side of the stance and is thrown further out by
    // the bank — which is both what a diving keeper does and what keeps the reach
    // spanning the body instead of collapsing to a point.
    let trail = |side: f32| {
        set(side).add(Vec3::new(
            side * span * 0.30 * eased,
            -0.30 * eased * (-bias).max(0.0) + 0.45 * eased * bias.max(0.0),
            -0.20 * eased,
        ))
    };
    // Which arm leads: the one whose shoulder is on the same side as the target.
    // Read off the target in the body's own frame rather than off the world-space
    // lean, so it stays right whichever way the figure is turned.
    let leads = |side: f32| target.x * side > 0.0;
    let arm_target = |side: f32| match leads(side) {
        true => toward(side),
        false => trail(side),
    };
    let solved = |root: Vec3, at: Vec3| ik::reach(root, at, upper, fore, ik::ELBOW_AXIS);
    let left = solved(PARTS[L_UPPER_ARM].offset, arm_target(-1.0));
    let right = solved(PARTS[R_UPPER_ARM].offset, arm_target(1.0));
    pose.joints[L_UPPER_ARM] = left.upper;
    pose.joints[L_FOREARM] = left.lower;
    pose.joints[R_UPPER_ARM] = right.upper;
    pose.joints[R_FOREARM] = right.lower;

    // Made anatomically legal HERE, before the reach is measured — so if a
    // shoulder had to give up some of what the solve asked for, the capsule gives
    // up exactly the same. Constraining afterwards would put the keeper's saves
    // back where its arms are not.
    let pose = pose.human();
    let left = ik::Solve { upper: pose.joints[L_UPPER_ARM], lower: pose.joints[L_FOREARM] };
    let right = ik::Solve { upper: pose.joints[R_UPPER_ARM], lower: pose.joints[R_FOREARM] };

    // The reach: a capsule between the two solved fingertips. It is derived from
    // the same solve the arms are drawn with, so its extent is exactly the
    // wingspan on screen — no second description to drift out of step.
    let tip = |root: Vec3, solve: ik::Solve| {
        let (_, wrist) = ik::chain(root, solve, upper, fore);
        // The glove runs on along the arm's OVERALL reach — shoulder to wrist —
        // rather than along the forearm. Two reasons, and the second is the one
        // that matters: it is what "reach" means for a capsule, and it is exactly
        // mirror-symmetric. Taking it off the forearm depends on where the elbow
        // ended up, and an elbow is a rotation — a mirrored shot does not produce
        // a mirrored one, so a left-corner shot and its right-corner twin came out
        // a centimetre apart and occasionally disagreed about a save.
        let out = wrist
            .subtract(root)
            .normalize()
            .unwrap_or(Vec3::new(0.0, -1.0, 0.0));
        yoke.transform_point(wrist.add(out.mul_scalar(glove)))
    };
    // Ordered by world x, so `a` is always the left-hand end of the reach and
    // `b` the right one, whichever of the keeper's arms happens to be there.
    let tips = [
        tip(PARTS[L_UPPER_ARM].offset, left),
        tip(PARTS[R_UPPER_ARM].offset, right),
    ];
    let swap = usize::from(tips[0].x > tips[1].x);
    let (hand_a, hand_b) = (tips[swap], tips[1 - swap]);
    let chest = Vec3::new(
        hips.x + bank.sin() * SHOULDER_RISE * 0.55,
        hips.y + SHOULDER_RISE * bank.cos().abs().max(0.35),
        hips.z,
    );

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
        ground,
        facing,
        pose,
        reach: Capsule::new(hand_a, hand_b, tuning.reach_radius),
        body: Capsule::new(boots, chest, tuning.body_radius),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tuning::Tuning;

    /// A keeper diving to `hands`, `extend` of the way through it.
    fn diving(hips: Vec3, lean: f32, extend: f32, bias: f32, hands: Vec3) -> KeeperMotion {
        KeeperMotion {
            hips,
            lean,
            extend,
            height_bias: bias,
            hands,
        }
    }

    fn standing() -> KeeperMotion {
        diving(
            Vec3::new(0.0, HIP_HEIGHT, 0.42),
            0.0,
            0.0,
            0.0,
            Vec3::new(0.0, HIP_HEIGHT + 0.35, 0.72),
        )
    }

    #[test]
    fn a_standing_keeper_reaches_level_and_stands_on_the_ground() {
        let tuning = Tuning::DEFAULT;
        let frame = keeper_frame(standing(), &tuning.keeper);
        assert!(frame.ground.y.abs() < 1.0e-5);
        assert!(frame.reach.b.x > 0.25 && frame.reach.a.x < -0.25, "arms out");
        assert_eq!(frame.obstacles().len(), 2);
        assert!(frame.body.b.y > frame.body.a.y, "the torso stands up");
    }

    #[test]
    fn the_leading_hand_goes_where_the_keeper_throws_it() {
        // The point of solving the arms: the fingertips arrive at the target
        // rather than near it, and the capsule the ball is tested against is
        // built from those same fingertips.
        let tuning = Tuning::DEFAULT;
        for (lean, at) in [
            (1.0f32, Vec3::new(1.9, 1.30, 0.76)),
            (-1.0, Vec3::new(-1.9, 1.30, 0.76)),
            (1.0, Vec3::new(1.6, 0.70, 0.76)),
        ] {
            // Stood where a keeper diving at that point would actually be — the
            // arms reach the last half metre, they do not teleport.
            let hips = Vec3::new(at.x - 0.35 * lean, at.y - 0.30, 0.42);
            let frame = keeper_frame(diving(hips, lean, 1.0, 0.0, at), &tuning.keeper);
            // Whichever end of the capsule is the leading hand, it is the one
            // nearest the target, and it is genuinely near it.
            let gap = frame
                .reach
                .a
                .subtract(at)
                .length()
                .min(frame.reach.b.subtract(at).length());
            assert!(
                gap < 0.22,
                "thrown at {at:?}, the nearest hand finished {gap:.2} m away"
            );
        }
    }

    #[test]
    fn a_target_beyond_the_arm_is_reached_toward_rather_than_snapped_to() {
        // An arm is 0.67 m long and cannot be told otherwise. What it can do is
        // straighten and point, which is what a keeper stretching for one it will
        // not get looks like.
        let tuning = Tuning::DEFAULT;
        let hips = Vec3::new(1.0, HIP_HEIGHT * 0.7, 0.42);
        let miles = Vec3::new(4.0, 1.2, 0.76);
        let frame = keeper_frame(diving(hips, 1.0, 1.0, 0.0, miles), &tuning.keeper);
        let hand = frame.reach.b;
        assert!(hand.x > hips.x + 0.30, "it is stretching that way: {hand:?}");
        assert!(hand.subtract(hips).length() < 1.4, "but not past its own arm");
    }

    #[test]
    fn a_dive_banks_the_body_and_the_hands_still_span_it() {
        let tuning = Tuning::DEFAULT;
        let right = keeper_frame(
            diving(
                Vec3::new(1.8, HIP_HEIGHT + 0.5, 0.42),
                1.0,
                1.0,
                0.6,
                Vec3::new(2.6, 1.7, 0.76),
            ),
            &tuning.keeper,
        );
        assert!(right.pose.root_roll > 0.8, "the body lays over");
        // The leading hand is above the trailing one: the reach is diagonal.
        assert!(right.reach.b.y > right.reach.a.y + 0.3);
        // A full-stretch dive is wider than a set stance.
        let set = keeper_frame(standing(), &tuning.keeper);
        assert!(
            right.reach.b.subtract(right.reach.a).length()
                > set.reach.b.subtract(set.reach.a).length()
        );
        // Diving the other way mirrors it.
        let left = keeper_frame(
            diving(
                Vec3::new(-1.8, HIP_HEIGHT + 0.5, 0.42),
                -1.0,
                1.0,
                0.6,
                Vec3::new(-2.6, 1.7, 0.76),
            ),
            &tuning.keeper,
        );
        assert!(left.pose.root_roll < -0.8);
        assert!((left.reach.a.x + right.reach.b.x).abs() < 0.35, "mirrored");
    }

    #[test]
    fn a_low_dive_keeps_the_hands_down() {
        let tuning = Tuning::DEFAULT;
        let at = |y: f32| Vec3::new(2.2, y, 0.76);
        let low = keeper_frame(
            diving(Vec3::new(1.5, HIP_HEIGHT * 0.6, 0.42), 1.0, 1.0, -1.0, at(0.25)),
            &tuning.keeper,
        );
        let high = keeper_frame(
            diving(Vec3::new(1.5, HIP_HEIGHT * 0.6, 0.42), 1.0, 1.0, 1.0, at(1.9)),
            &tuning.keeper,
        );
        assert!(low.reach.b.y < high.reach.b.y);
        // Out-of-range inputs clamp instead of producing a broken body.
        let wild = keeper_frame(
            diving(Vec3::new(0.0, 0.2, 0.42), 9.0, 9.0, 9.0, Vec3::new(40.0, 40.0, 40.0)),
            &tuning.keeper,
        );
        assert!(wild.pose.root_roll.is_finite() && wild.ground.y >= 0.0);
        assert!(wild.reach.a.x.is_finite() && wild.reach.b.y.is_finite());
    }
}
