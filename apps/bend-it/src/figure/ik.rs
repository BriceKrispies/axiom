//! Two-bone inverse kinematics: put the end of the limb *there*, and let the
//! joints work out how.
//!
//! Limbs used to be tables of joint angles that had been tuned until the boot
//! happened to arrive at the ball and the hands happened to look like a dive.
//! That works exactly once. The moment the body starts moving differently — a
//! harder shot leaning further in, a wider plant, a keeper diving to a different
//! read — every one of those angles is wrong, and the boot swings through fresh
//! air while the hands point at nothing.
//!
//! So a limb is solved rather than posed. The two joints are whatever they have
//! to be for the end of the chain to reach a position the game names, which means
//! the body above can do as it likes and the contact still happens.
//!
//! It is the classic planar solve, but built as a **basis** rather than as Euler
//! angles. Decomposing the aim into successive `Z` and `X` swings looks simpler
//! and is quietly broken: the sideways swing has to be clamped to keep the joint
//! out of the body, and the clamp makes whole directions — a boot lifted above
//! the hip, which is most of a follow-through — unreachable. So instead:
//!
//! 1. the joint is a hinge, so settle its axis first. A knee's axis is the body's
//!    lateral axis and it stays that way through the whole swing, so the axis is
//!    given directly and only made perpendicular to the reach. **Not** derived by
//!    crossing the reach with a fixed "the knee points forward" pole: that is the
//!    textbook formulation and it has a singularity exactly where a kick lives.
//!    When the leg comes up past horizontal the reach is parallel to "forward",
//!    the cross product collapses, and the hip rolls through 125° of abduction on
//!    its way to nothing in particular. Projecting a stable axis has its own
//!    singularity — a leg pointing straight out sideways — which is nowhere a leg
//!    goes;
//! 2. close the triangle with the law of cosines — the angle at the root between
//!    the upper bone and the straight line to the target, and the angle at the
//!    hinge;
//! 3. build the upper bone's rotation directly from the hinge and the bent
//!    direction, and bend the joint by what is left. Every direction is
//!    reachable, because nothing was ever decomposed.
//!
//! A target further away than the limb is long is not an error — a limb reaching
//! for something out of reach straightens and points at it, which is what a limb
//! does.

use axiom::prelude::Vec3;
use axiom_math::{Quat, Transform};

/// What the two joints have to be.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Solve {
    /// The joint at the root — hip, or shoulder.
    pub upper: Quat,
    /// The hinge — knee, or elbow.
    pub lower: Quat,
}

/// A **knee's** hinge axis: a leg folds forwards about the body's `+X`.
pub const KNEE_AXIS: Vec3 = Vec3::new(1.0, 0.0, 0.0);
/// An **elbow's** hinge axis: an arm folds the other way.
pub const ELBOW_AXIS: Vec3 = Vec3::new(-1.0, 0.0, 0.0);

/// Solve a two-bone limb so the end of it lands on `target`.
///
/// Everything is in the **parent joint's** space: `root` is the limb's own offset
/// from that pivot, the limb hangs along `-Y` at rest, and `hinge_axis` is the
/// axis the middle joint turns about ([`KNEE_AXIS`] or [`ELBOW_AXIS`]).
/// `upper` and `lower` are the two bone lengths.
pub fn reach(root: Vec3, target: Vec3, upper: f32, lower: f32, hinge_axis: Vec3) -> Solve {
    let to_target = target.subtract(root);
    let span = to_target.length().max(1.0e-4);
    let aim = to_target.normalize().unwrap_or(Vec3::new(0.0, -1.0, 0.0));

    // The hinge, made perpendicular to the reach — the component of the joint's
    // own axis that survives Gram-Schmidt against the direction the limb is
    // pointing. A knee turns about the body's lateral axis wherever the leg is,
    // so this stays put through a whole swing instead of collapsing halfway
    // through the follow-through (see the module docs).
    let hinge = hinge_axis
        .subtract(aim.mul_scalar(hinge_axis.dot(aim)))
        .normalize()
        .unwrap_or(Vec3::UNIT_X);

    // Close the triangle. `apex` is the angle at the root between the upper bone
    // and the straight line to the target; `interior` is the angle at the hinge,
    // so what is left of a straight limb is the joint's own flexion. Both cosines
    // are clamped, which is also what makes an out-of-reach target behave: the
    // triangle degenerates, the flexion falls to zero, and the limb straightens
    // and points — which is what a limb reaching for something does.
    let apex = clamped_acos((upper * upper + span * span - lower * lower) / (2.0 * upper * span));
    let interior =
        clamped_acos((upper * upper + lower * lower - span * span) / (2.0 * upper * lower));
    let flexion = core::f32::consts::PI - interior;

    // The upper bone points short of the target by the apex angle, rotated about
    // the hinge so the joint leads the right way.
    let bent = Quat::from_axis_angle(hinge, -apex)
        .map(|turn| turn.rotate(aim))
        .unwrap_or(aim);
    Solve {
        // Built as a basis rather than as Euler angles: the limb hangs along
        // local `-Y`, so `+Y` is the reverse of where it points, and local `+X`
        // must land exactly on the hinge or the joint would bend out of its plane.
        upper: Quat::look_rotation(hinge.cross(bent), bent.mul_scalar(-1.0))
            .unwrap_or(Quat::IDENTITY),
        lower: Quat::from_euler_xyz(flexion, 0.0, 0.0),
    }
}

/// Where the two joints of a solved limb actually end up: `(hinge, end)`, in the
/// same space the solve was made in.
///
/// The solve says what the rotations are; this says where they *put* things,
/// which is what a capsule or a contact test needs. Deriving it here rather than
/// re-deriving it at each call site is the point — a hand that is drawn in one
/// place and tested for a save in another is exactly the bug this prevents.
pub fn chain(root: Vec3, solve: Solve, upper: f32, lower: f32) -> (Vec3, Vec3) {
    let above = Transform::new(root, solve.upper, Vec3::ONE);
    let bend = Transform::new(Vec3::new(0.0, -upper, 0.0), solve.lower, Vec3::ONE);
    (
        Transform::combine(above, Transform::from_translation(bend.translation)).translation,
        Transform::combine(
            Transform::combine(above, bend),
            Transform::from_translation(Vec3::new(0.0, -lower, 0.0)),
        )
        .translation,
    )
}

/// `acos` that tolerates a hair outside its domain.
fn clamped_acos(value: f32) -> f32 {
    value.clamp(-1.0, 1.0).acos()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::figure::model::{L_FOOT, L_FOREARM, L_HAND, L_SHIN, L_THIGH, L_UPPER_ARM, PARTS};

    /// The lengths the figure actually has, so the solve is tested against the
    /// model rather than against numbers invented for the test.
    fn bones() -> (Vec3, f32, f32) {
        (
            PARTS[L_THIGH].offset,
            PARTS[L_SHIN].offset.y.abs(),
            PARTS[L_FOOT].offset.y.abs(),
        )
    }

    fn leg(target: Vec3) -> Solve {
        let (hip, thigh, shin) = bones();
        reach(hip, target, thigh, shin, KNEE_AXIS)
    }

    /// Where the ankle ends up when the chain is posed with a solve.
    fn ankle_of(solve: Solve) -> Vec3 {
        let (hip, thigh, shin) = bones();
        chain(hip, solve, thigh, shin).1
    }

    #[test]
    fn the_end_of_the_limb_lands_where_it_was_sent() {
        for target in [
            Vec3::new(-0.09, 0.10, 0.55),   // in front, foot up: mid-swing
            Vec3::new(-0.09, 0.05, 0.30),   // on the ball
            Vec3::new(-0.09, -0.45, -0.30), // behind and low: cocked
            Vec3::new(0.25, -0.30, 0.20),   // across the body
            Vec3::new(-0.40, -0.50, 0.05),  // out to the side
        ] {
            let solved = ankle_of(leg(target));
            assert!(
                solved.subtract(target).length() < 0.01,
                "asked for {target:?}, the limb reached {solved:?}"
            );
        }
    }

    #[test]
    fn a_limb_at_rest_is_a_straight_limb() {
        let (hip, thigh, shin) = bones();
        let straight_down = Vec3::new(hip.x, hip.y - thigh - shin, hip.z);
        let solve = leg(straight_down);
        assert!(solve.lower.x.abs() < 0.03, "the knee is straight: {:?}", solve.lower);
        assert!(ankle_of(solve).subtract(straight_down).length() < 0.01);
        // The hinge is where it should be: halfway out, not folded.
        let (knee, ankle) = chain(hip, solve, thigh, shin);
        assert!((knee.y - (hip.y - thigh)).abs() < 0.01);
        assert!(ankle.y < knee.y);
    }

    #[test]
    fn the_hinge_survives_a_limb_swinging_up_past_horizontal() {
        // The follow-through. A leg thrown through a ball ends up pointing
        // forwards and level, which is exactly where crossing the reach with a
        // fixed "forwards" pole degenerates. Walk the whole arc and check that
        // the hinge axis — and therefore the limb's roll — never lurches.
        let (hip, thigh, shin) = bones();
        let arc: Vec<Solve> = (0..64)
            .map(|i| {
                let a = 1.0 - (i as f32 / 63.0) * 3.0;
                leg(hip.add(Vec3::new(
                    0.0,
                    -a.cos() * 0.70,
                    -a.sin() * 0.70,
                )))
            })
            .collect();
        arc.windows(2).enumerate().for_each(|(i, w)| {
            let (a, b) = (w[0].upper.to_euler_xyz(), w[1].upper.to_euler_xyz());
            assert!(
                (a.z - b.z).abs() < 0.20,
                "step {i}: the limb rolled {:.0}° in one tick",
                (a.z - b.z).to_degrees()
            );
        });
        // And the roll stays near zero throughout — a sagittal swing is sagittal.
        arc.iter().for_each(|s| {
            let roll = s.upper.to_euler_xyz().z.abs();
            assert!(roll < 0.25, "the limb rolled to {:.0}°", roll.to_degrees());
        });
        let _ = (thigh, shin);
    }

    #[test]
    fn a_knee_bends_forwards_and_an_elbow_bends_back() {
        let (hip, thigh, shin) = bones();
        // A target drawn in toward the hip must fold the limb, not break it.
        let tucked = Vec3::new(hip.x, hip.y - 0.35, hip.z - 0.15);
        let knee_solve = reach(hip, tucked, thigh, shin, KNEE_AXIS);
        assert!(knee_solve.lower.x > 0.05, "it flexed: {:?}", knee_solve.lower);
        let (knee, ankle) = chain(hip, knee_solve, thigh, shin);
        assert!(ankle.z < knee.z + 1.0e-3, "the shin folded forwards through the knee");

        // The same shape with an arm's pole puts the joint on the other side —
        // which is the whole reason the pole is a parameter.
        let (shoulder, upper, fore) = (
            PARTS[L_UPPER_ARM].offset,
            PARTS[L_FOREARM].offset.y.abs(),
            PARTS[L_HAND].offset.y.abs(),
        );
        let out_front = Vec3::new(shoulder.x, shoulder.y - 0.30, shoulder.z + 0.25);
        let elbow_solve = reach(shoulder, out_front, upper, fore, ELBOW_AXIS);
        let (elbow, hand) = chain(shoulder, elbow_solve, upper, fore);
        assert!(hand.subtract(out_front).length() < 0.01, "the hand arrived");
        assert!(elbow.z < hand.z, "the elbow trails behind the hand");
    }

    #[test]
    fn a_target_out_of_reach_straightens_the_limb_and_points_at_it() {
        let (hip, thigh, shin) = bones();
        let miles = Vec3::new(hip.x, hip.y - 0.4, hip.z + 8.0);
        let solve = leg(miles);
        assert!(solve.lower.x.abs() < 0.05, "a reaching limb is a straight limb");
        let ankle = ankle_of(solve);
        let wanted = miles.subtract(hip).normalize().expect("a direction");
        let got = ankle.subtract(hip).normalize().expect("a direction");
        assert!(
            wanted.dot(got) > 0.995,
            "the limb aimed {got:?} at a target lying {wanted:?}"
        );
    }

    #[test]
    fn a_degenerate_target_does_not_produce_a_broken_limb() {
        let (hip, _, _) = bones();
        [hip, Vec3::new(hip.x, hip.y + 5.0, hip.z)]
            .into_iter()
            .for_each(|target| {
                let ankle = ankle_of(leg(target));
                assert!(ankle.x.is_finite() && ankle.y.is_finite() && ankle.z.is_finite());
            });
    }
}
