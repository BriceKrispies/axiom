//! What a joint can actually do.
//!
//! Everything upstream of here — the IK, the swing, the poses — produces a
//! *rotation*, and a rotation is an unbounded thing. A hip solved onto a boot
//! target will happily hand back 125° of abduction if that is what the arithmetic
//! says; a knee will bend backwards; a shin will roll about its own length. None
//! of that is a bug in the solve. It is the solve doing exactly what it was asked
//! and nobody having said what a leg is.
//!
//! So this is where the skeleton says. Each joint declares a **range** — how far
//! the limb may swing from its rest direction in each of the four directions it
//! can swing, and how far it may twist about its own length — and every pose is
//! put through it before anything draws or tests against it. A figure cannot
//! reach a shape a person could not, whatever asked for it.
//!
//! # How a rotation is bounded
//!
//! A limb rotation splits cleanly into two things that mean different things
//! anatomically, and are limited by different tissue:
//!
//! * the **swing** — where the limb points, bounded by the joint capsule and the
//!   muscles that cross it. It is a cone, and not a round one: a hip flexes far
//!   further forward than it extends back.
//! * the **twist** — the limb rotating about its own length, bounded by the
//!   ligaments. It is the one nobody thinks about and the one that reads as
//!   *broken* rather than merely strained when it goes wrong.
//!
//! [`swing_twist`] separates them, each is clamped against its own budget — the
//! swing against an ellipse blended from the four directional limits — and they
//! are put back together. Because the split is exact, a rotation already inside
//! its range comes back unchanged: the limits cost nothing until something asks
//! for the impossible.
//!
//! A hinge needs no special case. A knee is simply a joint with a large backward
//! budget and **zero** of everything else, which the same clamp resolves into
//! pure flexion.

use axiom::prelude::Vec3;
use axiom_math::Quat;

use super::model::{
    HAIR, HEAD, L_FOOT, L_FOREARM, L_HAND, L_SHIN, L_THIGH, L_UPPER_ARM, PART_COUNT, PELVIS,
    R_FOOT, R_FOREARM, R_HAND, R_SHIN, R_THIGH, R_UPPER_ARM, SHOULDERS, TORSO,
};

/// The direction every limb in this figure hangs at rest.
const REST: Vec3 = Vec3::new(0.0, -1.0, 0.0);

/// How far one joint may move, radians.
///
/// The four swing budgets are named for where the limb *goes*, in the figure's
/// own frame: `+Z` is the way it faces, `+X` is its own left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Range {
    pub forward: f32,
    pub back: f32,
    pub left: f32,
    pub right: f32,
    /// Rotation about the limb's own length.
    pub twist: f32,
}

impl Range {
    /// A joint that may do anything — the root, and the parts that are not
    /// joints at all.
    const FREE: Range = Range {
        forward: core::f32::consts::PI,
        back: core::f32::consts::PI,
        left: core::f32::consts::PI,
        right: core::f32::consts::PI,
        twist: core::f32::consts::PI,
    };

    const fn new(forward: f32, back: f32, left: f32, right: f32, twist: f32) -> Range {
        Range {
            forward,
            back,
            left,
            right,
            twist,
        }
    }
}

/// Degrees, as radians, at compile time.
const fn deg(d: f32) -> f32 {
    d * (core::f32::consts::PI / 180.0)
}

/// The figure's own ranges of motion, part by part.
///
/// These are a footballer's, not a contortionist's, and they are the ordinary
/// clinical numbers: a hip flexes about 120° with the knee bent and extends
/// about 20°; a knee flexes 140° and does not hyperextend; an ankle plantarflexes
/// about 50° and dorsiflexes about 25°; a shoulder does very nearly whatever it
/// likes, which is why it is the one joint here whose limits are mostly about the
/// twist.
pub const RANGES: [Range; PART_COUNT] = {
    let mut r = [Range::FREE; PART_COUNT];
    // The spine, in two modest sections.
    r[TORSO] = Range::new(deg(30.0), deg(25.0), deg(25.0), deg(25.0), deg(40.0));
    r[SHOULDERS] = Range::new(deg(20.0), deg(20.0), deg(20.0), deg(20.0), deg(35.0));
    r[HEAD] = Range::new(deg(45.0), deg(55.0), deg(40.0), deg(40.0), deg(70.0));
    r[HAIR] = Range::new(0.0, 0.0, 0.0, 0.0, 0.0);
    // Hips: flexion is the big one, extension is not, and the leg crosses the
    // midline much less freely than it opens away from it.
    r[L_THIGH] = Range::new(deg(120.0), deg(22.0), deg(30.0), deg(45.0), deg(40.0));
    r[R_THIGH] = Range::new(deg(120.0), deg(22.0), deg(45.0), deg(30.0), deg(40.0));
    // Knees: one direction, and nothing else at all. A knee that abducts or
    // twists is the single most broken-looking thing a figure can do.
    r[L_SHIN] = Range::new(0.0, deg(140.0), 0.0, 0.0, deg(5.0));
    r[R_SHIN] = Range::new(0.0, deg(140.0), 0.0, 0.0, deg(5.0));
    // Ankles.
    r[L_FOOT] = Range::new(deg(25.0), deg(50.0), deg(15.0), deg(20.0), deg(12.0));
    r[R_FOOT] = Range::new(deg(25.0), deg(50.0), deg(20.0), deg(15.0), deg(12.0));
    // Shoulders: nearly free in swing, firmly bounded in twist.
    r[L_UPPER_ARM] = Range::new(deg(175.0), deg(60.0), deg(175.0), deg(130.0), deg(85.0));
    r[R_UPPER_ARM] = Range::new(deg(175.0), deg(60.0), deg(130.0), deg(175.0), deg(85.0));
    // Elbows: the mirror of a knee — they fold forwards.
    r[L_FOREARM] = Range::new(deg(145.0), 0.0, 0.0, 0.0, deg(5.0));
    r[R_FOREARM] = Range::new(deg(145.0), 0.0, 0.0, 0.0, deg(5.0));
    // Wrists.
    r[L_HAND] = Range::new(deg(60.0), deg(60.0), deg(25.0), deg(25.0), deg(80.0));
    r[R_HAND] = Range::new(deg(60.0), deg(60.0), deg(25.0), deg(25.0), deg(80.0));
    // The pelvis is the root: it is not a joint, it is where the figure is.
    r[PELVIS] = Range::FREE;
    r
};

/// Split `q` into `(swing, twist)` about `axis`, such that `swing · twist == q`.
///
/// The twist is the part of the rotation that happens about the axis; the swing
/// is everything left over, and it moves the axis without spinning about it.
pub fn swing_twist(q: Quat, axis: Vec3) -> (Quat, Quat) {
    let along = axis.mul_scalar(Vec3::new(q.x, q.y, q.z).dot(axis));
    let twist = Quat {
        x: along.x,
        y: along.y,
        z: along.z,
        w: q.w,
    }
    .normalize()
    .unwrap_or(Quat::IDENTITY);
    let swing = q.multiply(twist.inverse().unwrap_or(Quat::IDENTITY));
    (swing, twist)
}

/// Bring one joint rotation inside its range.
pub fn constrain(q: Quat, range: Range) -> Quat {
    let (swing, twist) = swing_twist(q, REST);
    limited_swing(swing, range).multiply(limited_twist(twist, range))
}

/// The twist, clamped to the joint's own budget.
fn limited_twist(twist: Quat, range: Range) -> Quat {
    // Signed angle about `REST`. The vector part of a twist is parallel to the
    // axis, so its component along it carries the sign.
    let along = Vec3::new(twist.x, twist.y, twist.z).dot(REST);
    let angle = 2.0 * along.atan2(twist.w);
    let wrapped = wrap(angle);
    Quat::from_axis_angle(REST, wrapped.clamp(-range.twist, range.twist)).unwrap_or(Quat::IDENTITY)
}

/// The swing, clamped to the joint's own cone.
///
/// The cone is elliptical, blended from the four directional budgets by which way
/// the limb actually went — so a hip swinging forward-and-out is bounded by
/// something between its flexion limit and its abduction limit rather than by
/// whichever happens to be larger.
fn limited_swing(swing: Quat, range: Range) -> Quat {
    let pointed = swing.rotate(REST);
    let angle = pointed.dot(REST).clamp(-1.0, 1.0).acos();
    // Which way it swung, in the plane perpendicular to rest.
    let sideways = Vec3::new(pointed.x, 0.0, pointed.z);
    let way = sideways.normalize().unwrap_or(Vec3::UNIT_Z);
    let ahead = [range.back, range.forward][usize::from(way.z >= 0.0)];
    let across = [range.right, range.left][usize::from(way.x >= 0.0)];
    // The ellipse's radius in that direction. A zero budget on one axis leaves a
    // joint that can only move on the other, which is exactly a hinge.
    let limit = 1.0
        / ((way.z / ahead.max(1.0e-4)).powi(2) + (way.x / across.max(1.0e-4)).powi(2))
            .sqrt()
            .max(1.0e-4);
    let axis = REST.cross(pointed).normalize().unwrap_or(Vec3::UNIT_X);
    Quat::from_axis_angle(axis, angle.min(limit)).unwrap_or(Quat::IDENTITY)
}

/// An angle folded into `-π..π`, so a twist of 350° reads as −10° rather than as
/// something to be clamped down to the limit.
fn wrap(angle: f32) -> f32 {
    let turns = (angle + core::f32::consts::PI) / core::f32::consts::TAU;
    angle - turns.floor() * core::f32::consts::TAU
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::figure::pose::qx;

    fn about(axis: Vec3, angle: f32) -> Quat {
        Quat::from_axis_angle(axis, angle).expect("an axis")
    }

    /// Where a joint points, and how far it has twisted, after constraining.
    fn resolved(q: Quat, range: Range) -> (Vec3, f32) {
        let out = constrain(q, range);
        let (_, twist) = swing_twist(out, REST);
        let along = Vec3::new(twist.x, twist.y, twist.z).dot(REST);
        (out.rotate(REST), wrap(2.0 * along.atan2(twist.w)))
    }

    #[test]
    fn a_split_rotation_puts_itself_back_together() {
        [
            Quat::IDENTITY,
            qx(0.9),
            about(Vec3::new(0.0, -1.0, 0.0), 0.7),
            Quat::from_euler_xyz(0.6, -0.4, 0.9),
            Quat::from_euler_xyz(-1.9, 0.2, 2.1),
        ]
        .into_iter()
        .for_each(|q| {
            let (swing, twist) = swing_twist(q, REST);
            let back = swing.multiply(twist);
            // Quaternions double-cover, so compare what they DO.
            [Vec3::UNIT_X, Vec3::UNIT_Y, Vec3::UNIT_Z]
                .into_iter()
                .for_each(|v| {
                    assert!(
                        back.rotate(v).subtract(q.rotate(v)).length() < 1.0e-4,
                        "{q:?} did not survive being split"
                    );
                });
            // And the twist really is a twist: it leaves the limb's own axis alone.
            assert!(twist.rotate(REST).subtract(REST).length() < 1.0e-4);
        });
    }

    #[test]
    fn a_rotation_already_inside_its_range_is_left_alone() {
        // The whole rotation, not just where it points. A clamp that quietly
        // rebuilt the twist would leave the limb aimed correctly and rolled
        // wrongly, which is the exact defect the twist budget exists to stop.
        let hip = RANGES[L_THIGH];
        [
            Quat::IDENTITY,
            qx(-0.6),
            qx(0.3),
            Quat::from_euler_xyz(-0.5, 0.2, 0.15),
            Quat::from_euler_xyz(-1.2, -0.3, -0.2),
        ]
        .into_iter()
        .for_each(|q| {
            let out = constrain(q, hip);
            [Vec3::UNIT_X, Vec3::UNIT_Y, Vec3::UNIT_Z]
                .into_iter()
                .for_each(|v| {
                    assert!(
                        out.rotate(v).subtract(q.rotate(v)).length() < 1.0e-3,
                        "a legal {q:?} became {out:?}"
                    );
                });
        });
    }

    #[test]
    fn a_hip_flexes_much_further_than_it_extends() {
        let hip = RANGES[L_THIGH];
        // Asking for 170° of flexion gets the 120° a hip has.
        let (flexed, _) = resolved(qx(-3.0), hip);
        let forward = flexed.z.atan2(-flexed.y);
        assert!(
            (forward - hip.forward).abs() < 0.02,
            "flexed to {:.0}°, the limit is {:.0}°",
            forward.to_degrees(),
            hip.forward.to_degrees()
        );
        // And backwards it is stopped far sooner.
        let (extended, _) = resolved(qx(1.6), hip);
        let back = (-extended.z).atan2(-extended.y);
        assert!((back - hip.back).abs() < 0.02, "extended to {:.0}°", back.to_degrees());
        assert!(hip.back < hip.forward * 0.5, "a hip is not symmetric");
    }

    #[test]
    fn a_knee_only_bends_one_way_and_never_twists() {
        let knee = RANGES[L_SHIN];
        // Bending it the wrong way straightens it instead.
        let (wrong, _) = resolved(qx(-1.0), knee);
        assert!(wrong.subtract(REST).length() < 0.02, "a knee hyperextended to {wrong:?}");
        // Bending it the right way works, up to the limit.
        let (right, _) = resolved(qx(1.0), knee);
        assert!(right.z < -0.5, "the shin folded back: {right:?}");
        let (past, _) = resolved(qx(3.0), knee);
        let folded = (-past.z).atan2(-past.y);
        assert!((folded - knee.back).abs() < 0.02, "folded to {:.0}°", folded.to_degrees());
        // Sideways and twist are simply not available to it.
        let (sideways, twist) = resolved(Quat::from_euler_xyz(0.8, 1.2, 0.9), knee);
        assert!(sideways.x.abs() < 0.03, "a knee abducted to {sideways:?}");
        assert!(twist.abs() <= knee.twist + 0.01, "a knee twisted {:.0}°", twist.to_degrees());
    }

    #[test]
    fn the_swing_cone_blends_between_its_directions() {
        let hip = RANGES[L_THIGH];
        // Straight out to the side is bounded by abduction, not by flexion — and
        // by the budget for THAT side, which is not the same as the other.
        [(-2.5f32, hip.right), (2.5, hip.left)]
            .into_iter()
            .for_each(|(turn, budget)| {
                let (out, _) = resolved(about(Vec3::UNIT_Z, turn), hip);
                let sideways = out.x.atan2(-out.y).abs();
                assert!(
                    (sideways - budget).abs() < 0.02,
                    "swung {:.0}° to the side, the limit that way is {:.0}°",
                    sideways.to_degrees(),
                    budget.to_degrees()
                );
            });
        // Forward AND out lands between the two budgets, not at either. Asked for
        // explicitly: point the limb along a direction and see how far it gets.
        let toward = |d: Vec3| {
            let d = d.normalize().expect("a direction");
            about(REST.cross(d).normalize().expect("an axis"), d.dot(REST).acos())
        };
        let (mixed, _) = resolved(toward(Vec3::new(-0.6, -0.1, 0.6)), hip);
        let angle = mixed.dot(REST).clamp(-1.0, 1.0).acos();
        assert!(
            angle > hip.right && angle < hip.forward,
            "a forward-and-out swing reached {:.0}°, between {:.0}° and {:.0}°",
            angle.to_degrees(),
            hip.right.to_degrees(),
            hip.forward.to_degrees()
        );
    }

    /// How far a joint has swung from rest, and how far it has twisted.
    fn measure(q: Quat) -> (Vec3, f32) {
        let (_, twist) = swing_twist(q, REST);
        let along = Vec3::new(twist.x, twist.y, twist.z).dot(REST);
        (q.rotate(REST), wrap(2.0 * along.atan2(twist.w)))
    }

    /// Every joint of a pose, checked against its own range.
    fn assert_human(pose: &crate::figure::JointPose, what: &str) {
        (0..PART_COUNT).for_each(|i| {
            let range = RANGES[i];
            let (pointed, twist) = measure(pose.joints[i]);
            let angle = pointed.dot(REST).clamp(-1.0, 1.0).acos();
            let widest = range
                .forward
                .max(range.back)
                .max(range.left)
                .max(range.right);
            assert!(
                angle <= widest + 0.02,
                "{what}: joint {i} swung {:.0}°, its widest budget is {:.0}°",
                angle.to_degrees(),
                widest.to_degrees()
            );
            assert!(
                twist.abs() <= range.twist + 0.02,
                "{what}: joint {i} twisted {:.0}°, its budget is {:.0}°",
                twist.to_degrees(),
                range.twist.to_degrees()
            );
        });
    }

    #[test]
    fn no_tick_of_a_whole_kick_leaves_the_body_outside_its_range() {
        // The regression this module exists for. The IK is geometry, the swing is
        // physics and the blend is arithmetic; none of the three knows what a leg
        // is, and before there were limits the hip rolled through 125° of
        // abduction on its way out of a follow-through.
        use crate::figure::{kick_frame, KickDrive, KickPlan, Swing};
        use crate::pitch::ball_spot;
        use crate::shot::{BendCurve, GoalTarget, ShotIntent};
        use crate::stroke::Pace;
        use crate::tuning::Tuning;
        let tuning = Tuning::DEFAULT;
        let ball = ball_spot(tuning.flight.ball_radius);
        [(0.0f32, 0.0f32), (0.5, 1.8), (1.0, -1.8)]
            .into_iter()
            .for_each(|(pace, bend)| {
                let intent = ShotIntent {
                    target: GoalTarget::new(0.6, 0.7),
                    bend: BendCurve::through(0.4, bend, 0.14),
                    loft: BendCurve::through(0.5, 1.2, 0.14),
                    pace: Pace { speed: pace, easing: 0.0 },
                };
                let plan =
                    KickPlan::for_shot(ball, KickDrive::for_shot(&intent, &tuning), &tuning.kick);
                let release = plan.release_tick(&tuning.kick);
                let contact = plan.contact_angle();
                let mut swing = Swing::cocked(&tuning.kick);
                (0..release + 90).for_each(|tick| {
                    (tick >= release).then(|| swing.step(&plan.drive, contact, &tuning.kick));
                    let (_, _, pose) = kick_frame(&plan, &swing, tick, &tuning.kick);
                    assert_human(&pose, &format!("pace {pace} bend {bend} tick {tick}"));
                });
            });
    }

    #[test]
    fn no_moment_of_a_dive_leaves_the_keeper_outside_its_range() {
        use crate::figure::{keeper_frame, KeeperMotion};
        use crate::tuning::Tuning;
        let tuning = Tuning::DEFAULT;
        [-1.0f32, -0.4, 0.0, 0.6, 1.0].into_iter().for_each(|lean| {
            [0.0f32, 0.35, 1.0].into_iter().for_each(|extend| {
                [-1.0f32, 0.0, 1.0].into_iter().for_each(|bias| {
                    let frame = keeper_frame(
                        KeeperMotion {
                            hips: Vec3::new(lean * 2.2, 1.05, 0.26),
                            lean,
                            extend,
                            height_bias: bias,
                            hands: Vec3::new(lean * 3.2, 0.4 + bias + 1.0, 0.76),
                        },
                        &tuning.keeper,
                    );
                    assert_human(&frame.pose, &format!("lean {lean} extend {extend} bias {bias}"));
                });
            });
        });
    }

    #[test]
    fn every_part_of_the_figure_declares_a_range() {
        assert_eq!(RANGES.len(), PART_COUNT);
        // The legs are mirror images of one another, and nothing has a negative
        // budget, which would be a range that cannot contain even rest.
        assert_eq!(RANGES[L_THIGH].left, RANGES[R_THIGH].right);
        assert_eq!(RANGES[L_THIGH].right, RANGES[R_THIGH].left);
        assert_eq!(RANGES[L_SHIN], RANGES[R_SHIN]);
        RANGES.iter().for_each(|r| {
            assert!(r.forward >= 0.0 && r.back >= 0.0);
            assert!(r.left >= 0.0 && r.right >= 0.0 && r.twist >= 0.0);
        });
        // A degenerate rotation does not produce a broken joint.
        let out = constrain(Quat { x: 0.0, y: 0.0, z: 0.0, w: 0.0 }, RANGES[L_THIGH]);
        assert!(out.rotate(REST).length().is_finite());
    }
}
