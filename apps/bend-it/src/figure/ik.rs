//! Two-bone inverse kinematics: put the foot *there*, and let the joints work
//! out how.
//!
//! The kick used to be a table of joint angles that had been tuned until the
//! boot happened to arrive at the ball. That works exactly once. The moment the
//! body starts moving differently — a harder shot leaning further in, a wider
//! plant for a bent shot, a run-up that arrives at a different speed — every one
//! of those angles is wrong, and the boot swings through fresh air.
//!
//! So the leg is solved rather than posed. The hip and knee are whatever they
//! have to be for the ankle to reach a world position the game names, which
//! means the body above can do as it likes and the contact still happens.
//!
//! It is the classic planar solve, but built as a **basis** rather than as Euler
//! angles. Decomposing the aim into successive `Z` and `X` swings looks simpler
//! and is quietly broken: the sideways swing has to be clamped to keep the knee
//! out of the body, and the clamp makes whole directions — a boot lifted above
//! the hip, which is most of a follow-through — unreachable. So instead:
//!
//! 1. the knee is a hinge, so settle its axis first: perpendicular to both the
//!    direction of reach and the way the knee faces. That "pole" is what stops a
//!    solved leg rolling to some arbitrary angle and folding sideways;
//! 2. close the triangle with the law of cosines — the angle at the hip between
//!    the thigh and the straight line to the target, and the angle at the knee;
//! 3. build the thigh's rotation directly from the hinge and the bent direction,
//!    and bend the knee by what is left. Every direction is reachable, because
//!    nothing was ever decomposed.
//!
//! A target further away than the leg is long is not an error — a leg reaching
//! for something out of reach straightens and points at it, which is what a leg
//! does.

use axiom::prelude::Vec3;
use axiom_math::Quat;

/// What the two joints have to be.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LegSolve {
    pub thigh: Quat,
    pub shin: Quat,
}

/// Solve a leg so its ankle lands on `target`.
///
/// Everything is in the figure's **body-local** space: `hip` is the hip joint's
/// offset from the body origin, the leg hangs along `-Y` at rest, and the toes
/// point `+Z`. `thigh` and `shin` are the two bone lengths.
pub fn reach(hip: Vec3, target: Vec3, thigh: f32, shin: f32) -> LegSolve {
    let to_target = target.subtract(hip);
    let span = to_target.length().max(1.0e-4);
    let aim = to_target
        .normalize()
        .unwrap_or(Vec3::new(0.0, -1.0, 0.0));

    // The knee is a hinge, so the first thing to settle is which way it bends.
    // Its axis is perpendicular to both the direction of reach and the way the
    // knee faces — forward — which is the "pole" that stops a solved leg from
    // rotating to some arbitrary roll and folding sideways.
    let hinge = KNEE_FACES
        .cross(aim)
        .normalize()
        .unwrap_or(Vec3::UNIT_X);

    // Close the triangle. `apex` is the angle at the hip between the thigh and
    // the straight line to the target; `interior` is the angle at the knee, so
    // what is left of a straight leg is the joint's own flexion. Both cosines are
    // clamped, which is also what makes an out-of-reach target behave: the
    // triangle degenerates, the flexion falls to zero, and the leg straightens
    // and points — which is what a leg reaching for something does.
    let apex = clamped_acos((thigh * thigh + span * span - shin * shin) / (2.0 * thigh * span));
    let interior =
        clamped_acos((thigh * thigh + shin * shin - span * span) / (2.0 * thigh * shin));
    let flexion = core::f32::consts::PI - interior;

    // The thigh points short of the target by the apex angle, rotated about the
    // hinge so the knee leads forward rather than backward.
    let bent = Quat::from_axis_angle(hinge, -apex)
        .map(|turn| turn.rotate(aim))
        .unwrap_or(aim);
    LegSolve {
        // Built as a basis rather than as Euler angles: the leg hangs along
        // local `-Y`, so `+Y` is the reverse of where it points, and local `+X`
        // must land exactly on the hinge or the knee would bend out of its plane.
        thigh: Quat::look_rotation(hinge.cross(bent), bent.mul_scalar(-1.0))
            .unwrap_or(Quat::IDENTITY),
        shin: Quat::from_euler_xyz(flexion, 0.0, 0.0),
    }
}

/// The direction a knee faces — the pole that fixes which way the joint folds.
const KNEE_FACES: Vec3 = Vec3::new(0.0, 0.0, 1.0);

/// `acos` that tolerates a hair outside its domain.
fn clamped_acos(value: f32) -> f32 {
    value.clamp(-1.0, 1.0).acos()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::figure::model::{L_FOOT, L_SHIN, L_THIGH, PARTS};
    use axiom_math::Transform;

    /// The lengths the figure actually has, so the solve is tested against the
    /// model rather than against numbers invented for the test.
    fn bones() -> (Vec3, f32, f32) {
        (
            PARTS[L_THIGH].offset,
            PARTS[L_SHIN].offset.y.abs(),
            PARTS[L_FOOT].offset.y.abs(),
        )
    }

    /// Where the ankle ends up when the chain is posed with a solve.
    fn ankle_of(solve: LegSolve) -> Vec3 {
        let (hip, thigh_len, shin_len) = bones();
        let thigh = Transform::new(hip, solve.thigh, Vec3::ONE);
        let knee = Transform::combine(
            thigh,
            Transform::new(Vec3::new(0.0, -thigh_len, 0.0), solve.shin, Vec3::ONE),
        );
        Transform::combine(
            knee,
            Transform::from_translation(Vec3::new(0.0, -shin_len, 0.0)),
        )
        .translation
    }

    #[test]
    fn the_ankle_lands_where_it_was_sent() {
        let (hip, thigh, shin) = bones();
        for target in [
            Vec3::new(-0.09, 0.10, 0.55),  // in front, foot up: mid-swing
            Vec3::new(-0.09, 0.05, 0.30),  // on the ball
            Vec3::new(-0.09, -0.45, -0.30), // behind and low: cocked
            Vec3::new(0.25, -0.30, 0.20),  // across the body
            Vec3::new(-0.40, -0.50, 0.05), // out to the side
        ] {
            let solved = ankle_of(reach(hip, target, thigh, shin));
            assert!(
                solved.subtract(target).length() < 0.01,
                "asked for {target:?}, the ankle reached {solved:?}"
            );
        }
    }

    #[test]
    fn a_leg_at_rest_is_a_straight_leg() {
        let (hip, thigh, shin) = bones();
        let straight_down = Vec3::new(hip.x, hip.y - thigh - shin, hip.z);
        let solve = reach(hip, straight_down, thigh, shin);
        // Every joint is essentially identity, and the ankle is where it hangs.
        assert!(solve.shin.x.abs() < 0.03, "the knee is straight: {:?}", solve.shin);
        assert!(ankle_of(solve).subtract(straight_down).length() < 0.01);
    }

    #[test]
    fn the_knee_bends_the_way_a_knee_bends() {
        let (hip, thigh, shin) = bones();
        // A target drawn in toward the hip must fold the leg, not break it: the
        // ankle ends up BEHIND the knee, never in front of it.
        let tucked = Vec3::new(hip.x, hip.y - 0.35, hip.z - 0.15);
        let solve = reach(hip, tucked, thigh, shin);
        assert!(solve.shin.x > 0.05, "the knee flexed: {:?}", solve.shin);
        let knee = Transform::combine(
            Transform::new(hip, solve.thigh, Vec3::ONE),
            Transform::from_translation(Vec3::new(0.0, -thigh, 0.0)),
        )
        .translation;
        assert!(
            ankle_of(solve).z < knee.z + 1.0e-3,
            "the shin folded forwards through the knee"
        );
    }

    #[test]
    fn a_target_out_of_reach_straightens_the_leg_and_points_at_it() {
        let (hip, thigh, shin) = bones();
        let miles = Vec3::new(hip.x, hip.y - 0.4, hip.z + 8.0);
        let solve = reach(hip, miles, thigh, shin);
        assert!(solve.shin.x.abs() < 0.05, "a reaching leg is a straight leg");
        let ankle = ankle_of(solve);
        // It cannot get there, but it is pointing squarely at it.
        let wanted = miles.subtract(hip).normalize().expect("a direction");
        let got = ankle.subtract(hip).normalize().expect("a direction");
        assert!(
            wanted.dot(got) > 0.995,
            "the leg aimed {got:?} at a target lying {wanted:?}"
        );
    }

    #[test]
    fn a_degenerate_target_does_not_produce_a_broken_leg() {
        let (hip, thigh, shin) = bones();
        [hip, Vec3::new(hip.x, hip.y + 5.0, hip.z)]
            .into_iter()
            .for_each(|target| {
                let solve = reach(hip, target, thigh, shin);
                let ankle = ankle_of(solve);
                assert!(ankle.x.is_finite() && ankle.y.is_finite() && ankle.z.is_finite());
            });
    }
}
